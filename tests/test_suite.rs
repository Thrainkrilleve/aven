// tests/test_suite.rs — Comprehensive test suite for the Aven compiler toolchain

#[test]
fn test_lexer_and_parser() {
    let src = r#"
        // Comment
        relation Test {
            y = 2 * x + 5
        }
        solve Test with {
            x = 10
        }
    "#;
    let mut lexer = avenc::lexer::Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenize failed");
    let mut parser = avenc::parser::Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse failed");

    assert_eq!(module.blocks.len(), 1);
    assert_eq!(module.solves.len(), 1);
    assert_eq!(module.blocks[0].name, "Test");
    assert_eq!(module.blocks[0].equations.len(), 1);
    assert_eq!(module.blocks[0].equations[0].lhs, "y");
}

#[test]
fn test_symbolic_differentiation() {
    use avenc::ast::*;
    use avenc::diff::*;

    // d(x^2)/dx -> 2 * x
    let x_sq = Expr::Bin(
        BinaryOp::Pow,
        Box::new(Expr::Var("x".to_string())),
        Box::new(Expr::Num(2.0)),
    );
    let d = diff(&x_sq, "x").expect("diff failed");
    // Evaluate at x = 3: 2 * 3 = 6
    let mut env = std::collections::HashMap::new();
    env.insert("x".to_string(), 3.0);
    let val = eval_expr(&d, &env).expect("eval failed");
    assert!((val - 6.0).abs() < 1e-9);

    // d(sqrt(x))/dx at x = 4: 1 / (2 * sqrt(4)) = 0.25
    let sqrt_x = Expr::Call("sqrt".to_string(), Box::new(Expr::Var("x".to_string())));
    let d_sqrt = diff(&sqrt_x, "x").expect("diff failed");
    env.insert("x".to_string(), 4.0);
    let val_sqrt = eval_expr(&d_sqrt, &env).expect("eval failed");
    assert!((val_sqrt - 0.25).abs() < 1e-9);
}

#[test]
fn test_backward_1d_newton_solving() {
    use avenc::ast::*;
    use avenc::solver::SolverEngine;
    use std::collections::HashMap;

    let mut solver = SolverEngine::new();
    // depth = 0.5 * gravity * time^2
    let rhs = Expr::Bin(
        BinaryOp::Mul,
        Box::new(Expr::Bin(
            BinaryOp::Mul,
            Box::new(Expr::Num(0.5)),
            Box::new(Expr::Var("gravity".to_string())),
        )),
        Box::new(Expr::Bin(
            BinaryOp::Mul,
            Box::new(Expr::Var("time".to_string())),
            Box::new(Expr::Var("time".to_string())),
        )),
    );

    solver.add_relation(Equation {
        lhs: "depth".to_string(),
        op: ConstraintOp::Equal,
        rhs,
        original_text: "depth = 0.5 * gravity * time * time".to_string(),
        line: 1,
        col: 1,
    });

    let mut env = HashMap::new();
    env.insert("depth".to_string(), 45.0);
    env.insert("gravity".to_string(), 9.81);

    let solved = solver.solve(&env).expect("solve failed");
    let t = solved["time"];
    let analytic = (90.0 / 9.81f64).sqrt();
    assert!((t - analytic).abs() < 1e-4);
}

#[test]
fn test_coupled_multidimensional_newton_exact_jacobian() {
    use avenc::ast::*;
    use avenc::solver::SolverEngine;
    use std::collections::HashMap;

    // System:
    // x = y + 1
    // x = 2 * y
    // Exact solution: y = 1, x = 2
    let mut solver = SolverEngine::new();

    solver.add_relation(Equation {
        lhs: "x".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Add,
            Box::new(Expr::Var("y".to_string())),
            Box::new(Expr::Num(1.0)),
        ),
        original_text: "x = y + 1".to_string(),
        line: 1,
        col: 1,
    });

    solver.add_relation(Equation {
        lhs: "x".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Mul,
            Box::new(Expr::Num(2.0)),
            Box::new(Expr::Var("y".to_string())),
        ),
        original_text: "x = 2 * y".to_string(),
        line: 2,
        col: 1,
    });

    let env = HashMap::new();
    let solved = solver.solve(&env).expect("coupled solve failed");

    assert!((solved["x"] - 2.0).abs() < 1e-6);
    assert!((solved["y"] - 1.0).abs() < 1e-6);
}

