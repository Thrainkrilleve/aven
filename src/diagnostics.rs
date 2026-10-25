// src/diagnostics.rs — Under-determination diagnostics & Jacobian rank analysis
use crate::ast::*;
use crate::diff::{diff, eval_expr};
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone)]
pub struct Diagnosis {
    pub unknowns: Vec<String>,
    pub equations: Vec<String>,
    pub dof: usize,
    pub free_vars: Vec<String>,
    pub orphans: Vec<String>,
    pub minimal_fixes: Vec<String>,
    pub zero_gradient_relations: Vec<String>,
    pub degenerate_equations: Vec<String>,
    pub rank: usize,
    pub unreachable_targets: Vec<String>,
}

impl Diagnosis {
    pub fn report(&self) -> String {
        let width = 68;
        let mut lines = Vec::new();
        lines.push(format!("┌{}┐", "─".repeat(width)));

        let add_row = |lines: &mut Vec<String>, s: &str| {
            lines.push(format!("│ {:<width$} │", s, width = width - 2));
        };

        if !self.unreachable_targets.is_empty() {
            add_row(
                &mut lines,
                "DETERMINED SYSTEM (0 DOF) — solver stalled, here is why:",
            );
            for ut in &self.unreachable_targets {
                add_row(&mut lines, &format!("  {}", ut));
            }
        } else if self.dof == 0 {
            add_row(
                &mut lines,
                "DETERMINED SYSTEM (0 DOF) — solver stalled, here is why:",
            );
            add_row(
                &mut lines,
                "  System has full rank (equations match unknowns).",
            );
            add_row(
                &mut lines,
                "  Stall caused by non-convergence, root non-existence, or flat slopes.",
            );
        } else {
            add_row(
                &mut lines,
                "UNDER-DETERMINED SYSTEM — solver stalled, here is why:",
            );
        }
        add_row(&mut lines, "");
        add_row(
            &mut lines,
            &format!(
                "  unknowns  ({}): {}",
                self.unknowns.len(),
                self.unknowns.join(", ")
            ),
        );
        add_row(
            &mut lines,
            &format!("  relations ({}):", self.equations.len()),
        );
        for eq in &self.equations {
            add_row(&mut lines, &format!("      {}", eq));
        }
        add_row(&mut lines, "");
        add_row(
            &mut lines,
            &format!(
                "  Jacobian rank: {} of {} columns ⟹ {} degree(s) of freedom",
                self.rank,
                self.unknowns.len(),
                self.dof
            ),
        );

        if !self.zero_gradient_relations.is_empty() {
            add_row(&mut lines, "");
            add_row(
                &mut lines,
                "  ZERO-GRADIENT INTRINSIC DETECTED (derivative is 0):",
            );
            for zr in &self.zero_gradient_relations {
                add_row(&mut lines, &format!("      • {}", zr));
            }
            add_row(
                &mut lines,
                "  (piecewise functions like ceil/floor/round have zero slopes)",
            );
        }

        if !self.degenerate_equations.is_empty() {
            add_row(&mut lines, "");
            add_row(
                &mut lines,
                "  DEGENERATE / REDUNDANT EQUATION(S) (linearly dependent):",
            );
            for de in &self.degenerate_equations {
                add_row(&mut lines, &format!("      • {}", de));
            }
            add_row(
                &mut lines,
                "  (these equations add no independent constraint to the system)",
            );
        }

        if !self.free_vars.is_empty() && self.dof > 0 {
            add_row(&mut lines, "  FREE variables (unconstrained directions):");
            add_row(&mut lines, &format!("      {}", self.free_vars.join(", ")));
        }

        if !self.orphans.is_empty() {
            add_row(
                &mut lines,
                "  ORPHAN variables (no relation constrains them):",
            );
            add_row(&mut lines, &format!("      {}", self.orphans.join(", ")));
        }

        if !self.minimal_fixes.is_empty() && self.dof > 0 {
            add_row(&mut lines, "");
            add_row(
                &mut lines,
                "  MINIMAL FIX — any ONE of these bindings resolves it:",
            );
            for fix in &self.minimal_fixes {
                add_row(&mut lines, &format!("      • {}", fix));
            }
        } else if self.dof > 0 {
            add_row(
                &mut lines,
                "  (no single binding suffices — combine bindings or add a relation)",
            );
        }

        lines.push(format!("└{}┘", "─".repeat(width)));
        lines.join("\n")
    }
}

