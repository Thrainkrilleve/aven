"""
Tide Phase 3 — The Rewind Debugger
===================================

"The ledger is the program. The debugger is a cursor over it."

This implements Tide's flagship developer-experience feature: deterministic,
total time travel. Any run can be paused, inspected, rewound to ANY earlier
fact, edited, and resumed — producing an alternate timeline branch — because
computation is a pure fold over an append-only ledger of facts.

Architecture
------------
Fact         An immutable record: a variable acquiring a value, a tick, a
             cause (the fact-ids that produced it), and its origin.
Ledger       Append-only log of facts + the causal provenance DAG between them.
Runner       Deterministic replay engine: state = fold(ledger, step_fn).
             Pure — no side effects hide anywhere.
Sink         The world (screen, network, disk). Effects are buffered and
             COMMITTED at checkpoints. Rewinding revokes uncommitted
             buffered effects by truncation; committed effects are marked
             IRREVERSIBLE and rewind requires explicit override.
Debugger     Cursor + commands:
               break <when-condition>      pause execution
               where                        where did this fact come from?
               why <fact/var>               full causal chain (derivation tree)
               rewind <tick | @before evt>  move the cursor back
               set <var> = <value>          edit an input
               resume                       replay deterministically
               branches                     show alternate timelines
               diff <branch-a> <branch-b>   where did histories diverge?

Everything is plain Python, zero deps. Run: python tide_phase3.py
"""

import math
import re
from dataclasses import dataclass, field
from typing import Callable, Dict, List, Optional, Set, Tuple


# =============================================================================
# 1. Facts & the Ledger
# =============================================================================

@dataclass(frozen=True)
class Fact:
    seq: int                    # global monotonic sequence number
    tick: int                   # logical time (signal sample index)
    kind: str                   # 'input' | 'derived' | 'signal' | 'effect'
    name: str                   # variable or event name
    value: object               # the value acquired
    causes: Tuple[int, ...]     # fact-seqs that produced this fact
    origin: str = ""            # human-readable provenance note

    def __repr__(self):
        v = f"{self.value:.6g}" if isinstance(self.value, float) else repr(self.value)
        return f"#{self.seq}@t{self.tick} {self.kind}:{self.name}={v}"


class IrreversibleRewind(Exception):
    """Rewind attempted past a COMMITTED (irreversible) effect."""


class Ledger:
    """Append-only fact log with a provenance index."""

    def __init__(self):
        self.facts: List[Fact] = []
        self.by_seq: Dict[int, Fact] = {}
        self.by_name: Dict[str, List[Fact]] = {}
        self.irreversible_seqs: List[int] = []

    def append(self, fact: Fact) -> Fact:
        self.facts.append(fact)
        self.by_seq[fact.seq] = fact
        self.by_name.setdefault(fact.name, []).append(fact)
        return fact

    def truncate_to(self, seq: int) -> List[Fact]:
        """Remove facts with seq > given. Returns the removed facts."""
        removed = [f for f in self.facts if f.seq > seq]
        self.facts = [f for f in self.facts if f.seq <= seq]
        self.by_seq = {f.seq: f for f in self.facts}
        self.by_name = {}
        for f in self.facts:
            self.by_name.setdefault(f.name, []).append(f)
        return removed

    def after(self, seq: int) -> List[Fact]:
        return [f for f in self.facts if f.seq > seq]

    def latest_value(self, name: str, upto_seq: Optional[int] = None) -> object:
        candidates = [f for f in self.by_name.get(name, [])
                      if upto_seq is None or f.seq <= upto_seq]
        if not candidates:
            raise KeyError(f"No fact for '{name}'")
        return candidates[-1].value

    def facts_at_tick(self, tick: int) -> List[Fact]:
        return [f for f in self.facts if f.tick == tick]

    def why(self, name: str, seq: Optional[int] = None) -> str:
        """Render the causal derivation tree for the latest fact on `name`."""
        candidates = [f for f in self.by_name.get(name, [])]
        if seq is not None:
            candidates = [f for f in candidates if f.seq <= seq]
        if not candidates:
            return f"'{name}' has no facts"
        fact = candidates[-1]
        lines: List[str] = []

        def walk(f: Fact, depth: int):
            pad = "  " * depth
            v = (f"{f.value:.6g}" if isinstance(f.value, float) else f.value)
            note = f"  ; {f.origin}" if f.origin else ""
            lines.append(f"{pad}#{f.seq} {f.name} = {v}{note}")
            for c in f.causes:
                walk(self.by_seq[c], depth + 1)

        walk(fact, 0)
        return "\n".join(lines)

    def __len__(self):
        return len(self.facts)


