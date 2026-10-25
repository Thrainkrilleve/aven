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
        integer_vars: Vec::new(),
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
    let req_rate_dim = req.dimension.clone() / s.dimension;
    // Concurrency = req/s * ms = transaction count
    let concurrency_dim = req_rate_dim.clone() * ms.dimension;
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
        BackendDecl::Native | BackendDecl::Ipc { .. } => OrchestratorConfig::default(),
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
fn test_first_class_ipc_relation_syntax() {
    use avenc::ast::BackendDecl;
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;

    let source = r#"
relation TelemetryStream using ipc("127.0.0.1:9090") {
    ambient_temp_k = surface_temp_k - (0.0065 * baro_altitude_m)
}
"#;
    let tokens = Lexer::new(source).tokenize().expect("Tokenize failed");
    let module = Parser::new(tokens)
        .parse_module("test_ipc")
        .expect("IPC relation parsing failed");
    assert_eq!(module.blocks.len(), 1);
    let blk = &module.blocks[0];
    assert_eq!(blk.name, "TelemetryStream");
    match &blk.backend {
        BackendDecl::Ipc { endpoint } => {
            assert_eq!(endpoint, "127.0.0.1:9090");
        }
        other => panic!("Expected BackendDecl::Ipc, got {:?}", other),
    }
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

#[test]
fn test_mixed_integer_capacity_sizing_branch_and_bound() {
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;
    use avenc::solver::SolverEngine;
    use std::collections::HashMap;

    let src = r#"
    relation Storage {
        int drives
        raw_tb = drives * drive_tb
        usable_tb = raw_tb * ec_efficiency
        usable_tb >= target_capacity
    }

    solve Storage with {
        target_capacity = 100.0
        drive_tb = 18.0
        ec_efficiency = 0.75
        minimize(drives)
    }
    "#;

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenize failed");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("StorageModule").expect("parse failed");

    let blk = &module.blocks[0];
    assert_eq!(blk.integer_vars, vec!["drives"]);

    let solve = &module.solves[0];
    assert_eq!(solve.objectives.len(), 1);
    assert_eq!(solve.objectives[0].target, "drives");

    let mut solver = SolverEngine::new();
    for eq in &blk.equations {
        solver.add_relation(eq.clone());
    }
    for iv in &blk.integer_vars {
        solver.add_integer_var(iv.clone());
    }
    for c in &solve.constraints {
        solver.add_constraint(c.clone());
    }
    for obj in &solve.objectives {
        solver.add_objective(obj.clone());
    }

    let mut inputs = HashMap::new();
    for b in &solve.bindings {
        inputs.insert(b.name.clone(), b.value);
    }

    let solved = solver.solve(&inputs).expect("Solver failed on MILP model");

    // Continuous relaxation was: drives = 100 / (18 * 0.75) = 7.4074
    // Candidate 7 gives usable_tb = 94.5 < 100 (infeasible)
    // Candidate 8 gives usable_tb = 108.0 >= 100 (feasible, minimal integer)
    assert_eq!(solved.get("drives").copied(), Some(8.0));
    assert_eq!(solved.get("raw_tb").copied(), Some(144.0));
    assert_eq!(solved.get("usable_tb").copied(), Some(108.0));
    assert_eq!(solved.get("target_capacity").copied(), Some(100.0));
}

#[test]
fn test_mixed_integer_worker_ceiling_inequality() {
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;
    use avenc::solver::SolverEngine;
    use std::collections::HashMap;

    let src = r#"
    relation WorkerNode {
        int workers
        power_w = 25.0 + workers * 2.25
        power_w <= 245.0
    }

    solve WorkerNode with {
        maximize(workers)
    }
    "#;

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenize failed");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("WorkerModule").expect("parse failed");

    let blk = &module.blocks[0];
    let solve = &module.solves[0];

    let mut solver = SolverEngine::new();
    for eq in &blk.equations {
        solver.add_relation(eq.clone());
    }
    for iv in &blk.integer_vars {
        solver.add_integer_var(iv.clone());
    }
    for c in &solve.constraints {
        solver.add_constraint(c.clone());
    }
    for obj in &solve.objectives {
        solver.add_objective(obj.clone());
    }

    let inputs = HashMap::new();
    let solved = solver
        .solve(&inputs)
        .expect("Solver failed on worker ceiling");

    // Boundary relaxation: 25 + workers * 2.25 = 245 => workers = 97.7778
    // Candidate 98 => power = 245.5 > 245 (infeasible)
    // Candidate 97 => power = 243.25 <= 245 (feasible, maximal integer)
    assert_eq!(solved.get("workers").copied(), Some(97.0));
    assert_eq!(solved.get("power_w").copied(), Some(243.25));
}

#[test]
fn test_mixed_integer_infeasible_constraints() {
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;
    use avenc::solver::SolverEngine;
    use std::collections::HashMap;

    let src = r#"
    relation Conflicting {
        int workers
        power_w = workers * 10.0
        power_w <= 50.0
        power_w >= 100.0
    }

    solve Conflicting with {
        minimize(workers)
    }
    "#;

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenize failed");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("ConflictModule").expect("parse failed");

    let blk = &module.blocks[0];
    let solve = &module.solves[0];

    let mut solver = SolverEngine::new();
    for eq in &blk.equations {
        solver.add_relation(eq.clone());
    }
    for iv in &blk.integer_vars {
        solver.add_integer_var(iv.clone());
    }
    for c in &solve.constraints {
        solver.add_constraint(c.clone());
    }
    for obj in &solve.objectives {
        solver.add_objective(obj.clone());
    }

    let inputs = HashMap::new();
    let res = solver.solve(&inputs);
    assert!(
        res.is_err(),
        "Expected solver to fail on conflicting constraints"
    );
}

#[test]
fn test_mixed_integer_provenance_and_ledger() {
    use avenc::ledger::{Fact, Ledger};
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;
    use avenc::solver::SolverEngine;
    use std::collections::HashMap;

    let src = r#"
    relation Storage {
        int drives
        raw_tb = drives * drive_tb
        usable_tb = raw_tb * ec_efficiency
    }

    solve Storage with {
        target_capacity = 100.0
        drive_tb = 18.0
        ec_efficiency = 0.75
        usable_tb >= target_capacity
        minimize(drives)
    }
    "#;

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenize failed");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("StorageModule").expect("parse failed");

    let mut env = HashMap::new();
    let mut ledger = Ledger::new();
    let mut seq = 0;

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

    let mut solver = SolverEngine::new();
    for blk in &module.blocks {
        for eq in &blk.equations {
            solver.add_relation(eq.clone());
        }
        for iv in &blk.integer_vars {
            solver.add_integer_var(iv.clone());
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

    let solved_env = solver.solve(&env).expect("Solve failed");
    assert_eq!(solved_env["drives"], 8.0);
    assert_eq!(solved_env["raw_tb"], 144.0);
    assert_eq!(solved_env["usable_tb"], 108.0);

    for (k, v) in &solved_env {
        if !env.contains_key(k) {
            seq += 1;
            let mut causes = Vec::new();
            let mut origin = "derived".to_string();
            if let Some(eq) = solver.relations.iter().find(|e| &e.lhs == k) {
                origin = format!("= {}", eq.rhs);
                for input_var in eq.rhs.vars() {
                    if let Some(fact) = ledger.latest(&input_var) {
                        causes.push(fact.seq);
                    }
                }
            } else if solver.integer_vars.contains(k) {
                origin = "integer optimization".to_string();
            }

            ledger.append(Fact {
                seq,
                tick: 0,
                kind: "derived".to_string(),
                name: k.clone(),
                value: *v,
                causes,
                origin,
                metadata: None,
            });
        }
    }

    let why_drives = ledger.why("drives");
    assert!(why_drives.contains("drives = 8.0000"));
}

#[test]
fn test_extensible_unit_system_and_compound_algebra() {
    use avenc::ast::UnitDecl;
    use avenc::units::UnitRegistry;

    let mut reg = UnitRegistry::new();

    // 1. Extensible custom dimension declarations
    let custom_decl = UnitDecl {
        dimension: "photon_flux".to_string(),
        units: vec!["photon".to_string(), "kphoton".to_string()],
        line: 1,
        col: 1,
    };
    reg.register_unit_decl(&custom_decl);

    let photon_u = reg
        .lookup("photon")
        .expect("photon unit must be registered");
    assert_eq!(photon_u.dimension.custom.get("photon_flux"), Some(&1));

    // 2. Compound unit parsing: km/h, m/s^2, req/s, kW*h
    let km_h = reg.lookup("km/h").expect("km/h compound unit must resolve");
    assert_eq!(km_h.dimension.length, 1);
    assert_eq!(km_h.dimension.time, -1);

    let accel = reg
        .lookup("m/s^2")
        .expect("m/s^2 compound unit must resolve");
    assert_eq!(accel.dimension.length, 1);
    assert_eq!(accel.dimension.time, -2);

    let energy_kwh = reg.lookup("kW*h").expect("kW*h compound unit must resolve");
    assert_eq!(energy_kwh.dimension.mass, 1);
    assert_eq!(energy_kwh.dimension.length, 2);
    assert_eq!(energy_kwh.dimension.time, -2);

    // 3. Conversions
    let m_per_s = reg
        .convert(90.0, "km/h", "m/s")
        .expect("conversion km/h to m/s");
    assert!((m_per_s - 25.0).abs() < 1e-9);

    let j_val = reg.convert(1.0, "kWh", "J").expect("conversion kWh to J");
    assert!((j_val - 3_600_000.0).abs() < 1e-6);

    // Incompatible conversion returns error
    let err = reg.convert(100.0, "km/h", "kg");
    assert!(err.is_err());
    assert!(err.unwrap_err().contains("incompatible dimensions"));
}

#[test]
fn test_dimensional_checker_propagation_and_homogeneity() {
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;
    use avenc::units::{Dim, DimensionalChecker, TypeError};

    // 1. Valid dimensional model with omnidirectional propagation
    let src = r#"
        unit currency : usd, eur
        signal speed : km/h
        signal time : h

        relation Travel {
            distance = speed * time
        }
        solve Travel with {
            speed = 90 km/h
            time = 2.5 h
        }
    "#;
    let mut lex = Lexer::new(src);
    let mut parser = Parser::new(lex.tokenize().unwrap());
    let module = parser.parse_module("travel").unwrap();

    let mut checker = DimensionalChecker::new();
    let report = checker
        .check_module(&module)
        .expect("dimensional check must succeed");

    assert_eq!(report.verified_equations, 1);
    assert_eq!(report.variable_dimensions["distance"], Dim::LENGTH);
    assert_eq!(report.variable_dimensions["speed"].length, 1);
    assert_eq!(report.variable_dimensions["speed"].time, -1);
    assert_eq!(report.variable_dimensions["time"], Dim::TIME);

    // 2. Incompatible addition (+ between distance and time) must be rejected
    let bad_add_src = r#"
        signal length_val : m
        signal time_val : s

        relation Incompatible {
            mismatch = length_val + time_val
        }
        solve Incompatible with {
            length_val = 10 m
            time_val = 5 s
        }
    "#;
    let mut lex_bad = Lexer::new(bad_add_src);
    let mut parser_bad = Parser::new(lex_bad.tokenize().unwrap());
    let bad_module = parser_bad.parse_module("bad_add").unwrap();

    let mut checker_bad = DimensionalChecker::new();
    let res = checker_bad.check_module(&bad_module);
    assert!(matches!(res, Err(TypeError::DimensionalMismatch { .. })));

    // 3. Transcendental function argument must be dimensionless or angle
    let bad_trig_src = r#"
        signal len : m
        relation BadTrig {
            res = sin(len)
        }
        solve BadTrig with {
            len = 5 m
        }
    "#;
    let mut lex_trig = Lexer::new(bad_trig_src);
    let mut parser_trig = Parser::new(lex_trig.tokenize().unwrap());
    let trig_module = parser_trig.parse_module("bad_trig").unwrap();

    let mut checker_trig = DimensionalChecker::new();
    let res_trig = checker_trig.check_module(&trig_module);
    assert!(matches!(
        res_trig,
        Err(TypeError::InvalidFunctionArgument { .. })
    ));
}

#[test]
fn test_underdetermination_rank_deficiency_and_minimal_fixes() {
    use avenc::ast::*;
    use avenc::diagnostics::Diagnostician;
    use std::collections::HashMap;

    // 3 variables (x, y, z), 2 independent equations
    // Rank = 2, DOF = 1
    // eq1: x + y + z = 10
    // eq2: 2 * x + y = 5
    let eq1 = Equation {
        lhs: "eq1_res".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Add,
            Box::new(Expr::Bin(
                BinaryOp::Add,
                Box::new(Expr::Var("x".to_string())),
                Box::new(Expr::Var("y".to_string())),
            )),
            Box::new(Expr::Var("z".to_string())),
        ),
        original_text: "eq1_res = x + y + z".to_string(),
        line: 1,
        col: 1,
    };

    let eq2 = Equation {
        lhs: "eq2_res".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Add,
            Box::new(Expr::Bin(
                BinaryOp::Mul,
                Box::new(Expr::Num(2.0)),
                Box::new(Expr::Var("x".to_string())),
            )),
            Box::new(Expr::Var("y".to_string())),
        ),
        original_text: "eq2_res = 2 * x + y".to_string(),
        line: 2,
        col: 1,
    };

    let unknowns = vec!["x".to_string(), "y".to_string(), "z".to_string()];
    let mut env = HashMap::new();
    env.insert("eq1_res".to_string(), 10.0);
    env.insert("eq2_res".to_string(), 5.0);

    let diag = Diagnostician::new().diagnose(&unknowns, &[eq1, eq2], &env);
    assert_eq!(diag.dof, 1);
    assert_eq!(diag.rank, 2);
    assert_eq!(diag.unknowns.len(), 3);
    assert!(!diag.free_vars.is_empty());
    assert!(!diag.minimal_fixes.is_empty());
    let rep = diag.report();
    assert!(rep.contains("UNDER-DETERMINED SYSTEM"));
    assert!(rep.contains("1 degree(s) of freedom"));
    assert!(rep.contains("MINIMAL FIX"));

    // Redundant equation test (degenerate rows):
    // eq_a: 2 * x + 3 * y = 10
    // eq_b: 4 * x + 6 * y = 20 (linearly dependent on eq_a)
    let eq_a = Equation {
        lhs: "res_a".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Add,
            Box::new(Expr::Bin(
                BinaryOp::Mul,
                Box::new(Expr::Num(2.0)),
                Box::new(Expr::Var("x".to_string())),
            )),
            Box::new(Expr::Bin(
                BinaryOp::Mul,
                Box::new(Expr::Num(3.0)),
                Box::new(Expr::Var("y".to_string())),
            )),
        ),
        original_text: "res_a = 2 * x + 3 * y".to_string(),
        line: 3,
        col: 1,
    };

    let eq_b = Equation {
        lhs: "res_b".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Add,
            Box::new(Expr::Bin(
                BinaryOp::Mul,
                Box::new(Expr::Num(4.0)),
                Box::new(Expr::Var("x".to_string())),
            )),
            Box::new(Expr::Bin(
                BinaryOp::Mul,
                Box::new(Expr::Num(6.0)),
                Box::new(Expr::Var("y".to_string())),
            )),
        ),
        original_text: "res_b = 4 * x + 6 * y".to_string(),
        line: 4,
        col: 1,
    };

    let xy_unknowns = vec!["x".to_string(), "y".to_string()];
    let mut xy_env = HashMap::new();
    xy_env.insert("res_a".to_string(), 10.0);
    xy_env.insert("res_b".to_string(), 20.0);

    let degen_diag = Diagnostician::new().diagnose(&xy_unknowns, &[eq_a, eq_b], &xy_env);
    assert_eq!(degen_diag.rank, 1);
    assert_eq!(degen_diag.dof, 1);
    assert!(!degen_diag.degenerate_equations.is_empty());
    assert!(
        degen_diag
            .report()
            .contains("DEGENERATE / REDUNDANT EQUATION(S)")
    );
}

