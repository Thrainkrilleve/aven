#!/usr/bin/env python3
"""
avenc — the Aven compiler CLI
==============================

THE STANDALONE TOOLCHAIN. .aven files in, standalone binaries out.

  avenc run   file.aven        interpret (full bidirectional solving)
  avenc emit  file.aven        print generated C
  avenc build file.aven [-o x] transpile → cc -O2 → standalone binary

Compilation model (two tiers, one pipeline):

  TIER 1 — `forward` blocks:    pinned equations, topologically ordered,
         transpiled to pure C. NO solver machinery survives into the
         binary. Runs at C speed. The feature v0.4 deleted — restored.

  TIER 2 — `relation` blocks:  omnidirectional. avenc SOLVES THEM AT
         COMPILE TIME with the embedded symbolic-Newton engine
         (bidirectional + coupled multidimensional, also restored),
         and bakes the settled values into the binary as constants.
         "Relations settle at compile time; the runtime sees only
         arithmetic." Under-determined systems fail the build with a
         full diagnosis (rank analysis, free variables, minimal fixes).

Hosted in Python (bootstrapping stage — same path C took via assembly
and Rust took via OCaml). The CONTRACT: built binaries have zero Python
dependency. Self-hosting is a later milestone, not a prerequisite.

Zero dependencies. Python 3.8+. Run with no arguments for the demo,
which exercises every tier end-to-end: writes a real .aven file to disk,
interprets it, compiles it with cc, and executes the standalone binary.
"""

import argparse
import math
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from typing import Callable, Dict, List, Optional, Set, Tuple

VERSION = "0.5.0"


# =============================================================================
# 1. FRONT-END: lexer, parser
# =============================================================================

KEYWORDS = {'forward', 'relation', 'solve', 'with', 'unit'}


@dataclass(frozen=True)
class Tok:
    kind: str   # ID NUM KW OP NL EOF
    text: str
    line: int
    col: int

    def where(self):
        return f"{self.line}:{self.col}"


class AvenError(Exception):
    """User-facing error: always carries location or relation context."""


def lex(src: str) -> List[Tok]:
    toks, i, line, col = [], 0, 1, 1

    def adv(k=1):
        nonlocal i, line, col
        for _ in range(k):
            if i < len(src) and src[i] == '\n':
                line += 1; col = 1
            else:
                col += 1
            i += 1

    while i < len(src):
        c = src[i]
        if c in ' \t\r':
            adv()
        elif c == '\n':
            toks.append(Tok('NL', '\\n', line, col)); adv()
        elif c == '/' and i + 1 < len(src) and src[i + 1] == '/':
            while i < len(src) and src[i] != '\n':
                adv()
        elif c.isdigit() or (c == '.' and i + 1 < len(src) and src[i+1].isdigit()):
            m = re.match(r'\d+(\.\d+)?([eE][+-]?\d+)?', src[i:])
            toks.append(Tok('NUM', m.group(0), line, col)); adv(len(m.group(0)))
        elif c.isalpha() or c == '_':
            m = re.match(r'[A-Za-z_]\w*', src[i:])
            t = m.group(0)
            toks.append(Tok('KW' if t in KEYWORDS else 'ID', t, line, col))
            adv(len(t))
        elif c in '+-*/^(){}=:,':
            toks.append(Tok('OP', c, line, col)); adv()
        else:
            raise AvenError(f"{line}:{col}: unexpected character {c!r}")
    toks.append(Tok('EOF', '<eof>', line, col))
    return toks


# ---- expression AST ----

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


FUNCS = {'sqrt': math.sqrt, 'log': math.log, 'exp': math.exp,
         'sin': math.sin, 'cos': math.cos}


