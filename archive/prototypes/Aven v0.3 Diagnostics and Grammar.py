"""
AVEN v0.3 — Diagnostics (Phase 4.3) + Real .aven Grammar
==========================================================

Two milestones in one file:

PART A — Under-determination diagnostics (Phase 4.3)
  When the solver stalls, it now EXPLAINS itself:
    • counts equations vs unknowns and computes the true degrees of
      freedom via a rank analysis of the system Jacobian
    • identifies FREE variables (nullspace directions) — which unknowns
      are unconstrained, and why
    • flags ORPHAN variables (mentioned by no relation at all)
    • proposes MINIMAL FIXES: the smallest set of single bindings, any
      one of which pins the system (verified by re-ranking the reduced
      Jacobian, not guessed)

  Method: build residuals F_i(x) for every blocked relation, derive
  ∂F_i/∂x_j SYMBOLICALLY (falling back to central differences), evaluate
  at a deterministic probe point, and run Gaussian elimination to get
  rank + pivot columns. Non-pivot columns ⇒ free directions.

PART B — The real .aven grammar (Phase 4.1)
  A proper lexer + recursive-descent parser for actual .aven source:

      // fleet.aven
      unit Length: m, km
      relation Revenue {
          revenue = price * units
          profit  = revenue - cost
      }
      forward Trip {
          distance = speed * time
          fuel     = distance * consumption
      }
      signal room_temp: Celsius
      solve Trip with {
          speed = 90 km/h
          time  = 4 h
          consumption = 0.07
      }

  Features: line/column-tracked tokens, comments, unit literals on
  bindings (`90 km/h`), signal declarations, unit registries, `forward`
  direction-pin enforcement at solve time with targeted errors.

Run: python aven_v03.py      (zero dependencies, Python 3.8+)
"""

import math
import re
from dataclasses import dataclass, field
from typing import Callable, Dict, List, Optional, Set, Tuple

VERSION = "0.3.0"
LANG_NAME = "Aven"


# =============================================================================
# CORE — expressions, differentiation (carried from v0.2, condensed)
# =============================================================================

@dataclass(frozen=True)
class Num:
    value: float

@dataclass(frozen=True)
class Var:
    name: str

@dataclass(frozen=True)
class Bin:
    op: str
    left: object
    right: object

@dataclass(frozen=True)
class Call:
    fn: str
    arg: object


FUNCS: Dict[str, Callable] = {
    'sqrt': math.sqrt, 'log': math.log, 'exp': math.exp,
    'sin': math.sin, 'cos': math.cos,
}


def eval_expr(e, env):
    if isinstance(e, Num):
        return e.value
    if isinstance(e, Var):
        if e.name not in env:
            raise KeyError(e.name)
        return env[e.name]
    if isinstance(e, Call):
        return FUNCS[e.fn](eval_expr(e.arg, env))
    l, r = eval_expr(e.left, env), eval_expr(e.right, env)
    if e.op == '+': return l + r
    if e.op == '-': return l - r
    if e.op == '*': return l * r
    if e.op == '/': return l / r if r != 0 else float('inf')
    if e.op == '^': return math.pow(l, r)
    raise ValueError(f"unknown op {e.op}")


def collect_vars(e, acc=None):
    if acc is None:
        acc = set()
    if isinstance(e, Var):
        acc.add(e.name)
    elif isinstance(e, Bin):
        collect_vars(e.left, acc); collect_vars(e.right, acc)
    elif isinstance(e, Call):
        collect_vars(e.arg, acc)
    return acc


class UnsupportedDerivative(Exception):
    pass


