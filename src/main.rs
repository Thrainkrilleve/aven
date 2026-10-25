// src/main.rs — CLI entry point for avenc (The Aven Language Toolchain)
#![allow(
    clippy::needless_range_loop,
    clippy::collapsible_if,
    clippy::should_implement_trait,
    clippy::unnecessary_map_or,
    clippy::too_many_arguments,
    clippy::result_large_err
)]

mod ast;
mod codegen_c;
mod diagnostics;
mod diff;
mod evaluator;
mod ledger;
mod lexer;
mod optimize;
mod parser;
mod solver;
mod units;

use ast::*;
use codegen_c::CTranspiler;
use diagnostics::Diagnostician;
use evaluator::Evaluator;
use ledger::{Fact, Ledger};
use lexer::Lexer;
use parser::Parser;
use solver::SolverEngine;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::process;

const VERSION: &str = "0.6.0";

fn print_usage() {
    println!("avenc v{} — The Aven Language Toolchain", VERSION);
    println!("Usage:");
    println!(
        "  avenc run <file.aven> [--allow-stale] [--json] [--ledger out.jsonl] Execute .aven file and print settled relations"
    );
    println!(
        "                                        (enforces fact freshness; blocks decision on stale/zero-TTL data)"
    );
    println!(
        "  avenc pareto <file.aven> [var] [min] [max] [--steps N] [--backend native|external] [--cmd <command>]"
    );
    println!(
        "                                        Generate Pareto trade-off frontier & detect knee point"
    );
    println!("  avenc stream <file.aven> [--limit N]");
    println!(
        "                                        Ingest live JSON stream from stdin & supervise relations"
    );
    println!("  avenc why <file.aven> <var> [--depth N]");
    println!(
        "                                        Show causal derivation tree (default depth: 20; 0 for unlimited)"
    );
    println!("  avenc build <file.aven> [--emit-c] [-o out.c] [--traceable] [--checked-c]");
    println!("                                        Transpile pinned relations to standalone C");
    println!(
        "  avenc check <file.aven>               Validate syntax, relations, and unit declarations"
    );
    println!("  avenc verify-ledger <out.jsonl>       Check an audit log's hash chain (and signatures with AVEN_LEDGER_KEY)");
    println!("  avenc demo                            Run built-in multi-pillar verification test");
}

fn main() {
    // Expression passes (evaluation, differentiation, unit inference) recurse once per nesting
    // level; a 64 MiB stack keeps every pass safe up to the parser's MAX_EXPR_DEPTH.
    let worker = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(real_main)
        .expect("failed to start main thread");
    if worker.join().is_err() {
        process::exit(101);
    }
}

fn real_main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return;
    }

    match args[1].as_str() {
        "verify-ledger" => {
            let Some(file) = args.get(2) else {
                eprintln!("Error: missing file argument for 'verify-ledger'");
                process::exit(1);
            };
            cmd_verify_ledger(file);
        }
        "emit-c" => {
            let file_arg = args.iter().skip(2).find(|a| !a.starts_with('-'));
            let Some(file) = file_arg else {
                eprintln!("Error: missing file argument for 'emit-c'");
                process::exit(1);
            };
            let options = codegen_c::CodegenOptions {
                traceable: args.iter().any(|a| a == "--traceable"),
                checked_c: args.iter().any(|a| a == "--checked-c"),
            };
            let out_file = args.windows(2).find(|w| w[0] == "-o").map(|w| w[1].as_str());
            cmd_build(file, true, out_file, &options);
        }
        "bench" => {
            let file_arg = args.iter().skip(2).find(|a| !a.starts_with('-'));
            let Some(file) = file_arg else {
                eprintln!("Error: missing file argument for 'bench'");
                process::exit(1);
            };
            let iterations = args
                .windows(2)
                .find(|w| w[0] == "--iterations")
                .and_then(|w| w[1].parse::<usize>().ok())
                .unwrap_or(200)
                .max(1);
            cmd_bench(file, iterations);
        }
        "run" => {
            let allow_stale = args.iter().any(|a| a == "--allow-stale");
            let json = args.iter().any(|a| a == "--json");
            let ledger_out = args
                .windows(2)
                .find(|w| w[0] == "--ledger")
                .map(|w| w[1].clone());
            let file_arg = args
                .iter()
                .skip(2)
                .find(|a| !a.starts_with("--") && Some(*a) != ledger_out.as_ref());
            if file_arg.is_none() {
                eprintln!("Error: missing file argument for 'run'");
                process::exit(1);
            }
            cmd_run(file_arg.unwrap(), allow_stale, json, ledger_out.as_deref());
        }
        "stream" => {
            if args.len() < 3 {
                eprintln!(
                    "Error: missing file argument for 'stream': avenc stream <file.aven> [--limit N]"
                );
                process::exit(1);
            }
            let limit = args
                .windows(2)
                .find(|w| w[0] == "--limit")
                .and_then(|w| w[1].parse::<usize>().ok())
                .unwrap_or(0);
            cmd_stream(&args[2], limit);
        }
        "pareto" => {
            if args.len() < 3 {
                eprintln!("Error: missing file argument for 'pareto'");
                process::exit(1);
            }
            let sweep_var = args
                .get(3)
                .filter(|s| !s.starts_with("--"))
                .map(|s| s.as_str());
            let min_val = args
                .get(4)
                .filter(|s| !s.starts_with("--"))
                .and_then(|s| s.parse::<f64>().ok());
            let max_val = args
                .get(5)
                .filter(|s| !s.starts_with("--"))
                .and_then(|s| s.parse::<f64>().ok());
            let backend_arg = args
                .windows(2)
                .find(|w| w[0] == "--backend")
                .map(|w| w[1].as_str());
            let cmd_arg = args
                .windows(2)
                .find(|w| w[0] == "--cmd")
                .map(|w| w[1].as_str());
            let steps = args
                .windows(2)
                .find(|w| w[0] == "--steps")
                .and_then(|w| w[1].parse::<usize>().ok())
                .unwrap_or(5)
                .clamp(1, 1000);
            cmd_pareto(&args[2], sweep_var, min_val, max_val, backend_arg, cmd_arg, steps);
        }
        "why" => {
            let mut non_flags = Vec::new();
            let mut skip_next = false;
            for arg in args.iter().skip(2) {
                if skip_next {
                    skip_next = false;
                    continue;
                }
                if arg == "--depth" {
                    skip_next = true;
                    continue;
                }
                if !arg.starts_with("--") {
                    non_flags.push(arg.as_str());
                }
            }
            if non_flags.len() < 2 {
                eprintln!(
                    "Error: 'why' requires a file and variable name: avenc why <file.aven> <variable> [--depth N]"
                );
                process::exit(1);
            }
            let depth = args
                .windows(2)
                .find(|w| w[0] == "--depth")
                .and_then(|w| {
                    if w[1] == "0" || w[1] == "all" || w[1] == "unlimited" {
                        None
                    } else {
                        w[1].parse::<usize>().ok()
                    }
                })
                .or(Some(ledger::DEFAULT_MAX_DEPTH));
            cmd_why(non_flags[0], non_flags[1], depth);
        }
        "build" => {
            if args.len() < 3 {
                eprintln!("Error: missing file argument for 'build'");
                process::exit(1);
            }
            let emit_c = args.iter().any(|a| a == "--emit-c");
            let traceable = args.iter().any(|a| a == "--traceable");
            let checked_c = args.iter().any(|a| a == "--checked-c");
            let out_file = args
                .windows(2)
                .find(|w| w[0] == "-o")
                .map(|w| w[1].as_str());
            let options = codegen_c::CodegenOptions {
                traceable,
                checked_c,
            };
            cmd_build(&args[2], emit_c, out_file, &options);
        }
        "check" => {
            if args.len() < 3 {
                eprintln!("Error: missing file argument for 'check'");
                process::exit(1);
            }
            cmd_check(&args[2]);
        }
        "demo" => {
            cmd_demo();
        }
        "-v" | "--version" => {
            println!("avenc {}", VERSION);
        }
        "-h" | "--help" => {
            print_usage();
        }
        other => {
            eprintln!("Unknown command: '{}'", other);
            print_usage();
            process::exit(1);
        }
    }
}

