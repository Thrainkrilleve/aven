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

/// JSON keys that carry freshness metadata rather than model values
const FRESHNESS_KEYS: &[&str] = &["timestamp_ms", "timestamp", "ttl_ms", "ttl"];

/// Evaluation result returned by any backend.
#[derive(Clone, Debug)]
pub struct EvalResult {
    pub values: HashMap<String, f64>,
    pub backend_id: String,
    pub solve_time_us: f64,
    pub timestamp_ms: Option<u64>,
    pub ttl_ms: Option<u64>,
    pub warnings: Vec<String>,
    /// SHA-256 of the raw bytes the backend returned (None for in-process evaluation)
    pub payload_sha256: Option<String>,
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
            payload_sha256: None,
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
    pub schema_path: Option<std::path::PathBuf>,
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
            schema_path: None,
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

    pub fn with_schema_path<P: Into<std::path::PathBuf>>(mut self, path: P) -> Self {
        self.schema_path = Some(path.into());
        self
    }
}

/// Lightweight JSON Schema contract validator for external adapter stdout (pure std).
#[derive(Clone, Debug, Default)]
pub struct JsonSchema {
    pub required: Vec<String>,
    pub number_properties: HashMap<String, NumberPropertySchema>,
}

#[derive(Clone, Debug, Default)]
pub struct NumberPropertySchema {
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
}

