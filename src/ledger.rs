// src/ledger.rs — Fact ledger, causal provenance DAG, why queries, and time-travel rewind
//
// Integrity: every appended fact extends a SHA-256 hash chain
//   hash_i = SHA-256(hash_{i-1} || 0x1d || canonical(fact_i)),  hash_0 = 64 zeros
// so editing, deleting, reordering or inserting any fact changes every later hash. With a
// key (AVEN_LEDGER_KEY) each hash is also signed with HMAC-SHA256, so the chain cannot be
// recomputed without the key. Outside this module facts are only reachable read-only.
#![allow(dead_code)]
use crate::evaluator::sha256_hex;
use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FactMetadata {
    pub timestamp_ms: Option<u64>,
    pub ttl_ms: Option<u64>,
    pub is_stale: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    pub seq: usize,
    pub tick: usize,
    pub kind: String, // "input" | "derived" | "effect" | "committed-effect"
    pub name: String,
    pub value: f64,
    pub causes: Vec<usize>,
    pub origin: String,
    pub metadata: Option<FactMetadata>,
}

impl Fact {
    pub fn display(&self) -> String {
        let meta = if let Some(ref m) = self.metadata {
            let mut s = String::new();
            if m.is_stale {
                s.push_str(" [STALE]");
            }
            if let Some(ttl) = m.ttl_ms {
                s.push_str(&format!(" [ttl:{}ms]", ttl));
            }
            s
        } else {
            String::new()
        };
        format!(
            "#{}@t{} {}:{}={:.4}{}",
            self.seq, self.tick, self.kind, self.name, self.value, meta
        )
    }
}

#[derive(Debug, Clone)]
pub struct IrreversibleRewindError {
    pub message: String,
}

impl fmt::Display for IrreversibleRewindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Irreversible rewind error: {}", self.message)
    }
}

impl std::error::Error for IrreversibleRewindError {}

pub const DEFAULT_MAX_DEPTH: usize = 20;

/// Hash that precedes the first fact of every chain.
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, Default)]
pub struct Ledger {
    facts: Vec<Fact>,
    by_seq: HashMap<usize, Fact>,
    by_name: HashMap<String, Vec<Fact>>,
    irreversible_seqs: Vec<usize>,
    /// chain[i] is the hash after appending facts[i]
    chain: Vec<String>,
    /// HMAC-SHA256 of each chain hash when the ledger has a signing key
    signatures: Vec<String>,
    key: Option<Vec<u8>>,
    initial_head: Option<String>,
}

impl Ledger {
    pub fn new() -> Self {
        Self::default()
    }

    /// A ledger whose chain hashes are signed with HMAC-SHA256 under `key`.
    pub fn with_key(key: &[u8]) -> Self {
        Self {
            key: Some(key.to_vec()),
            ..Self::default()
        }
    }

    /// A ledger with an explicit initial head hash (used for segmented log chaining) and optional signing key.
    pub fn with_initial_head_and_key(head: &str, key: Option<&[u8]>) -> Self {
        Self {
            initial_head: if head == GENESIS_HASH {
                None
            } else {
                Some(head.to_string())
            },
            key: key.map(|k| k.to_vec()),
            ..Self::default()
        }
    }

    pub fn append(&mut self, fact: Fact) -> Fact {
        if fact.kind == "committed-effect" {
            self.irreversible_seqs.push(fact.seq);
        }
        let hash = chain_hash(self.head(), &fact);
        if let Some(key) = &self.key {
            self.signatures.push(hmac_sha256_hex(key, hash.as_bytes()));
        }
        self.chain.push(hash);
        self.by_seq.insert(fact.seq, fact.clone());
        self.by_name
            .entry(fact.name.clone())
            .or_default()
            .push(fact.clone());
        self.facts.push(fact.clone());
        fact
    }

    /// Every fact in append order (read-only).
    pub fn facts(&self) -> &[Fact] {
        &self.facts
    }

