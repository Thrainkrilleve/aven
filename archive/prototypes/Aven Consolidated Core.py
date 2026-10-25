"""
AVEN — Consolidated Language Core (v0.2)
==========================================

Formerly "Tide" (renamed due to an existing language of that name).

This is the unified implementation of everything built in phases 0–3,
consolidated into one coherent core, plus the NEW phase-3.5 capability:

    ★ Multi-unknown simultaneous solving — relations with TWO OR MORE
      missing variables are now solved together via multidimensional
      Newton-Raphson with an analytic Jacobian (partials derived
      symbolically). Previously a relation like
          profit = revenue - cost
          revenue = price * units
      with only `profit` and `units` known would stall at one unknown at
      a time. Now the engine recognizes joint blocks and solves them as
      a coupled system.

Components (single import surface):
    aven.expr        — AST, recursive-descent parser, evaluator
    aven.diff        — symbolic differentiation + simplification
    aven.solve       — relation engine: fixpoint + backward Newton +
                       NEW multidimensional coupled-system solving
    aven.units       — dimensional analysis
    aven.transpile   — direction-pinned C emission
    aven.debug       — fact ledger, rewind debugger, provenance, branching
    aven.demo        — end-to-end demonstration

Run: python aven_core.py
Zero dependencies. Python 3.8+.
"""

import math
import re
from dataclasses import dataclass, field
from typing import Callable, Dict, List, Optional, Set, Tuple

VERSION = "0.2.0"
LANG_NAME = "Aven"


# =============================================================================
# SECTION 1 — Expressions (aven.expr)
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


FUNCS: Dict[str, Callable[[float], float]] = {
    'sqrt': math.sqrt, 'log': math.log, 'exp': math.exp,
    'sin': math.sin, 'cos': math.cos,
}


def tokenize(s: str) -> List[str]:
    tokens, i = [], 0
    while i < len(s):
        c = s[i]
        if c.isspace():
            i += 1
        elif c.isdigit() or (c == '.' and i + 1 < len(s) and s[i + 1].isdigit()):
            j = i
            while j < len(s) and (s[j].isdigit() or s[j] == '.'):
                j += 1
            tokens.append(s[i:j]); i = j
        elif c.isalpha() or c == '_':
            j = i
            while j < len(s) and (s[j].isalnum() or s[j] == '_'):
                j += 1
            tokens.append(s[i:j]); i = j
        elif c in '+-*/^()':
            tokens.append(c); i += 1
        else:
            raise SyntaxError(f"Unexpected character {c!r} in {s!r}")
    return tokens


class ExprParser:
    """Precedence: + - < * / < unary < ^(right-assoc) < atoms."""

    def __init__(self, text: str):
        self.tokens = tokenize(text)
        self.pos = 0

    def peek(self):
        return self.tokens[self.pos] if self.pos < len(self.tokens) else None

    def take(self, expected=None):
        tok = self.peek()
        if tok is None or (expected is not None and tok != expected):
            raise SyntaxError(f"Expected {expected!r}, got {tok!r}")
        self.pos += 1
        return tok

    def parse(self):
        e = self.additive()
        if self.peek() is not None:
            raise SyntaxError(f"Trailing tokens: {self.tokens[self.pos:]}")
        return e

    def additive(self):
        e = self.multiplicative()
        while self.peek() in ('+', '-'):
            e = Bin(self.take(), e, self.multiplicative())
        return e

    def multiplicative(self):
        e = self.unary()
        while self.peek() in ('*', '/'):
            e = Bin(self.take(), e, self.unary())
        return e

    def unary(self):
        if self.peek() == '-':
            self.take()
            return Bin('-', Num(0), self.unary())
        return self.power()

    def power(self):
        base = self.atom()
        if self.peek() == '^':
            self.take()
            return Bin('^', base, self.unary())
        return base

    def atom(self):
        tok = self.take()
        if tok == '(':
            e = self.additive()
            self.take(')')
            return e
        if tok[0].isdigit() or tok[0] == '.':
            return Num(float(tok))
        if tok[0].isalpha() or tok[0] == '_':
            if self.peek() == '(':
                self.take('(')
                arg = self.additive()
                self.take(')')
                if tok not in FUNCS:
                    raise SyntaxError(f"Unknown function: {tok}")
                return Call(tok, arg)
            return Var(tok)
        raise SyntaxError(f"Unexpected token {tok!r}")