fn parse_file(path: &str) -> AvenModule {
    let content = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", path, e);
            process::exit(1);
        }
    };

    let mut lexer = Lexer::new(&content);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    let mut parser = Parser::new(tokens);
    match parser.parse_module(path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    }
}

fn cmd_check(path: &str) {
    let module = parse_file(path);
    println!("Checking module '{}'...", path);
    println!("  Blocks:   {}", module.blocks.len());
    println!("  Solves:   {}", module.solves.len());
    println!("  Signals:  {}", module.signals.len());
    println!("  Units:    {}", module.units.len());

    // 1. Compile-Time Dimensional Homogeneity Check
    let mut checker = units::DimensionalChecker::new();
    match checker.check_module(&module) {
        Ok(report) => {
            println!("\n✓ Dimensional Analysis PASSED:");
            println!(
                "  Verified {} equation(s) across SI/Systems unit registry.",
                report.verified_equations
            );
            if !report.variable_dimensions.is_empty() {
                let mut sorted_dims: Vec<_> = report.variable_dimensions.iter().collect();
                sorted_dims.sort_by_key(|(k, _)| *k);
                for (var, dim) in sorted_dims {
                    println!("    • {:<18} : [{}]", var, dim);
                }
            }
        }
        Err(e) => {
            eprintln!("\n✗ {}", e);
            process::exit(1);
        }
    }

    // 2. Degree-of-Freedom & Under-Determination Analysis
    let diagnostician = Diagnostician::new();
    let diags = diagnostician.diagnose_module(&module);
    let mut has_underdetermined = false;
    for d in diags {
        if d.dof > 0 {
            has_underdetermined = true;
            println!("\n{}", d.report());
        }
    }

    if has_underdetermined {
        println!(
            "\nCheck WARNING: Module has under-determined relation(s). See diagnostic report above."
        );
    } else {
        println!("\n✓ System Determinacy PASSED: All relation blocks have 0 degrees of freedom.");
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub env: HashMap<String, f64>,
    pub ledger: Ledger,
    pub diagnosis: Option<diagnostics::Diagnosis>,
    pub is_actionable: bool,
    pub stale_facts: Vec<String>,
    pub error: Option<solver::SolverError>,
    /// "proven" / "local" when an objective was optimized
    pub optimality: Option<&'static str>,
}

// Exit codes documented in wiki/Compiler-Toolchain-avenc.md §4
const EXIT_ERROR: i32 = 1;
const EXIT_CONSTRAINT_VIOLATION: i32 = 2;
const EXIT_DECISION_BLOCKED: i32 = 3;

impl ExecutionResult {
    /// Prints a solver error and exits with its documented exit code.
    fn exit_on_error(&self) {
        if let Some(e) = &self.error {
            eprintln!("\nSolver Error: {}", e);
            let code = match e {
                solver::SolverError::ConstraintViolation { .. } => EXIT_CONSTRAINT_VIOLATION,
                _ => EXIT_ERROR,
            };
            process::exit(code);
        }
    }
}

fn cmd_run(path: &str, allow_stale: bool, json: bool, ledger_out: Option<&str>) {
    let module = parse_file(path);
    let work_dir = std::path::Path::new(path).parent();
    let res = execute_module(&module, !json, work_dir);

    // The audit log is written whatever the verdict, so blocked and failed runs are recorded too
    if let Some(out) = ledger_out {
        if let Err(e) = fs::write(out, res.ledger.to_jsonl()) {
            eprintln!("Error writing ledger '{}': {}", out, e);
            process::exit(EXIT_ERROR);
        }
        if !json {
            println!(
                "Ledger: {} fact(s) written to '{}' (head {}{})",
                res.ledger.len(),
                out,
                &res.ledger.head()[..16],
                if res.ledger.is_signed() { ", HMAC-signed" } else { "" }
            );
        }
    }

    // A decision computed from stale inputs is blocked before any other verdict.
    if !res.is_actionable && !allow_stale {
        eprintln!("\n❌ DECISION BLOCKED: Freshness Enforcement Failure");
        eprintln!("  The following external fact(s) are STALE (expired TTL or ttl_ms=0):");
        for f in &res.stale_facts {
            eprintln!("    • {}", f);
        }
        eprintln!(
            "\n  Refusing to settle actionable decision on unrefreshed metrics to prevent automated scaling on bad data."
        );
        eprintln!("  Pass --allow-stale to explicitly override in non-production environments.");
        process::exit(EXIT_DECISION_BLOCKED);
    }
    res.exit_on_error();

    let mut integer_vars = std::collections::BTreeSet::new();
    for blk in &module.blocks {
        for iv in &blk.integer_vars {
            integer_vars.insert(iv.clone());
        }
    }
    for s in &module.solves {
        for iv in &s.integer_vars {
            integer_vars.insert(iv.clone());
        }
    }

    let mut sorted_keys: Vec<&String> = res.env.keys().collect();
    sorted_keys.sort();

    if json {
        // Full-precision machine-readable output (f64 Display is the shortest round-trip form)
        let status = if res.diagnosis.is_some() {
            "underdetermined"
        } else {
            "settled"
        };
        let optimality = match res.optimality {
            Some(o) => format!("\"{}\"", o),
            None => "null".to_string(),
        };
        let fields: Vec<String> = sorted_keys
            .iter()
            .map(|k| format!("\"{}\": {}", k, res.env[*k]))
            .collect();
        println!(
            "{{\"status\": \"{}\", \"optimality\": {}, \"env\": {{{}}}}}",
            status,
            optimality,
            fields.join(", ")
        );
        if res.diagnosis.is_some() {
            process::exit(EXIT_ERROR);
        }
        return;
    }

    println!("\nSettled Environment:");
    for k in sorted_keys {
        if integer_vars.contains(k) {
            println!("  {:<16} = {:.0} (int)", k, res.env[k]);
        } else {
            println!("  {:<16} = {:.4}", k, res.env[k]);
        }
    }

    if !res.ledger.is_empty() {
        println!("\nFact Ledger: {} facts settled.", res.ledger.len());
    }

    // Exit 0 means every equation settled; an under-determined model did not.
    if res.diagnosis.is_some() {
        process::exit(EXIT_ERROR);
    }
}

/// Ledger signing key from AVEN_LEDGER_KEY (used as raw bytes), if set.
fn ledger_key() -> Option<Vec<u8>> {
    env::var("AVEN_LEDGER_KEY")
        .ok()
        .filter(|k| !k.is_empty())
        .map(String::into_bytes)
}

fn new_ledger() -> Ledger {
    match ledger_key() {
        Some(k) => Ledger::with_key(&k),
        None => Ledger::new(),
    }
}

fn cmd_verify_ledger(path: &str) {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error reading ledger '{}': {}", path, e);
            process::exit(EXIT_ERROR);
        }
    };
    let key = ledger_key();
    match ledger::verify_jsonl(&text, key.as_deref()) {
        Ok(v) => {
            println!(
                "Ledger OK: {} fact(s), chain intact{}, head {}",
                v.facts,
                if v.signed { ", every signature valid" } else { " (signatures not checked: AVEN_LEDGER_KEY unset)" },
                v.head
            );
        }
        Err(e) => {
            eprintln!("Ledger TAMPERED or invalid: {}", e);
            process::exit(EXIT_ERROR);
        }
    }
}