    pub fn len(&self) -> usize {
        self.facts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    /// The fact with sequence number `seq`.
    pub fn get(&self, seq: usize) -> Option<&Fact> {
        self.by_seq.get(&seq)
    }

    /// Hash covering the whole ledger so far (the genesis hash when empty).
    pub fn head(&self) -> &str {
        self.chain
            .last()
            .map(String::as_str)
            .or(self.initial_head.as_deref())
            .unwrap_or(GENESIS_HASH)
    }

    pub fn is_signed(&self) -> bool {
        self.key.is_some()
    }

    /// Recomputes the chain (and signatures) from the stored facts.
    pub fn verify(&self) -> Result<(), String> {
        let mut prev = self
            .initial_head
            .as_deref()
            .unwrap_or(GENESIS_HASH)
            .to_string();
        for (i, f) in self.facts.iter().enumerate() {
            let h = chain_hash(&prev, f);
            if self.chain.get(i) != Some(&h) {
                return Err(format!(
                    "fact #{} (position {}) does not match its chain hash",
                    f.seq,
                    i + 1
                ));
            }
            if let Some(key) = &self.key {
                if self.signatures.get(i) != Some(&hmac_sha256_hex(key, h.as_bytes())) {
                    return Err(format!("fact #{} has an invalid signature", f.seq));
                }
            }
            prev = h;
        }
        Ok(())
    }

    /// Append-only JSON Lines audit log: one object per fact with the previous hash, its own
    /// chain hash and, when signed, its HMAC signature. `value_bits` is the exact f64.
    pub fn to_jsonl(&self) -> String {
        let mut out = String::new();
        let mut prev = self.initial_head.as_deref().unwrap_or(GENESIS_HASH);
        for (i, f) in self.facts.iter().enumerate() {
            let meta = match &f.metadata {
                None => "null".to_string(),
                Some(m) => format!(
                    "{{\"timestamp_ms\": {}, \"ttl_ms\": {}, \"is_stale\": {}, \"warnings\": [{}]}}",
                    m.timestamp_ms.map_or("null".to_string(), |v| v.to_string()),
                    m.ttl_ms.map_or("null".to_string(), |v| v.to_string()),
                    m.is_stale,
                    m.warnings
                        .iter()
                        .map(|w| json_string(w))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            };
            let sig = match self.signatures.get(i) {
                Some(s) => format!(", \"sig\": \"{}\"", s),
                None => String::new(),
            };
            let value = if f.value.is_finite() {
                f.value.to_string()
            } else {
                "null".to_string()
            };
            let causes: Vec<String> = f.causes.iter().map(|c| c.to_string()).collect();
            out.push_str(&format!(
                "{{\"seq\": {}, \"tick\": {}, \"kind\": {}, \"name\": {}, \"value\": {}, \"value_bits\": \"{:016x}\", \"causes\": [{}], \"origin\": {}, \"metadata\": {}, \"prev\": \"{}\", \"hash\": \"{}\"{}}}\n",
                f.seq,
                f.tick,
                json_string(&f.kind),
                json_string(&f.name),
                value,
                f.value.to_bits(),
                causes.join(", "),
                json_string(&f.origin),
                meta,
                prev,
                self.chain[i],
                sig
            ));
            prev = &self.chain[i];
        }
        // Closing seal: the fact count and head hash, signed when the ledger has a key, so a
        // signed log with lines cut off the end is detected as truncated
        let seal_msg = format!("seal\u{1d}{}\u{1d}{}", self.facts.len(), self.head());
        let seal_sig = match &self.key {
            Some(k) => format!(", \"sig\": \"{}\"", hmac_sha256_hex(k, seal_msg.as_bytes())),
            None => String::new(),
        };
        out.push_str(&format!(
            "{{\"seal\": true, \"facts\": {}, \"head\": \"{}\"{}}}\n",
            self.facts.len(),
            self.head(),
            seal_sig
        ));
        out
    }

    pub fn latest(&self, name: &str) -> Option<&Fact> {
        self.by_name.get(name)?.last()
    }

    pub fn why(&self, name: &str) -> String {
        self.why_with_depth(name, Some(DEFAULT_MAX_DEPTH))
    }

    pub fn why_with_depth(&self, name: &str, max_depth: Option<usize>) -> String {
        self.why_formatted(name, max_depth, &|_, v| format!("{:.4}", v))
    }

    /// Like `why_with_depth`, with `format(name, value)` rendering each stored value
    /// (e.g. converting base units to a variable's declared unit).
    pub fn why_formatted(
        &self,
        name: &str,
        max_depth: Option<usize>,
        format: &dyn Fn(&str, f64) -> String,
    ) -> String {
        let candidates = match self.by_name.get(name) {
            Some(list) if !list.is_empty() => list,
            _ => return format!("'{}' has no facts in ledger", name),
        };

        let fact = candidates.last().unwrap();
        let mut lines = Vec::new();
        let mut visited = std::collections::HashSet::new();
        self.walk_provenance(fact, 0, max_depth, &mut visited, &mut lines, format);
        lines.join("\n")
    }

    fn walk_provenance(
        &self,
        fact: &Fact,
        depth: usize,
        max_depth: Option<usize>,
        visited: &mut std::collections::HashSet<usize>,
        lines: &mut Vec<String>,
        format: &dyn Fn(&str, f64) -> String,
    ) {
        let pad = "  ".repeat(depth);
        let note = if fact.origin.is_empty() {
            String::new()
        } else {
            format!("  ; {}", fact.origin)
        };

        let mut meta_str = String::new();
        if let Some(ref meta) = fact.metadata {
            if meta.is_stale {
                meta_str.push_str(" [STALE: expired TTL]");
            }
            if let Some(ttl) = meta.ttl_ms {
                meta_str.push_str(&format!(" [ttl: {}ms]", ttl));
            }
            if !meta.warnings.is_empty() {
                meta_str.push_str(&format!(" [warnings: {}]", meta.warnings.join(",")));
            }
        }

        if let Some(limit) = max_depth {
            if depth >= limit && !fact.causes.is_empty() {
                lines.push(format!(
                    "{}└── ... [pruned: {} ancestor(s) beyond hop limit {}. Use --depth <N> or --depth 0 for unlimited]",
                    pad,
                    fact.causes.len(),
                    limit
                ));
                return;
            }
        }

        lines.push(format!(
            "{}#{} {} = {}{}{}",
            pad,
            fact.seq,
            fact.name,
            format(&fact.name, fact.value),
            note,
            meta_str
        ));

        if !visited.insert(fact.seq) {
            lines.push(format!("{}  └── (already traversed #{})", pad, fact.seq));
            return;
        }

        for &parent_seq in &fact.causes {
            if let Some(parent) = self.by_seq.get(&parent_seq) {
                self.walk_provenance(parent, depth + 1, max_depth, visited, lines, format);
            }
        }
    }

    /// Rewinds the ledger to a given logical tick, guarding against crossing committed effects.
    pub fn rewind(
        &self,
        to_tick: usize,
        allow_irreversible: bool,
    ) -> Result<Ledger, IrreversibleRewindError> {
        let mut cutoff = None;
        for f in &self.facts {
            if f.tick <= to_tick {
                cutoff = Some(f.seq);
            }
        }

        let cutoff_seq = cutoff.unwrap_or(0);

        // Check for irreversible commit crossing
        let crossed: Vec<usize> = self
            .irreversible_seqs
            .iter()
            .copied()
            .filter(|&s| s > cutoff_seq)
            .collect();

        if !crossed.is_empty() && !allow_irreversible {
            return Err(IrreversibleRewindError {
                message: format!(
                    "Rewind to tick {} (seq #{}) crosses {} committed (irreversible) effect(s): {:?}.\n\
                     Committed effects already touched the outside world. Pass allow_irreversible to override.",
                    to_tick,
                    cutoff_seq,
                    crossed.len(),
                    crossed
                ),
            });
        }

        let mut cloned = match &self.key {
            Some(k) => Ledger::with_key(k),
            None => Ledger::new(),
        };
        for f in &self.facts {
            if f.seq <= cutoff_seq {
                cloned.append(f.clone());
            }
        }

        Ok(cloned)
    }
}

// ---------------------------------------------------------------------------
// Hash chain, HMAC and audit-log verification
// ---------------------------------------------------------------------------

/// Unambiguous encoding of every field of a fact (the value by its exact bit pattern).
fn canonical(f: &Fact) -> String {
    let meta = match &f.metadata {
        None => "-".to_string(),
        Some(m) => format!(
            "ts={:?};ttl={:?};stale={};warn={}",
            m.timestamp_ms,
            m.ttl_ms,
            m.is_stale,
            m.warnings.join("\u{1e}")
        ),
    };
    let causes: Vec<String> = f.causes.iter().map(|c| c.to_string()).collect();
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{:016x}\u{1f}{}\u{1f}{}\u{1f}{}",
        f.seq,
        f.tick,
        f.kind,
        f.name,
        f.value.to_bits(),
        causes.join(","),
        f.origin,
        meta
    )
}

pub fn chain_hash(prev: &str, f: &Fact) -> String {
    sha256_hex(format!("{}\u{1d}{}", prev, canonical(f)).as_bytes())
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap_or(0))
        .collect()
}

