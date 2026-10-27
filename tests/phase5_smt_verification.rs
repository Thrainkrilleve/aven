// tests/phase5_smt_verification.rs -- Phase 5 SMT-LIB2 formal verification test suite
// Verifies SMT-LIB2 (v2.6) code generation, automated logic detection (QF_LRA, QF_NRA, QF_NIRA),
// satisfiability checking, theorem proving via refutation (UNSAT = proven), and counterexample search.

use avenc::ast::{AvenModule, BlockKind};
use avenc::lexer::Lexer;
use avenc::parser::Parser;
use avenc::smt::{SmtMode, SmtOptions, SmtStatus, SmtTranspiler};
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn parse_module_from_str(src: &str, name: &str) -> AvenModule {
    let tokens = Lexer::new(src).tokenize().expect("tokenize");
    Parser::new(tokens).parse_module(name).expect("parse")
}

// 64-bit XorShift pseudo-random generator seeded from OS entropy
struct SmtTestPrng(u64);

impl SmtTestPrng {
    fn from_entropy() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let pid = std::process::id() as u128;
        let seed = ((nanos ^ (pid << 32)) as u64) | 0xACDCBEEF;
        SmtTestPrng(seed)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_f64_range(&mut self, min: f64, max: f64) -> f64 {
        let r = (self.next_u64() % 100_000) as f64 / 100_000.0;
        min + r * (max - min)
    }

    fn next_ident(&mut self, prefix: &str) -> String {
        let val = self.next_u64() % 1_000_000;
        format!("{}_{}", prefix, val)
    }
}

#[test]
fn test_smt_golden_server_capacity() {
    let src = fs::read_to_string("examples/server_capacity.aven").expect("read server_capacity");
    let module = parse_module_from_str(&src, "server_capacity");
    let blk = module
        .blocks
        .iter()
        .find(|b| b.kind == BlockKind::Forward)
        .expect("forward block");
    let solve = module.solves.first();

    // 1. Check Satisfy mode transpilation
    let sat_opts = SmtOptions {
        mode: SmtMode::Satisfy,
        logic: None,
        prove_invariant: None,
        block_name: None,
    };
    let smt_code = SmtTranspiler::transpile_block(blk, solve, &sat_opts).expect("transpile");

    // Logic must detect integer operations (ceil) and non-linearities => QF_NIRA
    assert!(smt_code.contains("(set-logic QF_NIRA)"));
    assert!(smt_code.contains("(declare-const allocated_workers Real)"));
    assert!(smt_code.contains("(declare-const total_ram_needed Real)"));
    assert!(smt_code.contains("(assert (= reqs_per_second 50000.0))"));
    assert!(smt_code.contains("(assert (= target_utilization 0.7))"));
    assert!(smt_code.contains("(check-sat)"));
    assert!(smt_code.contains("(get-model)"));

    // 2. Solve satisfiability via SmtTranspiler::run_solver (Z3)
    let res = SmtTranspiler::run_solver(&smt_code, None).expect("run_solver");
    assert_eq!(res.status, SmtStatus::Sat);
    assert_eq!(res.model.get("allocated_workers").map(|s| s.as_str()), Some("1429"));
    assert_eq!(res.model.get("total_ram_needed").map(|s| s.as_str()), Some("32676"));
    assert_eq!(res.model.get("raw_workers").map(|s| s.as_str()), Some("1000"));

    // 3. Prove invariant: total_ram_needed <= 35000.0 MUST hold (UNSAT = theorem proven)
    let prove_opts = SmtOptions {
        mode: SmtMode::Prove,
        logic: None,
        prove_invariant: Some("(<= total_ram_needed 35000.0)".to_string()),
        block_name: None,
    };
    let prove_code = SmtTranspiler::transpile_block(blk, solve, &prove_opts).expect("prove transpile");
    assert!(prove_code.contains("(assert (not (<= total_ram_needed 35000.0)))"));

    let prove_res = SmtTranspiler::run_solver(&prove_code, None).expect("prove solver");
    assert_eq!(prove_res.status, SmtStatus::Unsat, "Safety theorem must be UNSAT");

    // 4. Counterexample detection: total_ram_needed <= 30000.0 is false (SAT = counterexample)
    let false_opts = SmtOptions {
        mode: SmtMode::Prove,
        logic: None,
        prove_invariant: Some("(<= total_ram_needed 30000.0)".to_string()),
        block_name: None,
    };
    let false_code = SmtTranspiler::transpile_block(blk, solve, &false_opts).expect("false transpile");
    let false_res = SmtTranspiler::run_solver(&false_code, None).expect("false solver");
    assert_eq!(false_res.status, SmtStatus::Sat, "Violated invariant must be SAT");
    assert_eq!(false_res.model.get("total_ram_needed").map(|s| s.as_str()), Some("32676"));
}