impl JsonSchema {
    pub fn parse(schema_str: &str) -> Self {
        let required = parse_json_string_array(schema_str, "required");
        let mut number_properties = HashMap::new();

        // Scan "properties": { ... }
        if let Some(pos) = schema_str.find("\"properties\"") {
            let rest = &schema_str[pos + 12..];
            if let Some(brace_start) = rest.find('{') {
                let inside = &rest[brace_start + 1..];
                for key in &required {
                    let prop_needle = format!("\"{}\"", key);
                    if let Some(kpos) = inside.find(&prop_needle) {
                        let prop_block = &inside[kpos + prop_needle.len()..];
                        if let Some(bstart) = prop_block.find('{') {
                            if let Some(bend) = prop_block[bstart..].find('}') {
                                let block = &prop_block[bstart..bstart + bend + 1];
                                let min_val = parse_json_float(block, "minimum");
                                let max_val = parse_json_float(block, "maximum");
                                number_properties.insert(
                                    key.clone(),
                                    NumberPropertySchema {
                                        minimum: min_val,
                                        maximum: max_val,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }

        Self {
            required,
            number_properties,
        }
    }

    pub fn validate(
        &self,
        parsed_numbers: &HashMap<String, f64>,
        backend_id: &str,
    ) -> Result<(), EvalError> {
        // 1. Check required keys
        for req in &self.required {
            if !parsed_numbers.contains_key(req) {
                return Err(EvalError::MalformedOutput(format!(
                    "JSON Schema validation failed for adapter '{}': missing required property '{}'",
                    backend_id, req
                )));
            }
        }

        // 2. Check property constraints (minimum / maximum bounds)
        for (prop, schema) in &self.number_properties {
            if let Some(&val) = parsed_numbers.get(prop) {
                if let Some(min) = schema.minimum {
                    if val < min {
                        return Err(EvalError::MalformedOutput(format!(
                            "JSON Schema validation failed for adapter '{}': property '{}' value {:.4} is below schema minimum {:.4}",
                            backend_id, prop, val, min
                        )));
                    }
                }
                if let Some(max) = schema.maximum {
                    if val > max {
                        return Err(EvalError::MalformedOutput(format!(
                            "JSON Schema validation failed for adapter '{}': property '{}' value {:.4} exceeds schema maximum {:.4}",
                            backend_id, prop, val, max
                        )));
                    }
                }
            }
        }

        Ok(())
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
            stdin.flush().map_err(|e| {
                EvalError::DomainError(format!(
                    "Failed to flush stdin of '{}': {}",
                    self.command, e
                ))
            })?;
            // Dropping the pipe is EOF. The adapter reads stdin to completion.
            drop(stdin);
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

        // Discover or use explicit JSON Schema file if present
        let mut active_schema: Option<JsonSchema> = None;
        if let Some(sp) = &self.schema_path {
            if let Ok(content) = std::fs::read_to_string(sp) {
                active_schema = Some(JsonSchema::parse(&content));
            }
        } else {
            // Auto-discover sibling schema file: e.g. adapter.py -> adapter.schema.json
            for arg in &self.args {
                let p = std::path::Path::new(arg);
                let schema_sibling = p.with_extension("schema.json");
                let candidate = if let Some(dir) = &self.working_dir {
                    dir.join(&schema_sibling)
                } else {
                    schema_sibling
                };
                if candidate.exists() {
                    if let Ok(content) = std::fs::read_to_string(&candidate) {
                        active_schema = Some(JsonSchema::parse(&content));
                        break;
                    }
                }
            }
        }

        if let Some(ref schema) = active_schema {
            schema.validate(&parsed_numbers, &backend_id)?;
        } else {
            // Fallback: Validate all required_keys are present
            for req in &self.required_keys {
                if !parsed_numbers.contains_key(req) {
                    return Err(EvalError::MalformedOutput(format!(
                        "Schema validation failed: external adapter '{}' omitted required numeric field '{}'",
                        backend_id, req
                    )));
                }
            }
        }

        let timestamp_ms = parse_json_u64(&stdout, "timestamp_ms")
            .or_else(|| parse_json_u64(&stdout, "timestamp"));
        let ttl_ms = parse_json_u64(&stdout, "ttl_ms")
            .or_else(|| parse_json_float(&stdout, "ttl").map(|s| (s * 1000.0) as u64));
        let warnings = parse_json_string_array(&stdout, "warnings");

        let mut out_values = inputs.values.clone();
        out_values.extend(parsed_numbers);
        // Freshness metadata is not a model variable
        for meta_key in FRESHNESS_KEYS {
            out_values.remove(*meta_key);
        }

        Ok(EvalResult {
            values: out_values,
            backend_id,
            solve_time_us: elapsed_us,
            timestamp_ms,
            ttl_ms,
            warnings,
            payload_sha256: Some(sha256_hex(&output.stdout)),
        })
    }

    fn name(&self) -> &str {
        &self.backend_id
    }
}

/// Persistent IPC evaluator communicating over local TCP socket stream (pure std).
pub struct IpcEvaluator {
    pub endpoint: String,
    pub backend_id: String,
    pub required_keys: Vec<String>,
}

impl IpcEvaluator {
    pub fn new(endpoint: &str) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            backend_id: format!("ipc_{}", endpoint),
            required_keys: Vec::new(),
        }
    }

    pub fn with_required_keys(mut self, keys: &[String]) -> Self {
        self.required_keys = keys.to_vec();
        self
    }

    /// Streams real-time actuation commands back to the IPC daemon.
    pub fn actuate(&self, commands: &HashMap<String, f64>) -> Result<(), EvalError> {
        use std::io::Write;
        use std::net::TcpStream;

        let addr_str = if !self.endpoint.contains(':') {
            format!("127.0.0.1:{}", self.endpoint)
        } else if self.endpoint.starts_with("localhost:") {
            self.endpoint.replacen("localhost:", "127.0.0.1:", 1)
        } else {
            self.endpoint.clone()
        };

        let socket_addr: std::net::SocketAddr = addr_str.parse().map_err(|e| {
            EvalError::DomainError(format!("Invalid IPC socket address '{}': {}", addr_str, e))
        })?;

        let mut stream =
            TcpStream::connect_timeout(&socket_addr, std::time::Duration::from_millis(500))
                .map_err(|e| {
                    EvalError::BlackBoxFailure(format!(
                        "Failed to connect to IPC daemon at '{}' for actuation: {}",
                        addr_str, e
                    ))
                })?;
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(1)));

        let mut json_str = String::from("{\"action\": \"actuate\", \"commands\": {");
        let mut first = true;
        let mut sorted_keys: Vec<&String> = commands.keys().collect();
        sorted_keys.sort();
        for k in sorted_keys {
            if !first {
                json_str.push_str(", ");
            }
            use std::fmt::Write as FmtWrite;
            let _ = write!(json_str, "\"{}\": {}", k, commands[k]);
            first = false;
        }
        json_str.push_str("}}\n");

        stream.write_all(json_str.as_bytes()).map_err(|e| {
            EvalError::DomainError(format!("Failed to send actuation packet: {}", e))
        })?;
        stream
            .flush()
            .map_err(|e| EvalError::DomainError(format!("Failed to flush socket: {}", e)))?;
        Ok(())
    }
}

impl Evaluator for IpcEvaluator {
    fn evaluate(&self, inputs: &VarMap) -> Result<EvalResult, EvalError> {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpStream;

        let t_start = std::time::Instant::now();

        // Resolve host and port (e.g. "9090" -> "127.0.0.1:9090", "localhost:9090" -> "127.0.0.1:9090")
        let addr_str = if !self.endpoint.contains(':') {
            format!("127.0.0.1:{}", self.endpoint)
        } else if self.endpoint.starts_with("localhost:") {
            self.endpoint.replacen("localhost:", "127.0.0.1:", 1)
        } else {
            self.endpoint.clone()
        };

        let socket_addr: std::net::SocketAddr = addr_str.parse().map_err(|e| {
            EvalError::DomainError(format!("Invalid IPC socket address '{}': {}", addr_str, e))
        })?;

        let stream = TcpStream::connect_timeout(&socket_addr, std::time::Duration::from_millis(500)).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to connect to IPC daemon at '{}': {}. Ensure the persistent daemon is running.",
                addr_str, e
            ))
        })?;

        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(3)));
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(3)));

        let mut reader = BufReader::new(stream.try_clone().map_err(|e| {
            EvalError::DomainError(format!("Failed to clone socket stream: {}", e))
        })?);
        let mut writer = stream;

        // Serialize inputs to JSON payload
        let mut json_str = String::from("{");
        let mut first = true;
        let mut keys: Vec<&String> = inputs.values.keys().collect();
        keys.sort();
        for k in keys {
            if let Some(v) = inputs.values.get(k) {
                if !first {
                    json_str.push_str(", ");
                }
                use std::fmt::Write as FmtWrite;
                let _ = write!(json_str, "\"{}\": {}", k, v);
                first = false;
            }
        }
        if let Ok(f_idx) = std::env::var("AVEN_FLIGHT_INDEX") {
            if !first {
                json_str.push_str(", ");
            }
            json_str.push_str(&format!("\"flight_index\": {}", f_idx));
            first = false;
        }
        if let Ok(cs) = std::env::var("AVEN_FLIGHT_CALLSIGN") {
            if !first {
                json_str.push_str(", ");
            }
            json_str.push_str(&format!("\"callsign\": \"{}\"", cs));
        }
        json_str.push_str("}\n");

        // Send query to daemon
        writer.write_all(json_str.as_bytes()).map_err(|e| {
            EvalError::DomainError(format!("Failed to send query to IPC daemon: {}", e))
        })?;
        writer
            .flush()
            .map_err(|e| EvalError::DomainError(format!("Failed to flush IPC socket: {}", e)))?;

        // Read response line
        let mut response_line = String::new();
        reader.read_line(&mut response_line).map_err(|e| {
            EvalError::BlackBoxFailure(format!("Failed to read response from IPC daemon: {}", e))
        })?;

        let elapsed_us = t_start.elapsed().as_secs_f64() * 1_000_000.0;
        let trimmed = response_line.trim();

        if trimmed.is_empty() {
            return Err(EvalError::BlackBoxFailure(format!(
                "IPC daemon at '{}' closed connection without returning a response",
                addr_str
            )));
        }

        if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
            return Err(EvalError::MalformedOutput(format!(
                "Malformed output: IPC daemon response must be a JSON object '{{...}}', found: '{}'",
                trimmed
            )));
        }

        let backend_id =
            parse_json_string(trimmed, "backend_id").unwrap_or_else(|| self.backend_id.clone());

        if let Some(err_msg) = parse_json_string(trimmed, "error") {
            return Err(EvalError::BlackBoxFailure(format!(
                "IPC daemon '{}' reported error: {}",
                backend_id, err_msg
            )));
        }

        let parsed_numbers = parse_json_object_numbers(trimmed);
        if parsed_numbers.is_empty() {
            return Err(EvalError::MalformedOutput(format!(
                "Malformed output: IPC daemon '{}' returned no numeric key-value pairs",
                backend_id
            )));
        }

        // Schema validation: ensure required keys exist
        for req in &self.required_keys {
            if !parsed_numbers.contains_key(req) {
                return Err(EvalError::MalformedOutput(format!(
                    "Schema validation failed: IPC daemon '{}' omitted required numeric field '{}'",
                    backend_id, req
                )));
            }
        }

        let timestamp_ms = parse_json_u64(trimmed, "timestamp_ms")
            .or_else(|| parse_json_u64(trimmed, "timestamp"));
        let ttl_ms = parse_json_u64(trimmed, "ttl_ms")
            .or_else(|| parse_json_float(trimmed, "ttl").map(|s| (s * 1000.0) as u64));
        let warnings = parse_json_string_array(trimmed, "warnings");

        let mut out_values = inputs.values.clone();
        out_values.extend(parsed_numbers);
        // Freshness metadata is not a model variable
        for meta_key in FRESHNESS_KEYS {
            out_values.remove(*meta_key);
        }

        Ok(EvalResult {
            values: out_values,
            backend_id,
            solve_time_us: elapsed_us,
            timestamp_ms,
            ttl_ms,
            warnings,
            payload_sha256: Some(sha256_hex(response_line.as_bytes())),
        })
    }

    fn name(&self) -> &str {
        &self.backend_id
    }
}

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Pure standard library Shared Memory Ring Buffer.
/// Supports zero-socket IPC, sub-100µs telemetry ingestion, and bidirectional reverse actuation.
#[derive(Debug)]
pub struct ShmRingBuffer {
    pub file_path: PathBuf,
    file: File,
    pub capacity: usize,
    pub slot_count: usize,
    pub slot_size: usize,
}

