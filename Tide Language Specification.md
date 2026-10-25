# Tide — Language Specification (Draft v0.1)

*A language where you write relationships, not instructions—and time is a dimension of every value.*

---

## 1. Why "Tide"?

A tide is water that moves in cycles, predictably, in *both* directions. That's the whole thesis:

1. Computation should be **reversible** — you state relationships, the machine solves them in whatever direction you need.
2. Every value lives **in time** — knowing *when* something was true matters as much as knowing it was true.
3. Programs should **flow**, not march — no rigid instruction order, just a network of facts settling into consistency.

---

## 2. The Three Pillars

### Pillar 1 — Relations, Not Functions

You don't write functions that take inputs and produce outputs. You write **relations**: statements of mathematical truth. Direction is inferred.

```tide
relation Motion {
    distance = speed × time
}
```

This single definition gives you a solver, a calculator, and a consistency checker:

```tide
solve Motion with {
    speed   = 60 km/h
    time    = 2 h
}
# ⟹ distance = 120 km

solve Motion with {
    distance = 450 km
    speed    = 90 km/h
}
# ⟹ time = 5 h

solve Motion with {
    distance = 100 km
    speed    = 50 km/h
    time     = 3 h
}
# ⟹ inconsistency: relation asserts time = 2 h, given = 3 h
```

Units are structural, not comments. `km/h × h = km` is checked at compile time.

**Why this matters:** most real software is arithmetic wearing a trench coat. Spreadsheets figured this out decades ago—cells declare relationships, not execution order—and they conquered the world. Tide is spreadsheet semantics as a general-purpose language.

### Pillar 2 — Time Is a Type Axis

Every value in Tide has an implicit temporal extent. A `Price` isn't a number; it's a number *valid over an interval*. The type system tracks this:

```tide
# A static value: valid forever
π: Float = 3.14159…

# A signal: a value that varies over time
room_temp: Signal<Celsius>   # has a history AND a present

# Temporal queries are native
avg(room_temp over last(24 h))
room_temp when window_open      # value of temp during episodes where window was open
last_known(gps_fix)             # most recent value, with staleness info
```

When two signals meet, the compiler checks temporal compatibility. You can't accidentally multiply a 2019 tax rate by 2024 income—unless you explicitly say that's what you mean:

```tide
tax_rate.at(income.when)  # deliberate, visible
```

### Pillar 3 — Rewind Is Semantics, Not Tooling

Because every computation is a settled network of facts (not a destroyed stack of instructions), Tide programs are **intrinsically reversible**. Any running program can be paused, rewound to any earlier state, edited, and resumed—guaranteed by the language semantics, deterministically.

```tide
debugger: rewind to @before(save_clicked) where user.name = "Ada"
# modify input, resume
```

Other languages bolt time-travel debuggers on as a tool. In Tide it's free, because it's what the program *is*: an append-only ledger of settling facts.

---

## 3. Core Language Tour

### 3.1 Relations compose

```tide
relation Trip {
    include Motion
    fuel_used = distance × consumption_per_km
    cost      = fuel_used × price_of_fuel
}

solve Trip with {
    speed            = 90 km/h
    time             = 4 h
    consumption      = 0.07 L/km
    price_of_fuel    = 1.75 €/L
}
# ⟹ distance = 360 km, fuel_used = 25.2 L, cost = 44.10 €
```

Change one input, everything downstream re-settles. Instantly. That's not reactivity bolted on—that's the base model.

### 3.2 Collections are relations over sets

```tide
relation FleetSummary {
    fleet_distance = sum(car.distance over cars)
    worst_mpg       = min(car.consumption over cars)
}

# Filtering is a temporal-ish selection, declaratively
overdue = invoice where invoice.due_at < now
```

### 3.3 Effects live at the edges

Pure relation-solving fills the core. The impure world (network, disk, UI) is confined to **sources** and **sinks**:

```tide
source keyboard: Event<Keystroke>
sink screen: Canvas

relation Typewriter {
    text = accumulate(keyboard)
    screen.draw(text.at(cursor))
}
```

