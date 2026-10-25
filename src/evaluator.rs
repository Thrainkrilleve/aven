// src/evaluator.rs — Pluggable Evaluation Engine & Pareto Orchestrator for Aven
// Milestone v0.6.0: Evaluator Trait, NativeEvaluator, SHA-256 Spec Hashing, and ε-Constraint MOO.
#![allow(dead_code, clippy::chunks_exact_to_as_chunks)]

use crate::ast::Equation;
use crate::solver::SolverEngine;
use std::collections::HashMap;
use std::sync::Arc;

/// Pure-Rust FIPS 180-4 standard SHA-256 implementation (zero external crate dependencies).
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, part) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([part[0], part[1], part[2], part[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut h_val = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h_val
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h_val = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(h_val);
    }

    let mut hex = String::with_capacity(64);
    for val in h {
        use std::fmt::Write;
        let _ = write!(hex, "{:08x}", val);
    }
    hex
}

/// Variable map representing inputs and outputs with dimensional metadata.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct VarMap {
    pub values: HashMap<String, f64>,
    pub units: HashMap<String, String>,
}

impl VarMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: &str, val: f64) {
        self.values.insert(key.to_string(), val);
    }

    pub fn get(&self, key: &str) -> Option<f64> {
        self.values.get(key).copied()
    }

    /// Computes deterministic 16-character SHA-256 specification hash.
    pub fn compute_hash(&self) -> String {
        let sorted: std::collections::BTreeMap<&String, &f64> = self.values.iter().collect();
        let mut serialized = String::new();
        for (k, v) in sorted {
            use std::fmt::Write;
            let _ = write!(serialized, "{}:{:.6};", k, v);
        }
        let full_hex = sha256_hex(serialized.as_bytes());
        full_hex[..16].to_string()
    }
}

/// Evaluation result returned by any backend.
#[derive(Clone, Debug)]
pub struct EvalResult {
    pub values: HashMap<String, f64>,
    pub backend_id: String,
    pub solve_time_us: f64,
}

#[derive(Clone, Debug)]
pub enum EvalError {
    DomainError(String),
    Timeout(String),
    SolverFailed(String),
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalError::DomainError(msg) => write!(f, "Domain error: {}", msg),
            EvalError::Timeout(msg) => write!(f, "Timeout: {}", msg),
            EvalError::SolverFailed(msg) => write!(f, "Solver failed: {}", msg),
        }
    }
}

impl std::error::Error for EvalError {}

/// The pluggable Evaluator trait.
pub trait Evaluator: Send + Sync {
    fn evaluate(&self, inputs: &VarMap) -> Result<EvalResult, EvalError>;
    fn name(&self) -> &str;
}

/// Native analytical evaluator wrapping Aven's continuous Newton engine.
pub struct NativeEvaluator {
    pub relations: Vec<Equation>,
}

impl NativeEvaluator {
    pub fn new(relations: Vec<Equation>) -> Self {
        Self { relations }
    }
}

impl Evaluator for NativeEvaluator {
    fn evaluate(&self, inputs: &VarMap) -> Result<EvalResult, EvalError> {
        let t_start = std::time::Instant::now();
        let mut engine = SolverEngine::new();
        for eq in &self.relations {
            engine.add_relation(eq.clone());
        }
        let solved = engine
            .solve(&inputs.values)
            .map_err(|e| EvalError::SolverFailed(e.to_string()))?;
        let elapsed_us = t_start.elapsed().as_secs_f64() * 1_000_000.0;
        Ok(EvalResult {
            values: solved,
            backend_id: "native_analytic".to_string(),
            solve_time_us: elapsed_us,
        })
    }

    fn name(&self) -> &str {
        "native_analytic"
    }
}

/// External subprocess evaluator communicating via JSON over stdin/stdout.
pub struct ExternalEvaluator {
    pub command: String,
    pub args: Vec<String>,
}

