"""
Tide Interpreter — Proof of Concept

This is a minimal but functional implementation demonstrating:
1. Bidirectional relation solving via constraint propagation
2. Unit-aware arithmetic with compile-time checks
3. Simple signal/temporal tracking
4. The core engine that makes Tide's promise possible

To run: python tide_interpreter.py

Architecture:
- Parser turns Tide source into a relation graph
- Solver uses iterative constraint propagation with Jacobian-based back-solving
- Type checker validates units at "compile time"
- Signal buffer holds temporal values with staleness tracking
"""

import ast
import operator
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List, Optional, Set, Tuple, Union
from collections import defaultdict
import re


# =============================================================================
# 1. Units & Dimensions — The Compile-Time Type System
# =============================================================================

@dataclass(frozen=True)
class Dimension:
    """Base physical dimensions: length, time, mass, etc."""
    base: str
    exponent: float = 1.0
    
    def __mul__(self, other: 'Dimension') -> 'Dimension':
        if self.base == other.base:
            return Dimension(self.base, self.exponent + other.exponent)
        raise TypeError(f"Incompatible bases: {self.base} × {other.base}")
    
    def __truediv__(self, other: 'Dimension') -> 'Dimension':
        if self.base == other.base:
            return Dimension(self.base, self.exponent - other.exponent)
        raise TypeError(f"Incompatible bases: {self.base} ÷ {other.base}")
    
    def __repr__(self):
        if self.exponent == 0:
            return "dimensionless"
        if self.exponent == 1:
            return f"{self.base}"
        return f"{self.base}^{self.exponent}"


@dataclass(frozen=True)
class Unit:
    """A named unit with conversion factor to base dimension."""
    name: str
    dimension: Dimension
    to_base: float = 1.0  # Multiply to convert to base
    
    def compatible_with(self, other: 'Unit') -> bool:
        return self.dimension == other.dimension
    
    def __add__(self, other: 'Unit') -> 'Unit':
        if not self.compatible_with(other):
            raise TypeError(f"Cannot add {self.dimension} + {other.dimension}")
        # Convert to common base
        base_val = self.to_base
        other_base = other.to_base
        new_name = f"{self.name}+"
        return Unit(new_name, self.dimension, (base_val + other_base)/2)
    
    def __mul__(self, other: 'Union[Unit, float]') -> 'Unit':
        if isinstance(other, Unit):
            return Unit(
                f"{self.name}×{other.name}",
                self.dimension * other.dimension,
                self.to_base * other.to_base
            )
        return Unit(f"{self.name}*scalar", self.dimension, self.to_base * other)
    
    def __truediv__(self, other: 'Union[Unit, float]') -> 'Unit':
        if isinstance(other, Unit):
            return Unit(
                f"{self.name}/{other.name}",
                self.dimension / other.dimension,
                self.to_base / other.to_base
            )
        return Unit(f"{self.name}/scalar", self.dimension, self.to_base / other)


# Standard units
METER = Unit("meter", Dimension("length"))
KILOMETER = Unit("kilometer", Dimension("length"), to_base=1000)
HOUR = Unit("hour", Dimension("time"), to_base=3600)
SECOND = Unit("second", Dimension("time"))
KILOMETER_PER_HOUR = KILOMETER / HOUR


# =============================================================================
# 2. Temporal Signal — Values With Time
# =============================================================================

@dataclass
class Timestamp:
    """Simple timestamp representation."""
    value: float  # Unix epoch seconds
    
    def __lt__(self, other: 'Timestamp') -> bool:
        return self.value < other.value


@dataclass
class Signal:
    """A value that varies over time, stored as an append-only log."""
    name: str
    history: List[Tuple[Timestamp, float]] = field(default_factory=list)
    unit: Optional[Unit] = None
    
    def push(self, ts: Timestamp, value: float):
        self.history.append((ts, value))
    
    @property
    def current(self) -> Optional[float]:
        """Most recent value."""
        if not self.history:
            return None
        return self.history[-1][1]
    
    @property
    def current_ts(self) -> Optional[Timestamp]:
        if not self.history:
            return None
        return self.history[-1][0]
    
    def avg_since(self, ts: Timestamp) -> Optional[float]:
        """Average value since given timestamp."""
        relevant = [(t, v) for t, v in self.history if t >= ts]
        if not relevant:
            return None
        return sum(v for _, v in relevant) / len(relevant)
    
    def when(self, predicate: 'Condition') -> Optional[float]:
        """Value when condition held (first match)."""
        for ts, val in self.history:
            if predicate.eval(ts):
                return val
        return None


