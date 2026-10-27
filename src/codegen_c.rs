// src/codegen_c.rs — Zero-overhead C99 code emission for direction-pinned relations
use crate::ast::*;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, Default)]
pub struct CodegenOptions {
    pub traceable: bool,
    pub checked_c: bool,
    pub aeric_shim: bool,
    pub no_main: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AericShimBundle {
    pub model_stem: String,
    pub block_name: String,
    pub model_hash: String,
    pub n_in: usize,
    pub n_out: usize,
    pub input_names: Vec<String>,
    pub output_names: Vec<String>,
    pub header_h: String,
    pub model_c: String,
    pub shim_c: String,
    pub single_unit_c: String,
}

#[derive(Debug, Clone)]
pub struct CodegenError(pub String);

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Codegen error: {}", self.0)
    }
}

impl std::error::Error for CodegenError {}

pub struct CTranspiler;

impl CTranspiler {
    /// Topologically sort equations in a pinned block.
    /// Returns the firing order as equation indices, or an error if unresolvable.
    pub fn topo_sort(block: &BlockDecl) -> Result<Vec<usize>, CodegenError> {
        let equal_indices: Vec<usize> = block
            .equations
            .iter()
            .enumerate()
            .filter(|(_, e)| e.op == ConstraintOp::Equal)
            .map(|(idx, _)| idx)
            .collect();

        let derived: HashSet<String> = equal_indices
            .iter()
            .map(|&i| block.equations[i].lhs.clone())
            .collect();
        let mut known: HashSet<String> = HashSet::new();

        for &i in &equal_indices {
            let eq = &block.equations[i];
            for v in eq.rhs.vars() {
                if !derived.contains(&v) {
                    known.insert(v); // Pure external input
                }
            }
        }

        let mut order = Vec::new();
        let mut placed = HashSet::new();

        while order.len() < equal_indices.len() {
            let mut progressed = false;
            for &idx in &equal_indices {
                if placed.contains(&idx) {
                    continue;
                }
                let eq = &block.equations[idx];
                let needed = eq.rhs.vars();
                if needed.iter().all(|v| known.contains(v)) {
                    order.push(idx);
                    placed.insert(idx);
                    known.insert(eq.lhs.clone());
                    progressed = true;
                }
            }

            if !progressed {
                let stuck: Vec<String> = equal_indices
                    .iter()
                    .filter(|idx| !placed.contains(idx))
                    .map(|&idx| block.equations[idx].lhs.clone())
                    .collect();

                return Err(CodegenError(format!(
                    "forward block '{}' cannot fire forward: circular/unresolvable [{}]. \
                     Declare it as a 'relation' to allow omnidirectional solving, or reorder.",
                    block.name,
                    stuck.join(", ")
                )));
            }
        }

        Ok(order)
    }

    /// Emits a complete, standalone C program for the pinned block with given bindings.
    pub fn transpile_block(
        block: &BlockDecl,
        bindings: &HashMap<String, f64>,
    ) -> Result<String, CodegenError> {
        Self::transpile_block_with_options(block, bindings, &CodegenOptions::default())
    }

