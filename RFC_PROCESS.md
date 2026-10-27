# The Aven RFC Process

The **Aven Request for Comments (RFC)** process provides a transparent, consensus-driven mechanism for evolving the Aven programming language, its mathematical solver semantics, cryptographic ledger specification, and package ecosystem.

---

## When is an RFC Required?

An RFC is required for any "substantial" change to Aven, including:
- Changes to language grammar, keywords, or syntax.
- Alterations to solver semantics, convergence tolerances, or interval arithmetic.
- Additions to unit algebra, dimensional checking rules, or builtin constants.
- Changes to the ledger schema, HMAC hashing canonicalization, or compaction formats.
- New code emission targets (e.g., WASM, hardware VHDL/Verilog).

Small bug fixes, refactorings, optimizations that preserve exact numerical semantics, and documentation improvements do not require an RFC and can be submitted directly as Pull Requests.

---

## RFC Lifecycle

```text
[ Proposal Draft ] ---> [ Active Discussion ] ---> [ Final Comment Period (FCP) ] ---> [ Approved / Rejected ]
```

1. **Drafting**: Fork the repository, copy `rfcs/0000-template.md` to `rfcs/0000-my-feature.md`, and detail the design, mathematical justification, and migration plan.
2. **Review & Discussion**: Open a Pull Request tagged `RFC: <Title>`. The community and core maintainers evaluate the proposal against Aven's core tenets:
   - *Determinism above all*: Identical inputs + identical ledger = identical output bit-for-bit.
   - *Physical realism*: Dimensions and units cannot be bypassed.
   - *Memory safety & zero runtime overhead*: Features must compile down to $O(1)$ memory.
3. **Final Comment Period (FCP)**: Once consensus is reached, the RFC enters an FCP lasting 14 calendar days.
4. **Approval**: Upon successful FCP completion, the PR is merged into the `rfcs/` directory, and implementation may begin in `avenc`.

---

## RFC Template Structure

Every RFC must contain:
1. **Summary**: One-paragraph explanation.
2. **Motivation**: Why existing relations or solvers cannot express this concept.
3. **Guide-Level Explanation**: How the feature is explained to an Aven engineer.
4. **Reference-Level Explanation**: Exact AST changes, solver convergence constraints, and codegen impact.
5. **Drawbacks & Rationale**: Trade-offs and why this design is superior to alternatives.
6. **Prior Art**: How other systems (Modelica, Julia, Rust, Z3) address this problem.
7. **Unresolved Questions**: Open edge cases or future directions.