impl ShmRingBuffer {
    const MAGIC: &'static [u8; 16] = b"AVEN_SHM_RING_01";
    const HEADER_SIZE: usize = 64;
    const ACTUATION_ZONE_SIZE: usize = 448; // offsets 64..512
    const SLOTS_OFFSET: usize = 512;

    /// Resolves an SHM name or path to an absolute path.
    pub fn resolve_path(path_str: &str) -> PathBuf {
        let p = Path::new(path_str);
        if p.is_absolute()
            || path_str.contains('/')
            || path_str.contains('\\')
            || path_str.contains(':')
        {
            p.to_path_buf()
        } else {
            std::env::temp_dir().join(format!("{}.shm", path_str))
        }
    }

    /// Creates or opens an SHM ring buffer file with given slot count and slot size.
    pub fn create_or_open(path: &Path, slot_count: u32, slot_size: u32) -> Result<Self, EvalError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| {
                EvalError::BlackBoxFailure(format!(
                    "Failed to open/create SHM ring buffer at '{:?}': {}",
                    path, e
                ))
            })?;

        let total_needed = Self::SLOTS_OFFSET + (slot_count as usize) * (slot_size as usize);
        let existing_len = file.metadata().map(|m| m.len()).unwrap_or(0);

        if existing_len < total_needed as u64 {
            let mut init_buf = vec![0u8; total_needed];
            init_buf[0..16].copy_from_slice(Self::MAGIC);
            init_buf[16..20].copy_from_slice(&(total_needed as u32).to_le_bytes());
            init_buf[20..24].copy_from_slice(&slot_count.to_le_bytes());
            init_buf[24..28].copy_from_slice(&slot_size.to_le_bytes());

            file.seek(SeekFrom::Start(0)).map_err(|e| {
                EvalError::BlackBoxFailure(format!(
                    "Failed to seek in SHM file '{:?}': {}",
                    path, e
                ))
            })?;
            file.write_all(&init_buf).map_err(|e| {
                EvalError::BlackBoxFailure(format!(
                    "Failed to initialize SHM file '{:?}': {}",
                    path, e
                ))
            })?;
            file.flush().map_err(|e| {
                EvalError::BlackBoxFailure(format!("Failed to flush SHM file '{:?}': {}", path, e))
            })?;
        }

        Ok(Self {
            file_path: path.to_path_buf(),
            file,
            capacity: total_needed,
            slot_count: slot_count as usize,
            slot_size: slot_size as usize,
        })
    }

    /// Opens an existing SHM ring buffer file.
    pub fn open(path: &Path) -> Result<Self, EvalError> {
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| {
                EvalError::BlackBoxFailure(format!(
                    "Failed to open SHM ring buffer at '{:?}': {}. Ensure the producer process created it.",
                    path, e
                ))
            })?;

        let mut header = [0u8; Self::HEADER_SIZE];
        file.seek(SeekFrom::Start(0)).map_err(|e| {
            EvalError::BlackBoxFailure(format!("Failed to seek header in SHM '{:?}': {}", path, e))
        })?;
        file.read_exact(&mut header).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to read header from SHM '{:?}': {}",
                path, e
            ))
        })?;

        if &header[0..16] != Self::MAGIC {
            return Err(EvalError::MalformedOutput(format!(
                "Invalid magic header in SHM file '{:?}', expected {:?}",
                path,
                Self::MAGIC
            )));
        }

        let capacity = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
        let slot_count = u32::from_le_bytes(header[20..24].try_into().unwrap()) as usize;
        let slot_size = u32::from_le_bytes(header[24..28].try_into().unwrap()) as usize;

        if slot_count == 0 || slot_size < 32 {
            return Err(EvalError::MalformedOutput(format!(
                "Corrupt SHM ring buffer header in '{:?}': slot_count={}, slot_size={}",
                path, slot_count, slot_size
            )));
        }

        Ok(Self {
            file_path: path.to_path_buf(),
            file,
            capacity,
            slot_count,
            slot_size,
        })
    }

    /// Producer writes a new telemetry frame into the next circular slot.
    pub fn write_telemetry(
        &mut self,
        json_payload: &str,
        timestamp_ns: u64,
    ) -> Result<u64, EvalError> {
        let payload_bytes = json_payload.as_bytes();
        if payload_bytes.len() + 20 > self.slot_size {
            return Err(EvalError::DomainError(format!(
                "Telemetry payload length ({} bytes) exceeds slot payload capacity ({} bytes)",
                payload_bytes.len(),
                self.slot_size - 20
            )));
        }

        // Read current write_seq
        self.file.seek(SeekFrom::Start(28)).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to seek write_seq in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        let mut seq_buf = [0u8; 8];
        self.file.read_exact(&mut seq_buf).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to read write_seq in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        let next_seq = u64::from_le_bytes(seq_buf).wrapping_add(1);

        let slot_idx = (next_seq % (self.slot_count as u64)) as usize;
        let slot_offset = Self::SLOTS_OFFSET + slot_idx * self.slot_size;

        // Write slot payload: slot_seq (8), timestamp_ns (8), payload_len (4), payload_bytes
        self.file
            .seek(SeekFrom::Start(slot_offset as u64))
            .map_err(|e| {
                EvalError::BlackBoxFailure(format!(
                    "Failed to seek slot in '{:?}': {}",
                    self.file_path, e
                ))
            })?;
        self.file.write_all(&next_seq.to_le_bytes()).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to write slot seq in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        self.file
            .write_all(&timestamp_ns.to_le_bytes())
            .map_err(|e| {
                EvalError::BlackBoxFailure(format!(
                    "Failed to write slot timestamp in '{:?}': {}",
                    self.file_path, e
                ))
            })?;
        self.file
            .write_all(&(payload_bytes.len() as u32).to_le_bytes())
            .map_err(|e| {
                EvalError::BlackBoxFailure(format!(
                    "Failed to write slot payload length in '{:?}': {}",
                    self.file_path, e
                ))
            })?;
        self.file.write_all(payload_bytes).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to write slot payload in '{:?}': {}",
                self.file_path, e
            ))
        })?;

        // Update header write_seq
        self.file.seek(SeekFrom::Start(28)).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to seek write_seq header in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        self.file.write_all(&next_seq.to_le_bytes()).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to commit write_seq in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        self.file.flush().map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to flush SHM write in '{:?}': {}",
                self.file_path, e
            ))
        })?;

        Ok(next_seq)
    }

    /// Consumer reads the latest telemetry frame from the circular slot buffer.
    pub fn read_latest(&self) -> Result<(u64, u64, String), EvalError> {
        let mut file = self.file.try_clone().map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to clone file handle for SHM '{:?}': {}",
                self.file_path, e
            ))
        })?;

        file.seek(SeekFrom::Start(28)).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to seek write_seq in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        let mut seq_buf = [0u8; 8];
        file.read_exact(&mut seq_buf).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to read write_seq in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        let write_seq = u64::from_le_bytes(seq_buf);

        if write_seq == 0 {
            return Err(EvalError::BlackBoxFailure(format!(
                "SHM ring buffer '{:?}' has no telemetry frames written yet (write_seq == 0)",
                self.file_path
            )));
        }

        let slot_idx = (write_seq % (self.slot_count as u64)) as usize;
        let slot_offset = Self::SLOTS_OFFSET + slot_idx * self.slot_size;

        file.seek(SeekFrom::Start(slot_offset as u64))
            .map_err(|e| {
                EvalError::BlackBoxFailure(format!(
                    "Failed to seek slot in '{:?}': {}",
                    self.file_path, e
                ))
            })?;

        let mut slot_seq_buf = [0u8; 8];
        file.read_exact(&mut slot_seq_buf).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to read slot_seq in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        let slot_seq = u64::from_le_bytes(slot_seq_buf);

        let mut ts_buf = [0u8; 8];
        file.read_exact(&mut ts_buf).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to read slot timestamp in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        let timestamp_ns = u64::from_le_bytes(ts_buf);

        let mut len_buf = [0u8; 4];
        file.read_exact(&mut len_buf).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to read slot len in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        let payload_len = u32::from_le_bytes(len_buf) as usize;

        if payload_len + 20 > self.slot_size {
            return Err(EvalError::MalformedOutput(format!(
                "Corrupt slot payload length {} in SHM frame {}",
                payload_len, slot_seq
            )));
        }

        let mut payload_buf = vec![0u8; payload_len];
        file.read_exact(&mut payload_buf).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to read slot payload in '{:?}': {}",
                self.file_path, e
            ))
        })?;

        let payload_str = String::from_utf8(payload_buf).map_err(|e| {
            EvalError::MalformedOutput(format!(
                "Invalid UTF-8 in SHM telemetry frame {}: {}",
                slot_seq, e
            ))
        })?;

        Ok((slot_seq, timestamp_ns, payload_str))
    }

    /// Writes reverse actuation commands into the SHM actuation zone.
    pub fn write_actuation(&mut self, json_payload: &str) -> Result<u64, EvalError> {
        let payload_bytes = json_payload.as_bytes();
        if payload_bytes.len() > Self::ACTUATION_ZONE_SIZE {
            return Err(EvalError::DomainError(format!(
                "Actuation payload length ({} bytes) exceeds SHM actuation zone limit ({} bytes)",
                payload_bytes.len(),
                Self::ACTUATION_ZONE_SIZE
            )));
        }

        // Read current actuation_seq
        self.file.seek(SeekFrom::Start(36)).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to seek actuation_seq in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        let mut seq_buf = [0u8; 8];
        self.file.read_exact(&mut seq_buf).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to read actuation_seq in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        let next_act_seq = u64::from_le_bytes(seq_buf).wrapping_add(1);

        // Write actuation payload into offset 64..512
        self.file
            .seek(SeekFrom::Start(Self::HEADER_SIZE as u64))
            .map_err(|e| {
                EvalError::BlackBoxFailure(format!(
                    "Failed to seek actuation zone in '{:?}': {}",
                    self.file_path, e
                ))
            })?;
        self.file.write_all(payload_bytes).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to write actuation payload in '{:?}': {}",
                self.file_path, e
            ))
        })?;

        // Update actuation_seq and actuation_len
        self.file.seek(SeekFrom::Start(36)).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to seek actuation_seq header in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        self.file
            .write_all(&next_act_seq.to_le_bytes())
            .map_err(|e| {
                EvalError::BlackBoxFailure(format!(
                    "Failed to commit actuation_seq in '{:?}': {}",
                    self.file_path, e
                ))
            })?;
        self.file
            .write_all(&(payload_bytes.len() as u32).to_le_bytes())
            .map_err(|e| {
                EvalError::BlackBoxFailure(format!(
                    "Failed to commit actuation_len in '{:?}': {}",
                    self.file_path, e
                ))
            })?;
        self.file.flush().map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to flush actuation in '{:?}': {}",
                self.file_path, e
            ))
        })?;

        Ok(next_act_seq)
    }

    /// Reads the latest reverse actuation commands from the SHM actuation zone.
    pub fn read_latest_actuation(&self) -> Result<(u64, String), EvalError> {
        let mut file = self.file.try_clone().map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to clone file for SHM '{:?}': {}",
                self.file_path, e
            ))
        })?;

        file.seek(SeekFrom::Start(36)).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to seek actuation_seq in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        let mut seq_buf = [0u8; 8];
        file.read_exact(&mut seq_buf).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to read actuation_seq in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        let act_seq = u64::from_le_bytes(seq_buf);

        let mut len_buf = [0u8; 4];
        file.read_exact(&mut len_buf).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to read actuation_len in '{:?}': {}",
                self.file_path, e
            ))
        })?;
        let act_len = u32::from_le_bytes(len_buf) as usize;

        if act_seq == 0 || act_len == 0 {
            return Ok((0, String::new()));
        }

        if act_len > Self::ACTUATION_ZONE_SIZE {
            return Err(EvalError::MalformedOutput(format!(
                "Invalid actuation length {} in SHM '{:?}'",
                act_len, self.file_path
            )));
        }

        file.seek(SeekFrom::Start(Self::HEADER_SIZE as u64))
            .map_err(|e| {
                EvalError::BlackBoxFailure(format!(
                    "Failed to seek actuation zone in '{:?}': {}",
                    self.file_path, e
                ))
            })?;
        let mut payload_buf = vec![0u8; act_len];
        file.read_exact(&mut payload_buf).map_err(|e| {
            EvalError::BlackBoxFailure(format!(
                "Failed to read actuation payload in '{:?}': {}",
                self.file_path, e
            ))
        })?;

        let payload_str = String::from_utf8(payload_buf).map_err(|e| {
            EvalError::MalformedOutput(format!("Invalid UTF-8 in SHM actuation packet: {}", e))
        })?;

        Ok((act_seq, payload_str))
    }
}

