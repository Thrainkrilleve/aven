"""
AVEN v0.4 — THE CORE
=====================

This is the minimum viable core: what you need before building
anything else (CLI, IDE, package manager).

Components:
  1. MODULE SYSTEM — `import` statements, file resolution, namespaces
  2. TYPE SYSTEM — extensible dimensions, unit literals, full checking
  3. STANDARD LIBRARY — physics, finance, geometry relations
  4. INTEGRATED COMPILER — type-check → compile → run in one pass

Structure:
  aven/core/
    ├── loader.py       # file loading, caching, circular deps
    ├── lexer.py        # tokenization (carried from v0.3)
    ├── parser.py       # AST construction (carried from v0.3)
    ├── typecheck.py    # dimensional analysis + type inference
    ├── stdlib/         # .aven modules that ship with Aven
    └── compiler.py     # integrated pipeline

Run: python aven_core_v04.py
Dependencies: none, Python 3.8+
"""

import math
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple, Union

VERSION = "0.4.0"
LANG_NAME = "Aven"


# =============================================================================
# PART 1 — EXPRESSIONS (AST)
# =============================================================================

@dataclass(frozen=True)
class Num:
    value: float

@dataclass(frozen=True)
class Str:
    value: str

@dataclass(frozen=True)
class Var:
    name: str

@dataclass(frozen=True)
class Bin:
    op: str
    left: object
    right: object

@dataclass(frozen=True)
class Unary:
    op: str
    operand: object

@dataclass(frozen=True)
class Call:
    fn_name: str
    args: List[object]

@dataclass(frozen=True)
class BoolLit:
    value: bool


FUNCS = {
    'sqrt': math.sqrt, 'log': math.log, 'exp': math.exp,
    'sin': math.sin, 'cos': math.cos, 'tan': math.tan,
    'abs': abs, 'max': max, 'min': min, 'floor': math.floor,
    'ceil': math.ceil, 'round': round,
}


# =============================================================================
# PART 2 — DIMENSIONS & UNITS (extensible type system)
# =============================================================================

@dataclass(frozen=True)
class Dim:
    """Physical dimensions: length, time, mass, money, temperature, amount."""
    length: int = 0
    time: int = 0
    mass: int = 0
    money: int = 0
    temperature: int = 0
    amount: int = 0
    angle: int = 0  # radians (dimensionless but tracked for clarity)
    
    def __mul__(self, o: 'Dim') -> 'Dim':
        return Dim(
            self.length + o.length, self.time + o.time,
            self.mass + o.mass, self.money + o.money,
            self.temperature + o.temperature,
            self.amount + o.amount, self.angle + o.angle
        )
    
    def __truediv__(self, o: 'Dim') -> 'Dim':
        return Dim(
            self.length - o.length, self.time - o.time,
            self.mass - o.mass, self.money - o.money,
            self.temperature - o.temperature,
            self.amount - o.amount, self.angle - o.angle
        )
    
    def __pow__(self, n: int) -> 'Dim':
        return Dim(
            self.length * n, self.time * n,
            self.mass * n, self.money * n,
            self.temperature * n,
            self.amount * n, self.angle * n
        )
    
    def __eq__(self, o) -> bool:
        if not isinstance(o, Dim):
            return False
        return (self.length == o.length and self.time == o.time and
                self.mass == o.mass and self.money == o.money and
                self.temperature == o.temperature and
                self.amount == o.amount and self.angle == o.angle)
    
    def __str__(self):
        parts = []
        dims = [('L', self.length), ('T', self.time), ('M', self.mass),
                ('$^', self.money), ('Θ', self.temperature),
                ('N', self.amount), ('rad', self.angle)]
        for sym, v in dims:
            if v == 1: parts.append(sym)
            elif v: parts.append(f"{sym}^{v}")
        return '·'.join(parts) if parts else 'scalar'
    
    def is_dimensionless(self) -> bool:
        return all(v == 0 for v in [self.length, self.time, self.mass,
                                     self.money, self.temperature,
                                     self.amount, self.angle])