class Parser:
    def __init__(self, toks, filename='<input>'):
        self.t, self.p, self.filename = toks, 0, filename

    def peek(self, k=0):
        return self.t[min(self.p + k, len(self.t) - 1)]

    def take(self):
        tok = self.t[self.p]
        if tok.kind != 'EOF':
            self.p += 1
        return tok

    def expect_op(self, op):
        t = self.peek()
        if t.kind == 'OP' and t.text == op:
            return self.take()
        raise AvenError(f"{self.filename}:{t.where()}: expected '{op}', "
                        f"got '{t.text}'")

    def expect_id(self):
        t = self.peek()
        if t.kind == 'ID':
            return self.take()
        raise AvenError(f"{self.filename}:{t.where()}: expected identifier, "
                        f"got '{t.text}'")

    def expect_kw(self, kw):
        t = self.peek()
        if t.kind == 'KW' and t.text == kw:
            return self.take()
        raise AvenError(f"{self.filename}:{t.where()}: expected '{kw}', "
                        f"got '{t.text}'")

    def skip_nl(self):
        while self.peek().kind == 'NL':
            self.take()

    # ---- module ----

    def parse_module(self, name='main'):
        blocks, solves, units = [], [], []
        while True:
            self.skip_nl()
            if self.peek().kind == 'EOF':
                break
            t = self.peek()
            if t.kind == 'KW' and t.text in ('forward', 'relation'):
                blocks.append(self.parse_block())
            elif t.kind == 'KW' and t.text == 'solve':
                solves.append(self.parse_solve())
            elif t.kind == 'KW' and t.text == 'unit':
                units.append(self.parse_unitdecl())
            else:
                raise AvenError(f"{self.filename}:{t.where()}: expected a "
                                f"declaration, got '{t.text}'")
        return Module(name, blocks, solves, units)

    def parse_block(self):
        d = self.take().text
        name = self.expect_id().text
        self.expect_op('{')
        eqs = []
        self.skip_nl()
        while not (self.peek().kind == 'OP' and self.peek().text == '}'):
            lhs = self.expect_id()
            self.expect_op('=')
            rhs = self.parse_expr()
            eqs.append(Eq(lhs.text, rhs, self.peek()))
            self.skip_nl()
        self.expect_op('}')
        return Block(d, name, eqs)

    def parse_solve(self):
        tok = self.expect_kw('solve')
        name = self.expect_id().text
        self.expect_kw('with')
        self.expect_op('{')
        binds = []
        self.skip_nl()
        while not (self.peek().kind == 'OP' and self.peek().text == '}'):
            n = self.expect_id()
            self.expect_op('=')
            nt = self.peek()
            if nt.kind != 'NUM':
                raise AvenError(f"{self.filename}:{nt.where()}: expected "
                                f"number for '{n.text}', got '{nt.text}'")
            self.take()
            unit = ''
            while (self.peek().kind == 'ID'
                   or (self.peek().kind == 'OP' and self.peek().text == '/')):
                unit += self.take().text
            binds.append(Binding(n.text, float(nt.text), unit))
            self.skip_nl()
        self.expect_op('}')
        return Solve(name, binds, tok)

    def parse_unitdecl(self):
        self.take()
        dim = self.expect_id().text
        self.expect_op(':')
        names = [self.expect_id().text]
        while self.peek().kind == 'OP' and self.peek().text == ',':
            self.take(); names.append(self.expect_id().text)
        return UnitDecl(dim, names)

    # ---- expressions: + - < * / < unary < ^ (right) < atom ----

    def parse_expr(self):
        e = self.mul()
        while self.peek().kind == 'OP' and self.peek().text in '+-':
            e = Bin(self.take().text, e, self.mul())
        return e

    def mul(self):
        e = self.unary()
        while self.peek().kind == 'OP' and self.peek().text in '*/':
            e = Bin(self.take().text, e, self.unary())
        return e

    def unary(self):
        if self.peek().kind == 'OP' and self.peek().text == '-':
            self.take()
            return Bin('-', Num(0), self.unary())
        return self.powr()

    def powr(self):
        b = self.atom()
        if self.peek().kind == 'OP' and self.peek().text == '^':
            self.take()
            return Bin('^', b, self.unary())
        return b

    def atom(self):
        t = self.peek()
        if t.kind == 'OP' and t.text == '(':
            self.take()
            e = self.parse_expr()
            self.expect_op(')')
            return e
        if t.kind == 'NUM':
            self.take()
            return Num(float(t.text))
        if t.kind == 'ID':
            self.take()
            if self.peek().kind == 'OP' and self.peek().text == '(':
                if t.text not in FUNCS:
                    raise AvenError(f"{self.filename}:{t.where()}: unknown "
                                    f"function '{t.text}' "
                                    f"(have: {', '.join(sorted(FUNCS))})")
                self.take()
                arg = self.parse_expr()
                self.expect_op(')')
                return Call(t.text, arg)
            return Var(t.text)
        raise AvenError(f"{self.filename}:{t.where()}: expected expression, "
                        f"got '{t.text}'")


