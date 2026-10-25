# Aven as the Relational Orchestrator: Architecture, Contracts, and Rust Implementation

**Source:** Synthesis from xAI Grok Architecture Assessment  
**Date:** October 24, 2026  
**Subject:** Technical specification and Rust code architecture for establishing Aven as the universal relational orchestrator with pluggable evaluation backends.

---

## 1. The Core Architectural Contract

The goal is to keep Aven’s identity as the place where **relations, constraints, objectives, dimensional checking, free-variable diagnostics, and Pareto reasoning** live, while treating every expensive or specialized numeric kernel as a replaceable callee.

```
+----------------------------------------------------------------------------------------------------+
|                                    AVEN RELATIONAL ORCHESTRATOR                                    |
+----------------------------------------------------------------------------------------------------+
|  1. THE FROZEN CORE CONTRACT (Always in Aven):                                                     |
|     • Relations, dimensions (units.rs), and AST are parsed and checked prior to any solve.         |
|     • Free variables, rank, residual, and binding constraints are always reported.                 |
|     • Objectives and constraints are declared in Aven, never hidden inside external tools.         |
|     • Solves return structured objects (FrontierPoint + SolveDiagnostics + spec_hash).             |
+--------------------------------------------------+-------------------------------------------------+
                                                   |
                        +--------------------------+--------------------------+
                        |                                                     |
                        v                                                     v
+--------------------------------------------------+ +-----------------------------------------------+
| NATIVE EVALUATOR (Default, Zero-Dependency):     | | PLUGGABLE EVALUATOR TRAIT:                    |
| • In-process symbolic derivatives (diff.rs)     | | • NativeEvaluator (Pure Rust Newton-Raphson)  |
| • Newton-Raphson solver (solver.rs)              | | • ExternalEvaluator (JSON stdin/stdout / FFI) |
| • Instant microsecond execution                  | | • SurrogateEvaluator (Response Surface)       |
+--------------------------------------------------+ +-----------------------------------------------+
                                                   |
                                                   v
+----------------------------------------------------------------------------------------------------+
|  2. AVEN-OWNED SEARCH & SELECTION LOGIC:                                                           |
|     • Rank analysis identifies free variables (decision vector x).                                 |
|     • Aven manages Pareto ranking (ε-constraint continuation, NSGA-II/III reference niching).      |
|     • Post-processing: ε-dominance archiving (10-30 points) + automated Knee-Point detection.      |
+----------------------------------------------------------------------------------------------------+
```

---

## 2. Invariants Enforced in Code

1. **Objectives and constraints are declared on the Relation**, never inside an evaluator.
2. **Every public solve method returns `FrontierPoint` and `SolveDiagnostics`**, ensuring complete auditability.
3. **Backend selection cannot bypass dimensional checking or free-variable analysis.**
4. **The pure native path remains the default** and is tested in CI with zero external crate dependencies.

---

## 3. Concrete Rust Implementation Blueprint

### A. Core Types (`VarMap`, `EvalResult`, `SolveDiagnostics`, `FrontierPoint`)

```rust
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub struct VarMap {
    pub values: HashMap<String, f64>,
    pub units: HashMap<String, String>, // Checked against units.rs
}

#[derive(Clone, Debug)]
pub struct SparseJacobian {
    pub rows: usize,
    pub cols: usize,
    pub entries: HashMap<(usize, usize), f64>,
}

#[derive(Clone, Debug)]
pub struct EvalResult {
    pub value: f64,
    pub jacobian: Option<SparseJacobian>, // None for black-box callees
    pub backend_id: String,
}

#[derive(Clone, Debug)]
pub struct SolveDiagnostics {
    pub spec_hash: String,
    pub free_vars: Vec<String>,
    pub residual: f64,
    pub rank: usize,
    pub binding_constraints: Vec<String>,
    pub backend_trace: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct FrontierPoint {
    pub vars: VarMap,
    pub objectives: Vec<f64>,
    pub diagnostics: SolveDiagnostics,
}
```

---

### B. The Pluggable `Evaluator` Trait