/// Shared-memory ring buffer evaluator driving latency below 100µs.
pub struct ShmEvaluator {
    pub path: PathBuf,
    pub backend_id: String,
    pub required_keys: Vec<String>,
    ring: std::sync::Mutex<Option<ShmRingBuffer>>,
}

impl ShmEvaluator {
    pub fn new(path_str: &str) -> Self {
        let path = ShmRingBuffer::resolve_path(path_str);
        let id_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(path_str)
            .to_string();
        Self {
            backend_id: format!("shm_{}", id_name),
            path,
            required_keys: Vec::new(),
            ring: std::sync::Mutex::new(None),
        }
    }

    pub fn with_required_keys(mut self, keys: &[String]) -> Self {
        self.required_keys = keys.to_vec();
        self
    }

    /// Stream reverse actuation commands back into the SHM actuation zone.
    pub fn actuate(&self, commands: &HashMap<String, f64>) -> Result<u64, EvalError> {
        let mut json_str = String::from("{");
        let mut first = true;
        let mut sorted_keys: Vec<&String> = commands.keys().collect();
        sorted_keys.sort();
        for k in sorted_keys {
            if !first {
                json_str.push_str(", ");
            }
            use std::fmt::Write as FmtWrite;
            let _ = write!(json_str, "\"{}\": {}", k, commands[k]);
            first = false;
        }
        json_str.push('}');

        let mut guard = self.ring.lock().map_err(|e| {
            EvalError::BlackBoxFailure(format!("Failed to lock SHM ring buffer mutex: {}", e))
        })?;
        if guard.is_none() {
            *guard = Some(ShmRingBuffer::open(&self.path)?);
        }
        guard.as_mut().unwrap().write_actuation(&json_str)
    }
}