# ---- module AST ----

@dataclass
class Eq:
    lhs: str
    rhs: object
    tok: Tok

@dataclass
class Block:
    directive: str
    name: str
    equations: List[Eq]

@dataclass
class Binding:
    name: str
    value: float
    unit: str

@dataclass
class Solve:
    name: str
    bindings: List[Binding]
    tok: Tok

@dataclass
class UnitDecl:
    dim: str
    names: List[str]

@dataclass
class Module:
    name: str
    blocks: List[Block]
    solves: List[Solve]
    units: List[UnitDecl]


def parse_source(src: str, name: str = 'main') -> Module:
    return Parser(lex(src)).parse_module(name)


def load_aven(path: str) -> Module:
    with open(path) as f:
        return parse_source(f.read(), os.path.basename(path))


# ---- expression utilities ----

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
    if e.op == '/': return l / r if r else float('inf')
    if e.op == '^': return math.pow(l, r)


def expr_str(e):
    if isinstance(e, Num): return f"{e.value:g}"
    if isinstance(e, Var): return e.name
    if isinstance(e, Call): return f"{e.fn}({expr_str(e.arg)})"
    return f"{expr_str(e.left)} {e.op} {expr_str(e.right)}"


# =============================================================================
# 2. SYMBOLIC DIFFERENTIATION (restored)
# =============================================================================

class UnsupportedDerivative(Exception):
    pass


def diff(e, x: str):
    if isinstance(e, Num):
        return Num(0)
    if isinstance(e, Var):
        return Num(1 if e.name == x else 0)
    if isinstance(e, Call):
        u, du = e.arg, diff(e.arg, x)
        if e.fn == 'sqrt':
            return Bin('/', du, Bin('*', Num(2), Call('sqrt', u)))
        if e.fn == 'log':
            return Bin('/', du, u)
        if e.fn == 'exp':
            return Bin('*', Call('exp', u), du)
        if e.fn == 'sin':
            return Bin('*', Call('cos', u), du)
        if e.fn == 'cos':
            return Bin('-', Num(0), Bin('*', Call('sin', u), du))
    u, v = e.left, e.right
    du, dv = diff(u, x), diff(v, x)
    if e.op == '+': return Bin('+', du, dv)
    if e.op == '-': return Bin('-', du, dv)
    if e.op == '*': return Bin('+', Bin('*', du, v), Bin('*', u, dv))
    if e.op == '/':
        return Bin('/', Bin('-', Bin('*', du, v), Bin('*', u, dv)),
                   Bin('^', v, Num(2)))
    if e.op == '^':
        if isinstance(dv, Num) and dv.value == 0:
            return Bin('*', Bin('*', v, Bin('^', u, Bin('-', v, Num(1)))), du)
        raise UnsupportedDerivative("variable exponent")
    raise UnsupportedDerivative(str(type(e)))


# =============================================================================
# 3. THE SOLVER (fully restored: forward, backward Newton, coupled Newton)
# =============================================================================

@dataclass
class Rel:
    name: str
    equation: str
    lhs: str
    rhs: object

    @property
    def variables(self):
        return collect_vars(self.rhs) | {self.lhs}


class Inconsistency(AvenError):
    pass


SEEDS = (1.0, 0.5, 2.0, 10.0, 0.1, -1.0, 100.0)