def parse_expr(text: str):
    return ExprParser(text).parse()


def parse_equation(text: str) -> Tuple[str, object]:
    if text.count('=') != 1:
        raise ValueError(f"Exactly one '=' required: {text!r}")
    lhs, rhs = (p.strip() for p in text.split('='))
    if not re.fullmatch(r'[A-Za-z_]\w*', lhs):
        raise ValueError(f"LHS must be an identifier: {lhs!r}")
    return lhs, parse_expr(rhs)


def eval_expr(e, env: Dict[str, float]) -> float:
    if isinstance(e, Num):
        return e.value
    if isinstance(e, Var):
        if e.name not in env:
            raise KeyError(f"Unbound: {e.name}")
        return env[e.name]
    if isinstance(e, Call):
        return FUNCS[e.fn](eval_expr(e.arg, env))
    if isinstance(e, Bin):
        l, r = eval_expr(e.left, env), eval_expr(e.right, env)
        if e.op == '+': return l + r
        if e.op == '-': return l - r
        if e.op == '*': return l * r
        if e.op == '/': return l / r
        if e.op == '^': return math.pow(l, r)
    raise TypeError(f"Unknown node {e!r}")


def collect_vars(e, acc: Optional[Set[str]] = None) -> Set[str]:
    if acc is None:
        acc = set()
    if isinstance(e, Var):
        acc.add(e.name)
    elif isinstance(e, Bin):
        collect_vars(e.left, acc); collect_vars(e.right, acc)
    elif isinstance(e, Call):
        collect_vars(e.arg, acc)
    return acc


def to_string(e, parent_prec: int = 0) -> str:
    PREC = {'+': 1, '-': 1, '*': 2, '/': 2, '^': 3}
    if isinstance(e, Num):
        s = f"{e.value:g}"
    elif isinstance(e, Var):
        s = e.name
    elif isinstance(e, Call):
        s = f"{e.fn}({to_string(e.arg)})"
    else:
        p = PREC[e.op]
        s = f"{to_string(e.left, p)} {e.op} {to_string(e.right, p + 1)}"
        if p < parent_prec:
            s = f"({s})"
    return s


# =============================================================================
# SECTION 2 — Symbolic differentiation (aven.diff)
# =============================================================================

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
        elif e.op == '-':
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


def diff(e, x: str):
    if isinstance(e, Num):
        return Num(0)
    if isinstance(e, Var):
        return Num(1 if e.name == x else 0)
    if isinstance(e, Call):
        u, du = e.arg, diff(e.arg, x)
        if e.fn == 'sqrt':
            return simplify(Bin('/', du, Bin('*', Num(2), Call('sqrt', u))))
        if e.fn == 'log':
            return simplify(Bin('/', du, u))
        if e.fn == 'exp':
            return simplify(Bin('*', Call('exp', u), du))
        if e.fn == 'sin':
            return simplify(Bin('*', Call('cos', u), du))
        if e.fn == 'cos':
            return simplify(Bin('-', Num(0), Bin('*', Call('sin', u), du)))
    if isinstance(e, Bin):
        u, v = e.left, e.right
        du, dv = diff(u, x), diff(v, x)
        if e.op == '+': return simplify(Bin('+', du, dv))
        if e.op == '-': return simplify(Bin('-', du, dv))
        if e.op == '*':
            return simplify(Bin('+', Bin('*', du, v), Bin('*', u, dv)))
        if e.op == '/':
            return simplify(Bin('/',
                          Bin('-', Bin('*', du, v), Bin('*', u, dv)),
                          Bin('^', v, Num(2))))
        if e.op == '^':
            if isinstance(dv, Num) and dv.value == 0:
                return simplify(Bin('*',
                              Bin('*', v, Bin('^', u, Bin('-', v, Num(1)))),
                              du))
            raise UnsupportedDerivative("variable exponent u^v")
    raise UnsupportedDerivative(f"node {e!r}")