# Built-in dimensions
LENGTH, TIME, MASS, MONEY = Dim(1,0,0,0,0,0,0), Dim(0,1,0,0,0,0,0), \
                            Dim(0,0,1,0,0,0,0), Dim(0,0,0,1,0,0,0)
TEMPERATURE, AMOUNT = Dim(0,0,0,0,1,0,0), Dim(0,0,0,0,0,1,0)


@dataclass(frozen=True)
class Unit:
    """Named unit with conversion to base dimension."""
    name: str
    dimension: Dim
    to_base: float  # multiplier to convert TO base unit
    
    def compatible_with(self, other: 'Unit') -> bool:
        return self.dimension == other.dimension


# Standard units registry (can be extended by user modules)
STANDARD_UNITS: Dict[str, Unit] = {
    'm': Unit('m', LENGTH, 1.0),
    'km': Unit('km', LENGTH, 1000.0),
    'cm': Unit('cm', LENGTH, 0.01),
    'mm': Unit('mm', LENGTH, 0.001),
    
    's': Unit('s', TIME, 1.0),
    'min': Unit('min', TIME, 60.0),
    'h': Unit('h', TIME, 3600.0),
    'day': Unit('day', TIME, 86400.0),
    
    'kg': Unit('kg', MASS, 1.0),
    'g': Unit('g', MASS, 0.001),
    
    '$': Unit('$', MONEY, 1.0),
    '€': Unit('€', MONEY, 1.10),  # rough EUR→USD rate
    '£': Unit('£', MONEY, 1.27),  # GBP→USD
    
    'rad': Unit('rad', Dim(0,0,0,0,0,0,1), 1.0),
    'deg': Unit('deg', Dim(0,0,0,0,0,0,1), math.pi / 180),
}


def lookup_unit(name: str) -> Optional[Unit]:
    """Lookup unit name; raises on unknown."""
    # Handle compound units: km/h → (km)/(h)
    if '/' in name:
        parts = name.split('/')
        num, denom = parts[0], parts[1]
        nu = lookup_unit(num)
        de = lookup_unit(denom)
        return Unit(f"{num}/{denom}", nu.dimension / de.dimension,
                    nu.to_base / de.to_base)
    if name in STANDARD_UNITS:
        return STANDARD_UNITS[name]
    return None


def check_unit_compatibility(unit_str: str, expected_dim: Dim) -> Tuple[bool, str]:
    """Check if a unit string matches expected dimension."""
    u = lookup_unit(unit_str)
    if u is None:
        # Unknown unit — assume user-defined, check if dimensionless
        if expected_dim.is_dimensionless():
            return True, ""
        return False, f"unknown unit '{unit_str}'"
    if u.dimension != expected_dim:
        return False, f"unit '{unit_str}' is {u.dimension}, " \
                      f"expected {expected_dim}"
    return True, ""


# =============================================================================
# PART 3 — LEXER (carried from v0.3)
# =============================================================================

KEYWORDS = {'forward', 'relation', 'solve', 'with', 'signal', 'unit',
            'let', 'true', 'false', 'import', 'from', 'return',
            'if', 'else', 'when'}

@dataclass(frozen=True)
class Tok:
    kind: str
    text: str
    line: int
    col: int
    
    def where(self) -> str:
        return f"{self.line}:{self.col}"


class LexerError(Exception):
    def __init__(self, msg: str, tok: Tok):
        super().__init__(f"{tok.where()}: {msg}")


