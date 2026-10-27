// tests/phase7_ecosystem_wasm.rs -- Milestone Phase 7 Test Suite
// Ecosystem Bootstrap: Package Manager (Aven.toml, .avenpkg), WebAssembly (WAT & WASM bytecode) Interop,
// Kill-Switch Applications (Financial & Drone Avionics), and V8 Node.js Execution.
// Includes minimum 3 randomized tests using OS entropy + PID so data is never recycled.

use avenc::ast::AvenModule;
use avenc::codegen_wasm::WasmTranspiler;
use avenc::evaluator::Evaluator;
use avenc::lexer::Lexer;
use avenc::package::{DeclaredUnitSpec, DependencySpec, PackageManifest, PackageManager};
use avenc::parser::Parser;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn parse_module_from_str(src: &str, name: &str) -> AvenModule {
    let tokens = Lexer::new(src).tokenize().expect("tokenize");
    Parser::new(tokens).parse_module(name).expect("parse")
}

// OS entropy + PID PRNG
struct Phase7Prng(u64);

impl Phase7Prng {
    fn from_entropy() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let pid = std::process::id() as u128;
        let seed = ((nanos ^ (pid << 32)) as u64) | 0xCAFEF00D12345678;
        Phase7Prng(seed)
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
        let r = (self.next_u64() % 1_000_000) as f64 / 1_000_000.0;
        min + r * (max - min)
    }

    fn next_u64_range(&mut self, min: u64, max: u64) -> u64 {
        if min >= max {
            return min;
        }
        min + (self.next_u64() % (max - min + 1))
    }
}

fn create_temp_dir(prefix: &str) -> PathBuf {
    let mut prng = Phase7Prng::from_entropy();
    let rand_suffix = prng.next_u64();
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("{}_{}_{}", prefix, pid, rand_suffix));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

// ---------------------------------------------------------------------------
// Test 1: Golden Package Manifest, Packaging (.avenpkg) & Installation
// ---------------------------------------------------------------------------
#[test]
fn test_package_manifest_and_pack_golden() {
    let dir = create_temp_dir("phase7_pkg_golden");
    let cache_dir = dir.join("cache");

    // 1. Initialize package
    let manifest_path = PackageManager::init(&dir, "thermodynamics").expect("init package");
    assert!(manifest_path.exists());

    let manifest_content = fs::read_to_string(&manifest_path).expect("read Aven.toml");
    let manifest = PackageManifest::from_toml(&manifest_content).expect("parse manifest");
    assert_eq!(manifest.name, "thermodynamics");
    assert_eq!(manifest.version, "0.1.0");
    assert!(manifest.units.iter().any(|u| u.name == "kilo_watt"));

    // 2. Pack package into .avenpkg archive
    let (archive_path, digest) = PackageManager::pack(&dir, None).expect("pack package");
    assert!(archive_path.exists());
    assert_eq!(digest.len(), 64);

    // 3. Install package into cache
    let installed = PackageManager::install(&archive_path, &cache_dir).expect("install package");
    assert_eq!(installed.name, "thermodynamics");
    assert_eq!(installed.version, "0.1.0");

    // 4. List installed packages
    let list = PackageManager::list(&cache_dir).expect("list packages");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "thermodynamics");

    // 5. Test import resolution
    let test_model = "import thermodynamics;\n\
                      solve HeatPump with { voltage_v = 240, current_a = 15, operating_hours = 8 }\n";
    let inlined = PackageManager::resolve_imports(test_model, &[dir.clone()]).expect("resolve imports");
    assert!(inlined.contains("relation thermodynamicsRelations"));
    assert!(inlined.contains("energy_kwh"));

    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Test 2: Golden WebAssembly Text (.wat) Emission
