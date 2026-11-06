# Units

A unit is written after the number, with no brackets: `rain = 1 in`, `signal conc : mol/L`. Aven converts that number into a base unit before solving, and shows the result again in the unit you declared.

The base units are the metre, second, kilogram, ampere, kelvin, and mole, plus the radian, the bit, the transaction, and the US dollar. A kilometre is stored as metres. A gallon is stored as cubic metres. The equation itself is the same either way.

You can multiply and divide different kinds of quantity. An inch times an acre is a volume. You cannot add different kinds. A temperature in kelvin does not add to a plain number, and a speed does not add to a time.

Every symbol below is built in. The [cheat sheet](CHEAT_SHEET.md) is the short table. This page says what each one is.

## A field and a bench

`examples/catalog_units.aven` is one afternoon: an inch of rain on two acres, fifty pounds of seed on that ground, a half-litre at 2 mol/L, and four events per second for three seconds.

```bash
avenc run examples/catalog_units.aven
```
```text
Solver Trace:
  [caught = rain * field] caught <- forward eval
  [seeding = seed / field] seeding <- forward eval
  [amount = conc * volume] amount <- forward eval
  [count = rate * duration] count <- forward eval

Settled Environment:
  amount           = 1.0000 mol
  caught           = 54308.5714 gal
  conc             = 2.0000 mol/L
  count            = 12.0000 scalar
  duration         = 3.0000 s
  field            = 2.0000 acre
  rain             = 1.0000 in
  rate             = 4.0000 1/s
  seed             = 50.0000 lb
  seeding          = 25.0000 lb/acre
  volume           = 0.5000 L

Fact Ledger: 11 facts settled.
```

One inch of rain on one acre is half of that catch: 27,154.2857 US gallons. Fifty pounds on two acres is 25 lb/acre. Two moles per litre in half a litre is one mole. Four per second for three seconds is twelve.

## Length

| Symbol | What one of them is |
|:---|:---|
| `m` | The metre. This is the base length. |
| `km` | A kilometre, 1,000 m. |
| `cm` | A centimetre, 0.01 m. |
| `mm` | A millimetre, 0.001 m. A millimetre of rain is a depth, which is a length. |
| `um` | A micrometre, 0.000001 m. The letter u stands in for µ. |
| `in` | An international inch, exactly 0.0254 m. |
| `ft` | A foot, exactly 0.3048 m. Twelve inches. |
| `mi` | A statute mile, exactly 1,609.344 m. |
| `nmi` | A nautical mile, exactly 1,852 m. The mile on a chart. |

## Area

| Symbol | What one of them is |
|:---|:---|
| `acre` | The international acre, exactly 4,046.8564224 m². Land surveys in the United States sometimes use a survey acre, which is slightly larger. This symbol is the international acre. |
| `ha` | A hectare, exactly 10,000 m². A square 100 m on a side. |

A square length is written with a power: `m^2`, `ft^2`. `wing_area = 32 m^2` is a square measure.

## Volume

| Symbol | What one of them is |
|:---|:---|
| `L` | A litre, exactly 0.001 m³. A cube 10 cm on a side. `l` is the same litre. |
| `mL` | A millilitre, 0.001 L. |
| `uL` | A microlitre, 0.000001 L. |
| `gal` | A US liquid gallon, exactly 231 cubic inches. The imperial gallon is a different size and is not this symbol. |
| `bu` | A US bushel, exactly 2,150.42 cubic inches. The imperial bushel is not this symbol. |

`mol/L` is moles per litre. One mol/L is 1,000 mol/m³.

## Mass

| Symbol | What one of them is |
|:---|:---|
| `kg` | The kilogram. This is the base mass. |
| `g` | A gram, 0.001 kg. |
| `mg` | A milligram, 0.000001 kg. |
| `lb` | An avoirdupois pound, exactly 0.45359237 kg. |
| `oz` | An avoirdupois ounce, one sixteenth of a pound. A fluid ounce is a volume; write that in `mL`. |
| `ton` | A US short ton, 2,000 pounds. The ton on a US truck scale. |
| `tonne` | A metric tonne, 1,000 kg. Write the word `tonne`. A lone `t` is left unrecognized so it can stay a name in a model. |

## Time

| Symbol | What one of them is |
|:---|:---|
| `s` | The second. This is the base time. |
| `ms` | A millisecond, 0.001 s. |
| `us` | A microsecond, 0.000001 s. This is a time, not a country. |
| `ns` | A nanosecond, 0.000000001 s. |
| `min` | A minute, 60 s. |
| `h` | An hour, 3,600 s. |
| `hr` | An hour, the same size as `h`. |
| `day` | A day, 86,400 s. No leap seconds. |