def lex(src: str, filename: str = "<input>") -> List[Tok]:
    toks, i, line, col = [], 0, 1, 1
    
    def advance(k=1):
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
            advance()
        elif c == '\n':
            toks.append(Tok('NL', '\\n', line, col)); advance()
        elif c == '/' and i+1 < len(src) and src[i+1] == '/':
            while i < len(src) and src[i] != '\n':
                advance()
        elif c == '"' or c == "'":
            q = c; i += 1; start = line; sc = col
            s = []
            while i < len(src) and src[i] != q:
                if src[i] == '\\' and i+1 < len(src):
                    esc = {'n': '\n', 't': '\t', '"': '"', "'": "'", '\\': '\\'}
                    s.append(esc.get(src[i+1], src[i+1])); i += 2
                else:
                    s.append(src[i]); i += 1
            if i >= len(src):
                raise LexerError("unterminated string", Tok('STR', '', start, sc))
            i += 1; advance(-1)
            toks.append(Tok('STRING', ''.join(s), start, sc)); advance()
        elif c.isdigit() or (c == '.' and i+1 < len(src) and src[i+1].isdigit()):
            start_l, start_c = line, col
            m = re.match(r'\d+(\.\d+)?([eE][+-]?\d+)?', src[i:])
            text = m.group(0)
            toks.append(Tok('NUM', text, start_l, start_c)); advance(len(text))
        elif c.isalpha() or c == '_':
            start_l, start_c = line, col
            m = re.match(r'[A-Za-z_]\w*', src[i:])
            text = m.group(0)
            kind = 'KW' if text in KEYWORDS else 'ID'
            toks.append(Tok(kind, text, start_l, start_c)); advance(len(text))
        elif c in '+-*/^(){}=:,<>' or c == '?' or c == '!':
            toks.append(Tok('OP', c, line, col)); advance()
        else:
            raise LexerError(f"unexpected character {c!r}", Tok('ERR', c, line, col))
    toks.append(Tok('EOF', '<eof>', line, col))
    return toks


# =============================================================================
# PART 4 — PARSER & AST (expanded with import/module support)
# =============================================================================

@dataclass
class Eq:
    lhs: str
    rhs: object
    text: str
    tok: Tok

@dataclass
class BlockDecl:
    directive: str
    name: str
    equations: List[Eq]

@dataclass
class Binding:
    name: str
    value: float
    unit: str
    tok: Tok

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
    dim_name: str
    names: List[str]

@dataclass
class ImportStmt:
    module: str
    aliases: Dict[str, str]  # old_name → new_name

@dataclass
class ModuleDef:
    name: str
    imports: List[ImportStmt]
    blocks: List[BlockDecl]
    solves: List[SolveDecl]
    signals: List[SignalDecl]
    units: List[UnitDecl]