#[test]
fn test_smt_golden_mixed_integer_storage_sizing() {
    let src = fs::read_to_string("examples/storage_sizing.aven").expect("read storage_sizing");
    let module = parse_module_from_str(&src, "storage_sizing");
    let blk = &module.blocks[0];
    let solve = module.solves.first();

    let opts = SmtOptions::default();
    let smt_code = SmtTranspiler::transpile_block(blk, solve, &opts).expect("transpile");

    // Verify integer variable declaration
    assert!(smt_code.contains("(declare-const drives Int)"));
    assert!(smt_code.contains("(declare-const usable_tb Real)"));
    assert!(smt_code.contains("(assert (>= usable_tb target_capacity))"));
    assert!(smt_code.contains("(assert (= target_capacity 100.0))"));

    let res = SmtTranspiler::run_solver(&smt_code, None).expect("run solver");
    assert_eq!(res.status, SmtStatus::Sat);
    assert_eq!(res.model.get("drives").map(|s| s.as_str()), Some("8"));
    assert_eq!(res.model.get("usable_tb").map(|s| s.as_str()), Some("108"));
}

#[test]
fn test_smt_randomized_1_continuous_invariant_proof() {
    let mut prng = SmtTestPrng::from_entropy();

    let flow_var = prng.next_ident("coolant_flow");
    let res_var = prng.next_ident("pipe_resistance");
    let drop_var = prng.next_ident("pressure_drop");

    let flow_val = prng.next_f64_range(10.0, 25.0);
    let res_val = prng.next_f64_range(1.5, 3.5);

    // Physical model: delta_P = R * flow^2
    let model_src = format!(
        r#"
        relation FluidCircuit {{
            {drop_var} = {res_var} * {flow_var}^2
        }}
        solve FluidCircuit with {{
            {flow_var} = {flow_val:.2}
            {res_var} = {res_val:.2}
        }}
        "#
    );

    let module = parse_module_from_str(&model_src, "fluid");
    let blk = &module.blocks[0];
    let solve = module.solves.first();

    let exact_expected = res_val * flow_val * flow_val;

    // Satisfy mode: verify Z3 finds matching state
    let sat_opts = SmtOptions::default();
    let smt_text = SmtTranspiler::transpile_block(blk, solve, &sat_opts).expect("sat transpile");
    let sat_res = SmtTranspiler::run_solver(&smt_text, None).expect("solve sat");
    assert_eq!(sat_res.status, SmtStatus::Sat);

    // Prove mode: prove that pressure_drop <= exact_expected + 100.0
    let upper_bound = exact_expected + 100.0;
    let prove_opts = SmtOptions {
        mode: SmtMode::Prove,
        logic: None,
        prove_invariant: Some(format!("(<= {} {:.2})", drop_var, upper_bound)),
        block_name: None,
    };
    let prove_text = SmtTranspiler::transpile_block(blk, solve, &prove_opts).expect("prove transpile");
    let prove_res = SmtTranspiler::run_solver(&prove_text, None).expect("prove solve");
    assert_eq!(prove_res.status, SmtStatus::Unsat, "Upper bound theorem must hold");

    // Counterexample test: prove pressure_drop <= exact_expected - 50.0 (impossible => SAT)
    let lower_invalid_bound = exact_expected - 50.0;
    let fail_opts = SmtOptions {
        mode: SmtMode::Prove,
        logic: None,
        prove_invariant: Some(format!("(<= {} {:.2})", drop_var, lower_invalid_bound)),
        block_name: None,
    };
    let fail_text = SmtTranspiler::transpile_block(blk, solve, &fail_opts).expect("fail transpile");
    let fail_res = SmtTranspiler::run_solver(&fail_text, None).expect("fail solve");
    assert_eq!(fail_res.status, SmtStatus::Sat, "Invalid invariant must produce SAT counterexample");
}