def simplify(e):
    if isinstance(e, Bin):
        l, r = simplify(e.left), simplify(e.right)
        if isinstance(l, Num) and isinstance(r, Num):
            try:
                v = {'+': l.value + r.value, '-': l.value - r.value,
                     '*': l.value * r.value,
                     '/': None if r.value == 0 else l.value / r.value,
                     '^': None if (l.value == 0 and r.value <= 0)
                          else math.pow(l.value, r.value)}[e.op]
                if v is not None and math.isfinite(v):
                    return Num(v)
            except (OverflowError, ValueError):
                pass
        if e.op == '*':
            if (isinstance(l, Num) and l.value == 0) or \
               (isinstance(r, Num) and r.value == 0):
                return Num(0)
            if isinstance(l, Num) and l.value == 1: return r
            if isinstance(r, Num) and r.value == 1: return l
        elif e.op == '+':
            if isinstance(l, Num) and l.value == 0: return r
            if isinstance(r, Num) and r.value == 0: return l
        elif e.op == '^':
            if isinstance(r, Num) and r.value == 1: return l
            if isinstance(r, Num) and r.value == 0: return Num(1)
        return Bin(e.op, l, r)
    if isinstance(e, Call):
        a = simplify(e.arg)
        if isinstance(a, Num):
            try:
                v = FUNCS[e.fn](a.value)
                if math.isfinite(v):
                    return Num(v)
            except (ValueError, OverflowError):
                pass
        return Call(e.fn, a)
    return e


def diff(e, x):
    if isinstance(e, Num):
        return Num(0)
    if isinstance(e, Var):
        return Num(1 if e.name == x else 0)
    if isinstance(e, Call):
        u, du = e.arg, diff(e.arg, x)
        table = {
            'sqrt': Bin('/', du, Bin('*', Num(2), Call('sqrt', u))),
            'log': Bin('/', du, u),
            'exp': Bin('*', Call('exp', u), du),
            'sin': Bin('*', Call('cos', u), du),
            'cos': Bin('-', Num(0), Bin('*', Call('sin', u), du)),
        }
        return simplify(table[e.fn])
    u, v = e.left, e.right
    du, dv = diff(u, x), diff(v, x)
    if e.op == '+': return simplify(Bin('+', du, dv))
    if e.op == '-': return simplify(Bin('-', du, dv))
    if e.op == '*': return simplify(Bin('+', Bin('*', du, v), Bin('*', u, dv)))
    if e.op == '/': return simplify(Bin('/', Bin('-', Bin('*', du, v),
                                  Bin('*', u, dv)), Bin('^', v, Num(2))))
    if e.op == '^':
        if isinstance(dv, Num) and dv.value == 0:
            return simplify(Bin('*', Bin('*', v,
                    Bin('^', u, Bin('-', v, Num(1)))), du))
        raise UnsupportedDerivative("u^v")
    raise UnsupportedDerivative(f"node {e!r}")


# =============================================================================
# PART A — UNDER-DETERMINATION DIAGNOSTICS (Phase 4.3)
# =============================================================================

@dataclass
class Relation:
    name: str
    equation: str
    lhs: str
    rhs: object

    @property
    def variables(self) -> Set[str]:
        return collect_vars(self.rhs) | {self.lhs}


def _rank_and_pivots(mat: List[List[float]], tol: float = 1e-9):
    """Gaussian elimination on a copy; returns (rank, pivot_columns)."""
    rows = len(mat)
    cols = len(mat[0]) if rows else 0
    A = [row[:] for row in mat]
    pivots: List[int] = []
    r = 0
    for c in range(cols):
        if r >= rows:
            break
        pr = max(range(r, rows), key=lambda i: abs(A[i][c]))
        if abs(A[pr][c]) <= tol:
            continue
        A[r], A[pr] = A[pr], A[r]
        for i in range(rows):
            if i != r and A[i][c] != 0:
                f = A[i][c] / A[r][c]
                for j in range(c, cols):
                    A[i][j] -= f * A[r][j]
        pivots.append(c)
        r += 1
    return r, pivots