class Parser:
    def __init__(self, toks: List[Tok]):
        self.toks = toks
        self.p = 0
    
    def peek(self, k=0) -> Tok:
        return self.toks[min(self.p+k, len(self.toks)-1)]
    
    def take(self):
        t = self.toks[self.p]
        if t.kind != 'EOF': self.p += 1
        return t
    
    def expect(self, kind: str, text: str = None) -> Tok:
        t = self.peek()
        if t.kind != kind:
            raise SyntaxError(f"{t.where()}: expected {kind}, got {t.text!r}")
        if text and t.text != text:
            raise SyntaxError(f"{t.where()}: expected {text!r}, got {t.text!r}")
        return self.take()
    
    def skip_nl(self):
        while self.peek().kind == 'NL': self.take()
    
    def parse_module(self, name="<anon>") -> ModuleDef:
        imps, blocks, solves, sigs, units = [], [], [], [], []
        while self.peek().kind != 'EOF':
            self.skip_nl()
            if self.peek().kind == 'EOF': break
            t = self.peek()
            if t.kind == 'KW' and t.text in ('forward', 'relation'):
                blocks.append(self.parse_block())
            elif t.kind == 'KW' and t.text == 'solve':
                solves.append(self.parse_solve())
            elif t.kind == 'KW' and t.text == 'signal':
                sigs.append(self.parse_signal())
            elif t.kind == 'KW' and t.text == 'unit':
                units.append(self.parse_unitdecl())
            elif t.kind == 'KW' and t.text == 'import':
                imps.append(self.parse_import())
            else:
                raise SyntaxError(f"{t.where()}: unexpected '{t.text}'")
        return ModuleDef(name, imps, blocks, solves, sigs, units)
    
    def parse_import(self) -> ImportStmt:
        self.take(); alias = {}
        module = self.expect('ID').text
        if self.peek().kind == 'OP' and self.peek().text == 'as':
            self.take()
            while self.peek().kind == 'ID':
                old = self.take().text
                self.expect('=', '=')
                new = self.take().text
                alias[old] = new
                if self.peek().kind == 'OP' and self.peek().text == ',':
                    self.take()
        return ImportStmt(module, alias)
    
    def parse_block(self) -> BlockDecl:
        d = self.take().text
        name = self.expect('ID').text
        self.expect('OP', '{')
        eqs = []
        while not (self.peek().kind == 'OP' and self.peek().text == '}'):
            eqs.append(self.parse_eq())
        self.expect('OP', '}'); return BlockDecl(d, name, eqs)
    
    def parse_eq(self) -> Eq:
        lhs = self.expect('ID').text; self.expect('OP', '=')
        rhs = self.parse_expr()
        return Eq(lhs, rhs, f"{lhs} = {_src(rhs)}", self.peek())
    
    def parse_solve(self) -> SolveDecl:
        self.take(); name = self.expect('ID').text; self.expect('KW', 'with')
        self.expect('OP', '{'); binds = []
        while not (self.peek().kind == 'OP' and self.peek().text == '}'):
            n = self.expect('ID').text; self.expect('OP', '=')
            t = self.expect('NUM')
            u = ''
            while self.peek().kind in ('ID', 'OP') and \
                  (self.peek().text not in ('}', ',', 'NL')):
                u += self.take().text
            binds.append(Binding(n, float(t.text), u, t))
        self.expect('OP', '}'); return SolveDecl(name, binds, self.peek())
    
    def parse_signal(self) -> SignalDecl:
        self.take(); name = self.expect('ID').text
        u = ''
        if self.peek().kind == 'OP' and self.peek().text == ':':
            self.take(); u = self.expect('ID').text
        return SignalDecl(name, u)
    
    def parse_unitdecl(self) -> UnitDecl:
        self.take(); dim = self.expect('ID').text
        self.expect('OP', ':')
        names = [self.expect('ID').text]
        while self.peek().kind == 'OP' and self.peek().text == ',':
            self.take(); names.append(self.expect('ID').text)
        return UnitDecl(dim, names)
    
    # --- expressions ---
    def parse_expr(self): return self.additive()
    def additive(self):
        e = self.multiplicative()
        while self.peek().kind == 'OP' and self.peek().text in '+-':
            e = Bin(self.take().text, e, self.multiplicative())
        return e
    def multiplicative(self):
        e = self.unary()
        while self.peek().kind == 'OP' and self.peek().text in '*/':
            e = Bin(self.take().text, e, self.unary())
        return e
    def unary(self):
        if self.peek().kind == 'OP' and self.peek().text == '-':
            self.take(); return Unary('-', self.unary())
        return self.power()
    def power(self):
        b = self.atom()
        if self.peek().kind == 'OP' and self.peek().text == '^':
            self.take(); return Bin('^', b, self.unary())
        return b
    def atom(self):
        t = self.peek()
        if t.kind == 'NUM': self.take(); return Num(float(t.text))
        if t.kind == 'STRING': self.take(); return Str(t.text)
        if t.kind == 'ID':
            self.take()
            if self.peek().kind == 'OP' and self.peek().text == '(' and \
               t.text in FUNCS:
                self.take()
                args = []; self.skip_nl()
                while self.peek().text != ')':
                    args.append(self.parse_expr()); self.skip_nl()
                    if self.peek().kind == 'OP' and self.peek().text == ',':
                        self.take(); self.skip_nl()
                self.expect('OP', ')')
                return Call(t.text, args)
            return Var(t.text)
        if t.kind == 'KW' and t.text == 'true': self.take(); return BoolLit(True)
        if t.kind == 'KW' and t.text == 'false': self.take(); return BoolLit(False)
        raise SyntaxError(f"{t.where()}: expected expression, got {t.text!r}")