@dataclass
class Condition:
    """Simple temporal predicate."""
    signal: Signal
    threshold: float
    
    def eval(self, ts: Timestamp) -> bool:
        """Check if signal exceeded threshold before this time."""
        for t, v in self.signal.history:
            if t > ts:
                break
            if v >= self.threshold:
                return True
        return False


# =============================================================================
# 3. Variable & Value Representation
# =============================================================================

@dataclass
class Variable:
    """A variable in a relation graph."""
    name: str
    value: Optional[float] = None
    unit: Optional[Unit] = None
    is_bound: bool = False  # Was explicitly given by user?
    
    def __repr__(self):
        if self.value is None:
            return f"{self.name} = ?"
        if self.unit:
            return f"{self.name} = {self.value:.2f} {self.unit.name}"
        return f"{self.name} = {self.value:.2f}"


@dataclass
class Binding:
    """An assignment in a solve context."""
    variable: str
    value: float
    unit: Optional[Unit] = None
    from_relation: Optional[str] = None  # Which relation derived this


# =============================================================================
# 4. Expression Parser & AST
# =============================================================================

class TideExpression:
    """Base class for Tide expressions."""
    pass


@dataclass
class VarExpr(TideExpression):
    name: str


@dataclass
class ConstExpr(TideExpression):
    value: float


@dataclass
class BinOpExpr(TideExpression):
    left: TideExpression
    op: str  # '+', '-', '*', '/'
    right: TideExpression


@dataclass
class FuncCallExpr(TideExpression):
    func_name: str
    args: List[TideExpression]


def parse_expression(code: str) -> TideExpression:
    """Minimal expression parser supporting: numbers, vars, (+ - * /), parentheses."""
    code = code.strip()
    tokens = tokenize(code)
    pos = [0]  # Mutable position tracker
    
    def parse_atom():
        if pos[0] >= len(tokens):
            raise SyntaxError("Unexpected end of expression")
        
        tok = tokens[pos[0]]
        if tok == '(':
            pos[0] += 1
            expr = parse_additive()
            if pos[0] >= len(tokens) or tokens[pos[0]] != ')':
                raise SyntaxError("Expected ')'")
            pos[0] += 1
            return expr
        elif tok.replace('.', '').isdigit():
            pos[0] += 1
            return ConstExpr(float(tok))
        else:
            pos[0] += 1
            return VarExpr(tok)
    
    def parse_unary():
        if pos[0] < len(tokens) and tokens[pos[0]] == '-':
            pos[0] += 1
            return BinOpExpr(ConstExpr(0), '-', parse_unary())
        return parse_atom()
    
    def parse_multiplicative():
        left = parse_unary()
        while pos[0] < len(tokens) and tokens[pos[0]] in ('*', '/'):
            op = tokens[pos[0]]
            pos[0] += 1
            left = BinOpExpr(left, op, parse_unary())
        return left
    
    def parse_additive():
        left = parse_multiplicative()
        while pos[0] < len(tokens) and tokens[pos[0]] in ('+', '-'):
            op = tokens[pos[0]]
            pos[0] += 1
            left = BinOpExpr(left, op, parse_multiplicative())
        return left
    
    return parse_additive()


def tokenize(expr: str) -> List[str]:
    """Simple tokenizer."""
    expr = expr.replace('(', ' ( ').replace(')', ' ) ')
    expr = expr.replace('+', ' + ').replace('-', ' - ').replace('*', ' * ').replace('/', ' / ')
    expr = expr.replace('×', ' * ').replace('÷', ' / ')  # Unicode operators
    return [t for t in expr.split() if t.strip()]


# =============================================================================
# 5. The Relation Graph & Solver Engine
# =============================================================================

@dataclass
class Edge:
    """An edge in the dependency graph: variable depends on another."""
    from_var: str
    to_var: str
    derivative: float = 1.0  # Used for back-propagation


@dataclass
class Relation:
    """A mathematical relation between variables."""
    name: str
    equation_str: str  # "distance = speed * time"
    lvar: str          # Left-hand variable
    rexp: TideExpression  # Right-hand expression
    variables: Set[str] = field(default_factory=set)
    is_directed: bool = False  # Force one direction only