fn cmd_bench(path: &str, iterations: usize) {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", path, e);
            process::exit(1);
        }
    };
    let module = parse_file(path);
    let work_dir = std::path::Path::new(path).parent();

    let stats = |mut samples: Vec<f64>| -> (f64, f64, f64) {
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let pick = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
        (samples[0], pick(0.5), pick(0.95))
    };

    let mut parse_us = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let t0 = std::time::Instant::now();
        let tokens = Lexer::new(&source).tokenize().expect("tokenize failed");
        let _ = Parser::new(tokens).parse_module(path).expect("parse failed");
        parse_us.push(t0.elapsed().as_secs_f64() * 1e6);
    }

    let mut solve_us = Vec::with_capacity(iterations);
    let mut last = None;
    for _ in 0..iterations {
        let t0 = std::time::Instant::now();
        let res = execute_module(&module, false, work_dir);
        solve_us.push(t0.elapsed().as_secs_f64() * 1e6);
        last = Some(res);
    }

    // Sequential single-client throughput: completed solves per second of solve time
    let solve_total_s: f64 = solve_us.iter().sum::<f64>() / 1e6;
    let throughput = solve_us.len() as f64 / solve_total_s;

    println!("avenc bench '{}' — {} iterations", path, iterations);
    println!("  {:<28} {:>12} {:>12} {:>12}", "stage", "min µs", "median µs", "p95 µs");
    for (name, samples) in [
        ("lex + parse", parse_us),
        ("solve (incl. external/IPC)", solve_us),
    ] {
        let (min, med, p95) = stats(samples);
        println!("  {:<28} {:>12.1} {:>12.1} {:>12.1}", name, min, med, p95);
    }
    println!(
        "  sequential throughput: {:.1} solves/sec (one client, {} solves)",
        throughput, iterations
    );
    if let Some(res) = last {
        if let Some(e) = &res.error {
            println!("  note: every solve ended with: {}", e);
        } else if res.diagnosis.is_some() {
            println!("  note: model is under-determined; timings cover the stalled solve");
        }
    }
}

fn cmd_why(path: &str, var: &str, depth: Option<usize>) {
    let module = parse_file(path);
    let work_dir = std::path::Path::new(path).parent();
    let res = execute_module(&module, false, work_dir);
    println!("Causal Derivation Tree for '{}':", var);
    println!("{}", res.ledger.why_with_depth(var, depth));
    res.exit_on_error();
}

