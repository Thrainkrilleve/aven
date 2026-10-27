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
mod smt;
mod temporal;
mod compaction;
mod debugger;
mod codegen_wasm;
mod package;

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
use std::io;
use std::process;

const VERSION: &str = "0.6.0";

fn print_usage() {
    println!("avenc v{} — The Aven Language Toolchain", VERSION);
    println!("Usage:");
    println!("Usage:");
    println!(
        "  avenc run <file.aven> [--allow-stale] [--json] [--ledger out.jsonl] [--key <KEY> | --random-key]"
    );
    println!(
        "                                        Execute .aven file and print settled relations"
    );
    println!(
        "                                        (enforces fact freshness; blocks decision on stale/zero-TTL data)"
    );
    println!(
        "  avenc pareto <file.aven> <var> <min> <max> [--steps N] [--backend native|external] [--cmd <command>]"
    );
    println!(
        "                                        Sweep a variable, tabulate every output, and mark the knee point"
    );
    println!("  avenc stream <file.aven> [--limit N]");
    println!(
        "                                        Ingest live JSON stream from stdin & supervise relations"
    );
    println!("  avenc why <file.aven> <var> [--depth N]");
    println!(
        "                                        Show causal derivation tree (default depth: 20; 0 for unlimited)"
    );
    println!("  avenc emit-c <file.aven> [-o out.c] [--traceable] [--checked-c] [--no-main] [--aeric-shim] [--out-dir <dir>]");
    println!("                                        Transpile the forward block to standalone C (same as build --emit-c)");
    println!("  avenc build <file.aven> [--emit-c] [-o out.c] [--traceable] [--checked-c] [--no-main] [--aeric-shim] [--out-dir <dir>]");
    println!("                                        Transpile pinned relations to standalone C or AERIC deployment bundle");
    println!("  avenc emit-smt <file.aven> [-o out.smt2] [--prove] [--logic <LOGIC>]");
    println!("                                        Transpile relation/forward blocks to SMT-LIB2 format for Z3/CVC5");
    println!("  avenc verify-smt <file.aven> [--prove] [--solver <CMD>]");
    println!("                                        Formally verify safety invariants or check satisfiability via SMT");
    println!("  avenc safety-case <file.aven> [-o report.md] [--format md|json] [--standard all|do178c|iso26262] [--invariant \"<expr>\"]");
    println!("                                        Generate formal safety case report for DO-178C (Table A-5) & ISO 26262 ASIL-D");
    println!("  avenc bench <file.aven> [--iterations N] [--latency-profile] [--target-hz HZ] [--compiled] [--json] [--markdown]");
    println!("                                        Benchmark solve performance, WCET single-tick latency, and cycle jitter");
    println!(
        "  avenc check <file.aven>               Validate syntax, relations, and unit declarations"
    );
    println!("  avenc verify-ledger <out.jsonl> [--key <KEY>]");
    println!("                                        Check an audit log's hash chain and HMAC-SHA256 signatures");
    println!("  avenc compact-ledger <in.jsonl> -o <out.jsonl>");
    println!("                                        Compact settled historical audit log to bound storage footprint");
    println!("  avenc debug <file.aven> [ledger.jsonl] [--goto <N>] [--what-if \"<var>=<val>\"] [--diff <t1>,<t2>]");
    println!("                                        Interactive time-travel debugger & counterfactual branch explorer");
  println!("  avenc emit-wasm <file.aven> [-o out.wasm] [--wat]");
    println!("                                        Transpile relation/forward blocks to WebAssembly (.wasm / .wat)");
    println!("  avenc pkg <init|pack|install|list>    Package manager: init, pack, install, and list unit-verified packages");
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
            let key_arg = args
                .windows(2)
                .find(|w| w[0] == "--key" || w[0] == "--ledger-key")
                .map(|w| w[1].clone());
            let file_arg = args
                .iter()
                .skip(2)
                .find(|a| !a.starts_with("--") && Some(*a) != key_arg.as_ref());
            let Some(file) = file_arg else {
                eprintln!("Error: missing file argument for 'verify-ledger': avenc verify-ledger <out.jsonl> [--key <KEY>]");
                process::exit(1);
            };
            cmd_verify_ledger(file, key_arg.as_deref());
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
                aeric_shim: args.iter().any(|a| a == "--aeric-shim"),
                no_main: args.iter().any(|a| a == "--no-main"),
            };
            let out_file = args.windows(2).find(|w| w[0] == "-o").map(|w| w[1].as_str());
            let out_dir = args.windows(2).find(|w| w[0] == "--out-dir").map(|w| w[1].as_str());
            cmd_build(file, true, out_file, out_dir, &options);
        }
        "emit-smt" => {
            let file_arg = args.iter().skip(2).find(|a| !a.starts_with('-'));
            let Some(file) = file_arg else {
                eprintln!("Error: missing file argument for 'emit-smt': avenc emit-smt <file.aven> [-o out.smt2] [--prove]");
                process::exit(1);
            };
            let prove = args.iter().any(|a| a == "--prove");
            let logic = args
                .windows(2)
                .find(|w| w[0] == "--logic")
                .map(|w| w[1].clone());
            let prove_inv = args
                .windows(2)
                .find(|w| w[0] == "--prove-invariant" || w[0] == "--invariant")
                .map(|w| w[1].clone());
            let out_file = args
                .windows(2)
                .find(|w| w[0] == "-o")
                .map(|w| w[1].as_str());
            let options = smt::SmtOptions {
                mode: if prove {
                    smt::SmtMode::Prove
                } else {
                    smt::SmtMode::Satisfy
                },
                logic,
                prove_invariant: prove_inv,
                block_name: None,
            };
            cmd_emit_smt(file, out_file, &options);
        }
        "verify-smt" => {
            let file_arg = args.iter().skip(2).find(|a| !a.starts_with('-'));
            let Some(file) = file_arg else {
                eprintln!("Error: missing file argument for 'verify-smt': avenc verify-smt <file.aven> [--prove] [--solver <CMD>]");
                process::exit(1);
            };
            let prove = args.iter().any(|a| a == "--prove");
            let solver = args
                .windows(2)
                .find(|w| w[0] == "--solver")
                .map(|w| w[1].as_str());
            let logic = args
                .windows(2)
                .find(|w| w[0] == "--logic")
                .map(|w| w[1].clone());
            let prove_inv = args
                .windows(2)
                .find(|w| w[0] == "--prove-invariant" || w[0] == "--invariant")
                .map(|w| w[1].clone());
            let options = smt::SmtOptions {
                mode: if prove {
                    smt::SmtMode::Prove
                } else {
                    smt::SmtMode::Satisfy
                },
                logic,
                prove_invariant: prove_inv,
                block_name: None,
            };
            cmd_verify_smt(file, solver, &options);
        }
        "safety-case" => {
            let file_arg = args.iter().skip(2).find(|a| !a.starts_with('-'));
            let Some(file) = file_arg else {
                eprintln!("Error: missing file argument for 'safety-case': avenc safety-case <file.aven> [-o report.md]");
                process::exit(1);
            };
            let out_file = args
                .windows(2)
                .find(|w| w[0] == "-o" || w[0] == "--out")
                .map(|w| w[1].as_str());
            let format = args
                .windows(2)
                .find(|w| w[0] == "--format")
                .map(|w| w[1].as_str())
                .unwrap_or("md");
            let standard_str = args
                .windows(2)
                .find(|w| w[0] == "--standard")
                .map(|w| w[1].as_str())
                .unwrap_or("all");
            let standard = match standard_str {
                "do178c" => smt::SafetyStandard::Do178c,
                "iso26262" => smt::SafetyStandard::Iso26262,
                _ => smt::SafetyStandard::All,
            };
            let solver = args
                .windows(2)
                .find(|w| w[0] == "--solver")
                .map(|w| w[1].as_str());
            let mut invariants = Vec::new();
            for w in args.windows(2) {
                if w[0] == "--invariant" || w[0] == "--prove-invariant" {
                    invariants.push(w[1].clone());
                }
            }
            cmd_safety_case(file, out_file, format, standard, &invariants, solver);
        }
        "bench" => {
            let file_arg = args.iter().skip(2).find(|a| !a.starts_with('-'));
            let Some(file) = file_arg else {
                eprintln!("Error: missing file argument for 'bench'");
                process::exit(1);
            };
            let iterations = args
                .windows(2)
                .find(|w| w[0] == "--iterations" || w[0] == "-n")
                .and_then(|w| w[1].parse::<usize>().ok());
            let latency_profile = args.iter().any(|a| a == "--latency-profile" || a == "--profile");
            let target_hz = args
                .windows(2)
                .find(|w| w[0] == "--target-hz" || w[0] == "--hz")
                .and_then(|w| w[1].parse::<f64>().ok());
            let compiled = args.iter().any(|a| a == "--compiled" || a == "--aot");
            let json = args.iter().any(|a| a == "--json");
            let markdown = args.iter().any(|a| a == "--markdown" || a == "--md");

            if latency_profile {
                let n = iterations.unwrap_or(10_000).max(10);
                cmd_bench_latency_profile(file, n, target_hz, compiled, json, markdown);
            } else {
                let n = iterations.unwrap_or(200).max(1);
                cmd_bench(file, n);
            }
        }
        "debug" => {
            let mut model_file = None;
            let mut ledger_file = None;
            let mut goto_tick = None;
            let mut what_if = None;
            let mut diff_ticks = None;

            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--goto" | "-t" => {
                        if i + 1 < args.len() {
                            goto_tick = args[i + 1].parse::<usize>().ok();
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                    "--what-if" => {
                        if i + 1 < args.len() {
                            what_if = Some(args[i + 1].clone());
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                    "--diff" | "--diff-ticks" => {
                        if i + 1 < args.len() {
                            let parts: Vec<&str> = args[i + 1].split(',').collect();
                            if parts.len() == 2 {
                                if let (Ok(t1), Ok(t2)) = (
                                    parts[0].trim().parse::<usize>(),
                                    parts[1].trim().parse::<usize>(),
                                ) {
                                    diff_ticks = Some((t1, t2));
                                }
                            }
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                    arg if !arg.starts_with('-') => {
                        if model_file.is_none() {
                            model_file = Some(arg.to_string());
                        } else if ledger_file.is_none() {
                            ledger_file = Some(arg.to_string());
                        }
                        i += 1;
                    }
                    _ => {
                        i += 1;
                    }
                }
            }

            let Some(model) = model_file else {
                eprintln!(
                    "Error: missing file argument for 'debug': avenc debug <file.aven> [ledger.jsonl]"
                );
                process::exit(1);
            };
            cmd_debug(
                &model,
                ledger_file.as_deref(),
                goto_tick,
                what_if.as_deref(),
                diff_ticks,
            );
        }
        "compact-ledger" => {
            let mut in_path = None;
            let mut out_path = None;

            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "-o" | "--out" => {
                        if i + 1 < args.len() {
                            out_path = Some(args[i + 1].clone());
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                    arg if !arg.starts_with('-') => {
                        if in_path.is_none() {
                            in_path = Some(arg.to_string());
                        } else if out_path.is_none() {
                            out_path = Some(arg.to_string());
                        }
                        i += 1;
                    }
                    _ => {
                        i += 1;
                    }
                }
            }

            let Some(in_file) = in_path else {
                eprintln!(
                    "Error: missing input file: avenc compact-ledger <in.jsonl> [-o] <out.jsonl>"
                );
                process::exit(1);
            };
            let Some(out_file) = out_path else {
                eprintln!(
                    "Error: missing output file: avenc compact-ledger <in.jsonl> [-o] <out.jsonl>"
                );
                process::exit(1);
            };
            cmd_compact_ledger(&in_file, &out_file);
        }
        "emit-wasm" => {
            let mut file_arg = None;
            let mut out_file = None;
            let mut emit_wat = false;

            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "-o" | "--out" => {
                        if i + 1 < args.len() {
                            out_file = Some(args[i + 1].clone());
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                    "--wat" => {
                        emit_wat = true;
                        i += 1;
                    }
                    arg if !arg.starts_with('-') => {
                        if file_arg.is_none() {
                            file_arg = Some(arg.to_string());
                        }
                        i += 1;
                    }
                    _ => {
                        i += 1;
                    }
                }
            }

            let Some(file) = file_arg else {
                eprintln!(
                    "Error: missing file argument for 'emit-wasm': avenc emit-wasm <file.aven> [-o out.wasm] [--wat]"
                );
                process::exit(1);
            };
            cmd_emit_wasm(&file, out_file.as_deref(), emit_wat);
        }
        "pkg" => {
            let subcmd = args.get(2).map(|s| s.as_str());
            match subcmd {
                Some("init") => {
                    let name = args.get(3).map(|s| s.as_str()).unwrap_or("my_package");
                    cmd_pkg_init(name);
                }
                Some("pack") => {
                    let dir = args.get(3).map(|s| s.as_str()).unwrap_or(".");
                    let out = args
                        .windows(2)
                        .find(|w| w[0] == "-o" || w[0] == "--out")
                        .map(|w| w[1].as_str());
                    cmd_pkg_pack(dir, out);
                }
                Some("install") => {
                    let pkg_file = args.get(3).map(|s| s.as_str());
                    let Some(p) = pkg_file else {
                        eprintln!(
                            "Error: missing package file for 'pkg install': avenc pkg install <package.avenpkg>"
                        );
                        process::exit(1);
                    };
                    cmd_pkg_install(p);
                }
                Some("list") => {
                    cmd_pkg_list();
                }
                _ => {
                    eprintln!("Usage: avenc pkg <init|pack|install|list>");
                    eprintln!("  avenc pkg init [name]            Initialize a new Aven package");
                    eprintln!("  avenc pkg pack [dir] [-o out]     Pack package into .avenpkg archive");
                    eprintln!("  avenc pkg install <file.avenpkg> Install package into local cache");
                    eprintln!("  avenc pkg list                   List installed packages");
                    process::exit(1);
                }
            }
        }
        "run" => {
            let allow_stale = args.iter().any(|a| a == "--allow-stale");
            let json = args.iter().any(|a| a == "--json");
            let random_key = args.iter().any(|a| a == "--random-key");
            let key_arg = args
                .windows(2)
                .find(|w| w[0] == "--key" || w[0] == "--ledger-key")
                .map(|w| w[1].clone());
            let ledger_out = args
                .windows(2)
                .find(|w| w[0] == "--ledger")
                .map(|w| w[1].clone());
            let file_arg = args
                .iter()
                .skip(2)
                .find(|a| !a.starts_with("--") && Some(*a) != ledger_out.as_ref() && Some(*a) != key_arg.as_ref());
            if file_arg.is_none() {
                eprintln!("Error: missing file argument for 'run'");
                process::exit(1);
            }
            cmd_run(
                file_arg.unwrap(),
                allow_stale,
                json,
                ledger_out.as_deref(),
                key_arg.as_deref(),
                random_key,
            );
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
            let positional: Vec<&String> =
                args.iter().skip(3).take(3).take_while(|s| !s.starts_with("--")).collect();
            let (sweep_var, min_val, max_val) = match positional.as_slice() {
                [v, lo, hi] => match (lo.parse::<f64>(), hi.parse::<f64>()) {
                    (Ok(lo), Ok(hi)) if lo < hi => (v.as_str(), lo, hi),
                    _ => {
                        eprintln!("Error: pareto range must be two numbers with min < max, got '{}' '{}'", lo, hi);
                        process::exit(1);
                    }
                },
                _ => {
                    eprintln!("Usage: avenc pareto <file.aven> <var> <min> <max> [--steps N] [--backend native|external] [--cmd <command>]");
                    process::exit(1);
                }
            };
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
            let aeric_shim = args.iter().any(|a| a == "--aeric-shim");
            let no_main = args.iter().any(|a| a == "--no-main");
            let out_file = args
                .windows(2)
                .find(|w| w[0] == "-o")
                .map(|w| w[1].as_str());
            let out_dir = args
                .windows(2)
                .find(|w| w[0] == "--out-dir")
                .map(|w| w[1].as_str());
            let options = codegen_c::CodegenOptions {
                traceable,
                checked_c,
                aeric_shim,
                no_main,
            };
            cmd_build(&args[2], emit_c, out_file, out_dir, &options);
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

    let parent_dir = std::path::Path::new(path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let search_dirs = [
        parent_dir,
        std::path::PathBuf::from("vendor"),
        std::path::PathBuf::from(".aven/packages"),
    ];

    let resolved_content = match package::PackageManager::resolve_imports(&content, &search_dirs) {
        Ok(rc) => rc,
        Err(e) => {
            eprintln!("Import resolution error: {}", e);
            process::exit(1);
        }
    };

    let mut lexer = Lexer::new(&resolved_content);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    let mut parser = Parser::new(tokens);
    let mut module = match parser.parse_module(path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };
    // Every command works in base units; see units::UnitScale.
    if let Err(e) = units::normalize_bindings(&mut module) {
        eprintln!("{}", e);
        process::exit(1);
    }
    module
}

/// The directory adapters run in: the model file's directory. A bare file name has an
/// empty parent, which means the current directory (None), not a directory named "".
fn model_dir(path: &str) -> Option<&std::path::Path> {
    std::path::Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
}

/// The module's unit scale. parse_file has already validated every declared unit.
fn unit_scale(module: &AvenModule) -> units::UnitScale {
    units::UnitScale::for_module(module).unwrap_or_default()
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

fn cmd_run(
    path: &str,
    allow_stale: bool,
    json: bool,
    ledger_out: Option<&str>,
    cli_key: Option<&str>,
    random_key: bool,
) {
    let module = parse_file(path);
    let work_dir = model_dir(path);

    let mut generated_key_hex: Option<String> = None;
    let signing_key: Option<Vec<u8>> = if let Some(k) = cli_key {
        if k == "random" {
            let rand_k = ledger::generate_random_key();
            let bytes = rand_k.as_bytes().to_vec();
            generated_key_hex = Some(rand_k);
            Some(bytes)
        } else {
            Some(k.as_bytes().to_vec())
        }
    } else if random_key {
        let rand_k = ledger::generate_random_key();
        let bytes = rand_k.as_bytes().to_vec();
        generated_key_hex = Some(rand_k);
        Some(bytes)
    } else if let Ok(env_k) = env::var("AVEN_LEDGER_KEY") {
        if env_k == "random" {
            let rand_k = ledger::generate_random_key();
            let bytes = rand_k.as_bytes().to_vec();
            generated_key_hex = Some(rand_k);
            Some(bytes)
        } else if !env_k.is_empty() {
            Some(env_k.into_bytes())
        } else {
            None
        }
    } else {
        None
    };

    let res = execute_module_with_key(&module, !json, work_dir, signing_key.as_deref());

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
            if let Some(ref k_hex) = generated_key_hex {
                println!("  Randomized verification key: {}", k_hex);
                println!("  To verify: avenc verify-ledger {} --key {}", out, k_hex);
            }
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
            "\n  Refusing to settle a decision on expired inputs."
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

    let scale = unit_scale(&module);
    let dims = units::DimensionalChecker::new()
        .check_module(&module)
        .map(|r| r.variable_dimensions)
        .unwrap_or_default();

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
        let ledger_key_field = match &generated_key_hex {
            Some(k) => format!(", \"ledger_key\": \"{}\"", k),
            None => String::new(),
        };
        // env values are in base units; "units" names the unit of each dimensioned one
        let unit_fields: Vec<String> = sorted_keys
            .iter()
            .filter_map(|k| {
                dims.get(*k)
                    .filter(|d| !d.is_dimensionless())
                    .map(|d| format!("\"{}\": \"{}\"", k, d))
            })
            .collect();
        println!(
            "{{\"status\": \"{}\", \"optimality\": {}, \"env\": {{{}}}, \"units\": {{{}}}{}}}",
            status,
            optimality,
            fields.join(", "),
            unit_fields.join(", "),
            ledger_key_field
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
            println!("  {:<16} = {}", k, show_value(&scale, &dims, k, res.env[k]));
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

fn cmd_verify_ledger(path: &str, cli_key: Option<&str>) {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error reading ledger '{}': {}", path, e);
            process::exit(EXIT_ERROR);
        }
    };
    let key = cli_key
        .filter(|k| !k.is_empty())
        .map(|k| k.as_bytes().to_vec())
        .or_else(ledger_key);
    match ledger::verify_jsonl(&text, key.as_deref()) {
        Ok(v) => {
            println!(
                "Ledger OK: {} fact(s), chain intact{}, head {}",
                v.facts,
                if v.signed {
                    ", every signature valid"
                } else {
                    " (signatures not checked: pass --key <code> or set AVEN_LEDGER_KEY)"
                },
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
    let work_dir = model_dir(path);

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

fn cmd_bench_latency_profile(
    path: &str,
    iterations: usize,
    target_hz: Option<f64>,
    _bench_compiled: bool,
    output_json: bool,
    output_markdown: bool,
) {
    let module = parse_file(path);
    let target_block = if let Some(fb) = module.blocks.iter().find(|b| b.kind == BlockKind::Forward) {
        fb
    } else if let Some(rb) = module.blocks.first() {
        rb
    } else {
        eprintln!("Error: model '{}' has no relation or forward blocks to profile", path);
        process::exit(1);
    };

    let order = match CTranspiler::topo_sort(target_block) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Error topologically ordering equations: {}", e);
            process::exit(1);
        }
    };

    // Prepare input environment from solve block
    let mut initial_env: HashMap<String, f64> = HashMap::new();
    if let Some(s) = module.solves.iter().find(|s| s.target == target_block.name || s.targets.contains(&target_block.name)).or_else(|| module.solves.first()) {
        for b in &s.bindings {
            initial_env.insert(b.name.clone(), b.value);
        }
    }

    // Warmup 100 ticks
    let mut warm_env = initial_env.clone();
    for _ in 0..100 {
        for &idx in &order {
            let eq = &target_block.equations[idx];
            let val = diff::eval_expr(&eq.rhs, &warm_env).unwrap_or(0.0);
            warm_env.insert(eq.lhs.clone(), val);
        }
    }

    // Measurement loop
    let mut durations_ns: Vec<u64> = Vec::with_capacity(iterations);
    let mut env = initial_env.clone();
    for _ in 0..iterations {
        let t0 = std::time::Instant::now();
        for &idx in &order {
            let eq = &target_block.equations[idx];
            let val = diff::eval_expr(&eq.rhs, &env).unwrap_or(0.0);
            env.insert(eq.lhs.clone(), val);
        }
        let dt = t0.elapsed();
        durations_ns.push(dt.as_nanos() as u64);
    }

    // Statistics calculation
    let mut sorted = durations_ns.clone();
    sorted.sort_unstable();

    let n = sorted.len();
    let min_ns = sorted[0];
    let p50_ns = sorted[((n - 1) as f64 * 0.50).round() as usize];
    let p90_ns = sorted[((n - 1) as f64 * 0.90).round() as usize];
    let p95_ns = sorted[((n - 1) as f64 * 0.95).round() as usize];
    let p99_ns = sorted[((n - 1) as f64 * 0.99).round() as usize];
    let p99_9_ns = sorted[((n - 1) as f64 * 0.999).round() as usize];
    let max_ns = sorted[n - 1]; // WCET

    let sum_ns: u64 = sorted.iter().sum();
    let mean_ns = sum_ns as f64 / n as f64;
    let variance: f64 = sorted.iter().map(|&x| (x as f64 - mean_ns).powi(2)).sum::<f64>() / n as f64;
    let std_dev_ns = variance.sqrt();
    let jitter_range_ns = max_ns.saturating_sub(min_ns);

    let target_deadline_ns = target_hz.map(|hz| (1e9 / hz).round() as u64);
    let deadline_violations = target_deadline_ns.map(|d| sorted.iter().filter(|&&x| x > d).count());

    if output_json {
        println!("{{");
        println!("  \"model\": \"{}\",", path.replace('\\', "/"));
        println!("  \"iterations\": {},", n);
        println!("  \"target_block\": \"{}\",", target_block.name);
        println!("  \"min_ns\": {},", min_ns);
        println!("  \"p50_ns\": {},", p50_ns);
        println!("  \"p90_ns\": {},", p90_ns);
        println!("  \"p95_ns\": {},", p95_ns);
        println!("  \"p99_ns\": {},", p99_ns);
        println!("  \"p99_9_ns\": {},", p99_9_ns);
        println!("  \"max_ns\": {},", max_ns);
        println!("  \"mean_ns\": {:.2},", mean_ns);
        println!("  \"std_dev_ns\": {:.2},", std_dev_ns);
        println!("  \"jitter_range_ns\": {},", jitter_range_ns);
        if let Some(d) = target_deadline_ns {
            println!("  \"target_deadline_ns\": {},", d);
            println!("  \"deadline_violations\": {},", deadline_violations.unwrap_or(0));
        }
        println!("  \"throughput_solves_per_sec\": {:.0}", 1e9 / mean_ns);
        println!("}}");
        return;
    }

    if output_markdown {
        println!("### Real-Time Tick Latency & Jitter Profile: `{}`", path);
        println!("- **Iterations:** {} | **Target Block:** `{}`", n, target_block.name);
        if let Some(hz) = target_hz {
            println!("- **Target Control Rate:** {:.1} Hz (Deadline: {:.2} µs)", hz, 1e6 / hz);
        }
        println!("\n| Metric | Latency (ns) | Latency (µs) | Definition |");
        println!("|---|---:|---:|---|");
        println!("| **Min** | {} | {:.3} | Best-case single-tick execution time |", min_ns, min_ns as f64 / 1000.0);
        println!("| **p50 (Median)** | {} | {:.3} | 50th percentile nominal cycle latency |", p50_ns, p50_ns as f64 / 1000.0);
        println!("| **p90** | {} | {:.3} | 90th percentile upper boundary |", p90_ns, p90_ns as f64 / 1000.0);
        println!("| **p95** | {} | {:.3} | 95th percentile upper boundary |", p95_ns, p95_ns as f64 / 1000.0);
        println!("| **p99** | {} | {:.3} | 99th percentile real-time latency |", p99_ns, p99_ns as f64 / 1000.0);
        println!("| **p99.9** | {} | {:.3} | 99.9th percentile tail latency |", p99_9_ns, p99_9_ns as f64 / 1000.0);
        println!("| **Max (WCET)** | {} | {:.3} | Worst-Case Execution Time across battery |", max_ns, max_ns as f64 / 1000.0);
        println!("| **Std Dev (σ)** | {:.1} | {:.3} | Latency dispersion / cycle jitter |", std_dev_ns, std_dev_ns / 1000.0);
        println!("| **Jitter Range** | {} | {:.3} | Max − Min cycle variance |", jitter_range_ns, jitter_range_ns as f64 / 1000.0);
        return;
    }

    // Default formatted console view
    println!("============================================================");
    println!(" Aven High-Resolution Real-Time Latency & Jitter Profiler");
    println!(" Model: '{}' | Iterations: {}", path, n);
    println!("============================================================");
    println!(" Target Block: '{}' ({} equations)", target_block.name, target_block.equations.len());
    if let Some(hz) = target_hz {
        let deadline = target_deadline_ns.unwrap();
        let violations = deadline_violations.unwrap();
        println!(" Target Rate: {:.1} Hz | Deadline: {} ns ({:.2} µs)", hz, deadline, deadline as f64 / 1000.0);
        if violations == 0 {
            println!(" Deadline Adherence: ✓ 100.0% (0 / {} violations)", n);
        } else {
            println!(" Deadline Adherence: ✗ {:.2}% ({} / {} violations)", (1.0 - violations as f64 / n as f64) * 100.0, violations, n);
        }
    }
    println!("------------------------------------------------------------");
    println!(" {:<20} {:>14} {:>14}", "Quantile", "Latency (ns)", "Latency (µs)");
    println!("------------------------------------------------------------");
    println!(" {:<20} {:>14} {:>14.3}", "Min (Best Case)", min_ns, min_ns as f64 / 1000.0);
    println!(" {:<20} {:>14} {:>14.3}", "p50 (Median)", p50_ns, p50_ns as f64 / 1000.0);
    println!(" {:<20} {:>14} {:>14.3}", "p90", p90_ns, p90_ns as f64 / 1000.0);
    println!(" {:<20} {:>14} {:>14.3}", "p95", p95_ns, p95_ns as f64 / 1000.0);
    println!(" {:<20} {:>14} {:>14.3}", "p99", p99_ns, p99_ns as f64 / 1000.0);
    println!(" {:<20} {:>14} {:>14.3}", "p99.9", p99_9_ns, p99_9_ns as f64 / 1000.0);
    println!(" {:<20} {:>14} {:>14.3}", "Max (WCET)", max_ns, max_ns as f64 / 1000.0);
    println!("------------------------------------------------------------");
    println!(" Mean (µ):           {:>14.1} ns ({:.3} µs)", mean_ns, mean_ns / 1000.0);
    println!(" Jitter (σ):         {:>14.1} ns ({:.3} µs)", std_dev_ns, std_dev_ns / 1000.0);
    println!(" Jitter Range:       {:>14} ns ({:.3} µs)", jitter_range_ns, jitter_range_ns as f64 / 1000.0);
    println!(" Throughput:         {:>14.0} solves/sec", 1e9 / mean_ns);
    println!("============================================================");
}

fn cmd_why(path: &str, var: &str, depth: Option<usize>) {
    let module = parse_file(path);
    let work_dir = model_dir(path);
    let res = execute_module(&module, false, work_dir);
    let scale = unit_scale(&module);
    let dims = units::DimensionalChecker::new()
        .check_module(&module)
        .map(|r| r.variable_dimensions)
        .unwrap_or_default();
    println!("Causal Derivation Tree for '{}':", var);
    println!(
        "{}",
        res.ledger
            .why_formatted(var, depth, &|name, v| show_value(&scale, &dims, name, v))
    );
    res.exit_on_error();
}

fn cmd_build(
    path: &str,
    emit_c: bool,
    out_file: Option<&str>,
    out_dir: Option<&str>,
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

    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("model");

    if options.aeric_shim {
        let bundle = match CTranspiler::generate_aeric_shim(blk, stem) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{}", e);
                process::exit(1);
            }
        };

        if let Some(dir) = out_dir {
            if let Err(e) = fs::create_dir_all(dir) {
                eprintln!("Error creating output directory '{}': {}", dir, e);
                process::exit(1);
            }
            let model_c_path = format!("{}/model_{}.c", dir, stem);
            let model_h_path = format!("{}/model_{}.h", dir, stem);
            let shim_c_path = format!("{}/aven_shim.c", dir);

            if let Err(e) = fs::write(&model_c_path, &bundle.model_c) {
                eprintln!("Error writing '{}': {}", model_c_path, e);
                process::exit(1);
            }
            if let Err(e) = fs::write(&model_h_path, &bundle.header_h) {
                eprintln!("Error writing '{}': {}", model_h_path, e);
                process::exit(1);
            }
            if let Err(e) = fs::write(&shim_c_path, &bundle.shim_c) {
                eprintln!("Error writing '{}': {}", shim_c_path, e);
                process::exit(1);
            }
            println!(
                "Successfully emitted AERIC bundle to '{}' (model_{}.c, model_{}.h, aven_shim.c) [SHA256: {}]",
                dir, stem, stem, bundle.model_hash
            );
            return;
        } else if let Some(out_path) = out_file {
            let p = std::path::Path::new(out_path);
            if p.is_dir() {
                let dir = out_path;
                let _ = fs::create_dir_all(dir);
                let model_c_path = format!("{}/model_{}.c", dir, stem);
                let model_h_path = format!("{}/model_{}.h", dir, stem);
                let shim_c_path = format!("{}/aven_shim.c", dir);

                let _ = fs::write(&model_c_path, &bundle.model_c);
                let _ = fs::write(&model_h_path, &bundle.header_h);
                let _ = fs::write(&shim_c_path, &bundle.shim_c);
                println!(
                    "Successfully emitted AERIC bundle to '{}' (model_{}.c, model_{}.h, aven_shim.c) [SHA256: {}]",
                    dir, stem, stem, bundle.model_hash
                );
                return;
            } else {
                if let Err(e) = fs::write(out_path, &bundle.single_unit_c) {
                    eprintln!("Error writing output C file '{}': {}", out_path, e);
                    process::exit(1);
                }
                println!(
                    "Successfully emitted AERIC C unit to '{}' [SHA256: {}]",
                    out_path, bundle.model_hash
                );
                return;
            }
        } else if emit_c {
            println!("{}", bundle.single_unit_c);
            return;
        } else {
            println!(
                "Generated AERIC shim for model '{}' [SHA256: {}].",
                blk.name, bundle.model_hash
            );
            println!("Use --emit-c to view the C unit, or --out-dir <dir> to save the bundle.");
            return;
        }
    }

    let work_dir = model_dir(path);
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

fn cmd_emit_smt(path: &str, out_file: Option<&str>, options: &smt::SmtOptions) {
    let module = parse_file(path);
    let smt_text = match smt::SmtTranspiler::transpile_module(&module, options) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    if let Some(out_path) = out_file {
        if let Err(e) = fs::write(out_path, &smt_text) {
            eprintln!("Error writing output SMT-LIB2 file '{}': {}", out_path, e);
            process::exit(1);
        }
        println!("Successfully emitted SMT-LIB2 to '{}'", out_path);
    } else {
        println!("{}", smt_text);
    }
}

fn cmd_verify_smt(path: &str, solver_cmd: Option<&str>, options: &smt::SmtOptions) {
    let module = parse_file(path);
    let smt_text = match smt::SmtTranspiler::transpile_module(&module, options) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    println!("============================================================");
    println!(" Aven SMT Formal Verification Engine (Phase 5)");
    println!(" Model: '{}' | Mode: {:?}", path, options.mode);
    println!("============================================================");

    let res = match smt::SmtTranspiler::run_solver(&smt_text, solver_cmd) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Verification Solver Error: {}", e);
            process::exit(1);
        }
    };

    println!("Solver: {}", res.solver_used);
    match (options.mode, res.status) {
        (smt::SmtMode::Prove, smt::SmtStatus::Unsat) => {
            println!("\n✓ FORMALLY PROVEN (UNSAT)");
            println!("  Safety invariant holds unconditionally across the entire parameter envelope.");
        }
        (smt::SmtMode::Prove, smt::SmtStatus::Sat) => {
            println!("\n✗ INVARIANT VIOLATED (SAT)");
            println!("  Counterexample found! The safety invariant can be broken under the following state:");
            let mut keys: Vec<&String> = res.model.keys().collect();
            keys.sort();
            for k in keys {
                println!("    {} = {}", k, res.model[k]);
            }
            process::exit(EXIT_CONSTRAINT_VIOLATION);
        }
        (smt::SmtMode::Satisfy, smt::SmtStatus::Sat) => {
            println!("\n✓ SATISFIED (SAT)");
            println!("  Feasible physical state solved:");
            let mut keys: Vec<&String> = res.model.keys().collect();
            keys.sort();
            for k in keys {
                println!("    {} = {}", k, res.model[k]);
            }
        }
        (smt::SmtMode::Satisfy, smt::SmtStatus::Unsat) => {
            println!("\n✗ INFEASIBLE (UNSAT)");
            println!("  No physical state exists that satisfies all declared equations and constraints.");
            process::exit(EXIT_CONSTRAINT_VIOLATION);
        }
        (_, smt::SmtStatus::Unknown(msg)) => {
            println!("\n? UNDECIDED (UNKNOWN)");
            println!("  Solver returned: {}", msg);
            process::exit(1);
        }
    }
}

fn cmd_safety_case(
    path: &str,
    out_file: Option<&str>,
    format: &str,
    standard: smt::SafetyStandard,
    custom_invariants: &[String],
    solver: Option<&str>,
) {
    let module = parse_file(path);
    println!("============================================================");
    println!(" Aven Safety Case Generator (DO-178C / ISO 26262 ASIL-D)");
    println!(" Model: '{}' | Standard: {}", path, standard);
    println!("============================================================");

    let report = match smt::SafetyCaseGenerator::generate(
        &module,
        path,
        standard,
        custom_invariants,
        solver,
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Safety Case Generation Error: {}", e);
            process::exit(1);
        }
    };

    let content = if format == "json" {
        report.to_json()
    } else {
        report.to_markdown()
    };

    if let Some(out_path) = out_file {
        if let Err(e) = fs::write(out_path, &content) {
            eprintln!("Error writing safety case to '{}': {}", out_path, e);
            process::exit(1);
        }
        println!("Successfully generated Safety Case report to '{}'", out_path);
    } else {
        println!("{}", content);
    }

    if !report.all_passed {
        eprintln!("\nNotice: One or more safety invariants were violated.");
        process::exit(EXIT_CONSTRAINT_VIOLATION);
    }
}

fn cmd_debug(
    model_path: &str,
    ledger_path: Option<&str>,
    goto_tick: Option<usize>,
    what_if: Option<&str>,
    diff_ticks: Option<(usize, usize)>,
) {
    let module = parse_file(model_path);
    let work_dir = model_dir(model_path);

    let ledger = if let Some(lp) = ledger_path {
        let text = match fs::read_to_string(lp) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Error reading ledger '{}': {}", lp, e);
                process::exit(1);
            }
        };
        match ledger::from_jsonl(&text, None) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Error loading ledger '{}': {}", lp, e);
                process::exit(1);
            }
        }
    } else {
        let res = execute_module(&module, false, work_dir);
        res.ledger
    };

    let mut session = debugger::DebuggerSession::new(model_path, module, ledger);

    if let Some((t1, t2)) = diff_ticks {
        let diffs = session.diff_ticks(t1, t2);
        println!("Diff between tick {} and tick {}:", t1, t2);
        if diffs.is_empty() {
            println!("  No variable changes.");
        } else {
            for d in diffs {
                let (old_s, new_s, delta_s) = session.show_diff(&d);
                let delta_s = if delta_s == "None" { String::new() } else { format!(" (Δ {})", delta_s) };
                println!("  {:<24} : {} -> {}{}", d.name, old_s, new_s, delta_s);
            }
        }
        return;
    }

    if let Some(wi) = what_if {
        if let Some(pos) = wi.find('=') {
            let var = wi[..pos].trim();
            let val = match session.parse_value(var, &wi[pos + 1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("What-if error: {}", e);
                    process::exit(EXIT_ERROR);
                }
            };
            match session.what_if(&[(var.to_string(), val)]) {
                Ok(res) => {
                    println!(
                        "Counterfactual What-If Result (mutating '{}' = {} at t{}):",
                        var,
                        session.show(var, val),
                        res.baseline_tick
                    );
                    println!(
                        "  {:<24} {:>14} {:>14} {:>14}",
                        "Output Variable", "Nominal", "Branch", "Delta"
                    );
                    println!("  --------------------------------------------------------------------");
                    for d in res.diffs {
                        let (old_s, new_s, delta_s) = session.show_diff(&d);
                        println!("  {:<24} {:>14} {:>14} {:>14}", d.name, old_s, new_s, delta_s);
                    }
                }
                Err(e) => {
                    eprintln!("What-if error: {}", e);
                    process::exit(EXIT_ERROR);
                }
            }
        }
        return;
    }

    if let Some(t) = goto_tick {
        match session.goto_tick(t, true) {
            Ok(st) => {
                println!("Scrubbed to tick {}:", st.tick);
                let mut keys: Vec<&String> = st.vars.keys().collect();
                keys.sort();
                for k in keys {
                    let v = &st.vars[k];
                    println!("  {:<24} = {:>20}  ({})", v.name, session.show(&v.name, v.value), v.kind);
                }
            }
            Err(e) => eprintln!("Error jumping to tick {}: {}", t, e),
        }
        return;
    }

    // Default interactive REPL mode
    let stdin = io::stdin();
    let stdout = io::stdout();
    let _ = session.run_interactive(stdin.lock(), stdout.lock());
}

