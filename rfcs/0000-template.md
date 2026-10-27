# RFC 0000: Feature Name Template

- **Feature Name:** `feature_name`
- **Start Date:** YYYY-MM-DD
- **RFC PR:** [aven/rfcs#0000](https://github.com/Thrainkrilleve/aven/pull/0000)
- **Tracking Issue:** [aven/issues#0000](https://github.com/Thrainkrilleve/aven/issues/0000)
- **Author(s):** Name `<author@avenc.dev>`

---

## 1. Summary

A concise, one-paragraph explanation of the proposed feature, mathematical relation, or compiler capability.

---

## 2. Motivation

Why are we doing this? What use cases does it support? What problems does it solve?
Why cannot existing relations, forward blocks, SMT exports, or solvers express this concept cleanly?

---

## 3. Guide-Level Explanation

Explain the proposal as if it were already implemented in Aven and you were teaching it to a systems engineer or mathematician:
- Introduce new syntax or concepts with concrete examples.
- Show how common engineering workflows are improved.
- Explain edge cases or common mistakes beginners might make.

```aven
// Example code demonstrating the proposed syntax
relation ExampleRelation {
    // ...
}
```

---

## 4. Reference-Level Explanation

The technical, normative specification for implementing this proposal:
- Exact AST changes in `src/ast.rs`, `src/lexer.rs`, and `src/parser.rs`.
- Solver semantics: how symbolic differentiation, Jacobians, or branch-and-bound optimization are affected.
- Dimensional analysis: rules for unit propagation and validation.
- Target codegen impact: C99 (`src/codegen_c.rs`) and WebAssembly Core 1.0 (`src/codegen_wasm.rs`).
- Determinism and ledger invariants: guarantees that replay remains bit-for-bit identical.

---

## 5. Drawbacks & Trade-Offs

Why should we *not* do this?
- What complexity is added to the compiler, grammar, or solver?
- Does it risk numerical instability, non-convergence, or solver divergence?
- What is the cost of teaching this concept?

---

## 6. Rationale & Alternatives

- Why is this design chosen over other designs?
- What other designs were considered and what were their shortcomings?
- What is the impact of not doing this?

---

## 7. Prior Art

Discuss prior art in other languages and constraint systems:
- How does Modelica, Julia/ModelingToolkit, Rust, Z3, or MiniZinc address this problem?
- What lessons can be drawn from their experience?

---

## 8. Unresolved Questions

- What parts of the design remain open or are intentionally deferred to future iterations?
- What edge cases require further empirical data or real-world deployment feedback?