pub struct Diagnostician;

impl Default for Diagnostician {
    fn default() -> Self {
        Self::new()
    }
}

impl Diagnostician {
    pub fn new() -> Self {
        Self
    }

    fn probe_val(i: usize) -> f64 {
        1.0 + 0.37 * (i as f64) + 0.11 * ((i % 3) as f64)
    }

    pub fn diagnose_module(&self, module: &AvenModule) -> Vec<Diagnosis> {
        let mut diagnoses = Vec::new();
        for blk in &module.blocks {
            if blk.kind == BlockKind::Relation {
                let mut known = BTreeSet::new();
                if let Some(solve) = module
                    .solves
                    .iter()
                    .find(|s| s.target == blk.name || s.targets.contains(&blk.name))
                {
                    for b in &solve.bindings {
                        known.insert(b.name.clone());
                    }
                }
                let mut all_vars = BTreeSet::new();
                for eq in &blk.equations {
                    all_vars.insert(eq.lhs.clone());
                    for v in eq.rhs.vars() {
                        all_vars.insert(v);
                    }
                }
                let unknowns: Vec<String> = all_vars
                    .into_iter()
                    .filter(|v| !known.contains(v))
                    .collect();

                if !unknowns.is_empty() {
                    let mut probe_env = HashMap::new();
                    if let Some(solve) = module
                        .solves
                        .iter()
                        .find(|s| s.target == blk.name || s.targets.contains(&blk.name))
                    {
                        for b in &solve.bindings {
                            probe_env.insert(b.name.clone(), b.value);
                        }
                    }
                    for (i, u) in unknowns.iter().enumerate() {
                        probe_env.insert(u.clone(), Self::probe_val(i));
                    }
                    let diag = self.diagnose(&unknowns, &blk.equations, &probe_env);
                    diagnoses.push(diag);
                }
            }
        }
        diagnoses
    }