    /// Emits C program with compiler options (--traceable for eBPF, --checked-c for division guards)
    pub fn transpile_block_with_options(
        block: &BlockDecl,
        bindings: &HashMap<String, f64>,
        options: &CodegenOptions,
    ) -> Result<String, CodegenError> {
        let order = Self::topo_sort(block)?;

        let equal_indices: Vec<usize> = order.clone();
        let derived: HashSet<String> = equal_indices
            .iter()
            .map(|&i| block.equations[i].lhs.clone())
            .collect();
        let mut inputs_set = BTreeSet::new();
        for &idx in &equal_indices {
            for v in block.equations[idx].rhs.vars() {
                if !derived.contains(&v) {
                    inputs_set.insert(v);
                }
            }
        }
        let inputs: Vec<String> = inputs_set.into_iter().collect();

        let is_lib = options.no_main || options.aeric_shim;
        let is_traceable = options.traceable || options.aeric_shim;

        // Check for missing inputs
        if !is_lib {
            let missing: Vec<&String> = inputs
                .iter()
                .filter(|v| !bindings.contains_key(*v))
                .collect();
            if !missing.is_empty() {
                let missing_names: Vec<String> = missing.into_iter().cloned().collect();
                return Err(CodegenError(format!(
                    "missing bindings for inputs: {}",
                    missing_names.join(", ")
                )));
            }
        }

        let mut needs_math = options.aeric_shim;
        let mut lines = Vec::new();

        lines.push("/* Generated by avenc -- direction-pinned, solver-free */".to_string());
        lines.push("#include <stdio.h>".to_string());
        if options.checked_c {
            lines.push("#include <stdlib.h>".to_string());
        }
        lines.push("/* math_include_marker */".to_string());
        lines.push("".to_string());

        if is_traceable {
            lines.push("/* eBPF uprobe export: preserve symbol in .symtab under -O2 and prevent IPA cloning */".to_string());
            lines.push("/* clang defines __GNUC__ but has no 'noipa'; use noinline+used there, GCC's noipa below */".to_string());
            lines.push("#if defined(__clang__)".to_string());
            lines.push("#define AVEN_EXPORT __attribute__((noinline, used))".to_string());
            lines.push("#elif defined(__GNUC__)".to_string());
            lines.push("#define AVEN_EXPORT __attribute__((noipa, noinline, used))".to_string());
            lines.push("#else".to_string());
            lines.push("#define AVEN_EXPORT".to_string());
            lines.push("#endif".to_string());
            lines.push("".to_string());
        }

        // Function signature
        let mut param_parts: Vec<String> = inputs.iter().map(|p| format!("double {}", p)).collect();
        for &idx in &equal_indices {
            let eq = &block.equations[idx];
            param_parts.push(format!("double* out_{}", eq.lhs));
        }

        let fn_decl = if is_traceable {
            format!(
                "AVEN_EXPORT void aven_{}({}) {{",
                block.name,
                param_parts.join(", ")
            )
        } else {
            format!(
                "static void aven_{}({}) {{",
                block.name,
                param_parts.join(", ")
            )
        };
        lines.push(fn_decl);

        // Optional domain/zero-division guards
        if options.checked_c {
            needs_math = true;
            let mut denoms = HashSet::new();
            for &idx in &equal_indices {
                collect_denominators(&block.equations[idx].rhs, &mut denoms);
            }
            let mut checked_vars: Vec<String> =
                denoms.into_iter().filter(|v| inputs.contains(v)).collect();
            checked_vars.sort();

            for var in checked_vars {
                lines.push("    /* Domain check: prevent IEEE-754 division-by-zero, NaN bypass, and uninitialized reads */".to_string());
                if var.contains("utilization") || var.contains("efficiency") {
                    lines.push(format!(
                        "    if (isnan({}) || !({} > 0.0 && {} <= 1.0)) {{",
                        var, var, var
                    ));
                    lines.push(format!(
                        "        fprintf(stderr, \"[avenc runtime error] parameter '{}' must be in (0.0, 1.0] and not NaN (got %g)\\n\", {});",
                        var, var
                    ));
                } else {
                    lines.push(format!("    if (isnan({}) || !({} > 0.0)) {{", var, var));
                    lines.push(format!(
                        "        fprintf(stderr, \"[avenc runtime error] parameter '{}' must be strictly positive (> 0.0) and not NaN (got %g)\\n\", {});",
                        var, var
                    ));
                }
                for &idx in &equal_indices {
                    let eq = &block.equations[idx];
                    lines.push(format!(
                        "        if (out_{}) *out_{} = 0.0;",
                        eq.lhs, eq.lhs
                    ));
                }
                lines.push("        exit(EXIT_FAILURE);".to_string());
                lines.push("    }".to_string());
            }
        }

        // Emit invariant constraints as documentation guards
        for eq in &block.equations {
            if eq.op != ConstraintOp::Equal {
                lines.push(format!(
                    "    /* Invariant constraint: {} */",
                    eq.original_text
                ));
            }
        }

        // Function body in topological order
        for &idx in &order {
            let eq = &block.equations[idx];
            let c_expr = expr_to_c(&eq.rhs, &mut needs_math);
            lines.push(format!("    const double {} = {};", eq.lhs, c_expr));
        }

        // Store outputs
        for &idx in &equal_indices {
            let eq = &block.equations[idx];
            lines.push(format!("    *out_{} = {};", eq.lhs, eq.lhs));
        }
        lines.push("}".to_string());
        lines.push("".to_string());

        if !is_lib {
            // Standalone main()
            lines.push("int main(void) {".to_string());
            for &idx in &equal_indices {
                let eq = &block.equations[idx];
                lines.push(format!("    double {} = 0.0;", eq.lhs));
            }

            let mut call_args: Vec<String> = inputs
                .iter()
                .map(|v| {
                    let val = bindings.get(v).copied().unwrap_or(0.0);
                    if val.fract() == 0.0 {
                        format!("{:.1}", val)
                    } else {
                        format!("{}", val)
                    }
                })
                .collect();
            for &idx in &equal_indices {
                let eq = &block.equations[idx];
                call_args.push(format!("&{}", eq.lhs));
            }

            lines.push(format!(
                "    aven_{}({});",
                block.name,
                call_args.join(", ")
            ));

            for &idx in &equal_indices {
                let eq = &block.equations[idx];
                lines.push(format!("    printf(\"{} = %.6g\\n\", {});", eq.lhs, eq.lhs));
            }
            lines.push("    return 0;".to_string());
            lines.push("}".to_string());
        }

        // Replace marker if math functions used
        let output = lines.join("\n");
        let final_code = if needs_math {
            output.replace("/* math_include_marker */", &math_prelude(&output))
        } else {
            output.replace("/* math_include_marker */\n", "")
        };

        Ok(final_code)
    }