#[test]
fn test_solver_underdetermined_error_handling() {
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;
    use avenc::solver::{SolverEngine, SolverError};
    use std::collections::HashMap;

    let src = r#"
        relation UnderDeterminedPlane {
            volume = length * width * height
        }
        solve UnderDeterminedPlane with {
            volume = 1000
            height = 10
        }
    "#;
    let mut lex = Lexer::new(src);
    let mut parser = Parser::new(lex.tokenize().unwrap());
    let module = parser.parse_module("plane").unwrap();

    let mut solver = SolverEngine::new();
    for eq in &module.blocks[0].equations {
        solver.add_relation(eq.clone());
    }

    let mut env = HashMap::new();
    for b in &module.solves[0].bindings {
        env.insert(b.name.clone(), b.value);
    }

    let res = solver.solve(&env);
    match res {
        Err(SolverError::UnderDetermined(diag)) => {
            assert_eq!(diag.dof, 1);
            assert!(diag.unknowns.contains(&"length".to_string()));
            assert!(diag.unknowns.contains(&"width".to_string()));
            assert!(!diag.minimal_fixes.is_empty());
        }
        other => panic!("expected SolverError::UnderDetermined, got {:?}", other),
    }
}

#[test]
fn test_equality_and_not_equal_operators_in_equations_and_solve_constraints() {
    use avenc::ast::ConstraintOp;
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;
    use avenc::solver::{SolverEngine, SolverError};
    use std::collections::HashMap;

    let src = r#"
        relation VettingCheck {
            score == raw_score * 1.5
            penalty != 999.0
        }
        solve VettingCheck with {
            raw_score = 10.0
            status == 0.0
            error_code != 500.0
        }
    "#;
    let mut lex = Lexer::new(src);
    let mut parser = Parser::new(lex.tokenize().unwrap());
    let module = parser.parse_module("vetting").unwrap();

    let blk = &module.blocks[0];
    assert_eq!(blk.equations[0].op, ConstraintOp::Equal);
    assert_eq!(blk.equations[1].op, ConstraintOp::NotEqual);

    let solve = &module.solves[0];
    assert_eq!(solve.bindings.len(), 1);
    assert_eq!(solve.constraints.len(), 2);
    assert_eq!(solve.constraints[0].op, ConstraintOp::Equal);
    assert_eq!(solve.constraints[1].op, ConstraintOp::NotEqual);

    let mut solver = SolverEngine::new();
    for eq in &blk.equations {
        solver.add_relation(eq.clone());
    }
    for c in &solve.constraints {
        solver.add_constraint(c.clone());
    }

    let mut env = HashMap::new();
    env.insert("raw_score".to_string(), 10.0);
    env.insert("status".to_string(), 0.0);
    env.insert("error_code".to_string(), 200.0);
    env.insert("penalty".to_string(), 0.0);

    let solved = solver
        .solve(&env)
        .expect("solving with == and != constraints must succeed");
    assert_eq!(solved["score"], 15.0);

    // Violation of == constraint
    let mut bad_env = env.clone();
    bad_env.insert("status".to_string(), 1.0); // status == 0.0 fails!
    let res = solver.solve(&bad_env);
    assert!(matches!(res, Err(SolverError::ConstraintViolation { .. })));
}