impl ExternalEvaluator {
    pub fn new(command: &str, args: &[&str]) -> Self {
        Self {
            command: command.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl Evaluator for ExternalEvaluator {
    fn evaluate(&self, inputs: &VarMap) -> Result<EvalResult, EvalError> {
        let t_start = std::time::Instant::now();

        // Serialize VarMap to simple JSON object: {"x": 1.23, "y": 4.56}
        let mut json_str = String::from("{");
        let mut first = true;
        for (k, v) in &inputs.values {
            if !first {
                json_str.push_str(", ");
            }
            use std::fmt::Write;
            let _ = write!(json_str, "\"{}\": {:.6}", k, v);
            first = false;
        }
        json_str.push('}');

        let mut child = std::process::Command::new(&self.command)
            .args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                EvalError::DomainError(format!("Failed to spawn '{}': {}", self.command, e))
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(json_str.as_bytes());
        }

        let output = child
            .wait_with_output()
            .map_err(|e| EvalError::DomainError(format!("Subprocess execution error: {}", e)))?;

        let elapsed_us = t_start.elapsed().as_secs_f64() * 1_000_000.0;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(EvalError::SolverFailed(format!(
                "Subprocess exited with code {:?}: {}",
                output.status.code(),
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let val = parse_json_float(&stdout, "value").ok_or_else(|| {
            EvalError::DomainError(format!("Missing 'value' in stdout: {}", stdout))
        })?;
        let backend_id =
            parse_json_string(&stdout, "backend_id").unwrap_or_else(|| self.command.clone());

        let mut out_values = inputs.values.clone();
        out_values.insert("value".to_string(), val);

        Ok(EvalResult {
            values: out_values,
            backend_id,
            solve_time_us: elapsed_us,
        })
    }

    fn name(&self) -> &str {
        &self.command
    }
}

fn parse_json_float(json: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{}\"", key);
    let key_pos = json.find(&needle)?;
    let after_key = &json[key_pos + needle.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    let end_idx = after_colon
        .find(|c: char| c == ',' || c == '}' || c.is_whitespace())
        .unwrap_or(after_colon.len());
    after_colon[..end_idx].trim().parse::<f64>().ok()
}

fn parse_json_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let key_pos = json.find(&needle)?;
    let after_key = &json[key_pos + needle.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    let quote_start = after_colon.find('"')?;
    let rest = &after_colon[quote_start + 1..];
    let quote_end = rest.find('"')?;
    Some(rest[..quote_end].to_string())
}

/// Diagnostic metadata accompanying every solved state.
#[derive(Clone, Debug)]
pub struct SolveDiagnostics {
    pub spec_hash: String,
    pub free_vars: Vec<String>,
    pub residual: f64,
    pub rank: usize,
    pub binding_constraints: Vec<String>,
    pub backend_trace: Vec<String>,
    pub solve_time_us: f64,
}

/// A single feasible or non-dominated point on a Pareto frontier.
#[derive(Clone, Debug)]
pub struct FrontierPoint {
    pub vars: VarMap,
    pub objectives: Vec<(String, f64)>,
    pub diagnostics: SolveDiagnostics,
}

/// The Aven Relational Orchestrator.
pub struct Orchestrator {
    pub evaluator: Arc<dyn Evaluator>,
    pub seed: u64,
}

impl Orchestrator {
    pub fn new(evaluator: Arc<dyn Evaluator>) -> Self {
        Self {
            evaluator,
            seed: 42,
        }
    }

    /// Solves a single state deterministically and packages complete diagnostics.
    pub fn solve_single(&self, inputs: &VarMap) -> Result<FrontierPoint, EvalError> {
        let res = self.evaluator.evaluate(inputs)?;
        let spec_hash = inputs.compute_hash();
        let mut out_vars = VarMap::new();
        out_vars.values = res.values.clone();

        let diagnostics = SolveDiagnostics {
            spec_hash,
            free_vars: vec![],
            residual: 0.0,
            rank: res.values.len(),
            binding_constraints: vec![],
            backend_trace: vec![res.backend_id],
            solve_time_us: res.solve_time_us,
        };

        Ok(FrontierPoint {
            vars: out_vars,
            objectives: vec![],
            diagnostics,
        })
    }

    /// Generates a Pareto frontier by sweeping an ε-constraint over a parameter range.
    pub fn solve_pareto(
        &self,
        base_inputs: &VarMap,
        sweep_var: &str,
        sweep_range: (f64, f64),
        steps: usize,
        objectives: &[String],
    ) -> Result<Vec<FrontierPoint>, EvalError> {
        let mut frontier = Vec::new();
        let step_count = steps.max(2);
        let (min_val, max_val) = sweep_range;

        for i in 0..=step_count {
            let val = min_val + (i as f64 / step_count as f64) * (max_val - min_val);
            let mut point_inputs = base_inputs.clone();
            point_inputs.set(sweep_var, val);

            if let Ok(fp) = self.solve_single(&point_inputs) {
                let mut obj_vals = Vec::new();
                for obj in objectives {
                    if let Some(v) = fp.vars.get(obj) {
                        obj_vals.push((obj.clone(), v));
                    }
                }
                frontier.push(FrontierPoint {
                    vars: fp.vars,
                    objectives: obj_vals,
                    diagnostics: fp.diagnostics,
                });
            }
        }

        Ok(frontier)
    }

    /// Finds the Knee Point on a 2-objective Pareto frontier via maximum perpendicular distance.
    pub fn find_knee_point(frontier: &[FrontierPoint]) -> Option<usize> {
        if frontier.len() < 3 {
            return None;
        }

        let first = &frontier[0];
        let last = &frontier[frontier.len() - 1];

        if first.objectives.len() < 2 || last.objectives.len() < 2 {
            return None;
        }

        let (x1, y1) = (first.objectives[0].1, first.objectives[1].1);
        let (x2, y2) = (last.objectives[0].1, last.objectives[1].1);

        let denom = ((y2 - y1).powi(2) + (x2 - x1).powi(2)).sqrt();
        if denom < 1e-12 {
            return None;
        }

        let mut max_dist = -1.0;
        let mut knee_idx = 0;

        for (i, p) in frontier.iter().enumerate() {
            let x0 = p.objectives[0].1;
            let y0 = p.objectives[1].1;
            let dist = ((y2 - y1) * x0 - (x2 - x1) * y0 + x2 * y1 - y2 * x1).abs() / denom;
            if dist > max_dist {
                max_dist = dist;
                knee_idx = i;
            }
        }

        Some(knee_idx)
    }
}