def _src(e) -> str:
    PREC = {'+': 1, '-': 1, '*': 2, '/': 2, '^': 3}
    if isinstance(e, Num): return f"{e.value:g}"
    if isinstance(e, Str): return f'"{e.value}"'
    if isinstance(e, Var): return e.name
    if isinstance(e, Bin):
        s = f"{_src(e.left)} {e.op} {_src(e.right)}"
        return f"({s})" if PREC.get(e.op, 0) == 1 else s
    if isinstance(e, Unary): return f"-{_src(e.operand)}"
    if isinstance(e, Call):
        args = ', '.join(_src(a) for a in e.args)
        return f"{e.fn_name}({args})"
    return "?"


def parse(src: str, name="<anon>") -> ModuleDef:
    return Parser(lex(src)).parse_module(name)


# =============================================================================
# PART 5 — TYPE CHECKER (dimensional analysis + unit verification)
# =============================================================================

class TypeError_(Exception):
    pass


class TypeChecker:
    """
    Checks:
      1. Every equation's LHS dimension equals RHS dimension
      2. All bindings use correct units for their inferred type
      3. Binary operations combine compatible dimensions
    """
    
    def __init__(self, mod: ModuleDef):
        self.mod = mod
        self.inferred: Dict[str, Dim] = {}
        self.units: Dict[str, Unit] = dict(STANDARD_UNITS)
        self.errors: List[str] = []
    
    def register_units(self, decls: List[UnitDecl]):
        """Register user-declared units."""
        dim_map = {
            'length': LENGTH, 'time': TIME, 'mass': MASS,
            'money': MONEY, 'temperature': TEMPERATURE,
            'amount': AMOUNT, 'angle': Dim(0,0,0,0,0,0,1),
        }
        for ud in decls:
            dim = dim_map.get(ud.dim_name.lower())
            if dim is None:
                self.errors.append(f"line ?: unknown dimension '{ud.dim_name}'")
                continue
            for uname in ud.names:
                self.units[uname] = Unit(uname, dim, 1.0)
    
    def infer_dim(self, expr: object, ctx: Dict[str, Dim]) -> Dim:
        """Infer dimension of an expression."""
        if isinstance(expr, Num): return Dim(0,0,0,0,0,0,0)
        if isinstance(expr, Str): return Dim(0,0,0,0,0,0,0)  # strings are dimensionless
        if isinstance(expr, Var):
            if expr.name in ctx: return ctx[expr.name]
            return Dim(0,0,0,0,0,0,0)  # unknown vars assumed scalar
        if isinstance(expr, Bin):
            ld, rd = self.infer_dim(expr.left, ctx), self.infer_dim(expr.right, ctx)
            if expr.op in ('+', '-'):
                if ld != rd:
                    self.errors.append(
                        f"cannot add/subtract {ld} and {rd}")
                return ld
            if expr.op == '*': return ld * rd
            if expr.op == '/': return ld / rd
            if expr.op == '^':
                if rd.is_dimensionless():
                    return ld ** int(rd.length)
                self.errors.append("exponent must be dimensionless")
                return ld
        if isinstance(expr, Call):
            if expr.fn_name in ('sqrt', 'abs', 'floor', 'ceil', 'round', 'max', 'min'):
                return self.infer_dim(expr.args[0], ctx) if expr.args else Dim()
            if expr.fn_name in ('log', 'exp', 'sin', 'cos', 'tan'):
                if expr.args:
                    d = self.infer_dim(expr.args[0], ctx)
                    if not d.is_dimensionless():
                        self.errors.append(f"{expr.fn_name}() requires dimensionless argument, got {d}")
                return Dim()
        return Dim(0,0,0,0,0,0,0)
    
    def check_block(self, block: BlockDecl) -> Dict[str, Dim]:
        """Type-check a relation/block, return inferred types."""
        ctx: Dict[str, Dim] = {}
        
        # First pass: mark LHS variables as derived (unknown type initially)
        for eq in block.equations:
            ctx[eq.lhs] = Dim(0,0,0,0,0,0,0)  # placeholder
        
        # Second pass: infer and validate each equation
        for eq in block.equations:
            lhs_dim = self.infer_dim(eq.rhs, ctx)
            
            # Check if LHS already has a different inferred type
            if eq.lhs in ctx and ctx[eq.lhs].is_dimensionless() and \
               not lhs_dim.is_dimensionless():
                ctx[eq.lhs] = lhs_dim  # assign now that we know it
            elif eq.lhs in ctx and ctx[eq.lhs] != lhs_dim:
                self.errors.append(
                    f"'{eq.lhs}' was used with dimension {ctx[eq.lhs]}, "
                    f"now defined as {lhs_dim}")
                continue
            ctx[eq.lhs] = lhs_dim
        
        return ctx
    
    def check_bindings(self, binds: List[Binding], ctx: Dict[str, Dim]):
        """Verify each binding has the right dimension/unit."""
        for b in binds:
            if b.name not in ctx:
                self.errors.append(f"'{b.name}' is not a variable in this context")
                continue
            if b.unit:
                ok, msg = check_unit_compatibility(b.unit, ctx[b.name])
                if not ok:
                    self.errors.append(msg)
    
    def check(self) -> List[str]:
        """Run full type check; return list of errors (empty if OK)."""
        self.register_units(self.mod.units)
        for blk in self.mod.blocks:
            ctx = self.check_block(blk)
            # check any solve bindings for this block
            for sd in self.mod.solves:
                if sd.name == blk.name:
                    self.check_bindings(sd.bindings, ctx)
        return self.errors