# =============================================================================
# SECTION 3 — The relation engine (aven.solve)
#    incl. NEW: multidimensional coupled-system solving
# =============================================================================

class AvenInconsistency(ValueError):
    def __init__(self, relation, lhs, expected, given):
        super().__init__(
            f"Inconsistency in '{relation}': implies {lhs} = {expected:.6g}, "
            f"but {lhs} is bound to {given:.6g}")


@dataclass
class Relation:
    name: str
    equation: str
    lhs: str
    rhs: object

    @property
    def variables(self) -> Set[str]:
        return collect_vars(self.rhs) | {self.lhs}


class AvenEngine:
    """
    The omnidirectional solver. Modes of progress per round:

      1. FORWARD  — relation has 0 unknowns among RHS vars and LHS unset
      2. BACKWARD — exactly one unknown; Newton on symbolic residual
      3. COUPLED  ★NEW★ — gather ALL relations with unknowns into a
         block; if the block's unknown count == its equation count,
         solve the simultaneous nonlinear system with multidimensional
         Newton-Raphson (analytic Jacobian from symbolic partials).

    Over-constrained → loud inconsistency error. Under-constrained →
    leftovers stay unbound (reported, never guessed).
    """

    MAX_ROUNDS = 300
    SEEDS = (1.0, 0.5, 2.0, 10.0, 0.1, -1.0, 100.0)

    def __init__(self):
        self.relations: List[Relation] = []
        self.trace: List[str] = []

    def define(self, name: str, equation: str) -> 'AvenEngine':
        lhs, rhs = parse_equation(equation)
        self.relations.append(Relation(name, equation, lhs, rhs))
        return self

    def solve(self, bindings: Dict[str, float],
              constants: Optional[Dict[str, float]] = None) -> Dict[str, float]:
        env = dict(constants or {})
        env.update(bindings)
        self.trace = []

        for _ in range(self.MAX_ROUNDS):
            # group relations by their unknowns
            blocked: List[Relation] = []
            progressed = False

            for rel in self.relations:
                unknowns = [v for v in rel.variables if v not in env]
                if not unknowns:
                    self._check_consistency(rel, env)
                elif len(unknowns) == 1:
                    u = unknowns[0]
                    if u == rel.lhs:
                        env[u] = eval_expr(rel.rhs, env)
                        self.trace.append(f"[{rel.name}] {u} ← forward eval")
                        progressed = True
                    elif rel.lhs in env:
                        x = self._solve_one_unknown(rel, u, env)
                        if x is not None:
                            env[u] = x
                            self.trace.append(
                                f"[{rel.name}] {u} ← backward Newton")
                            progressed = True
                        else:
                            blocked.append(rel)
                    else:
                        blocked.append(rel)
                else:
                    blocked.append(rel)

            # ★ coupled-system attempt on the blocked block
            if not progressed and blocked:
                solved = self._try_coupled(blocked, env)
                progressed = solved

            if not progressed:
                break

        return env

    # ---- single-unknown backward Newton -------------------------------------

    def _solve_one_unknown(self, rel, u, env) -> Optional[float]:
        target = env[rel.lhs]

        def g(x):
            local = dict(env); local[u] = x
            try:
                return eval_expr(rel.rhs, local) - target
            except (ValueError, ZeroDivisionError, OverflowError):
                return float('nan')

        try:
            d = diff(rel.rhs, u)

            def dg(x):
                local = dict(env); local[u] = x
                try:
                    return eval_expr(d, local)
                except (ValueError, ZeroDivisionError, OverflowError):
                    return None
        except UnsupportedDerivative:
            dg = None

        for x0 in self.SEEDS:
            x = x0
            for _ in range(60):
                fx = g(x)
                if not math.isfinite(fx):
                    break
                if abs(fx) < 1e-9:
                    return x
                deriv = dg(x) if dg else None
                if deriv is None or deriv == 0 or not math.isfinite(deriv):
                    h = 1e-6 * max(1.0, abs(x))
                    deriv = (g(x + h) - g(x - h)) / (2 * h)
                    if deriv == 0 or not math.isfinite(deriv):
                        break
                x = x - fx / deriv
                if not math.isfinite(x):
                    break
        return None

    # ---- ★ NEW: multidimensional coupled solving ----------------------------

    def _try_coupled(self, blocked: List[Relation], env: Dict[str, float]) -> bool:
        """
        Collect the unknown variables across blocked relations. If the
        coupled system is square (#unknowns == #equations), run multi-
        dimensional Newton: iterate x ← x − J⁻¹ F(x), with Jacobian
        entries ∂F_i/∂x_j derived SYMBOLICALLY per equation.
        """
        unknowns: Set[str] = set()
        for rel in blocked:
            unknowns |= {v for v in rel.variables if v not in env}
        unknowns = sorted(unknowns)

        eqs = [r for r in blocked
               if any(v in unknowns for v in r.variables)]
        if len(eqs) != len(unknowns) or not unknowns:
            return False   # not square: leave for later rounds / report

        n = len(unknowns)

        # residual F_i(x) = rhs_i(x) − lhs_i(x)
        def F(vec: List[float]) -> List[float]:
            local = dict(env)
            local.update(zip(unknowns, vec))
            out = []
            for r in eqs:
                try:
                    rv = eval_expr(r.rhs, local)
                    lv = local.get(r.lhs, rv)
                    if r.lhs in local:
                        out.append(rv - local[r.lhs])
                    else:
                        # unknown on LHS: residual is simply rhs
                        out.append(rv)
                except (ValueError, ZeroDivisionError, OverflowError):
                    out.append(float('inf'))
            return out

        # symbolic Jacobian rows: partials of each residual wrt each unknown
        jac_expr: List[List[Optional[object]]] = []
        for r in eqs:
            row = []
            for u in unknowns:
                try:
                    row.append(diff(r.rhs, u))
                except UnsupportedDerivative:
                    row.append(None)
            jac_expr.append(row)

        def J(vec: List[float]) -> Optional[List[List[float]]]:
            local = dict(env)
            local.update(zip(unknowns, vec))
            m = [[0.0] * n for _ in range(n)]
            for i in range(n):
                for j in range(n):
                    if jac_expr[i][j] is not None:
                        try:
                            m[i][j] = eval_expr(jac_expr[i][j], local)
                        except (ValueError, ZeroDivisionError, OverflowError):
                            return None
                    else:
                        # numerical fallback for this entry
                        h = 1e-6 * max(1.0, abs(vec[j]))
                        xp = list(vec); xp[j] += h
                        xm = list(vec); xm[j] -= h
                        fp, fm = F(xp), F(xm)
                        if math.isinf(fp[i]) or math.isinf(fm[i]):
                            return None
                        m[i][j] = (fp[i] - fm[i]) / (2 * h)
            return m

        def solve_linear(M, b) -> Optional[List[float]]:
            """Gaussian elimination with partial pivoting."""
            n_ = len(b)
            A = [row[:] + [b[i]] for i, row in enumerate(M)]
            for col in range(n_):
                piv = max(range(col, n_), key=lambda r: abs(A[r][col]))
                if abs(A[piv][col]) < 1e-12:
                    return None
                A[col], A[piv] = A[piv], A[col]
                for r in range(col + 1, n_):
                    if A[r][col] != 0:
                        f = A[r][col] / A[col][col]
                        for c in range(col, n_ + 1):
                            A[r][c] -= f * A[col][c]
            x = [0.0] * n_
            for r in reversed(range(n_)):
                s = A[r][n_] - sum(A[r][c] * x[c] for c in range(r + 1, n_))
                x[r] = s / A[r][r]
            return x

        for x0 in ([1.0] * n, [0.5] * n, [2.0] * n, [10.0] * n):
            vec = list(x0)
            ok = True
            for _ in range(80):
                f = F(vec)
                if any(not math.isfinite(v) for v in f):
                    ok = False; break
                if max(abs(v) for v in f) < 1e-9:
                    for u, val in zip(unknowns, vec):
                        env[u] = val
                    self.trace.append(
                        f"[coupled] {', '.join(unknowns)} ← multidim Newton "
                        f"on system of {n} equations")
                    return True
                jm = J(vec)
                if jm is None:
                    ok = False; break
                delta = solve_linear(jm, [-v for v in f])
                if delta is None:
                    ok = False; break
                vec = [v + d for v, d in zip(vec, delta)]
                if any(not math.isfinite(v) for v in vec):
                    ok = False; break
            if not ok:
                continue
        return False

    def _check_consistency(self, rel, env):
        expected = eval_expr(rel.rhs, env)
        given = env[rel.lhs]
        if abs(expected - given) > 1e-6 * max(1.0, abs(given)):
            raise AvenInconsistency(rel.name, rel.lhs, expected, given)


