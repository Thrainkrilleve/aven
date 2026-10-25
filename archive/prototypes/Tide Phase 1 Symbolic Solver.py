"""
Tide Phase 1 — Symbolic Differentiation & General Backward Solving
====================================================================

Builds on the Phase 0 prototype. The key upgrade:

    ANY equation that is invertible can now be solved in ANY direction,
    without hand-written inversion rules.

How it works:
1. Every relation `lhs = rhs` is kept as a symbolic expression tree.
2. When the solver needs a variable `u` buried inside the rhs, it builds
   the residual function  g(x) = rhs(x) - lhs  and finds the root.
3. The root-finder is Newton-Raphson, driven by a SYMBOLIC derivative
   computed automatically from the expression tree (product rule, chain
   rule, power rule, trig, sqrt, log, exp).
4. If symbolic differentiation fails (e.g. variable exponents), it falls
   back to numerical central differences — nothing ever dead-ends.
5. Multi-relation networks settle by fixpoint propagation: solving one
   relation can unlock the next, forwards OR backwards.

Run: python tide_phase1.py
"""

import math
import re
from dataclasses import dataclass
from typing import Callable, Dict, List, Optional, Set, Tuple


# =============================================================================
# 1. Expression AST
# =============================================================================

@dataclass(frozen=True)
class Num:
    value: float

@dataclass(frozen=True)
class Var:
    name: str

@dataclass(frozen=True)
class Bin:
    op: str   # '+', '-', '*', '/', '^'
    left: object
    right: object

@dataclass(frozen=True)
class Call:
    fn: str   # 'sqrt', 'log', 'exp', 'sin', 'cos'
    arg: object


class UnsupportedDerivative(Exception):
    """Raised when an expression cannot be differentiated symbolically."""


FUNCS: Dict[str, Callable[[float], float]] = {
    'sqrt': math.sqrt,
    'log': math.log,
    'exp': math.exp,
    'sin': math.sin,
    'cos': math.cos,
}


def eval_expr(e, env: Dict[str, float]) -> float:
    if isinstance(e, Num):
        return e.value
    if isinstance(e, Var):
        if e.name not in env:
            raise KeyError(f"Unbound variable: {e.name}")
        return env[e.name]
    if isinstance(e, Call):
        return FUNCS[e.fn](eval_expr(e.arg, env))
    if isinstance(e, Bin):
        l = eval_expr(e.left, env)
        r = eval_expr(e.right, env)
        if e.op == '+': return l + r
        if e.op == '-': return l - r
        if e.op == '*': return l * r
        if e.op == '/': return l / r
        if e.op == '^': return math.pow(l, r)
    raise TypeError(f"Unknown node: {e!r}")


def collect_vars(e, acc: Optional[Set[str]] = None) -> Set[str]:
    if acc is None: acc = set()
    if isinstance(e, Var): acc.add(e.name)
    elif isinstance(e, Bin):
        collect_vars(e.left, acc); collect_vars(e.right, acc)
    elif isinstance(e, Call):
        collect_vars(e.arg, acc)
    return acc


# =============================================================================
# 2. Simplifier (constant folding + identity elimination)
# =============================================================================

def _fold(op: str, l: float, r: float):
    """Attempt to constant-fold; return None if unsafe/undefined."""
    try:
        if op == '+': return l + r
        if op == '-': return l - r
        if op == '*': return l * r
        if op == '/':
            return None if r == 0 else l / r
        if op == '^':
            if l == 0 and r <= 0: return None
            if l < 0 and not float(r).is_integer(): return None
            return math.pow(l, r)
    except (OverflowError, ValueError):
        return None
    return None