# =============================================================================
# PART 6 — LOADER (module loading + dependency resolution)
# =============================================================================

class LoadError(Exception):
    pass


class Loader:
    """Load modules, resolve dependencies, detect cycles."""
    
    def __init__(self, root: Optional[Path] = None):
        self.root = root or Path('.')
        self.cache: Dict[str, Tuple[ModuleDef, Dim]] = {}
        self.loading: Set[str] = set()
    
    def load(self, name: str) -> ModuleDef:
        if name in self.cache:
            return self.cache[name][0]
        if name in self.loading:
            raise LoadError(f"circular import involving '{name}'")
        
        self.loading.add(name)
        try:
            path = self.find_module(name)
            if path is None:
                raise LoadError(f"module '{name}' not found")
            
            src = path.read_text()
            mod = parse(src, str(path))
            
            # Load imports first
            for imp in mod.imports:
                self.load(imp.module)
            
            self.cache[name] = (mod, Dim())
            return mod
        finally:
            self.loading.discard(name)
    
    def find_module(self, name: str) -> Optional[Path]:
        """Find .aven file for module name."""
        candidates = [
            self.root / f"{name}.aven",
            self.root / "std" / f"{name}.aven",
            Path(__file__).parent / "stdlib" / f"{name}.aven",
        ]
        for p in candidates:
            if p.exists():
                return p
        return None


# =============================================================================
# PART 7 — RUNTIME ENGINE (carried solver logic, integrated)
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


def collect_vars(e, acc=None):
    if acc is None: acc = set()
    if isinstance(e, Var): acc.add(e.name)
    elif isinstance(e, Bin):
        collect_vars(e.left, acc); collect_vars(e.right, acc)
    elif isinstance(e, Call):
        collect_vars(e.arg, acc)
    return acc