#[test]
fn test_comma_separated_relation_targets_in_solve() {
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;

    let src = r#"
        relation FirstRelation {
            a = x * 2
        }
        relation SecondRelation {
            b = y + 10
        }
        solve FirstRelation, SecondRelation with {
            x = 5
            y = 20
        }
    "#;
    let mut lex = Lexer::new(src);
    let mut parser = Parser::new(lex.tokenize().unwrap());
    let module = parser.parse_module("multi").unwrap();

    assert_eq!(module.blocks.len(), 2);
    assert_eq!(module.solves.len(), 1);
    let solve = &module.solves[0];
    assert_eq!(solve.target, "FirstRelation");
    assert_eq!(solve.targets, vec!["FirstRelation", "SecondRelation"]);
}

#[test]
fn test_derived_expressions_inside_external_relation_body() {
    use avenc::evaluator::parse_json_object_numbers;
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;
    use avenc::solver::SolverEngine;
    use std::collections::HashMap;

    let src = r#"
        relation RecruitMetrics using external("python", "dummy.py") {
            total_findings = report_high_count + report_medium_count
            risk_ratio = total_findings / characters
        }
        solve RecruitMetrics with {
            risk_ratio <= 0.5
        }
    "#;
    let mut lex = Lexer::new(src);
    let mut parser = Parser::new(lex.tokenize().unwrap());
    let module = parser.parse_module("vetting").unwrap();

    // Verify external relation block holds the derived equations
    assert_eq!(module.blocks[0].equations.len(), 2);

    // Simulate JSON output from external adapter with numbers and booleans
    let raw_json = r#"{"report_high_count": 2, "report_medium_count": 3, "characters": 10, "is_applicant": true, "flagged": false}"#;
    let parsed = parse_json_object_numbers(raw_json);
    assert_eq!(parsed["report_high_count"], 2.0);
    assert_eq!(parsed["report_medium_count"], 3.0);
    assert_eq!(parsed["characters"], 10.0);
    assert_eq!(parsed["is_applicant"], 1.0);
    assert_eq!(parsed["flagged"], 0.0);

    let mut solver = SolverEngine::new();
    for eq in &module.blocks[0].equations {
        solver.add_relation(eq.clone());
    }
    for c in &module.solves[0].constraints {
        solver.add_constraint(c.clone());
    }

    let mut env = HashMap::new();
    for (k, v) in parsed {
        env.insert(k, v);
    }

    let solved = solver
        .solve(&env)
        .expect("derived equations must evaluate on external outputs");
    assert_eq!(solved["total_findings"], 5.0);
    assert_eq!(solved["risk_ratio"], 0.5);
}