@dataclass
class Diagnosis:
    unknowns: List[str]
    equations: List[str]
    dof: int                       # degrees of freedom
    free_vars: List[str]           # non-pivot directions (unconstrained)
    orphans: List[str]             # unknowns no relation mentions
    minimal_fixes: List[str]       # any ONE binding here pins the system
    rank: int

    def report(self) -> str:
        W = 62
        lines = ["┌" + "─" * W + "┐"]
        def row(s):
            lines.append("│ " + s.ljust(W - 2) + " │")
        row("UNDER-DETERMINED SYSTEM — the solver stalled, here is why:")
        row("")
        row(f"  unknowns  ({len(self.unknowns)}): "
            f"{', '.join(self.unknowns)}")
        row(f"  relations ({len(self.equations)}):")
        for e in self.equations:
            row(f"      {e}")
        row("")
        row(f"  Jacobian rank: {self.rank} of {len(self.unknowns)} columns "
            f"⟹ {self.dof} degree(s) of freedom")
        if self.free_vars:
            row(f"  FREE variables (can slide without violating anything):")
            row(f"      {', '.join(self.free_vars)}")
        if self.orphans:
            row(f"  ORPHAN variables (no relation constrains them at all):")
            row(f"      {', '.join(self.orphans)}")
        if self.minimal_fixes:
            row("")
            row("  MINIMAL FIX — any ONE of these bindings resolves it:")
            for f in self.minimal_fixes:
                row(f"      • {f}")
        elif self.dof > 0:
            row("  (no single binding suffices — combine bindings or add")
            row("   a relation to constrain more directions)")
        lines.append("└" + "─" * W + "┘")
        return "\n".join(lines)


class Diagnostician:
    """
    Analyzes a stalled solve: which unknowns exist, how independent the
    equations really are, which directions are unconstrained, and which
    minimal single bindings would pin the system.
    """

    # deterministic probe values, varied to avoid accidental symmetry
    @staticmethod
    def probe(i: int) -> float:
        return 1.0 + 0.37 * i + 0.11 * (i % 3)

    def diagnose(self, unknowns: List[str],
                 rels: List[Relation],
                 env: Dict[str, float]) -> Diagnosis:
        n = len(unknowns)
        idx = {u: i for i, u in enumerate(unknowns)}

        probe_env = dict(env)
        for u in unknowns:
            probe_env[u] = self.probe(idx[u])

        def residual(rel: Relation, x_env: Dict[str, float]) -> float:
            rv = eval_expr(rel.rhs, x_env)
            if rel.lhs in x_env:
                return rv - x_env[rel.lhs]
            return rv

        # symbolic Jacobian where possible, numeric fallback otherwise
        J = [[0.0] * n for _ in rels]
        for i, rel in enumerate(rels):
            for u in unknowns:
                try:
                    d = diff(rel.rhs, u)
                    val = eval_expr(d, probe_env)
                    if not math.isfinite(val):
                        raise ValueError
                except (UnsupportedDerivative, KeyError, ValueError,
                        ZeroDivisionError, OverflowError):
                    # central-difference fallback
                    h = 1e-6
                    ep, em = dict(probe_env), dict(probe_env)
                    ep[u] += h
                    em[u] -= h
                    try:
                        val = (residual(rel, ep) - residual(rel, em)) / (2 * h)
                    except Exception:
                        val = 0.0
                J[i][idx[u]] = val
            # subtract the LHS indicator: d(lhs)/d(lhs) = 1
            if rel.lhs in idx:
                J[i][idx[rel.lhs]] -= 1.0

        rank, pivots = _rank_and_pivots(J)
        free_cols = [c for c in range(n) if c not in pivots]
        free_vars = [unknowns[c] for c in free_cols]

        # orphans: unknowns that appear in no relation's variables
        mentioned: Set[str] = set()
        for rel in rels:
            mentioned |= rel.variables
        orphans = [u for u in unknowns if u not in mentioned]

        # minimal fixes: binding unknown v works iff the Jacobian restricted
        # to the OTHER columns has full column rank (n-1)
        fixes: List[str] = []
        for v in unknowns:
            keep = [idx[u] for u in unknowns if u != v]
            if not keep:
                continue
            sub = [[J[i][c] for c in keep] for i in range(len(rels))]
            r_sub, _ = _rank_and_pivots(sub)
            if r_sub == n - 1:
                others = [u for u in unknowns if u != v]
                fixes.append(
                    f"bind `{v}` — then {', '.join(others)} follow "
                    f"(rank rises to {n - 1})")

        return Diagnosis(
            unknowns=unknowns,
            equations=[r.equation for r in rels],
            dof=max(0, n - rank),
            free_vars=free_vars,
            orphans=orphans,
            minimal_fixes=fixes,
            rank=rank,
        )