impl Evaluator for ShmEvaluator {
    fn evaluate(&self, inputs: &VarMap) -> Result<EvalResult, EvalError> {
        let mut guard = self.ring.lock().map_err(|e| {
            EvalError::BlackBoxFailure(format!("Failed to lock SHM ring buffer mutex: {}", e))
        })?;
        if guard.is_none() {
            *guard = Some(ShmRingBuffer::open(&self.path)?);
        }
        let ring = guard.as_mut().unwrap();

        let t_start = std::time::Instant::now();
        let (seq, ts_ns, payload) = ring.read_latest()?;
        let elapsed_us = t_start.elapsed().as_secs_f64() * 1_000_000.0;

        let trimmed = payload.trim();
        if trimmed.is_empty() {
            return Err(EvalError::BlackBoxFailure(format!(
                "SHM ring buffer '{:?}' contained empty payload in frame {}",
                self.path, seq
            )));
        }

        if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
            return Err(EvalError::MalformedOutput(format!(
                "Malformed output: SHM frame {} payload must be a JSON object, found: '{}'",
                seq, trimmed
            )));
        }

        let parsed_numbers = parse_json_object_numbers(trimmed);
        if parsed_numbers.is_empty() {
            return Err(EvalError::MalformedOutput(format!(
                "SHM ring buffer '{:?}' returned no numeric key-value pairs in frame {}",
                self.path, seq
            )));
        }

