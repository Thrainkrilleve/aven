# Aven — Product Thesis

*The decision: Aven's product is a desktop IDE for engineering calculations. This document is the spine the build and the pitch are measured against. It is deliberately opinionated; where it says "not now," that is a scope decision, not a judgment that the idea is bad.*

---

## 1. One line

**A desktop app for engineering calculations that check their own units, solve in any direction, and export a report you can defend** — the `avenc` compiler in app form, so you don't need the CLI to use it.

## 2. The problem

Engineers do defensible math in **spreadsheets**. Spreadsheets are free, universal, and shareable — and they will happily add `m/s` to seconds, run a formula in exactly one direction, and leave no trace of how a number was reached. When the number has to be right and has to be *shown* to be right, that's a real gap. The alternatives (a Python/NumPy script, Modelica) trade the gap for a stack to install and maintain, which many shops can't or won't do.

Aven's competitor is **the spreadsheet or the one-off script** — not NOAA, not Modelica. We win only in the places those tools are weak.

## 3. The product — the IDE

The CLI already works; not everyone wants a terminal. The IDE is the CLI in app form: **write your own models, for your own system, for your own work**, with the checks, the solve, and the receipts built in.

Core loop (the first thing anyone sees):
- **Editor** with inline errors from `avenc check` — unit mismatches and degrees-of-freedom problems, flagged as you type.
- **Solve panel** — forward from your inputs, or leave any variable blank and it solves backward for it.
- **`why` tree** — the derivation of any value, back to the inputs.
- **Save / open** real model files.
- **Import / export** — read a spreadsheet's values in (CSV first, then `.xlsx` cell ranges; formulas and live links are out of scope for now); export a model, a results **report (HTML first, PDF next)**, or back to a spreadsheet. *This is make-or-break, not a nice-to-have: spreadsheets win on sharing, so Aven files have to travel too.*
- **Graphs** and (later) **live data** — the things a VS Code linter fundamentally can't do.

Alongside it:
- **A guided learning series** — build your first model, read the results, then break it down. Doubles as onboarding and as marketing.
- **Starter templates** — nobody begins from a blank editor; they begin from a trusted model (pipe sizing, battery runtime, Little's Law capacity).

Everything from the current orchestrator screen that isn't in the core loop goes into an advanced menu or out.

**Why an IDE and not the VS Code extension:** the extension is a linter — syntax and nothing else. It can't solve, can't graph, can't pull live data, can't walk a learner through a model. The IDE is where Aven actually *computes*.

## 4. Why us (lead with the first two)

1. **Units that refuse a bad conversion** — the everyday win. The mistake spreadsheets make silently.
2. **Solve in any direction** — spec the output, recover the input. No spreadsheet does this by default, for any variable, from one model (Excel's Goal Seek and Solver reach one target at a time, each set up by hand).
3. **Show your work** — a shareable report backed by a tamper-evident ledger. Framed as receipts, not cryptography; the hash chain only matters when someone audits, and most days nobody does.
4. **Air-gappable, no stack to install** — no network, no toolchain, nothing to install beyond the app. The `avenc` CLI and the emitted C are a single zero-dependency static binary. *(Windows/WebView2 packaging caveat is in Risks.)*

## 5. Who first (beachhead)

The first users are the ones who **feel the spreadsheet pain, can adopt a tool without a procurement committee, and have a reason to care that the number is defensible**:

- **Small consultancies and test / lab / field engineers** — pay-capable, defensibility matters, no bureaucracy.
- **The home-lab / self-hosting / infra-planning crowd** — self-adopting, shares templates, and *we can dogfood it credibly* (`server_capacity` already lives here).

Students, makers, and hobbyists are the **adoption funnel and evangelists**, not the revenue or the evidence. The regulated buyers (aerospace, medical devices, nuclear, defense) are the **later market** — they need tool qualification and long procurement a solo project can't provide yet, and the first users become the evidence that earns them.

## 6. How we prove it — two pieces

1. **One validated reference example.** Match a trusted authority *exactly* — a NWS derived value (dew point, heat index, density altitude), a NIST constant, or a textbook formula. This earns the word "defensible." It is a line in the proof, **not the pitch** — we are not claiming to out-forecast anyone.
2. **The spreadsheet head-to-head.** The same real calculation in a spreadsheet and in Aven, showing: (a) Aven **catches a planted unit error** the sheet computes straight through, (b) Aven **solves it backward** where the sheet can't, (c) Aven exports a **report** the sheet has no equivalent for. This is "provably better than what you use today," and we control every bit of it — no external feed required.

## 7. Non-goals (for now)

- Out-forecasting NOAA, or winning any live-data domain as a market.
- Safety-critical certification / tool qualification.
- Beating Modelica or Julia on breadth of features.
- Level-3 real-time control (the AERIC RT split, MISRA, WCET) — parked until a named machine and deadline ask for it.
- A package *registry* — resolution isn't implemented; templates ship with the app instead.

## 8. MVP scope & sequencing

Built on the hardened Tauri shell, which is a thin GUI over the one `avenc` (no second solver). Minimal loop first, then depth:

1. **Loop:** editor + inline `avenc check` errors + solve panel (forward & backward) + save/open + **HTML report export** + one excellent template + one guided walkthrough.
2. **Share:** spreadsheet import (CSV, then `.xlsx` ranges) and export, PDF report export, and report verification: a free reader mode in the app plus an `avenc verify-report` command, so anyone who receives a report can re-run the model and check its ledger hash.
3. **See:** graphs of sweeps and sensitivities.
4. **Pull:** one live-data example (the validated reference) as an *optional* template. The core loop never touches the network — Aven is fully usable offline.
5. **Learn:** the full guided series and the template library.

**Done tests for the loop (step 1):**
- The 30-second demo lands: type a relation, leave a variable blank → it solves for it; break a unit → it refuses and points at where.
- A new user can go *open a template → change it to their own numbers → solve backward → export a report* in **under five minutes, without the docs**.

## 9. Honest risks

- **Excel's gravity is sharing, not just habit.** If Aven files don't export/import and travel easily, the guarantees won't matter. Import/export is in the MVP for this reason.
- **The IDE is a real multi-month build.** The cheap validation (ship the WASM core as a Python/Jupyter binding) is the lower-cost path we are *choosing not to take first* — a deliberate bet that the integrated app, with learning and graphs, is the stronger product even though it costs more up front.
- **Distribution & install.** The IDE is a Tauri app; on Windows it needs the WebView2 runtime, which we bundle in the installer because locked-down shops sometimes strip it. The installer needs a home — GitHub Releases and/or the avenc.dev site — and that, plus the CLI's `cargo install --git` and the site's install commands, only works once the GitHub repo is public. Decide the installer channel before the first external user.
- **Lead with the right features.** Units and backward-solve are the daily hook; the ledger is "show your work." Pitching cryptography first loses the room.

## 10. What success looks like

An engineer opens Aven, starts from a template, changes it to their own system, watches it catch a unit slip, leaves a variable blank and gets the answer, and exports a report they hand to a client or a reviewer — and never opens the spreadsheet they would have used. The report embeds the model and its ledger hash, so the recipient can **re-run it and verify the numbers** — via a free reader mode in the app, or an `avenc verify-report` command for anyone who only has the CLI. That re-runnability is what makes it *defensible* rather than just a PDF.