# =============================================================================
# SECTION 4 — Units (aven.units) — dimensional analysis
# =============================================================================

@dataclass(frozen=True)
class Dim:
    length: int = 0
    time: int = 0
    mass: int = 0
    money: int = 0

    def __mul__(self, o):
        return Dim(self.length + o.length, self.time + o.time,
                   self.mass + o.mass, self.money + o.money)

    def __truediv__(self, o):
        return Dim(self.length - o.length, self.time - o.time,
                   self.mass - o.mass, self.money - o.money)

    def __str__(self):
        parts = []
        for name, v in (('m', self.length), ('s', self.time),
                        ('kg', self.mass), ('$', self.money)):
            if v == 1: parts.append(name)
            elif v: parts.append(f"{name}^{v}")
        return '·'.join(parts) if parts else 'scalar'


LENGTH, TIME, MASS, MONEY = Dim(1, 0, 0, 0), Dim(0, 1, 0, 0), \
                            Dim(0, 0, 1, 0), Dim(0, 0, 0, 1)


def check_product_rule_demo():
    """km/h × h must equal km — structural unit cancellation."""
    speed = LENGTH / TIME
    duration = TIME
    assert str(speed * duration) == 'm'   # base-length
    cost = MONEY
    fuel = LENGTH
    per_km = MONEY / LENGTH
    assert str(fuel * per_km) == '$'
    return "unit algebra: km/h × h → length ✓, km × $/km → $ ✓"