        // Schema validation: ensure required keys exist
        for req in &self.required_keys {
            if !parsed_numbers.contains_key(req) {
                return Err(EvalError::MalformedOutput(format!(
                    "Schema validation failed: SHM ring buffer '{:?}' omitted required numeric field '{}'",
                    self.path, req
                )));
            }
        }

        let timestamp_ms = parse_json_u64(trimmed, "timestamp_ms")
            .or_else(|| parse_json_u64(trimmed, "timestamp"))
            .or((ts_ns > 0).then_some(ts_ns / 1_000_000));
        let ttl_ms = parse_json_u64(trimmed, "ttl_ms")
            .or_else(|| parse_json_float(trimmed, "ttl").map(|s| (s * 1000.0) as u64));
        let warnings = parse_json_string_array(trimmed, "warnings");

        let mut out_values = inputs.values.clone();
        out_values.extend(parsed_numbers);
        for meta_key in FRESHNESS_KEYS {
            out_values.remove(*meta_key);
        }

        Ok(EvalResult {
            values: out_values,
            backend_id: self.backend_id.clone(),
            solve_time_us: elapsed_us,
            timestamp_ms,
            ttl_ms,
            warnings,
            payload_sha256: Some(sha256_hex(payload.as_bytes())),
        })
    }

    fn name(&self) -> &str {
        &self.backend_id
    }
}

#[cfg(windows)]
#[allow(clippy::upper_case_acronyms)] // HMODULE / FARPROC are the Win32 type names
mod os_ffi {
    use std::ffi::c_void;
    pub type HMODULE = *mut c_void;
    pub type FARPROC = *mut c_void;

    unsafe extern "system" {
        pub fn LoadLibraryW(lpLibFileName: *const u16) -> HMODULE;
        pub fn GetProcAddress(hModule: HMODULE, lpProcName: *const u8) -> FARPROC;
        pub fn FreeLibrary(hLibModule: HMODULE) -> i32;
        pub fn GetLastError() -> u32;
    }
}

#[cfg(unix)]
mod os_ffi {
    use std::ffi::{c_char, c_void};
    unsafe extern "C" {
        pub fn dlopen(filename: *const c_char, flags: i32) -> *mut c_void;
        pub fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        pub fn dlclose(handle: *mut c_void) -> i32;
        pub fn dlerror() -> *mut c_char;
    }
    pub const RTLD_NOW: i32 = 2;
}

/// Cross-platform dynamic library handle (`.dll` on Windows, `.so`/`.dylib` on Unix).
pub struct DynamicLibrary {
    #[cfg(windows)]
    handle: os_ffi::HMODULE,
    #[cfg(unix)]
    handle: *mut std::ffi::c_void,
}

unsafe impl Send for DynamicLibrary {}
unsafe impl Sync for DynamicLibrary {}

impl DynamicLibrary {
    pub fn load<P: AsRef<std::path::Path>>(path: P) -> Result<Self, EvalError> {
        let p = path.as_ref();
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let wide: Vec<u16> = p
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let handle = unsafe { os_ffi::LoadLibraryW(wide.as_ptr()) };
            if handle.is_null() {
                let err_code = unsafe { os_ffi::GetLastError() };
                return Err(EvalError::BlackBoxFailure(format!(
                    "Failed to load dynamic library '{:?}': Windows error code {}",
                    p, err_code
                )));
            }
            Ok(Self { handle })
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let mut bytes = p.as_os_str().as_bytes().to_vec();
            bytes.push(0);
            let handle = unsafe { os_ffi::dlopen(bytes.as_ptr() as *const _, os_ffi::RTLD_NOW) };
            if handle.is_null() {
                let err_msg = unsafe {
                    let err_ptr = os_ffi::dlerror();
                    if err_ptr.is_null() {
                        "Unknown dlopen error".to_string()
                    } else {
                        std::ffi::CStr::from_ptr(err_ptr)
                            .to_string_lossy()
                            .into_owned()
                    }
                };
                return Err(EvalError::BlackBoxFailure(format!(
                    "Failed to load dynamic library '{:?}': {}",
                    p, err_msg
                )));
            }
            Ok(Self { handle })
        }
        #[cfg(not(any(windows, unix)))]
        {
            Err(EvalError::BlackBoxFailure(format!(
                "dynamic external libraries are not supported on this target (cannot load '{:?}')",
                p
            )))
        }
    }

    /// Copy the named symbol out of this library as a `T`.
    ///
    /// # Safety
    ///
    /// `symbol_name` must name a symbol whose stored value is a valid `T`.
    /// `T` is the symbol's real type, usually a function pointer, and the
    /// caller must not use the value after this library is dropped. A
    /// mismatched `T` is undefined behavior.
    pub unsafe fn get_symbol<T: Copy>(&self, symbol_name: &str) -> Result<T, EvalError> {
        #[cfg(windows)]
        {
            let mut sym_bytes = symbol_name.as_bytes().to_vec();
            sym_bytes.push(0);
            let proc = unsafe { os_ffi::GetProcAddress(self.handle, sym_bytes.as_ptr()) };
            if proc.is_null() {
                let err_code = unsafe { os_ffi::GetLastError() };
                return Err(EvalError::BlackBoxFailure(format!(
                    "Failed to find symbol '{}' in dynamic library: Windows error code {}",
                    symbol_name, err_code
                )));
            }
            Ok(unsafe { std::mem::transmute_copy(&proc) })
        }
        #[cfg(unix)]
        {
            let mut sym_bytes = symbol_name.as_bytes().to_vec();
            sym_bytes.push(0);
            let proc = unsafe { os_ffi::dlsym(self.handle, sym_bytes.as_ptr() as *const _) };
            if proc.is_null() {
                let err_msg = unsafe {
                    let err_ptr = os_ffi::dlerror();
                    if err_ptr.is_null() {
                        "Symbol not found".to_string()
                    } else {
                        std::ffi::CStr::from_ptr(err_ptr)
                            .to_string_lossy()
                            .into_owned()
                    }
                };
                return Err(EvalError::BlackBoxFailure(format!(
                    "Failed to find symbol '{}' in dynamic library: {}",
                    symbol_name, err_msg
                )));
            }
            Ok(unsafe { std::mem::transmute_copy(&proc) })
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = symbol_name;
            Err(EvalError::BlackBoxFailure(
                "dynamic external libraries are not supported on this target".to_string(),
            ))
        }
    }
}

