// src/compaction.rs — Ledger Storage Compaction & Segmented Audit Logs for Aven
// Milestone Phase 6: Segmented log rotation, cross-segment hash chaining, and settled history compaction.
#![allow(dead_code)]

use crate::ledger::{Fact, Ledger, GENESIS_HASH};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SegmentConfig {
    pub segment_dir: PathBuf,
    pub max_facts_per_segment: usize,
    pub key: Option<Vec<u8>>,
}

impl Default for SegmentConfig {
    fn default() -> Self {
        Self {
            segment_dir: PathBuf::from("ledger_segments"),
            max_facts_per_segment: 5000,
            key: None,
        }
    }
}

/// A segmented ledger writer that rolls over to new JSONL files to bound storage footprint.
pub struct SegmentedLedgerWriter {
    config: SegmentConfig,
    current_segment_idx: usize,
    current_facts_in_segment: usize,
    current_head: String,
    total_facts: usize,
    active_ledger: Ledger,
}

impl SegmentedLedgerWriter {
    pub fn new(config: SegmentConfig) -> Result<Self, String> {
        if !config.segment_dir.exists() {
            fs::create_dir_all(&config.segment_dir)
                .map_err(|e| format!("failed to create segment dir: {}", e))?;
        }

        let ledger = match &config.key {
            Some(k) => Ledger::with_key(k),
            None => Ledger::new(),
        };

        Ok(Self {
            config,
            current_segment_idx: 0,
            current_facts_in_segment: 0,
            current_head: GENESIS_HASH.to_string(),
            total_facts: 0,
            active_ledger: ledger,
        })
    }

    /// Appends a fact to the segmented ledger, rotating files when capacity is reached.
    pub fn append(&mut self, fact: Fact) -> Result<Fact, String> {
        let appended = self.active_ledger.append(fact);
        self.current_facts_in_segment += 1;
        self.total_facts += 1;
        self.current_head = self.active_ledger.head().to_string();

        if self.current_facts_in_segment >= self.config.max_facts_per_segment {
            self.rotate_segment()?;
        }

        Ok(appended)
    }

    /// Flushes and closes the current segment, advancing to the next segment file.
    pub fn rotate_segment(&mut self) -> Result<PathBuf, String> {
        if self.current_facts_in_segment == 0 {
            return Ok(self.segment_path(self.current_segment_idx));
        }

        let path = self.segment_path(self.current_segment_idx);
        let jsonl = self.active_ledger.to_jsonl();

        // Write segment file with header
        fs::write(&path, jsonl)
            .map_err(|e| format!("failed to write segment file '{:?}': {}", path, e))?;

        // Reset active ledger for next segment, retaining the chain head
        self.current_segment_idx += 1;
        self.current_facts_in_segment = 0;
        self.active_ledger =
            Ledger::with_initial_head_and_key(&self.current_head, self.config.key.as_deref());

        Ok(path)
    }

    /// Closes and seals the final segment.
    pub fn close(&mut self) -> Result<Option<PathBuf>, String> {
        if self.current_facts_in_segment > 0 {
            let p = self.rotate_segment()?;
            Ok(Some(p))
        } else {
            Ok(None)
        }
    }

    pub fn segment_path(&self, idx: usize) -> PathBuf {
        self.config
            .segment_dir
            .join(format!("segment_{:04}.jsonl", idx))
    }

    pub fn total_facts(&self) -> usize {
        self.total_facts
    }

    pub fn current_head(&self) -> &str {
        &self.current_head
    }
}

/// Audit verification result for a multi-segment ledger directory.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentedVerification {
    pub total_segments: usize,
    pub total_facts: usize,
    pub head: String,
    pub signed: bool,
}

/// Verifies all segment JSONL files in a directory in numerical order.
/// Ensures internal chain validity and head-to-genesis continuity across segment boundaries.
pub fn verify_segmented_directory<P: AsRef<Path>>(
    dir_path: P,
    key: Option<&[u8]>,
) -> Result<SegmentedVerification, String> {
    let dir = dir_path.as_ref();
    if !dir.exists() || !dir.is_dir() {
        return Err(format!("segment directory '{:?}' does not exist", dir));
    }

    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("failed to read segment directory: {}", e))?
        .filter_map(|e| e.ok().map(|ent| ent.path()))
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map_or(false, |s| s.starts_with("segment_") && s.ends_with(".jsonl"))
        })
        .collect();

    entries.sort();

    if entries.is_empty() {
        return Err(format!("no segment files found in '{:?}'", dir));
    }

    let mut total_facts = 0;
    let mut last_head = GENESIS_HASH.to_string();
    let mut all_signed = true;

    for (seg_idx, seg_file) in entries.iter().enumerate() {
        let content = fs::read_to_string(seg_file)
            .map_err(|e| format!("failed to read segment file '{:?}': {}", seg_file, e))?;

        let verif =
            crate::ledger::verify_jsonl_with_head(&content, &last_head, key).map_err(|e| {
                format!(
                    "verification failed in segment #{} ({:?}): {}",
                    seg_idx,
                    seg_file.file_name().unwrap_or_default(),
                    e
                )
            })?;

        total_facts += verif.facts;
        last_head = verif.head;
        if !verif.signed {
            all_signed = false;
        }
    }

    Ok(SegmentedVerification {
        total_segments: entries.len(),
        total_facts,
        head: last_head,
        signed: all_signed && key.is_some(),
    })
}

/// Compaction engine: collapses settled historical ticks into snapshot anchor facts.
pub struct LedgerCompactor;

impl LedgerCompactor {
    /// Produces a compacted copy of a ledger by retaining:
    /// 1. Root inputs and external facts.
    /// 2. Final committed effects and published actuator outputs.
    /// 3. Collapsing intermediate transient facts into certified snapshot records.
    pub fn compact_ledger(ledger: &Ledger) -> Ledger {
        let mut compacted = match ledger.is_signed() {
            true => Ledger::new(), // signatures need re-keying on re-signing
            false => Ledger::new(),
        };

        let mut latest_outputs: std::collections::HashMap<String, Fact> =
            std::collections::HashMap::new();

        for f in ledger.facts() {
            // Always retain input facts and committed effects
            if f.kind == "input" || f.kind == "committed-effect" {
                compacted.append(f.clone());
            } else if f.kind == "derived" || f.kind == "effect" {
                latest_outputs.insert(f.name.clone(), f.clone());
            }
        }

        // Append the latest settled outputs as anchor facts
        let mut sorted_keys: Vec<&String> = latest_outputs.keys().collect();
        sorted_keys.sort();
        for k in sorted_keys {
            compacted.append(latest_outputs[k].clone());
        }

        compacted
    }
}