#[test]
fn test_underdetermination_diagnostics() {
    use avenc::ast::*;
    use avenc::diagnostics::Diagnostician;
    use std::collections::HashMap;

    // Equation: revenue = price * units
    // Unknowns: price, units
    // Given: revenue = 800
    // Degrees of freedom = 1
    let eq = Equation {
        lhs: "revenue".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Mul,
            Box::new(Expr::Var("price".to_string())),
            Box::new(Expr::Var("units".to_string())),
        ),
        original_text: "revenue = price * units".to_string(),
        line: 1,
        col: 1,
    };

    let unknowns = vec!["price".to_string(), "units".to_string()];
    let mut env = HashMap::new();
    env.insert("revenue".to_string(), 800.0);

    let diag = Diagnostician::new().diagnose(&unknowns, &[eq], &env);

    assert_eq!(diag.dof, 1);
    assert_eq!(diag.rank, 1);
    assert!(!diag.minimal_fixes.is_empty());
}

#[test]
fn test_c_code_generation() {
    use avenc::ast::*;
    use avenc::codegen_c::CTranspiler;
    use std::collections::HashMap;

    let block = BlockDecl {
        kind: BlockKind::Forward,
        name: "Trip".to_string(),
        backend: BackendDecl::Native,
        equations: vec![
            Equation {
                lhs: "distance".to_string(),
                op: ConstraintOp::Equal,
                rhs: Expr::Bin(
                    BinaryOp::Mul,
                    Box::new(Expr::Var("speed".to_string())),
                    Box::new(Expr::Var("time".to_string())),
                ),
                original_text: "distance = speed * time".to_string(),
                line: 1,
                col: 1,
            },
            Equation {
                lhs: "fuel".to_string(),
                op: ConstraintOp::Equal,
                rhs: Expr::Bin(
                    BinaryOp::Mul,
                    Box::new(Expr::Var("distance".to_string())),
                    Box::new(Expr::Var("consumption".to_string())),
                ),
                original_text: "fuel = distance * consumption".to_string(),
                line: 2,
                col: 1,
            },
        ],
        line: 1,
        col: 1,
    };

    let mut binds = HashMap::new();
    binds.insert("speed".to_string(), 90.0);
    binds.insert("time".to_string(), 4.0);
    binds.insert("consumption".to_string(), 0.07);

    let c_code = CTranspiler::transpile_block(&block, &binds).expect("transpile failed");
    assert!(c_code.contains("static void aven_Trip"));
    assert!(c_code.contains("const double distance = (speed * time);"));
    assert!(c_code.contains("const double fuel = (distance * consumption);"));
}

#[test]
fn test_units_and_dimensions() {
    use avenc::units::{Dim, UnitRegistry};

    let reg = UnitRegistry::new();
    let km = reg.lookup("km").expect("km not found");
    let h = reg.lookup("h").expect("h not found");

    assert_eq!(km.dimension, Dim::LENGTH);
    assert_eq!(h.dimension, Dim::TIME);

    let speed_dim = km.dimension / h.dimension;
    assert_eq!(speed_dim.length, 1);
    assert_eq!(speed_dim.time, -1);

    // Systems Engineering: Little's Law dimensional algebra
    let req = reg.lookup("req").expect("req not found");
    let s = reg.lookup("s").expect("s not found");
    let ms = reg.lookup("ms").expect("ms not found");
    let kb = reg.lookup("kb").expect("kb not found");
    let gbps = reg.lookup("gbps").expect("gbps not found");
    let gib = reg.lookup("gib").expect("gib not found");

    // Binary vs Decimal data: 1 GiB = 1024^3 bytes, 1 GB = 1000^3 bytes
    let gb = reg.lookup("gb").expect("gb not found");
    assert_eq!(gib.dimension, Dim::DATA);
    assert_eq!(gb.dimension, Dim::DATA);
    assert!((gib.to_base / gb.to_base - 1.073741824).abs() < 1e-6);

    // Rate: req/s
    let req_rate_dim = req.dimension / s.dimension;
    // Concurrency = req/s * ms = transaction count
    let concurrency_dim = req_rate_dim * ms.dimension;
    assert_eq!(concurrency_dim, Dim::TRANSACTION);

    // Bandwidth = (req/s) * kb = DATA / TIME
    // In our model, payload is data per req: DATA / TRANSACTION
    let payload_dim = kb.dimension / req.dimension;
    let bandwidth_dim = req_rate_dim * payload_dim;
    assert_eq!(bandwidth_dim, gbps.dimension);
}