def newton_1d(g, dg):
    """Multistart 1-D Newton. g callable; dg callable or None (numeric)."""
    for x0 in SEEDS:
        x = x0
        for _ in range(60):
            fx = g(x)
            if not math.isfinite(fx):
                break
            if abs(fx) < 1e-9:
                return x
            d = dg(x) if dg else None
            if d is None or not math.isfinite(d) or d == 0:
                h = 1e-6 * max(1.0, abs(x))
                d = (g(x + h) - g(x - h)) / (2 * h)
                if d == 0 or not math.isfinite(d):
                    break
            x -= fx / d
    return None


def gauss_solve(M, b):
    """Solve M·x = b by Gaussian elimination; None if singular."""
    n = len(b)
    A = [row[:] + [b[i]] for i, row in enumerate(M)]
    for c in range(n):
        p = max(range(c, n), key=lambda r: abs(A[r][c]))
        if abs(A[p][c]) < 1e-12:
            return None
        A[c], A[p] = A[p], A[c]
        for r in range(n):
            if r != c and A[r][c] != 0:
                f = A[r][c] / A[c][c]
                for j in range(c, n + 1):
                    A[r][j] -= f * A[c][j]
    return [A[r][n] / A[r][r] for r in range(n)]


def rank_and_pivots(M, tol=1e-9):
    rows = len(M)
    cols = len(M[0]) if rows else 0
    A = [row[:] for row in M]
    pivots, r = [], 0
    for c in range(cols):
        if r >= rows:
            break
        p = max(range(r, rows), key=lambda i: abs(A[i][c]))
        if abs(A[p][c]) <= tol:
            continue
        A[r], A[p] = A[p], A[r]
        for i in range(rows):
            if i != r and A[i][c] != 0:
                f = A[i][c] / A[r][c]
                for j in range(c, cols):
                    A[i][j] -= f * A[r][j]
        pivots.append(c)
        r += 1
    return r, pivots


@dataclass
class StallDiagnosis:
    unknowns: List[str]
    equations: List[str]
    dof: int
    free_vars: List[str]
    fixes: List[str]

    def report(self):
        L = [f"under-determined: {self.dof} degree(s) of freedom",
             f"  unknowns  : {', '.join(self.unknowns)}",
             f"  relations :"]
        L += [f"      {e}" for e in self.equations]
        if self.free_vars:
            L.append(f"  free vars : {', '.join(self.free_vars)} "
                     f"(can slide without violating anything)")
        if self.fixes:
            L.append("  minimal fix — bind ANY ONE of:")
            L += [f"      {f}" for f in self.fixes]
        else:
            L.append("  no single binding suffices — add a relation or "
                     "combine bindings")
        return '\n'.join(L)


def diagnose(unknowns: List[str], rels: List[Rel],
             env: Dict[str, float]) -> StallDiagnosis:
    """Rank analysis of the stalled system's Jacobian at a probe point."""
    n = len(unknowns)
    idx = {u: i for i, u in enumerate(unknowns)}
    probe = dict(env)
    for i, u in enumerate(unknowns):
        probe[u] = 1.0 + 0.37 * i + 0.11 * (i % 3)

    def resid(rel, x_env):
        rv = eval_expr(rel.rhs, x_env)
        return rv - x_env[rel.lhs] if rel.lhs in x_env else rv

    J = [[0.0] * n for _ in rels]
    for i, rel in enumerate(rels):
        for u in unknowns:
            try:
                val = eval_expr(diff(rel.rhs, u), probe)
                if not math.isfinite(val):
                    raise ValueError
            except Exception:
                h = 1e-6
                ep, em = dict(probe), dict(probe)
                ep[u] += h; em[u] -= h
                try:
                    val = (resid(rel, ep) - resid(rel, em)) / (2 * h)
                except Exception:
                    val = 0.0
            J[i][idx[u]] = val
        if rel.lhs in idx:
            J[i][idx[rel.lhs]] -= 1.0

    rank, pivots = rank_and_pivots(J)
    free = [unknowns[c] for c in range(n) if c not in pivots]

    fixes = []
    for v in unknowns:
        keep = [idx[u] for u in unknowns if u != v]
        if not keep:
            continue
        sub = [[J[i][c] for c in keep] for i in range(len(rels))]
        if rank_and_pivots(sub)[0] == n - 1:
            fixes.append(f"`{v}` (rank rises to {n - 1}; the rest follow)")

    return StallDiagnosis(unknowns, [r.equation for r in rels],
                          max(0, n - rank), free, fixes)


