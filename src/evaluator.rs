// src/evaluator.rs — Pluggable Evaluation Engine & Pareto Orchestrator for Aven
// Milestone v0.6.0: Evaluator Trait, NativeEvaluator, ExternalEvaluator, SHA-256 Spec Hashing, and ε-Constraint MOO.
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
    pub timestamp_ms: Option<u64>,
    pub ttl_ms: Option<u64>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EvalError {
    DomainError(String),
    Timeout(String),
    SolverFailed(String),
    BlackBoxFailure(String),
    MalformedOutput(String),
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalError::DomainError(msg) => write!(f, "Domain error: {}", msg),
            EvalError::Timeout(msg) => write!(f, "Timeout: {}", msg),
            EvalError::SolverFailed(msg) => write!(f, "Solver failed: {}", msg),
            EvalError::BlackBoxFailure(msg) => write!(f, "Black-box failure: {}", msg),
            EvalError::MalformedOutput(msg) => write!(f, "Malformed output: {}", msg),
        }
    }
}

impl std::error::Error for EvalError {}

/// Solve convergence status recorded in SolveDiagnostics.
#[derive(Clone, Debug, PartialEq)]
pub enum SolveStatus {
    Converged,
    BlackBoxFailure(String),
    DomainError(String),
    Timeout(String),
    SolverFailed(String),
}

impl std::fmt::Display for SolveStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolveStatus::Converged => write!(f, "Converged"),
            SolveStatus::BlackBoxFailure(msg) => write!(f, "Black-box failure: {}", msg),
            SolveStatus::DomainError(msg) => write!(f, "Domain error: {}", msg),
            SolveStatus::Timeout(msg) => write!(f, "Timeout: {}", msg),
            SolveStatus::SolverFailed(msg) => write!(f, "Solver failed: {}", msg),
        }
    }
}

/// Diagnostic metadata accompanying every solved or failed state.
#[derive(Clone, Debug)]
pub struct SolveDiagnostics {
    pub spec_hash: String,
    pub free_vars: Vec<String>,
    pub residual: f64,
    pub rank: usize,
    pub binding_constraints: Vec<String>,
    pub backend_trace: Vec<String>,
    pub solve_time_us: f64,
    pub status: SolveStatus,
}

/// Failure outcome carrying complete diagnostics for auditing even on error paths.
#[derive(Clone, Debug)]
pub struct SolveFailure {
    pub error: EvalError,
    pub diagnostics: Box<SolveDiagnostics>,
}

impl std::fmt::Display for SolveFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: status={}, spec_hash={}",
            self.error, self.diagnostics.status, self.diagnostics.spec_hash
        )
    }
}

impl std::error::Error for SolveFailure {}

/// A single feasible or non-dominated point on a Pareto frontier.
#[derive(Clone, Debug)]
pub struct FrontierPoint {
    pub vars: VarMap,
    pub objectives: Vec<(String, f64)>,
    pub diagnostics: SolveDiagnostics,
    pub is_feasible: bool,
}

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
            timestamp_ms: None,
            ttl_ms: None,
            warnings: Vec::new(),
        })
    }

    fn name(&self) -> &str {
        "native_analytic"
    }
}

/// External subprocess evaluator communicating via JSON over stdin/stdout/stderr.
pub struct ExternalEvaluator {
    pub command: String,
    pub args: Vec<String>,
    pub backend_id: String,
    pub working_dir: Option<std::path::PathBuf>,
    pub timeout: Option<std::time::Duration>,
    pub required_keys: Vec<String>,
}

impl ExternalEvaluator {
    pub fn new(command: &str, args: &[&str]) -> Self {
        Self {
            command: command.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            backend_id: command.to_string(),
            working_dir: None,
            timeout: None,
            required_keys: Vec::new(),
        }
    }

    pub fn with_backend_id(mut self, id: &str) -> Self {
        self.backend_id = id.to_string();
        self
    }