#[test]
fn test_symmetric_system_coupled_solving() {
    use avenc::ast::*;
    use avenc::solver::SolverEngine;
    use std::collections::HashMap;

    let mut solver = SolverEngine::new();
    // total = a * b
    solver.add_relation(Equation {
        lhs: "total".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Mul,
            Box::new(Expr::Var("a".to_string())),
            Box::new(Expr::Var("b".to_string())),
        ),
        original_text: "total = a * b".to_string(),
        line: 1,
        col: 1,
    });
    // other = a + b
    solver.add_relation(Equation {
        lhs: "other".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Add,
            Box::new(Expr::Var("a".to_string())),
            Box::new(Expr::Var("b".to_string())),
        ),
        original_text: "other = a + b".to_string(),
        line: 2,
        col: 1,
    });

    let mut env = HashMap::new();
    env.insert("total".to_string(), 6.0);
    env.insert("other".to_string(), 5.0);

    let solved = solver
        .solve(&env)
        .expect("Symmetric system must converge without singular Jacobian");
    let a = solved["a"];
    let b = solved["b"];
    assert!(
        ((a - 2.0).abs() < 1e-4 && (b - 3.0).abs() < 1e-4)
            || ((a - 3.0).abs() < 1e-4 && (b - 2.0).abs() < 1e-4)
    );
}

#[test]
fn test_backward_ceil_inversion() {
    use avenc::ast::*;
    use avenc::solver::SolverEngine;
    use std::collections::HashMap;

    let mut solver = SolverEngine::new();
    // allocated_workers = ceil(raw_workers / target_utilization)
    solver.add_relation(Equation {
        lhs: "allocated_workers".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Call(
            "ceil".to_string(),
            Box::new(Expr::Bin(
                BinaryOp::Div,
                Box::new(Expr::Var("raw_workers".to_string())),
                Box::new(Expr::Var("target_utilization".to_string())),
            )),
        ),
        original_text: "allocated_workers = ceil(raw_workers / target_utilization)".to_string(),
        line: 1,
        col: 1,
    });

    let mut env = HashMap::new();
    env.insert("allocated_workers".to_string(), 1429.0);
    env.insert("target_utilization".to_string(), 0.70);

    let solved = solver
        .solve(&env)
        .expect("Backward ceil inversion must succeed");
    let raw = solved["raw_workers"];
    // 1429 * 0.70 = 1000.3
    assert!((raw - 1000.3).abs() < 1e-4);
}

#[test]
fn test_abs_derivative_sign() {
    use avenc::ast::*;
    use avenc::diff::{diff, eval_expr};
    use std::collections::HashMap;

    let expr = Expr::Call("abs".to_string(), Box::new(Expr::Var("x".to_string())));
    let d_expr = diff(&expr, "x").expect("diff failed");

    let mut env_pos = HashMap::new();
    env_pos.insert("x".to_string(), 5.0);
    let slope_pos = eval_expr(&d_expr, &env_pos).expect("eval positive failed");
    assert_eq!(slope_pos, 1.0);

    let mut env_neg = HashMap::new();
    env_neg.insert("x".to_string(), -5.0);
    let slope_neg = eval_expr(&d_expr, &env_neg).expect("eval negative failed");
    assert_eq!(slope_neg, -1.0);
}