impl Drop for DynamicLibrary {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            if !self.handle.is_null() {
                unsafe { os_ffi::FreeLibrary(self.handle) };
            }
        }
        #[cfg(unix)]
        {
            if !self.handle.is_null() {
                unsafe { os_ffi::dlclose(self.handle) };
            }
        }
    }
}

/// Standard C ABI function pointer for native in-process evaluators.
/// Returns 0 on success, non-zero on error.
pub type AvenNativePluginFn = unsafe extern "C" fn(
    input_json: *const std::ffi::c_char,
    output_buf: *mut std::ffi::c_char,
    output_buf_len: usize,
) -> i32;

/// In-process Native FFI Evaluator loading dynamic C/Rust libraries (`.so`/`.dll`).
pub struct NativeFfiEvaluator {
    pub lib_path: PathBuf,
    pub symbol: String,
    pub backend_id: String,
    pub required_keys: Vec<String>,
    lib: std::sync::Mutex<Option<Arc<DynamicLibrary>>>,
}

impl NativeFfiEvaluator {
    pub fn new(lib_path: &str, symbol: &str) -> Self {
        let path = PathBuf::from(lib_path);
        let id_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(lib_path)
            .to_string();
        Self {
            backend_id: format!("native_{}_{}", id_name, symbol),
            lib_path: path,
            symbol: symbol.to_string(),
            required_keys: Vec::new(),
            lib: std::sync::Mutex::new(None),
        }
    }

    pub fn with_required_keys(mut self, keys: &[String]) -> Self {
        self.required_keys = keys.to_vec();
        self
    }
}

impl Evaluator for NativeFfiEvaluator {
    fn evaluate(&self, inputs: &VarMap) -> Result<EvalResult, EvalError> {
        let lib = {
            let mut guard = self.lib.lock().map_err(|e| {
                EvalError::BlackBoxFailure(format!("Failed to lock dynamic library mutex: {}", e))
            })?;
            if guard.is_none() {
                *guard = Some(Arc::new(DynamicLibrary::load(&self.lib_path)?));
            }
            Arc::clone(guard.as_ref().unwrap())
        };
        let plugin_fn: AvenNativePluginFn = unsafe { lib.get_symbol(&self.symbol)? };

        // Serialize inputs into null-terminated C string
        let mut json_str = String::from("{");
        let mut first = true;
        let mut sorted_keys: Vec<&String> = inputs.values.keys().collect();
        sorted_keys.sort();
        for k in sorted_keys {
            if !first {
                json_str.push_str(", ");
            }
            use std::fmt::Write as FmtWrite;
            let _ = write!(json_str, "\"{}\": {}", k, inputs.values[k]);
            first = false;
        }
        json_str.push_str("}\0");

        let mut output_buf = vec![0 as std::ffi::c_char; 65536];

        let t_start = std::time::Instant::now();
        let ret_code = unsafe {
            plugin_fn(
                json_str.as_ptr() as *const std::ffi::c_char,
                output_buf.as_mut_ptr(),
                output_buf.len(),
            )
        };
        let elapsed_us = t_start.elapsed().as_secs_f64() * 1_000_000.0;

        let out_c_str = unsafe { std::ffi::CStr::from_ptr(output_buf.as_ptr()) };
        let out_str = out_c_str.to_str().map_err(|e| {
            EvalError::MalformedOutput(format!(
                "Native plugin '{}' in '{:?}' returned invalid UTF-8: {}",
                self.symbol, self.lib_path, e
            ))
        })?;

        let trimmed = out_str.trim();

        if ret_code != 0 {
            return Err(EvalError::BlackBoxFailure(format!(
                "Native FFI plugin '{}' in '{:?}' returned non-zero code {}: {}",
                self.symbol, self.lib_path, ret_code, trimmed
            )));
        }

        if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
            return Err(EvalError::MalformedOutput(format!(
                "Malformed output: Native FFI plugin response must be a JSON object '{{...}}', found: '{}'",
                trimmed
            )));
        }

        let parsed_numbers = parse_json_object_numbers(trimmed);
        if parsed_numbers.is_empty() {
            return Err(EvalError::MalformedOutput(format!(
                "Malformed output: Native FFI plugin '{}' returned no numeric key-value pairs",
                self.symbol
            )));
        }

        for req in &self.required_keys {
            if !parsed_numbers.contains_key(req) {
                return Err(EvalError::MalformedOutput(format!(
                    "Schema validation failed: Native FFI plugin '{}' omitted required numeric field '{}'",
                    self.symbol, req
                )));
            }
        }

