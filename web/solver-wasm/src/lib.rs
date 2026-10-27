// aven-solver-wasm — the real avenc omnidirectional solver, compiled to wasm32
// and exposed over a tiny hand-written ABI (no wasm-bindgen, so avenc stays
// zero-dependency). The browser writes a model's source text into wasm memory,
// calls `solve`, and reads back a length-prefixed UTF-8 JSON buffer.
//
// ABI:
//   alloc(len)            -> ptr        // reserve `len` bytes for the source text
//   solve(ptr, len)       -> out_ptr    // out buffer = [u32 len LE][json bytes]
//   dealloc(ptr, len)                   // free a buffer handed back to JS
//
// The solve path is exactly what `avenc run` does for a relation: lex, parse,
// load every Relation block's equations/integer vars, apply the solve block's
// constraints/objectives/bindings, and run the Newton/optimizing solver.

use avenc::ast::BlockKind;
use avenc::lexer::Lexer;
use avenc::parser::Parser;
use avenc::solver::SolverEngine;
use std::collections::HashMap;

/// Reserve `len` bytes of wasm memory and hand the pointer to the host.
#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Free a buffer previously handed to the host (input or output).
///
/// # Safety
/// `ptr`/`len` must come from `alloc` or a `solve` result and be freed once.
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len != 0 {
        let _ = Vec::from_raw_parts(ptr, 0, len);
    }
}

/// Solve the model whose UTF-8 source is `len` bytes at `ptr`.
/// Returns a pointer to `[u32 payload_len LE][payload]`; payload is JSON.
///
/// # Safety
/// `ptr`/`len` must describe a valid buffer written by the host via `alloc`.
#[no_mangle]
pub unsafe extern "C" fn solve(ptr: *const u8, len: usize) -> *mut u8 {
    let src = {
        let bytes = std::slice::from_raw_parts(ptr, len);
        std::str::from_utf8(bytes).unwrap_or("").to_owned()
    };
    pack(run_solve(&src))
}

/// Length-prefix a JSON string into a freshly leaked buffer for the host.
fn pack(json: String) -> *mut u8 {
    let bytes = json.into_bytes();
    let mut out = Vec::with_capacity(4 + bytes.len());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&bytes);
    let p = out.as_mut_ptr();
    std::mem::forget(out);
    p
}

fn run_solve(src: &str) -> String {
    let tokens = match Lexer::new(src).tokenize() {
        Ok(t) => t,
        Err(e) => return err_json("lex", &format!("{e:?}")),
    };
    let module = match Parser::new(tokens).parse_module("demo") {
        Ok(m) => m,
        Err(e) => return err_json("parse", &format!("{e:?}")),
    };

    let mut solver = SolverEngine::new();
    for blk in &module.blocks {
        if blk.kind == BlockKind::Relation {
            for eq in &blk.equations {
                solver.add_relation(eq.clone());
            }
            for iv in &blk.integer_vars {
                solver.add_integer_var(iv.clone());
            }
        }
    }
    let mut env = HashMap::new();
    for s in &module.solves {
        for c in &s.constraints {
            solver.add_constraint(c.clone());
        }
        for o in &s.objectives {
            solver.add_objective(o.clone());
        }
        for iv in &s.integer_vars {
            solver.add_integer_var(iv.clone());
        }
        for b in &s.bindings {
            env.insert(b.name.clone(), b.value);
        }
    }

    match solver.solve(&env) {
        Ok(settled) => ok_json(&settled),
        Err(e) => err_json("solve", &format!("{e:?}")),
    }
}

fn ok_json(values: &HashMap<String, f64>) -> String {
    let mut keys: Vec<&String> = values.keys().collect();
    keys.sort();
    let mut s = String::from("{\"ok\":true,\"values\":{");
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let v = values[*k];
        // JSON has no NaN/Infinity; emit null so the host can flag it.
        if v.is_finite() {
            s.push_str(&format!("{}:{}", json_str(k), fmt_num(v)));
        } else {
            s.push_str(&format!("{}:null", json_str(k)));
        }
    }
    s.push_str("}}");
    s
}

fn err_json(stage: &str, msg: &str) -> String {
    format!(
        "{{\"ok\":false,\"stage\":{},\"error\":{}}}",
        json_str(stage),
        json_str(msg)
    )
}

/// Minimal JSON string escaping (zero-dependency; no serde).
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn fmt_num(v: f64) -> String {
    // Round-trippable, compact: integers print without a trailing ".0".
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}