    /// Emits a complete AERIC deployment bundle for the pinned block:
    /// - `model_c`: linkable model kernel without main()
    /// - `header_h`: model declarations and ABI contract
    /// - `shim_c`: stable `aven_solve` adapter with domain checks
    /// - `single_unit_c`: all-in-one compilation unit
    pub fn generate_aeric_shim(
        block: &BlockDecl,
        model_stem: &str,
    ) -> Result<AericShimBundle, CodegenError> {
        let order = Self::topo_sort(block)?;
        let equal_indices: Vec<usize> = order.clone();

        let derived: HashSet<String> = equal_indices
            .iter()
            .map(|&i| block.equations[i].lhs.clone())
            .collect();
        let mut inputs_set = BTreeSet::new();
        for &idx in &equal_indices {
            for v in block.equations[idx].rhs.vars() {
                if !derived.contains(&v) {
                    inputs_set.insert(v);
                }
            }
        }
        let inputs: Vec<String> = inputs_set.into_iter().collect();
        let outputs: Vec<String> = equal_indices
            .iter()
            .map(|&idx| block.equations[idx].lhs.clone())
            .collect();

        let n_in = inputs.len();
        let n_out = outputs.len();
        let stem = if model_stem.is_empty() {
            block.name.to_lowercase()
        } else {
            model_stem.to_string()
        };
        let stem_upper = stem.to_uppercase().replace('-', "_");

        // Compute deterministic SHA-256 model-identity hash
        let mut spec_repr = format!("block:{}\ninputs:\n", block.name);
        for inp in &inputs {
            spec_repr.push_str(&format!("  {}\n", inp));
        }
        spec_repr.push_str("equations:\n");
        for &idx in &order {
            let eq = &block.equations[idx];
            spec_repr.push_str(&format!("  {} = {}\n", eq.lhs, eq.rhs));
        }
        let model_hash = crate::evaluator::sha256_hex(spec_repr.as_bytes());

        // Model C generation (without main)
        let model_options = CodegenOptions {
            traceable: true,
            checked_c: false,
            aeric_shim: true,
            no_main: true,
        };
        let empty_bindings = HashMap::new();
        let model_c = Self::transpile_block_with_options(block, &empty_bindings, &model_options)?;

        // Header generation: model_<stem>.h
        let mut h_lines = Vec::new();
        let guard = format!("MODEL_{}_H", stem_upper);
        h_lines.push(format!("/* Generated by avenc -- AERIC model header for '{}' */", block.name));
        h_lines.push(format!("#ifndef {}", guard));
        h_lines.push(format!("#define {}", guard));
        h_lines.push("".to_string());
        h_lines.push("#include <stddef.h>".to_string());
        h_lines.push("".to_string());
        h_lines.push(format!("#define MODEL_{}_N_IN   {}", stem_upper, n_in));
        h_lines.push(format!("#define MODEL_{}_N_OUT  {}", stem_upper, n_out));
        h_lines.push(format!("#define MODEL_{}_SHA256 \"{}\"", stem_upper, model_hash));
        h_lines.push("".to_string());
        if n_in > 0 {
            h_lines.push(format!("extern const char* const MODEL_{}_INPUT_NAMES[{}];", stem_upper, n_in));
        } else {
            h_lines.push(format!("extern const char* const MODEL_{}_INPUT_NAMES[1];", stem_upper));
        }
        if n_out > 0 {
            h_lines.push(format!("extern const char* const MODEL_{}_OUTPUT_NAMES[{}];", stem_upper, n_out));
        } else {
            h_lines.push(format!("extern const char* const MODEL_{}_OUTPUT_NAMES[1];", stem_upper));
        }
        h_lines.push("".to_string());

        // Function signature in header
        let mut param_parts: Vec<String> = inputs.iter().map(|p| format!("double {}", p)).collect();
        for out in &outputs {
            param_parts.push(format!("double* out_{}", out));
        }
        h_lines.push(format!("void aven_{}({});", block.name, param_parts.join(", ")));
        h_lines.push("".to_string());
        h_lines.push("/* Stable AERIC ABI */".to_string());
        h_lines.push("int aven_solve(const double *in, size_t n_in, double *out, size_t n_out);".to_string());
        h_lines.push("const char* aven_model_hash(void);".to_string());
        h_lines.push("size_t aven_model_n_in(void);".to_string());
        h_lines.push("size_t aven_model_n_out(void);".to_string());
        h_lines.push("".to_string());
        h_lines.push(format!("#endif /* {} */", guard));
        let header_h = h_lines.join("\n");

        // Shim generation: aven_shim.c (or model_<stem>_shim.c)
        let mut s_lines = Vec::new();
        s_lines.push(format!("/* Generated by avenc -- AERIC adapter shim for '{}' */", block.name));
        s_lines.push(format!("#include \"model_{}.h\"", stem));
        s_lines.push("#include <math.h>".to_string());
        s_lines.push("#include <stddef.h>".to_string());
        s_lines.push("".to_string());

        // Input names array
        let in_size = if n_in > 0 { n_in } else { 1 };
        s_lines.push(format!("const char* const MODEL_{}_INPUT_NAMES[{}] = {{", stem_upper, in_size));
        for inp in &inputs {
            s_lines.push(format!("    \"{}\",", inp));
        }
        if n_in == 0 {
            s_lines.push("    \"\"".to_string());
        }
        s_lines.push("};".to_string());
        s_lines.push("".to_string());

        // Output names array
        let out_size = if n_out > 0 { n_out } else { 1 };
        s_lines.push(format!("const char* const MODEL_{}_OUTPUT_NAMES[{}] = {{", stem_upper, out_size));
        for out in &outputs {
            s_lines.push(format!("    \"{}\",", out));
        }
        if n_out == 0 {
            s_lines.push("    \"\"".to_string());
        }
        s_lines.push("};".to_string());
        s_lines.push("".to_string());

        s_lines.push(format!("const char* aven_model_hash(void) {{ return \"{}\"; }}", model_hash));
        s_lines.push(format!("size_t aven_model_n_in(void) {{ return {}; }}", n_in));
        s_lines.push(format!("size_t aven_model_n_out(void) {{ return {}; }}", n_out));
        s_lines.push("".to_string());

        s_lines.push("int aven_solve(const double *in, size_t n_in, double *out, size_t n_out) {".to_string());
        s_lines.push("    if (!in || !out) return 1;".to_string());
        s_lines.push(format!("    if (n_in != {} || n_out != {}) return 1; /* contract mismatch */", n_in, n_out));
        s_lines.push("".to_string());

        // Call generated kernel
        let mut local_vars = Vec::new();
        for out in &outputs {
            local_vars.push(format!("r_{}", out));
        }
        s_lines.push(format!("    double {};", local_vars.join(", ")));

        let mut call_in_parts = Vec::new();
        for (idx, _inp) in inputs.iter().enumerate() {
            call_in_parts.push(format!("in[{}]", idx));
        }
        for out in &outputs {
            call_in_parts.push(format!("&r_{}", out));
        }
        s_lines.push(format!("    aven_{}({});", block.name, call_in_parts.join(", ")));
        s_lines.push("".to_string());

        // Domain finiteness check: if any output is NaN or Inf, reject with code 2
        let finite_checks: Vec<String> = outputs.iter().map(|o| format!("isfinite(r_{})", o)).collect();
        s_lines.push("    /* Non-aborting domain check: catch NaN/Inf from domain errors and signal HOLD */".to_string());
        if finite_checks.is_empty() {
            s_lines.push("    /* No outputs to validate */".to_string());
        } else {
            s_lines.push(format!("    if (!({})) return 2;", finite_checks.join(" && ")));
        }
        s_lines.push("".to_string());

        // Assign to out buffer
        for (idx, out) in outputs.iter().enumerate() {
            s_lines.push(format!("    out[{}] = r_{};", idx, out));
        }
        s_lines.push("    return 0;".to_string());
        s_lines.push("}".to_string());
        let shim_c = s_lines.join("\n");

        // Single-unit C (combining model kernel and shim in one standalone file)
        let mut u_lines = Vec::new();
        u_lines.push(format!("/* Generated by avenc -- Single-Unit AERIC Kernel + Shim for '{}' */", block.name));
        u_lines.push(format!("/* Model SHA256: {} */", model_hash));
        u_lines.push(model_c.clone());
        u_lines.push("".to_string());
        u_lines.push(format!("#define MODEL_{}_N_IN   {}", stem_upper, n_in));
        u_lines.push(format!("#define MODEL_{}_N_OUT  {}", stem_upper, n_out));
        u_lines.push(format!("#define MODEL_{}_SHA256 \"{}\"", stem_upper, model_hash));
        u_lines.push("".to_string());
        u_lines.push(format!("const char* const MODEL_{}_INPUT_NAMES[{}] = {{", stem_upper, in_size));
        for inp in &inputs {
            u_lines.push(format!("    \"{}\",", inp));
        }
        if n_in == 0 {
            u_lines.push("    \"\"".to_string());
        }
        u_lines.push("};".to_string());
        u_lines.push(format!("const char* const MODEL_{}_OUTPUT_NAMES[{}] = {{", stem_upper, out_size));
        for out in &outputs {
            u_lines.push(format!("    \"{}\",", out));
        }
        if n_out == 0 {
            u_lines.push("    \"\"".to_string());
        }
        u_lines.push("};".to_string());
        u_lines.push("".to_string());
        u_lines.push(format!("const char* aven_model_hash(void) {{ return \"{}\"; }}", model_hash));
        u_lines.push(format!("size_t aven_model_n_in(void) {{ return {}; }}", n_in));
        u_lines.push(format!("size_t aven_model_n_out(void) {{ return {}; }}", n_out));
        u_lines.push("".to_string());
        u_lines.push("int aven_solve(const double *in, size_t n_in, double *out, size_t n_out) {".to_string());
        u_lines.push("    if (!in || !out) return 1;".to_string());
        u_lines.push(format!("    if (n_in != {} || n_out != {}) return 1; /* contract mismatch */", n_in, n_out));
        u_lines.push("".to_string());
        u_lines.push(format!("    double {};", local_vars.join(", ")));
        u_lines.push(format!("    aven_{}({});", block.name, call_in_parts.join(", ")));
        u_lines.push("".to_string());
        if !finite_checks.is_empty() {
            u_lines.push(format!("    if (!({})) return 2;", finite_checks.join(" && ")));
        }
        u_lines.push("".to_string());
        for (idx, out) in outputs.iter().enumerate() {
            u_lines.push(format!("    out[{}] = r_{};", idx, out));
        }
        u_lines.push("    return 0;".to_string());
        u_lines.push("}".to_string());
        let single_unit_c = u_lines.join("\n");

        Ok(AericShimBundle {
            model_stem: stem,
            block_name: block.name.clone(),
            model_hash,
            n_in,
            n_out,
            input_names: inputs,
            output_names: outputs,
            header_h,
            model_c,
            shim_c,
            single_unit_c,
        })
    }
}