fn cmd_compact_ledger(in_path: &str, out_path: &str) {
    let text = match fs::read_to_string(in_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error reading input ledger '{}': {}", in_path, e);
            process::exit(1);
        }
    };

    let original = match ledger::from_jsonl(&text, None) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Error parsing ledger '{}': {}", in_path, e);
            process::exit(1);
        }
    };

    let compacted = compaction::LedgerCompactor::compact_ledger(&original);
    let compacted_jsonl = compacted.to_jsonl();

    if let Err(e) = fs::write(out_path, &compacted_jsonl) {
        eprintln!("Error writing compacted ledger to '{}': {}", out_path, e);
        process::exit(1);
    }

    println!(
        "Compacted ledger '{}' -> '{}': {} facts compressed to {} facts ({:.1}% reduction)",
        in_path,
        out_path,
        original.len(),
        compacted.len(),
        (1.0 - (compacted.len() as f64 / original.len().max(1) as f64)) * 100.0
    );
}

/// Whether the optimizer proved its result globally optimal, from its trace.
fn optimality_of(trace: &[String]) -> Option<&'static str> {
    let line = trace.iter().rev().find(|l| l.starts_with("[optimize]") && l.contains("objective ="))?;
    Some(if line.contains("proven optimal") { "proven" } else { "local" })
}