# =============================================================================
# PART B — THE .aven GRAMMAR: lexer + parser
# =============================================================================

KEYWORDS = {'forward', 'relation', 'solve', 'with', 'signal', 'unit'}


@dataclass(frozen=True)
class Tok:
    kind: str      # IDENT NUMBER KW OP NEWLINE EOF
    text: str
    line: int
    col: int

    def where(self) -> str:
        return f"line {self.line}, col {self.col}"


class AvenSyntaxError(Exception):
    def __init__(self, msg: str, tok: Tok):
        super().__init__(f"{tok.where()}: {msg}")


def lex(src: str, filename: str = "input.aven") -> List[Tok]:
    toks: List[Tok] = []
    i, line, col = 0, 1, 1

    def advance(k=1):
        nonlocal i, line, col
        for _ in range(k):
            if i < len(src) and src[i] == '\n':
                line += 1
                col = 1
            else:
                col += 1
            i += 1

    while i < len(src):
        c = src[i]
        if c in ' \t\r':
            advance()
        elif c == '\n':
            toks.append(Tok('NEWLINE', '\\n', line, col))
            advance()
        elif c == '/' and i + 1 < len(src) and src[i + 1] == '/':
            while i < len(src) and src[i] != '\n':
                advance()
        elif c.isdigit() or (c == '.' and i + 1 < len(src)
                              and src[i + 1].isdigit()):
            start_col = col
            m = re.match(r'\d+(\.\d+)?([eE][+-]?\d+)?', src[i:])
            text = m.group(0)
            toks.append(Tok('NUMBER', text, line, start_col))
            advance(len(text))
        elif c.isalpha() or c == '_':
            start_col = col
            m = re.match(r'[A-Za-z_]\w*', src[i:])
            text = m.group(0)
            kind = 'KW' if text in KEYWORDS else 'IDENT'
            toks.append(Tok(kind, text, line, start_col))
            advance(len(text))
        elif c in '+-*/^(){}=:,':
            toks.append(Tok('OP', c, line, col))
            advance()
        else:
            raise AvenSyntaxError(
                f"unexpected character {c!r}", Tok('OP', c, line, col))
    toks.append(Tok('EOF', '<eof>', line, col))
    return toks


# ---- module AST ---------------------------------------------------------------

@dataclass
class Equation:
    lhs: str
    rhs: object
    text: str
    tok: Tok

@dataclass
class BlockDecl:
    directive: str          # 'forward' | 'relation'
    name: str
    equations: List[Equation]

@dataclass
class Binding:
    name: str
    value: float
    unit: str               # '' if none

@dataclass
class SolveDecl:
    name: str
    bindings: List[Binding]
    tok: Tok

@dataclass
class SignalDecl:
    name: str
    unit: str

@dataclass
class UnitDecl:
    dimension: str
    units: List[str]

@dataclass
class AvenModule:
    name: str
    blocks: List[BlockDecl]
    solves: List[SolveDecl]
    signals: List[SignalDecl]
    unit_decls: List[UnitDecl]


# ---- expression parser over the token stream ---------------------------------