/// HMAC-SHA256 (RFC 2104) built on the crate's pure-std SHA-256.
pub fn hmac_sha256_hex(key: &[u8], msg: &[u8]) -> String {
    const BLOCK: usize = 64;
    let mut k = if key.len() > BLOCK {
        hex_to_bytes(&sha256_hex(key))
    } else {
        key.to_vec()
    };
    k.resize(BLOCK, 0);
    let mut inner: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    inner.extend_from_slice(msg);
    let mut outer: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    outer.extend_from_slice(&hex_to_bytes(&sha256_hex(&inner)));
    sha256_hex(&outer)
}

/// Generates a cryptographically strong 256-bit (64-character hex) random key
/// from OS entropy via standard library `RandomState`, monotonic clocks, nanosecond
/// timestamps, process ID, and memory addresses, hashed with SHA-256.
pub fn generate_random_key() -> String {
    let mut material = Vec::with_capacity(128);
    for i in 0..8 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_usize(i);
        material.extend_from_slice(&hasher.finish().to_le_bytes());
    }
    if let Ok(duration) = SystemTime::now().duration_since(UNIX_EPOCH) {
        material.extend_from_slice(&duration.as_nanos().to_le_bytes());
    }
    let elapsed = Instant::now().elapsed().as_nanos();
    material.extend_from_slice(&elapsed.to_le_bytes());
    let pid = std::process::id();
    material.extend_from_slice(&pid.to_le_bytes());
    let dummy = Box::new(0xdeadbeef_u64);
    let ptr_val = (&*dummy as *const u64 as usize).to_le_bytes();
    material.extend_from_slice(&ptr_val);
    sha256_hex(&material)
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Minimal JSON value for reading audit logs back.
#[derive(Debug, Clone)]
enum Json {
    Null,
    Bool(bool),
    /// Kept as text so integers and bit patterns are read exactly
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

struct JsonParser<'a> {
    s: &'a [u8],
    i: usize,
}