#[test]
fn test_sha256_spec_hashing() {
    use avenc::evaluator::{VarMap, sha256_hex};

    // Standard FIPS test vector for empty string
    let empty_hash = sha256_hex(b"");
    assert_eq!(
        empty_hash,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );

    // Deterministic 16-char spec hash from VarMap
    let mut vars = VarMap::new();
    vars.set("reqs_per_second", 50000.0);
    vars.set("avg_latency_ms", 20.0);

    let hash1 = vars.compute_hash();
    let hash2 = vars.compute_hash();
    assert_eq!(hash1, hash2);
    assert_eq!(hash1.len(), 16);
}

#[test]
fn test_inequality_and_objective_parsing() {
    use avenc::ast::*;
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;

    let src = r#"
    relation ServiceSizing {
        latency = 1000.0 / rps
    }

    solve ServiceSizing with {
        rps >= 50.0
        latency <= 20.0
        minimize(latency)
        maximize(rps)
    }
    "#;

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenize failed");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("Service").expect("parse failed");

    assert_eq!(module.blocks.len(), 1);
    assert_eq!(module.solves.len(), 1);

    let solve = &module.solves[0];
    assert_eq!(solve.constraints.len(), 2);
    assert_eq!(solve.constraints[0].lhs, "rps");
    assert_eq!(solve.constraints[0].op, ConstraintOp::GreaterEqual);
    assert_eq!(solve.constraints[1].lhs, "latency");
    assert_eq!(solve.constraints[1].op, ConstraintOp::LessEqual);

    assert_eq!(solve.objectives.len(), 2);
    assert_eq!(solve.objectives[0].kind, ObjectiveKind::Minimize);
    assert_eq!(solve.objectives[0].target, "latency");
    assert_eq!(solve.objectives[1].kind, ObjectiveKind::Maximize);
    assert_eq!(solve.objectives[1].target, "rps");
}

#[test]
fn test_evaluator_trait_and_native_backend() {
    use avenc::ast::*;
    use avenc::evaluator::{Evaluator, NativeEvaluator, VarMap};
    use std::sync::Arc;

    let eq = Equation {
        lhs: "distance".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Mul,
            Box::new(Expr::Var("speed".to_string())),
            Box::new(Expr::Var("time".to_string())),
        ),
        original_text: "distance = speed * time".to_string(),
        line: 1,
        col: 1,
    };

    let backend: Arc<dyn Evaluator> = Arc::new(NativeEvaluator::new(vec![eq]));
    assert_eq!(backend.name(), "native_analytic");

    let mut inputs = VarMap::new();
    inputs.set("speed", 100.0);
    inputs.set("time", 2.5);

    let res = backend.evaluate(&inputs).expect("evaluation failed");
    assert_eq!(res.values.get("distance").copied(), Some(250.0));
    assert_eq!(res.backend_id, "native_analytic");
}

#[test]
fn test_pareto_frontier_and_knee_point_detection() {
    use avenc::ast::*;
    use avenc::evaluator::{NativeEvaluator, Orchestrator, VarMap};
    use std::sync::Arc;

    // Little's Law capacity relation:
    // raw_workers = (reqs_per_second * avg_latency_ms) / 1000.0
    let eq = Equation {
        lhs: "raw_workers".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Div,
            Box::new(Expr::Bin(
                BinaryOp::Mul,
                Box::new(Expr::Var("reqs_per_second".to_string())),
                Box::new(Expr::Var("avg_latency_ms".to_string())),
            )),
            Box::new(Expr::Num(1000.0)),
        ),
        original_text: "raw_workers = (reqs_per_second * avg_latency_ms) / 1000.0".to_string(),
        line: 1,
        col: 1,
    };

    let evaluator = Arc::new(NativeEvaluator::new(vec![eq]));
    let orchestrator = Orchestrator::new(evaluator);

    let mut base_inputs = VarMap::new();
    base_inputs.set("avg_latency_ms", 20.0);

    // ε-Constraint Pareto sweep: sweep reqs_per_second from 10k to 50k in 4 steps
    let frontier = orchestrator
        .solve_pareto(
            &base_inputs,
            "reqs_per_second",
            (10000.0, 50000.0),
            4,
            &["reqs_per_second".to_string(), "raw_workers".to_string()],
        )
        .expect("Pareto sweep failed");

    assert_eq!(frontier.len(), 5);
    // At 10k: raw_workers = (10000 * 20)/1000 = 200
    assert_eq!(frontier[0].vars.get("raw_workers"), Some(200.0));
    // At 50k: raw_workers = (50000 * 20)/1000 = 1000
    assert_eq!(frontier[4].vars.get("raw_workers"), Some(1000.0));

    // Knee point detection
    let knee = Orchestrator::find_knee_point(&frontier);
    assert!(knee.is_some());
}