`min` is the minute. A metre times an inch is `m*in`. Writing `m in` with a space is joined into the letters `min`.

## Speed and how often

| Symbol | What one of them is |
|:---|:---|
| `mph` | A mile per hour, exactly 0.44704 m/s. The same quantity as `mi/h`. |
| `Hz` | One per second. |
| `kHz` | A thousand per second. |
| `MHz` | A million per second. |
| `GHz` | A billion per second. |
| `rpm` | A turn per minute, 1/60 Hz. A turn is a count. For radians per second, multiply by `2 * pi` yourself. |

`km/h`, `m/s`, and `1/s` are compounds, described below. `1/s` is the same quantity as `Hz`.

## Temperature and the weather report

| Symbol | What one of them is |
|:---|:---|
| `K` | The kelvin. This is the base temperature. A difference of one kelvin is the size to use for a temperature change. |
| `degC` | Degrees Celsius. The steps are the same size as kelvin, and 0 °C is 273.15 K. Write `degC`. The degree sign is not a symbol. |
| `degF` | Degrees Fahrenheit. A step is 5/9 of a kelvin, and 0 °F sits at 255.372222… K. |
| `hPa` | A hectopascal, 100 Pa. The unit on a station-pressure report. |
| `dBZ` | A radar reflectivity written as a decibel number. 35 dBZ stays 35. Aven does not convert it to linear Z. |
| `dB` | A decibel label. The number stays as written. It is not converted into a power ratio. |
| `dBm` | A decibel-milliwatt label, kept as written the same way. |

`degC` and `degF` are absolute temperatures. A rate such as `degC/s` drops the offset, because a change of one degree per second is the same size as one kelvin per second.

A plain number is dimensionless. Kelvin is a temperature. This model is rejected:

```aven-error
signal temp : K
relation Bad {
    mixed = temp + 1
}
solve Bad with {
    temp = 300 K
}
```

A formula whose constants were written for Celsius degrees, such as a crop coefficient times a thermometer reading, wants the plain number from the thermometer. Declare the signal in `degC` when the equation is written in kelvin.

`dBZ` adds to another plain number, because both are dimensionless. It does not add to a height in metres.

## The lab

| Symbol | What one of them is |
|:---|:---|
| `mol` | The mole. This is the base amount of substance. |
| `percent` | One hundredth of a whole. `50 percent` is 0.5. |
| `ppm` | Parts per million, 0.000001 of a whole. |
| `ppb` | Parts per billion, 0.000000001 of a whole. |
| `scalar` | A pure number: a coefficient, an efficiency, a count. |
| `ratio` | The same kind of pure number as `scalar`. |

`percent`, `ppm`, and `ppb` scale whatever they multiply. Fifty percent of a 10 m board is 5 m. They do not add to a length.

Concentration is `mol/L`. A first-order rate is often `1/s`. Both are compounds of units on this page.

The name `log10` passes the checker, and the solver has no such function. A base-10 logarithm is `ln(x) / ln(10)`.

```aven
signal conc : mol/L
signal volume : L
relation Dose {
    amount = conc * volume
}
solve Dose with {
    conc = 2 mol/L
    volume = 0.5 L
}
```

## Force, pressure, energy, electricity

These are the named quantities. Each one is also a compound of kilograms, metres, and seconds, and that compound is accepted too. One newton is `kg*m/s^2`. One pascal is `kg/(m*s^2)`.

| Symbol | What one of them is |
|:---|:---|
| `N` | A newton. The base force. |
| `kN` | A kilonewton, 1,000 N. |
| `Pa` | A pascal, one newton on a square metre. |
| `kPa` | A kilopascal, 1,000 Pa. |
| `MPa` | A megapascal, 1,000,000 Pa. |
| `bar` | 100,000 Pa. |
| `atm` | One standard atmosphere, 101,325 Pa. |
| `psi` | A pound-force per square inch, about 6,894.757 Pa. Shop air and tire pressure. |
| `J` | A joule. The base energy. |
| `kJ` | A kilojoule, 1,000 J. |
| `MJ` | A megajoule, 1,000,000 J. |
| `Wh` | A watt-hour, 3,600 J. |
| `kWh` | A kilowatt-hour, 3,600,000 J. The same quantity as `kW*h`. |
| `MWh` | A megawatt-hour, 3,600,000,000 J. |
| `W` | A watt. The base power. |
| `mW` | A milliwatt, 0.001 W. |
| `kW` | A kilowatt, 1,000 W. |
| `MW` | A megawatt, 1,000,000 W. |
| `A` | An ampere. The base electric current. |
| `mA` | A milliampere, 0.001 A. |
| `V` | A volt. |
| `mV` | A millivolt, 0.001 V. |
| `kV` | A kilovolt, 1,000 V. |
| `ohm` | An ohm. |

