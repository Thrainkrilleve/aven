// src/codegen_wasm.rs — WebAssembly (WASM & WAT) Code Emission for Aven
// Milestone Phase 7: Ecosystem Bootstrap & Browser/Edge Interop Bridge
// Emits both human-readable WebAssembly Text (.wat) and pure-std binary WebAssembly (.wasm) bytecode.
#![allow(dead_code)]

use crate::ast::*;
use crate::codegen_c::{CTranspiler, CodegenError};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct WasmModelBundle {
    pub model_name: String,
    pub block_name: String,
    pub wat: String,
    pub wasm_bytes: Vec<u8>,
    pub input_names: Vec<String>,
    pub output_names: Vec<String>,
}

pub struct WasmTranspiler;

/// Math the module takes from its host, imported from module "math" when a model uses it.
/// WebAssembly 1.0 has no transcendental instructions, so these come from the host
/// (`Math.exp`, `Math.log`, ... in JavaScript): results can differ from `avenc run` in the
/// last bits, unlike everything compiled to plain instructions.
const HOST_MATH: [&str; 6] = ["exp", "ln", "sin", "cos", "tan", "pow"];

/// Functions compiled into the module so they match the evaluator exactly: floor and ceil
/// with its 1e-9 snap to a whole number, round half away from zero, and sign.
const HELPERS: [&str; 4] = ["aven_floor", "aven_ceil", "aven_round", "aven_sign"];

/// Which host imports and helpers a block uses, and their function indices. Imports come
/// first in the function index space, then the two entry points, then the helpers.
struct FuncTable {
    imports: Vec<&'static str>,
    helpers: Vec<&'static str>,
}

impl FuncTable {
    fn for_block(block: &BlockDecl, order: &[usize]) -> Result<Self, CodegenError> {
        let mut used = HashSet::new();
        for &i in order {
            collect_calls(&block.equations[i].rhs, &mut used)?;
        }
        Ok(FuncTable {
            imports: HOST_MATH.iter().copied().filter(|f| used.contains(f)).collect(),
            helpers: HELPERS.iter().copied().filter(|f| used.contains(f)).collect(),
        })
    }

    /// Index of the direct-call entry point; `aven_solve` follows it.
    fn first_entry(&self) -> u32 {
        self.imports.len() as u32
    }

    fn index(&self, name: &str) -> u32 {
        if let Some(i) = self.imports.iter().position(|f| *f == name) {
            return i as u32;
        }
        let h = self.helpers.iter().position(|f| *f == name).expect("function collected before emission");
        self.first_entry() + 2 + h as u32
    }
}

/// The function an Aven builtin compiles to, or None for a single WebAssembly instruction.
fn call_target(name: &str) -> Result<Option<&'static str>, CodegenError> {
    Ok(match name {
        "sqrt" | "abs" => None,
        "floor" => Some("aven_floor"),
        "ceil" => Some("aven_ceil"),
        "round" => Some("aven_round"),
        "sign" => Some("aven_sign"),
        "exp" => Some("exp"),
        "ln" | "log" => Some("ln"),
        "sin" => Some("sin"),
        "cos" => Some("cos"),
        "tan" => Some("tan"),
        _ => return Err(CodegenError(format!("Unsupported WASM builtin function '{}'", name))),
    })
}

fn collect_calls(expr: &Expr, used: &mut HashSet<&'static str>) -> Result<(), CodegenError> {
    match expr {
        Expr::Num(_) | Expr::Var(_) => {}
        Expr::UnaryMinus(inner) => collect_calls(inner, used)?,
        Expr::Bin(BinaryOp::Pow, l, r) => match exact_pow(l, r) {
            Some(lowered) => collect_calls(&lowered, used)?,
            None => {
                used.insert("pow");
                collect_calls(l, used)?;
                collect_calls(r, used)?;
            }
        },
        Expr::Bin(_, l, r) => {
            collect_calls(l, used)?;
            collect_calls(r, used)?;
        }
        Expr::Call(name, arg) => {
            if let Some(f) = call_target(name)? {
                used.insert(f);
            }
            collect_calls(arg, used)?;
        }
    }
    Ok(())
}