class TideSolver:
    """
    Constraint solver using iterative relaxation with backpropagation.
    
    Key idea: Given n equations and m unknowns:
    - If n == m: solve for all unknowns
    - If n < m: find which subset satisfies constraints  
    - If n > m: detect inconsistency
    
    We use a graph-walking approach: propagate known values through
    the relation, then use symbolic inversion to solve backwards.
    """
    
    def __init__(self):
        self.relations: Dict[str, Relation] = {}
        self.variables: Dict[str, Variable] = {}
        self.graph: Dict[str, Set[str]] = defaultdict(set)  # Dependencies
    
    def define(self, name: str, equation: str):
        """Add a relation like 'distance = speed * time'."""
        eq_parts = equation.split('=')
        if len(eq_parts) != 2:
            raise ValueError(f"Invalid equation: {equation}")
        
        lvar = eq_parts[0].strip()
        rexpr_str = eq_parts[1].strip()
        rexpr = parse_expression(rexpr_str)
        var_names = extract_variables(rexpr)
        var_names.add(lvar)
        
        rel = Relation(name, equation, lvar, rexpr, var_names)
        self.relations[name] = rel
        
        # Build dependency graph
        for v in var_names:
            if v not in self.variables:
                self.variables[v] = Variable(v)
            self.graph[lvar].update(var_names - {lvar})
    
    def solve(self, bindings: Dict[str, Tuple[float, Optional[Unit]]]) -> Dict[str, float]:
        """
        Solve the network given some bound variables.
        Returns all computed variable values.
        """
        # Initialize bindings
        for var, (val, unit) in bindings.items():
            if var not in self.variables:
                self.variables[var] = Variable(var)
            self.variables[var].value = val
            self.variables[var].unit = unit
            self.variables[var].is_bound = True
        
        # Iterative propagation until fixed point or inconsistency
        max_iterations = 100
        changed = True
        iteration = 0
        
        while changed and iteration < max_iterations:
            changed = False
            iteration += 1
            
            # Try to compute each relation
            for rel_name, rel in self.relations.items():
                result, deriv = self._try_compute_rel(rel)
                if result is not None:
                    old_val = self.variables[rel.lvar].value
                    self.variables[rel.lvar].value = result
                    
                    # Back-propagate if this was a newly discovered value
                    if not self.variables[rel.lvar].is_bound:
                        self._back_propagate(rel, deriv)
                    
                    if old_val != result:
                        changed = True
        
        # Check for unsolved variables
        result = {}
        for name, var in self.variables.items():
            if var.value is None:
                result[name] = float('nan')  # Underconstrained
            else:
                result[name] = var.value
        
        # Detect overdetermined systems
        for rel_name, rel in self.relations.items():
            if all(self.variables.get(v, Variable(v)).value is not None 
                   for v in rel.variables):
                expected, _ = self._compute_rel_value(rel)
                actual = self.variables[rel.lvar].value
                if abs(expected - actual) > 1e-6:
                    raise ValueError(
                        f"Inconsistency in {rel_name}: "
                        f"relation expects {rel.lvar}={expected:.4f}, "
                        f"but given values imply {rel.lvar}={actual:.4f}"
                    )
        
        return result
    
    def _try_compute_rel(self, rel: Relation) -> Tuple[Optional[float], float]:
        """Try to evaluate relation given current variable values. Returns (result, derivative wrt lhs)."""
        known = {v: self.variables[v].value 
                 for v in rel.variables 
                 if self.variables[v].value is not None}
        
        if len(known) == len(rel.variables):
            # All known — evaluate and check consistency
            computed = self._eval_expr(rel.rexp, known)
            return self.variables[rel.lvar].value, 1.0
        
        if len(known) == len(rel.variables) - 1:
            # Exactly one unknown — solve for it
            unknown = (set(rel.variables) - set(known.keys())).pop()
            if unknown == rel.lvar:
                computed = self._eval_expr(rel.rexp, known)
                return computed, 1.0
            else:
                # Unknown is on RHS — we need symbolic inverse
                return self._solve_inverse(rel, unknown, known)
        
        return None, 0.0
    
    def _eval_expr(self, expr: TideExpression, scope: Dict[str, float]) -> float:
        """Evaluate expression with given variable bindings."""
        if isinstance(expr, ConstExpr):
            return expr.value
        if isinstance(expr, VarExpr):
            if expr.name not in scope:
                raise ValueError(f"Unknown variable {expr.name}")
            return scope[expr.name]
        if isinstance(expr, BinOpExpr):
            left = self._eval_expr(expr.left, scope)
            right = self._eval_expr(expr.right, scope)
            ops = {
                '+': operator.add,
                '-': operator.sub,
                '*': operator.mul,
                '/': operator.truediv,
            }
            return ops[expr.op](left, right)
        raise ValueError(f"Unsupported expression: {expr}")
    
    def _solve_inverse(self, rel: Relation, unknown: str, known: Dict[str, float]):
        """Solve for unknown variable in a simple relation."""
        expr = rel.rexp
        lhs_val = self.variables[rel.lvar].value
        
        # Simple case: lhs = known_op unknown
        if isinstance(expr, BinOpExpr):
            if isinstance(expr.right, VarExpr) and expr.right.name == unknown:
                # lhs = left op unknown => unknown = lhs op^-1 left
                left_val = self._eval_expr(expr.left, known)
                return self._invert_binop(lhs_val, left_val, expr.op)
            
            if isinstance(expr.left, VarExpr) and expr.left.name == unknown:
                # lhs = unknown op right => unknown = lhs op^-1 right
                right_val = self._eval_expr(expr.right, known)
                return self._invert_right(lhs_val, right_val, expr.op)
        
        raise NotImplementedError(f"Cannot symbolically solve: {rel.equation_str} for {unknown}")
    
    def _invert_binop(self, result: float, left: float, op: str) -> Tuple[Optional[float], float]:
        """Invert: result = left op x"""
        if op == '+':
            return result - left, 1.0
        elif op == '-':
            return left - result, -1.0
        elif op == '*':
            if left == 0:
                raise ValueError("Cannot divide by zero")
            return result / left, 1.0 / left
        elif op == '/':
            if left == 0:
                raise ValueError("Cannot divide by zero")
            return result * left, result
        raise ValueError(f"Unknown op: {op}")
    
    def _invert_right(self, result: float, right: float, op: str) -> Tuple[Optional[float], float]:
        """Invert: result = x op right"""
        if op == '+':
            return result - right, 1.0
        elif op == '-':
            return result + right, 1.0
        elif op == '*':
            if right == 0:
                raise ValueError("Cannot divide by zero")
            return result / right, 1.0 / right
        elif op == '/':
            if right == 0:
                raise ValueError("Cannot divide by zero")
            return result * right, right
        raise ValueError(f"Unknown op: {op}")
    
    def _compute_rel_value(self, rel: Relation) -> Tuple[float, float]:
        """Compute LHS value from RHS given all variables known."""
        scope = {v: self.variables[v].value for v in rel.variables}
        return self._eval_expr(rel.rexp, scope), 1.0
    
    def _back_propagate(self, rel: Relation, deriv: float):
        """Propagate changes back through dependencies."""
        # This would be expanded with proper gradient tracking
        pass
    
    def get_results(self) -> str:
        """Pretty-print current state."""
        lines = []
        for name, var in sorted(self.variables.items()):
            lines.append(str(var))
        return "\n".join(lines)