    pub fn with_working_dir<P: Into<std::path::PathBuf>>(mut self, dir: P) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_required_keys(mut self, keys: &[String]) -> Self {
        self.required_keys = keys.to_vec();
        self
    }
}

impl Evaluator for ExternalEvaluator {
    fn evaluate(&self, inputs: &VarMap) -> Result<EvalResult, EvalError> {
        let t_start = std::time::Instant::now();

        // Serialize VarMap to sorted JSON object: {"x": 0.5, "y": -0.2}
        let mut json_str = String::from("{");
        let mut first = true;
        let mut keys: Vec<&String> = inputs.values.keys().collect();
        keys.sort();
        for k in keys {
            if let Some(v) = inputs.values.get(k) {
                if !first {
                    json_str.push_str(", ");
                }
                use std::fmt::Write;
                let _ = write!(json_str, "\"{}\": {}", k, v);
                first = false;
            }
        }
        json_str.push('}');

        // Spawn child process with piped stdin/stdout/stderr
        let mut cmd = std::process::Command::new(&self.command);
        let mut resolved_args: Vec<String> = self.args.clone();
        let ws_env = std::env::var("AVEN_WORKSPACE")
            .ok()
            .map(std::path::PathBuf::from);
        let effective_dir = self.working_dir.as_ref().or(ws_env.as_ref());

        if let Some(dir) = effective_dir {
            for arg in &mut resolved_args {
                let p = std::path::Path::new(arg);
                if !dir.join(p).exists() && p.exists() {
                    if let Ok(abs) = std::fs::canonicalize(p) {
                        *arg = abs.to_string_lossy().to_string();
                    }
                } else if dir.join(p).exists() {
                    if let Ok(abs) = std::fs::canonicalize(dir.join(p)) {
                        *arg = abs.to_string_lossy().to_string();
                    }
                }
            }
            cmd.current_dir(dir);
        }
        cmd.args(&resolved_args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            EvalError::DomainError(format!("Failed to spawn '{}': {}", self.command, e))
        })?;

        // Write JSON payload to stdin
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            stdin.write_all(json_str.as_bytes()).map_err(|e| {
                EvalError::DomainError(format!(
                    "Failed to write to stdin of '{}': {}",
                    self.command, e
                ))
            })?;
        }

        // Wait for output and check exit status
        let output = child
            .wait_with_output()
            .map_err(|e| EvalError::DomainError(format!("Subprocess execution error: {}", e)))?;

        let elapsed_us = t_start.elapsed().as_secs_f64() * 1_000_000.0;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let trimmed = stderr.trim();
            let msg = if trimmed.is_empty() {
                format!("Subprocess exited with code {:?}", output.status.code())
            } else {
                trimmed.to_string()
            };
            return Err(EvalError::BlackBoxFailure(msg));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let trimmed_stdout = stdout.trim();

        if !trimmed_stdout.starts_with('{') || !trimmed_stdout.ends_with('}') {
            return Err(EvalError::MalformedOutput(format!(
                "Malformed output: adapter stdout must be a JSON object '{{...}}', found: '{}'",
                trimmed_stdout
            )));
        }

        let backend_id =
            parse_json_string(&stdout, "backend_id").unwrap_or_else(|| self.backend_id.clone());

        if let Some(err_msg) = parse_json_string(&stdout, "error") {
            return Err(EvalError::BlackBoxFailure(format!(
                "Adapter '{}' reported error: {}",
                backend_id, err_msg
            )));
        }

        let parsed_numbers = parse_json_object_numbers(&stdout);
        if parsed_numbers.is_empty() {
            return Err(EvalError::MalformedOutput(format!(
                "No numeric outputs found in adapter stdout: {}",
                stdout
            )));
        }

        // Schema validation: verify all required keys are present
        for req in &self.required_keys {
            if !parsed_numbers.contains_key(req) {
                return Err(EvalError::MalformedOutput(format!(
                    "Schema validation failed: external adapter '{}' omitted required numeric field '{}'",
                    backend_id, req
                )));
            }
        }

        let timestamp_ms = parse_json_u64(&stdout, "timestamp_ms")
            .or_else(|| parse_json_u64(&stdout, "timestamp"));
        let ttl_ms = parse_json_u64(&stdout, "ttl_ms")
            .or_else(|| parse_json_float(&stdout, "ttl").map(|s| (s * 1000.0) as u64));
        let warnings = parse_json_string_array(&stdout, "warnings");

        let mut out_values = inputs.values.clone();
        out_values.extend(parsed_numbers);
        if !out_values.contains_key("value") {
            let first_val = out_values.values().next().copied().unwrap_or(0.0);
            out_values.insert("value".to_string(), first_val);
        }

        Ok(EvalResult {
            values: out_values,
            backend_id,
            solve_time_us: elapsed_us,
            timestamp_ms,
            ttl_ms,
            warnings,
        })
    }

    fn name(&self) -> &str {
        &self.backend_id
    }
}