impl WasmTranspiler {
    /// Transpiles a pinned block to WebAssembly Text (.wat) and binary (.wasm).
    pub fn transpile(module: &AvenModule) -> Result<WasmModelBundle, CodegenError> {
        let block = if let Some(fb) = module.blocks.iter().find(|b| b.kind == BlockKind::Forward) {
            fb
        } else if let Some(rb) = module.blocks.first() {
            rb
        } else {
            return Err(CodegenError("Module has no forward or relation block to compile to WASM".to_string()));
        };

        let order = CTranspiler::topo_sort(block)?;

        // Collect outputs (LHS of ordered equations)
        let output_names: Vec<String> = order.iter().map(|&i| block.equations[i].lhs.clone()).collect();
        let derived_set: HashSet<String> = output_names.iter().cloned().collect();

        // Collect inputs (all variables appearing on RHS that are not outputs)
        let mut input_set = HashSet::new();
        for &i in &order {
            for v in block.equations[i].rhs.vars() {
                if !derived_set.contains(&v) {
                    input_set.insert(v);
                }
            }
        }
        let mut input_names: Vec<String> = input_set.into_iter().collect();
        input_names.sort();

        let funcs = FuncTable::for_block(block, &order)?;
        let wat = Self::generate_wat(module, block, &order, &input_names, &output_names, &funcs)?;
        let wasm_bytes = Self::generate_wasm_binary(module, block, &order, &input_names, &output_names, &funcs)?;

        Ok(WasmModelBundle {
            model_name: module.name.clone(),
            block_name: block.name.clone(),
            wat,
            wasm_bytes,
            input_names,
            output_names,
        })
    }

    /// Generates standard WebAssembly Text format (.wat).
    fn generate_wat(
        module: &AvenModule,
        block: &BlockDecl,
        order: &[usize],
        input_names: &[String],
        output_names: &[String],
        funcs: &FuncTable,
    ) -> Result<String, CodegenError> {
        let mut wat = String::new();
        wat.push_str(";; Generated by Aven Compiler (avenc) — Phase 7 WASM Bridge\n");
        wat.push_str(&format!(";; Model: {} | Block: {}\n", module.name, block.name));
        wat.push_str("(module\n");

        for f in &funcs.imports {
            let params = if *f == "pow" { "(param f64 f64)" } else { "(param f64)" };
            wat.push_str(&format!("  (import \"math\" \"{}\" (func ${} {} (result f64)))\n", f, f, params));
        }
        if !funcs.imports.is_empty() {
            wat.push('\n');
        }

        // Memory: 1 page (64 KB) exported for browser/edge buffer sharing
        wat.push_str("  (memory (export \"memory\") 1)\n\n");

        // 1. Direct function taking inputs as params and returning the first output
        wat.push_str(&format!("  ;; Direct calculation function for '{}'\n", block.name));
        wat.push_str(&format!("  (func ${} (export \"{}\")\n", block.name, block.name));
        for in_var in input_names {
            wat.push_str(&format!("    (param ${} f64)\n", in_var));
        }
        if !output_names.is_empty() {
            wat.push_str("    (result f64)\n");
        }

        // Declare locals for intermediate derived outputs
        for out_var in output_names {
            if !input_names.contains(out_var) {
                wat.push_str(&format!("    (local ${} f64)\n", out_var));
            }
        }

        // Emit equations in topological order
        for &idx in order {
            let eq = &block.equations[idx];
            wat.push_str(&format!("    ;; {} = {}\n", eq.lhs, eq.original_text));
            Self::emit_wat_expr(&eq.rhs, &mut wat, 2)?;
            wat.push_str(&format!("    local.set ${}\n", eq.lhs));
        }

        // Return last output as result if present
        if let Some(last_out) = output_names.last() {
            wat.push_str(&format!("    local.get ${}\n", last_out));
        }
        wat.push_str("  )\n\n");

        // 2. Memory-mapped I/O solver function for zero-copy buffer interop:
        // aven_solve(in_ptr: i32, out_ptr: i32) -> i32 (0 = success)
        wat.push_str("  ;; Buffer-oriented solver for zero-copy host interop\n");
        wat.push_str("  (func (export \"aven_solve\") (param $in_ptr i32) (param $out_ptr i32) (result i32)\n");
        for in_var in input_names {
            wat.push_str(&format!("    (local ${} f64)\n", in_var));
        }
        for out_var in output_names {
            if !input_names.contains(out_var) {
                wat.push_str(&format!("    (local ${} f64)\n", out_var));
            }
        }

        // Load inputs from in_ptr
        for (i, in_var) in input_names.iter().enumerate() {
            let offset = i * 8;
            wat.push_str("    local.get $in_ptr\n");
            wat.push_str(&format!("    f64.load offset={}\n", offset));
            wat.push_str(&format!("    local.set ${}\n", in_var));
        }

        // Compute equations
        for &idx in order {
            let eq = &block.equations[idx];
            Self::emit_wat_expr(&eq.rhs, &mut wat, 2)?;
            wat.push_str(&format!("    local.set ${}\n", eq.lhs));
        }

        // Store outputs to out_ptr
        for (j, out_var) in output_names.iter().enumerate() {
            let offset = j * 8;
            wat.push_str("    local.get $out_ptr\n");
            wat.push_str(&format!("    local.get ${}\n", out_var));
            wat.push_str(&format!("    f64.store offset={}\n", offset));
        }

        // Return 0 (success)
        wat.push_str("    i32.const 0\n");
        wat.push_str("  )\n");

        for h in &funcs.helpers {
            wat.push_str(&format!("\n  (func ${} (param $v f64) (result f64) (local $t f64)\n", h));
            wat.push_str(match *h {
                "aven_floor" | "aven_ceil" => {
                    if *h == "aven_floor" {
                        "    ;; floor, snapping values within 1e-9 of a whole number to it\n    local.get $v\n    f64.nearest\n    local.set $t\n    local.get $t\n    local.get $v\n    f64.floor\n"
                    } else {
                        "    ;; ceil, snapping values within 1e-9 of a whole number to it\n    local.get $v\n    f64.nearest\n    local.set $t\n    local.get $t\n    local.get $v\n    f64.ceil\n"
                    }
                }
                "aven_round" => "    ;; round half away from zero\n    local.get $v\n    f64.trunc\n    local.set $t\n    local.get $t\n    f64.const 1\n    local.get $v\n    f64.copysign\n    f64.add\n    local.get $t\n",
                _ => "    ;; sign: -1, 0 or +1 (0 for NaN)\n    local.get $v\n    f64.const 0\n    f64.gt\n    local.get $v\n    f64.const 0\n    f64.lt\n    i32.sub\n    f64.convert_i32_s\n",
            });
            match *h {
                "aven_floor" | "aven_ceil" => wat.push_str("    local.get $v\n    local.get $t\n    f64.sub\n    f64.abs\n    f64.const 1e-9\n    f64.lt\n    select\n"),
                "aven_round" => wat.push_str("    local.get $v\n    local.get $t\n    f64.sub\n    f64.abs\n    f64.const 0.5\n    f64.ge\n    select\n"),
                _ => {}
            }
            wat.push_str("  )\n");
        }

        wat.push_str(")\n");
        Ok(wat)
    }