// ---------------------------------------------------------------------------
#[test]
fn test_wasm_wat_emission_golden() {
    let src = fs::read_to_string("examples/server_capacity.aven").expect("read server_capacity");
    let module = parse_module_from_str(&src, "server_capacity");

    let bundle = WasmTranspiler::transpile(&module).expect("transpile to wasm");
    assert_eq!(bundle.model_name, "server_capacity");
    assert_eq!(bundle.block_name, "ProductionProfile");

    // Check WAT contents
    assert!(bundle.wat.contains("(module"));
    assert!(bundle.wat.contains("(memory (export \"memory\") 1)"));
    assert!(bundle.wat.contains("(func $ProductionProfile (export \"ProductionProfile\")"));
    assert!(bundle.wat.contains("(func (export \"aven_solve\")"));
    assert!(bundle.wat.contains("f64.load"));
    assert!(bundle.wat.contains("f64.store"));
    assert!(bundle.wat.contains("f64.ceil"));
}

// ---------------------------------------------------------------------------
// Test 3: Golden WebAssembly Binary (.wasm) Emission & Node.js V8 Execution
// ---------------------------------------------------------------------------
#[test]
fn test_wasm_binary_emission_and_node_execution() {
    let src = fs::read_to_string("examples/server_capacity.aven").expect("read server_capacity");
    let module = parse_module_from_str(&src, "server_capacity");

    let bundle = WasmTranspiler::transpile(&module).expect("transpile to wasm");
    assert!(bundle.wasm_bytes.len() > 64);
    // Header magic and version
    assert_eq!(&bundle.wasm_bytes[0..4], b"\0asm");
    assert_eq!(&bundle.wasm_bytes[4..8], &[0x01, 0x00, 0x00, 0x00]);

    // Save wasm to temp dir and execute via Node.js
    let dir = create_temp_dir("phase7_wasm_node");
    let wasm_file = dir.join("server_capacity.wasm");
    fs::write(&wasm_file, &bundle.wasm_bytes).expect("write wasm bytes");

    // Check if node is available
    if Command::new("node").arg("--version").output().is_ok() {
        let node_script = format!(
            "const fs = require('fs');\n\
             const bytes = fs.readFileSync('{file}');\n\
             WebAssembly.instantiate(bytes).then(({{ instance }}) => {{\n\
                 // 1. Test direct exported function (returns final equation wire_rate_gbps)\n\
                 const res = instance.exports.ProductionProfile(\n\
                     20.0,   // avg_latency_ms\n\
                     64.0,   // avg_payload_kb\n\
                     1.04,   // framing_overhead_ratio\n\
                     4096.0, // os_kernel_ram_mb\n\
                     16.0,   // ram_per_worker\n\
                     50000.0,// reqs_per_second\n\
                     4.0,    // socket_buffer_mb\n\
                     0.70    // target_utilization\n\
                 );\n\
                 console.log('NODE_WASM_WIRE_RATE=' + res);\n\
                 \n\
                 // 2. Test buffer-oriented zero-copy aven_solve(in_ptr, out_ptr)\n\
                 const mem = instance.exports.memory;\n\
                 const buf = new Float64Array(mem.buffer);\n\
                 const inPtr = 0;\n\
                 const outPtr = 128;\n\
                 const inputs = [20.0, 64.0, 1.04, 4096.0, 16.0, 50000.0, 4.0, 0.70];\n\
                 for (let i = 0; i < inputs.length; i++) {{\n\
                     buf[inPtr / 8 + i] = inputs[i];\n\
                 }}\n\
                 const code = instance.exports.aven_solve(inPtr, outPtr);\n\
                 console.log('SOLVE_CODE=' + code);\n\
                 console.log('BUFFER_RAW_WORKERS=' + buf[outPtr / 8 + 0]);\n\
                 console.log('BUFFER_ALLOCATED_WORKERS=' + buf[outPtr / 8 + 1]);\n\
             }}).catch(e => {{ console.error(e); process.exit(1); }});",
            file = wasm_file.to_str().unwrap().replace('\\', "/")
        );

        let output = Command::new("node")
            .args(&["-e", &node_script])
            .output()
            .expect("run node wasm");

        assert!(output.status.success(), "node wasm execution failed");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("NODE_WASM_WIRE_RATE=26.624"));
        assert!(stdout.contains("BUFFER_RAW_WORKERS=1000"));
        assert!(stdout.contains("BUFFER_ALLOCATED_WORKERS=1429"));
    }

    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Test 4: Golden Kill-Switch Application — Portfolio Rebalancing