/// Extracts all numeric key-value pairs from a flat JSON object string.
pub fn parse_json_object_numbers(json: &str) -> HashMap<String, f64> {
    let mut map = HashMap::new();
    let bytes = json.as_bytes();
    let len = bytes.len();
    let mut idx = 0;

    while idx < len {
        if bytes[idx] == b'"' {
            let key_start = idx + 1;
            idx += 1;
            while idx < len && bytes[idx] != b'"' {
                if bytes[idx] == b'\\' && idx + 1 < len {
                    idx += 2;
                } else {
                    idx += 1;
                }
            }
            if idx >= len {
                break;
            }
            let key_end = idx;
            let key = String::from_utf8_lossy(&bytes[key_start..key_end]).to_string();
            idx += 1; // skip closing quote

            while idx < len && bytes[idx].is_ascii_whitespace() {
                idx += 1;
            }
            if idx < len && bytes[idx] == b':' {
                idx += 1; // skip ':'
                while idx < len && bytes[idx].is_ascii_whitespace() {
                    idx += 1;
                }
                if idx < len
                    && (bytes[idx].is_ascii_digit() || bytes[idx] == b'-' || bytes[idx] == b'+')
                {
                    let num_start = idx;
                    while idx < len
                        && (bytes[idx].is_ascii_digit()
                            || bytes[idx] == b'.'
                            || bytes[idx] == b'e'
                            || bytes[idx] == b'E'
                            || bytes[idx] == b'-'
                            || bytes[idx] == b'+')
                    {
                        idx += 1;
                    }
                    let num_str = String::from_utf8_lossy(&bytes[num_start..idx]);
                    if let Ok(num) = num_str.trim().parse::<f64>() {
                        map.insert(key, num);
                    }
                } else if idx + 4 <= len && &bytes[idx..idx + 4] == b"true" {
                    map.insert(key, 1.0);
                    idx += 4;
                } else if idx + 5 <= len && &bytes[idx..idx + 5] == b"false" {
                    map.insert(key, 0.0);
                    idx += 5;
                }
            }
        } else {
            idx += 1;
        }
    }
    map
}

pub fn parse_json_float(json: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{}\"", key);
    let mut search_idx = 0;
    while let Some(pos) = json[search_idx..].find(&needle) {
        let abs_pos = search_idx + pos;
        let after_key = &json[abs_pos + needle.len()..];
        let trimmed = after_key.trim_start();
        if let Some(stripped) = trimmed.strip_prefix(':') {
            let after_colon = stripped.trim_start();
            let end_idx = after_colon
                .find(|c: char| c == ',' || c == '}' || c.is_whitespace())
                .unwrap_or(after_colon.len());
            return after_colon[..end_idx].trim().parse::<f64>().ok();
        }
        search_idx = abs_pos + needle.len();
    }
    None
}