#[test]
fn test_external_evaluator_subprocess() {
    use avenc::evaluator::{Evaluator, ExternalEvaluator, VarMap};

    // Use python to mock an external simulator
    let py_cmd = if cfg!(windows) { "python" } else { "python3" };
    let py_script = "import sys, json; data = json.load(sys.stdin); print(json.dumps({'value': data.get('x', 0.0) * 3.0, 'backend_id': 'mock_external_sim'}))";

    let ext = ExternalEvaluator::new(py_cmd, &["-c", py_script]);
    assert_eq!(ext.name(), py_cmd);

    let mut inputs = VarMap::new();
    inputs.set("x", 14.0);

    let res = ext.evaluate(&inputs).expect("External evaluation failed");
    assert_eq!(res.values.get("value").copied(), Some(42.0));
    assert_eq!(res.backend_id, "mock_external_sim");
    assert!(res.solve_time_us > 0.0);
}

#[test]
fn test_external_evaluator_flame_contract_and_diagnostic_integrity() {
    use avenc::evaluator::{
        EvaluatorBackend, Orchestrator, OrchestratorConfig, SolveStatus, VarMap,
    };

    let py_cmd = if cfg!(windows) { "python" } else { "python3" };
    let script_path = "evaluations/tests/flame_eval.py";

    // Configure ExternalEvaluator behind OrchestratorConfig
    let config = OrchestratorConfig {
        backend: EvaluatorBackend::External {
            command: py_cmd.to_string(),
            args: vec![script_path.to_string()],
            backend_id: Some("flame_eval".to_string()),
            working_dir: None,
        },
        seed: 42,
    };

    let orchestrator = Orchestrator::from_config(&config, vec![]);
    assert_eq!(orchestrator.evaluator.name(), "flame_eval");

    // Case 1: Normal point -> Ok(value + backend_id)
    // Retry up to 3 times to absorb flame_eval.py's deliberate 5% random crash
    let mut pt1 = None;
    for _ in 0..3 {
        let mut inputs = VarMap::new();
        inputs.set("x", 0.5);
        inputs.set("y", -0.2);
        if let Ok(res) = orchestrator.solve_single(&inputs) {
            pt1 = Some(res);
            break;
        }
    }
    let pt1 = pt1.expect("Normal point solve failed");
    assert!(pt1.is_feasible);
    assert_eq!(pt1.diagnostics.backend_trace, vec!["flame_eval"]);
    assert_eq!(pt1.diagnostics.residual, 0.0);
    assert_eq!(pt1.diagnostics.status, SolveStatus::Converged);
    assert_eq!(pt1.diagnostics.free_vars, vec!["x", "y"]);
    assert_eq!(pt1.diagnostics.spec_hash.len(), 16);
    assert!(pt1.vars.get("value").is_some());

    // Case 2: Missing variable {"x": 1.0} -> clean failure with complete diagnostics intact
    let mut inputs_missing = VarMap::new();
    inputs_missing.set("x", 1.0);
    let expected_hash = inputs_missing.compute_hash();

    let fail_res = orchestrator.solve_single(&inputs_missing);
    assert!(fail_res.is_err());
    let failure = fail_res.unwrap_err();

    // Verify diagnostics on error path:
    assert_eq!(failure.diagnostics.spec_hash, expected_hash);
    assert_eq!(failure.diagnostics.free_vars, vec!["x"]);
    assert_eq!(failure.diagnostics.backend_trace, vec!["flame_eval"]);
    assert!(
        failure.diagnostics.residual.is_nan(),
        "Black-box failure residual must be NaN"
    );
    assert!(
        matches!(
            failure.diagnostics.status,
            SolveStatus::BlackBoxFailure(ref msg) if msg.contains("missing vars")
        ),
        "Expected missing vars message in SolveStatus, got: {:?}",
        failure.diagnostics.status
    );
    assert!(failure.diagnostics.solve_time_us > 0.0);

    // Verify solve_point also preserves diagnostic ledger
    let pt_missing = orchestrator.solve_point(&inputs_missing);
    assert!(!pt_missing.is_feasible);
    assert_eq!(pt_missing.diagnostics.spec_hash, expected_hash);
    assert!(pt_missing.diagnostics.residual.is_nan());

    // Case 3: Spike region {"x": 0.3, "y": -0.7} -> ~49.6
    let mut pt3 = None;
    for _ in 0..3 {
        let mut inputs = VarMap::new();
        inputs.set("x", 0.3);
        inputs.set("y", -0.7);
        if let Ok(res) = orchestrator.solve_single(&inputs) {
            pt3 = Some(res);
            break;
        }
    }
    let pt3 = pt3.expect("Spike point solve failed");
    let val3 = pt3.vars.get("value").unwrap();
    assert!(
        (48.0..51.5).contains(&val3),
        "Expected spike value ~49.6, got {}",
        val3
    );
    assert_eq!(pt3.diagnostics.backend_trace, vec!["flame_eval"]);

    // Case 4: Discontinuity wall {"x": 2.0, "y": 0.0} -> ~9.8
    let mut pt4 = None;
    for _ in 0..3 {
        let mut inputs = VarMap::new();
        inputs.set("x", 2.0);
        inputs.set("y", 0.0);
        if let Ok(res) = orchestrator.solve_single(&inputs) {
            pt4 = Some(res);
            break;
        }
    }
    let pt4 = pt4.expect("Discontinuity wall solve failed");
    let val4 = pt4.vars.get("value").unwrap();
    assert!(
        (9.0..11.0).contains(&val4),
        "Expected discontinuity wall value ~9.8, got {}",
        val4
    );
    assert_eq!(pt4.diagnostics.backend_trace, vec!["flame_eval"]);
}