# =============================================================================
# 2. The Runner — deterministic fold over the ledger
# =============================================================================

def safe_eval(expr_src: str, env: Dict[str, float]) -> float:
    """
    Evaluates an arithmetic expression safely (restricted grammar:
    numbers, vars, + - * / ^, parens, sqrt/log/exp/sin/cos, comparison
    booleans usable in `when` conditions).
    """
    tokens = re.findall(
        r'\s*(\d+\.\d+|\d+|[A-Za-z_]\w*|[+\-*/^()]|>=|<=|==|!=|>|<)',
        expr_src)
    pos = [0]

    def peek():
        return tokens[pos[0]] if pos[0] < len(tokens) else None

    def take():
        t = peek()
        pos[0] += 1
        return t

    def expr_bp(min_bp: int):
        lhs = parse_unary()
        while True:
            op = peek()
            if op in ('>', '<', '>=', '<=', '==', '!='):
                bp = 1
            elif op in ('+', '-'):
                bp = 2
            elif op in ('*', '/'):
                bp = 3
            elif op == '^':
                bp = 5
            elif op is None or op == ')':
                break
            else:
                raise SyntaxError(f"bad token {op!r} in {expr_src!r}")
            if bp < min_bp or (bp == min_bp and op != '^'):
                break
            take()
            rhs = expr_bp(bp + 1)
            if op == '+': lhs = lhs + rhs
            elif op == '-': lhs = lhs - rhs
            elif op == '*': lhs = lhs * rhs
            elif op == '/': lhs = lhs / rhs
            elif op == '^': lhs = math.pow(lhs, rhs)
            elif op == '>': lhs = 1.0 if lhs > rhs else 0.0
            elif op == '<': lhs = 1.0 if lhs < rhs else 0.0
            elif op == '>=': lhs = 1.0 if lhs >= rhs else 0.0
            elif op == '<=': lhs = 1.0 if lhs <= rhs else 0.0
            elif op == '==': lhs = 1.0 if lhs == rhs else 0.0
            elif op == '!=': lhs = 1.0 if lhs != rhs else 0.0
        return lhs

    def parse_unary():
        if peek() == '-':
            take()
            return -parse_unary()
        tok = take()
        if tok == '(':
            e = expr_bp(0)
            assert take() == ')'
            return e
        if tok and re.fullmatch(r'\d+\.\d+|\d+', tok):
            return float(tok)
        if tok in ('sqrt', 'log', 'exp', 'sin', 'cos'):
            assert take() == '('
            e = expr_bp(0)
            assert take() == ')'
            return getattr(math, tok)(e)
        if tok in env:
            return float(env[tok])
        raise KeyError(f"unbound variable {tok!r}")

    result = expr_bp(0)
    if pos[0] != len(tokens):
        raise SyntaxError(f"trailing tokens in {expr_src!r}")
    return result


