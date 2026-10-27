// src/bin/bench_aven.rs -- Rust backend: drives AVEN's own evaluator in-process.
//
// This is the "use the Rust files" backend: it parses examples/server_capacity.aven
// and evaluates the forward block through diff::eval_expr (the exact expression
// evaluator `avenc run` uses, with the same 1e-9 ceil-snap), over the shared
// battery. It therefore measures the INTERPRETER path -- the honest Rust-side
// number to contrast with the compiled emit-c backend.
//
// Run: cargo run --release --bin bench_aven -- <scratchdir>
use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::time::Instant;

use avenc::ast::{AvenModule, BlockKind};
use avenc::codegen_c::CTranspiler;
use avenc::diff;
use avenc::lexer::Lexer;
use avenc::parser::Parser;

const INPUTS: [&str; 8] = [
    "avg_latency_ms",
    "avg_payload_kb",
    "framing_overhead_ratio",
    "os_kernel_ram_mb",
    "ram_per_worker",
    "reqs_per_second",
    "socket_buffer_mb",
    "target_utilization",
];
const OUTPUTS: [&str; 5] = [
    "raw_workers",
    "allocated_workers",
    "total_ram_needed",
    "payload_gbps",
    "wire_rate_gbps",
];

fn parse_model(path: &str) -> AvenModule {
    let src = std::fs::read_to_string(path).expect("read model");
    let toks = Lexer::new(&src).tokenize().expect("lex");
    Parser::new(toks).parse_module(path).expect("parse")
}

fn main() {
    let sp = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let model = parse_model("examples/server_capacity.aven");
    let blk = model
        .blocks
        .iter()
        .find(|b| b.kind == BlockKind::Forward)
        .expect("no forward block");
    let order = CTranspiler::topo_sort(blk).expect("topo");

    // load battery
    let f = std::fs::File::open(format!("{sp}/battery.txt")).expect("open battery");
    let mut r = BufReader::new(f);
    let mut hdr = String::new();
    r.read_line(&mut hdr).unwrap();
    let n: usize = hdr.split_whitespace().next().unwrap().parse().unwrap();
    let mut rows: Vec<[f64; 8]> = Vec::with_capacity(n);
    for line in r.lines() {
        let line = line.unwrap();
        let mut it = line.split_whitespace();
        let mut v = [0.0f64; 8];
        for slot in &mut v {
            *slot = it.next().unwrap().parse().unwrap();
        }
        rows.push(v);
    }

    // correctness pass -> out_rust.txt (bits)
    let mut env: HashMap<String, f64> = HashMap::new();
    let of = std::fs::File::create(format!("{sp}/out_rust.txt")).expect("create out");
    let mut w = BufWriter::new(of);
    for (i, v) in rows.iter().enumerate() {
        for k in 0..8 {
            env.insert(INPUTS[k].to_string(), v[k]);
        }
        for &idx in &order {
            let eq = &blk.equations[idx];
            let val = diff::eval_expr(&eq.rhs, &env).unwrap_or(f64::NAN);
            env.insert(eq.lhs.clone(), val);
        }
        write!(w, "{i}").unwrap();
        for name in OUTPUTS {
            let b = env.get(name).copied().unwrap_or(f64::NAN).to_bits();
            write!(w, " {:016x}", b).unwrap();
        }
        writeln!(w).unwrap();
    }
    w.flush().unwrap();

    // timing: warm interpreter, K reps over the battery
    let k_reps = 20usize;
    let mut sink = 0.0f64;
    let t0 = Instant::now();
    for _ in 0..k_reps {
        for v in &rows {
            for k in 0..8 {
                env.insert(INPUTS[k].to_string(), v[k]);
            }
            for &idx in &order {
                let eq = &blk.equations[idx];
                let val = diff::eval_expr(&eq.rhs, &env).unwrap_or(f64::NAN);
                env.insert(eq.lhs.clone(), val);
            }
            sink += *env.get("allocated_workers").unwrap();
        }
    }
    let ns = t0.elapsed().as_nanos() as f64 / (n as f64 * k_reps as f64);
    println!(
        "{{\"backend\":\"rust_evaluator\",\"n\":{n},\"ns_per_solve\":{:.3},\"solves_per_sec\":{:.0},\"_sink\":{}}}",
        ns,
        1e9 / ns,
        sink
    );
}
