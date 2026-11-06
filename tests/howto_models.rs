// The follow-along models in examples/howto/ and docs/HOW_TO.md.
// A change to those files that breaks the lesson fails here.

use avenc::api::{check, solve};

fn source(name: &str) -> String {
    let path = format!("examples/howto/{name}");
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {path}: {err}"))
}

fn settled(name: &str) -> avenc::api::Solved {
    let src = source(name);
    assert!(
        check(&src).is_empty(),
        "{name} should pass check, got {:?}",
        check(&src)
    );
    solve(&src).unwrap_or_else(|err| panic!("{name} should settle: {}", err.message))
}

#[test]
fn add_settles_both_names_forward() {
    let solved = settled("01_add.aven");
    assert_eq!(solved.values["x"], 11.0);
    assert_eq!(solved.values["y"], 22.0);
    assert!(
        solved
            .trace
            .iter()
            .any(|step| step.contains("x <- forward eval")),
        "{:?}",
        solved.trace
    );
}

#[test]
fn reversed_sum_is_the_same_calculation() {
    let solved = solve("1 + 2 = x").expect("reversed equation");
    assert_eq!(solved.values["x"], 3.0);
}

#[test]
fn field_forward_computes_area() {
    let solved = settled("02_field_forward.aven");
    assert_eq!(solved.values["area"], 40.0);
    assert_eq!(solved.solving, vec!["area".to_string()]);
    assert!(
        solved
            .trace
            .iter()
            .any(|step| step.contains("area <- forward eval")),
        "{:?}",
        solved.trace
    );
}

#[test]
fn field_backward_finds_the_width() {
    let solved = settled("03_field_backward.aven");
    assert!((solved.values["width"] - 12.5).abs() < 1e-9);
    assert_eq!(solved.solving, vec!["width".to_string()]);
    assert!(solved.bound.iter().any(|name| name == "area"));
    assert!(solved.bound.iter().any(|name| name == "length"));
    assert!(
        solved
            .trace
            .iter()
            .any(|step| step.contains("width <- backward Newton")),
        "{:?}",
        solved.trace
    );
}

#[test]
fn an_unused_pin_does_not_steer_the_answer() {
    let solved = settled("04_unused_target.aven");
    assert_eq!(solved.values["area"], 40.0);
    assert_eq!(solved.values["target_area"], 100.0);
    assert!(
        solved
            .trace
            .iter()
            .all(|step| !step.contains("target_area")),
        "{:?}",
        solved.trace
    );
}

#[test]
fn a_leftover_equation_computes_the_difference() {
    let solved = settled("05_leftover.aven");
    assert_eq!(solved.values["leftover"], 3.0);
    assert!(
        solved
            .trace
            .iter()
            .any(|step| step.contains("leftover <- forward eval")),
        "{:?}",
        solved.trace
    );
}

#[test]
fn pinning_the_leftover_at_zero_finds_the_blank() {
    let solved = settled("06_leftover_zero.aven");
    assert_eq!(solved.values["leftover"], 0.0);
    assert_eq!(solved.values["expected"], 10.0);
    assert_eq!(solved.solving, vec!["expected".to_string()]);
    assert!(
        solved
            .trace
            .iter()
            .any(|step| step.contains("expected <- backward Newton")),
        "{:?}",
        solved.trace
    );
}

#[test]
fn two_hours_at_ninety_kilometres_per_hour_is_180_kilometres() {
    let solved = settled("07_trip.aven");
    assert!((solved.shown["distance"] - 180_000.0).abs() < 1e-6);
    assert_eq!(solved.units["distance"], "m");
    assert!((solved.shown["speed"] - 90.0).abs() < 1e-9);
    assert_eq!(solved.units["speed"], "km/h");
    assert!((solved.shown["hours"] - 2.0).abs() < 1e-9);
    assert_eq!(solved.units["hours"], "h");
}

#[test]
fn one_blank_too_many_does_not_settle() {
    let src = source("08_needs_another_number.aven");
    let err = solve(&src).expect_err("area and width cannot both be blank");
    let message = err.message.to_lowercase();
    assert!(
        message.contains("under") || message.contains("determin") || message.contains("unknown"),
        "{message}"
    );
}

#[test]
fn an_unknown_unit_and_a_speed_added_to_a_time_are_refused() {
    // Compounds such as mol/L and 1/s now check fine; an unknown *base* unit
    // like `furlong` is what stops the check.
    let unknown = "signal length : furlong\nrelation R {\n doubled = 2 * length\n}\nsolve R with {\n length = 1\n}\n";
    let diags = check(unknown);
    assert!(
        diags.iter().any(|d| d.message.contains("furlong")),
        "{diags:?}"
    );
    let mixed =
        "signal velocity : km/h\nsignal gust : s\nrelation Gust {\n test = velocity + gust\n}\n";
    let mixed_diags = check(mixed);
    assert!(!mixed_diags.is_empty(), "km/h + s should fail check");
    assert!(solve(mixed).is_err());
}