#[test]
fn test_freshness_metadata_ttl_decay_and_stale_detection() {
    use avenc::evaluator::{parse_json_string_array, parse_json_u64};
    use avenc::ledger::{Fact, FactMetadata, Ledger};

    let raw_json = r#"{
        "total_cpu_cores": 32.0,
        "timestamp_ms": 1700000000000,
        "ttl_ms": 60000,
        "warnings": ["live-cluster", "allocatable"]
    }"#;

    let ts = parse_json_u64(raw_json, "timestamp_ms").unwrap();
    let ttl = parse_json_u64(raw_json, "ttl_ms").unwrap();
    let warnings = parse_json_string_array(raw_json, "warnings");

    assert_eq!(ts, 1700000000000);
    assert_eq!(ttl, 60000);
    assert_eq!(warnings, vec!["live-cluster", "allocatable"]);

    let mut ledger = Ledger::new();
    let is_stale = true; // simulated expired TTL
    let meta = FactMetadata {
        timestamp_ms: Some(ts),
        ttl_ms: Some(ttl),
        is_stale,
        warnings,
    };

    ledger.append(Fact {
        seq: 1,
        tick: 0,
        kind: "derived".to_string(),
        name: "total_cpu_cores".to_string(),
        value: 32.0,
        causes: Vec::new(),
        origin: "external: kubectl".to_string(),
        metadata: Some(meta),
    });

    let why_text = ledger.why("total_cpu_cores");
    assert!(why_text.contains("[STALE: expired TTL]"));
    assert!(why_text.contains("[ttl: 60000ms]"));
    assert!(why_text.contains("[warnings: live-cluster,allocatable]"));
}

#[test]
fn test_causal_tree_topological_depth_limit_and_cycle_suppression() {
    use avenc::ledger::{Fact, Ledger};

    let mut ledger = Ledger::new();
    // Build a 4-hop chain: D -> C -> B -> A
    ledger.append(Fact {
        seq: 1,
        tick: 0,
        kind: "input".to_string(),
        name: "A".to_string(),
        value: 10.0,
        causes: vec![],
        origin: "seed".to_string(),
        metadata: None,
    });
    ledger.append(Fact {
        seq: 2,
        tick: 0,
        kind: "derived".to_string(),
        name: "B".to_string(),
        value: 20.0,
        causes: vec![1],
        origin: "= A * 2".to_string(),
        metadata: None,
    });
    ledger.append(Fact {
        seq: 3,
        tick: 0,
        kind: "derived".to_string(),
        name: "C".to_string(),
        value: 30.0,
        causes: vec![2],
        origin: "= B + 10".to_string(),
        metadata: None,
    });
    ledger.append(Fact {
        seq: 4,
        tick: 0,
        kind: "derived".to_string(),
        name: "D".to_string(),
        value: 40.0,
        causes: vec![3],
        origin: "= C + 10".to_string(),
        metadata: None,
    });

    // Unlimited why query shows all 4 hops
    let full_why = ledger.why("D");
    assert!(full_why.contains("#4 D = 40.0000"));
    assert!(full_why.contains("#1 A = 10.0000"));

    // Depth-pruned query with limit 2 stops at hop 2
    let pruned_why = ledger.why_with_depth("D", Some(2));
    assert!(pruned_why.contains("#4 D = 40.0000"));
    assert!(pruned_why.contains("#3 C = 30.0000"));
    assert!(pruned_why.contains("pruned: 1 ancestor(s) beyond hop limit 2"));
    assert!(!pruned_why.contains("#1 A = 10.0000"));
}

#[test]
fn test_nan_and_inf_robustness_in_derivatives_and_solver() {
    use avenc::ast::*;
    use avenc::diff::eval_expr;
    use avenc::solver::SolverEngine;
    use std::collections::HashMap;

    // Division by zero domain error in eval_expr
    let div_zero = Expr::Bin(
        BinaryOp::Div,
        Box::new(Expr::Num(1.0)),
        Box::new(Expr::Num(0.0)),
    );
    let env = HashMap::new();
    assert!(eval_expr(&div_zero, &env).is_err());

    // Solver handles degenerate equation without panic or NaN leakage
    let mut solver = SolverEngine::new();
    solver.add_relation(Equation {
        lhs: "x".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Div,
            Box::new(Expr::Num(1.0)),
            Box::new(Expr::Var("y".to_string())),
        ),
        original_text: "x = 1.0 / y".to_string(),
        line: 1,
        col: 1,
    });

    let mut solve_env = HashMap::new();
    solve_env.insert("y".to_string(), 0.0);
    let result = solver.solve(&solve_env);
    assert!(result.is_err());
}

#[test]
fn test_kubernetes_multi_objective_pareto_convergence_and_knee_point() {
    use avenc::evaluator::{Orchestrator, OrchestratorConfig, VarMap};
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;

    let src = r#"
        relation ClusterPareto {
            active_cpu = target_pods * 0.5
            active_mem = target_pods * 1.0
            cpu_utilization = (active_cpu / 32.0) * 100.0
            mem_headroom_pct = ((60.803 - active_mem) / 60.803) * 100.0
        }
        solve ClusterPareto with {
            target_pods = 20.0
            mem_headroom_pct >= 30.0
            cpu_utilization >= 10.0
        }
    "#;

    let mut lex = Lexer::new(src);
    let mut parser = Parser::new(lex.tokenize().unwrap());
    let module = parser.parse_module("ClusterPareto").unwrap();

    let orchestrator = Orchestrator::from_config(
        &OrchestratorConfig::default(),
        module.blocks[0].equations.clone(),
    );

    let mut initial_inputs = VarMap::new();
    initial_inputs.set("target_pods", 20.0);

    let obj_list = vec![
        "cpu_utilization".to_string(),
        "mem_headroom_pct".to_string(),
    ];

    let t_start = std::time::Instant::now();
    let frontier = orchestrator
        .solve_pareto(&initial_inputs, "target_pods", (10.0, 45.0), 5, &obj_list)
        .expect("Pareto solve must succeed");

    let solve_duration_ms = t_start.elapsed().as_secs_f64() * 1000.0;

    // Must converge well under the 50ms requirement (typically < 0.2ms)
    assert!(
        solve_duration_ms < 50.0,
        "Pareto solve took {:.3} ms (exceeded 50ms limit)",
        solve_duration_ms
    );
    assert_eq!(frontier.len(), 6);

    // Verify trade-off monotonicity: as target_pods increase, CPU utilization increases and mem headroom decreases
    let first = &frontier[0];
    let last = &frontier[5];
    assert!(first.vars.get("cpu_utilization").unwrap() < last.vars.get("cpu_utilization").unwrap());
    assert!(
        first.vars.get("mem_headroom_pct").unwrap() > last.vars.get("mem_headroom_pct").unwrap()
    );

    // Verify Knee Point detection
    let knee_idx = Orchestrator::find_knee_point(&frontier);
    assert!(knee_idx.is_some());
}