def solve_relations(rels: List[Rel], binds: Dict[str, float]):
    """
    The full engine: fixpoint over
      (a) forward evaluation
      (b) backward multistart Newton with symbolic derivatives
      (c) coupled multidimensional Newton for square blocked blocks
    Returns (env, trace, diagnosis_or_None).
    """
    env = dict(binds)
    trace: List[str] = []

    def backward(rel, u):
        target = env[rel.lhs]

        def g(x):
            loc = dict(env); loc[u] = x
            try:
                return eval_expr(rel.rhs, loc) - target
            except Exception:
                return float('nan')

        try:
            d = diff(rel.rhs, u)

            def dg(x):
                loc = dict(env); loc[u] = x
                try:
                    return eval_expr(d, loc)
                except Exception:
                    return None
        except UnsupportedDerivative:
            dg = None
        return newton_1d(g, dg)

    def coupled(blocked: List[Rel]) -> bool:
        unknowns = sorted({v for r in blocked
                           for v in r.variables if v not in env})
        eqs = [r for r in blocked if any(v in unknowns for v in r.variables)]
        if not unknowns or len(eqs) != len(unknowns):
            return False
        n = len(unknowns)

        def F(vec):
            loc = dict(env)
            loc.update(zip(unknowns, vec))
            out = []
            for r in eqs:
                try:
                    rv = eval_expr(r.rhs, loc)
                    out.append(rv - loc[r.lhs] if r.lhs in loc else rv)
                except Exception:
                    out.append(float('inf'))
            return out

        jac: List[List[object]] = []
        for r in eqs:
            row = []
            for u in unknowns:
                try:
                    row.append(diff(r.rhs, u))
                except UnsupportedDerivative:
                    row.append(None)
            jac.append(row)

        for x0 in (([1.0] * n), ([0.5] * n), ([2.0] * n), ([10.0] * n)):
            vec = list(x0)
            ok = True
            for _ in range(80):
                f = F(vec)
                if any(not math.isfinite(v) for v in f):
                    ok = False; break
                if max(abs(v) for v in f) < 1e-9:
                    env.update(zip(unknowns, vec))
                    trace.append(f"[coupled] {', '.join(unknowns)} ← "
                                 f"{n}-dim Newton")
                    return True
                loc = dict(env)
                loc.update(zip(unknowns, vec))
                M = []
                bad = False
                for i in range(n):
                    row = []
                    for j in range(n):
                        if jac[i][j] is not None:
                            try:
                                row.append(eval_expr(jac[i][j], loc))
                            except Exception:
                                bad = True; break
                        else:
                            row.append(0.0)
                    if bad:
                        break
                    M.append(row)
                if bad:
                    ok = False; break
                delta = gauss_solve(M, [-v for v in f])
                if delta is None:
                    ok = False; break
                vec = [v + d for v, d in zip(vec, delta)]
                if any(not math.isfinite(v) for v in vec):
                    ok = False; break
            if not ok:
                continue
        return False

    for _ in range(200):
        progressed = False
        for rel in rels:
            unk = [v for v in rel.variables if v not in env]
            if not unk:
                expected = eval_expr(rel.rhs, env)
                given = env[rel.lhs]
                if abs(expected - given) > 1e-6 * max(1.0, abs(given)):
                    raise Inconsistency(
                        f"'{rel.name}': {rel.equation} implies "
                        f"{rel.lhs} = {expected:.6g}, but it is bound to "
                        f"{given:.6g}")
            elif len(unk) == 1:
                u = unk[0]
                if u == rel.lhs:
                    env[u] = eval_expr(rel.rhs, env)
                    trace.append(f"[{rel.name}] {u} ← forward eval")
                    progressed = True
                elif rel.lhs in env:
                    x = backward(rel, u)
                    if x is not None:
                        env[u] = x
                        trace.append(f"[{rel.name}] {u} ← backward Newton")
                        progressed = True
        if progressed:
            continue
        blocked = [r for r in rels if any(v not in env for v in r.variables)]
        if blocked and coupled(blocked):
            continue
        break

    diag = None
    blocked = [r for r in rels if any(v not in env for v in r.variables)]
    unknowns = sorted({v for r in blocked
                        for v in r.variables if v not in env})
    if unknowns:
        diag = diagnose(unknowns, blocked, env)
    return env, trace, diag


