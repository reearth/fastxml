//! Symbol-bound content-model automaton.
//!
//! A [`ContentAutomaton`]'s element positions match by probing
//! `FxHashSet<String>` name sets — one or two string hashes per candidate
//! transition per child element. [`BoundAutomaton`] binds each position's
//! name set to a sorted slice of interned [`SymbolId`]s once per automaton,
//! so the per-element match becomes integer binary searches.
//!
//! The step control flow itself is **shared, not replicated**: `step_syms`
//! drives [`ContentAutomaton::step_matched`], the same implementation that
//! backs the string-based [`ContentAutomaton::step`], with only the matcher
//! predicate swapped. Failure paths (expected-list construction, `finish`)
//! never consult the matcher, so error messages are byte-identical to the
//! string path.
//!
//! Labels contain only schema-declared names (the declared name, its local
//! part, and substitution-group member names — exactly the automaton's
//! `names` sets). Binding interns them into the validator's symbol table, so
//! a document name matches iff it interns to one of these symbols, i.e. iff
//! it is string-equal — correct by construction.

use std::sync::Arc;

use crate::schema::types::WildcardConstraint;
use crate::schema::xsd::content_automaton::{
    AutomatonState, ContentAutomaton, PosMatcher, StepResult,
};

use super::symbols::{SymbolId, SymbolTable};

/// What a bound position matches.
pub(crate) enum BoundMatcher {
    /// An element particle: matched when the child's qname or local symbol
    /// is in the sorted symbol slice.
    Element {
        /// Interned symbols of every name in the position's name set,
        /// sorted for binary search.
        syms: Box<[SymbolId]>,
    },
    /// An `xs:any` wildcard, matched by namespace (unchanged semantics).
    Wildcard(Arc<WildcardConstraint>),
}

/// A [`ContentAutomaton`] with positions bound to interned symbols.
pub(crate) struct BoundAutomaton {
    /// The wrapped automaton (drives control flow, `finish`, and error
    /// construction).
    auto: Arc<ContentAutomaton>,
    /// Bound matcher per position (parallel to `auto.matchers()`).
    bound: Vec<BoundMatcher>,
}

impl BoundAutomaton {
    /// Binds every position's name set into `symbols`. Called once per
    /// distinct automaton per validator run.
    pub fn bind(auto: Arc<ContentAutomaton>, symbols: &mut SymbolTable) -> Self {
        let bound = auto
            .matchers()
            .iter()
            .map(|m| match m {
                PosMatcher::Element { names, .. } => {
                    let mut syms: Vec<SymbolId> = names.iter().map(|n| symbols.intern(n)).collect();
                    syms.sort_unstable();
                    syms.dedup();
                    BoundMatcher::Element {
                        syms: syms.into_boxed_slice(),
                    }
                }
                PosMatcher::Wildcard(wc) => BoundMatcher::Wildcard(Arc::clone(wc)),
            })
            .collect();
        Self { auto, bound }
    }

    /// Feeds one child element identified by its interned qname and local
    /// symbols. Semantics are identical to [`ContentAutomaton::step`] with
    /// the corresponding strings: both instantiate the same
    /// `step_matched` control flow, differing only in how a position
    /// matches (symbol binary search vs string-set probe).
    pub fn step_syms(
        &self,
        st: &mut AutomatonState,
        qname_sym: SymbolId,
        local_sym: SymbolId,
        ns: Option<&str>,
    ) -> StepResult {
        self.auto.step_matched(st, |pos| match &self.bound[pos] {
            BoundMatcher::Element { syms } => {
                syms.binary_search(&qname_sym).is_ok()
                    || (local_sym != qname_sym && syms.binary_search(&local_sym).is_ok())
            }
            BoundMatcher::Wildcard(wc) => wc.matches(ns),
        })
    }