    /// Recursively translates an Aven AST expression into stack-based WAT instructions.
    fn emit_wat_expr(expr: &Expr, out: &mut String, indent: usize) -> Result<(), CodegenError> {
        let pad = "  ".repeat(indent);
        match expr {
            Expr::Num(n) => {
                out.push_str(&format!("{}f64.const {}\n", pad, n));
            }
            Expr::Var(v) => {
                match v.to_lowercase().as_str() {
                    "pi" => out.push_str(&format!("{}f64.const 3.141592653589793\n", pad)),
                    "e" => out.push_str(&format!("{}f64.const 2.718281828459045\n", pad)),
                    "tau" => out.push_str(&format!("{}f64.const 6.283185307179586\n", pad)),
                    _ => out.push_str(&format!("{}local.get ${}\n", pad, v)),
                }
            }
            Expr::UnaryMinus(inner) => {
                Self::emit_wat_expr(inner, out, indent)?;
                out.push_str(&format!("{}f64.neg\n", pad));
            }
            Expr::Bin(BinaryOp::Pow, left, right) => match exact_pow(left, right) {
                Some(lowered) => Self::emit_wat_expr(&lowered, out, indent)?,
                None => {
                    Self::emit_wat_expr(left, out, indent)?;
                    Self::emit_wat_expr(right, out, indent)?;
                    out.push_str(&format!("{}call $pow\n", pad));
                }
            },
            Expr::Bin(op, left, right) => {
                Self::emit_wat_expr(left, out, indent)?;
                Self::emit_wat_expr(right, out, indent)?;
                match op {
                    BinaryOp::Add => out.push_str(&format!("{}f64.add\n", pad)),
                    BinaryOp::Sub => out.push_str(&format!("{}f64.sub\n", pad)),
                    BinaryOp::Mul => out.push_str(&format!("{}f64.mul\n", pad)),
                    BinaryOp::Div => out.push_str(&format!("{}f64.div\n", pad)),
                    BinaryOp::Pow => unreachable!("lowered above"),
                }
            }
            Expr::Call(name, arg) => {
                Self::emit_wat_expr(arg, out, indent)?;
                match call_target(name)? {
                    Some(f) => out.push_str(&format!("{}call ${}\n", pad, f)),
                    None if name == "sqrt" => out.push_str(&format!("{}f64.sqrt\n", pad)),
                    None => out.push_str(&format!("{}f64.abs\n", pad)),
                }
            }
        }
        Ok(())
    }