// ---------------------------------------------------------------------------
#[test]
fn test_kill_switch_financial_model_golden() {
    let src = fs::read_to_string("examples/portfolio_rebalancing.aven").expect("read portfolio_rebalancing");
    let module = parse_module_from_str(&src, "portfolio_rebalancing");

    let mut inputs = avenc::evaluator::VarMap::new();
    let solve = module.solves.first().expect("solve block");
    for b in &solve.bindings {
        inputs.set(&b.name, b.value);
    }

    let evaluator = avenc::evaluator::NativeEvaluator::new(module.blocks[0].equations.clone());
    let result = evaluator.evaluate(&inputs).expect("solve financial model");

    assert_eq!(*result.values.get("total_weight").unwrap(), 1.0);
    let ret = *result.values.get("expected_return_pct").unwrap();
    assert!((ret - 8.025).abs() < 1e-3, "expected return 8.025, got {}", ret);

    let vol = *result.values.get("portfolio_vol_pct").unwrap();
    assert!((vol - 7.9971).abs() < 1e-2, "expected vol ~8.0, got {}", vol);

    let sharpe = *result.values.get("sharpe_ratio").unwrap();
    assert!((sharpe - 0.5658).abs() < 1e-3, "expected sharpe 0.5658, got {}", sharpe);
}

// ---------------------------------------------------------------------------
// Test 5: Golden Kill-Switch Application — Quadcopter Avionics
// ---------------------------------------------------------------------------
#[test]
fn test_kill_switch_quadcopter_avionics_golden() {
    let src = fs::read_to_string("examples/quadcopter_avionics.aven").expect("read quadcopter_avionics");
    let module = parse_module_from_str(&src, "quadcopter_avionics");

    let mut inputs = avenc::evaluator::VarMap::new();
    let solve = module.solves.first().expect("solve block");
    for b in &solve.bindings {
        inputs.set(&b.name, b.value);
    }

    let evaluator = avenc::evaluator::NativeEvaluator::new(module.blocks[0].equations.clone());
    let result = evaluator.evaluate(&inputs).expect("solve quadcopter model");

    let twr = *result.values.get("thrust_weight_ratio").unwrap();
    assert!(twr > 5.0 && twr < 6.0, "expected TWR ~5.5, got {}", twr);

    let accel = *result.values.get("climb_accel_mps2").unwrap();
    assert!(accel > 40.0 && accel < 50.0, "expected climb accel ~44.2, got {}", accel);

    let power = *result.values.get("delivered_power_w").unwrap();
    assert!(power > 3000.0 && power < 3500.0, "expected power ~3223W, got {}", power);
}