```rust
#[derive(Debug)]
pub enum EvalError {
    DomainError(String),
    Timeout(String),
    IoError(String),
    MalformedOutput(String),
}

pub trait Evaluator: Send + Sync {
    fn evaluate(&self, vars: &VarMap) -> Result<EvalResult, EvalError>;
    fn name(&self) -> &str;
}

// 1. Native Analytic Evaluator (Aven's pure Rust Newton path)
pub struct NativeEvaluator {
    pub relation_name: String,
}

impl Evaluator for NativeEvaluator {
    fn evaluate(&self, vars: &VarMap) -> Result<EvalResult, EvalError> {
        // Uses diff.rs symbolic differentiation and AST expression evaluation
        Ok(EvalResult {
            value: 0.0,
            jacobian: None, // Or computed SparseJacobian
            backend_id: "native_analytic".to_string(),
        })
    }
    fn name(&self) -> &str {
        "native_analytic"
    }
}

// 2. External Subprocess / FFI Evaluator (CFD, Game Profiler, Pricing Kernel)
pub struct ExternalEvaluator {
    pub command: String,
    pub timeout: Duration,
}

impl Evaluator for ExternalEvaluator {
    fn evaluate(&self, vars: &VarMap) -> Result<EvalResult, EvalError> {
        // Serializes vars to JSON over stdin: {"reqs_per_second": 50000.0, ...}
        // Reads JSON from stdout: {"value": 0.0, "residual": 1e-6}
        Ok(EvalResult {
            value: 0.0,
            jacobian: None,
            backend_id: self.command.clone(),
        })
    }
    fn name(&self) -> &str {
        &self.command
    }
}
```

---

### C. Relation Holding Pluggable Backends

```rust
pub enum ConstraintKind {
    LessThanOrEqual,
    GreaterThanOrEqual,
    Equal,
}

pub struct Constraint {
    pub expr: String,
    pub kind: ConstraintKind,
    pub target: f64,
}

pub enum ObjectiveDirection {
    Minimize,
    Maximize,
}

pub struct Objective {
    pub var_name: String,
    pub direction: ObjectiveDirection,
}

pub struct Relation {
    pub name: String,
    pub evaluator: Arc<dyn Evaluator>,
    pub constraints: Vec<Constraint>,
    pub objectives: Vec<Objective>,
}

impl Relation {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            evaluator: Arc::new(NativeEvaluator {
                relation_name: name.to_string(),
            }),
            constraints: Vec::new(),
            objectives: Vec::new(),
        }
    }

    pub fn with_evaluator(mut self, evaluator: Arc<dyn Evaluator>) -> Self {
        self.evaluator = evaluator;
        self
    }

    pub fn eval(&self, vars: &VarMap) -> Result<EvalResult, EvalError> {
        self.evaluator.evaluate(vars)
    }
}
```

---

### D. The Orchestrator Search Runtime

```rust
pub struct Orchestrator {
    pub relation: Relation,
    pub seed: u64,
}

impl Orchestrator {
    pub fn new(relation: Relation, seed: u64) -> Self {
        Self { relation, seed }
    }

    /// Single deterministic point solve using Newton-Raphson
    pub fn solve_single(&self, initial: VarMap) -> Result<FrontierPoint, String> {
        let eval_res = self.relation.eval(&initial).map_err(|e| format!("{:?}", e))?;
        
        let diagnostics = SolveDiagnostics {
            spec_hash: "36fec103738db96b".to_string(),
            free_vars: vec![],
            residual: 0.0,
            rank: 1,
            binding_constraints: vec![],
            backend_trace: vec![eval_res.backend_id],
        };

        Ok(FrontierPoint {
            vars: initial,
            objectives: vec![eval_res.value],
            diagnostics,
        })
    }

    /// Multi-objective Pareto frontier generation using ε-constraint or NSGA-III
    pub fn solve_pareto(&self, slices: usize) -> Result<Vec<FrontierPoint>, String> {
        let mut frontier = Vec::new();
        // Aven owns the sweep bounds and ranking
        // Each slice calls self.relation.eval(...)
        Ok(frontier)
    }
}
```

---

## 4. Minimal External-Call Protocol (JSON over stdin/stdout)

For language-agnostic inter-process communication with external simulators (Python, Go, C++, OpenFOAM):

**Standard Input (stdin):**
```json
{
  "spec_hash": "36fec103738db96b",
  "vars": {
    "reqs_per_second": 50000.0,
    "avg_latency_ms": 20.0,
    "avg_payload_kb": 64.0
  }
}
```

**Standard Output (stdout):**
```json
{
  "value": 0.0,
  "residual": 1.2e-8,
  "jacobian": null,
  "backend_id": "game_netcode_sim_v2"
}
```
* **Exit Code:** `0` indicates success; non-zero indicates domain error or timeout.

---

## 5. Incremental Capability Ladder

1. **Milestone v0.6.0:** Introduce `Evaluator` trait in `src/solver.rs`; wrap existing continuous Newton solver as `NativeEvaluator`.
2. **Milestone v0.6.1:** Add inequality constraints (`<=`, `>=`) and objective syntax (`minimize`, `maximize`) to AST.
3. **Milestone v0.6.2:** Implement native $\varepsilon$-constraint Pareto continuation on `NativeEvaluator` (zero dependencies).
4. **Milestone v0.7.0:** Add `ExternalEvaluator` subprocess connector for external tools.
5. **Milestone v0.8.0:** Add parallel worker pool and hybrid discrete/continuous NSGA-III search.