    /// Generates pure WebAssembly 1.0 binary bytecode without external crates.
    fn generate_wasm_binary(
        _module: &AvenModule,
        block: &BlockDecl,
        order: &[usize],
        input_names: &[String],
        output_names: &[String],
        funcs: &FuncTable,
    ) -> Result<Vec<u8>, CodegenError> {
        let mut wasm = Vec::new();
        // Magic & Version
        wasm.extend_from_slice(&[0x00, 0x61, 0x73, 0x6D]); // \0asm
        wasm.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // version 1

        // Map variables to local indices:
        // params: 0..input_names.len()
        // locals: input_names.len()..(input_names.len() + new_locals.len())
        let mut var_to_idx = HashMap::new();
        for (i, name) in input_names.iter().enumerate() {
            var_to_idx.insert(name.clone(), i as u32);
        }
        let mut next_local = input_names.len() as u32;
        let mut num_locals = 0;
        for name in output_names {
            if !var_to_idx.contains_key(name) {
                var_to_idx.insert(name.clone(), next_local);
                next_local += 1;
                num_locals += 1;
            }
        }

        // Section 1: Type Section
        // Type 0: (param f64 ... [N_in]) -> (result f64)
        // Type 1: (param i32, i32) -> (result i32) [for aven_solve]
        // Type 2: (f64) -> f64 [unary math and helpers]; Type 3: (f64, f64) -> f64 [pow]
        let mut type_sec = Vec::new();
        let num_types = 4_u32;
        type_sec.extend(encode_u32_leb128(num_types));

        // Type 0 (direct function)
        type_sec.push(0x60); // func
        type_sec.extend(encode_u32_leb128(input_names.len() as u32));
        for _ in 0..input_names.len() {
            type_sec.push(0x7C); // f64
        }
        if !output_names.is_empty() {
            type_sec.push(0x01); // 1 result
            type_sec.push(0x7C); // f64
        } else {
            type_sec.push(0x00);
        }

        // Type 1 (aven_solve: (i32, i32) -> i32)
        type_sec.push(0x60);
        type_sec.extend(encode_u32_leb128(2));
        type_sec.push(0x7F); // i32 in_ptr
        type_sec.push(0x7F); // i32 out_ptr
        type_sec.push(0x01); // 1 result
        type_sec.push(0x7F); // i32 return code

        type_sec.extend_from_slice(&[0x60, 0x01, 0x7C, 0x01, 0x7C]); // Type 2
        type_sec.extend_from_slice(&[0x60, 0x02, 0x7C, 0x7C, 0x01, 0x7C]); // Type 3

        wasm.extend(encode_section(1, &type_sec));

        // Section 2: Import Section (host math, only when used)
        if !funcs.imports.is_empty() {
            let mut imp_sec = Vec::new();
            imp_sec.extend(encode_u32_leb128(funcs.imports.len() as u32));
            for f in &funcs.imports {
                imp_sec.extend(encode_name("math"));
                imp_sec.extend(encode_name(f));
                imp_sec.push(0x00); // func import
                imp_sec.extend(encode_u32_leb128(if *f == "pow" { 3 } else { 2 }));
            }
            wasm.extend(encode_section(2, &imp_sec));
        }

        // Section 3: Function Section
        // Direct -> Type 0, aven_solve -> Type 1, helpers -> Type 2
        let mut func_sec = Vec::new();
        func_sec.extend(encode_u32_leb128(2 + funcs.helpers.len() as u32));
        func_sec.extend(encode_u32_leb128(0));
        func_sec.extend(encode_u32_leb128(1));
        for _ in &funcs.helpers {
            func_sec.extend(encode_u32_leb128(2));
        }
        wasm.extend(encode_section(3, &func_sec));

        // Section 5: Memory Section (1 page, 64KB)
        let mut mem_sec = Vec::new();
        mem_sec.extend(encode_u32_leb128(1)); // 1 memory
        mem_sec.push(0x00); // flags: min only
        mem_sec.extend(encode_u32_leb128(1)); // 1 page
        wasm.extend(encode_section(5, &mem_sec));

        // Section 7: Export Section
        // 1. memory -> kind 0x02, index 0
        // 2. <block_name> -> kind 0x00, func 0
        // 3. aven_solve -> kind 0x00, func 1
        let mut exp_sec = Vec::new();
        exp_sec.extend(encode_u32_leb128(3));

        // Export "memory"
        exp_sec.extend(encode_name("memory"));
        exp_sec.push(0x02); // memory export
        exp_sec.extend(encode_u32_leb128(0));

        // Export block name
        exp_sec.extend(encode_name(&block.name));
        exp_sec.push(0x00); // func export
        exp_sec.extend(encode_u32_leb128(funcs.first_entry()));

        // Export "aven_solve"
        exp_sec.extend(encode_name("aven_solve"));
        exp_sec.push(0x00); // func export
        exp_sec.extend(encode_u32_leb128(funcs.first_entry() + 1));

        wasm.extend(encode_section(7, &exp_sec));

        // Section 10: Code Section
        let mut code_sec = Vec::new();
        code_sec.extend(encode_u32_leb128(2 + funcs.helpers.len() as u32));

        // Function 0 Body (Direct Calc)
        let mut body0 = Vec::new();
        // Locals: num_locals f64 variables
        if num_locals > 0 {
            body0.extend(encode_u32_leb128(1)); // 1 local entry
            body0.extend(encode_u32_leb128(num_locals));
            body0.push(0x7C); // f64
        } else {
            body0.push(0x00); // 0 local entries
        }

        // Instructions for equations
        for &idx in order {
            let eq = &block.equations[idx];
            Self::emit_wasm_expr(&eq.rhs, &var_to_idx, funcs, &mut body0)?;
            let target_idx = *var_to_idx.get(&eq.lhs).unwrap();
            body0.push(0x21); // local.set
            body0.extend(encode_u32_leb128(target_idx));
        }

        // Return last output if present
        if let Some(last_out) = output_names.last() {
            let out_idx = *var_to_idx.get(last_out).unwrap();
            body0.push(0x20); // local.get
            body0.extend(encode_u32_leb128(out_idx));
        }
        body0.push(0x0B); // end

        code_sec.extend(encode_u32_leb128(body0.len() as u32));
        code_sec.extend(body0);

        // Function 1 Body (aven_solve: loads from in_ptr, executes, stores to out_ptr)
        let mut body1 = Vec::new();
        let total_locals = input_names.len() as u32 + num_locals;
        if total_locals > 0 {
            body1.extend(encode_u32_leb128(1));
            body1.extend(encode_u32_leb128(total_locals));
            body1.push(0x7C); // f64
        } else {
            body1.push(0x00);
        }

        // Variable mapping inside func 1:
        // params: 0 = in_ptr (i32), 1 = out_ptr (i32)
        // locals: 2..(2 + total_locals)
        let mut func1_var_map = HashMap::new();
        for (i, name) in input_names.iter().enumerate() {
            func1_var_map.insert(name.clone(), 2 + i as u32);
        }
        for (j, name) in output_names.iter().enumerate() {
            if !func1_var_map.contains_key(name) {
                func1_var_map.insert(name.clone(), 2 + input_names.len() as u32 + j as u32);
            }
        }

        // Load inputs: local.get 0 (in_ptr), f64.load offset, local.set (2+i)
        for (i, name) in input_names.iter().enumerate() {
            let loc_idx = *func1_var_map.get(name).unwrap();
            body1.push(0x20); // local.get
            body1.extend(encode_u32_leb128(0)); // in_ptr
            body1.push(0x2B); // f64.load
            body1.extend(encode_u32_leb128(3)); // align (2^3 = 8 bytes)
            body1.extend(encode_u32_leb128((i * 8) as u32)); // offset
            body1.push(0x21); // local.set
            body1.extend(encode_u32_leb128(loc_idx));
        }

        // Compute equations
        for &idx in order {
            let eq = &block.equations[idx];
            Self::emit_wasm_expr(&eq.rhs, &func1_var_map, funcs, &mut body1)?;
            let target_idx = *func1_var_map.get(&eq.lhs).unwrap();
            body1.push(0x21); // local.set
            body1.extend(encode_u32_leb128(target_idx));
        }

        // Store outputs: local.get 1 (out_ptr), local.get (loc_idx), f64.store offset
        for (j, name) in output_names.iter().enumerate() {
            let loc_idx = *func1_var_map.get(name).unwrap();
            body1.push(0x20); // local.get
            body1.extend(encode_u32_leb128(1)); // out_ptr
            body1.push(0x20); // local.get
            body1.extend(encode_u32_leb128(loc_idx));
            body1.push(0x39); // f64.store
            body1.extend(encode_u32_leb128(3)); // align 8
            body1.extend(encode_u32_leb128((j * 8) as u32)); // offset
        }

        // Return i32 0 (success)
        body1.push(0x41); // i32.const
        body1.extend(encode_i32_leb128(0));
        body1.push(0x0B); // end

        code_sec.extend(encode_u32_leb128(body1.len() as u32));
        code_sec.extend(body1);

        for h in &funcs.helpers {
            let body = helper_body(h);
            code_sec.extend(encode_u32_leb128(body.len() as u32));
            code_sec.extend(body);
        }

        wasm.extend(encode_section(10, &code_sec));

        Ok(wasm)
    }