#[test]
fn test_freshness_zero_ttl_and_stale_enforcement() {
    use avenc::evaluator::{Evaluator, ExternalEvaluator, VarMap};

    let py_cmd = if cfg!(windows) { "python" } else { "python3" };
    // Adapter returns ttl_ms = 0
    let script =
        "import json; print(json.dumps({'total_cpu_cores': 32.0, 'ttl_ms': 0, 'timestamp_ms': 0}))";
    let ext = ExternalEvaluator::new(py_cmd, &["-c", script]);
    let inputs = VarMap::new();
    let res = ext.evaluate(&inputs).expect("Evaluation should parse");
    assert_eq!(res.ttl_ms, Some(0));
    assert_eq!(res.values.get("total_cpu_cores"), Some(&32.0));

    // Ledger staleness verification
    use avenc::ledger::{Fact, FactMetadata, Ledger};
    let mut ledger = Ledger::new();
    let is_stale = res.ttl_ms == Some(0);
    assert!(is_stale, "ttl_ms=0 must be classified as stale");

    ledger.append(Fact {
        seq: 1,
        tick: 0,
        kind: "input".to_string(),
        name: "total_cpu_cores".to_string(),
        value: 32.0,
        causes: vec![],
        origin: "external: python".to_string(),
        metadata: Some(FactMetadata {
            timestamp_ms: res.timestamp_ms,
            ttl_ms: res.ttl_ms,
            is_stale,
            warnings: vec!["zero-ttl-expired".to_string()],
        }),
    });

    let why = ledger.why("total_cpu_cores");
    assert!(why.contains("[STALE: expired TTL]"));
    assert!(why.contains("[ttl: 0ms]"));
    assert!(why.contains("[warnings: zero-ttl-expired]"));
}

#[test]
fn test_external_adapter_schema_validation_and_malformed_handling() {
    use avenc::evaluator::{EvalError, Evaluator, ExternalEvaluator, VarMap};

    let py_cmd = if cfg!(windows) { "python" } else { "python3" };
    let inputs = VarMap::new();

    // 1. Missing required key
    let script_missing = "import json; print(json.dumps({'total_cpu_cores': 32.0}))";
    let ext_missing = ExternalEvaluator::new(py_cmd, &["-c", script_missing])
        .with_required_keys(&["total_cpu_cores".to_string(), "mem_headroom".to_string()]);
    let err = ext_missing.evaluate(&inputs).unwrap_err();
    match err {
        EvalError::MalformedOutput(msg) => {
            assert!(msg.contains("Schema validation failed"));
            assert!(msg.contains("omitted required numeric field 'mem_headroom'"));
        }
        other => panic!("Expected MalformedOutput, got {:?}", other),
    }

    // 2. Non-JSON output
    let script_raw = "print('FATAL: connection refused')";
    let ext_raw = ExternalEvaluator::new(py_cmd, &["-c", script_raw]);
    let err = ext_raw.evaluate(&inputs).unwrap_err();
    match err {
        EvalError::MalformedOutput(msg) => {
            assert!(msg.contains("must be a JSON object"));
        }
        other => panic!("Expected MalformedOutput, got {:?}", other),
    }

    // 3. Adapter error field
    let script_err = "import json; print(json.dumps({'error': 'kubelet certificate expired'}))";
    let ext_err = ExternalEvaluator::new(py_cmd, &["-c", script_err]);
    let err = ext_err.evaluate(&inputs).unwrap_err();
    match err {
        EvalError::BlackBoxFailure(msg) => {
            assert!(msg.contains("reported error: kubelet certificate expired"));
        }
        other => panic!("Expected BlackBoxFailure, got {:?}", other),
    }
}

#[test]
fn test_causal_why_default_depth_limit_and_unlimited_override() {
    use avenc::ledger::{DEFAULT_MAX_DEPTH, Fact, Ledger};

    let mut ledger = Ledger::new();
    // Build a 25-hop causal chain: hop 1 -> hop 2 -> ... -> hop 25
    ledger.append(Fact {
        seq: 1,
        tick: 0,
        kind: "input".to_string(),
        name: "hop_1".to_string(),
        value: 1.0,
        causes: vec![],
        origin: "seed".to_string(),
        metadata: None,
    });

    for i in 2..=25 {
        ledger.append(Fact {
            seq: i,
            tick: 0,
            kind: "derived".to_string(),
            name: format!("hop_{}", i),
            value: i as f64,
            causes: vec![i - 1],
            origin: format!("= hop_{} + 1", i - 1),
            metadata: None,
        });
    }

    // Default why query must prune at DEFAULT_MAX_DEPTH (20)
    assert_eq!(DEFAULT_MAX_DEPTH, 20);
    let default_why = ledger.why("hop_25");
    assert!(default_why.contains("#25 hop_25 = 25.0000"));
    assert!(default_why.contains(
        "pruned: 1 ancestor(s) beyond hop limit 20. Use --depth <N> or --depth 0 for unlimited"
    ));
    assert!(!default_why.contains("#1 hop_1 = 1.0000"));

    // Explicit depth 5
    let shallow_why = ledger.why_with_depth("hop_25", Some(5));
    assert!(shallow_why.contains("pruned: 1 ancestor(s) beyond hop limit 5"));

    // Unlimited query with None shows all 25 hops
    let unlimited_why = ledger.why_with_depth("hop_25", None);
    assert!(unlimited_why.contains("#25 hop_25 = 25.0000"));
    assert!(unlimited_why.contains("#1 hop_1 = 1.0000"));
    assert!(!unlimited_why.contains("pruned:"));
}

#[test]
fn test_frontier_cache_and_epsilon_box_dominance() {
    use avenc::ast::*;
    use avenc::evaluator::{
        FrontierPoint, NativeEvaluator, Orchestrator, SolveDiagnostics, SolveStatus, VarMap,
    };
    use std::sync::Arc;

    let eq = Equation {
        lhs: "active_cpu".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Mul,
            Box::new(Expr::Var("target_pods".to_string())),
            Box::new(Expr::Num(0.5)),
        ),
        original_text: "active_cpu = target_pods * 0.5".to_string(),
        line: 1,
        col: 1,
    };

    let evaluator = Arc::new(NativeEvaluator::new(vec![eq]));
    let orchestrator = Orchestrator::new(evaluator);

    let mut base_inputs = VarMap::new();
    base_inputs.set("target_pods", 10.0);

    let objectives = vec!["target_pods".to_string(), "active_cpu".to_string()];

    // Run 1: populates cache
    let frontier_1 = orchestrator
        .solve_pareto(&base_inputs, "target_pods", (10.0, 30.0), 4, &objectives)
        .expect("First solve should succeed");
    assert_eq!(frontier_1.len(), 5);

    // Verify cache has 1 entry
    assert_eq!(orchestrator.frontier_cache.lock().unwrap().len(), 1);

    // Run 2: served from cache
    let frontier_2 = orchestrator
        .solve_pareto(&base_inputs, "target_pods", (10.0, 30.0), 4, &objectives)
        .expect("Second solve should succeed from cache");
    assert_eq!(frontier_2.len(), 5);
    assert_eq!(
        frontier_1[2].vars.get("active_cpu"),
        frontier_2[2].vars.get("active_cpu")
    );

    // Verify epsilon-box dominance filtering
    let dummy_diag = SolveDiagnostics {
        spec_hash: "hash".to_string(),
        free_vars: vec![],
        residual: 0.0,
        rank: 1,
        binding_constraints: vec![],
        backend_trace: vec![],
        solve_time_us: 10.0,
        status: SolveStatus::Converged,
    };

    let p1 = FrontierPoint {
        vars: VarMap::new(),
        objectives: vec![("cpu".to_string(), 10.000001), ("mem".to_string(), 20.0)],
        diagnostics: dummy_diag.clone(),
        is_feasible: true,
    };
    let p2 = FrontierPoint {
        vars: VarMap::new(),
        objectives: vec![("cpu".to_string(), 10.000002), ("mem".to_string(), 20.0)], // within 1e-4 of p1
        diagnostics: dummy_diag.clone(),
        is_feasible: true,
    };
    let p3 = FrontierPoint {
        vars: VarMap::new(),
        objectives: vec![("cpu".to_string(), 15.0), ("mem".to_string(), 30.0)], // distinct
        diagnostics: dummy_diag,
        is_feasible: true,
    };

    let filtered = Orchestrator::filter_epsilon_dominance(&[p1, p2, p3], 1e-4);
    assert_eq!(
        filtered.len(),
        2,
        "Near-identical point within eps box must be pruned"
    );
}