class Parser:
    def __init__(self, toks: List[Tok]):
        self.toks = toks
        self.p = 0

    def peek(self, k=0) -> Tok:
        return self.toks[min(self.p + k, len(self.toks) - 1)]

    def next(self) -> Tok:
        t = self.toks[self.p]
        if t.kind != 'EOF':
            self.p += 1
        return t

    def expect_op(self, op: str) -> Tok:
        t = self.peek()
        if t.kind == 'OP' and t.text == op:
            return self.next()
        raise AvenSyntaxError(f"expected '{op}', found '{t.text}'", t)

    def expect_kw(self, kw: str) -> Tok:
        t = self.peek()
        if t.kind == 'KW' and t.text == kw:
            return self.next()
        raise AvenSyntaxError(f"expected '{kw}', found '{t.text}'", t)

    def expect_ident(self) -> Tok:
        t = self.peek()
        if t.kind == 'IDENT':
            return self.next()
        raise AvenSyntaxError(
            f"expected an identifier, found '{t.text}'", t)

    def skip_newlines(self):
        while self.peek().kind == 'NEWLINE':
            self.next()

    # ---- statements ----

    def parse_module(self, name="module") -> AvenModule:
        blocks, solves, signals, units = [], [], [], []
        while self.peek().kind != 'EOF':
            self.skip_newlines()
            if self.peek().kind == 'EOF':
                break
            t = self.peek()
            if t.kind == 'KW' and t.text in ('forward', 'relation'):
                blocks.append(self.parse_block())
            elif t.kind == 'KW' and t.text == 'solve':
                solves.append(self.parse_solve())
            elif t.kind == 'KW' and t.text == 'signal':
                signals.append(self.parse_signal())
            elif t.kind == 'KW' and t.text == 'unit':
                units.append(self.parse_unitdecl())
            else:
                raise AvenSyntaxError(
                    "expected a declaration ('forward', 'relation', "
                    "'solve', 'signal', or 'unit'), "
                    f"found '{t.text}'", t)
        return AvenModule(name, blocks, solves, signals, units)

    def parse_block(self) -> BlockDecl:
        d = self.next()                      # forward | relation
        name_tok = self.expect_ident()
        self.expect_op('{')
        eqs: List[Equation] = []
        self.skip_newlines()
        while not (self.peek().kind == 'OP' and self.peek().text == '}'):
            eqs.append(self.parse_equation())
            self.skip_newlines()
        self.expect_op('}')
        return BlockDecl(d.text, name_tok.text, eqs)

    def parse_equation(self) -> Equation:
        lhs_tok = self.expect_ident()
        self.expect_op('=')
        rhs = self.parse_expr()
        return Equation(lhs_tok.text, rhs,
                        f"{lhs_tok.text} = {_src(rhs)}", lhs_tok)

    def parse_solve(self) -> SolveDecl:
        kw = self.expect_kw('solve')
        name = self.expect_ident()
        self.expect_kw('with')
        self.expect_op('{')
        self.skip_newlines()
        binds: List[Binding] = []
        while not (self.peek().kind == 'OP' and self.peek().text == '}'):
            binds.append(self.parse_binding())
            self.skip_newlines()
        self.expect_op('}')
        return SolveDecl(name.text, binds, kw)

    def parse_binding(self) -> Binding:
        name = self.expect_ident()
        self.expect_op('=')
        nt = self.peek()
        if nt.kind != 'NUMBER':
            raise AvenSyntaxError(
                f"expected a number for binding '{name.text}', "
                f"found '{nt.text}'", nt)
        self.next()
        # optional unit literal: IDENT (/ IDENT)* until newline/comma
        unit_parts: List[str] = []
        while (self.peek().kind == 'IDENT'
               or (self.peek().kind == 'OP' and self.peek().text == '/')):
            unit_parts.append(self.next().text)
        return Binding(name.text, float(nt.text), ''.join(unit_parts))

    def parse_signal(self) -> SignalDecl:
        self.expect_kw('signal')
        name = self.expect_ident()
        unit = ''
        if self.peek().kind == 'OP' and self.peek().text == ':':
            self.next()
            unit = self.expect_ident().text
        return SignalDecl(name.text, unit)

    def parse_unitdecl(self) -> UnitDecl:
        self.expect_kw('unit')
        dim = self.expect_ident()
        self.expect_op(':')
        units = [self.expect_ident().text]
        while self.peek().kind == 'OP' and self.peek().text == ',':
            self.next()
            units.append(self.expect_ident().text)
        return UnitDecl(dim.text, units)

    # ---- expressions: precedence + - < * / < unary < ^ (right) < atom ----

    def parse_expr(self):
        e = self.additive()
        return e

    def additive(self):
        e = self.multiplicative()
        while self.peek().kind == 'OP' and self.peek().text in '+-':
            op = self.next().text
            e = Bin(op, e, self.multiplicative())
        return e

    def multiplicative(self):
        e = self.unary()
        while self.peek().kind == 'OP' and self.peek().text in '*/':
            op = self.next().text
            e = Bin(op, e, self.unary())
        return e

    def unary(self):
        if self.peek().kind == 'OP' and self.peek().text == '-':
            self.next()
            return Bin('-', Num(0), self.unary())
        return self.power()

    def power(self):
        base = self.atom()
        if self.peek().kind == 'OP' and self.peek().text == '^':
            self.next()
            return Bin('^', base, self.unary())
        return base

    def atom(self):
        t = self.peek()
        if t.kind == 'OP' and t.text == '(':
            self.next()
            e = self.additive()
            self.expect_op(')')
            return e
        if t.kind == 'NUMBER':
            self.next()
            return Num(float(t.text))
        if t.kind == 'IDENT':
            self.next()
            if (self.peek().kind == 'OP' and self.peek().text == '('
                    and t.text in FUNCS):
                self.next()
                arg = self.additive()
                self.expect_op(')')
                return Call(t.text, arg)
            if self.peek().kind == 'OP' and self.peek().text == '(':
                raise AvenSyntaxError(
                    f"unknown function '{t.text}' (have: "
                    f"{', '.join(sorted(FUNCS))})", t)
            return Var(t.text)
        raise AvenSyntaxError(f"expected a value or name, found '{t.text}'", t)