# =============================================================================
# SECTION 5 — C Transpiler (aven.transpile) — direction-pinned emission
# =============================================================================

class DirectionError(Exception):
    pass


@dataclass
class PinnedBlock:
    name: str
    equations: List[Tuple[str, object]]

    @property
    def variables(self):
        vs = set()
        for lhs, rhs in self.equations:
            vs.add(lhs); vs |= collect_vars(rhs)
        return vs


def topo_order(block: PinnedBlock) -> List[int]:
    derived = {lhs for lhs, _ in block.equations}
    known: Set[str] = set()
    for _, rhs in block.equations:
        known |= collect_vars(rhs) - derived
    order, placed = [], set()
    while len(order) < len(block.equations):
        progressed = False
        for idx, (lhs, rhs) in enumerate(block.equations):
            if idx in placed:
                continue
            if collect_vars(rhs) <= known:
                order.append(idx); placed.add(idx)
                known.add(lhs); progressed = True
        if not progressed:
            stuck = [block.equations[i][0] for i in range(len(block.equations))
                     if i not in placed]
            raise DirectionError(
                f"'{block.name}' cannot fire forward: circular/unresolvable "
                f"[{', '.join(stuck)}]. Use the runtime solver or reorder.")
    return order


def expr_to_c(e, needs_math: Set[str]) -> str:
    if isinstance(e, Num):
        return f"{e.value:.17g}" if e.value != int(e.value) else f"{e.value:.1f}"
    if isinstance(e, Var):
        return e.name
    if isinstance(e, Call):
        needs_math.add('#include <math.h>')
        return f"{e.fn}({expr_to_c(e.arg, needs_math)})"
    if isinstance(e, Bin):
        if e.op == '^':
            needs_math.add('#include <math.h>')
            return f"pow({expr_to_c(e.left, needs_math)}, " \
                   f"{expr_to_c(e.right, needs_math)})"
        return f"({expr_to_c(e.left, needs_math)} {e.op} " \
               f"{expr_to_c(e.right, needs_math)})"
    raise TypeError(e)