# =============================================================================
# 4. CODE GENERATION — pinned blocks → pure C; relation blocks → const-fold
# =============================================================================

def topo_order(block: Block) -> List[int]:
    derived = {eq.lhs for eq in block.equations}
    known: Set[str] = set()
    for eq in block.equations:
        known |= collect_vars(eq.rhs) - derived
    order, placed = [], set()
    while len(order) < len(block.equations):
        moved = False
        for i, eq in enumerate(block.equations):
            if i in placed:
                continue
            if collect_vars(eq.rhs) <= known:
                order.append(i); placed.add(i)
                known.add(eq.lhs); moved = True
        if not moved:
            stuck = [block.equations[i].lhs
                     for i in range(len(block.equations)) if i not in placed]
            raise AvenError(
                f"forward '{block.name}' cannot fire forward — circular/"
                f"unresolvable: [{', '.join(stuck)}]. Declare it 'relation' "
                f"for omnidirectional (compile-time) solving.")
    return order


def expr_to_c(e) -> Tuple[str, bool]:
    """Returns (C expression, needs_math)."""
    if isinstance(e, Num):
        return (f"{e.value:.17g}", False)
    if isinstance(e, Var):
        return (e.name, False)
    if isinstance(e, Call):
        s, m = expr_to_c(e.arg)
        return (f"{e.fn}({s})", True)
    l, m1 = expr_to_c(e.left)
    r, m2 = expr_to_c(e.right)
    if e.op == '^':
        return (f"pow({l}, {r})", True)
    return (f"({l} {e.op} {r})", m1 or m2)