pub fn parse_json_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let mut search_idx = 0;
    while let Some(pos) = json[search_idx..].find(&needle) {
        let abs_pos = search_idx + pos;
        let after_key = &json[abs_pos + needle.len()..];
        let trimmed = after_key.trim_start();
        if let Some(stripped) = trimmed.strip_prefix(':') {
            let after_colon = stripped.trim_start();
            let quote_start = after_colon.find('"')?;
            let rest = &after_colon[quote_start + 1..];
            let quote_end = rest.find('"')?;
            return Some(rest[..quote_end].to_string());
        }
        search_idx = abs_pos + needle.len();
    }
    None
}

pub fn parse_json_u64(json: &str, key: &str) -> Option<u64> {
    parse_json_float(json, key).map(|v| v as u64)
}

pub fn parse_json_string_array(json: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{}\"", key);
    if let Some(pos) = json.find(&needle) {
        let after_key = &json[pos + needle.len()..];
        if let Some(bracket_start) = after_key.find('[') {
            if let Some(bracket_end) = after_key[bracket_start..].find(']') {
                let inside = &after_key[bracket_start + 1..bracket_start + bracket_end];
                let mut list = Vec::new();
                for part in inside.split(',') {
                    let trimmed = part
                        .trim()
                        .trim_matches(|c| c == '"' || c == '\'' || c == ' ');
                    if !trimmed.is_empty() {
                        list.push(trimmed.to_string());
                    }
                }
                return list;
            }
        }
    }
    Vec::new()
}

/// Backend configuration enum. Native Newton path is the zero-dependency default.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum EvaluatorBackend {
    /// Pure Rust continuous analytical Newton-Raphson solver (default).
    #[default]
    Native,
    /// External subprocess evaluator communicating via JSON over stdio.
    External {
        command: String,
        args: Vec<String>,
        backend_id: Option<String>,
        working_dir: Option<std::path::PathBuf>,
    },
}

/// Runtime configuration for the Orchestrator.
#[derive(Clone, Debug)]
pub struct OrchestratorConfig {
    pub backend: EvaluatorBackend,
    pub seed: u64,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            backend: EvaluatorBackend::Native,
            seed: 42,
        }
    }
}

/// The Aven Relational Orchestrator.
pub struct Orchestrator {
    pub evaluator: Arc<dyn Evaluator>,
    pub seed: u64,
    pub frontier_cache: std::sync::Mutex<HashMap<String, Vec<FrontierPoint>>>,
}