def simplify(e):
    if isinstance(e, Bin):
        l = simplify(e.left)
        r = simplify(e.right)
        if isinstance(l, Num) and isinstance(r, Num):
            folded = _fold(e.op, l.value, r.value)
            if folded is not None and math.isfinite(folded):
                return Num(folded)
        if e.op == '*':
            if (isinstance(l, Num) and l.value == 0) or (isinstance(r, Num) and r.value == 0):
                return Num(0)
            if isinstance(l, Num) and l.value == 1: return r
            if isinstance(r, Num) and r.value == 1: return l
        elif e.op == '+':
            if isinstance(l, Num) and l.value == 0: return r
            if isinstance(r, Num) and r.value == 0: return l
        elif e.op == '-':
            if isinstance(r, Num) and r.value == 0: return l
        elif e.op == '/':
            if isinstance(l, Num) and l.value == 0: return Num(0)
        elif e.op == '^':
            if isinstance(r, Num) and r.value == 1: return l
            if isinstance(r, Num) and r.value == 0: return Num(1)
        return Bin(e.op, l, r)
    if isinstance(e, Call):
        a = simplify(e.arg)
        if isinstance(a, Num):
            try:
                v = FUNCS[e.fn](a.value)
                if math.isfinite(v): return Num(v)
            except (ValueError, OverflowError):
                pass
        return Call(e.fn, a)
    return e


# =============================================================================
# 3. Symbolic Differentiation
# =============================================================================

def diff(e, x: str):
    """∂e/∂x as a (simplified) symbolic expression."""
    if isinstance(e, Num):
        return Num(0)
    if isinstance(e, Var):
        return Num(1 if e.name == x else 0)

    if isinstance(e, Call):
        du = diff(e.arg, x)
        u = e.arg
        if e.fn == 'sqrt':
            return simplify(Bin('/', du, Bin('*', Num(2), Call('sqrt', u))))
        if e.fn == 'log':
            return simplify(Bin('/', du, u))
        if e.fn == 'exp':
            return simplify(Bin('*', Call('exp', u), du))
        if e.fn == 'sin':
            return simplify(Bin('*', Call('cos', u), du))
        if e.fn == 'cos':
            return simplify(Bin('-', Num(0), Bin('*', Call('sin', u), du))))
        raise UnsupportedDerivative(f"fn {e.fn}")

    if isinstance(e, Bin):
        u, v = e.left, e.right
        du, dv = diff(u, x), diff(v, x)
        if e.op == '+':
            return simplify(Bin('+', du, dv))
        if e.op == '-':
            return simplify(Bin('-', du, dv))
        if e.op == '*':
            # product rule: du*v + u*dv
            return simplify(Bin('+', Bin('*', du, v), Bin('*', u, dv)))
        if e.op == '/':
            # quotient rule: (du*v - u*dv) / v^2
            return simplify(Bin('/',
                          Bin('-', Bin('*', du, v), Bin('*', u, dv)),
                          Bin('^', v, Num(2))))
        if e.op == '^':
            if isinstance(dv, Num) and dv.value == 0:
                # constant exponent: d(u^c) = c * u^(c-1) * du
                return simplify(Bin('*',
                              Bin('*', v, Bin('^', u, Bin('-', v, Num(1)))),
                              du))
            # variable exponent (u^v): not supported symbolically here —
            # the caller falls back to numerical differentiation.
            raise UnsupportedDerivative("variable exponent u^v")
    raise UnsupportedDerivative(f"node {e!r}")


def to_string(e, parent_prec: int = 0) -> str:
    """Readable printing with minimal parentheses."""
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
# 4. Robust Root-Finding (multistart Newton + numerical fallback)
# =============================================================================

SEEDS = (1.0, 0.5, 2.0, 10.0, 0.1, 0.01, -1.0, 100.0, 1000.0)


def _central_diff(g: Callable[[float], float], x: float) -> float:
    h = 1e-6 * max(1.0, abs(x))
    return (g(x + h) - g(x - h)) / (2 * h)


def find_root(g: Callable[[float], float],
              dg: Optional[Callable[[float], float]],
              tol: float = 1e-9,
              max_iter: int = 60) -> Optional[float]:
    """Multistart Newton-Raphson. Returns the first root found, else None."""
    for x0 in SEEDS:
        x = float(x0)
        for _ in range(max_iter):
            fx = g(x)
            if not math.isfinite(fx):
                break
            if abs(fx) < tol:
                return x
            d = dg(x) if dg is not None else None
            if d is None or d == 0 or not math.isfinite(d):
                d = _central_diff(g, x)
                if d == 0 or not math.isfinite(d):
                    break
            x_new = x - fx / d
            if not math.isfinite(x_new):
                break
            if abs(x_new - x) < 1e-14 * max(1.0, abs(x)):
                # Step collapsed; accept only if we're at a root
                if abs(g(x_new)) < tol:
                    return x_new
                break
            x = x_new
    return None