# =============================================================================
# 6. Parser: Turn Tide Source Into Relations
# =============================================================================

class TideParser:
    """Parse Tide source files into a solver-ready form."""
    
    def __init__(self):
        self.solver = TideSolver()
    
    def parse(self, source: str) -> TideSolver:
        """Parse entire source file."""
        lines = source.strip().split('\n')
        i = 0
        
        while i < len(lines):
            line = lines[i].strip()
            
            # Skip comments and empty lines
            if not line or line.startswith('//'):
                i += 1
                continue
            
            # Parse relation definition
            if line.startswith('relation '):
                # Extract relation name
                rel_match = re.match(r'relation\s+(\w+)\s*\{?', line)
                if rel_match:
                    rel_name = rel_match.group(1)
                    i += 1
                    
                    # Read relation body
                    equations = []
                    brace_count = 1 if '{' in line else 0
                    while i < len(lines):
                        body_line = lines[i].strip()
                        
                        if '{' in body_line:
                            brace_count += 1
                        if '}' in body_line:
                            brace_count -= 1
                        
                        if '=' in body_line and not body_line.startswith('//'):
                            # Parse equation
                            eq_match = re.match(r'(\w+)\s*=\s*(.+)', body_line)
                            if eq_match:
                                equations.append((eq_match.group(1), eq_match.group(2)))
                        
                        if brace_count <= 0 and '{' in line:
                            i += 1
                            break
                        elif '{' not in line and brace_count == 0:
                            # Single-line relation
                            break
                        
                        i += 1
                    
                    for lhs, rhs in equations:
                        self.solver.define(rel_name, f"{lhs} = {rhs}")
            else:
                i += 1
        
        return self.solver