fn collect_denominators(expr: &Expr, denoms: &mut HashSet<String>) {
    match expr {
        Expr::Bin(BinaryOp::Div, left, right) => {
            collect_denominators(left, denoms);
            for v in right.vars() {
                denoms.insert(v);
            }
            collect_denominators(right, denoms);
        }
        Expr::Bin(_, left, right) => {
            collect_denominators(left, denoms);
            collect_denominators(right, denoms);
        }
        Expr::UnaryMinus(inner) => collect_denominators(inner, denoms),
        Expr::Call(_, arg) => collect_denominators(arg, denoms),
        _ => {}
    }
}

fn expr_to_c(expr: &Expr, needs_math: &mut bool) -> String {
    match expr {
        Expr::Num(n) => {
            if n.fract() == 0.0 && n.abs() < 1e12 {
                format!("{:.1}", n)
            } else {
                format!("{}", n)
            }
        }
        Expr::Var(name) => match name.as_str() {
            "pi" | "PI" => "3.14159265358979323846".to_string(),
            "e" | "E" => "2.71828182845904523536".to_string(),
            "tau" | "TAU" => "6.28318530717958647692".to_string(),
            other => other.to_string(),
        },
        Expr::UnaryMinus(inner) => format!("-({})", expr_to_c(inner, needs_math)),
        Expr::Call(fn_name, arg) => {
            *needs_math = true;
            match fn_name.as_str() {
                // Same snapping rule as the evaluator: values within 1e-9 of a whole number
                // round to it, so ceil(1000.3 / 0.7) is 1429 in C exactly as in `avenc run`
                "ceil" => format!("aven_ceil({})", expr_to_c(arg, needs_math)),
                "floor" => format!("aven_floor({})", expr_to_c(arg, needs_math)),
                "round" => format!("round({})", expr_to_c(arg, needs_math)),
                "abs" => format!("fabs({})", expr_to_c(arg, needs_math)),
                // C's log() is the natural logarithm
                "ln" | "log" => format!("log({})", expr_to_c(arg, needs_math)),
                "sign" => {
                    let a = expr_to_c(arg, needs_math);
                    format!("(double)(({} > 0.0) - ({} < 0.0))", a, a)
                }
                other => format!("{}({})", other, expr_to_c(arg, needs_math)),
            }
        }
        Expr::Bin(op, left, right) => {
            let l = expr_to_c(left, needs_math);
            let r = expr_to_c(right, needs_math);
            match op {
                BinaryOp::Add => format!("({} + {})", l, r),
                BinaryOp::Sub => format!("({} - {})", l, r),
                BinaryOp::Mul => format!("({} * {})", l, r),
                BinaryOp::Div => format!("({} / {})", l, r),
                BinaryOp::Pow => {
                    *needs_math = true;
                    format!("pow({}, {})", l, r)
                }
            }
        }
    }
}

/// `#include <math.h>` plus the snapping helpers the emitted code calls. The helpers are
/// branch-free: a comparison yields 0 or 1 and selects between the two candidates.
fn math_prelude(code: &str) -> String {
    let mut out = String::from("#include <math.h>");
    for (name, base) in [("aven_ceil", "ceil"), ("aven_floor", "floor")] {
        if code.contains(&format!("{}(", name)) {
            out.push_str(&[
                String::new(),
                String::new(),
                format!("/* {} with the evaluator's 1e-9 snap to the nearest whole number */", base),
                format!("static double {}(double v) {{", name),
                "    const double r = round(v);".to_string(),
                "    const double snap = (double)(fabs(v - r) < 1e-9);".to_string(),
                format!("    return snap * r + (1.0 - snap) * {}(v);", base),
                "}".to_string(),
            ]
            .join("
"));
        }
    }
    out
}