def _src(e) -> str:
    PREC = {'+': 1, '-': 1, '*': 2, '/': 2, '^': 3}
    if isinstance(e, Num):
        return f"{e.value:g}"
    if isinstance(e, Var):
        return e.name
    if isinstance(e, Call):
        return f"{e.fn}({_src(e.arg)})"
    p = PREC[e.op]
    s = f"{_src(e.left)} {e.op} {_src(e.right)}"
    return f"({s})" if p == 1 else s


def parse_aven(src: str, name: str = "module") -> AvenModule:
    return Parser(lex(src)).parse_module(name)


# =============================================================================
# PART C — Front-end: module → engine, with pin enforcement + diagnostics
# =============================================================================

class AvenInconsistency(ValueError):
    pass


class PinnedDirectionError(Exception):
    pass


def topo_order(eq_texts: List[Tuple[str, Set[str]]], blockname: str):
    """eq_texts: [(lhs, rhs_vars)]; returns firing indices or raises."""
    derived = {lhs for lhs, _ in eq_texts}
    known = set()
    for _, rv in eq_texts:
        known |= rv - derived
    order, placed = [], set()
    while len(order) < len(eq_texts):
        progressed = False
        for idx, (lhs, rv) in enumerate(eq_texts):
            if idx in placed:
                continue
            if rv <= known:
                order.append(idx)
                placed.add(idx)
                known.add(lhs)
                progressed = True
        if not progressed:
            stuck = [eq_texts[i][0] for i in range(len(eq_texts))
                     if i not in placed]
            raise PinnedDirectionError(
                f"forward block '{blockname}' cannot fire forward — "
                f"circular/unresolvable: [{', '.join(stuck)}]. "
                f"Declare it 'relation' to allow omnidirectional solving.")
    return order