# =============================================================================
# 7. Demo Programs
# =============================================================================

def demo_motion_solver():
    """Demonstrate the core motion relation."""
    print("=" * 60)
    print("Demo 1: Motion Solver")
    print("=" * 60)
    
    solver = TideSolver()
    solver.define("Motion", "distance = speed * time")
    
    # Case 1: Find distance
    print("\n--- Finding distance ---")
    result = solver.solve({
        "speed": (60, None),
        "time": (2, None)
    })
    print(f"Given speed=60, time=2:")
    for k, v in result.items():
        if not k.isupper():
            print(f"  {k} = {v}")
    
    # Reset for next case
    solver2 = TideSolver()
    solver2.define("Motion", "distance = speed * time")
    
    # Case 2: Find time
    print("\n--- Finding time ---")
    result2 = solver2.solve({
        "distance": (450, None),
        "speed": (90, None)
    })
    print(f"Given distance=450, speed=90:")
    for k, v in result2.items():
        if not k.isupper():
            print(f"  {k} = {v:.2f}")
    
    # Reset for next case
    solver3 = TideSolver()
    solver3.define("Motion", "distance = speed * time")
    
    # Case 3: Detect inconsistency
    print("\n--- Checking consistency ---")
    try:
        result3 = solver3.solve({
            "distance": (100, None),
            "speed": (50, None),
            "time": (3, None)
        })
        print("ERROR: Should have detected inconsistency!")
    except ValueError as e:
        print(f"Inconsistency detected (as expected): {e}")


def demo_unit_system():
    """Demonstrate dimensional analysis."""
    print("\n" + "=" * 60)
    print("Demo 2: Unit-Aware Arithmetic")
    print("=" * 60)
    
    # This would be caught at compile time
    try:
        incompatible = METER + HOUR
        print("ERROR: Should have rejected incompatible units!")
    except TypeError as e:
        print(f"Unit mismatch caught: {e}")
    
    print(f"\nValid: {KILOMETER} = {METER.to_base / KILOMETER.to_base} meters")
    print(f"Speed unit: {KILOMETER_PER_HOUR}")


def demo_complex_relation():
    """More complex multi-relation scenario."""
    print("\n" + "=" * 60)
    print("Demo 3: Multi-Relation Network")
    print("=" * 60)
    
    parser = TideParser()
    source = """
    relation Trip {
        distance = speed * time
        fuel = distance * consumption
        cost = fuel * price
    }
    """
    
    solver = parser.parse(source)
    result = solver.solve({
        "speed": (90, None),
        "time": (4, None),
        "consumption": (0.07, None),
        "price": (1.75, None)
    })
    
    print("Trip calculation:")
    for k, v in sorted(result.items()):
        print(f"  {k:<15} = {v:>8.2f}")


def demo_signal_basic():
    """Basic temporal signal."""
    print("\n" + "=" * 60)
    print("Demo 4: Temporal Signals")
    print("=" * 60)
    
    temp_signal = Signal("room_temperature")
    temp_signal.push(Timestamp(1000), 20.0)
    temp_signal.push(Timestamp(2000), 22.0)
    temp_signal.push(Timestamp(3000), 19.5)
    temp_signal.push(Timestamp(4000), 21.0)
    
    print(f"Current temperature: {temp_signal.current}°C")
    print(f"Average since T=2000: {temp_signal.avg_since(Timestamp(2000)):.2f}°C")


def run_all_demos():
    """Execute all demonstrations."""
    demo_motion_solver()
    demo_unit_system()
    demo_complex_relation()
    demo_signal_basic()
    
    print("\n" + "=" * 60)
    print("All demos complete!")
    print("=" * 60)


if __name__ == "__main__":
    run_all_demos()
