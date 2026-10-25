// src/ledger.rs — Fact ledger, causal provenance DAG, why queries, and time-travel rewind
#![allow(dead_code)]
use std::collections::HashMap;
use std::fmt;

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

#[derive(Debug, Clone, Default)]
pub struct Ledger {
    pub facts: Vec<Fact>,
    pub by_seq: HashMap<usize, Fact>,
    pub by_name: HashMap<String, Vec<Fact>>,
    pub irreversible_seqs: Vec<usize>,
}

impl Ledger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, fact: Fact) -> Fact {
        if fact.kind == "committed-effect" {
            self.irreversible_seqs.push(fact.seq);
        }
        self.by_seq.insert(fact.seq, fact.clone());
        self.by_name
            .entry(fact.name.clone())
            .or_default()
            .push(fact.clone());
        self.facts.push(fact.clone());
        fact
    }

    pub fn latest(&self, name: &str) -> Option<&Fact> {
        self.by_name.get(name)?.last()
    }

    pub fn why(&self, name: &str) -> String {
        self.why_with_depth(name, None)
    }

    pub fn why_with_depth(&self, name: &str, max_depth: Option<usize>) -> String {
        let candidates = match self.by_name.get(name) {
            Some(list) if !list.is_empty() => list,
            _ => return format!("'{}' has no facts in ledger", name),
        };

        let fact = candidates.last().unwrap();
        let mut lines = Vec::new();
        let mut visited = std::collections::HashSet::new();
        self.walk_provenance(fact, 0, max_depth, &mut visited, &mut lines);
        lines.join("\n")
    }

    fn walk_provenance(
        &self,
        fact: &Fact,
        depth: usize,
        max_depth: Option<usize>,
        visited: &mut std::collections::HashSet<usize>,
        lines: &mut Vec<String>,
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
                    "{}#{} {} = {:.4}{}{}  ... [pruned: {} ancestor(s) beyond hop limit {}]",
                    pad,
                    fact.seq,
                    fact.name,
                    fact.value,
                    note,
                    meta_str,
                    fact.causes.len(),
                    limit
                ));
                return;
            }
        }

        lines.push(format!(
            "{}#{} {} = {:.4}{}{}",
            pad, fact.seq, fact.name, fact.value, note, meta_str
        ));

        if !visited.insert(fact.seq) {
            lines.push(format!("{}  └── (already traversed #{})", pad, fact.seq));
            return;
        }

        for &parent_seq in &fact.causes {
            if let Some(parent) = self.by_seq.get(&parent_seq) {
                self.walk_provenance(parent, depth + 1, max_depth, visited, lines);
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

        let mut cloned = Ledger::new();
        for f in &self.facts {
            if f.seq <= cutoff_seq {
                cloned.append(f.clone());
            }
        }

        Ok(cloned)
    }
}