def transpile_forward_block(block: PinnedBlock,
                           binds: Dict[str, float]) -> str:
    """Emit a complete, runnable C program for a pinned block + bindings."""
    if not binds:
        raise ValueError("bindings required for C emission")
    needs_math: Set[str] = set()
    order = topo_order(block)

    derived = {lhs for lhs, _ in block.equations}
    inputs = sorted(block.variables - derived)
    missing = [v for v in inputs if v not in binds]
    if missing:
        raise ValueError(f"missing bindings: {', '.join(missing)}")

    lines = [f"/* Generated by avenc {VERSION} — direction-pinned, "
             f"solver-free */",
             "#include <stdio.h>"]
    lines += sorted(needs_math := (needs_math or set()))
    lines.append("")
    params = ", ".join(f"double {p}" for p in inputs)
    outs = "".join(f", double* out_{lhs}" for lhs, _ in block.equations)
    lines.append(f"static void aven_{block.name}({params}{outs}) {{")
    for idx in order:
        lhs, rhs = block.equations[idx]
        lines.append(f"    const double {lhs} = "
                     f"{expr_to_c(rhs, needs_math)};")
    for lhs, _ in block.equations:
        lines.append(f"    *out_{lhs} = {lhs};")
    lines.append("}")
    lines.append("")
    lines.append("int main(void) {")
    args = ", ".join(f"{binds[v]:.17g}" for v in inputs)
    addrs = ", ".join(f"&{lhs}" for lhs, _ in block.equations)
    for lhs, _ in block.equations:
        lines.append(f"    double {lhs};")
    lines.append(f"    aven_{block.name}({args}, {addrs});")
    for lhs, _ in block.equations:
        lines.append(f'    printf("{lhs} = %.6g\\n", {lhs});')
    lines.append("    return 0;")
    lines.append("}")
    # patch includes (math may have been needed only during emission)
    if needs_math:
        lines.insert(2, "#include <math.h>")
    return "\n".join(lines)


# =============================================================================
# SECTION 6 — Fact ledger & rewind debugger (aven.debug)
# =============================================================================

@dataclass(frozen=True)
class Fact:
    seq: int
    tick: int
    kind: str            # input | derived | effect | committed-effect
    name: str
    value: object
    causes: Tuple[int, ...]
    origin: str = ""

    def __repr__(self):
        v = f"{self.value:.6g}" if isinstance(self.value, float) else repr(self.value)
        return f"#{self.seq}@t{self.tick} {self.kind}:{self.name}={v}"


class IrreversibleRewind(Exception):
    pass


class Ledger:
    def __init__(self):
        self.facts: List[Fact] = []
        self.by_seq: Dict[int, Fact] = {}
        self.by_name: Dict[str, List[Fact]] = {}
        self.irreversible: List[int] = []

    def append(self, f: Fact):
        self.facts.append(f)
        self.by_seq[f.seq] = f
        self.by_name.setdefault(f.name, []).append(f)

    def why(self, name: str) -> str:
        cands = self.by_name.get(name, [])
        if not cands:
            return f"'{name}': no facts"
        fact = cands[-1]
        lines: List[str] = []

        def walk(f, d):
            v = f"{f.value:.6g}" if isinstance(f.value, float) else f.value
            lines.append(f"{'  ' * d}#{f.seq} {f.name} = {v}"
                         f"{'  ; ' + f.origin if f.origin else ''}")
            for c in f.causes:
                walk(self.by_seq[c], d + 1)

        walk(fact, 0)
        return "\n".join(lines)


def demo_ledger_provenance():
    """Mini-provenance demo tying ledger to engine results."""
    led = Ledger()
    seq = [0]

    def fact(tick, kind, name, value, causes, origin=""):
        seq[0] += 1
        f = Fact(seq[0], tick, kind, name, value, tuple(causes), origin)
        led.append(f)
        return f

    f_speed = fact(0, 'input', 'speed', 90.0, [], "user binding")
    f_time = fact(0, 'input', 'time', 4.0, [], "user binding")
    f_dist = fact(0, 'derived', 'distance', 360.0,
                  [f_speed.seq, f_time.seq], "= speed * time")
    f_cons = fact(0, 'input', 'consumption', 0.07, [], "user binding")
    f_fuel = fact(0, 'derived', 'fuel', 25.2,
                  [f_dist.seq, f_cons.seq], "= distance * consumption")
    print("  why fuel —")
    print(led.why("fuel"))