impl Orchestrator {
    pub fn new(evaluator: Arc<dyn Evaluator>) -> Self {
        Self {
            evaluator,
            seed: 42,
            frontier_cache: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Creates an Orchestrator from configuration, defaulting to Native Newton evaluation.
    pub fn from_config(config: &OrchestratorConfig, equations: Vec<Equation>) -> Self {
        match &config.backend {
            EvaluatorBackend::Native => {
                let eval = Arc::new(NativeEvaluator::new(equations));
                Self {
                    evaluator: eval,
                    seed: config.seed,
                    frontier_cache: std::sync::Mutex::new(HashMap::new()),
                }
            }
            EvaluatorBackend::External {
                command,
                args,
                backend_id,
                working_dir,
            } => {
                let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let mut ext = ExternalEvaluator::new(command, &arg_refs);
                if let Some(id) = backend_id {
                    ext = ext.with_backend_id(id);
                }
                if let Some(dir) = working_dir {
                    ext = ext.with_working_dir(dir.clone());
                }
                Self {
                    evaluator: Arc::new(ext),
                    seed: config.seed,
                    frontier_cache: std::sync::Mutex::new(HashMap::new()),
                }
            }
        }
    }

    /// Solves a single state deterministically and packages complete diagnostics.
    /// On success: returns Ok(FrontierPoint) with is_feasible = true.
    /// On error: returns Err(SolveFailure) which carries full SolveDiagnostics:
    ///   - spec_hash (deterministic SHA-256 of inputs)
    ///   - free_vars (input decision variables)
    ///   - backend_trace (backend identifier)
    ///   - residual: f64::NAN ("black-box failure" residual marker)
    ///   - status: SolveStatus::BlackBoxFailure(err)
    pub fn solve_single(&self, inputs: &VarMap) -> Result<FrontierPoint, SolveFailure> {
        let spec_hash = inputs.compute_hash();
        let mut free_vars: Vec<String> = inputs.values.keys().cloned().collect();
        free_vars.sort();
        let t_start = std::time::Instant::now();

        match self.evaluator.evaluate(inputs) {
            Ok(res) => {
                let mut out_vars = VarMap::new();
                out_vars.values = res.values.clone();

                let diagnostics = SolveDiagnostics {
                    spec_hash,
                    free_vars,
                    residual: 0.0,
                    rank: res.values.len(),
                    binding_constraints: vec![],
                    backend_trace: vec![res.backend_id],
                    solve_time_us: res.solve_time_us,
                    status: SolveStatus::Converged,
                };

                Ok(FrontierPoint {
                    vars: out_vars,
                    objectives: vec![],
                    diagnostics,
                    is_feasible: true,
                })
            }
            Err(e) => {
                let elapsed_us = t_start.elapsed().as_secs_f64() * 1_000_000.0;
                let backend_id = self.evaluator.name().to_string();

                let diagnostics = SolveDiagnostics {
                    spec_hash,
                    free_vars,
                    residual: f64::NAN, // "black-box failure" residual marker
                    rank: 0,
                    binding_constraints: vec![],
                    backend_trace: vec![backend_id],
                    solve_time_us: elapsed_us,
                    status: SolveStatus::BlackBoxFailure(e.to_string()),
                };

                Err(SolveFailure {
                    error: e,
                    diagnostics: Box::new(diagnostics),
                })
            }
        }
    }

    /// Solves a single point returning FrontierPoint directly with is_feasible flag and diagnostics intact.
    pub fn solve_point(&self, inputs: &VarMap) -> FrontierPoint {
        match self.solve_single(inputs) {
            Ok(pt) => pt,
            Err(failure) => FrontierPoint {
                vars: inputs.clone(),
                objectives: vec![],
                diagnostics: *failure.diagnostics,
                is_feasible: false,
            },
        }
    }

    /// Generates a Pareto frontier by sweeping an ε-constraint over a parameter range,
    /// backed by an in-memory cache keyed on input spec hash and sweep parameters.
    pub fn solve_pareto(
        &self,
        base_inputs: &VarMap,
        sweep_var: &str,
        sweep_range: (f64, f64),
        steps: usize,
        objectives: &[String],
    ) -> Result<Vec<FrontierPoint>, SolveFailure> {
        let (min_val, max_val) = sweep_range;
        let cache_key = format!(
            "{}:{}:{:.4}:{:.4}:{}",
            base_inputs.compute_hash(),
            sweep_var,
            min_val,
            max_val,
            steps
        );

        if let Ok(guard) = self.frontier_cache.lock() {
            if let Some(cached) = guard.get(&cache_key) {
                return Ok(cached.clone());
            }
        }

        let mut frontier = Vec::new();
        let step_count = steps.max(2);

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
                    is_feasible: true,
                });
            }
        }

        let filtered = Self::filter_epsilon_dominance(&frontier, 1e-5);
        if let Ok(mut guard) = self.frontier_cache.lock() {
            guard.insert(cache_key, filtered.clone());
        }

        Ok(filtered)
    }

    /// Filters candidate frontier points using ε-box dominance to remove redundant or near-duplicate solutions.
    pub fn filter_epsilon_dominance(candidates: &[FrontierPoint], eps: f64) -> Vec<FrontierPoint> {
        if candidates.len() <= 1 {
            return candidates.to_vec();
        }

        let mut non_dominated: Vec<FrontierPoint> = Vec::new();
        for p in candidates {
            if !p.is_feasible || p.objectives.is_empty() {
                continue;
            }

            // Check if p is epsilon-dominated by any point already accepted in non_dominated
            let dominated = non_dominated.iter().any(|existing| {
                if existing.objectives.len() != p.objectives.len() {
                    return false;
                }
                // If all objectives are within eps, existing covers p
                existing
                    .objectives
                    .iter()
                    .zip(&p.objectives)
                    .all(|((_, val_e), (_, val_p))| (val_e - val_p).abs() <= eps)
            });

            if !dominated {
                non_dominated.push(p.clone());
            }
        }

        if non_dominated.is_empty() {
            candidates.to_vec()
        } else {
            non_dominated
        }
    }

    /// Finds the Knee Point on a multi-objective Pareto frontier via normalized trade-off curvature.
    pub fn find_knee_point(frontier: &[FrontierPoint]) -> Option<usize> {
        if frontier.is_empty() {
            return None;
        }
        if frontier.len() <= 2 {
            return Some(0);
        }

        let n_objs = frontier[0].objectives.len();
        if n_objs < 2 {
            return Some(0);
        }

        // Find the pair of objectives with the strongest trade-off (negative correlation)
        let mut best_pair = (0, 1);
        let mut best_tradeoff_score = f64::NEG_INFINITY;

        for j1 in 0..n_objs {
            for j2 in (j1 + 1)..n_objs {
                let vals1: Vec<f64> = frontier.iter().map(|p| p.objectives[j1].1).collect();
                let vals2: Vec<f64> = frontier.iter().map(|p| p.objectives[j2].1).collect();

                let min1 = vals1.iter().cloned().fold(f64::INFINITY, f64::min);
                let max1 = vals1.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let min2 = vals2.iter().cloned().fold(f64::INFINITY, f64::min);
                let max2 = vals2.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

                let span1 = max1 - min1;
                let span2 = max2 - min2;

                if span1 < 1e-9 || span2 < 1e-9 {
                    continue;
                }

                let d1 = vals1.last().unwrap() - vals1.first().unwrap();
                let d2 = vals2.last().unwrap() - vals2.first().unwrap();

                let is_conflicting = (d1 * d2) < 0.0;
                let score = if is_conflicting {
                    1.0 + (span1.abs() * span2.abs())
                } else {
                    span1.abs() + span2.abs()
                };

                if score > best_tradeoff_score {
                    best_tradeoff_score = score;
                    best_pair = (j1, j2);
                }
            }
        }

        let (j1, j2) = best_pair;

        let vals1: Vec<f64> = frontier.iter().map(|p| p.objectives[j1].1).collect();
        let vals2: Vec<f64> = frontier.iter().map(|p| p.objectives[j2].1).collect();

        let min1 = vals1.iter().cloned().fold(f64::INFINITY, f64::min);
        let max1 = vals1.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min2 = vals2.iter().cloned().fold(f64::INFINITY, f64::min);
        let max2 = vals2.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        let span1 = (max1 - min1).max(1e-12);
        let span2 = (max2 - min2).max(1e-12);

        let x1 = (vals1[0] - min1) / span1;
        let y1 = (vals2[0] - min2) / span2;
        let x2 = (vals1[vals1.len() - 1] - min1) / span1;
        let y2 = (vals2[vals2.len() - 1] - min2) / span2;

        let denom = ((y2 - y1).powi(2) + (x2 - x1).powi(2)).sqrt();
        if denom < 1e-12 {
            return Some(frontier.len() / 2);
        }

        let mut max_dist = -1.0;
        let mut knee_idx = frontier.len() / 2;

        // Scan interior points (indices 1 to n-2) to pick the true balanced trade-off
        for i in 1..(frontier.len() - 1) {
            let x0 = (frontier[i].objectives[j1].1 - min1) / span1;
            let y0 = (frontier[i].objectives[j2].1 - min2) / span2;
            let dist = ((y2 - y1) * x0 - (x2 - x1) * y0 + x2 * y1 - y2 * x1).abs() / denom;
            if dist > max_dist {
                max_dist = dist;
                knee_idx = i;
            }
        }

        Some(knee_idx)
    }
}