#[test]
fn test_evaluator_config_flag_defaults_to_native_newton() {
    use avenc::ast::*;
    use avenc::evaluator::{EvaluatorBackend, Orchestrator, OrchestratorConfig, VarMap};

    // Verify default config backend is Native
    let default_config = OrchestratorConfig::default();
    assert_eq!(default_config.backend, EvaluatorBackend::Native);

    let eq = Equation {
        lhs: "y".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Mul,
            Box::new(Expr::Var("x".to_string())),
            Box::new(Expr::Num(2.0)),
        ),
        original_text: "y = x * 2.0".to_string(),
        line: 1,
        col: 1,
    };

    let orchestrator = Orchestrator::from_config(&default_config, vec![eq]);
    assert_eq!(orchestrator.evaluator.name(), "native_analytic");

    let mut inputs = VarMap::new();
    inputs.set("x", 21.0);
    let pt = orchestrator
        .solve_single(&inputs)
        .expect("Native solve failed");
    assert_eq!(pt.vars.get("y"), Some(42.0));
    assert_eq!(pt.diagnostics.backend_trace, vec!["native_analytic"]);
    assert_eq!(pt.diagnostics.residual, 0.0);
}

#[test]
fn test_first_class_external_relation_syntax_and_autobinding() {
    use avenc::ast::*;
    use avenc::evaluator::{EvaluatorBackend, Orchestrator, OrchestratorConfig, VarMap};
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;

    let src = r#"
        relation Flame using external("python", "evaluations/tests/flame_eval.py") {
            value = 0
        }
        solve Flame with {
            x = 0.5
            y = -0.2
        }
    "#;

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("Tokenize failed");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("test_flame").expect("Parse failed");

    assert_eq!(module.blocks.len(), 1);
    let blk = &module.blocks[0];
    assert_eq!(blk.name, "Flame");

    // Verify BackendDecl::External is parsed accurately
    match &blk.backend {
        BackendDecl::External { command, args } => {
            assert_eq!(command, "python");
            assert_eq!(args, &["evaluations/tests/flame_eval.py"]);
        }
        other => panic!("Expected BackendDecl::External, got {:?}", other),
    }

    // Auto-bind into OrchestratorConfig from AST
    let config = match &blk.backend {
        BackendDecl::External { command, args } => {
            let b_id = args.first().cloned().unwrap_or_else(|| command.clone());
            OrchestratorConfig {
                backend: EvaluatorBackend::External {
                    command: command.clone(),
                    args: args.clone(),
                    backend_id: Some(b_id),
                    working_dir: None,
                },
                seed: 42,
            }
        }
        BackendDecl::Native => OrchestratorConfig::default(),
    };

    let orchestrator = Orchestrator::from_config(&config, blk.equations.clone());
    assert_eq!(
        orchestrator.evaluator.name(),
        "evaluations/tests/flame_eval.py"
    );

    let mut inputs = VarMap::new();
    inputs.set("x", 0.5);
    inputs.set("y", -0.2);

    let mut pt = None;
    for _ in 0..3 {
        if let Ok(res) = orchestrator.solve_single(&inputs) {
            pt = Some(res);
            break;
        }
    }
    let pt = pt.expect("Solve with auto-bound external evaluator failed");
    assert!(pt.is_feasible);
    assert_eq!(pt.diagnostics.backend_trace, vec!["flame_eval"]);
    assert!(pt.vars.get("value").is_some());
}