    /// Emits bytecode instructions for an expression.
    fn emit_wasm_expr(
        expr: &Expr,
        var_map: &HashMap<String, u32>,
        funcs: &FuncTable,
        out: &mut Vec<u8>,
    ) -> Result<(), CodegenError> {
        match expr {
            Expr::Num(n) => {
                out.push(0x44); // f64.const
                out.extend_from_slice(&n.to_le_bytes());
            }
            Expr::Var(v) => {
                match v.to_lowercase().as_str() {
                    "pi" => {
                        out.push(0x44);
                        out.extend_from_slice(&std::f64::consts::PI.to_le_bytes());
                    }
                    "e" => {
                        out.push(0x44);
                        out.extend_from_slice(&std::f64::consts::E.to_le_bytes());
                    }
                    "tau" => {
                        out.push(0x44);
                        out.extend_from_slice(&(std::f64::consts::PI * 2.0).to_le_bytes());
                    }
                    _ => {
                        if let Some(&idx) = var_map.get(v) {
                            out.push(0x20); // local.get
                            out.extend(encode_u32_leb128(idx));
                        } else {
                            return Err(CodegenError(format!("Unresolved variable '{}' in WASM compilation", v)));
                        }
                    }
                }
            }
            Expr::UnaryMinus(inner) => {
                Self::emit_wasm_expr(inner, var_map, funcs, out)?;
                out.push(0x9A); // f64.neg
            }
            Expr::Bin(BinaryOp::Pow, left, right) => match exact_pow(left, right) {
                Some(lowered) => Self::emit_wasm_expr(&lowered, var_map, funcs, out)?,
                None => {
                    Self::emit_wasm_expr(left, var_map, funcs, out)?;
                    Self::emit_wasm_expr(right, var_map, funcs, out)?;
                    out.push(0x10); // call
                    out.extend(encode_u32_leb128(funcs.index("pow")));
                }
            },
            Expr::Bin(op, left, right) => {
                Self::emit_wasm_expr(left, var_map, funcs, out)?;
                Self::emit_wasm_expr(right, var_map, funcs, out)?;
                match op {
                    BinaryOp::Add => out.push(0xA0), // f64.add
                    BinaryOp::Sub => out.push(0xA1), // f64.sub
                    BinaryOp::Mul => out.push(0xA2), // f64.mul
                    BinaryOp::Div => out.push(0xA3), // f64.div
                    BinaryOp::Pow => unreachable!("lowered above"),
                }
            }
            Expr::Call(name, arg) => {
                Self::emit_wasm_expr(arg, var_map, funcs, out)?;
                match call_target(name)? {
                    Some(f) => {
                        out.push(0x10); // call
                        out.extend(encode_u32_leb128(funcs.index(f)));
                    }
                    None if name == "sqrt" => out.push(0x9F), // f64.sqrt
                    None => out.push(0x99),                  // f64.abs
                }
            }
        }
        Ok(())
    }
}