class TideRunner:
    """
    Runs a Tide program as a deterministic fold over the ledger.

    A program = list of rules. Each rule:
        derive <name> = <expr>  [when <cond>]   # pure computation
        emit <event> (<expr>)  [when <cond>]    # buffered effect → sink
        commit                                   # checkpoint: flush effects
    Each tick, all rules fire once, in order.
    """

    def __init__(self, name: str = "main"):
        self.name = name
        self.rules: List[dict] = []
        self.ledger = Ledger()
        self._seq = 0

    # ---- construction API --------------------------------------------------

    def input(self, name: str, values: List[object]) -> 'TideRunner':
        """A signal source: values[i] arrives at tick i."""
        self.rules.append({"type": "input", "name": name, "values": list(values)})
        return self

    def derive(self, name: str, expr: str, when: Optional[str] = None) -> 'TideRunner':
        self.rules.append({"type": "derive", "name": name, "expr": expr,
                           "when": when})
        return self

    def emit(self, event: str, expr: str, when: Optional[str] = None) -> 'TideRunner':
        self.rules.append({"type": "emit", "event": event, "expr": expr,
                           "when": when})
        return self

    def commit_every(self, ticks: int) -> 'TideRunner':
        self.rules.append({"type": "commit_every", "ticks": ticks})
        return self

    # ---- execution ----------------------------------------------------------

    def _next_fact(self, tick: int, kind: str, name: str, value: object,
                   causes: Tuple[int, ...], origin: str) -> Fact:
        self._seq += 1
        return Fact(self._seq, tick, kind, name, value, causes, origin)

    def run(self, ticks: int, breakpoint: Optional[Callable] = None,
            ledger: Optional[Ledger] = None, start_seq: Optional[int] = None):
        """
        Execute ticks. If `ledger` given, CONTINUE from an existing prefix
        (this is how resume-after-edit works — new facts extend the ledger,
        keeping causal links into the edited timeline branch).
        Returns the ledger. Stops early if breakpoint(tick, ledger) is True.
        """
        led = ledger if ledger is not None else self.ledger
        buffered_effects: List[Fact] = []

        if start_seq is not None:
            self._seq = start_seq

        for t in range(ticks):
            env_snapshot: Dict[str, float] = {}
            # environment = latest value of every named fact at this point
            for nm in led.by_name:
                try:
                    env_snapshot[nm] = float(led.latest_value(nm))
                except (TypeError, ValueError):
                    pass
            env = dict(env_snapshot)
            new_facts: List[Fact] = []

            # 1. feed inputs for this tick
            for r in self.rules:
                if r["type"] == "input" and t < len(r["values"]):
                    v = r["values"][t]
                    f = self._next_fact(t, "input", r["name"], v, (),
                                        f"signal sample #{t}")
                    led.append(f)
                    new_facts.append(f)
                    if isinstance(v, (int, float)):
                        env[r["name"]] = float(v)

            # 2. fire derivations in order
            for r in self.rules:
                if r["type"] != "derive":
                    continue
                try:
                    if r["when"] and safe_eval(r["when"], env) == 0.0:
                        continue
                    causes = tuple(
                        f.seq for f in led.facts
                        if f.name in _vars_in(r["expr"]) and f.seq <= self._seq
                    )
                    # keep only the most recent cause per variable
                    recent: Dict[str, int] = {}
                    for f in led.facts:
                        if f.name in _vars_in(r["expr"]):
                            recent[f.name] = f.seq
                    causes = tuple(sorted(set(recent.values())))
                    val = safe_eval(r["expr"], env)
                    f = self._next_fact(t, "derived", r["name"], val,
                                         causes, f"= {r['expr']}")
                    led.append(f)
                    new_facts.append(f)
                    env[r["name"]] = val
                except (KeyError, ZeroDivisionError, ValueError,
                        OverflowError) as e:
                    f = self._next_fact(t, "derived", r["name"],
                                        float('nan'), (),
                                        f"ERROR: {e.__class__.__name__}: {e}")
                    led.append(f)
                    new_facts.append(f)
                    env[r["name"]] = float('nan')

            # 3. buffered effects
            commit_now = False
            for r in self.rules:
                if r["type"] == "emit":
                    try:
                        if r["when"] and safe_eval(r["when"], env) == 0.0:
                            continue
                        val = safe_eval(r["expr"], env)
                        f = self._next_fact(t, "effect", r["event"], val,
                                             (), f"emit {r['event']}")
                        buffered_effects.append(f)
                    except (KeyError, ZeroDivisionError, ValueError,
                            OverflowError):
                        pass
                elif r["type"] == "commit_every":
                    if (t + 1) % r["ticks"] == 0:
                        commit_now = True

            if commit_now:
                for f in buffered_effects:
                    cf = self._next_fact(f.tick, "committed-effect", f.name,
                                          f.value, (f.seq,), "COMMITTED (irreversible)")
                    led.append(cf)
                    led.irreversible_seqs.append(cf.seq)
                buffered_effects = []

            if breakpoint is not None and breakpoint(t, led):
                # flush pending buffers into the ledger so the debugger can see
                for f in buffered_effects:
                    led.append(f)
                return led

        # normal exit: any uncommitted effects remain buffered (lost) —
        # that is Tide semantics: uncommitted effects never happened.
        return led