This keeps programs debuggable and rewound-able: effects are checkpoints at the boundary, so rewinding never "un-sends" an email unless the sink protocol allows it.

### 3.4 Escape ladder (performance tiers)

| Tier | Usage | Cost |
|------|-------|------|
| 0 — Solver | Default: omnidirectional relations | Compile-time specialization makes the common case as fast as a normal function call |
| 1 — Direction pinning | `forward Motion` | Compiler emits one-way code; reverse becomes an explicit error |
| 2 — Locality | `@region` blocks | Manual memory regions, C-speed arithmetic |
| 3 — Foreign | `extern "C"` | Drop to C ABI |

**Key claim:** in Tide, `distance = speed × time` with pinned direction compiles to *exactly* the same machine code as the equivalent C. Generality is paid only when generality is used. The solver machinery is compiled out.

---

## 4. How It Stays Fast (the part everyone gets wrong)

The graveyard of relational/reactive languages is performance. Tide's strategy:

1. **Static direction inference.** 95% of relation usage has a known direction at compile time (this is the "spreadsheet compiled to a binary" case). The solver is dead code eliminated.
2. **Solve-spans, not reactive redraw.** When direction is dynamic, Tide doesn't re-evaluate the whole graph—only the minimal unsettled span, like incremental constraint solvers but with tighter compilation.
3. **Temporal values as ring buffers.** Signals are stored as chunked immutable logs—lock-free reads for concurrent consumers, memory-bounded by declared retention windows.
4. **No GC.** Regions + escape analysis at tier 0/1; arena allocation in solver spans.

Realistic target: within 10% of C for pinned-direction numeric code; within 2× of Go for full dynamic solving—while never giving up the rewind guarantee.

---

## 5. Ease of Learning

- **The spec is ~30 pages.** The core surface is: relations, `solve`, signals, sources/sinks, `over`/`when`. That's most of it.
- **First lesson works with algebra.** A 12-year-old who knows `speed = distance / time` can already write working Tide. The language *looks like what they learned in math class*.
- **Errors are contradictions, stated as such.** "You told me `time = 3 h`, but Motion implies 2 h—here are both derivation chains" is checkable, explainable, and teachable.
- **The debugger teaches.** Rewind-anywhere means every crash is a reproducible lab experiment. You don't read a stack trace; you scroll back through causality.

---

## 6. Honest Limits (because there are always limits)

1. **Some problems are genuinely irreversible.** Hashing, randomness, destructive IO. Tide handles these with explicit `irreversible` blocks—you can't rewind past one, and the type system makes that visible. It's a wart, but a labeled one.
2. **Omnidirectionality can be exponentially expensive.** The solver is only pleasant on relations that are locally solvable (trees, mostly). Tide ships with cycle-detection that forces you to pin direction on nasty graphs. Constraint-solving is P-hard in general; pretending otherwise is lying.
3. **Existing prior art, and how Tide differs.** Spreadsheet engines (Excel) do bidirectional-ish evaluation but only in a grid, untyped, with no compilation story. Logic languages (Prolog, miniKanren) do relations but with performance cliffs and dated tooling. FRP languages (Elm, Fran) do signals but one-directionally. The novelty of Tide is fusing all three behind **compile-time specialization** so you only pay for generality when you use it—plus units and time as type-level dimensions, which to my knowledge no mainstream language does.
4. **Bootstrapping the ecosystem is the real boss fight.** Transpile to C first, steal C libraries via `extern`, ship the debugger as the killer app—the one thing incumbents genuinely cannot retrofit.

---

## 7. Hello, World — Done Properly

```tide
relation Greeter {
    greeting = "Hello, " ++ name ++ "!"
    screen.show(greeting when greeting.changed)
}

solve Greeter with { name = "world" }
# ⟹ screen: Hello, world!
```

Note what's absent: no main function, no event loop, no callback, no build boilerplate. The program *is* the invariant; the runtime finds the way to satisfy it.

---

*Tide v0.1 — relations settle; time flows; and you can always row back.*