## Angle

| Symbol | What one of them is |
|:---|:---|
| `rad` | A radian. This is the base angle. `sin`, `cos`, and `tan` receive radians. |
| `deg` | A degree, π/180 radians. A signal declared `deg` is converted before those functions run. |

## Data, jobs, and money

| Symbol | What one of them is |
|:---|:---|
| `b`, `bit` | A bit. This is the base amount of data. |
| `B`, `byte`, `bytes` | A byte, 8 bits. |
| `kbit`, `Mbit`, `Gbit`, `Tbit` | 10³, 10⁶, 10⁹, and 10¹² bits. |
| `kB`, `KB` | 1,000 bytes. Both symbols are that size. |
| `MB`, `GB`, `TB`, `PB` | 10⁶, 10⁹, 10¹², and 10¹⁵ bytes. |
| `KiB`, `MiB`, `GiB`, `TiB` | 1,024, 1,024², 1,024³, and 1,024⁴ bytes. |
| `bps`, `kbps`, `Mbps`, `Gbps`, `Tbps` | Bits per second, with the same thousands as the bit symbols. |
| `req` | One request. |
| `iop` | One I/O operation. |
| `pkt` | One packet. |
| `tx` | One transaction. |
| `iops` | One I/O operation per second. |
| `rps` | One request per second. |
| `pph` | One part per hour. |
| `$`, `USD` | A US dollar. This is the base money. |
| `EUR` | A euro. Its own dimension. There is no built-in exchange rate. |
| `GBP` | A pound sterling. Its own dimension, same rule as the euro. |

`b` is a bit and `B` is a byte. A spelling that could be either, such as `Mb` or `kb`, is rejected and the message lists the symbols to write instead. `MW` is a megawatt and `mW` is a milliwatt. Another casing is accepted only when it means one symbol: `gbps` is `Gbps`.

## Compound units

A compound is built from the symbols on this page with `/`, `*`, and an integer power `^`. Parentheses group. `*` and `/` run left to right, and `^` binds tighter.

| You write | It means |
|:---|:---|
| `km/h` | Kilometres per hour. |
| `mi/h` | The same quantity as `mph`. |
| `m/s^2` | Metres per second squared. |
| `m/s/s` | The same acceleration. Each `/` divides again. |
| `1/s` | Per second. The `1` is a plain numerator, and the quantity matches `Hz`. |
| `s^-1` | Per second, written as a negative power. |
| `mol/L` | Moles per litre. |
| `lb/acre` | Pounds per acre. |
| `bu/acre` | Bushels per acre. |
| `kg/m^3` | Kilograms per cubic metre. |
| `kg*m/s^2` | A newton. |
| `kg/(m*s^2)` | A pascal. The parentheses keep the seconds in the denominator. |
| `kW*h` | A kilowatt-hour. |
| `Pa/m` | Pascals per metre. |
| `mm/h` | Millimetres per hour. A rainfall rate. |

`m/s*s` is left to right, so it is a length again: metres per second, times seconds. Write `m/s^2`, `m/s/s`, or `m/(s*s)` for an acceleration.

Spaces between the pieces are fine: `mol / L` is `mol/L`. A space with no `*` joins the letters, so `m s` becomes the millisecond symbol `ms`. Write `m*s` for metre-seconds.

A temperature offset is dropped inside a compound. `degC/s` is a rate of change.

## A name of your own

```text
unit moisture : gravimetric, volumetric
```

That adds `gravimetric` and `volumetric` as units of one new dimension. They convert only among themselves, at the same scale. They do not become kilograms or cubic metres. A new name placed under a built-in dimension, such as `unit length : furlong`, gets a dimension of its own rather than silently equalling a metre.

## What the checker still rejects

- Adding a kelvin to a plain number, or a speed to a time.
- A lone `t`. Write `tonne` for 1,000 kg or `ton` for the US short ton.
- A fluid ounce, an imperial gallon, an imperial bushel, a British long ton, and a US survey acre. The symbols `gal`, `bu`, `ton`, and `acre` are the sizes in the tables above.
- The degree sign and the µ sign. Write `degC` and `um`.
- `log10` as a function the solver can run. Write `ln(x) / ln(10)`.
- A spelling that could be bits or bytes, such as `Mb`.