#[test]
fn test_openplc_external_adapter_and_stream_varmap() {
    use avenc::evaluator::{ExternalEvaluator, Orchestrator, SolveStatus, VarMap};

    let script = "evaluations/tests/PLC-test/openplc_adapter.py";
    let ext = ExternalEvaluator::new("python", &[script]).with_backend_id("openplc");
    let orchestrator = Orchestrator::new(std::sync::Arc::new(ext));

    let mut inputs = VarMap::new();
    inputs.set("io_points", 30.0);
    inputs.set("cpu_load_pct", 40.0);

    let res = orchestrator
        .solve_single(&inputs)
        .expect("OpenPLC external adapter solve failed");
    assert!(res.is_feasible);
    assert_eq!(res.diagnostics.status, SolveStatus::Converged);
    assert_eq!(res.diagnostics.backend_trace, vec!["openplc"]);
    assert_eq!(res.diagnostics.residual, 0.0);
    assert_eq!(res.diagnostics.spec_hash.len(), 16);

    // Verify all industrial variables populated into VarMap
    let vars = &res.vars;
    assert!(vars.get("scan_ms").is_some());
    assert!(vars.get("power_w").is_some());
    assert!(vars.get("temp_c").is_some());
    assert!(vars.get("throughput_pph").is_some());
    assert!(vars.get("quality_pct").is_some());
    assert!(vars.get("alarm_count").is_some());
    assert_eq!(vars.get("io_points"), Some(30.0));
    assert_eq!(vars.get("cpu_load_pct"), Some(40.0));
    assert_eq!(vars.get("power_w"), Some(120.0 + 30.0 * 2.8)); // 204.0 W
}

#[test]
fn test_parser_inequality_constraints_with_numeric_bindings() {
    use avenc::ast::*;
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;

    let src = r#"
    relation IndustrialLine {
        quality = 99.8 - scan_ms * 0.05
    }

    solve IndustrialLine with {
        io_points = 28.0
        cpu_load = 35.0
        scan_ms <= 9.0
        alarm_count <= 0.0
    }
    "#;

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenize failed");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("TestModule").expect("parse failed");

    let solve = &module.solves[0];
    assert_eq!(solve.bindings.len(), 2);
    assert_eq!(solve.bindings[0].name, "io_points");
    assert_eq!(solve.bindings[1].name, "cpu_load");
    assert_eq!(solve.constraints.len(), 2);
    assert_eq!(solve.constraints[0].lhs, "scan_ms");
    assert_eq!(solve.constraints[0].op, ConstraintOp::LessEqual);
    assert_eq!(solve.constraints[1].lhs, "alarm_count");
    assert_eq!(solve.constraints[1].op, ConstraintOp::LessEqual);
}