    pub fn diagnose(
        &self,
        unknowns: &[String],
        relations: &[Equation],
        env: &HashMap<String, f64>,
    ) -> Diagnosis {
        let n = unknowns.len();
        let m = relations.len();

        let mut probe_env = env.clone();
        for (i, u) in unknowns.iter().enumerate() {
            probe_env.insert(u.clone(), Self::probe_val(i));
        }

        let mut jac = vec![vec![0.0; n]; m];

        for (i, rel) in relations.iter().enumerate() {
            for (j, u) in unknowns.iter().enumerate() {
                let mut val = 0.0;
                if let Ok(d_rhs) = diff(&rel.rhs, u) {
                    if let Ok(eval_val) = eval_expr(&d_rhs, &probe_env) {
                        if eval_val.is_finite() {
                            val = eval_val;
                        }
                    }
                }
                // Subtract d(lhs)/du
                if u == &rel.lhs {
                    val -= 1.0;
                }
                jac[i][j] = val;
            }
        }

        let (rank, pivots, degenerate_row_indices) = rank_and_pivots(&jac);
        let free_cols: Vec<usize> = (0..n).filter(|c| !pivots.contains(c)).collect();
        let free_vars: Vec<String> = free_cols.into_iter().map(|c| unknowns[c].clone()).collect();

        // Check for unreachable discrete targets (ceil, floor, round with non-integer targets)
        let mut unreachable_targets = Vec::new();
        let mut unreachable_rel_texts = BTreeSet::new();
        let mut unreachable_unknowns = BTreeSet::new();

        for rel in relations {
            if let Some(&target) = env.get(&rel.lhs) {
                if let Expr::Call(ref fn_name, _) = rel.rhs {
                    if matches!(fn_name.as_str(), "ceil" | "floor" | "round") {
                        if target.fract().abs() > 1e-6 {
                            unreachable_targets.push(format!(
                                "{} only returns whole numbers, {} is not reachable",
                                fn_name, target
                            ));
                            unreachable_rel_texts.insert(rel.original_text.clone());
                            for u in rel.rhs.vars() {
                                unreachable_unknowns.insert(u);
                            }
                        }
                    }
                }
            }
        }

        // Identify zero-gradient relations (e.g. ceil, floor, round)
        let mut zero_gradient_relations = Vec::new();
        for (i, rel) in relations.iter().enumerate() {
            let row_norm: f64 = jac[i].iter().map(|v| v.abs()).fold(0.0, f64::max);
            if row_norm <= 1e-12 {
                zero_gradient_relations.push(rel.original_text.clone());
            }
        }

        // Identify degenerate / redundant equations (excluding discrete relations with unreachable targets)
        let degenerate_equations: Vec<String> = degenerate_row_indices
            .into_iter()
            .filter(|&idx| idx < relations.len())
            .map(|idx| relations[idx].original_text.clone())
            .filter(|eq| !unreachable_rel_texts.contains(eq))
            .collect();

        // Check for orphan variables
        let mut mentioned = BTreeSet::new();
        for rel in relations {
            let mut v = rel.rhs.vars();
            v.insert(rel.lhs.clone());
            mentioned.extend(v);
        }
        let orphans: Vec<String> = unknowns
            .iter()
            .filter(|u| !mentioned.contains(*u))
            .cloned()
            .collect();

        // Minimal fixes: find if removing unknown v leaves sub-matrix with rank n - 1
        let mut minimal_fixes = Vec::new();
        if n > 1 {
            for (v_idx, v) in unknowns.iter().enumerate() {
                let keep_cols: Vec<usize> = (0..n).filter(|&c| c != v_idx).collect();
                let sub_jac: Vec<Vec<f64>> = jac
                    .iter()
                    .map(|row| keep_cols.iter().map(|&c| row[c]).collect())
                    .collect();

                let (sub_rank, _, _) = rank_and_pivots(&sub_jac);
                if sub_rank == n - 1 {
                    let others: Vec<&String> = unknowns.iter().filter(|u| *u != v).collect();
                    let others_str: Vec<String> = others.into_iter().cloned().collect();
                    minimal_fixes.push(format!(
                        "bind '{}' — reduces system to {} independent unknown(s) [{}] (0 DOF)",
                        v,
                        n - 1,
                        others_str.join(", ")
                    ));
                }
            }
        }

        let dof = if !unreachable_targets.is_empty() {
            0
        } else {
            n.saturating_sub(rank)
        };

        let free_vars = if !unreachable_targets.is_empty() {
            free_vars
                .into_iter()
                .filter(|v| !unreachable_unknowns.contains(v))
                .collect()
        } else {
            free_vars
        };

        Diagnosis {
            unknowns: unknowns.to_vec(),
            equations: relations.iter().map(|r| r.original_text.clone()).collect(),
            dof,
            free_vars,
            orphans,
            minimal_fixes,
            zero_gradient_relations,
            degenerate_equations,
            rank,
            unreachable_targets,
        }
    }
}

/// Gaussian elimination on matrix copy to compute rank, pivot columns, and degenerate rows
fn rank_and_pivots(mat: &[Vec<f64>]) -> (usize, Vec<usize>, Vec<usize>) {
    let rows = mat.len();
    if rows == 0 {
        return (0, Vec::new(), Vec::new());
    }
    let cols = mat[0].len();
    let mut a: Vec<Vec<f64>> = mat.to_vec();
    let mut row_indices: Vec<usize> = (0..rows).collect();

    // Dynamically scale tolerance to matrix magnitude to handle both micro (1e-9) and macro (1e9) scales
    let max_entry: f64 = mat
        .iter()
        .flat_map(|row| row.iter())
        .map(|v| v.abs())
        .fold(0.0f64, f64::max);
    let pivot_tol = 1e-12 * max_entry.max(1e-15);

    let mut pivots = Vec::new();
    let mut r = 0;

    for c in 0..cols {
        if r >= rows {
            break;
        }

        // Find pivot
        let mut max_row = r;
        let mut max_val = a[r][c].abs();
        for i in (r + 1)..rows {
            let val = a[i][c].abs();
            if val > max_val {
                max_val = val;
                max_row = i;
            }
        }

        if max_val <= pivot_tol {
            continue;
        }

        a.swap(r, max_row);
        row_indices.swap(r, max_row);

        for i in 0..rows {
            if i != r && a[i][c].abs() > pivot_tol {
                let factor = a[i][c] / a[r][c];
                for j in c..cols {
                    let sub = factor * a[r][j];
                    a[i][j] -= sub;
                }
            }
        }

        pivots.push(c);
        r += 1;
    }

    let degenerate_rows: Vec<usize> = (r..rows).map(|k| row_indices[k]).collect();
    (r, pivots, degenerate_rows)
}