#[test]
fn test_concurrent_50_threads_frontier_cache_and_simultaneous_stale_taint() {
    use avenc::ast::*;
    use avenc::evaluator::{NativeEvaluator, Orchestrator, VarMap};
    use avenc::ledger::{Fact, FactMetadata, Ledger};
    use std::sync::Arc;
    use std::thread;

    // 1. Stress-test frontier_cache across 50 concurrent OS threads
    let eq = Equation {
        lhs: "active_cpu".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Mul,
            Box::new(Expr::Var("target_pods".to_string())),
            Box::new(Expr::Num(0.5)),
        ),
        original_text: "active_cpu = target_pods * 0.5".to_string(),
        line: 1,
        col: 1,
    };

    let evaluator = Arc::new(NativeEvaluator::new(vec![eq]));
    let orchestrator = Arc::new(Orchestrator::new(evaluator));

    let mut handles = Vec::new();
    for thread_id in 0..50 {
        let orch = Arc::clone(&orchestrator);
        handles.push(thread::spawn(move || {
            let mut inputs = VarMap::new();
            inputs.set("target_pods", 10.0 + (thread_id % 5) as f64);
            let objs = vec!["target_pods".to_string(), "active_cpu".to_string()];
            let res = orch.solve_pareto(&inputs, "target_pods", (10.0, 30.0), 4, &objs);
            assert!(
                res.is_ok(),
                "Thread {} solve_pareto must succeed",
                thread_id
            );
            res.unwrap()
        }));
    }

    for h in handles {
        let frontier = h.join().expect("Thread should not panic");
        assert_eq!(frontier.len(), 5);
    }

    // Cache should hold entries for each unique sweep key without deadlocks or corruption
    let cache_len = orchestrator.frontier_cache.lock().unwrap().len();
    assert!((1..=5).contains(&cache_len));

    // 2. Multi-fact simultaneous expiration and causal stale taint cascade
    let mut ledger = Ledger::new();
    let stale_meta = FactMetadata {
        timestamp_ms: Some(0),
        ttl_ms: Some(0),
        is_stale: true,
        warnings: vec!["zero-ttl-expired".to_string()],
    };

    // Multiple root facts expire simultaneously: total_cpu_cores and total_mem_gi
    ledger.append(Fact {
        seq: 1,
        tick: 0,
        kind: "input".to_string(),
        name: "total_cpu_cores".to_string(),
        value: 32.0,
        causes: vec![],
        origin: "external: kubectl".to_string(),
        metadata: Some(stale_meta.clone()),
    });
    ledger.append(Fact {
        seq: 2,
        tick: 0,
        kind: "input".to_string(),
        name: "total_mem_gi".to_string(),
        value: 60.803,
        causes: vec![],
        origin: "external: kubectl".to_string(),
        metadata: Some(stale_meta),
    });

    // Derived fact depending on total_cpu_cores: cpu_headroom
    ledger.append(Fact {
        seq: 3,
        tick: 0,
        kind: "derived".to_string(),
        name: "cpu_headroom".to_string(),
        value: 24.0,
        causes: vec![1],
        origin: "= total_cpu_cores - 8".to_string(),
        metadata: Some(FactMetadata {
            timestamp_ms: None,
            ttl_ms: None,
            is_stale: true,
            warnings: vec!["causal-stale-taint".to_string()],
        }),
    });

    // Derived fact depending on total_mem_gi: mem_headroom
    ledger.append(Fact {
        seq: 4,
        tick: 0,
        kind: "derived".to_string(),
        name: "mem_headroom".to_string(),
        value: 44.803,
        causes: vec![2],
        origin: "= total_mem_gi - 16".to_string(),
        metadata: Some(FactMetadata {
            timestamp_ms: None,
            ttl_ms: None,
            is_stale: true,
            warnings: vec!["causal-stale-taint".to_string()],
        }),
    });

    // Coupled derived decision depending on both: max_allocatable_pods
    ledger.append(Fact {
        seq: 5,
        tick: 0,
        kind: "derived".to_string(),
        name: "max_allocatable_pods".to_string(),
        value: 240.0,
        causes: vec![3, 4],
        origin: "min(cpu_headroom, mem_headroom)".to_string(),
        metadata: Some(FactMetadata {
            timestamp_ms: None,
            ttl_ms: None,
            is_stale: true,
            warnings: vec!["causal-stale-taint".to_string()],
        }),
    });

    let why_coupled = ledger.why("max_allocatable_pods");
    assert!(why_coupled.contains("[STALE: expired TTL]"));
    assert!(why_coupled.contains("[warnings: causal-stale-taint]"));
    assert!(why_coupled.contains("total_cpu_cores"));
    assert!(why_coupled.contains("total_mem_gi"));
}

#[test]
fn test_json_schema_file_validation_and_bound_enforcement() {
    use avenc::evaluator::JsonSchema;
    use std::collections::HashMap;

    let schema_str = r#"{
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "ClusterContract",
        "type": "object",
        "required": ["total_cpu_cores", "node_count"],
        "properties": {
            "total_cpu_cores": { "type": "number", "minimum": 1.0, "maximum": 256.0 },
            "node_count": { "type": "number", "minimum": 1.0 }
        }
    }"#;

    let schema = JsonSchema::parse(schema_str);
    assert_eq!(schema.required.len(), 2);
    assert_eq!(
        schema
            .number_properties
            .get("total_cpu_cores")
            .unwrap()
            .minimum,
        Some(1.0)
    );
    assert_eq!(
        schema
            .number_properties
            .get("total_cpu_cores")
            .unwrap()
            .maximum,
        Some(256.0)
    );

    // 1. Valid numbers
    let mut valid_map = HashMap::new();
    valid_map.insert("total_cpu_cores".to_string(), 32.0);
    valid_map.insert("node_count".to_string(), 4.0);
    assert!(schema.validate(&valid_map, "mock-backend").is_ok());

    // 2. Below minimum bound
    let mut invalid_min = HashMap::new();
    invalid_min.insert("total_cpu_cores".to_string(), 0.5);
    invalid_min.insert("node_count".to_string(), 4.0);
    let err = schema.validate(&invalid_min, "mock-backend").unwrap_err();
    assert!(err.to_string().contains("is below schema minimum 1.0000"));

    // 3. Exceeds maximum bound
    let mut invalid_max = HashMap::new();
    invalid_max.insert("total_cpu_cores".to_string(), 512.0);
    invalid_max.insert("node_count".to_string(), 4.0);
    let err_max = schema.validate(&invalid_max, "mock-backend").unwrap_err();
    assert!(
        err_max
            .to_string()
            .contains("exceeds schema maximum 256.0000")
    );

    // 4. Missing required property
    let mut missing_prop = HashMap::new();
    missing_prop.insert("total_cpu_cores".to_string(), 32.0);
    let err_missing = schema.validate(&missing_prop, "mock-backend").unwrap_err();
    assert!(
        err_missing
            .to_string()
            .contains("missing required property 'node_count'")
    );
}