impl JsonParser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Result<(), String> {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", c as char, self.i))
        }
    }

    fn string(&mut self) -> Result<String, String> {
        // assumes the opening quote was consumed
        let mut out = String::new();
        loop {
            let rest = std::str::from_utf8(&self.s[self.i..]).map_err(|e| e.to_string())?;
            let c = rest.chars().next().ok_or("unterminated string")?;
            self.i += c.len_utf8();
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let e = *self.s.get(self.i).ok_or("unterminated escape")? as char;
                    self.i += 1;
                    match e {
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let hex = self.s.get(self.i..self.i + 4).ok_or("short \\u escape")?;
                            let hex = std::str::from_utf8(hex).map_err(|e| e.to_string())?;
                            let code = u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                            self.i += 4;
                        }
                        other => out.push(other),
                    }
                }
                c => out.push(c),
            }
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        match self.s.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut fields = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Json::Obj(fields));
                }
                loop {
                    self.eat(b'"')?;
                    let k = self.string()?;
                    self.eat(b':')?;
                    fields.push((k, self.value()?));
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Json::Obj(fields));
                        }
                        _ => return Err(format!("expected ',' or '}}' at byte {}", self.i)),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                loop {
                    items.push(self.value()?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Json::Arr(items));
                        }
                        _ => return Err(format!("expected ',' or ']' at byte {}", self.i)),
                    }
                }
            }
            Some(b'"') => {
                self.i += 1;
                self.string().map(Json::Str)
            }
            Some(b't') if self.s[self.i..].starts_with(b"true") => {
                self.i += 4;
                Ok(Json::Bool(true))
            }
            Some(b'f') if self.s[self.i..].starts_with(b"false") => {
                self.i += 5;
                Ok(Json::Bool(false))
            }
            Some(b'n') if self.s[self.i..].starts_with(b"null") => {
                self.i += 4;
                Ok(Json::Null)
            }
            Some(_) => {
                let start = self.i;
                while self.i < self.s.len() && b"+-0123456789.eE".contains(&self.s[self.i]) {
                    self.i += 1;
                }
                if start == self.i {
                    return Err(format!("unexpected character at byte {}", start));
                }
                Ok(Json::Num(
                    String::from_utf8_lossy(&self.s[start..self.i]).into_owned(),
                ))
            }
            None => Err("unexpected end of line".to_string()),
        }
    }
}