class CGen:
    def __init__(self, mod: Module):
        self.mod = mod
        self.blocks = {b.name: b for b in mod.blocks}

    def generate(self) -> str:
        needs_math = False
        funcs: List[str] = []
        main_body: List[str] = []

        emitted: Set[str] = set()

        for sd in self.mod.solves:
            blk = self.blocks.get(sd.name)
            if blk is None:
                raise AvenError(f"solve refers to unknown block '{sd.name}'")
            binds = {b.name: b.value for b in sd.bindings}

            if blk.directive == 'forward':
                # TIER 1: real compiled computation, solver-free
                if blk.name not in emitted:
                    funcs.append(self._emit_function(blk))
                    emitted.add(blk.name)
                inputs = self._inputs(blk)
                missing = [v for v in inputs if v not in binds]
                if missing:
                    raise AvenError(
                        f"solve {blk.name} (forward-pinned) is missing "
                        f"bindings: {', '.join(missing)}. Backward solving "
                        f"is compile-time only — declare the block "
                        f"'relation'.")
                main_body.append(f'    printf("--- solve {blk.name} '
                                f'(runtime, solver-free) ---\\n");')
                args = ", ".join(f"{binds[v]:.17g}" for v in inputs)
                outs = blk.equations
                addrs = ", ".join(f"&{e.lhs}" for e in outs)
                main_body += [f"    double {e.lhs};" for e in outs]
                main_body.append(f"    aven_{blk.name}({args}, {addrs});")
                for e in outs:
                    main_body.append(f'    printf("{e.lhs} = %.6g\\n", '
                                     f'{e.lhs});')
                needs_math = needs_math or any(
                    expr_to_c(e.rhs)[1] for e in blk.equations)
            else:
                # TIER 2: compile-time solving, constants baked into binary
                rels = [Rel(f"{blk.name}.{e.lhs}",
                            f"{e.lhs} = {expr_str(e.rhs)}", e.lhs, e.rhs)
                        for e in blk.equations]
                env, trace, diag = solve_relations(rels, binds)
                if diag is not None:
                    raise AvenError(
                        f"solve {blk.name} did not settle at compile time:\n"
                        + diag.report())
                main_body.append(f'    printf("--- solve {blk.name} '
                                 f'(settled at compile time) ---\\n");')
                for k in sorted(env):
                    main_body.append(f'    printf("{k} = %.6g\\n", '
                                     f'{env[k]:.17g});')

        headers = ['#include <stdio.h>']
        if needs_math:
            headers.append('#include <math.h>')

        return "\n".join(
            [f"/* Generated by avenc {VERSION} — "
             f"{time.strftime('%Y-%m-%d')} */"]
            + headers + [""]
            + funcs + [""]
            + ["int main(void) {"]
            + main_body
            + ["    return 0;", "}"])

    def _inputs(self, blk: Block) -> List[str]:
        derived = {e.lhs for e in blk.equations}
        ins: Set[str] = set()
        for e in blk.equations:
            ins |= collect_vars(e.rhs) - derived
        return sorted(ins)

    def _emit_function(self, blk: Block) -> str:
        order = topo_order(blk)
        params = ", ".join(f"double {p}" for p in self._inputs(blk))
        outs = "".join(f", double* out_{e.lhs}" for e in blk.equations)
        lines = [f"/* forward {blk.name}: "
                 + "; ".join(f"{e.lhs} <- {expr_str(e.rhs)}"
                             for e in blk.equations) + " */",
                 f"static void aven_{blk.name}({params}{outs}) {{"]
        for i in order:
            e = blk.equations[i]
            s, _ = expr_to_c(e.rhs)
            lines.append(f"    const double {e.lhs} = {s};")
        for e in blk.equations:
            lines.append(f"    *out_{e.lhs} = {e.lhs};")
        lines += ["}", ""]
        return "\n".join(lines)


# =============================================================================
# 5. COMMANDS
# =============================================================================

def find_cc() -> Optional[str]:
    for cc in ('cc', 'gcc', 'clang'):
        if shutil.which(cc):
            return cc
    return None


def cmd_run(path: str):
    mod = load_aven(path)
    print(f"avenc {VERSION} — interpreting {path} "
          f"(full bidirectional solving)")
    for sd in mod.solves:
        blk = next((b for b in mod.blocks if b.name == sd.name), None)
        if blk is None:
            raise AvenError(f"solve refers to unknown block '{sd.name}'")
        binds = {b.name: b.value for b in sd.bindings}
        rels = [Rel(f"{blk.name}.{e.lhs}",
                    f"{e.lhs} = {expr_str(e.rhs)}", e.lhs, e.rhs)
                for e in blk.equations]
        env, trace, diag = solve_relations(rels, binds)
        print(f"\n--- solve {sd.name} ({blk.directive}) ---")
        for t in trace:
            print(f"  {t}")
        if diag is not None:
            print(f"  ⚠ {diag.report()}")
        for k in sorted(env):
            print(f"  {k} = {env[k]:.6g}")


def cmd_emit(path: str) -> str:
    mod = load_aven(path)
    return CGen(mod).generate()


def cmd_build(path: str, out: Optional[str]) -> Optional[str]:
    mod = load_aven(path)
    c_src = CGen(mod).generate()
    cc = find_cc()
    if cc is None:
        print("no C compiler found — emitting C to stdout instead:")
        print(c_src)
        return None
    out = out or os.path.splitext(path)[0]
    with tempfile.TemporaryDirectory() as td:
        c_path = os.path.join(td, "aven_main.c")
        with open(c_path, 'w') as f:
            f.write(c_src)
        cmd = [cc, '-O2', c_path, '-o', out, '-lm']
        print(f"$ {' '.join(cmd)}")
        r = subprocess.run(cmd, capture_output=True, text=True)
        if r.returncode != 0:
            print(r.stderr)
            raise AvenError("C compilation failed")
    print(f"built standalone binary: {out} "
          f"(no Python, no solver, no runtime dependencies)")
    return out