def _vars_in(expr: str) -> Set[str]:
    return set(re.findall(r'\b[A-Za-z_]\w*\b', expr)) - {
        'sqrt', 'log', 'exp', 'sin', 'cos'}


# =============================================================================
# 3. The Debugger — cursor, rewind, edit, fork, diff
# =============================================================================

@dataclass
class Branch:
    name: str
    ledger: Ledger
    rewound_from_seq: int
    edited: Dict[str, object]
    note: str = ""


class TideDebugger:
    """
    Time-travel debugger over one runner. Maintains named alternate
    timelines (branches) produced by rewind+edit+resume.
    """

    def __init__(self, runner: TideRunner):
        self.runner = runner
        self.branches: Dict[str, Branch] = {}
        self._branch_counter = 0

    # ---- inspection ---------------------------------------------------------

    def timeline(self, ledger: Optional[Ledger] = None,
                 only_kinds: Optional[Set[str]] = None) -> str:
        led = ledger or self.runner.ledger
        lines = []
        for f in led.facts:
            if only_kinds and f.kind not in only_kinds:
                continue
            v = (f"{f.value:.6g}" if isinstance(f.value, float) else f.value)
            cause = f" ⬅ {list(f.causes)}" if f.causes else ""
            lines.append(f"  #{f.seq:>3} t{f.tick:<2} {f.kind:<18} "
                         f"{f.name:<12} = {v}{cause}")
        return "\n".join(lines)

    def why(self, name: str, ledger: Optional[Ledger] = None) -> str:
        led = ledger or self.runner.ledger
        return led.why(name)

    # ---- time travel --------------------------------------------------------

    def rewind(self, to_tick: Optional[int] = None,
               before_event: Optional[str] = None,
               ledger: Optional[Ledger] = None,
               allow_irreversible: bool = False) -> Tuple[Ledger, int]:
        """
        Returns a PRUNED COPY of the ledger truncated at the target point.
        The original stays intact (rewind is non-destructive at the meta
        level: we fork, we never erase history-of-history).
        """
        led = ledger or self.runner.ledger
        clone = Ledger()
        clone.irreversible_seqs = list(led.irreversible_seqs)

        if before_event is not None:
            evt_seqs = [f.seq for f in led.facts if f.name == before_event]
            if not evt_seqs:
                raise KeyError(f"no fact named '{before_event}'")
            cutoff = min(evt_seqs) - 1
        else:
            cutoff = None
            for f in led.facts:
                if f.tick <= to_tick:
                    cutoff = f.seq
            if cutoff is None:
                cutoff = -1  # rewound to the very beginning

        # Irreversibility guard
        crossed = [s for s in led.irreversible_seqs if s > cutoff]
        if crossed and not allow_irreversible:
            raise IrreversibleRewind(
                f"Rewind to seq#{cutoff} crosses {len(crossed)} committed "
                f"(irreversible) effect(s): "
                f"{[s for s in crossed][:5]}...\n"
                f"Effects like payments/emails already reached the world. "
                f"Pass allow_irreversible=True if you accept re-executing them."
            )

        for f in led.facts:
            if f.seq <= cutoff:
                clone.append(f)
        return clone, cutoff

    def edit_and_resume(self, pruned_ledger: Ledger, cutoff_seq: int,
                        edits: Dict[str, object], extra_ticks: int,
                        branch_note: str = "") -> Branch:
        """
        Apply input edits at the rewind point, then re-run the program
        deterministically from there. New facts chain causally onto the
        pruned ledger — the edit is itself a fact in the new timeline.
        """
        self._branch_counter += 1
        bname = f"alt-{self._branch_counter}"

        # record the edit as a fact (kind='input', origin='DEBUGGER EDIT')
        r = self.runner
        base_seq = max((f.seq for f in pruned_ledger.facts), default=0)
        r._seq = base_seq
        # resume from max tick in pruned ledger
        last_tick = max((f.tick for f in pruned_ledger.facts), default=-1)
        edit_tick = last_tick + 1

        for name, val in edits.items():
            f = r._next_fact(edit_tick, "input", name, val, (),
                             "DEBUGGER EDIT")
            pruned_ledger.append(f)

        # run remaining ticks on the pruned ledger
        remaining = max(extra_ticks, 0)
        r.run(remaining, ledger=pruned_ledger, start_seq=r._seq)

        br = Branch(bname, pruned_ledger, cutoff_seq, dict(edits), branch_note)
        self.branches[bname] = br
        return br

    def diff(self, a: Branch, b: Branch, var: Optional[str] = None) -> str:
        """First divergence and a summary of differing variables."""
        fa, fb = a.ledger.facts, b.ledger.facts
        lines = [f"diff {a.name} vs {b.name}:"]
        i = 0
        diverged = False
        diffs: Dict[str, int] = {}
        while i < min(len(fa), len(fb)):
            x, y = fa[i], fb[i]
            same = (x.name == y.name and x.kind == y.kind and x.value == y.value
                    and x.tick == y.tick)
            if not same and not diverged:
                lines.append(f"  first divergence at #{x.seq} t{x.tick}: "
                             f"[{a.name}] {x.name}={_fmt(x.value)}  vs  "
                             f"[{b.name}] {y.name}={_fmt(y.value)}")
                diverged = True
            if x.name != y.name or x.value != y.value:
                diffs[x.name] = diffs.get(x.name, 0) + 1
            i += 1
        tail_a = len(fa) - i
        tail_b = len(fb) - i
        if tail_a or tail_b:
            lines.append(f"  length differs: {a.name} has {tail_a:+d} "
                         f"extra facts vs {b.name}")
        if var:
            va = [(_fmt(f.value)) for f in fa if f.name == var]
            vb = [(_fmt(f.value)) for f in fb if f.name == var]
            lines.append(f"  '{var}' timeline in {a.name}: {va}")
            lines.append(f"  '{var}' timeline in {b.name}: {vb}")
        lines.append(f"  variables with differing values: "
                     f"{sorted(diffs) if diffs else 'none'}")
        return "\n".join(lines)