/// WebAssembly 1.0 has no pow instruction. Constant integer exponents become repeated
/// multiplication (reciprocal for negative exponents) and `^ 0.5` becomes `f64.sqrt`, all
/// exact; any other exponent returns None and calls the host's `math.pow`.
const MAX_WASM_POW_EXPONENT: f64 = 64.0;

fn exact_pow(base: &Expr, exp: &Expr) -> Option<Expr> {
    let n = match exp {
        Expr::Num(n) => *n,
        Expr::UnaryMinus(inner) => match inner.as_ref() {
            Expr::Num(n) => -*n,
            _ => return None,
        },
        _ => return None,
    };
    if n == 0.5 {
        return Some(Expr::Call("sqrt".to_string(), Box::new(base.clone())));
    }
    if n.fract() != 0.0 || n.abs() > MAX_WASM_POW_EXPONENT {
        return None;
    }
    let k = n.abs() as usize;
    let mut product = if k == 0 { Expr::Num(1.0) } else { base.clone() };
    for _ in 1..k {
        product = Expr::Bin(BinaryOp::Mul, Box::new(product), Box::new(base.clone()));
    }
    if n < 0.0 {
        product = Expr::Bin(BinaryOp::Div, Box::new(Expr::Num(1.0)), Box::new(product));
    }
    Some(product)
}