fn execute_module(
    module: &AvenModule,
    verbose: bool,
    working_dir: Option<&std::path::Path>,
) -> ExecutionResult {
    execute_module_with_key(module, verbose, working_dir, None)
}

fn execute_module_with_key(
    module: &AvenModule,
    verbose: bool,
    working_dir: Option<&std::path::Path>,
    signing_key: Option<&[u8]>,
) -> ExecutionResult {
    // 0. Verify compile-time dimensional homogeneity across all relation blocks
    let mut type_checker = units::DimensionalChecker::new();
    if let Err(e) = type_checker.check_module(module) {
        eprintln!("\n{}", e);
        process::exit(1);
    }
    let scale = unit_scale(module);

    let mut env = HashMap::new();
    let mut ledger = match signing_key {
        Some(k) => Ledger::with_key(k),
        None => new_ledger(),
    };
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

    // Execute external, persistent IPC, SHM ring, or native FFI evaluators
    for blk in &module.blocks {
        let is_external = matches!(&blk.backend, BackendDecl::External { .. });
        let is_ipc = matches!(&blk.backend, BackendDecl::Ipc { .. });
        let is_shm = matches!(&blk.backend, BackendDecl::Shm { .. });
        let is_native_ffi = matches!(&blk.backend, BackendDecl::NativeFfi { .. });
        if !is_external && !is_ipc && !is_shm && !is_native_ffi {
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

        // Adapters exchange values in each variable's declared unit
        let mut varmap = evaluator::VarMap::new();
        for (k, v) in &env {
            varmap.set(k, scale.from_base(k, *v));
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
            BackendDecl::Shm { path } => {
                let shm_eval = evaluator::ShmEvaluator::new(path);
                if verbose {
                    println!(
                        "Reading from shared-memory ring buffer at '{}' for relation '{}'...",
                        path, blk.name
                    );
                }
                (shm_eval.evaluate(&varmap), "shm")
            }
            BackendDecl::NativeFfi { lib_path, symbol } => {
                let ffi_eval = evaluator::NativeFfiEvaluator::new(lib_path, symbol);
                if verbose {
                    println!(
                        "Executing in-process native FFI '{}' ({}) for relation '{}'...",
                        lib_path, symbol, blk.name
                    );
                }
                (ffi_eval.evaluate(&varmap), "native_ffi")
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
                    .map(|(k, v)| {
                        let base = scale.to_base(&k, v);
                        (k, base)
                    })
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

            if let Some(telem) = &solver.opt_telemetry {
                seq += 1;
                ledger.append(Fact {
                    seq,
                    tick: 0,
                    kind: "audit".to_string(),
                    name: "milp_certificate".to_string(),
                    value: telem.incumbent,
                    causes: Vec::new(),
                    origin: format!(
                        "bound:{:.6};incumbent:{:.6};duality_gap:{:.2e};cuts_active:{};dual_norm:{:.2e};proven_optimal:{}",
                        telem.bound, telem.incumbent, telem.duality_gap, telem.cuts_active, telem.dual_norm, telem.proven_optimal
                    ),
                    metadata: Some(ledger::FactMetadata {
                        warnings: if !telem.proven_optimal {
                            vec!["duality_gap_unclosed".to_string()]
                        } else {
                            Vec::new()
                        },
                        ..Default::default()
                    }),
                });
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

            // Stream real-time actuation commands to IPC daemons and SHM ring buffers
            for blk in &module.blocks {
                match &blk.backend {
                    BackendDecl::Ipc { endpoint } => {
                        let ipc_eval = evaluator::IpcEvaluator::new(endpoint);
                        if let Err(e) = ipc_eval.actuate(&solved_env) {
                            if verbose {
                                eprintln!("  [ipc] Note: Actuation stream to '{}' failed or closed: {}", endpoint, e);
                            }
                        } else if verbose {
                            println!("  [ipc] Streamed {} actuation command(s) to daemon at {}", solved_env.len(), endpoint);
                        }
                    }
                    BackendDecl::Shm { path } => {
                        let shm_eval = evaluator::ShmEvaluator::new(path);
                        if let Err(e) = shm_eval.actuate(&solved_env) {
                            if verbose {
                                eprintln!("  [shm] Note: Actuation stream to '{}' failed: {}", path, e);
                            }
                        } else if verbose {
                            println!("  [shm] Written {} actuation command(s) to ring buffer at {}", solved_env.len(), path);
                        }
                    }
                    _ => {}
                }
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
                println!("
{}", diag.report());
            }
            // Keep everything that did settle, with its causes, so a stalled model still
            // reports its solved parts
            let partial = solver.partial.clone();
            for (k, cause_vars) in solver.derivation_order(&env, &partial) {
                seq += 1;
                let mut causes = Vec::new();
                let mut tainted = false;
                for v in &cause_vars {
                    if let Some(fact) = ledger.latest(v) {
                        causes.push(fact.seq);
                        tainted |= fact.metadata.as_ref().map_or(false, |m| m.is_stale);
                    }
                }
                if tainted && !stale_facts.contains(&k) {
                    stale_facts.push(k.clone());
                }
                let origin = match solver.relations.iter().find(|e| e.lhs == k) {
                    Some(eq) => format!("= {}", eq.rhs),
                    None => "derived".to_string(),
                };
                ledger.append(Fact {
                    seq,
                    tick: 0,
                    kind: "derived".to_string(),
                    name: k.clone(),
                    value: partial[&k],
                    causes,
                    origin,
                    metadata: tainted.then(|| ledger::FactMetadata {
                        timestamp_ms: None,
                        ttl_ms: None,
                        is_stale: true,
                        warnings: vec!["causal-stale-taint".to_string()],
                    }),
                });
            }
            let env = partial;
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
    println!("Pillar 5: Trade-off sweep & knee detection");
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
            "  => Knee point: Point {}\n",
            k + 1
        );
    }

    println!("============================================================");
    println!("Verification Complete: All 5 Pillars Functioning Natively.");
    println!("============================================================");
}

fn cmd_pareto(
    path: &str,
    var: &str,
    min_val: f64,
    max_val: f64,
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

    let scale = unit_scale(&module);
    let dims = units::DimensionalChecker::new()
        .check_module(&module)
        .map(|r| r.variable_dimensions)
        .unwrap_or_default();
    if !module_variables(&module).contains(var) {
        eprintln!("Error: '{}' is not a variable of '{}'", var, path);
        process::exit(1);
    }
    // The range is given in the swept variable's own unit
    let min = scale.to_base(var, min_val);
    let max = scale.to_base(var, max_val);
    let header = |name: &str| {
        let label = scale.label(name, dims.get(name));
        if label.is_empty() { name.to_string() } else { format!("{} [{}]", name, label) }
    };

    println!("============================================================");
    println!(" Aven trade-off sweep");
    println!(
        " Relation: '{}' | Sweep: {} in [{}, {}]",
        rel_block.name,
        var,
        show_value(&scale, &dims, var, min),
        show_value(&scale, &dims, var, max)
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

                print!("\n{:<6} {:<18}", "Step", header(var));
                for vname in &display_vars {
                    print!(" {:<18}", header(vname));
                }
                println!();
                println!("{}", "-".repeat(24 + 19 * display_vars.len()));

                for (idx, pt) in frontier.iter().enumerate() {
                    let v_sweep = scale.from_base(var, pt.vars.get(var).unwrap_or(0.0));
                    print!("{:<6} {:<18.2}", idx + 1, v_sweep);
                    for vname in &display_vars {
                        let val = scale.from_base(vname, pt.vars.get(vname).unwrap_or(0.0));
                        if val.fract() == 0.0 && val.abs() < 1e9 {
                            print!(" {:<18.0}", val);
                        } else {
                            print!(" {:<18.4}", val);
                        }
                    }
                    println!();
                }
            }

            let requested = steps.max(2) + 1;
            if frontier.len() < requested {
                println!(
                    "\n{} of {} sweep points shown; the others failed to solve or duplicated a shown point.",
                    frontier.len(),
                    requested
                );
            }
            if let Some(knee) = evaluator::Orchestrator::find_knee_point(&frontier) {
                println!(
                    "\nKnee point: step {} (largest normalized trade-off between the two most strongly conflicting outputs)",
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
    let scale = unit_scale(&module);
    let dims = units::DimensionalChecker::new()
        .check_module(&module)
        .map(|r| r.variable_dimensions)
        .unwrap_or_else(|e| {
            eprintln!("{}", e);
            process::exit(EXIT_ERROR);
        });

    // Solve-block values are fixed parameters; each stdin line supplies or overrides facts.
    let mut parameters = HashMap::new();
    for solve in &module.solves {
        for b in &solve.bindings {
            parameters.insert(b.name.clone(), b.value);
        }
    }

    println!("Aven stream supervisor — model '{}', reading one JSON object per line from stdin", path);
    println!("{:<7} | {:<16} | Status", "Cycle", "Spec hash");
    println!("{}", "-".repeat(72));

    let stdin = std::io::stdin();
    let reader = stdin.lock();
    let mut cycle = 0;
    let mut ledger = new_ledger();
    let mut seq = 0;
    let mut failures = 0;
    let mut alarm_cycles = 0;

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

        let mut env = parameters.clone();
        let mut inputs: Vec<(String, f64)> = parsed_numbers
            .iter()
            .map(|(k, v)| (k.clone(), scale.to_base(k, *v)))
            .collect();
        inputs.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in &inputs {
            env.insert(k.clone(), *v);
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
        let given: std::collections::HashSet<String> = env.keys().cloned().collect();

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

        // A broken bound is an alarm about the readings; anything else is a failure to solve
        let mut alarms = Vec::new();
        let mut failure = match solver.solve(&env) {
            Ok(solved_env) => {
                for (k, v) in solved_env {
                    env.entry(k).or_insert(v);
                }
                None
            }
            Err(e @ solver::SolverError::ConstraintViolation { .. }) => {
                alarms.push(e.to_string());
                None
            }
            Err(e) => Some(e.to_string()),
        };
        if failure.is_none() {
            let mut stale = Vec::new();
            if let Err(e) = eval_forward_blocks(&module, &mut env, &mut ledger, &mut seq, &mut stale, false) {
                failure = Some(e.to_string());
            }
        }

        // Evaluate constraints from solve directives
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
                            alarms.push(format!(
                                "{}: {} {} {}",
                                c.lhs,
                                show_value(&scale, &dims, &c.lhs, lhs_val),
                                c.op,
                                show_value(&scale, &dims, &c.lhs, rhs_val)
                            ));
                        }
                    }
                }
            }
        }

        let mut varmap = evaluator::VarMap::new();
        for (k, v) in &env {
            varmap.set(k, *v);
        }
        let mut derived: Vec<&String> = env.keys().filter(|k| !given.contains(*k)).collect();
        derived.sort();
        let derived_str: Vec<String> = derived
            .iter()
            .map(|k| format!("{} = {}", k, show_value(&scale, &dims, k, env[*k])))
            .collect();

        let status = if let Some(e) = &failure {
            failures += 1;
            format!("UNSOLVED: {}", e)
        } else if !alarms.is_empty() {
            alarm_cycles += 1;
            alarms.dedup();
            format!("ALARM: {}", alarms.join("; "))
        } else {
            "SETTLED".to_string()
        };
        println!("#{:<6} | {:<16} | {}", cycle, varmap.compute_hash(), status);
        if !derived_str.is_empty() {
            println!("        {}", derived_str.join(", "));
        }

        if limit > 0 && cycle >= limit {
            break;
        }
    }

    println!("{}", "-".repeat(72));
    println!(
        "Stream completed: {} cycle(s), {} settled, {} alarm(s), {} unsolved.",
        cycle,
        cycle - failures - alarm_cycles,
        alarm_cycles,
        failures
    );
}

/// Formats a base-unit value in the variable's display unit, e.g. "69.4444 m·s^-1" or
/// "250.0000 km/h" for a variable declared in km/h.
fn show_value(
    scale: &units::UnitScale,
    dims: &HashMap<String, units::Dim>,
    name: &str,
    base: f64,
) -> String {
    let label = scale.label(name, dims.get(name));
    let v = scale.from_base(name, base);
    if label.is_empty() {
        format!("{:.4}", v)
    } else {
        format!("{:.4} {}", v, label)
    }
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

fn cmd_emit_wasm(file: &str, out_file: Option<&str>, emit_wat: bool) {
    let module = parse_file(file);
    let bundle = match codegen_wasm::WasmTranspiler::transpile(&module) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("WASM Transpiler Error: {}", e);
            process::exit(1);
        }
    };

    if emit_wat {
        if let Some(out_path) = out_file {
            if let Err(e) = fs::write(out_path, &bundle.wat) {
                eprintln!("Error writing WAT to '{}': {}", out_path, e);
                process::exit(1);
            }
            println!("Successfully emitted WAT to '{}'", out_path);
        } else {
            print!("{}", bundle.wat);
        }
    } else {
        let default_out = format!("{}.wasm", bundle.model_name);
        let target_path = out_file.unwrap_or(&default_out);
        if let Err(e) = fs::write(target_path, &bundle.wasm_bytes) {
            eprintln!("Error writing WASM bytecode to '{}': {}", target_path, e);
            process::exit(1);
        }
        println!(
            "Successfully emitted WebAssembly binary to '{}' ({} bytes, {} inputs, {} outputs)",
            target_path,
            bundle.wasm_bytes.len(),
            bundle.input_names.len(),
            bundle.output_names.len()
        );
    }
}

