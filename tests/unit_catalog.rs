// Units people write: litres, per-second, field measures, and compound chains.
// These go through avenc::api::solve, the same path as `avenc run`.

use avenc::api::{check, solve};
use avenc::units::UnitRegistry;

fn close(got: f64, expected: f64) {
    let tol = 1e-6 * (1.0 + expected.abs());
    assert!(
        (got - expected).abs() <= tol,
        "{got} vs {expected} (tol {tol})"
    );
}

#[test]
fn mole_per_litre_times_litres_is_moles() {
    let solved = solve(
        r#"
        signal conc : mol/L
        signal volume : L
        signal amount : mol
        relation Dose {
            amount = conc * volume
        }
        solve Dose with {
            conc = 2 mol/L
            volume = 0.5 L
        }
        "#,
    )
    .expect("mol/L");
    close(solved.shown["amount"], 1.0);
    assert_eq!(solved.units["amount"], "mol");
    close(solved.shown["conc"], 2.0);
    assert_eq!(solved.units["volume"], "L");
    close(solved.shown["volume"], 0.5);
}

#[test]
fn one_per_second_counts_events() {
    let solved = solve(
        r#"
        signal rate : 1/s
        signal duration : s
        signal count : scalar
        relation Ticks {
            count = rate * duration
        }
        solve Ticks with {
            rate = 4 1/s
            duration = 3 s
        }
        "#,
    )
    .expect("1/s");
    close(solved.shown["count"], 12.0);
    assert_eq!(solved.units["rate"], "1/s");
}

#[test]
fn inch_of_rain_on_an_acre_is_us_gallons() {
    let solved = solve(
        r#"
        signal field : acre
        signal rain : in
        signal caught : gal
        relation Rainfall {
            caught = rain * field
        }
        solve Rainfall with {
            rain = 1 in
            field = 1 acre
        }
        "#,
    )
    .expect("acre");
    let inch: f64 = 0.0254;
    let acre = 4046.8564224;
    let gallon = 231.0 * inch.powi(3);
    close(solved.shown["caught"], inch * acre / gallon);
    assert_eq!(solved.units["caught"], "gal");
    close(solved.shown["field"], 1.0);
}

#[test]
fn pounds_per_acre_and_miles_per_hour() {
    let solved = solve(
        r#"
        signal field : acre
        signal seed : lb
        signal seeding : lb/acre
        signal pace : mph
        relation Plant {
            seeding = seed / field
        }
        solve Plant with {
            seed = 50 lb
            field = 2 acre
            pace = 60 mph
        }
        "#,
    )
    .expect("lb/acre");
    close(solved.shown["seeding"], 25.0);
    assert_eq!(solved.units["seeding"], "lb/acre");
    close(solved.shown["pace"], 60.0);
    assert_eq!(solved.units["pace"], "mph");
}

#[test]
fn revolutions_per_minute_over_two_minutes() {
    let solved = solve(
        r#"
        signal spin : rpm
        signal minutes : min
        signal turns : scalar
        relation Spin {
            turns = spin * minutes
        }
        solve Spin with {
            spin = 100 rpm
            minutes = 2 min
        }
        "#,
    )
    .expect("rpm");
    close(solved.shown["turns"], 200.0);
}

#[test]
fn percent_scales_a_length_and_dbz_stays_the_number_written() {
    let solved = solve(
        r#"
        signal part : percent
        signal width : m
        signal cut : m
        signal echo : dBZ
        signal tenth : scalar
        relation Shop {
            cut = part * width
            tenth = echo / 10
        }
        solve Shop with {
            part = 50 percent
            width = 10 m
            echo = 35 dBZ
        }
        "#,
    )
    .expect("percent and dBZ");
    close(solved.shown["cut"], 5.0);
    assert_eq!(solved.units["cut"], "m");
    close(solved.shown["echo"], 35.0);
    assert_eq!(solved.units["echo"], "dBZ");
    close(solved.shown["tenth"], 3.5);
}

#[test]
fn kelvin_does_not_add_to_a_plain_number_and_dbz_does_not_add_to_metres() {
    let temp = check(
        r#"
        signal temp : K
        relation Bad {
            mixed = temp + 1
        }
        solve Bad with {
            temp = 300 K
        }
        "#,
    );
    assert!(
        temp.iter()
            .any(|d| d.message.contains("K") && d.message.contains("scalar")),
        "{temp:?}"
    );
    let radar = check(
        r#"
        signal echo : dBZ
        signal height : m
        relation Bad {
            mixed = echo + height
        }
        solve Bad with {
            echo = 35 dBZ
            height = 2 m
        }
        "#,
    );
    assert!(radar.iter().any(|d| d.message.contains("m")), "{radar:?}");
}

#[test]
fn compound_chains_match_the_named_unit() {
    let reg = UnitRegistry::new();
    let per_s = reg.lookup("1/s").expect("1/s");
    let hz = reg.lookup("Hz").expect("Hz");
    assert_eq!(per_s.dimension, hz.dimension);
    assert!((per_s.to_base - hz.to_base).abs() < 1e-12);

    let chain = reg.lookup("m/s/s").expect("m/s/s");
    let accel = reg.lookup("m/s^2").expect("m/s^2");
    assert_eq!(chain.dimension, accel.dimension);
    assert!((chain.to_base - accel.to_base).abs() < 1e-12);

    let newton = reg.lookup("N").expect("N");
    let written = reg.lookup("kg*m/s^2").expect("kg*m/s^2");
    assert_eq!(written.dimension, newton.dimension);
    assert!((written.to_base - 1.0).abs() < 1e-12);

    let pascal = reg.lookup("Pa").expect("Pa");
    let grouped = reg.lookup("kg/(m*s^2)").expect("parentheses");
    assert_eq!(grouped.dimension, pascal.dimension);
    assert!((grouped.to_base - 1.0).abs() < 1e-12);

    // * and / associate left to right, so m/s*s is a length.
    let back = reg.lookup("m/s*s").expect("m/s*s");
    let metre = reg.lookup("m").expect("m");
    assert_eq!(back.dimension, metre.dimension);

    let per_negative = reg.lookup("s^-1").expect("s^-1");
    assert_eq!(per_negative.dimension, hz.dimension);

    let mol_per_l = reg.convert(1.0, "mol/L", "mol/m^3").expect("mol/L");
    assert!((mol_per_l - 1000.0).abs() < 1e-6);

    let same_speed = reg.convert(60.0, "mph", "mi/h").expect("mph");
    assert!((same_speed - 60.0).abs() < 1e-9);

    assert!(reg.lookup("t").is_none(), "a lone t stays unrecognized");
    assert!(reg.lookup("Mb").is_none());
}

#[test]
fn parentheses_in_a_model_are_pascals() {
    let solved = solve(
        r#"
        signal pressure : kg/(m*s^2)
        relation Load {
            reading = pressure * 1
        }
        solve Load with {
            pressure = 2 kg/(m*s^2)
        }
        "#,
    )
    .expect("parentheses");
    close(solved.shown["pressure"], 2.0);
    assert_eq!(solved.units["pressure"], "kg/(m*s^2)");
    close(solved.shown["reading"], 2.0);
    assert_eq!(solved.units["reading"], "Pa");
}