    /// The wrapped string-based automaton (used by the equivalence tests;
    /// the validator's `finish` path keeps using the schema's automaton
    /// directly).
    #[cfg(test)]
    pub fn inner(&self) -> &ContentAutomaton {
        &self.auto
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::types::{ElementDef, Particle};
    use crate::schema::xsd::content_automaton::build_automaton;

    fn elem(name: &str, min: u32, max: Option<u32>) -> Particle {
        let mut def = ElementDef::new(name);
        def.min_occurs = min;
        def.max_occurs = max;
        Particle::Element(def)
    }

    fn no_subst(_: &str) -> Vec<String> {
        Vec::new()
    }

    /// Runs `children` through BOTH the string `step` and the bound
    /// `step_syms`, asserting identical results (variant, payload, and the
    /// resulting configuration set) at every step, then `finish` through the
    /// wrapped automaton.
    fn run_both_arc(auto: &Arc<ContentAutomaton>, children: &[&str]) -> Result<(), String> {
        let mut symbols = SymbolTable::default();
        let bound = BoundAutomaton::bind(Arc::clone(auto), &mut symbols);

        let mut st_str = AutomatonState::default();
        let mut st_sym = AutomatonState::default();
        for child in children {
            let qsym = symbols.intern(child);
            let lsym = symbols.local(qsym);
            let local: &str = child.rsplit(':').next().unwrap_or(child);

            let r_str = auto.step(&mut st_str, child, local, None);
            let r_sym = bound.step_syms(&mut st_sym, qsym, lsym, None);
            assert_eq!(
                r_str, r_sym,
                "step vs step_syms diverged on child '{child}'"
            );
            assert_eq!(
                st_str, st_sym,
                "configuration sets diverged after child '{child}'"
            );

            match r_sym {
                StepResult::Matched => {}
                StepResult::TooMany { max } => {
                    return Err(format!("'{child}' too many (max {max})"));
                }
                StepResult::NotExpected { expected } => {
                    return Err(format!("'{child}' not expected ({expected:?})"));
                }
            }
        }
        bound
            .inner()
            .finish(&st_sym)
            .map_err(|e| format!("incomplete: {e:?}"))
    }

    fn build(p: &Particle) -> Arc<ContentAutomaton> {
        Arc::new(build_automaton(p, &no_subst).unwrap())
    }

    #[test]
    fn sequence_order_and_occurrence() {
        // Mirrors content_automaton::tests::sequence_order_and_occurrence
        // through the bound path.
        let p = Particle::Sequence {
            min: 1,
            max: Some(1),
            items: vec![
                elem("a", 1, Some(1)),
                elem("b", 0, Some(2)),
                elem("c", 1, None),
            ],
        };
        let a = build(&p);
        assert!(a.upa_violation.is_none());
        assert!(run_both_arc(&a, &["a", "c"]).is_ok());
        assert!(run_both_arc(&a, &["a", "b", "b", "c", "c"]).is_ok());
        assert!(run_both_arc(&a, &["b", "c"]).is_err(), "missing required a");
        assert!(
            run_both_arc(&a, &["a", "b", "b", "b", "c"]).is_err(),
            "b too many"
        );
        assert!(
            run_both_arc(&a, &["a", "c", "b"]).is_err(),
            "b out of order"
        );
        assert!(run_both_arc(&a, &["a"]).is_err(), "missing c");
    }

    #[test]
    fn choice_totals_across_alternatives() {
        // Mirrors content_automaton::tests::choice_totals_across_alternatives.
        let p = Particle::Choice {
            min: 2,
            max: Some(2),
            items: vec![elem("a", 1, Some(1)), elem("b", 1, Some(1))],
        };
        let a = build(&p);
        assert!(run_both_arc(&a, &["a", "b"]).is_ok());
        assert!(run_both_arc(&a, &["b", "b"]).is_ok());
        assert!(run_both_arc(&a, &["a"]).is_err(), "only one of two");
        assert!(run_both_arc(&a, &["a", "b", "a"]).is_err(), "three of two");
    }

    #[test]
    fn nested_group_occurrence_ambiguity() {
        // (item{2,4}){1,2}: the config-set (run-count) machinery in action.
        let p = Particle::Sequence {
            min: 1,
            max: Some(2),
            items: vec![elem("item", 2, Some(4))],
        };
        let a = build(&p);
        assert!(run_both_arc(&a, &["item", "item"]).is_ok());
        assert!(run_both_arc(&a, &["item", "item", "item", "item"]).is_ok());
        assert!(run_both_arc(&a, &["item"]).is_err(), "below min");
    }

    #[test]
    fn prefixed_child_matches_local_label() {
        // A schema label 'LinearRing' must admit the document child
        // 'gml:LinearRing' via its local symbol, matching the string path's
        // local-name fallback.
        let p = Particle::Sequence {
            min: 1,
            max: Some(1),
            items: vec![elem("LinearRing", 1, Some(1))],
        };
        let a = build(&p);
        assert!(run_both_arc(&a, &["gml:LinearRing"]).is_ok());
        assert!(run_both_arc(&a, &["gml:Other"]).is_err());
    }

    #[test]
    fn substitution_group_member_matches_head_label() {
        // The automaton's name sets include substitution members; a member
        // name must match the head's position through the bound path.
        let subst = |head: &str| -> Vec<String> {
            if head == "_Geometry" {
                vec!["gml:Point".to_string(), "gml:LineString".to_string()]
            } else {
                Vec::new()
            }
        };
        let p = Particle::Sequence {
            min: 1,
            max: Some(1),
            items: vec![elem("_Geometry", 1, Some(2))],
        };
        let a = Arc::new(build_automaton(&p, &subst).unwrap());
        assert!(run_both_arc(&a, &["gml:Point"]).is_ok());
        assert!(run_both_arc(&a, &["Point"]).is_ok(), "member local name");
        assert!(run_both_arc(&a, &["gml:Point", "gml:LineString"]).is_ok());
        assert!(run_both_arc(&a, &["gml:Polygon"]).is_err(), "non-member");
    }
}