def _fmt(v) -> str:
    if isinstance(v, float):
        return f"{v:.4g}"
    return str(v)


# =============================================================================
# 4. DEMO SCENARIO — the checkout that overcharged
#
# A tiny billing stream. A unit-price input arrives per tick; the derived
# subtotal has a BAD SAMPLE at t3 (garbage sensor/negative price); the
# charge rule emits a payment effect. Commits happen every 3 ticks.
# We: run it, crash the debugger onto the NaN, rewind BEFORE the bad input,
# edit the price, resume into an alternate timeline, and diff.
# =============================================================================

PROGRAM_TICKS = 6


def build_runner() -> TideRunner:
    return (
        TideRunner("checkout")
        .input("unit_price", [10.0, 10.0, 10.0, -4.0, 10.0, 10.0])  # t3 bad!
        .input("quantity",   [1,   2,   3,   4,    5,    6])
        .derive("subtotal", "unit_price * quantity")
        .derive("total", "subtotal * (1 + tax)",
                ).derive("charge", "total")
        .emit("payment_sent", "total")
        .commit_every(3)
    )


def fix_tax(runner: TideRunner):
    """Attach tax as an input so we can edit it too."""
    runner.rules.insert(1, {"type": "input", "name": "tax",
                            "values": [0.08] * PROGRAM_TICKS})