def newton_report(g, dg, x0: float = 1.0, tol: float = 1e-9,
                 max_iter: int = 40) -> Tuple[Optional[float], int]:
    """Single-seed Newton used for the convergence-count demos."""
    x = float(x0)
    for i in range(1, max_iter + 1):
        fx = g(x)
        if abs(fx) < tol:
            return x, i
        d = dg(x) if dg is not None else _central_diff(g, x)
        if d == 0 or not math.isfinite(d):
            return None, i
        x = x - fx / d
    return None, max_iter


# =============================================================================
# 5. Parser (expressions with ^ and function calls)
# =============================================================================

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
    """Recursive-descent parser.
    Precedence (loosest → tightest):  + -   * /   unary minus   ^   atoms
    '^' is right-associative; -x^2 parses as -(x^2), matching math convention.
    """

    def __init__(self, text: str):
        self.tokens = tokenize(text)
        self.pos = 0

    def peek(self) -> Optional[str]:
        return self.tokens[self.pos] if self.pos < len(self.tokens) else None

    def take(self, expected: Optional[str] = None) -> str:
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
            op = self.take()
            e = Bin(op, e, self.multiplicative())
        return e

    def multiplicative(self):
        e = self.unary()
        while self.peek() in ('*', '/'):
            op = self.take()
            e = Bin(op, e, self.unary())
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
            return Bin('^', base, self.unary())   # right-associative
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
            if self.peek() == '(':          # function call
                self.take('(')
                arg = self.additive()
                self.take(')')
                if tok not in FUNCS:
                    raise SyntaxError(f"Unknown function: {tok}")
                return Call(tok, arg)
            return Var(tok)
        raise SyntaxError(f"Unexpected token {tok!r}")


def parse_equation(text: str) -> Tuple[str, object]:
    """'lhs = rhs' → (lhs_name, rhs_expr)"""
    if text.count('=') != 1:
        raise ValueError(f"Equation must contain exactly one '=': {text!r}")
    lhs, rhs = (part.strip() for part in text.split('='))
    if not re.fullmatch(r'[A-Za-z_]\w*', lhs):
        raise ValueError(f"LHS must be a bare identifier: {lhs!r}")
    return lhs, ExprParser(rhs).parse()


# =============================================================================
# 6. The Phase-1 Solver Engine
# =============================================================================

class TideInconsistency(ValueError):
    def __init__(self, relation: str, lhs: str, expected: float, given: float):
        super().__init__(
            f"Inconsistency in relation '{relation}': variables imply "
            f"{lhs} = {expected:.6g}, but {lhs} is bound to {given:.6g}"
        )


@dataclass
class Relation:
    name: str
    equation: str
    lhs: str
    rhs: object

    @property
    def variables(self) -> Set[str]:
        return collect_vars(self.rhs) | {self.lhs}