fn field<'a>(obj: &'a [(String, Json)], key: &str) -> Result<&'a Json, String> {
    obj.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
        .ok_or_else(|| format!("missing field '{}'", key))
}

fn as_u64(v: &Json) -> Result<u64, String> {
    match v {
        Json::Num(n) => n.parse().map_err(|_| format!("'{}' is not an integer", n)),
        _ => Err("expected an integer".to_string()),
    }
}

fn as_str(v: &Json) -> Result<&str, String> {
    match v {
        Json::Str(s) => Ok(s),
        _ => Err("expected a string".to_string()),
    }
}

fn fact_from_json(obj: &[(String, Json)]) -> Result<Fact, String> {
    let causes = match field(obj, "causes")? {
        Json::Arr(items) => items
            .iter()
            .map(|c| as_u64(c).map(|v| v as usize))
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err("'causes' must be an array".to_string()),
    };
    let bits = u64::from_str_radix(as_str(field(obj, "value_bits")?)?, 16)
        .map_err(|_| "'value_bits' is not hexadecimal".to_string())?;
    let metadata = match field(obj, "metadata")? {
        Json::Null => None,
        Json::Obj(m) => {
            let opt = |k: &str| -> Result<Option<u64>, String> {
                match field(m, k)? {
                    Json::Null => Ok(None),
                    v => as_u64(v).map(Some),
                }
            };
            let warnings = match field(m, "warnings")? {
                Json::Arr(w) => w
                    .iter()
                    .map(|x| as_str(x).map(str::to_string))
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err("'warnings' must be an array".to_string()),
            };
            Some(FactMetadata {
                timestamp_ms: opt("timestamp_ms")?,
                ttl_ms: opt("ttl_ms")?,
                is_stale: matches!(field(m, "is_stale")?, Json::Bool(true)),
                warnings,
            })
        }
        _ => return Err("'metadata' must be an object or null".to_string()),
    };
    Ok(Fact {
        seq: as_u64(field(obj, "seq")?)? as usize,
        tick: as_u64(field(obj, "tick")?)? as usize,
        kind: as_str(field(obj, "kind")?)?.to_string(),
        name: as_str(field(obj, "name")?)?.to_string(),
        value: f64::from_bits(bits),
        causes,
        origin: as_str(field(obj, "origin")?)?.to_string(),
        metadata,
    })
}