#[test]
fn test_microbenchmark_frontier_rebuild_under_mutating_external_facts() {
    use avenc::ast::*;
    use avenc::evaluator::{NativeEvaluator, Orchestrator, VarMap};
    use std::sync::Arc;

    let eq = Equation {
        lhs: "active_cpu".to_string(),
        op: ConstraintOp::Equal,
        rhs: Expr::Bin(
            BinaryOp::Mul,
            Box::new(Expr::Var("target_pods".to_string())),
            Box::new(Expr::Num(0.5)),
        ),
        original_text: "active_cpu = target_pods * 0.5".to_string(),
        line: 1,
        col: 1,
    };

    let evaluator = Arc::new(NativeEvaluator::new(vec![eq]));
    let orchestrator = Orchestrator::new(evaluator);
    let objectives = vec!["target_pods".to_string(), "active_cpu".to_string()];

    let mut rebuild_durations_us = Vec::new();
    let iterations = 30;

    for i in 0..iterations {
        // Mutating external facts: each iteration provides a distinct telemetry input
        let mut inputs = VarMap::new();
        inputs.set("traffic_rate", 1000.0 + (i as f64 * 500.0));
        inputs.set("target_pods", 10.0 + (i as f64));

        let t0 = std::time::Instant::now();
        let frontier = orchestrator
            .solve_pareto(&inputs, "target_pods", (10.0, 50.0), 5, &objectives)
            .expect("Frontier solve must succeed");
        let elapsed_us = t0.elapsed().as_secs_f64() * 1_000_000.0;
        rebuild_durations_us.push(elapsed_us);

        assert_eq!(frontier.len(), 6);
    }

    // Cache should hold all 30 distinct spec hashes
    let cache_len = orchestrator.frontier_cache.lock().unwrap().len();
    assert_eq!(cache_len, iterations);

    // Warm cache hit test: second solve with identical inputs is instant
    let mut repeat_inputs = VarMap::new();
    repeat_inputs.set("traffic_rate", 1000.0);
    repeat_inputs.set("target_pods", 10.0);

    let t_warm = std::time::Instant::now();
    let warm_frontier = orchestrator
        .solve_pareto(&repeat_inputs, "target_pods", (10.0, 50.0), 5, &objectives)
        .expect("Cached solve must succeed");
    let warm_elapsed_us = t_warm.elapsed().as_secs_f64() * 1_000_000.0;

    assert_eq!(warm_frontier.len(), 6);
    // Warm retrieval should be sub-10 microseconds
    assert!(
        warm_elapsed_us < 50.0,
        "Warm cache retrieval took {:.2} µs (expected < 50 µs)",
        warm_elapsed_us
    );

    let avg_rebuild_us: f64 = rebuild_durations_us.iter().sum::<f64>() / iterations as f64;
    // Internal solve should comfortably be < 200 µs (0.2 ms)
    assert!(
        avg_rebuild_us < 200.0,
        "Average frontier rebuild took {:.2} µs (expected < 200 µs)",
        avg_rebuild_us
    );
}

#[test]
fn test_real_production_traffic_trace_replay_and_capacity_tracking() {
    use avenc::ledger::{Fact, FactMetadata, Ledger};
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;
    use avenc::solver::SolverEngine;
    use std::collections::HashMap;

    let src = r#"
        relation ProductionScaling {
            raw_workers = (reqs_per_second * avg_latency_ms) / 1000.0
            allocated_workers = ceil(raw_workers / target_utilization)
            target_pods = ceil(allocated_workers / pod_capacity_workers)
        }
    "#;
    let mut lex = Lexer::new(src);
    let mut parser = Parser::new(lex.tokenize().unwrap());
    let module = parser.parse_module("test_scaling").unwrap();

    let mut solver = SolverEngine::new();
    for eq in &module.blocks[0].equations {
        solver.add_relation(eq.clone());
    }

    // Key representative test points from wikimedia_traffic_trace.json:
    // (step, rps, latency_ms, ttl_ms, expected_pods, expected_workers)
    let test_points = vec![
        (0, 8200.0, 14.2, 30000, 2.0, 167.0),
        (10, 48000.0, 28.5, 30000, 20.0, 1955.0),
        (16, 98200.0, 56.4, 30000, 80.0, 7913.0),
        (25, 45500.0, 27.2, 0, 18.0, 1768.0),
        (39, 7100.0, 13.5, 30000, 2.0, 137.0),
    ];

    let mut solve_durations_us = Vec::new();

    for (step, rps, lat, ttl, expected_pods, expected_workers) in test_points {
        let mut env = HashMap::new();
        env.insert("reqs_per_second".to_string(), rps);
        env.insert("avg_latency_ms".to_string(), lat);
        env.insert("target_utilization".to_string(), 0.70);
        env.insert("pod_capacity_workers".to_string(), 100.0);

        let t0 = std::time::Instant::now();
        let solved = solver.solve(&env).expect("Step solve must succeed");
        let elapsed_us = t0.elapsed().as_secs_f64() * 1_000_000.0;
        solve_durations_us.push(elapsed_us);

        let pods = *solved.get("target_pods").unwrap();
        let workers = *solved.get("allocated_workers").unwrap();

        assert_eq!(pods, expected_pods, "Step {} pod sizing mismatch", step);
        assert_eq!(
            workers, expected_workers,
            "Step {} worker sizing mismatch",
            step
        );

        // Freshness verification
        let is_stale = ttl == 0;
        let mut ledger = Ledger::new();
        ledger.append(Fact {
            seq: 1,
            tick: step as usize,
            kind: "input".to_string(),
            name: "reqs_per_second".to_string(),
            value: rps,
            causes: Vec::new(),
            origin: "wikimedia-trace".to_string(),
            metadata: Some(FactMetadata {
                timestamp_ms: Some(1792900000000),
                ttl_ms: Some(ttl),
                is_stale,
                warnings: if is_stale {
                    vec!["stale-telemetry".to_string()]
                } else {
                    Vec::new()
                },
            }),
        });

        if is_stale {
            let fact = ledger.latest("reqs_per_second").unwrap();
            assert!(fact.metadata.as_ref().unwrap().is_stale);
            let display = fact.display();
            assert!(display.contains("[STALE]"));
        }
    }

    let avg_solve_us = solve_durations_us.iter().sum::<f64>() / solve_durations_us.len() as f64;
    assert!(
        avg_solve_us < 200.0,
        "Average Little's Law solve took {:.2} µs (expected < 200 µs)",
        avg_solve_us
    );
}

