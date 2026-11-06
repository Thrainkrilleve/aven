# How to write a model and check it

Follow this page from the top. Each finished model is a file in `examples/howto/`. The command under a file is what you run. The numbers under that command are what a correct run prints. When your numbers match, and the trace names the variable you meant to find, the file is doing what you think it is doing.

Build the tool once, from the repository:

```bash
cargo build --release
```

Every command below assumes that build. You can also run `avenc` if you have installed it.

## The two commands

`avenc check` reads the file. A finished file ends with this line:

```text
✓ System Determinacy PASSED: All relation blocks have 0 degrees of freedom.
```

`avenc run` prints a trace and then the numbers. Read the trace before the numbers. It names the variable that was found, and it says how:

- `forward eval` means the name on the left of an equation was computed from names you already knew.
- `backward Newton` means a name on the right was the blank, and Aven found it from the name on the left.

Two parts make a model:

- A `relation` is the rule. It stays true whichever number is the blank.
- A `solve` block is the list of numbers you already know. Any name in the rule that you leave off that list is a name Aven tries to find.

A short calculation does not need either block. The name on the left of `=` is the result.

Every name that a `relation` uses must have a unit. You give it one with a `signal` line — `signal area : m^2` — and Aven infers the rest from there, so once `length` and `width` are declared in metres, `area` is known to be in `m^2` without a line of its own. When a quantity is a plain count or ratio with no physical unit, declare it `: scalar`. A relation that uses a name Aven cannot pin a unit to fails `avenc check` with a message telling you which name to declare. The bare calculations above need no `signal` line because they are not inside a relation.

## 1. Add two numbers

`examples/howto/01_add.aven`:

```aven
// The name on the left receives the number.
x = 1 + 10
y = x * 2
```

```bash
./target/release/avenc run examples/howto/01_add.aven
```

```text
Solver Trace:
  [x = 1 + 10] x <- forward eval
  [y = x * 2] y <- forward eval

Settled Environment:
  x                = 11.0000
  y                = 22.0000

Fact Ledger: 2 facts settled.
```

Hand check: 1 + 10 is 11, and 11 × 2 is 22. Both lines say `forward eval`, because each result sits on the left and the right-hand side was already known.

Writing the result on the right is the same calculation. `1 + 2 = x` settles `x` at 3.

```aven
1 + 2 = x
```

## 2. A rule, solved forward

You know the length and the width of a field. Area is the blank.

`examples/howto/02_field_forward.aven`:

```aven
// You know the length and the width. Area is the blank, so it is computed.
signal area   : m^2
signal length : m
signal width  : m

relation Field {
    area = length * width
}

solve Field with {
    length = 8
    width = 5
}
```

```bash
./target/release/avenc run examples/howto/02_field_forward.aven
```

```text
Solver Trace:
  [area = length * width] area <- forward eval

Settled Environment:
  area             = 40.0000 m^2
  length           = 8.0000 m
  width            = 5.0000 m

Fact Ledger: 3 facts settled.
```

Hand check: 8 × 5 = 40. The trace says `area <- forward eval`.

## 3. The same rule, solved backward

You know the area you want, and you know the length. Leave width out of the solve block.

`examples/howto/03_field_backward.aven`:

```aven
// You know the area and the length. Width is left out, so it is found.
signal area   : m^2
signal length : m
signal width  : m

relation Field {
    area = length * width
}

solve Field with {
    length = 8
    area = 100
}
```

```bash
./target/release/avenc run examples/howto/03_field_backward.aven
```

```text
Solver Trace:
  [area = length * width] width <- backward Newton

Settled Environment:
  area             = 100.0000 m^2
  length           = 8.0000 m
  width            = 12.5000 m

Fact Ledger: 3 facts settled.
```

Hand check: 100 / 8 = 12.5. The trace says `width <- backward Newton`. You did not write a second equation for width. The same rule was solved the other way because width was the name missing from the solve block.

Ask why that number is there:

```bash
./target/release/avenc why examples/howto/03_field_backward.aven width
```

```text
Causal Derivation Tree for 'width':
#3 width = 12.5000  ; solved from area = length * width
  #2 area = 100.0000  ; user binding
  #1 length = 8.0000  ; user binding
```

To flip direction yourself, delete the line for the name you want found, and add a line for the name that used to be the blank, using the number the last run printed.

## 4. A number the rule never reads

Pinning a name does not make it a goal. The name has to appear in the relation.

`examples/howto/04_unused_target.aven` pins `target_area = 100` and also pins length and width:

```aven
// target_area is pinned, but it never appears in the relation.
// The area is still length times width.
signal area   : m^2
signal length : m
signal width  : m

relation Field {
    area = length * width
}

solve Field with {
    length = 8
    width = 5
    target_area = 100
}
```

```bash
./target/release/avenc run examples/howto/04_unused_target.aven
```

```text
Solver Trace:
  [area = length * width] area <- forward eval

Settled Environment:
  area             = 40.0000 m^2
  length           = 8.0000 m
  target_area      = 100.0000
  width            = 5.0000 m

Fact Ledger: 4 facts settled.
```

Area is 40. `target_area` is still 100, and the trace never mentions it. The pin was stored and printed. To make the area 100, put `area = 100` in the solve block and leave width out, which is the file in section 3.

## 5. A leftover you want to be zero

This equation computes a leftover. With both measurements pinned, the leftover comes out as 3.

`examples/howto/05_leftover.aven`:

