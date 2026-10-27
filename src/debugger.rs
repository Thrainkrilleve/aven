// src/debugger.rs — Interactive Time-Travel Debugger & Counterfactual Branching for Aven
// Milestone Phase 6: Timeline scrubbing, causal provenance DAG rendering, and what-if branch diffing.
#![allow(dead_code)]

use crate::ast::*;
use crate::diff;
use crate::ledger::Ledger;
use std::collections::HashMap;
use std::io::{self, BufRead};

#[derive(Debug, Clone, PartialEq)]
pub struct VarState {
    pub name: String,
    pub value: f64,
    pub kind: String,
    pub origin: String,
    pub tick: usize,
    pub seq: usize,
    pub is_stale: bool,
    pub causes: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TickState {
    pub tick: usize,
    pub vars: HashMap<String, VarState>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VarDiff {
    pub name: String,
    pub old_value: Option<f64>,
    pub new_value: Option<f64>,
    pub delta: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct WhatIfResult {
    pub baseline_tick: usize,
    pub mutated_inputs: HashMap<String, f64>,
    pub baseline_outputs: HashMap<String, f64>,
    pub counterfactual_outputs: HashMap<String, f64>,
    pub diffs: Vec<VarDiff>,
}

/// An interactive time-travel debugging session.
pub struct DebuggerSession {
    pub module_path: String,
    pub module: AvenModule,
    pub ledger: Ledger,
    pub current_tick: usize,
    pub max_tick: usize,
}

impl DebuggerSession {
    pub fn new(module_path: &str, module: AvenModule, ledger: Ledger) -> Self {
        let max_tick = ledger.facts().iter().map(|f| f.tick).max().unwrap_or(0);
        Self {
            module_path: module_path.to_string(),
            module,
            ledger,
            current_tick: max_tick,
            max_tick,
        }
    }

    /// Retrieves all variables in state at the current logical tick cursor.
    pub fn current_state(&self) -> TickState {
        self.state_at_tick(self.current_tick)
    }

    /// Gathers all variable values valid up to a specific tick.
    pub fn state_at_tick(&self, tick: usize) -> TickState {
        let mut vars = HashMap::new();
        for f in self.ledger.facts() {
            if f.tick <= tick {
                let is_stale = f.metadata.as_ref().map_or(false, |m| m.is_stale);
                vars.insert(
                    f.name.clone(),
                    VarState {
                        name: f.name.clone(),
                        value: f.value,
                        kind: f.kind.clone(),
                        origin: f.origin.clone(),
                        tick: f.tick,
                        seq: f.seq,
                        is_stale,
                        causes: f.causes.clone(),
                    },
                );
            }
        }
        TickState { tick, vars }
    }

    /// Moves the cursor to a specific tick, validating bounds.
    pub fn goto_tick(&mut self, tick: usize, allow_irreversible: bool) -> Result<TickState, String> {
        if tick > self.max_tick {
            return Err(format!(
                "target tick {} exceeds maximum timeline tick {}",
                tick, self.max_tick
            ));
        }

        if tick < self.current_tick {
            // Check rewind against irreversible boundary
            self.ledger.rewind(tick, allow_irreversible).map_err(|e| e.message)?;
        }

        self.current_tick = tick;
        Ok(self.current_state())
    }

    /// Advances forward by N ticks.
    pub fn step_forward(&mut self, n: usize) -> Result<TickState, String> {
        let target = (self.current_tick + n).min(self.max_tick);
        self.goto_tick(target, true)
    }

    /// Rewinds backward by N ticks with commit boundary enforcement.
    pub fn step_backward(&mut self, n: usize, allow_irreversible: bool) -> Result<TickState, String> {
        let target = self.current_tick.saturating_sub(n);
        self.goto_tick(target, allow_irreversible)
    }

    /// Causal provenance derivation for a variable at current tick.
    pub fn why(&self, var: &str, depth: Option<usize>) -> String {
        // Build sub-ledger up to current tick
        let sub_ledger = match self.ledger.rewind(self.current_tick, true) {
            Ok(l) => l,
            Err(_) => self.ledger.clone(),
        };
        sub_ledger.why_with_depth(var, depth)
    }

    /// Compares variable states between two historical ticks.
    pub fn diff_ticks(&self, t1: usize, t2: usize) -> Vec<VarDiff> {
        let s1 = self.state_at_tick(t1);
        let s2 = self.state_at_tick(t2);

        let mut all_names: std::collections::BTreeSet<String> = s1.vars.keys().cloned().collect();
        all_names.extend(s2.vars.keys().cloned());

        let mut diffs = Vec::new();
        for name in all_names {
            let v1 = s1.vars.get(&name).map(|s| s.value);
            let v2 = s2.vars.get(&name).map(|s| s.value);

            if v1 != v2 {
                let delta = match (v1, v2) {
                    (Some(a), Some(b)) => Some(b - a),
                    _ => None,
                };
                diffs.push(VarDiff {
                    name,
                    old_value: v1,
                    new_value: v2,
                    delta,
                });
            }
        }

        diffs
    }

    /// Runs a counterfactual what-if branch starting from the current tick.
    /// Mutates specified input variables and re-solves downstream relations.
    pub fn what_if(&self, mutations: &[(String, f64)]) -> Result<WhatIfResult, String> {
        let baseline = self.current_state();
        let target_block = if let Some(fb) = self.module.blocks.iter().find(|b| b.kind == BlockKind::Forward) {
            fb
        } else if let Some(rb) = self.module.blocks.first() {
            rb
        } else {
            return Err("model has no relation or forward block".to_string());
        };

        let mut env: HashMap<String, f64> = baseline
            .vars
            .iter()
            .map(|(k, v)| (k.clone(), v.value))
            .collect();

        // Record mutated inputs
        let mut mutated_inputs = HashMap::new();
        for (k, v) in mutations {
            env.insert(k.clone(), *v);
            mutated_inputs.insert(k.clone(), *v);
        }

        // Topo order
        let order = crate::codegen_c::CTranspiler::topo_sort(target_block)
            .map_err(|e| e.0)?;

        // Collect baseline outputs
        let mut baseline_outputs = HashMap::new();
        for eq in &target_block.equations {
            if let Some(st) = baseline.vars.get(&eq.lhs) {
                baseline_outputs.insert(eq.lhs.clone(), st.value);
            }
        }

        // Re-solve forward relations with mutated values
        let mut counterfactual_outputs = HashMap::new();
        for &idx in &order {
            let eq = &target_block.equations[idx];
            let val = diff::eval_expr(&eq.rhs, &env)
                .map_err(|e| format!("evaluation error in what-if solve: {}", e))?;
            env.insert(eq.lhs.clone(), val);
            counterfactual_outputs.insert(eq.lhs.clone(), val);
        }

        // Calculate diffs
        let mut diffs = Vec::new();
        let mut output_keys: Vec<String> = counterfactual_outputs.keys().cloned().collect();
        output_keys.sort();

        for k in output_keys {
            let old = baseline_outputs.get(&k).copied();
            let new = counterfactual_outputs.get(&k).copied();
            let delta = match (old, new) {
                (Some(o), Some(n)) => Some(n - o),
                _ => None,
            };
            diffs.push(VarDiff {
                name: k,
                old_value: old,
                new_value: new,
                delta,
            });
        }

        Ok(WhatIfResult {
            baseline_tick: self.current_tick,
            mutated_inputs,
            baseline_outputs,
            counterfactual_outputs,
            diffs,
        })
    }

    /// Runs interactive REPL loop reading from stdin and writing to stdout.
    pub fn run_interactive<R: BufRead, W: io::Write>(
        &mut self,
        mut reader: R,
        mut writer: W,
    ) -> io::Result<()> {
        writeln!(
            writer,
            "============================================================"
        )?;
        writeln!(
            writer,
            " Aven Time-Travel Debugger (Phase 6)"
        )?;
        writeln!(
            writer,
            " Model: '{}' | Timeline: 0..{} ticks"
            , self.module_path, self.max_tick
        )?;
        writeln!(
            writer,
            " Type 'help' for commands, 'exit' to quit."
        )?;
        writeln!(
            writer,
            "============================================================"
        )?;

        loop {
            write!(writer, "\naven-dbg [t={}/{}]> ", self.current_tick, self.max_tick)?;
            writer.flush()?;

            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            let cmd = parts[0];

            match cmd {
                "q" | "quit" | "exit" => {
                    writeln!(writer, "Exiting debugger.")?;
                    break;
                }
                "h" | "help" => {
                    self.print_help(&mut writer)?;
                }
                "vars" | "inspect" => {
                    if parts.len() > 1 {
                        let var = parts[1];
                        if let Some(vs) = self.current_state().vars.get(var) {
                            writeln!(writer, "Variable: {}", vs.name)?;
                            writeln!(writer, "  Value:       {}", vs.value)?;
                            writeln!(writer, "  Kind:        {}", vs.kind)?;
                            writeln!(writer, "  Origin:      {}", vs.origin)?;
                            writeln!(writer, "  Tick / Seq:  t{} / #{}", vs.tick, vs.seq)?;
                            writeln!(writer, "  Stale:       {}", vs.is_stale)?;
                            writeln!(writer, "  Causes:      {:?}", vs.causes)?;
                        } else {
                            writeln!(writer, "Variable '{}' not found at tick {}.", var, self.current_tick)?;
                        }
                    } else {
                        let st = self.current_state();
                        let mut keys: Vec<&String> = st.vars.keys().collect();
                        keys.sort();
                        writeln!(writer, "Variables at tick {}:", st.tick)?;
                        for k in keys {
                            let v = &st.vars[k];
                            let stale_tag = if v.is_stale { " [STALE]" } else { "" };
                            writeln!(writer, "  {:<24} = {:>14.4}  ({:<8}){}", v.name, v.value, v.kind, stale_tag)?;
                        }
                    }
                }
                "step" => {
                    let n = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
                    match self.step_forward(n) {
                        Ok(_) => writeln!(writer, "Stepped forward to tick {}.", self.current_tick)?,
                        Err(e) => writeln!(writer, "Error: {}", e)?,
                    }
                }
                "prev" => {
                    let n = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
                    let force = parts.iter().any(|&p| p == "--force" || p == "-f");
                    match self.step_backward(n, force) {
                        Ok(_) => writeln!(writer, "Rewound to tick {}.", self.current_tick)?,
                        Err(e) => writeln!(writer, "Error: {}", e)?,
                    }
                }
                "goto" => {
                    let Some(t_str) = parts.get(1) else {
                        writeln!(writer, "Usage: goto <tick> [--force]")?;
                        continue;
                    };
                    let Ok(t) = t_str.parse::<usize>() else {
                        writeln!(writer, "Invalid tick number '{}'", t_str)?;
                        continue;
                    };
                    let force = parts.iter().any(|&p| p == "--force" || p == "-f");
                    match self.goto_tick(t, force) {
                        Ok(_) => writeln!(writer, "Jumped to tick {}.", self.current_tick)?,
                        Err(e) => writeln!(writer, "Error: {}", e)?,
                    }
                }
                "why" => {
                    let Some(var) = parts.get(1) else {
                        writeln!(writer, "Usage: why <var> [--depth N]")?;
                        continue;
                    };
                    let depth = parts.windows(2).find(|w| w[0] == "--depth").and_then(|w| w[1].parse().ok());
                    let tree = self.why(var, depth);
                    writeln!(writer, "{}", tree)?;
                }
                "diff" => {
                    let (t1, t2) = if parts.len() >= 3 {
                        let t1 = parts[1].parse::<usize>().unwrap_or(0);
                        let t2 = parts[2].parse::<usize>().unwrap_or(self.current_tick);
                        (t1, t2)
                    } else if self.current_tick > 0 {
                        (self.current_tick - 1, self.current_tick)
                    } else {
                        (0, 0)
                    };

                    let diffs = self.diff_ticks(t1, t2);
                    writeln!(writer, "Diff between tick {} and tick {}:", t1, t2)?;
                    if diffs.is_empty() {
                        writeln!(writer, "  No variable changes.")?;
                    } else {
                        for d in diffs {
                            let old_s = d.old_value.map_or("None".to_string(), |v| format!("{:.4}", v));
                            let new_s = d.new_value.map_or("None".to_string(), |v| format!("{:.4}", v));
                            let delta_s = d.delta.map_or(String::new(), |v| format!(" (Δ {:+.4})", v));
                            writeln!(writer, "  {:<24} : {} -> {}{}", d.name, old_s, new_s, delta_s)?;
                        }
                    }
                }
                "what-if" => {
                    let expr = parts[1..].join(" ");
                    if !expr.contains('=') {
                        writeln!(writer, "Usage: what-if <var>=<value>")?;
                        continue;
                    }
                    let split: Vec<&str> = expr.split('=').collect();
                    if split.len() != 2 {
                        writeln!(writer, "Usage: what-if <var>=<value>")?;
                        continue;
                    }
                    let var = split[0].trim();
                    let Ok(val) = split[1].trim().parse::<f64>() else {
                        writeln!(writer, "Invalid number '{}'", split[1])?;
                        continue;
                    };

                    match self.what_if(&[(var.to_string(), val)]) {
                        Ok(res) => {
                            writeln!(writer, "Counterfactual Branch Result (mutating '{}' = {} at t{}):", var, val, res.baseline_tick)?;
                            writeln!(writer, "  {:<24} {:>14} {:>14} {:>14}", "Output Variable", "Nominal", "Branch", "Delta")?;
                            writeln!(writer, "  --------------------------------------------------------------------")?;
                            for d in res.diffs {
                                let old_s = d.old_value.map_or("None".to_string(), |v| format!("{:.4}", v));
                                let new_s = d.new_value.map_or("None".to_string(), |v| format!("{:.4}", v));
                                let delta_s = d.delta.map_or("None".to_string(), |v| format!("{:+.4}", v));
                                writeln!(writer, "  {:<24} {:>14} {:>14} {:>14}", d.name, old_s, new_s, delta_s)?;
                            }
                        }
                        Err(e) => writeln!(writer, "What-If Error: {}", e)?,
                    }
                }
                "seal" => {
                    writeln!(writer, "Ledger Chain Head:   {}", self.ledger.head())?;
                    writeln!(writer, "Total Facts:         {}", self.ledger.len())?;
                    writeln!(writer, "Signed:              {}", self.ledger.is_signed())?;
                    match self.ledger.verify() {
                        Ok(_) => writeln!(writer, "Cryptographic Chain: ✓ VALID")?,
                        Err(e) => writeln!(writer, "Cryptographic Chain: ✗ TAMPERED ({})", e)?,
                    }
                }
                other => {
                    writeln!(writer, "Unknown command '{}'. Type 'help' for available commands.", other)?;
                }
            }
        }

        Ok(())
    }

    fn print_help<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        writeln!(writer, "Available Debugger Commands:")?;
        writeln!(writer, "  step [N]             Advance forward N ticks (default 1)")?;
        writeln!(writer, "  prev [N] [--force]   Rewind backward N ticks (guards committed effects)")?;
        writeln!(writer, "  goto <tick> [--force]Jump to logical tick")?;
        writeln!(writer, "  vars                 List all variables at current tick")?;
        writeln!(writer, "  inspect <var>        Display details and metadata for variable")?;
        writeln!(writer, "  why <var> [--depth N]Show causal provenance derivation tree")?;
        writeln!(writer, "  diff [t1] [t2]       Diff variable values between two ticks")?;
        writeln!(writer, "  what-if <var>=<val>  Counterfactual branch exploration and setpoint diff")?;
        writeln!(writer, "  seal                 Inspect cryptographic hash chain integrity")?;
        writeln!(writer, "  quit / exit          Exit debugger")?;
        Ok(())
    }
}
