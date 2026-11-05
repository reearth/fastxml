use super::*;
use crate::schema::types::ElementDef;

fn elem(name: &str, min: u32, max: Option<u32>) -> Particle {
    let mut def = ElementDef::new(name);
    def.min_occurs = min;
    def.max_occurs = max;
    Particle::Element(def)
}

fn no_subst(_: &str) -> Vec<String> {
    Vec::new()
}

fn run(automaton: &ContentAutomaton, children: &[&str]) -> Result<(), String> {
    let mut st = AutomatonState::default();
    for child in children {
        match automaton.step(&mut st, child, child, None) {
            StepResult::Matched => {}
            StepResult::TooMany { max } => {
                return Err(format!("'{child}' too many (max {max})"));
            }
            StepResult::NotExpected { expected } => {
                return Err(format!("'{child}' not expected ({expected:?})"));
            }
        }
    }
    automaton
        .finish(&st)
        .map_err(|e| format!("incomplete: {e:?}"))
}

#[test]
fn sequence_order_and_occurrence() {
    let p = Particle::Sequence {
        min: 1,
        max: Some(1),
        items: vec![
            elem("a", 1, Some(1)),
            elem("b", 0, Some(2)),
            elem("c", 1, None),
        ],
    };
    let a = build_automaton(&p, &no_subst).unwrap();
    assert!(a.upa_violation.is_none());
    assert!(run(&a, &["a", "c"]).is_ok());
    assert!(run(&a, &["a", "b", "b", "c", "c"]).is_ok());
    assert!(run(&a, &["b", "c"]).is_err(), "missing required a");
    assert!(run(&a, &["a", "b", "b", "b", "c"]).is_err(), "b too many");
    assert!(run(&a, &["a", "c", "b"]).is_err(), "b out of order");
    assert!(run(&a, &["a"]).is_err(), "missing c");
}

#[test]
fn choice_totals_across_alternatives() {
    // (a | b){2,2}: exactly two children, each either a or b.
    let p = Particle::Choice {
        min: 2,
        max: Some(2),
        items: vec![elem("a", 1, Some(1)), elem("b", 1, Some(1))],
    };
    let a = build_automaton(&p, &no_subst).unwrap();
    assert!(run(&a, &["a", "b"]).is_ok());
    assert!(run(&a, &["b", "b"]).is_ok());
    assert!(run(&a, &["a"]).is_err(), "only one of two");
    assert!(run(&a, &["a", "b", "a"]).is_err(), "three of two");
}

#[test]
fn sequence_repeats_as_unit() {
    // (a b){0,unbounded}: pairs only.
    let p = Particle::Sequence {
        min: 0,
        max: None,
        items: vec![elem("a", 1, Some(1)), elem("b", 1, Some(1))],
    };
    let a = build_automaton(&p, &no_subst).unwrap();
    assert!(run(&a, &[]).is_ok());
    assert!(run(&a, &["a", "b"]).is_ok());
    assert!(run(&a, &["a", "b", "a", "b"]).is_ok());
    assert!(run(&a, &["a", "b", "a"]).is_err(), "dangling a");
    assert!(run(&a, &["a", "a"]).is_err(), "a a is not a pair");
}

#[test]
fn nested_choice_in_sequence() {
    // a (b | c) d
    let p = Particle::Sequence {
        min: 1,
        max: Some(1),
        items: vec![
            elem("a", 1, Some(1)),
            Particle::Choice {
                min: 1,
                max: Some(1),
                items: vec![elem("b", 1, Some(1)), elem("c", 1, Some(1))],
            },
            elem("d", 1, Some(1)),
        ],
    };
    let a = build_automaton(&p, &no_subst).unwrap();
    assert!(run(&a, &["a", "b", "d"]).is_ok());
    assert!(run(&a, &["a", "c", "d"]).is_ok());
    assert!(run(&a, &["a", "d"]).is_err(), "choice missing");
    assert!(run(&a, &["a", "b", "c", "d"]).is_err(), "both alternatives");
}

#[test]
fn upa_violation_detected() {
    // (a?, a) is the classic UPA violation.
    let p = Particle::Sequence {
        min: 1,
        max: Some(1),
        items: vec![elem("a", 0, Some(1)), elem("a", 1, Some(1))],
    };
    let a = build_automaton(&p, &no_subst).unwrap();
    assert!(a.upa_violation.is_some());
}

#[test]
fn prefixed_names_without_namespace_do_not_clash() {
    // Hand-built definitions carry no namespace; their declared names
    // differ, so `a:x` and `b:x` are not the same element.
    let p = Particle::Choice {
        min: 1,
        max: Some(1),
        items: vec![elem("a:x", 1, Some(1)), elem("b:x", 1, Some(1))],
    };
    let a = build_automaton(&p, &no_subst).unwrap();
    assert!(a.upa_violation.is_none(), "{:?}", a.upa_violation);
}

#[test]
fn same_local_name_matches_only_its_own_namespace() {
    // (a:x?, b:x?) with a, b in different namespaces.
    let ns_elem = |ns: &str| {
        let mut def = ElementDef::new("x");
        def.namespace = Some(ns.to_string());
        def.min_occurs = 0;
        Particle::Element(def)
    };
    let p = Particle::Sequence {
        min: 1,
        max: Some(1),
        items: vec![ns_elem("urn:a"), ns_elem("urn:b")],
    };
    let a = build_automaton(&p, &no_subst).unwrap();
    assert!(a.upa_violation.is_none(), "{:?}", a.upa_violation);
    let run_ns = |children: &[&str]| {
        let mut st = AutomatonState::default();
        children
            .iter()
            .all(|ns| a.step(&mut st, "x", "x", Some(ns)) == StepResult::Matched)
    };
    assert!(run_ns(&["urn:a", "urn:b"]));
    assert!(run_ns(&["urn:b"]));
    assert!(!run_ns(&["urn:a", "urn:a"]), "urn:a twice");
    assert!(!run_ns(&["urn:b", "urn:a"]), "out of order");
    // A child whose namespace is unknown still matches by name.
    let mut st = AutomatonState::default();
    assert_eq!(a.step(&mut st, "x", "x", None), StepResult::Matched);
}

#[test]
fn exact_bound_above_the_cap_is_lenient() {
    // (a b){20,20}: the bound is beyond EXPAND_CAP, so it degrades to
    // "at least the capped count" and never rejects 20 pairs.
    let p = Particle::Sequence {
        min: 20,
        max: Some(20),
        items: vec![elem("a", 1, Some(1)), elem("b", 1, Some(1))],
    };
    let a = build_automaton(&p, &no_subst).unwrap();
    let twenty: Vec<&str> = ["a", "b"].iter().copied().cycle().take(40).collect();
    assert!(run(&a, &twenty).is_ok(), "{:?}", run(&a, &twenty));
    assert!(run(&a, &["a", "b", "a"]).is_err(), "dangling a");
}

#[test]
fn all_yields_no_automaton() {
    let p = Particle::All {
        min: 1,
        elements: vec![],
    };
    assert!(build_automaton(&p, &no_subst).is_none());
}