class AvenCompiler:
    """Turns an AvenModule into a runnable program."""

    def __init__(self, mod: AvenModule):
        self.mod = mod
        self.relations: List[Relation] = []
        self.blocks_by_name: Dict[str, BlockDecl] = {}
        self._prepare()

    def _prepare(self):
        for blk in self.mod.blocks:
            self.blocks_by_name[blk.name] = blk
            for eq in blk.equations:
                self.relations.append(
                    Relation(f"{blk.name}.{eq.lhs}", eq.text, eq.lhs, eq.rhs))

    # ---- the solve driver ----

    def run_solve(self, sd: SolveDecl,
                  verbose: bool = True) -> Tuple[Dict[str, float],
                                                 Optional[Diagnosis]]:
        blk = self.blocks_by_name.get(sd.name)
        if blk is None:
            raise ValueError(f"solve refers to unknown block '{sd.name}'")

        env: Dict[str, float] = {}
        bound_units: Dict[str, str] = {}
        for b in sd.bindings:
            env[b.name] = b.value
            bound_units[b.name] = b.unit

        # forward-pin enforcement: all RHS-inputs must be bound; derived
        # variables must NOT be given contradictory bindings
        if blk.directive == 'forward':
            derived = {eq.lhs for eq in blk.equations}
            order = topo_order(
                [(eq.lhs, collect_vars(eq.rhs)) for eq in blk.equations],
                blk.name)
            for oi in order:
                eq = blk.equations[oi]
                missing = [v for v in collect_vars(eq.rhs) if v not in env]
                if missing:
                    raise PinnedDirectionError(
                        f"forward block '{blk.name}' is direction-pinned: "
                        f"equation '{eq.text}' needs bindings for "
                        f"{', '.join(missing)}. Solve backwards requires "
                        f"a 'relation' block.")
                try:
                    env[eq.lhs] = eval_expr(eq.rhs, env)
                except KeyError as k:
                    raise PinnedDirectionError(
                        f"unbound input {k} in '{eq.text}'")
            if verbose:
                print(f"  [{blk.directive} {blk.name}] "
                      f"fired {len(blk.equations)} equations in pinned "
                      f"order — no solver involved")
            return env, None

        # omnidirectional solve with fixpoint + diagnostics on stall
        diag: Optional[Diagnosis] = None
        for _round in range(300):
            progressed = False
            for rel in self.relations:
                unknowns = [v for v in rel.variables if v not in env]
                if not unknowns:
                    # consistency check
                    exp = eval_expr(rel.rhs, env)
                    if abs(exp - env[rel.lhs]) > 1e-6 * max(1.0, abs(exp)):
                        raise AvenInconsistency(
                            f"'{rel.name}': implies {rel.lhs}={exp:.6g}, "
                            f"bound to {env[rel.lhs]:.6g}")
                elif len(unknowns) == 1:
                    u = unknowns[0]
                    if u == rel.lhs:
                        env[u] = eval_expr(rel.rhs, env)
                        progressed = True
                    elif rel.lhs in env:
                        x = _backward_newton(rel, u, env)
                        if x is not None:
                            env[u] = x
                            progressed = True

            if progressed:
                continue

            # stall: under-determined or coupled-block needed
            blocked = [r for r in self.relations
                       if any(v not in env for v in r.variables)]
            unknowns = sorted({v for r in blocked
                               for v in r.variables if v not in env})
            if unknowns:
                diag = Diagnostician().diagnose(unknowns, blocked, env)
            break

        return env, diag


def _backward_newton(rel: Relation, u: str,
                     env: Dict[str, float]) -> Optional[float]:
    target = env[rel.lhs]

    def g(x):
        local = dict(env)
        local[u] = x
        try:
            return eval_expr(rel.rhs, local) - target
        except Exception:
            return float('nan')

    try:
        d = diff(rel.rhs, u)

        def dg(x):
            local = dict(env)
            local[u] = x
            try:
                return eval_expr(d, local)
            except Exception:
                return None
    except UnsupportedDerivative:
        dg = None

    for x0 in (1.0, 0.5, 2.0, 10.0, 0.1, -1.0, 100.0):
        x = x0
        for _ in range(60):
            fx = g(x)
            if not math.isfinite(fx):
                break
            if abs(fx) < 1e-9:
                return x
            der = dg(x) if dg else None
            if der is None or der == 0:
                h = 1e-6 * max(1.0, abs(x))
                der = (g(x + h) - g(x - h)) / (2 * h)
                if der == 0:
                    break
            x = x - fx / der
    return None


# =============================================================================
# DEMOS
# =============================================================================