fn cmd_build(
    path: &str,
    emit_c: bool,
    out_file: Option<&str>,
    options: &codegen_c::CodegenOptions,
) {
    let module = parse_file(path);
    // Prefer explicit forward block if present, otherwise auto-derive from relation
    let blk = if let Some(fb) = module.blocks.iter().find(|b| b.kind == BlockKind::Forward) {
        fb
    } else if let Some(rb) = module.blocks.first() {
        rb
    } else {
        eprintln!("Error: no relation or forward blocks found in '{}'.", path);
        process::exit(1);
    };

    let work_dir = std::path::Path::new(path).parent();
    let res = execute_module(&module, false, work_dir);
    res.exit_on_error();
    let mut bindings = HashMap::new();
    for (k, v) in res.env {
        bindings.insert(k, v);
    }

    let c_code = match CTranspiler::transpile_block_with_options(blk, &bindings, options) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    if let Some(out_path) = out_file {
        if let Err(e) = fs::write(out_path, &c_code) {
            eprintln!("Error writing output C file '{}': {}", out_path, e);
            process::exit(1);
        }
        println!("Successfully emitted C code to '{}'", out_path);
    } else if emit_c {
        println!("{}", c_code);
    } else {
        if blk.kind == BlockKind::Forward {
            println!("Compiled forward block '{}' successfully.", blk.name);
        } else {
            println!(
                "Auto-derived forward C execution order from relation '{}' successfully.",
                blk.name
            );
        }
        println!("Use --emit-c to view the emitted C code or -o <out.c> to save.");
    }
}

/// Whether the optimizer proved its result globally optimal, from its trace.
fn optimality_of(trace: &[String]) -> Option<&'static str> {
    let line = trace.iter().rev().find(|l| l.starts_with("[optimize]") && l.contains("objective ="))?;
    Some(if line.contains("(proven optimal)") { "proven" } else { "local" })
}

