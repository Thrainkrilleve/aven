// src/main.rs — CLI entry point for avenc (The Aven Language Toolchain)
#![allow(
    clippy::needless_range_loop,
    clippy::collapsible_if,
    clippy::should_implement_trait,
    clippy::unnecessary_map_or,
    clippy::too_many_arguments
)]

mod ast;
mod codegen_c;
mod diagnostics;
mod diff;
mod evaluator;
mod ledger;
mod lexer;
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
    println!("  avenc run <file.aven>            Execute .aven file and print settled relations");
    println!(
        "  avenc pareto <file.aven> [var] [min] [max] [--backend native|external] [--cmd <command>]"
    );
    println!(
        "                                   Generate Pareto trade-off frontier & detect knee point"
    );
    println!("  avenc why <file.aven> <var>      Show causal provenance derivation tree for <var>");
    println!("  avenc build <file.aven> [--emit-c] [-o out.c] [--traceable] [--checked-c]");
    println!("                                   Transpile pinned relations to standalone C");
    println!(
        "  avenc check <file.aven>          Validate syntax, relations, and unit declarations"
    );
    println!("  avenc demo                       Run built-in multi-pillar verification test");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return;
    }

    match args[1].as_str() {
        "run" => {
            if args.len() < 3 {
                eprintln!("Error: missing file argument for 'run'");
                process::exit(1);
            }
            cmd_run(&args[2]);
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
            cmd_pareto(&args[2], sweep_var, min_val, max_val, backend_arg, cmd_arg);
        }
        "why" => {
            if args.len() < 4 {
                eprintln!(
                    "Error: 'why' requires a file and variable name: avenc why <file.aven> <variable>"
                );
                process::exit(1);
            }
            cmd_why(&args[2], &args[3]);
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
    println!("Check OK: '{}' parsed successfully.", path);
    println!("  Blocks:   {}", module.blocks.len());
    println!("  Solves:   {}", module.solves.len());
    println!("  Signals:  {}", module.signals.len());
    println!("  Units:    {}", module.units.len());
}

fn cmd_run(path: &str) {
    let module = parse_file(path);
    let (env, ledger, _) = execute_module(&module, true);

    println!("\nSettled Environment:");
    let mut sorted_keys: Vec<&String> = env.keys().collect();
    sorted_keys.sort();
    for k in sorted_keys {
        println!("  {:<16} = {:.4}", k, env[k]);
    }

    if !ledger.facts.is_empty() {
        println!("\nFact Ledger: {} facts settled.", ledger.facts.len());
    }
}

fn cmd_why(path: &str, var: &str) {
    let module = parse_file(path);
    let (_, ledger, _) = execute_module(&module, false);
    println!("Causal Derivation Tree for '{}':", var);
    println!("{}", ledger.why(var));
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

    let mut bindings = HashMap::new();
    if let Some(solve) = module.solves.iter().find(|s| s.target == blk.name) {
        for b in &solve.bindings {
            bindings.insert(b.name.clone(), b.value);
        }
    } else {
        // Fall back to running solver on the module to auto-fill inputs from settled relations
        let (env, _, _) = execute_module(&module, false);
        for (k, v) in env {
            bindings.insert(k, v);
        }
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

fn execute_module(
    module: &AvenModule,
    verbose: bool,
) -> (HashMap<String, f64>, Ledger, Option<diagnostics::Diagnosis>) {
    let mut env = HashMap::new();
    let mut ledger = Ledger::new();
    let mut seq = 0;

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
            });
        }
    }

    // Execute external evaluators if declared via 'using external(...)'
    for blk in &module.blocks {
        if let BackendDecl::External { command, args } = &blk.backend {
            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let ext_eval = evaluator::ExternalEvaluator::new(command, &arg_refs);
            let mut varmap = evaluator::VarMap::new();
            for (k, v) in &env {
                varmap.set(k, *v);
            }
            if verbose {
                println!(
                    "Executing external evaluator '{}' for relation '{}'...",
                    command, blk.name
                );
            }
            match ext_eval.evaluate(&varmap) {
                Ok(res) => {
                    if verbose {
                        println!(
                            "  [external] {} settled in {:.1} µs (backend: {})",
                            blk.name, res.solve_time_us, res.backend_id
                        );
                    }
                    for (k, v) in res.values {
                        if !env.contains_key(&k) {
                            seq += 1;
                            env.insert(k.clone(), v);
                            ledger.append(Fact {
                                seq,
                                tick: 0,
                                kind: "derived".to_string(),
                                name: k,
                                value: v,
                                causes: Vec::new(),
                                origin: format!("external: {}", res.backend_id),
                            });
                        }
                    }
                }
                Err(e) => {
                    eprintln!("External evaluation failed for block '{}': {}", blk.name, e);
                }
            }
        }
    }

    // Set up solver engine (only load native Relation blocks, never duplicate with Forward or external blocks)
    let mut solver = SolverEngine::new();
    for blk in &module.blocks {
        if blk.kind == BlockKind::Relation && blk.backend == BackendDecl::Native {
            for eq in &blk.equations {
                solver.add_relation(eq.clone());
            }
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

            // Record derived facts into the ledger
            for (k, v) in &solved_env {
                if !env.contains_key(k) {
                    seq += 1;
                    // Find causes: which equations defined this?
                    let mut causes = Vec::new();
                    let mut origin = "derived".to_string();
                    if let Some(eq) = solver.relations.iter().find(|e| &e.lhs == k) {
                        origin = format!("= {}", eq.rhs);
                        for input_var in eq.rhs.vars() {
                            if let Some(fact) = ledger.latest(&input_var) {
                                causes.push(fact.seq);
                            }
                        }
                    }

                    ledger.append(Fact {
                        seq,
                        tick: 0,
                        kind: "derived".to_string(),
                        name: k.clone(),
                        value: *v,
                        causes,
                        origin,
                    });
                }
            }

            // Check if any unknowns remain
            let mut all_vars = std::collections::BTreeSet::new();
            for eq in &solver.relations {
                all_vars.insert(eq.lhs.clone());
                eq.rhs.collect_vars(&mut all_vars);
            }
            let remaining_unknowns: Vec<String> = all_vars
                .into_iter()
                .filter(|v| !solved_env.contains_key(v))
                .collect();

            let diag = if !remaining_unknowns.is_empty() {
                let blocked: Vec<Equation> = solver
                    .relations
                    .iter()
                    .filter(|eq| {
                        let mut v = eq.rhs.vars();
                        v.insert(eq.lhs.clone());
                        v.iter().any(|var| remaining_unknowns.contains(var))
                    })
                    .cloned()
                    .collect();

                let d = Diagnostician::new().diagnose(&remaining_unknowns, &blocked, &solved_env);
                if verbose {
                    println!("\n{}", d.report());
                }
                Some(d)
            } else {
                None
            };

            (solved_env, ledger, diag)
        }
        Err(e) => {
            eprintln!("\nSolver Error: {}", e);
            process::exit(1);
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
    let (env1, _, _) = execute_module(&mod1, true);
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
    let (env2, _, _) = execute_module(&mod2, true);
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
    let (_, ledger3, _) = execute_module(&mod3, false);
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

    match orchestrator.solve_pareto(&initial_inputs, var, (min, max), 5, &obj_list) {
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