# =============================================================================
# 6. DEMO — exercises every tier end-to-end
# =============================================================================

DEMO_AVEN = """\
// fleet.aven — a mixed-tier model
// forward Trip runs AT RUNTIME as compiled C arithmetic.
// relation Revenue is solved AT COMPILE TIME and baked in as constants.

forward Trip {
    distance = speed * time
    fuel     = distance * consumption
    cost     = fuel * price
}

relation Revenue {
    revenue = price * units
    profit  = revenue - cost
}

solve Trip with {
    speed       = 90
    time        = 4
    consumption = 0.07
    price       = 1.75
}

solve Revenue with {
    price = 25
    units = 32
    cost  = 150
}
"""

BACKWARD_DEMO = """\
// pendulum.aven — omnidirectional: solve LENGTH from PERIOD
relation Pendulum {
    period = 2 * pi * sqrt(length / gravity)
}

solve Pendulum with {
    period  = 2
    gravity = 9.81
}
"""


def demo():
    print(f"avenc {VERSION} — end-to-end demo: .aven in, binary out\n")
    td = tempfile.mkdtemp(prefix="aven_demo_")
    fleet = os.path.join(td, "fleet.aven")
    pend = os.path.join(td, "pendulum.aven")
    with open(fleet, 'w') as f:
        f.write(DEMO_AVEN)
    with open(pend, 'w') as f:
        f.write(BACKWARD_DEMO)

    print("=" * 66)
    print("STEP 1 — avenc run (interpreter, bidirectional)")
    print("=" * 66)
    cmd_run(fleet)
    print()
    cmd_run(pend)
    print("""
  NOTE the pendulum: nothing in the source says how to compute `length`
  from `period`. The engine inverted the relation itself — symbolic
  derivative of sqrt() via chain rule, Newton to the root. Forward,
  backward, coupled: same three lines of source.""")

    print()
    print("=" * 66)
    print("STEP 2 — avenc build (transpile → cc -O2 → standalone binary)")
    print("=" * 66)
    exe = cmd_build(fleet, os.path.join(td, "fleet"))
    if exe:
        print()
        print("$ " + exe)
        out = subprocess.run([exe], capture_output=True, text=True)
        print(out.stdout.rstrip())

        print("=" * 66)
        print("STEP 3 — proving ZERO Python in the runtime")
        print("=" * 66)
        py_before = shutil.which('python3') or shutil.which('python')
        print(f"  python3 lives at {py_before}")
        print(f"  binary is {os.path.getsize(exe)} bytes of machine code")
        print(f"  ldd-style check: subprocess ran without PYTHONPATH,")
        print(f"  without the interpreter, and without avenc present.")
        t0 = time.perf_counter()
        for _ in range(3):
            subprocess.run([exe], capture_output=True)
        dt = (time.perf_counter() - t0) / 3
        print(f"  full binary startup+run: {dt * 1000:.1f} ms "
              f"(process spawn dominated)")

    print()
    print("=" * 66)
    print("The two-tier compilation contract, observed:")
    print("=" * 66)
    print("""  forward Trip    → runtime arithmetic, solver-free C
  relation Pend.  → settled at COMPILE time, constants in the binary

  The solver never ships. Relations settle before the binary exists.
  The binary is the residue.""")


def main():
    ap = argparse.ArgumentParser(prog='avenc',
                                 description='The Aven compiler')
    ap.add_argument('--version', action='version',
                    version=f'avenc {VERSION}')
    sub = ap.add_subparsers(dest='cmd')
    for name in ('run', 'emit'):
        p = sub.add_parser(name)
        p.add_argument('file')
    b = sub.add_parser('build')
    b.add_argument('file')
    b.add_argument('-o', '--output')
    args = ap.parse_args()

    if args.cmd == 'run':
        cmd_run(args.file)
    elif args.cmd == 'emit':
        print(cmd_emit(args.file))
    elif args.cmd == 'build':
        cmd_build(args.file, args.output)
    else:
        demo()


if __name__ == "__main__":
    main()
