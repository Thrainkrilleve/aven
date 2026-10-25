// src/ledger.rs — Fact ledger, causal provenance DAG, why queries, and time-travel rewind
#![allow(dead_code)]
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    pub seq: usize,
    pub tick: usize,
    pub kind: String, // "input" | "derived" | "effect" | "committed-effect"
    pub name: String,
    pub value: f64,
    pub causes: Vec<usize>,
    pub origin: String,
}

impl Fact {
    pub fn display(&self) -> String {
        format!(
            "#{}@t{} {}:{}={:.4}",
            self.seq, self.tick, self.kind, self.name, self.value
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
        let candidates = match self.by_name.get(name) {
            Some(list) if !list.is_empty() => list,
            _ => return format!("'{}' has no facts in ledger", name),
        };

        let fact = candidates.last().unwrap();
        let mut lines = Vec::new();
        self.walk_provenance(fact, 0, &mut lines);
        lines.join("\n")
    }

    fn walk_provenance(&self, fact: &Fact, depth: usize, lines: &mut Vec<String>) {
        let pad = "  ".repeat(depth);
        let note = if fact.origin.is_empty() {
            String::new()
        } else {
            format!("  ; {}", fact.origin)
        };

        lines.push(format!(
            "{}#{} {} = {:.4}{}",
            pad, fact.seq, fact.name, fact.value, note
        ));

        for &parent_seq in &fact.causes {
            if let Some(parent) = self.by_seq.get(&parent_seq) {
                self.walk_provenance(parent, depth + 1, lines);
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