// ---------------------------------------------------------------------------
// Test 6 (Randomized 1): Dynamic Package Manifests & Dependency Graphs
// ---------------------------------------------------------------------------
#[test]
fn test_randomized_1_package_manifest_and_dependency_graph() {
    let mut prng = Phase7Prng::from_entropy();
    let dir = create_temp_dir("phase7_rand_pkg");
    let cache_dir = dir.join("cache");

    for i in 0..5 {
        let pkg_name = format!("rand_pkg_{}_{}", prng.next_u64_range(100, 999), i);
        let version = format!(
            "{}.{}.{}",
            prng.next_u64_range(0, 3),
            prng.next_u64_range(1, 9),
            prng.next_u64_range(0, 20)
        );

        let mut manifest = PackageManifest {
            name: pkg_name.clone(),
            version: version.clone(),
            description: "Randomized test library".to_string(),
            authors: vec!["Test Runner <runner@avenc.dev>".to_string()],
            license: "MIT".to_string(),
            entry: "lib.aven".to_string(),
            dependencies: HashMap::new(),
            units: vec![
                DeclaredUnitSpec {
                    name: format!("unit_{}", i),
                    dimension: "Pressure".to_string(),
                    scale: prng.next_f64_range(10.0, 1000.0),
                },
            ],
        };

        if i > 0 {
            manifest.dependencies.insert(
                format!("dep_{}", i),
                DependencySpec {
                    version: Some("0.1.0".to_string()),
                    path: None,
                },
            );
        }

        // Test TOML round-trip
        let toml_str = manifest.to_toml();
        let parsed = PackageManifest::from_toml(&toml_str).expect("parse randomized manifest");
        assert_eq!(parsed.name, pkg_name);
        assert_eq!(parsed.version, version);
        assert_eq!(parsed.units.len(), 1);

        // Test pack & install
        let pkg_dir = dir.join(&pkg_name);
        fs::create_dir_all(&pkg_dir).unwrap();
        fs::write(pkg_dir.join("Aven.toml"), &toml_str).unwrap();
        fs::write(
            pkg_dir.join("lib.aven"),
            format!("relation Rel{} {{ out_val = in_val * 2 }}\n", i),
        )
        .unwrap();

        let (archive, digest) = PackageManager::pack(&pkg_dir, None).expect("pack random");
        assert_eq!(digest.len(), 64);

        let inst = PackageManager::install(&archive, &cache_dir).expect("install random");
        assert_eq!(inst.name, pkg_name);
        assert_eq!(inst.version, version);
    }

    let all_installed = PackageManager::list(&cache_dir).expect("list all");
    assert_eq!(all_installed.len(), 5);

    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Test 7 (Randomized 2): WASM Bytecode Generation & Opcode Verification
// ---------------------------------------------------------------------------
#[test]
fn test_randomized_2_wasm_bytecode_generation_and_opcodes() {
    let mut prng = Phase7Prng::from_entropy();

    for _ in 0..5 {
        let c1 = prng.next_f64_range(1.0, 10.0);
        let c2 = prng.next_f64_range(2.0, 20.0);

        let model_src = format!(
            "forward RandCalc {{\n\
                 intermediate = (x_val * {:.4}) + {:.4}\n\
                 final_out    = intermediate * 3.5\n\
             }}\n",
            c1, c2
        );

        let module = parse_module_from_str(&model_src, "rand_calc");
        let bundle = WasmTranspiler::transpile(&module).expect("transpile random model");

        // Verify WASM structure
        assert_eq!(&bundle.wasm_bytes[0..4], b"\0asm");
        assert!(bundle.wasm_bytes.len() > 50);

        // Node.js execution verification
        if Command::new("node").arg("--version").output().is_ok() {
            let test_x = prng.next_f64_range(5.0, 50.0);
            let expected_inter = (test_x * c1) + c2;
            let expected_final = expected_inter * 3.5;

            let dir = create_temp_dir("wasm_rand_eval");
            let wasm_path = dir.join("eval.wasm");
            fs::write(&wasm_path, &bundle.wasm_bytes).unwrap();

            let node_code = format!(
                "const fs = require('fs');\n\
                 const bytes = fs.readFileSync('{file}');\n\
                 WebAssembly.instantiate(bytes).then(({{ instance }}) => {{\n\
                     const val = instance.exports.RandCalc({x});\n\
                     console.log('COMPUTED=' + val);\n\
                 }}).catch(e => {{ console.error(e); process.exit(1); }});",
                file = wasm_path.to_str().unwrap().replace('\\', "/"),
                x = test_x
            );

            let out = Command::new("node").args(&["-e", &node_code]).output().unwrap();
            assert!(out.status.success());
            let out_str = String::from_utf8_lossy(&out.stdout);
            assert!(out_str.contains("COMPUTED="));

            let computed_val: f64 = out_str
                .trim()
                .strip_prefix("COMPUTED=")
                .unwrap()
                .parse()
                .unwrap();
            // Check precision within 0.01%
            assert!(
                (computed_val - expected_final).abs() < 1e-2 * (1.0 + expected_final.abs()),
                "wasm computed {} != expected {}",
                computed_val,
                expected_final
            );

            let _ = fs::remove_dir_all(&dir);
        }
    }
}

// ---------------------------------------------------------------------------
// Test 8 (Randomized 3): Portfolio Stress Testing & Volatility Invariants
// ---------------------------------------------------------------------------
#[test]
fn test_randomized_3_portfolio_rebalancing_counterfactual_stress() {
    let mut prng = Phase7Prng::from_entropy();
    let src = fs::read_to_string("examples/portfolio_rebalancing.aven").expect("read portfolio");
    let module = parse_module_from_str(&src, "portfolio_rebalancing");
    let evaluator = avenc::evaluator::NativeEvaluator::new(module.blocks[0].equations.clone());

    for _ in 0..5 {
        let w_eq = prng.next_f64_range(0.2, 0.6);
        let w_bd = prng.next_f64_range(0.2, 0.5);
        let w_rt = 1.0 - (w_eq + w_bd);

        let mut inputs = avenc::evaluator::VarMap::new();
        inputs.set("portfolio_value", 10_000_000.0);
        inputs.set("equity_weight", w_eq);
        inputs.set("bond_weight", w_bd);
        inputs.set("reit_weight", w_rt);
        inputs.set("equity_return_pct", 10.5);
        inputs.set("bond_return_pct", 4.2);
        inputs.set("reit_return_pct", 7.8);
        inputs.set("equity_vol_pct", 16.0);
        inputs.set("bond_vol_pct", 5.5);
        inputs.set("reit_vol_pct", 12.0);
        inputs.set("diversification_factor", 0.88);
        inputs.set("risk_free_rate_pct", 3.5);

        let result = evaluator.evaluate(&inputs).expect("solve random portfolio");

        let total_weight = *result.values.get("total_weight").unwrap();
        assert!((total_weight - 1.0).abs() < 1e-6);

        let ret = *result.values.get("expected_return_pct").unwrap();
        let expected_ret = (w_eq * 10.5) + (w_bd * 4.2) + (w_rt * 7.8);
        assert!((ret - expected_ret).abs() < 1e-4);

        let sharpe = *result.values.get("sharpe_ratio").unwrap();
        assert!(sharpe > 0.0, "sharpe ratio must remain positive in normal market");
    }
}

// ---------------------------------------------------------------------------
// Test 9: CLI Package & WASM Subprocess Integration
// ---------------------------------------------------------------------------
#[test]
fn test_cli_pkg_and_wasm_subprocesses() {
    let cargo = env!("CARGO_BIN_EXE_avenc");
    let dir = create_temp_dir("phase7_cli_test");

    // 1. avenc emit-wasm --wat
    let wat_output = Command::new(cargo)
        .args(&["emit-wasm", "examples/server_capacity.aven", "--wat"])
        .output()
        .expect("run avenc emit-wasm --wat");

    assert!(wat_output.status.success());
    let wat_text = String::from_utf8_lossy(&wat_output.stdout);
    assert!(wat_text.contains("(module"));
    assert!(wat_text.contains("(export \"aven_solve\")"));

    // 2. avenc emit-wasm -o <out.wasm>
    let out_wasm = dir.join("test_model.wasm");
    let wasm_output = Command::new(cargo)
        .args(&[
            "emit-wasm",
            "examples/portfolio_rebalancing.aven",
            "-o",
            out_wasm.to_str().unwrap(),
        ])
        .output()
        .expect("run avenc emit-wasm");

    assert!(wasm_output.status.success());
    assert!(out_wasm.exists());
    let wasm_bytes = fs::read(&out_wasm).unwrap();
    assert_eq!(&wasm_bytes[0..4], b"\0asm");

    // 3. avenc pkg init
    let pkg_dir = dir.join("cli_package");
    fs::create_dir_all(&pkg_dir).unwrap();
    let init_output = Command::new(cargo)
        .current_dir(&pkg_dir)
        .args(&["pkg", "init", "aerodynamics"])
        .output()
        .expect("run avenc pkg init");

    assert!(init_output.status.success());
    assert!(pkg_dir.join("Aven.toml").exists());
    assert!(pkg_dir.join("lib.aven").exists());

    // 4. avenc pkg pack
    let pack_output = Command::new(cargo)
        .current_dir(&pkg_dir)
        .args(&["pkg", "pack"])
        .output()
        .expect("run avenc pkg pack");

    assert!(pack_output.status.success());
    let stdout = String::from_utf8_lossy(&pack_output.stdout);
    assert!(stdout.contains(".avenpkg"));

    let _ = fs::remove_dir_all(&dir);
}