        let timestamp_ms = parse_json_u64(trimmed, "timestamp_ms")
            .or_else(|| parse_json_u64(trimmed, "timestamp"));
        let ttl_ms = parse_json_u64(trimmed, "ttl_ms")
            .or_else(|| parse_json_float(trimmed, "ttl").map(|s| (s * 1000.0) as u64));
        let warnings = parse_json_string_array(trimmed, "warnings");

        let mut out_values = inputs.values.clone();
        out_values.extend(parsed_numbers);
        for meta_key in FRESHNESS_KEYS {
            out_values.remove(*meta_key);
        }

        Ok(EvalResult {
            values: out_values,
            backend_id: self.backend_id.clone(),
            solve_time_us: elapsed_us,
            timestamp_ms,
            ttl_ms,
            warnings,
            payload_sha256: Some(sha256_hex(trimmed.as_bytes())),
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
        // A ragged frontier has no single objective axis to measure.
        if frontier.iter().any(|p| p.objectives.len() != n_objs) {
            return Some(frontier.len() / 2);
        }

        let series: Vec<Vec<f64>> = (0..n_objs)
            .map(|j| frontier.iter().map(|p| p.objectives[j].1).collect())
            .collect();
        // A flat or empty trade-off still yields a point, matching earlier callers
        // that only check `is_some`. The balanced sample on a straight front is
        // the midpoint.
        Some(knee_of_series(&series).unwrap_or(frontier.len() / 2))
    }
}

/// Below this normalized chord distance the curve is straight, and the
/// balanced compromise is the midpoint rather than a noisy interior sample.
const KNEE_FLAT: f64 = 1e-6;

/// Index of the balanced compromise across parallel series.
///
/// Each series is one objective sampled at the same parameter values. The pair
/// whose normalized curve bows farthest from the chord between its endpoints
/// supplies the index, so a large-unit straight objective cannot hide a smaller
/// curved trade-off. A straight pair stays at the midpoint. A pair whose
/// endpoints meet is measured by distance from that point, so a spike is not
/// reported as the middle sample. `None` when fewer than two series move or
/// fewer than three samples exist.
pub fn knee_of_series(series: &[Vec<f64>]) -> Option<usize> {
    if series.len() < 2 {
        return None;
    }
    let n = series[0].len();
    if n <= 2 || series.iter().any(|s| s.len() != n) {
        return None;
    }
    let mut best: Option<(usize, f64)> = None;
    for a in 0..series.len() {
        for b in (a + 1)..series.len() {
            let Some((idx, dev)) = chord_knee(&series[a], &series[b]) else {
                continue;
            };
            let replace = match best {
                None => true,
                Some((_, best_dev)) => dev > best_dev,
            };
            if replace {
                best = Some((idx, dev));
            }
        }
    }
    best.map(|(idx, _)| idx)
}

/// Knee index and its normalized deviation for one pair. `None` when either
/// series is non-finite or does not move.
fn chord_knee(a: &[f64], b: &[f64]) -> Option<(usize, f64)> {
    if a.iter().chain(b.iter()).any(|v| !v.is_finite()) {
        return None;
    }
    let extent = |s: &[f64]| {
        let lo = s.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = s.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (lo, hi - lo)
    };
    let (lo_a, span_a) = extent(a);
    let (lo_b, span_b) = extent(b);
    if span_a < 1e-9 || span_b < 1e-9 {
        return None;
    }
    let norm = |v: f64, lo: f64, span: f64| (v - lo) / span;
    let last = a.len() - 1;
    let x1 = norm(a[0], lo_a, span_a);
    let y1 = norm(b[0], lo_b, span_b);
    let x2 = norm(a[last], lo_a, span_a);
    let y2 = norm(b[last], lo_b, span_b);
    let dx = x2 - x1;
    let dy = y2 - y1;
    let denom = (dx * dx + dy * dy).sqrt();
    let mid = a.len() / 2;
    let mut best_i = mid;
    let mut best_d = 0.0;
    for i in 1..last {
        let x0 = norm(a[i], lo_a, span_a);
        let y0 = norm(b[i], lo_b, span_b);
        let dist = if denom < 1e-12 {
            ((x0 - x1).powi(2) + (y0 - y1).powi(2)).sqrt()
        } else {
            (dy * x0 - dx * y0 + x2 * y1 - y2 * x1).abs() / denom
        };
        if dist > KNEE_FLAT && dist > best_d {
            best_d = dist;
            best_i = i;
        }
    }
    Some((best_i, best_d))
}

#[cfg(test)]
mod knee_tests {
    use super::knee_of_series;

    #[test]
    fn straight_tradeoff_knee_is_the_midpoint() {
        let a: Vec<f64> = (0..21).map(|i| i as f64).collect();
        let b: Vec<f64> = a.iter().map(|v| v * 3.0).collect();
        assert_eq!(knee_of_series(&[a, b]), Some(10));
    }

    #[test]
    fn bowed_pair_beats_a_larger_straight_pair() {
        let mut quality = Vec::new();
        let mut cost = Vec::new();
        let mut noise = Vec::new();
        for i in 0..=20 {
            let effort = 1.0 + (i as f64 / 20.0) * 99.0;
            quality.push(effort.sqrt());
            cost.push(effort * 2.0);
            noise.push(effort * 1000.0);
        }
        assert_eq!(knee_of_series(&[quality, cost, noise]), Some(6));
    }

    #[test]
    fn spike_off_the_midpoint_when_endpoints_meet() {
        let a = vec![0.0, 5.0, 0.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0, 0.0, 0.0];
        assert_eq!(knee_of_series(&[a, b]), Some(1));
    }

    #[test]
    fn a_non_finite_series_does_not_erase_the_other_pair() {
        let mut quality = Vec::new();
        let mut cost = Vec::new();
        let ghost = vec![f64::NAN; 21];
        for i in 0..=20 {
            let effort = 1.0 + (i as f64 / 20.0) * 99.0;
            quality.push(effort.sqrt());
            cost.push(effort * 2.0);
        }
        assert_eq!(knee_of_series(&[quality, cost, ghost]), Some(6));
    }
}