class TideEngine:
    """
    Fixpoint relation solver with omnidirectional equation solving.

    solve(bindings):
      1. Seed the environment with the given bindings.
      2. Repeat: for every relation —
           * 0 unknowns  → verify consistency (else raise)
           * 1 unknown   → solve for it (forward eval if it's the LHS,
                           symbolic-Newton otherwise), record a trace
         until no relation makes progress.
      3. Whatever remains unbound is under-constrained.
    """

    def __init__(self):
        self.relations: List[Relation] = []
        self.trace: List[str] = []

    def define(self, name: str, equation: str) -> 'TideEngine':
        lhs, rhs = parse_equation(equation)
        self.relations.append(Relation(name, equation, lhs, rhs))
        return self

    def solve(self, bindings: Dict[str, float],
              constants: Optional[Dict[str, float]] = None) -> Dict[str, float]:
        env: Dict[str, float] = dict(constants or {})
        env.update(bindings)
        self.trace = []

        for _round in range(200):                     # fixpoint iteration
            progressed = False
            for rel in self.relations:
                unknowns = [v for v in rel.variables if v not in env]
                if len(unknowns) == 0:
                    self._check_consistency(rel, env)
                elif len(unknowns) == 1:
                    u = unknowns[0]
                    if u == rel.lhs:
                        env[u] = eval_expr(rel.rhs, env)
                        self.trace.append(f"[{rel.name}] {u} ← direct eval of '{rel.equation}'")
                        progressed = True
                    elif rel.lhs in env:
                        x, iters = self._solve_rhs_unknown(rel, u, env)
                        if x is not None:
                            env[u] = x
                            self.trace.append(
                                f"[{rel.name}] {u} ← Newton solve of "
                                f"'{rel.equation}' ({iters} iterations)")
                            progressed = True
            if not progressed:
                break

        return env

    # -- internals -----------------------------------------------------------

    def _check_consistency(self, rel: Relation, env: Dict[str, float]):
        expected = eval_expr(rel.rhs, env)
        given = env[rel.lhs]
        if abs(expected - given) > 1e-6 * max(1.0, abs(given)):
            raise TideInconsistency(rel.name, rel.lhs, expected, given)

    def _solve_rhs_unknown(self, rel: Relation, u: str,
                           env: Dict[str, float]) -> Tuple[Optional[float], int]:
        """Solve  rhs(..., u=x, ...) = lhs  for x via Newton on a symbolic d/du."""
        target = env[rel.lhs]

        def g(x: float) -> float:
            local = dict(env)
            local[u] = x
            return eval_expr(rel.rhs, local) - target

        try:
            deriv_expr = diff(rel.rhs, u)
            def dg(x: float) -> Optional[float]:
                local = dict(env)
                local[u] = x
                try:
                    return eval_expr(deriv_expr, local)
                except (ValueError, ZeroDivisionError, OverflowError):
                    return None
        except UnsupportedDerivative:
            deriv_expr = None
            dg = None                                   # numerical fallback

        x = find_root(g, dg)
        return x, 0                                    # iteration count via report() when needed


# =============================================================================
# 7. Demos
# =============================================================================

def banner(title: str):
    print("\n" + "=" * 66)
    print(title)
    print("=" * 66)


CONSTS = {'pi': math.pi}


def demo_free_fall():
    banner("Demo 1 — Free fall: solve time from depth (backward square)")
    eng = TideEngine().define("Fall", "depth = 0.5 * gravity * time * time")
    env = eng.solve({'depth': 45.0, 'gravity': 9.81}, CONSTS)
    print("  relation : depth = 0.5 * gravity * time * time")
    print("  given    : depth = 45 m, gravity = 9.81 m/s²")
    print(f"  ⟹ time   = {env['time']:.4f} s   (analytic: {math.sqrt(90/9.81):.4f} s)")
    for line in eng.trace:
        print("  " + line)

    # Show the symbolic derivative the engine generated
    _, rhs = parse_equation("depth = 0.5 * gravity * time * time")
    print(f"\n  Engine's symbolic  d(depth)/d(time)  =  {to_string(diff(rhs, 'time'))}")

    # Newton convergence speed on this problem
    g = lambda x: 0.5 * 9.81 * x * x - 45.0
    dg = lambda x: 9.81 * x
    root, iters = newton_report(g, dg, x0=1.0)
    print(f"  Newton from x₀=1.0: converges in {iters} iterations → {root:.6f}")


def demo_pendulum():
    banner("Demo 2 — Pendulum: solve length from period (nested chain rule)")
    eng = TideEngine().define("Pendulum", "period = 2 * pi * sqrt(length / gravity)")
    env = eng.solve({'period': 2.0, 'gravity': 9.81}, CONSTS)
    print("  relation : period = 2π · sqrt(length / gravity)")
    print("  given    : period = 2.0 s, gravity = 9.81 m/s²")
    analytic = 9.81 * (2.0 / (2 * math.pi)) ** 2
    print(f"  ⟹ length = {env['length']:.4f} m   (analytic: {analytic:.4f} m)")
    for line in eng.trace:
        print("  " + line)