#[test]
fn test_smt_randomized_2_mixed_integer_capacity_prover() {
    let mut prng = SmtTestPrng::from_entropy();

    let unit_size = prng.next_f64_range(12.0, 24.0);
    let overhead = prng.next_f64_range(5.0, 15.0);
    let min_units: i64 = (prng.next_u64() % 5 + 3) as i64; // 3 to 7 units

    let model_src = format!(
        r#"
        relation CapacityAllocation {{
            int units
            raw_capacity = units * {unit_size:.1}
            net_capacity = raw_capacity - {overhead:.1}
            net_capacity >= 100.0
        }}
        solve CapacityAllocation with {{
            units >= {min_units}
        }}
        "#
    );

    let module = parse_module_from_str(&model_src, "cap");
    let blk = &module.blocks[0];
    let solve = module.solves.first();

    let opts = SmtOptions::default();
    let smt_text = SmtTranspiler::transpile_block(blk, solve, &opts).expect("transpile");

    assert!(smt_text.contains("(declare-const units Int)"));
    assert!(smt_text.contains("(declare-const net_capacity Real)"));

    let res = SmtTranspiler::run_solver(&smt_text, None).expect("run solver");
    assert_eq!(res.status, SmtStatus::Sat);

    let solved_units: i64 = res
        .model
        .get("units")
        .expect("units in model")
        .parse()
        .expect("parse int");
    assert!(solved_units >= min_units);
    let raw = (solved_units as f64) * unit_size;
    let net = raw - overhead;
    assert!(net >= 99.999);
}

#[test]
fn test_smt_randomized_3_rapid_model_cycling_and_syntax() {
    let mut prng = SmtTestPrng::from_entropy();

    for cycle in 0..5 {
        let v_in1 = prng.next_ident(&format!("x{}_a", cycle));
        let v_in2 = prng.next_ident(&format!("x{}_b", cycle));
        let v_out = prng.next_ident(&format!("y{}_out", cycle));

        let k1 = prng.next_f64_range(1.0, 20.0);
        let k2 = prng.next_f64_range(0.5, 5.0);

        let model_src = format!(
            r#"
            relation Cycle_{cycle} {{
                {v_out} = abs({v_in1} * {k1:.3}) + ({v_in2} / {k2:.3})
            }}
            solve Cycle_{cycle} with {{
                {v_in1} = 10.0
                {v_in2} = 5.0
            }}
            "#
        );

        let module = parse_module_from_str(&model_src, &format!("cycle_{}", cycle));
        let blk = &module.blocks[0];
        let solve = module.solves.first();

        let smt_text = SmtTranspiler::transpile_block(blk, solve, &SmtOptions::default())
            .expect("transpile");

        // Verify balanced parentheses
        let open_count = smt_text.chars().filter(|&c| c == '(').count();
        let close_count = smt_text.chars().filter(|&c| c == ')').count();
        assert_eq!(open_count, close_count, "SMT S-expressions must have balanced parentheses");

        // Execute Z3 solver to verify zero syntax errors
        let res = SmtTranspiler::run_solver(&smt_text, None).expect("run solver");
        assert_eq!(res.status, SmtStatus::Sat);
        assert!(res.model.contains_key(&v_out));
    }
}

#[test]
fn test_smt_cli_emit_and_verify() {
    let avenc_bin = env!("CARGO_BIN_EXE_avenc");
    let temp_smt = std::env::temp_dir().join(format!("test_aero_{}.smt2", std::process::id()));

    // 1. Test avenc emit-smt to file
    let status = Command::new(avenc_bin)
        .args([
            "emit-smt",
            "examples/aerodynamics.aven",
            "-o",
            temp_smt.to_str().unwrap(),
        ])
        .status()
        .expect("execute avenc emit-smt");
    assert!(status.success(), "avenc emit-smt failed");
    assert!(temp_smt.exists());

    let content = fs::read_to_string(&temp_smt).expect("read smt2");
    assert!(content.contains("(set-logic QF_NRA)"));
    assert!(content.contains("(declare-const dynamic_pressure Real)"));
    assert!(content.contains("(declare-const lift Real)"));
    let _ = fs::remove_file(&temp_smt);

    // 2. Test avenc verify-smt
    let output = Command::new(avenc_bin)
        .args(["verify-smt", "examples/aerodynamics.aven"])
        .output()
        .expect("execute avenc verify-smt");
    assert!(output.status.success(), "avenc verify-smt failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("✓ SATISFIED (SAT)"));
    assert!(stdout.contains("lift ="));
}