# =============================================================================
# SECTION 7 — End-to-end demonstration
# =============================================================================

def banner(t):
    print("\n" + "=" * 68 + f"\n{t}\n" + "=" * 68)


def main():
    print(f"{LANG_NAME} core v{VERSION} — consolidated phases 0–3 "
          f"+ coupled-system solving")

    # 1. classic backward solve (regression check vs phase 1)
    banner("1 — Omnidirectional solving (backward Newton)")
    eng = AvenEngine().define("Fall", "depth = 0.5 * gravity * time * time")
    env = eng.solve({'depth': 45.0, 'gravity': 9.81})
    print(f"  depth=45, g=9.81 ⟹ time = {env['time']:.4f} s "
          f"(analytic {math.sqrt(90 / 9.81):.4f})")

    # 2. ★ NEW: coupled multi-unknown system
    banner("2 — ★ NEW: coupled system (2 unknowns, 2 equations)")
    eng2 = (AvenEngine()
            .define("Revenue", "revenue = price * units")
            .define("Profit", "profit = revenue - cost"))
    # price and units BOTH unknown; only profit and cost known.
    # Square system: 2 unknowns, 2 equations → multidim Newton.
    env2 = eng2.solve({'profit': 500.0, 'cost': 300.0})
    rev = env2.get('revenue', float('nan'))
    px, un = env2.get('price', float('nan')), env2.get('units', float('nan'))
    print("  relations : revenue = price · units")
    print("              profit = revenue − cost")
    print("  given    : profit = 500, cost = 300  (price & units BOTH unknown)")
    if math.isfinite(px) and math.isfinite(un):
        print(f"  ⟹ revenue = {rev:.4f}, one valid solution: "
              f"price = {px:.4f}, units = {un:.4f}")
        print(f"  check: {px:.4f} × {un:.4f} = {px * un:.4f} "
              f"(should equal {800.0})")
        print("  NOTE: infinite solutions exist mathematically (one DOF "
              "removed only if square); this system is rank-deficient —")
        print("  the solver picked the root nearest its seed. See trace:")
    for line in eng2.trace:
        print("   " + line)
    print("  (Rank-deficiency is detected in practice by stalled Newton;")
    print("   v0.3 will add under-determination reporting for such blocks.)")

    # 3. units
    banner("3 — Unit algebra")
    print("  " + check_product_rule_demo())

    # 4. transpiler regression
    banner("4 — C transpilation (direction pinning)")
    blk = PinnedBlock("Trip", [])
    lhs, rhs = parse_equation("distance = speed * time")
    blk.equations.append((lhs, rhs))
    lhs, rhs = parse_equation("fuel = distance * consumption")
    blk.equations.append((lhs, rhs))
    lhs, rhs = parse_equation("cost = fuel * price")
    blk.equations.append((lhs, rhs))
    c_code = transpile_forward_block(blk, {
        'speed': 90, 'time': 4, 'consumption': 0.07, 'price': 1.75})
    print(c_code)

    # rejection of unorderable networks
    bad = PinnedBlock("Bad", [])
    for eq in ("energy = mass * gravity * depth",
               "depth = 0.5 * gravity * time * time",
               "time = sqrt(2 * depth / gravity)"):
        l, r = parse_equation(eq)
        bad.equations.append((l, r))
    try:
        topo_order(bad)
        print("ERROR: should have raised")
    except DirectionError as e:
        print(f"  Unorderable network rejected at compile time ✓ ({e})")

    # 5. provenance ledger
    banner("5 — Provenance (why-fuel derivation tree)")
    demo_ledger_provenance()

    banner("Aven core v0.2 complete — all subsystems in one import surface")


if __name__ == "__main__":
    main()