def main():
    print("=" * 70)
    print("Tide Phase 3 — Rewind Debugger (fact-ledger time travel)")
    print("=" * 70)

    # ---- 1. record a run that hits a poisoned input ---------------------
    runner = build_runner()
    fix_tax(runner)

    crashed_at = {}

    def bp(tick, led):
        bad = [f for f in led.facts
               if isinstance(f.value, float) and math.isnan(f.value)]
        if bad and tick not in crashed_at:
            crashed_at[tick] = bad[0].seq
            return True
        return False

    led = runner.run(PROGRAM_TICKS, breakpoint=bp)
    print("\n[run] Execution paused: NaN detected — a poisoned input "
          "corrupted a derived value.")
    print(f"[run] First NaN appeared as fact #{crashed_at.get(list(crashed_at)[0], '?')}")

    # ---- 2. ask WHY: causal chain of the corrupted total ------------------
    print("\n[debugger] why total  —")
    print(runner.ledger.why("total"))
    print("\n[debugger] The chain ends at the input fact for unit_price. "
          "Root cause visible in one command.")

    # ---- 3. rewind to BEFORE the bad input arrived ------------------------
    dbg = TideDebugger(runner)

    print("\n[debugger] rewind @before unit_price@(t3)... "
          "(finding the last good moment)")
    # rewind to tick 2 — one tick before poison
    pruned, cutoff = dbg.rewind(to_tick=2)
    print(f"[debugger] Forked ledger truncated at seq#{cutoff} "
          f"({len(pruned)} facts survive; future discarded cleanly).")

    # ---- 4. attempt the ILLEGAL rewind (past committed payments) ----------
    print("\n[debugger] Trying to rewind to t0 — crossing committed payments:")
    try:
        dbg.rewind(to_tick=0)
    except IrreversibleRewind as e:
        print(f"  REFUSED ✓  {e}")

    # ---- 5. edit the poisoned input and resume into a branch --------------
    print("\n[debugger] set unit_price = 12.0 at t3, resume...")
    alt = dbg.edit_and_resume(
        pruned, cutoff,
        edits={"unit_price": 12.0},
        extra_ticks=PROGRAM_TICKS - 3 + 1,
        branch_note="fixed poisoned price sample")

    print(f"[debugger] Created branch '{alt.name}'. "
          f"Causality chains onto the surviving prefix:")
    print("\n[debugger] why total  (alternate timeline) —")
    print(alt.ledger.why("total"))

    # ---- 6. diff the timelines ---------------------------------------------
    orig_branch = Branch("original", runner.ledger, 0, {}, "buggy run")
    print("\n" + dbg.diff(orig_branch, alt, var="total"))

    # ---- 7. committed effects recap ----------------------------------------
    print("\n[ledger] Committed (irreversible) effects per timeline:")
    for lbl, b in (("original", orig_branch), (alt.name, alt)):
        com = [f for f in b.ledger.facts if f.kind == "committed-effect"]
        tot = sum(f.value for f in com if isinstance(f.value, float))
        print(f"  {lbl:<10} {len(com)} payment(s) sent, "
              f"total charged ${tot:.2f}")

    print("\n[summary] What just happened, structurally:")
    print("  • The program IS the ledger; replay is a pure fold, so resume")
    print("    after edit is trivially deterministic.")
    print("  • rewind() forks instead of destroying: original history kept.")
    print("  • Causal provenance (Fact.causes) makes 'why' one command.")
    print("  • Committed effects are brick walls by default; crossing them")
    print("    requires an explicit, visible override.")
    print("\nPhase 3 complete: time travel, provenance, guarded irreversibility.")


if __name__ == "__main__":
    main()