```aven
// This line computes leftover. It does not force leftover to be zero.
signal leftover : scalar
signal measured : scalar
signal expected : scalar

relation Gap {
    leftover = measured - expected
}

solve Gap with {
    measured = 10
    expected = 7
}
```

```bash
./target/release/avenc run examples/howto/05_leftover.aven
```

```text
Solver Trace:
  [leftover = measured - expected] leftover <- forward eval

Settled Environment:
  expected         = 7.0000 scalar
  leftover         = 3.0000 scalar
  measured         = 10.0000 scalar

Fact Ledger: 3 facts settled.
```

To make the leftover come out as zero, pin `leftover = 0` and leave `expected` out of the solve block. The blank is the name Aven finds.

`examples/howto/06_leftover_zero.aven`:

```aven
// leftover is pinned at 0, and expected is the blank, so expected is found.
signal leftover : scalar
signal measured : scalar
signal expected : scalar

relation Gap {
    leftover = measured - expected
}

solve Gap with {
    measured = 10
    leftover = 0
}
```

```bash
./target/release/avenc run examples/howto/06_leftover_zero.aven
```

```text
Solver Trace:
  [leftover = measured - expected] expected <- backward Newton

Settled Environment:
  expected         = 10.0000 scalar
  leftover         = 0.0000 scalar
  measured         = 10.0000 scalar

Fact Ledger: 3 facts settled.
```

Hand check: 10 minus expected is 0, so expected is 10. The trace says `expected <- backward Newton`.

## 6. One blank too many

`examples/howto/08_needs_another_number.aven` knows the length and leaves both area and width blank. That is one unknown too many. `avenc check` prints `UNDER-DETERMINED SYSTEM` and tells you to bind `area` or `width`. `avenc run` stops. A file in that state is not ready to be a test.

```aven-fragment
// Length is known. Area and width are both blank, so this cannot settle.
signal area   : m^2
signal length : m
signal width  : m

relation Field {
    area = length * width
}

solve Field with {
    length = 8
}
```

```bash
# This run is supposed to stop.
./target/release/avenc run examples/howto/08_needs_another_number.aven
```

## 7. Units

Write the unit after the number, with no brackets: `90 km/h`. A `signal` line says which unit a result is shown in.

`examples/howto/07_trip.aven` is 90 kilometres per hour for 2 hours. The distance is 180 kilometres, reported in metres because the signal says `m`.

```aven
// The unit follows the number. Distance is shown in the unit on its signal.
signal distance : m
signal speed : km/h
signal hours : h

relation Trip {
    distance = speed * hours
}

solve Trip with {
    speed = 90 km/h
    hours = 2 h
}
```

```bash
./target/release/avenc run examples/howto/07_trip.aven
```

```text
Solver Trace:
  [distance = speed * hours] distance <- forward eval

Settled Environment:
  distance         = 180000.0000 m
  hours            = 2.0000 h
  speed            = 90.0000 km/h

Fact Ledger: 3 facts settled.
```

Hand check: 90 km/h × 2 h = 180 km = 180,000 m.

Aven refuses to add a speed to a time. This file fails `avenc check`:

```aven-error
signal velocity : km/h
signal gust : s

relation Gust {
    test = velocity + gust
}
```

The full unit table is in [the cheat sheet](CHEAT_SHEET.md). These are the spellings the models on this page use, plus the ones people reach for next: `m`, `km`, `s`, `h`, `kg`, `lb`, `km/h`, `m/s`, `kg/m^3`, `N`, `Pa`, `degC`, `K`, `Hz`, `mol`, `scalar`. A compound is two known pieces joined by `/` or `*`, and a power is `^`, as in `m/s^2` and `kg/m^3`.

Compounds built from known pieces work — `mol/L`, `1/s`, and chains like `m/s/s` all check fine. What is rejected is an unknown *base* unit, so a test file that reaches for one will not settle. `furlong`, for example, is not built in:

```aven-error
signal length : furlong

relation R {
    doubled = 2 * length
}

solve R with {
    length = 1
}
```

Very small numbers print with four decimal places, so a value under 0.00005 shows as `0.0000`. Trust the trace, and trust a number you can check by hand.

## 8. In the desktop app

The app opens on Newton's second law: force is 4,200, acceleration is 5, and mass is the blank, so mass solves to 840. Change the force box to 8400. Mass becomes 1,680. That is section 3 with different numbers: the blank is still the name missing from the solve block.

**Solve for this**, on a number you pinned, moves that name out of the solve block and pins the name that was blank. The trace then says `backward Newton` on the name you chose.

## 9. Checklist before you trust the file

Use this list on every model you are about to treat as a test.

1. `avenc check` ends with `0 degrees of freedom`. An `UNDER-DETERMINED SYSTEM` box means the file still needs one of the bindings it names.
2. Every number you meant as a goal appears in the relation. A name that exists only in the solve block is printed unchanged. Section 4 is that case: `target_area` stayed 100 while area stayed 40.
3. The name you wanted found is absent from the solve block.
4. The trace names that variable. It says `backward Newton` when you meant backward, and `forward eval` when you meant forward.
5. One settled number matches a hand calculation. For the field, that is 100 / 8 = 12.5.
6. Change one input, run again, and the answer moves the way the rule says. Changing length from 8 to 10 in the backward field makes width 10.

When all six hold, the file is a valid test of the model you wrote.

Next, [Aven by Example](TUTORIAL.md) goes on to optimization, checks, and larger models. [Getting Started](GETTING_STARTED.md) is the short install path for someone who already writes code.
