// tests/toolchain_aeric_shim.rs -- Toolchain-A verification suite for avenc emit-c --aeric-shim
// Verifies automatic AERIC adapter generation, alphabetical input sorting, forward equation
// output ordering, SHA-256 model-identity hashing, domain finiteness guards, and CLI emission.

use avenc::ast::{AvenModule, BlockKind};
use avenc::codegen_c::CTranspiler;
use avenc::diff;
use avenc::lexer::Lexer;
use avenc::parser::Parser;
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn parse_module_from_str(src: &str, name: &str) -> AvenModule {
    let tokens = Lexer::new(src).tokenize().expect("tokenize");
    Parser::new(tokens).parse_module(name).expect("parse")
}

// 64-bit XorShift pseudo-random generator seeded from OS entropy
struct TestPrng(u64);

impl TestPrng {
    fn from_entropy() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let pid = std::process::id() as u128;
        let seed = ((nanos ^ (pid << 32)) as u64) | 0xDEADBEEF;
        TestPrng(seed)
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
fn test_aeric_shim_server_capacity_golden() {
    let src = fs::read_to_string("examples/server_capacity.aven").expect("read server_capacity");
    let module = parse_module_from_str(&src, "server_capacity");
    let blk = module
        .blocks
        .iter()
        .find(|b| b.kind == BlockKind::Forward)
        .expect("forward block");

    let bundle =
        CTranspiler::generate_aeric_shim(blk, "server_capacity").expect("generate_aeric_shim");

    // 1. Check exact arities
    assert_eq!(bundle.n_in, 8);
    assert_eq!(bundle.n_out, 5);

    // 2. Canonical alphabetical input order
    let expected_inputs = [
        "avg_latency_ms",
        "avg_payload_kb",
        "framing_overhead_ratio",
        "os_kernel_ram_mb",
        "ram_per_worker",
        "reqs_per_second",
        "socket_buffer_mb",
        "target_utilization",
    ];
    assert_eq!(bundle.input_names, expected_inputs);

    // 3. Forward equation output order
    let expected_outputs = [
        "raw_workers",
        "allocated_workers",
        "total_ram_needed",
        "payload_gbps",
        "wire_rate_gbps",
    ];
    assert_eq!(bundle.output_names, expected_outputs);

    // 4. Model SHA-256 identity hash verification
    assert_eq!(bundle.model_hash.len(), 64);
    assert!(bundle.model_hash.chars().all(|c| c.is_ascii_hexdigit()));

    // 5. Header checks
    assert!(
        bundle
            .header_h
            .contains("#define MODEL_SERVER_CAPACITY_N_IN   8")
    );
    assert!(
        bundle
            .header_h
            .contains("#define MODEL_SERVER_CAPACITY_N_OUT  5")
    );
    assert!(
        bundle
            .header_h
            .contains(&format!("\"{}\"", bundle.model_hash))
    );
    assert!(
        bundle
            .header_h
            .contains("int aven_solve(const double *in, size_t n_in, double *out, size_t n_out);")
    );
    assert!(
        bundle
            .header_h
            .contains("const char* aven_model_hash(void);")
    );

    // 6. Shim checks
    assert!(
        bundle
            .shim_c
            .contains("#include \"model_server_capacity.h\"")
    );
    assert!(
        bundle
            .shim_c
            .contains("int aven_solve(const double *in, size_t n_in, double *out, size_t n_out)")
    );
    assert!(
        bundle
            .shim_c
            .contains("if (n_in != 8 || n_out != 5) return 1;")
    );
    assert!(bundle.shim_c.contains("aven_ProductionProfile("));
    assert!(bundle.shim_c.contains("isfinite(r_allocated_workers)"));
    assert!(bundle.shim_c.contains("return 2;"));

    // 7. Single unit C checks
    assert!(bundle.single_unit_c.contains("aven_ceil"));
    assert!(
        bundle
            .single_unit_c
            .contains("AVEN_EXPORT void aven_ProductionProfile")
    );
    assert!(
        bundle
            .single_unit_c
            .contains("int aven_solve(const double *in, size_t n_in, double *out, size_t n_out)")
    );
}

#[test]
fn test_aeric_shim_aerodynamics_golden() {
    let src = fs::read_to_string("examples/aerodynamics.aven").expect("read aerodynamics");
    let module = parse_module_from_str(&src, "aerodynamics");
    let blk = &module.blocks[0];

    let bundle =
        CTranspiler::generate_aeric_shim(blk, "aerodynamics").expect("generate_aeric_shim");

    assert_eq!(bundle.n_in, 4);
    assert_eq!(bundle.n_out, 2);

    let expected_inputs = ["air_density", "lift_coeff", "velocity", "wing_area"];
    assert_eq!(bundle.input_names, expected_inputs);

    let expected_outputs = ["dynamic_pressure", "lift"];
    assert_eq!(bundle.output_names, expected_outputs);

    assert_eq!(bundle.model_hash.len(), 64);
    assert!(
        bundle
            .header_h
            .contains("#define MODEL_AERODYNAMICS_N_IN   4")
    );
    assert!(
        bundle
            .header_h
            .contains("#define MODEL_AERODYNAMICS_N_OUT  2")
    );
}

#[test]
fn test_aeric_shim_randomized_1_polynomial_generation() {
    let mut prng = TestPrng::from_entropy();

    let in1 = prng.next_ident("coeff_a");
    let in2 = prng.next_ident("coeff_b");
    let in3 = prng.next_ident("factor_c");
    let out1 = prng.next_ident("target_alpha");
    let out2 = prng.next_ident("target_beta");

    let c1 = prng.next_f64_range(1.5, 9.5);
    let c2 = prng.next_f64_range(2.0, 5.0);

    let model_src = format!(
        r#"
        forward DynamicPoly {{
            {out1} = {in1} * {c1:.4} + {in2} * {c2:.4}
            {out2} = {out1} * {in3} + {in1}
        }}
        "#
    );

    let module = parse_module_from_str(&model_src, "dynamic_poly");
    let blk = &module.blocks[0];

    let bundle = CTranspiler::generate_aeric_shim(blk, "dynamic_poly").expect("shim");

    assert_eq!(bundle.n_in, 3);
    assert_eq!(bundle.n_out, 2);

    // Verify alphabetical input ordering
    let mut sorted_inputs = vec![in1.clone(), in2.clone(), in3.clone()];
    sorted_inputs.sort();
    assert_eq!(bundle.input_names, sorted_inputs);

    // Verify output ordering matches equation order
    assert_eq!(bundle.output_names, vec![out1.clone(), out2.clone()]);

    // Deterministic hash check: same model must yield identical SHA256
    let bundle2 = CTranspiler::generate_aeric_shim(blk, "dynamic_poly").expect("shim2");
    assert_eq!(bundle.model_hash, bundle2.model_hash);

    // Single unit C must contain correct definitions and arities
    assert!(
        bundle
            .single_unit_c
            .contains(&format!("MODEL_DYNAMIC_POLY_N_IN   {}", bundle.n_in))
    );
    assert!(
        bundle
            .single_unit_c
            .contains(&format!("MODEL_DYNAMIC_POLY_N_OUT  {}", bundle.n_out))
    );
    assert!(bundle.single_unit_c.contains(&format!(
        "if (n_in != {} || n_out != {}) return 1;",
        bundle.n_in, bundle.n_out
    )));
}

#[test]
fn test_aeric_shim_randomized_2_domain_fault_guards() {
    let mut prng = TestPrng::from_entropy();

    let denom = prng.next_ident("sensor_divisor");
    let numer = prng.next_ident("sensor_numerator");
    let out_ratio = prng.next_ident("actuator_ratio");

    let model_src = format!(
        r#"
        forward DivGuard {{
            {out_ratio} = ceil({numer} / {denom})
        }}
        "#
    );

    let module = parse_module_from_str(&model_src, "div_guard");
    let blk = &module.blocks[0];

    let bundle = CTranspiler::generate_aeric_shim(blk, "div_guard").expect("shim");

    // Ceil snapping helper must be included in single_unit_c
    assert!(bundle.single_unit_c.contains("aven_ceil"));

    // Finiteness check must guard the actuator output
    assert!(
        bundle
            .single_unit_c
            .contains(&format!("isfinite(r_{})", out_ratio))
    );
    assert!(bundle.single_unit_c.contains("return 2;"));

    // Verify differential math against diff::eval_expr
    let mut env = HashMap::new();
    let num_val = prng.next_f64_range(10.0, 100.0);
    let den_val = prng.next_f64_range(1.0, 5.0);
    env.insert(numer.clone(), num_val);
    env.insert(denom.clone(), den_val);

    let expected = diff::eval_expr(&blk.equations[0].rhs, &env).expect("eval");
    assert!(expected.is_finite());
    assert!(expected >= (num_val / den_val));
}

#[test]
fn test_aeric_shim_randomized_3_rapid_model_cycling() {
    let mut prng = TestPrng::from_entropy();

    for cycle in 0..5 {
        let v_in1 = prng.next_ident(&format!("p{}_in1", cycle));
        let v_in2 = prng.next_ident(&format!("p{}_in2", cycle));
        let v_in3 = prng.next_ident(&format!("p{}_in3", cycle));
        let v_out1 = prng.next_ident(&format!("p{}_out1", cycle));
        let v_out2 = prng.next_ident(&format!("p{}_out2", cycle));

        let k1 = prng.next_f64_range(0.1, 10.0);
        let k2 = prng.next_f64_range(1.0, 50.0);

        let model_src = format!(
            r#"
            forward RapidCycle_{cycle} {{
                {v_out1} = ({v_in1} * {k1:.3}) + {v_in2}
                {v_out2} = ({v_out1} * {v_in3}) - {k2:.3}
            }}
            "#
        );

        let module = parse_module_from_str(&model_src, &format!("rapid_{}", cycle));
        let blk = &module.blocks[0];

        let bundle =
            CTranspiler::generate_aeric_shim(blk, &format!("rapid_{}", cycle)).expect("rapid shim");

        assert_eq!(bundle.n_in, 3);
        assert_eq!(bundle.n_out, 2);

        // Inputs must be alphabetically sorted
        let mut expected_in = vec![v_in1.clone(), v_in2.clone(), v_in3.clone()];
        expected_in.sort();
        assert_eq!(bundle.input_names, expected_in);

        // Outputs must be in equation order
        assert_eq!(bundle.output_names, vec![v_out1.clone(), v_out2.clone()]);

        // SHA-256 must be valid
        assert_eq!(bundle.model_hash.len(), 64);
    }
}

#[test]
fn test_aeric_shim_cli_out_dir_emission() {
    let temp_dir = std::env::temp_dir().join(format!("aven_aeric_test_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_dir);

    // Run avenc CLI via binary command
    let avenc_bin = env!("CARGO_BIN_EXE_avenc");
    let status = Command::new(avenc_bin)
        .args([
            "emit-c",
            "examples/server_capacity.aven",
            "--aeric-shim",
            "--out-dir",
            temp_dir.to_str().unwrap(),
        ])
        .status()
        .expect("execute avenc");

    assert!(
        status.success(),
        "avenc emit-c --aeric-shim --out-dir failed"
    );

    let model_c_path = temp_dir.join("model_server_capacity.c");
    let model_h_path = temp_dir.join("model_server_capacity.h");
    let shim_c_path = temp_dir.join("aven_shim.c");

    assert!(model_c_path.exists(), "model_server_capacity.c not created");
    assert!(model_h_path.exists(), "model_server_capacity.h not created");
    assert!(shim_c_path.exists(), "aven_shim.c not created");

    let c_content = fs::read_to_string(&model_c_path).expect("read c");
    let h_content = fs::read_to_string(&model_h_path).expect("read h");
    let shim_content = fs::read_to_string(&shim_c_path).expect("read shim");

    assert!(c_content.contains("aven_ProductionProfile"));
    assert!(
        !c_content.contains("int main(void)"),
        "library C file must not contain main()"
    );

    assert!(h_content.contains("MODEL_SERVER_CAPACITY_N_IN"));
    assert!(h_content.contains("MODEL_SERVER_CAPACITY_N_OUT"));
    assert!(h_content.contains("MODEL_SERVER_CAPACITY_SHA256"));

    assert!(shim_content.contains("aven_solve"));
    assert!(shim_content.contains("#include \"model_server_capacity.h\""));

    // Clean up temporary test directory
    let _ = fs::remove_dir_all(&temp_dir);
}