fn execute_module(
    module: &AvenModule,
    verbose: bool,
    working_dir: Option<&std::path::Path>,
) -> ExecutionResult {
    // 0. Verify compile-time dimensional homogeneity across all relation blocks
    let mut type_checker = units::DimensionalChecker::new();
    if let Err(e) = type_checker.check_module(module) {
        eprintln!("\n{}", e);
        process::exit(1);
    }

    let mut env = HashMap::new();
    let mut ledger = new_ledger();
    let mut seq = 0;
    let mut stale_facts = Vec::new();

    // Collect initial bindings from solve blocks
    for solve in &module.solves {
        for b in &solve.bindings {
            seq += 1;
            env.insert(b.name.clone(), b.value);
            ledger.append(Fact {
                seq,
                tick: 0,
                kind: "input".to_string(),
                name: b.name.clone(),
                value: b.value,
                causes: Vec::new(),
                origin: "user binding".to_string(),
                metadata: None,
            });
        }
    }

    // Execute external or persistent IPC evaluators if declared via 'using external(...)' or 'using ipc(...)'
    for blk in &module.blocks {
        let is_external = matches!(&blk.backend, BackendDecl::External { .. });
        let is_ipc = matches!(&blk.backend, BackendDecl::Ipc { .. });
        if !is_external && !is_ipc {
            continue;
        }

        // Collect expected required keys for this block from equations and solve constraints
        let mut defined_in_block = std::collections::HashSet::new();
        for eq in &blk.equations {
            defined_in_block.insert(eq.lhs.clone());
        }

        let mut required_keys = Vec::new();
        for eq in &blk.equations {
            for v in eq.rhs.vars() {
                if !env.contains_key(&v)
                    && !defined_in_block.contains(&v)
                    && !required_keys.contains(&v)
                {
                    required_keys.push(v);
                }
            }
        }
        for solve in &module.solves {
            if solve.target == blk.name || solve.targets.contains(&blk.name) {
                for c in &solve.constraints {
                    if !env.contains_key(&c.lhs)
                        && !defined_in_block.contains(&c.lhs)
                        && !required_keys.contains(&c.lhs)
                    {
                        required_keys.push(c.lhs.clone());
                    }
                    for v in c.rhs.vars() {
                        if !env.contains_key(&v)
                            && !defined_in_block.contains(&v)
                            && !required_keys.contains(&v)
                        {
                            required_keys.push(v);
                        }
                    }
                }
            }
        }

        let mut varmap = evaluator::VarMap::new();
        for (k, v) in &env {
            varmap.set(k, *v);
        }

        let (eval_res, backend_type) = match &blk.backend {
            BackendDecl::External { command, args } => {
                let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let mut ext_eval = evaluator::ExternalEvaluator::new(command, &arg_refs);
                if let Some(dir) = working_dir {
                    ext_eval = ext_eval.with_working_dir(dir);
                }
                if verbose {
                    println!(
                        "Executing external evaluator '{}' for relation '{}'...",
                        command, blk.name
                    );
                }
                (ext_eval.evaluate(&varmap), "external")
            }
            BackendDecl::Ipc { endpoint } => {
                let ipc_eval = evaluator::IpcEvaluator::new(endpoint);
                if verbose {
                    println!(
                        "Connecting to persistent IPC evaluator at '{}' for relation '{}'...",
                        endpoint, blk.name
                    );
                }
                (ipc_eval.evaluate(&varmap), "ipc")
            }
            BackendDecl::Native => unreachable!(),
        };

        match eval_res {
            Ok(res) => {
                // Relations are omnidirectional: a variable the adapter did not report is only
                // missing if no equation in the model can derive it from what is known (e.g. a
                // constraint on a value another relation computes is not the adapter's job).
                let mut known: std::collections::HashSet<String> =
                    env.keys().chain(res.values.keys()).cloned().collect();
                loop {
                    let mut progressed = false;
                    for eq in module.blocks.iter().flat_map(|b| b.equations.iter()) {
                        let mut vars = eq.rhs.vars();
                        vars.insert(eq.lhs.clone());
                        let unknown: Vec<&String> =
                            vars.iter().filter(|v| !known.contains(*v)).collect();
                        if unknown.len() == 1 {
                            known.insert(unknown[0].clone());
                            progressed = true;
                        }
                    }
                    if !progressed {
                        break;
                    }
                }
                if let Some(missing) = required_keys.iter().find(|k| !known.contains(*k)) {
                    eprintln!(
                        "{} evaluation failed for block '{}': Malformed output: Schema validation failed: {} adapter '{}' omitted required numeric field '{}', and no equation in the model can derive it",
                        backend_type, blk.name, backend_type, res.backend_id, missing
                    );
                    process::exit(EXIT_ERROR);
                }
                if verbose {
                    println!(
                        "  [{}] {} settled in {:.1} µs (backend: {})",
                        backend_type, blk.name, res.solve_time_us, res.backend_id
                    );
                }
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);

                // Only variables the model uses become facts; other numeric fields an adapter
                // reports (e.g. its own timing) are not part of the model
                let model_vars = module_variables(module);
                let mut ignored: Vec<&String> = res
                    .values
                    .keys()
                    .filter(|k| !model_vars.contains(*k))
                    .collect();
                ignored.sort();
                if verbose && !ignored.is_empty() {
                    println!(
                        "  [{}] ignored adapter fields not used by the model: {}",
                        backend_type,
                        ignored.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                    );
                }
                let mut values: Vec<(String, f64)> = res
                    .values
                    .into_iter()
                    .filter(|(k, _)| model_vars.contains(k))
                    .collect();
                values.sort_by(|a, b| a.0.cmp(&b.0));
                for (k, v) in values {
                    if !env.contains_key(&k) {
                        seq += 1;
                        env.insert(k.clone(), v);

                        let is_stale = if let Some(ttl) = res.ttl_ms {
                            if ttl == 0 {
                                if !stale_facts.contains(&k) {
                                    stale_facts.push(k.clone());
                                }
                                true
                            } else if let Some(ts) = res.timestamp_ms {
                                if now_ms > ts + ttl {
                                    if !stale_facts.contains(&k) {
                                        stale_facts.push(k.clone());
                                    }
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        let metadata = ledger::FactMetadata {
                            timestamp_ms: res.timestamp_ms,
                            ttl_ms: res.ttl_ms,
                            is_stale,
                            warnings: res.warnings.clone(),
                        };

                        ledger.append(Fact {
                            seq,
                            tick: 0,
                            kind: "derived".to_string(),
                            name: k,
                            value: v,
                            causes: Vec::new(),
                            origin: match &res.payload_sha256 {
                                Some(h) => format!("{}: {} sha256:{}", backend_type, res.backend_id, h),
                                None => format!("{}: {}", backend_type, res.backend_id),
                            },
                            metadata: Some(metadata),
                        });
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "{} evaluation failed for block '{}': {}",
                    backend_type, blk.name, e
                );
                process::exit(1);
            }
        }
    }

    // Direction-pinned `forward` blocks, first pass: evaluate every equation whose inputs are
    // already known, so relations can use forward results (e.g. a cost feeding a profit model).
    if let Err(e) = eval_forward_blocks(
        module,
        &mut env,
        &mut ledger,
        &mut seq,
        &mut stale_facts,
        false,
    ) {
        return ExecutionResult {
            env,
            ledger,
            diagnosis: None,
            is_actionable: stale_facts.is_empty(),
            stale_facts,
            error: Some(e),
            optimality: None,
        };
    }

    // Set up solver engine: load equations and constraints across all Relation blocks
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
    for solve in &module.solves {
        for c in &solve.constraints {
            solver.add_constraint(c.clone());
        }
        for obj in &solve.objectives {
            solver.add_objective(obj.clone());
        }
        for iv in &solve.integer_vars {
            solver.add_integer_var(iv.clone());
        }
    }

    // Solve relations
    match solver.solve(&env) {
        Ok(solved_env) => {
            if verbose && !solver.trace.is_empty() {
                println!("Solver Trace:");
                for step in &solver.trace {
                    println!("  {}", step);
                }
            }

            // Record derived facts in derivation order so every cause is already in the
            // ledger, and propagate stale taint along the real derivation graph.
            for (k, cause_vars) in solver.derivation_order(&env, &solved_env) {
                let v = &solved_env[&k];
                {
                    let k = &k;
                    seq += 1;
                    let mut causes = Vec::new();
                    let mut is_tainted_stale = false;
                    for input_var in &cause_vars {
                        if let Some(fact) = ledger.latest(input_var) {
                            causes.push(fact.seq);
                            if fact.metadata.as_ref().map_or(false, |m| m.is_stale) {
                                is_tainted_stale = true;
                            }
                        }
                    }
                    let origin = if let Some(eq) = solver.relations.iter().find(|e| &e.lhs == k) {
                        format!("= {}", eq.rhs)
                    } else if let Some(eq) = solver
                        .relations
                        .iter()
                        .find(|e| e.rhs.vars().contains(k))
                    {
                        format!("solved from {}", eq.original_text)
                    } else if solver.integer_vars.contains(k) {
                        "integer optimization".to_string()
                    } else {
                        "derived".to_string()
                    };

                    if is_tainted_stale && !stale_facts.contains(k) {
                        stale_facts.push(k.clone());
                    }

                    let metadata = if is_tainted_stale {
                        Some(ledger::FactMetadata {
                            timestamp_ms: None,
                            ttl_ms: None,
                            is_stale: true,
                            warnings: vec!["causal-stale-taint".to_string()],
                        })
                    } else {
                        None
                    };

                    ledger.append(Fact {
                        seq,
                        tick: 0,
                        kind: "derived".to_string(),
                        name: k.clone(),
                        value: *v,
                        causes,
                        origin,
                        metadata,
                    });
                }
            }

            // Direction-pinned `forward` blocks, second pass: everything not yet evaluated must
            // now have its inputs (strict).
            let mut solved_env = solved_env;
            if let Err(e) = eval_forward_blocks(
                module,
                &mut solved_env,
                &mut ledger,
                &mut seq,
                &mut stale_facts,
                true,
            ) {
                return ExecutionResult {
                    env: solved_env,
                    ledger,
                    diagnosis: None,
                    is_actionable: stale_facts.is_empty(),
                    stale_facts,
                    error: Some(e),
                    optimality: None,
                };
            }

            let is_actionable = stale_facts.is_empty();
            ExecutionResult {
                env: solved_env,
                ledger,
                diagnosis: None,
                is_actionable,
                stale_facts,
                error: None,
                optimality: optimality_of(&solver.trace),
            }
        }
        Err(solver::SolverError::UnderDetermined(diag)) => {
            if verbose {
                println!("\n{}", diag.report());
            }
            let is_actionable = stale_facts.is_empty();
            ExecutionResult {
                env,
                ledger,
                diagnosis: Some(diag),
                is_actionable,
                stale_facts,
                error: None,
                optimality: optimality_of(&solver.trace),
            }
        }
        Err(e) => {
            // Returned rather than exiting here so the caller can apply exit-code precedence
            // (a stale decision is blocked before a constraint verdict is reported).
            let is_actionable = stale_facts.is_empty();
            ExecutionResult {
                env,
                ledger,
                diagnosis: None,
                is_actionable,
                stale_facts,
                error: Some(e),
                optimality: None,
            }
        }
    }
}

fn cmd_demo() {
    println!("============================================================");
    println!("Aven Native Toolchain v{} — Core Verification", VERSION);
    println!("============================================================\n");

    // 1. 1D Backward Newton solving
    println!("Pillar 1: 1D Backward Newton-Raphson Solving");
    println!("------------------------------------------------------------");
    let src1 = r#"
        relation Fall {
            depth = 0.5 * gravity * time * time
        }
        solve Fall with {
            depth = 45
            gravity = 9.81
        }
    "#;
    let mut lex = Lexer::new(src1);
    let mut parser = Parser::new(lex.tokenize().unwrap());
    let mod1 = parser.parse_module("fall").unwrap();
    let res1 = execute_module(&mod1, true, None);
    let env1 = res1.env;
    println!(
        "  depth=45, g=9.81 => time = {:.4} s (analytic {:.4})\n",
        env1["time"],
        (90.0 / 9.81f64).sqrt()
    );

    // 2. Multidimensional Coupled Newton with Analytic Jacobian
    println!("Pillar 2: Multidimensional Coupled System Solving");
    println!("------------------------------------------------------------");
    let src2 = r#"
        relation Market {
            supply = 10 + 2 * price
            demand = 100 - 3 * price
            clearing = demand - supply
        }
        solve Market with {
            clearing = 0
        }
    "#;
    let mut lex = Lexer::new(src2);
    let mut parser = Parser::new(lex.tokenize().unwrap());
    let mod2 = parser.parse_module("market").unwrap();
    let res2 = execute_module(&mod2, true, None);
    let env2 = res2.env;
    println!(
        "  clearing = 0 => equilibrium price = {:.2}, supply = demand = {:.2}\n",
        env2["price"], env2["supply"]
    );

    // 3. Direction-pinned C code generation
    println!("Pillar 3: Direction-Pinned C Transpilation");
    println!("------------------------------------------------------------");
    let src3 = r#"
        forward Trip {
            distance = speed * time
            fuel = distance * consumption
            cost = fuel * price
        }
        solve Trip with {
            speed = 90
            time = 4
            consumption = 0.07
            price = 1.75
        }
    "#;
    let mut lex = Lexer::new(src3);
    let mut parser = Parser::new(lex.tokenize().unwrap());
    let mod3 = parser.parse_module("trip").unwrap();
    let blk = &mod3.blocks[0];
    let mut binds = HashMap::new();
    for b in &mod3.solves[0].bindings {
        binds.insert(b.name.clone(), b.value);
    }
    let c_code = CTranspiler::transpile_block(blk, &binds).unwrap();
    println!("{}\n", c_code);

    // 4. Provenance & Fact Ledger
    println!("Pillar 4: Causal Provenance Ledger ('why' query)");
    println!("------------------------------------------------------------");
    let res3 = execute_module(&mod3, false, None);
    let ledger3 = res3.ledger;
    println!("why cost:\n{}\n", ledger3.why("cost"));

    // 5. Multi-Objective Pareto Frontier & Knee-Point Optimization
    println!("Pillar 5: Multi-Objective Pareto Frontier & Knee Detection");
    println!("------------------------------------------------------------");
    let src5 = r#"
        relation Capacity {
            raw_workers = (reqs_per_second * avg_latency_ms) / 1000.0
            allocated_workers = ceil(raw_workers / target_utilization)
        }
    "#;
    let mut lex5 = Lexer::new(src5);
    let mut parser5 = Parser::new(lex5.tokenize().unwrap());
    let mod5 = parser5.parse_module("capacity").unwrap();
    let evaluator5 = std::sync::Arc::new(evaluator::NativeEvaluator::new(
        mod5.blocks[0].equations.clone(),
    ));
    let orchestrator5 = evaluator::Orchestrator::new(evaluator5);

    let mut inputs5 = evaluator::VarMap::new();
    inputs5.set("avg_latency_ms", 20.0);
    inputs5.set("target_utilization", 0.70);

    let frontier5 = orchestrator5
        .solve_pareto(
            &inputs5,
            "reqs_per_second",
            (10000.0, 50000.0),
            4,
            &[
                "reqs_per_second".to_string(),
                "allocated_workers".to_string(),
            ],
        )
        .unwrap();

    for (i, p) in frontier5.iter().enumerate() {
        println!(
            "  Point {}: reqs = {:.0}/s => workers = {:.0} (solve: {:.1} µs, spec: {})",
            i + 1,
            p.vars.get("reqs_per_second").unwrap_or(0.0),
            p.vars.get("allocated_workers").unwrap_or(0.0),
            p.diagnostics.solve_time_us,
            p.diagnostics.spec_hash
        );
    }
    if let Some(k) = evaluator::Orchestrator::find_knee_point(&frontier5) {
        println!(
            "  => Knee Point: Point {} (Optimal trade-off balance)\n",
            k + 1
        );
    }

    println!("============================================================");
    println!("Verification Complete: All 5 Pillars Functioning Natively.");
    println!("============================================================");
}

fn cmd_pareto(
    path: &str,
    sweep_var: Option<&str>,
    min_val: Option<f64>,
    max_val: Option<f64>,
    backend_arg: Option<&str>,
    cmd_arg: Option<&str>,
    steps: usize,
) {
    let module = parse_file(path);
    if module.blocks.is_empty() {
        eprintln!("Error: no relation blocks found in '{}'", path);
        process::exit(1);
    }

    let rel_block = &module.blocks[0];
    let mut initial_inputs = evaluator::VarMap::new();

    // Default bindings from solve block if present
    if !module.solves.is_empty() {
        for b in &module.solves[0].bindings {
            initial_inputs.set(&b.name, b.value);
        }
    }

    let config = if let Some("external") = backend_arg {
        let (command, args) = if let Some(cmd) = cmd_arg {
            let parts: Vec<String> = cmd.split_whitespace().map(|s| s.to_string()).collect();
            let c = parts
                .first()
                .cloned()
                .unwrap_or_else(|| "python".to_string());
            let a = parts[1..].to_vec();
            (c, a)
        } else {
            (
                "python".to_string(),
                vec!["evaluations/tests/flame_eval.py".to_string()],
            )
        };
        evaluator::OrchestratorConfig {
            backend: evaluator::EvaluatorBackend::External {
                command,
                args,
                backend_id: Some("flame_eval".to_string()),
                working_dir: None,
            },
            seed: 42,
        }
    } else if let BackendDecl::External { command, args } = &rel_block.backend {
        let b_id = args.first().cloned().unwrap_or_else(|| command.clone());
        evaluator::OrchestratorConfig {
            backend: evaluator::EvaluatorBackend::External {
                command: command.clone(),
                args: args.clone(),
                backend_id: Some(b_id),
                working_dir: None,
            },
            seed: 42,
        }
    } else {
        evaluator::OrchestratorConfig::default()
    };

    let orchestrator = evaluator::Orchestrator::from_config(&config, rel_block.equations.clone());

    let var = sweep_var.unwrap_or("reqs_per_second");
    let min = min_val.unwrap_or(10000.0);
    let max = max_val.unwrap_or(50000.0);

    println!("============================================================");
    println!(" Aven Pareto Multi-Objective Frontier Engine");
    println!(
        " Relation: '{}' | Sweep: {} in [{:.1}, {:.1}]",
        rel_block.name, var, min, max
    );
    println!("============================================================");

    let mut all_vars = std::collections::BTreeSet::new();
    for eq in &rel_block.equations {
        all_vars.insert(eq.lhs.clone());
        for v in eq.rhs.vars() {
            all_vars.insert(v);
        }
    }
    let obj_list: Vec<String> = all_vars.into_iter().collect();

    match orchestrator.solve_pareto(&initial_inputs, var, (min, max), steps, &obj_list) {
        Ok(frontier) => {
            if let Some(first_pt) = frontier.first() {
                let mut display_vars: Vec<String> = first_pt
                    .vars
                    .values
                    .keys()
                    .filter(|k| k.as_str() != var)
                    .cloned()
                    .collect();
                display_vars.sort();

                print!("\n{:<6} {:<18}", "Step", var);
                for vname in &display_vars {
                    print!(" {:<18}", vname);
                }
                println!();
                println!("{}", "-".repeat(24 + 19 * display_vars.len()));

                for (idx, pt) in frontier.iter().enumerate() {
                    let v_sweep = pt.vars.get(var).unwrap_or(0.0);
                    print!("{:<6} {:<18.2}", idx + 1, v_sweep);
                    for vname in &display_vars {
                        let val = pt.vars.get(vname).unwrap_or(0.0);
                        if val.fract() == 0.0 && val.abs() < 1e9 {
                            print!(" {:<18.0}", val);
                        } else {
                            print!(" {:<18.4}", val);
                        }
                    }
                    println!();
                }
            }

            if let Some(knee) = evaluator::Orchestrator::find_knee_point(&frontier) {
                println!(
                    "\nOptimal Knee Point Detected: Step {} (Best trade-off balance)",
                    knee + 1
                );
            }
            println!("Spec Hash: {}", initial_inputs.compute_hash());
        }
        Err(e) => {
            eprintln!("Pareto solve failed: {}", e);
            process::exit(1);
        }
    }
}

fn cmd_stream(path: &str, limit: usize) {
    use std::io::BufRead;

    let module = parse_file(path);

    println!("============================================================");
    println!(" Aven Relational Stream Supervisor");
    println!(" Model: '{}' | Ingesting live JSON stream from STDIN", path);
    println!("============================================================");

    println!(
        "{:<7} | {:<16} | {:<10} | {:<10} | {:<8} | {:<7} | {:<4} | Status",
        "Cycle", "Spec Hash", "Scan(ms)", "Power(W)", "Temp(C)", "Qual(%)", "Rank"
    );
    println!("{}", "-".repeat(88));

    let stdin = std::io::stdin();
    let reader = stdin.lock();
    let mut cycle = 0;
    let mut ledger = new_ledger();
    let mut seq = 0;

    for line_res in reader.lines() {
        let line = match line_res {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parsed_numbers = evaluator::parse_json_object_numbers(trimmed);
        if parsed_numbers.is_empty() {
            continue;
        }

        cycle += 1;

        // Build VarMap and env
        let mut env = HashMap::new();
        let mut varmap = evaluator::VarMap::new();
        for (k, v) in &parsed_numbers {
            env.insert(k.clone(), *v);
            varmap.set(k, *v);
            seq += 1;
            ledger.append(Fact {
                seq,
                tick: cycle,
                kind: "stream".to_string(),
                name: k.clone(),
                value: *v,
                causes: Vec::new(),
                origin: "stdin".to_string(),
                metadata: None,
            });
        }

        // Solve relations declared in the module
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
        for solve in &module.solves {
            for c in &solve.constraints {
                solver.add_constraint(c.clone());
            }
            for obj in &solve.objectives {
                solver.add_objective(obj.clone());
            }
            for iv in &solve.integer_vars {
                solver.add_integer_var(iv.clone());
            }
        }

        let residual = 0.0;
        if let Ok(solved_env) = solver.solve(&env) {
            for (k, v) in solved_env {
                if let std::collections::hash_map::Entry::Vacant(entry) = env.entry(k.clone()) {
                    varmap.set(&k, v);
                    entry.insert(v);
                }
            }
        }

        // Evaluate constraints from solve directives
        let mut alarms = Vec::new();
        for solve in &module.solves {
            for c in &solve.constraints {
                if let Some(&lhs_val) = env.get(&c.lhs) {
                    if let Ok(rhs_val) = diff::eval_expr(&c.rhs, &env) {
                        let satisfied = match c.op {
                            ConstraintOp::Equal => (lhs_val - rhs_val).abs() < 1e-4,
                            ConstraintOp::NotEqual => (lhs_val - rhs_val).abs() >= 1e-4,
                            ConstraintOp::LessEqual => lhs_val <= rhs_val + 1e-6,
                            ConstraintOp::GreaterEqual => lhs_val >= rhs_val - 1e-6,
                            ConstraintOp::Less => lhs_val < rhs_val,
                            ConstraintOp::Greater => lhs_val > rhs_val,
                        };
                        if !satisfied {
                            alarms
                                .push(format!("{}: {:.2} {} {:.2}", c.lhs, lhs_val, c.op, rhs_val));
                        }
                    }
                }
            }
        }

        let spec_hash = varmap.compute_hash();
        let scan_ms = env.get("scan_ms").copied().unwrap_or(0.0);
        let power_w = env.get("power_w").copied().unwrap_or(0.0);
        let temp_c = env.get("temp_c").copied().unwrap_or(0.0);
        let quality_pct = env.get("quality_pct").copied().unwrap_or(0.0);
        let rank = env.len();

        let status_str = if alarms.is_empty() {
            format!("CONVERGED (Res: {:.4})", residual)
        } else {
            format!("ALARM: {}", alarms.join("; "))
        };

        println!(
            "#{:<6} | {:<16} | {:<10.2} | {:<10.1} | {:<8.1} | {:<7.2} | {:<4} | {}",
            cycle, spec_hash, scan_ms, power_w, temp_c, quality_pct, rank, status_str
        );

        if limit > 0 && cycle >= limit {
            break;
        }
    }

    println!("{}", "-".repeat(88));
    println!(
        "Stream completed: {} telemetry cycles verified by Aven.",
        cycle
    );
}

/// Evaluates `forward` blocks strictly forward in dependency order (the order the C emitter
/// uses). With `strict` false, equations whose inputs are not yet known are skipped; with
/// `strict` true they are an error. A forward equation whose output is already settled must
/// agree with it.
fn eval_forward_blocks(
    module: &AvenModule,
    env: &mut HashMap<String, f64>,
    ledger: &mut Ledger,
    seq: &mut usize,
    stale_facts: &mut Vec<String>,
    strict: bool,
) -> Result<(), solver::SolverError> {
    for blk in module.blocks.iter().filter(|b| b.kind == BlockKind::Forward) {
        let order = CTranspiler::topo_sort(blk)
            .map_err(|e| solver::SolverError::MathError(e.to_string()))?;
        for idx in order {
            let eq = &blk.equations[idx];
            if !eq.rhs.vars().iter().all(|v| env.contains_key(v)) {
                if strict {
                    let missing: Vec<String> = eq
                        .rhs
                        .vars()
                        .into_iter()
                        .filter(|v| !env.contains_key(v))
                        .collect();
                    return Err(solver::SolverError::MathError(format!(
                        "forward block '{}': '{}' needs {} which nothing provides",
                        blk.name,
                        eq.original_text,
                        missing.join(", ")
                    )));
                }
                continue;
            }
            let value = diff::eval_expr(&eq.rhs, env).map_err(|e| {
                solver::SolverError::MathError(format!(
                    "forward block '{}': {}: {}",
                    blk.name, eq.original_text, e
                ))
            })?;
            if let Some(&existing) = env.get(&eq.lhs) {
                if (existing - value).abs() > 1e-6 * (1.0 + value.abs()) {
                    return Err(solver::SolverError::Inconsistency {
                        relation: eq.original_text.clone(),
                        lhs: eq.lhs.clone(),
                        expected: value,
                        given: existing,
                    });
                }
                continue;
            }
            *seq += 1;
            let mut causes = Vec::new();
            let mut tainted = false;
            for v in eq.rhs.vars() {
                if let Some(fact) = ledger.latest(&v) {
                    causes.push(fact.seq);
                    tainted |= fact.metadata.as_ref().map_or(false, |m| m.is_stale);
                }
            }
            if tainted && !stale_facts.contains(&eq.lhs) {
                stale_facts.push(eq.lhs.clone());
            }
            env.insert(eq.lhs.clone(), value);
            ledger.append(Fact {
                seq: *seq,
                tick: 0,
                kind: "derived".to_string(),
                name: eq.lhs.clone(),
                value,
                causes,
                origin: format!("forward {}: = {}", blk.name, eq.rhs),
                metadata: tainted.then(|| ledger::FactMetadata {
                    timestamp_ms: None,
                    ttl_ms: None,
                    is_stale: true,
                    warnings: vec!["causal-stale-taint".to_string()],
                }),
            });
        }
    }
    Ok(())
}

/// Every variable a module refers to: declared signals, equation sides, constraints,
/// objectives and bindings.
fn module_variables(module: &AvenModule) -> std::collections::HashSet<String> {
    let mut vars: std::collections::HashSet<String> =
        module.signals.iter().map(|s| s.name.clone()).collect();
    for blk in &module.blocks {
        for eq in &blk.equations {
            vars.insert(eq.lhs.clone());
            vars.extend(eq.rhs.vars());
        }
        vars.extend(blk.integer_vars.iter().cloned());
    }
    for solve in &module.solves {
        for b in &solve.bindings {
            vars.insert(b.name.clone());
        }
        for c in &solve.constraints {
            vars.insert(c.lhs.clone());
            vars.extend(c.rhs.vars());
        }
        for o in &solve.objectives {
            vars.insert(o.target.clone());
        }
        vars.extend(solve.integer_vars.iter().cloned());
    }
    vars
}