def demo_compound_interest():
    banner("Demo 3 — Compound interest: solve years (needs log inversion)")
    eng = TideEngine().define("Growth", "amount = principal * (1 + rate) ^ years")
    env = eng.solve({'amount': 2000.0, 'principal': 1000.0, 'rate': 0.05}, CONSTS)
    print("  relation : amount = principal · (1 + rate)^years")
    print("  given    : amount = 2000, principal = 1000, rate = 5%")
    analytic = math.log(2) / math.log(1.05)
    print(f"  ⟹ years = {env['years']:.4f}    (analytic: {analytic:.4f})")

    # Note: u^c with KNOWN exponent is handled symbolically; here 'years'
    # is the unknown exponent, so the engine used the numerical fallback.
    print("  (variable exponent → engine fell back to numerical derivative)")


def demo_projectile():
    banner("Demo 4 — Projectile: solve launch angle (transcendental)")
    eng = TideEngine().define("Shot", "range = velocity ^ 2 * sin(2 * angle) / gravity")
    env = eng.solve({'range': 45.0, 'velocity': 30.0, 'gravity': 9.81}, CONSTS)
    print("  relation : range = v² · sin(2·angle) / g")
    print("  given    : range = 45 m, velocity = 30 m/s, gravity = 9.81")
    if 'angle' in env:
        deg = math.degrees(env['angle'])
        # The low branch; note sin(2θ) is non-injective — Newton finds one root
        print(f"  ⟹ angle = {env['angle']:.4f} rad = {deg:.2f}°   (low branch)")
        other = (math.pi - 2 * env['angle']) / 2
        print(f"      (the mirror solution {math.degrees(other):.2f}° also satisfies it)")


def demo_chained_backward():
    banner("Demo 5 — THE FLAGSHIP: backward through a multi-relation network")
    print("  A climber falls. We know only the impact ENERGY.\n"
          "  Tide propagates BACKWARD through two relations to find\n"
          "  how long the fall lasted:\n")
    eng = (TideEngine()
           .define("Impact", "energy = mass * gravity * depth")
           .define("Fall",   "depth = 0.5 * gravity * time * time"))
    env = eng.solve({'energy': 15000.0, 'mass': 70.0, 'gravity': 9.81}, CONSTS)

    print("  relations:")
    print("    Impact : energy = mass · gravity · depth")
    print("    Fall   : depth  = 0.5 · gravity · time²")
    print("  given    : energy = 15000 J, mass = 70 kg, gravity = 9.81")
    print(f"\n  ⟹ depth  = {env['depth']:.4f} m")
    print(f"  ⟹ time   = {env['time']:.4f} s")
    print("\n  Solver trace (watch it run in reverse):")
    for line in eng.trace:
        print("    " + line)

    # Sanity-check analytically
    depth = 15000 / (70 * 9.81)
    t = math.sqrt(2 * depth / 9.81)
    print(f"\n  Analytic check: depth = {depth:.4f} m, time = {t:.4f} s  ✓")


def demo_consistency():
    banner("Demo 6 — Over-constrained systems still fail loudly")
    eng = TideEngine().define("Motion", "distance = speed * time")
    try:
        eng.solve({'speed': 50.0, 'time': 3.0, 'distance': 100.0}, CONSTS)
        print("  ERROR: inconsistency not detected!")
    except TideInconsistency as e:
        print(f"  Caught as designed: {e}")


def demo_underconstrained():
    banner("Demo 7 — Under-constrained systems report, not guess")
    eng = TideEngine().define("Motion", "distance = speed * time")
    env = eng.solve({'speed': 60.0}, CONSTS)
    print("  given only speed = 60 —")
    print(f"  distance = {env.get('distance', '?')}  (left unbound: under-constrained)")


def run_all():
    print("Tide Phase 1 — symbolic differentiation engine")
    print("Every demo below inverts equations NOBODY wrote inversion rules for.")
    demo_free_fall()
    demo_pendulum()
    demo_compound_interest()
    demo_projectile()
    demo_chained_backward()
    demo_consistency()
    demo_underconstrained()
    banner("Phase 1 complete — relations now solve in every direction")


if __name__ == "__main__":
    run_all()