FLEET = """\
// fleet.aven — economics of a delivery fleet
unit Length: m, km
unit Money: eur

relation Revenue {
    revenue = price * units
    profit  = revenue - cost
}

forward Trip {
    distance = speed * time
    fuel     = distance * consumption
    cost     = fuel * price
}

signal fleet_temp: Celsius

solve Revenue with {
    profit = 500
    cost   = 300
}
"""

FLEET_FIXED = """\
relation Revenue {
    revenue = price * units
    profit  = revenue - cost
}

solve Revenue with {
    profit = 500
    cost   = 300
    price  = 25
}
"""

BAD_SYNTAX = """\
forward Oops {
    distance = speed *
}
"""

BACKWARDS_PINNED = """\
forward Trip {
    distance = speed * time
}

solve Trip with {
    distance = 450
    speed = 90
}
"""

ORPHAN_DEMO = """\
relation Weird {
    a = b + c
}

solve Weird with {
    a = 10
}
"""


def banner(t):
    print("\n" + "=" * 68 + f"\n{t}\n" + "=" * 68)


def main():
    print(f"{LANG_NAME} v{VERSION} — Phase 4.3 diagnostics + real grammar")

    # ---- 1. parse a real .aven file ------------------------------------
    banner("1 — Parsing a real .aven source file")
    mod = parse_aven(FLEET, "fleet")
    print(f"  parsed '{mod.name}':")
    for blk in mod.blocks:
        print(f"    {blk.directive} {blk.name}:")
        for eq in blk.equations:
            print(f"        {eq.text}")
    for sd in mod.solves:
        bs = ", ".join(f"{b.name}={b.value}{b.unit}"
                       for b in sd.bindings)
        print(f"    solve {sd.name} with {{ {bs} }}")
    print(f"    signals: {[s.name + ':' + s.unit for s in mod.signals]}")
    print(f"    units:   {[(u.dimension, u.units) for u in mod.unit_decls]}")

    # ---- 2. THE diagnostics demo: under-determined solve ----------------
    banner("2 — Under-determined solve → full diagnosis")
    comp = AvenCompiler(mod)
    env, diag = comp.run_solve(mod.solves[0])
    if diag is not None:
        print(diag.report())
        print("\n  Interpretation: profit and cost pin revenue (=800), but")
        print("  'revenue = price × units' leaves one degree of freedom —")
        print("  price and units can slide against each other freely.")

    # ---- 3. apply the suggested fix -------------------------------------
    banner("3 — Applying the suggested fix (bind price = 25)")
    mod2 = parse_aven(FLEET_FIXED, "fleet-fixed")
    comp2 = AvenCompiler(mod2)
    env2, diag2 = comp2.run_solve(mod2.solves[0])
    if diag2 is None:
        print("  solve Revenue with { profit=500, cost=300, price=25 }")
        print(f"  ⟹ revenue = {env2['revenue']:.4f}")
        print(f"  ⟹ units   = {env2['units']:.4f}   "
              f"(check: 25 × {env2['units']:.4f} = "
              f"{25 * env2['units']:.4f} = revenue ✓)")

    # ---- 4. grammar errors are precise ----------------------------------
    banner("4 — Syntax errors carry line/column and say what they want")
    try:
        parse_aven(BAD_SYNTAX)
    except AvenSyntaxError as e:
        print(f"  error: {e}")

    # ---- 5. forward-pin enforcement -------------------------------------
    banner("5 — Direction pinning is enforced with guidance")
    try:
        modb = parse_aven(BACKWARDS_PINNED, "pinned")
        AvenCompiler(modb).run_solve(modb.solves[0])
    except PinnedDirectionError as e:
        print(f"  rejected ✓ {e}")

    # ---- 6. orphan detection ----------------------------------------------
    banner("6 — Orphan variables are called out separately")
    modo = parse_aven(ORPHAN_DEMO, "weird")
    envo, diago = AvenCompiler(modo).run_solve(modo.solves[0])
    if diago is not None:
        print(diago.report())

    banner(f"{LANG_NAME} v{VERSION} complete — stalled solves now teach")


if __name__ == "__main__":
    main()