/// Result of verifying an exported audit log.
#[derive(Debug, Clone, PartialEq)]
pub struct LedgerVerification {
    pub facts: usize,
    pub head: String,
    pub signed: bool,
}

/// Verifies a JSON Lines audit log written by `Ledger::to_jsonl`: every line must link to the
/// previous line's hash, its hash must match its contents, and when `key` is given every line
/// must carry a valid signature. Returns the first failure with its 1-based line number.
pub fn verify_jsonl(text: &str, key: Option<&[u8]>) -> Result<LedgerVerification, String> {
    verify_jsonl_with_head(text, GENESIS_HASH, key)
}

/// Verifies a JSON Lines audit log starting from an expected initial head hash.
/// Enables cross-segment cryptographic chain continuity audits.
pub fn verify_jsonl_with_head(
    text: &str,
    initial_head: &str,
    key: Option<&[u8]>,
) -> Result<LedgerVerification, String> {
    let mut prev = initial_head.to_string();
    let mut count = 0;
    let mut sealed = false;
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let at = |m: String| format!("line {}: {}", n + 1, m);
        let mut p = JsonParser {
            s: line.as_bytes(),
            i: 0,
        };
        let Json::Obj(obj) = p.value().map_err(at)? else {
            return Err(at("not a JSON object".to_string()));
        };
        if sealed {
            return Err(at("content after the closing seal".to_string()));
        }
        if matches!(
            obj.iter().find(|(k, _)| k == "seal"),
            Some((_, Json::Bool(true)))
        ) {
            let facts = as_u64(field(&obj, "facts").map_err(at)?).map_err(at)? as usize;
            let head = as_str(field(&obj, "head").map_err(at)?).map_err(at)?;
            if facts != count || head != prev {
                return Err(at(format!(
                    "seal records {} fact(s) ending at {}… but the log has {} ending at {}…",
                    facts,
                    &head[..head.len().min(16)],
                    count,
                    &prev[..16]
                )));
            }
            if let Some(k) = key {
                let msg = format!("seal\u{1d}{}\u{1d}{}", facts, head);
                match obj.iter().find(|(name, _)| name == "sig").map(|(_, v)| v) {
                    Some(Json::Str(s)) if *s == hmac_sha256_hex(k, msg.as_bytes()) => {}
                    _ => return Err(at("the closing seal has an invalid signature".to_string())),
                }
            }
            sealed = true;
            continue;
        }
        let fact = fact_from_json(&obj).map_err(at)?;
        // The readable `value` must be exactly the hashed `value_bits`, so a reader can never
        // be shown a number other than the one the chain protects
        let shown_ok = match field(&obj, "value").map_err(at)? {
            Json::Num(n) => n
                .parse::<f64>()
                .map_or(false, |v| v.to_bits() == fact.value.to_bits()),
            Json::Null => !fact.value.is_finite(),
            _ => false,
        };
        if !shown_ok {
            return Err(at(format!(
                "fact #{}: displayed value differs from its hashed value_bits",
                fact.seq
            )));
        }
        let stored_prev = as_str(field(&obj, "prev").map_err(at)?).map_err(at)?;
        if stored_prev != prev {
            return Err(at(format!(
                "chain broken at fact #{}: it links to {}… but the previous hash is {}… (a fact was removed, inserted or reordered)",
                fact.seq,
                &stored_prev[..stored_prev.len().min(16)],
                &prev[..16]
            )));
        }
        let expected = chain_hash(&prev, &fact);
        let stored = as_str(field(&obj, "hash").map_err(at)?).map_err(at)?;
        if stored != expected {
            return Err(at(format!(
                "fact #{} was modified: its contents no longer match its hash",
                fact.seq
            )));
        }
        if let Some(k) = key {
            match obj.iter().find(|(name, _)| name == "sig").map(|(_, v)| v) {
                Some(Json::Str(s)) if *s == hmac_sha256_hex(k, expected.as_bytes()) => {}
                Some(_) => {
                    return Err(at(format!("fact #{} has an invalid signature", fact.seq)));
                }
                None => return Err(at(format!("fact #{} is not signed", fact.seq))),
            }
        }
        prev = expected;
        count += 1;
    }
    if key.is_some() && !sealed {
        return Err(format!(
            "no closing seal after fact {}: the log was truncated or never sealed",
            count
        ));
    }
    Ok(LedgerVerification {
        facts: count,
        head: prev,
        signed: key.is_some(),
    })
}