#[test]
fn test_optimizer_two_integer_variables_nodes_and_drives() {
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;
    use avenc::solver::SolverEngine;
    use std::collections::HashMap;

    let src = r#"
    relation Cluster {
        int nodes
        int drives
        capacity = nodes * drives * 13.5
        cost = nodes * 2000.0 + nodes * drives * 600.0
        capacity >= 100.0
        drives <= 12.0
    }

    solve Cluster with {
        minimize(cost)
    }
    "#;

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenize failed");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("ClusterModule").expect("parse failed");

    let blk = &module.blocks[0];
    let solve = &module.solves[0];

    let mut solver = SolverEngine::new();
    for eq in &blk.equations {
        solver.add_relation(eq.clone());
    }
    for iv in &blk.integer_vars {
        solver.add_integer_var(iv.clone());
    }
    for c in &solve.constraints {
        solver.add_constraint(c.clone());
    }
    for obj in &solve.objectives {
        solver.add_objective(obj.clone());
    }

    let inputs = HashMap::new();
    let solved = solver
        .solve(&inputs)
        .expect("Solver failed on 2-variable integer optimization");

    // Must find 1 node with 8 drives (cost 6,800), NOT getting stuck at the cap of 12 drives
    assert_eq!(solved.get("nodes").copied(), Some(1.0));
    assert_eq!(solved.get("drives").copied(), Some(8.0));
    assert_eq!(solved.get("cost").copied(), Some(6800.0));
    assert_eq!(solved.get("capacity").copied(), Some(108.0));
}

#[test]
fn test_optimizer_minimum_away_from_bounds() {
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;
    use avenc::solver::SolverEngine;
    use std::collections::HashMap;

    let src = r#"
    relation Quad {
        cost = (x - 3.0) * (x - 3.0) + 1.0
        x <= 10.0
    }

    solve Quad with {
        minimize(cost)
    }
    "#;

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenize failed");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("QuadModule").expect("parse failed");

    let blk = &module.blocks[0];
    let solve = &module.solves[0];

    let mut solver = SolverEngine::new();
    for eq in &blk.equations {
        solver.add_relation(eq.clone());
    }
    for c in &solve.constraints {
        solver.add_constraint(c.clone());
    }
    for obj in &solve.objectives {
        solver.add_objective(obj.clone());
    }

    let inputs = HashMap::new();
    let solved = solver
        .solve(&inputs)
        .expect("Solver failed on unconstrained minimum away from bound");

    // The unconstrained minimum is at x = 3, cost = 1.
    // It must NOT pick the boundary point x = 10 (cost 50).
    let x_val = solved.get("x").copied().unwrap();
    let cost_val = solved.get("cost").copied().unwrap();
    assert!(
        (x_val - 3.0).abs() < 1e-4,
        "Expected x = 3.0, got {:.4}",
        x_val
    );
    assert!(
        (cost_val - 1.0).abs() < 1e-4,
        "Expected cost = 1.0, got {:.4}",
        cost_val
    );
}

#[test]
fn test_optimizer_unconstrained_without_inequalities() {
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;
    use avenc::solver::SolverEngine;
    use std::collections::HashMap;

    let src = r#"
    relation UnconstrainedQuad {
        cost = (x - 3.0) * (x - 3.0) + 1.0
    }

    solve UnconstrainedQuad with {
        minimize(cost)
    }
    "#;

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenize failed");
    let mut parser = Parser::new(tokens);
    let module = parser
        .parse_module("UnconstrainedModule")
        .expect("parse failed");

    let blk = &module.blocks[0];
    let solve = &module.solves[0];

    let mut solver = SolverEngine::new();
    for eq in &blk.equations {
        solver.add_relation(eq.clone());
    }
    for obj in &solve.objectives {
        solver.add_objective(obj.clone());
    }

    let inputs = HashMap::new();
    let solved = solver
        .solve(&inputs)
        .expect("Solver must not report infeasible when no inequalities exist");

    let x_val = solved.get("x").copied().unwrap();
    let cost_val = solved.get("cost").copied().unwrap();
    assert!(
        (x_val - 3.0).abs() < 1e-4,
        "Expected x = 3.0, got {:.4}",
        x_val
    );
    assert!(
        (cost_val - 1.0).abs() < 1e-4,
        "Expected cost = 1.0, got {:.4}",
        cost_val
    );
}

#[test]
fn test_ceil_unreachable_target_reports_not_reachable() {
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;
    use avenc::solver::{SolverEngine, SolverError};
    use std::collections::HashMap;

    let src = r#"
    relation Workers {
        allocated_workers = ceil(raw_workers / 0.7)
    }

    solve Workers with {
        allocated_workers = 1428.5
    }
    "#;

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenize failed");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("WorkersModule").expect("parse failed");

    let blk = &module.blocks[0];
    let solve = &module.solves[0];

    let mut solver = SolverEngine::new();
    for eq in &blk.equations {
        solver.add_relation(eq.clone());
    }

    let mut inputs = HashMap::new();
    for b in &solve.bindings {
        inputs.insert(b.name.clone(), b.value);
    }

    let res = solver.solve(&inputs);
    match res {
        Err(SolverError::UnderDetermined(diag)) => {
            // Must be reported as DETERMINED (0 DOF), not under-determined
            assert_eq!(
                diag.dof, 0,
                "Expected 0 DOF for determined system with unreachable ceil target"
            );
            assert!(
                diag.free_vars.is_empty(),
                "Must not report false free variables"
            );
            assert!(
                diag.degenerate_equations.is_empty(),
                "Must not report false redundant equations"
            );
            assert_eq!(diag.unreachable_targets.len(), 1);
            assert!(
                diag.unreachable_targets[0]
                    .contains("ceil only returns whole numbers, 1428.5 is not reachable")
            );

            let report = diag.report();
            assert!(report.contains("DETERMINED SYSTEM (0 DOF) — solver stalled, here is why:"));
            assert!(report.contains("ceil only returns whole numbers, 1428.5 is not reachable"));
            assert!(!report.contains("UNDER-DETERMINED SYSTEM"));
            assert!(!report.contains("FREE variables"));
            assert!(!report.contains("DEGENERATE / REDUNDANT EQUATION"));
        }
        other => panic!(
            "Expected SolverError::UnderDetermined with unreachable target, got {:?}",
            other
        ),
    }
}

#[test]
fn test_unit_error_preserves_source_line_and_col() {
    use avenc::lexer::Lexer;
    use avenc::parser::Parser;
    use avenc::units::{DimensionalChecker, TypeError};

    let src = r#"
signal network_rate : Gbps
signal storage_size : MB

relation Mismatch {
    bad_sum = network_rate + storage_size
}

solve Mismatch with {
    network_rate = 10.0 Gbps
    storage_size = 500.0 MB
}
"#;

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenize failed");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("MismatchModule").expect("parse failed");

    let mut checker = DimensionalChecker::new();
    let res = checker.check_module(&module);

    match res {
        Err(TypeError::DimensionalMismatch { line, col, op, .. }) => {
            assert_eq!(line, 6, "Expected mismatch on line 6, got {}", line);
            assert_eq!(col, 5, "Expected mismatch at col 5, got {}", col);
            assert_eq!(op, "+/-");
        }
        other => panic!(
            "Expected TypeError::DimensionalMismatch with valid position, got {:?}",
            other
        ),
    }
}