/// Body of a helper function: one f64 parameter (local 0) and one f64 scratch local (1).
fn helper_body(name: &str) -> Vec<u8> {
    const GET_V: [u8; 2] = [0x20, 0x00];
    const GET_T: [u8; 2] = [0x20, 0x01];
    const SET_T: [u8; 2] = [0x21, 0x01];
    let f64_const = |x: f64| {
        let mut b = vec![0x44];
        b.extend_from_slice(&x.to_le_bytes());
        b
    };
    let mut b = vec![0x01, 0x01, 0x7C]; // 1 local entry: 1 x f64
    match name {
        "aven_floor" | "aven_ceil" => {
            // t = nearest(v); select(t, floor/ceil(v), |v - t| < 1e-9)
            b.extend(GET_V);
            b.push(0x9E); // f64.nearest
            b.extend(SET_T);
            b.extend(GET_T);
            b.extend(GET_V);
            b.push(if name == "aven_floor" { 0x9C } else { 0x9B });
            b.extend(GET_V);
            b.extend(GET_T);
            b.push(0xA1); // f64.sub
            b.push(0x99); // f64.abs
            b.extend(f64_const(1e-9));
            b.push(0x63); // f64.lt
            b.push(0x1B); // select
        }
        "aven_round" => {
            // t = trunc(v); select(t + copysign(1, v), t, |v - t| >= 0.5)
            b.extend(GET_V);
            b.push(0x9D); // f64.trunc
            b.extend(SET_T);
            b.extend(GET_T);
            b.extend(f64_const(1.0));
            b.extend(GET_V);
            b.push(0xA6); // f64.copysign
            b.push(0xA0); // f64.add
            b.extend(GET_T);
            b.extend(GET_V);
            b.extend(GET_T);
            b.push(0xA1); // f64.sub
            b.push(0x99); // f64.abs
            b.extend(f64_const(0.5));
            b.push(0x66); // f64.ge
            b.push(0x1B); // select
        }
        _ => {
            // (v > 0) - (v < 0), as f64
            b.extend(GET_V);
            b.extend(f64_const(0.0));
            b.push(0x64); // f64.gt
            b.extend(GET_V);
            b.extend(f64_const(0.0));
            b.push(0x63); // f64.lt
            b.push(0x6B); // i32.sub
            b.push(0xB7); // f64.convert_i32_s
        }
    }
    b.push(0x0B); // end
    b
}

// ---------------------------------------------------------------------------
// Pure standard library LEB128 & Section Encoders
// ---------------------------------------------------------------------------

fn encode_u32_leb128(mut value: u32) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
    out
}

fn encode_i32_leb128(mut value: i32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut more = true;
    while more {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        let sign_bit = (byte & 0x40) != 0;
        if (value == 0 && !sign_bit) || (value == -1 && sign_bit) {
            more = false;
        } else {
            byte |= 0x80;
        }
        out.push(byte);
    }
    out
}

fn encode_name(name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(encode_u32_leb128(name.len() as u32));
    out.extend_from_slice(name.as_bytes());
    out
}

fn encode_section(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(id);
    out.extend(encode_u32_leb128(payload.len() as u32));
    out.extend_from_slice(payload);
    out
}