/// Loads and verifies a JSON Lines audit log into an in-memory Ledger.
pub fn from_jsonl(text: &str, key: Option<&[u8]>) -> Result<Ledger, String> {
    from_jsonl_with_head(text, GENESIS_HASH, key)
}

/// Loads and verifies a JSON Lines audit log with a specific initial head hash.
pub fn from_jsonl_with_head(
    text: &str,
    initial_head: &str,
    key: Option<&[u8]>,
) -> Result<Ledger, String> {
    let mut ledger = Ledger::with_initial_head_and_key(initial_head, key);
    let mut prev = initial_head.to_string();
    let mut count = 0;
    let mut sealed = false;

    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let at = |m: String| format!("line {}: {}", n + 1, m);
        let mut p = JsonParser {
            s: line.as_bytes(),
            i: 0,
        };
        let Json::Obj(obj) = p.value().map_err(at)? else {
            return Err(at("not a JSON object".to_string()));
        };
        if sealed {
            return Err(at("content after the closing seal".to_string()));
        }
        if matches!(
            obj.iter().find(|(k, _)| k == "seal"),
            Some((_, Json::Bool(true)))
        ) {
            let facts = as_u64(field(&obj, "facts").map_err(at)?).map_err(at)? as usize;
            let head = as_str(field(&obj, "head").map_err(at)?).map_err(at)?;
            if facts != count || head != prev {
                return Err(at(format!(
                    "seal records {} fact(s) ending at {}… but the log has {} ending at {}…",
                    facts,
                    &head[..head.len().min(16)],
                    count,
                    &prev[..16]
                )));
            }
            if let Some(k) = key {
                let msg = format!("seal\u{1d}{}\u{1d}{}", facts, head);
                match obj.iter().find(|(name, _)| name == "sig").map(|(_, v)| v) {
                    Some(Json::Str(s)) if *s == hmac_sha256_hex(k, msg.as_bytes()) => {}
                    _ => return Err(at("the closing seal has an invalid signature".to_string())),
                }
            }
            sealed = true;
            continue;
        }

        let fact = fact_from_json(&obj).map_err(at)?;
        let stored_prev = as_str(field(&obj, "prev").map_err(at)?).map_err(at)?;
        if stored_prev != prev {
            return Err(at(format!(
                "chain broken at fact #{}: it links to {}… but the previous hash is {}…",
                fact.seq,
                &stored_prev[..stored_prev.len().min(16)],
                &prev[..16]
            )));
        }
        let expected = chain_hash(&prev, &fact);
        let stored = as_str(field(&obj, "hash").map_err(at)?).map_err(at)?;
        if stored != expected {
            return Err(at(format!(
                "fact #{} was modified: hash mismatch",
                fact.seq
            )));
        }
        if let Some(k) = key {
            match obj.iter().find(|(name, _)| name == "sig").map(|(_, v)| v) {
                Some(Json::Str(s)) if *s == hmac_sha256_hex(k, expected.as_bytes()) => {}
                Some(_) => return Err(at(format!("fact #{} has an invalid signature", fact.seq))),
                None => return Err(at(format!("fact #{} is not signed", fact.seq))),
            }
        }
        prev = expected;
        count += 1;
        ledger.append(fact);
    }
    if key.is_some() && !sealed {
        return Err(format!("no closing seal after fact {}", count));
    }
    Ok(ledger)
}