fn cmd_pkg_init(name: &str) {
    let cur_dir = std::path::Path::new(".");
    match package::PackageManager::init(cur_dir, name) {
        Ok(manifest_path) => {
            println!("Initialized Aven package '{}' at '{:?}'", name, manifest_path);
        }
        Err(e) => {
            eprintln!("Package init error: {}", e);
            process::exit(1);
        }
    }
}

fn cmd_pkg_pack(dir_str: &str, out_str: Option<&str>) {
    let dir = std::path::Path::new(dir_str);
    let out = out_str.map(std::path::Path::new);
    match package::PackageManager::pack(dir, out) {
        Ok((path, digest)) => {
            println!("Successfully packaged '{:?}' [SHA256: {}]", path, digest);
        }
        Err(e) => {
            eprintln!("Package pack error: {}", e);
            process::exit(1);
        }
    }
}

fn cmd_pkg_install(archive_str: &str) {
    let archive_path = std::path::Path::new(archive_str);
    let cache_dir = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(".aven")
        .join("packages");

    match package::PackageManager::install(archive_path, &cache_dir) {
        Ok(manifest) => {
            println!(
                "Successfully installed package '{}' v{} into '{:?}'",
                manifest.name, manifest.version, cache_dir
            );
        }
        Err(e) => {
            eprintln!("Package install error: {}", e);
            process::exit(1);
        }
    }
}

fn cmd_pkg_list() {
    let cache_dir = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(".aven")
        .join("packages");

    match package::PackageManager::list(&cache_dir) {
        Ok(list) => {
            if list.is_empty() {
                println!("No packages installed in '{:?}'", cache_dir);
            } else {
                println!("Installed Aven packages in '{:?}':", cache_dir);
                println!("  {:<24} {:<12} {}", "Package", "Version", "Description");
                println!("  -------------------------------------------------------------");
                for p in list {
                    println!("  {:<24} {:<12} {}", p.name, p.version, p.description);
                }
            }
        }
        Err(e) => {
            eprintln!("Package list error: {}", e);
            process::exit(1);
        }
    }
}