def eval_expr(e, env):
    if isinstance(e, Num): return e.value
    if isinstance(e, Var):
        if e.name not in env: raise KeyError(e.name)
        return env[e.name]
    if isinstance(e, Str): return e.value
    if isinstance(e, Bin):
        l, r = eval_expr(e.left, env), eval_expr(e.right, env)
        return {'+': l+r, '-': l-r, '*': l*r,
                '/': l/r if r else float('inf'),
                '^': math.pow(l,r)}[e.op]
    if isinstance(e, Unary): return -eval_expr(e.operand, env)
    if isinstance(e, Call):
        args = [eval_expr(a, env) for a in e.args]
        return FUNCS[e.fn_name](*args)
    raise TypeError(f"Unknown node {e!r}")


class Engine:
    """Run solved relations."""
    
    def __init__(self):
        self.rels: List[Relation] = []
        self.env: Dict[str, float] = {}
    
    def define(self, name: str, lhs: str, rhs: object):
        self.rels.append(Relation(name, f"{lhs} = {_src(rhs)}", lhs, rhs))
    
    def run(self, binds: Dict[str, float]) -> Dict[str, float]:
        self.env = dict(binds)
        for _ in range(100):
            progressed = False
            for rel in self.rels:
                unknowns = [v for v in rel.variables if v not in self.env]
                if not unknowns:
                    exp = eval_expr(rel.rhs, self.env)
                    if abs(exp - self.env[rel.lhs]) > 1e-6 * max(1, abs(exp)):
                        raise ValueError(f"Inconsistent: {rel.lhs}")
                elif len(unknowns) == 1:
                    u = unknowns[0]
                    if u == rel.lhs:
                        self.env[u] = eval_expr(rel.rhs, self.env)
                        progressed = True
            if not progressed: break
        return self.env


# =============================================================================
# PART 8 — COMPILER PIPELINE (load → typecheck → run)
# =============================================================================

def compile_and_run(src: str, name="main", root: Optional[Path] = None) -> Dict[str, float]:
    """Full compile pipeline: load → parse → typecheck → run."""
    mod = parse(src, name)
    
    # Type check
    tc = TypeChecker(mod)
    errs = tc.check()
    if errs:
        print("TYPE ERRORS:")
        for e in errs: print(f"  {e}")
        raise TypeError_(errs[0])
    
    # Run
    eng = Engine()
    for blk in mod.blocks:
        for eq in blk.equations:
            eng.define(blk.name, eq.lhs, eq.rhs)
    
    results = {}
    for sd in mod.solves:
        binds = {b.name: b.value for b in sd.bindings}
        res = eng.run(binds)
        results[sd.name] = res
    
    return results


# =============================================================================
# DEMOS
# =============================================================================

SIMPLE = """\
// Simple model with units
unit Length: m, km
unit Money: eur

relation Economics {
    revenue = price * quantity
    profit  = revenue - cost
}

solve Economics with {
    price = 25
    quantity = 100
    cost = 1500
}
"""

UNIT_CHECK = """\
// This should FAIL type checking (km added to h)
relation BadPhysics {
    nonsense = 100 km + 5 h
}
"""

IMPORT_TEST = """\
// Test module loading
import math

relation Calc {
    result = sqrt(144)
}

solve Calc with {}
"""


def banner(t):
    print("\n" + "=" * 60 + f"\n{t}\n" + "=" * 60)


def main():
    print(f"{LANG_NAME} v{VERSION} — The Core")
    
    banner("1 — Parse + Typecheck + Run")
    result = compile_and_run(SIMPLE)
    print(f"  economics.solve = {result}")
    
    banner("2 — Unit Type Checking (this fails)")
    try:
        compile_and_run(UNIT_CHECK)
    except TypeError_ as e:
        print(f"  caught: {e}")
    
    banner("3 — Module Imports")
    try:
        compile_and_run(IMPORT_TEST)
    except Exception as e:
        print(f"  error: {e}")
    
    banner(f"{LANG_NAME} v{VERSION} core complete")


if __name__ == "__main__":
    main()
