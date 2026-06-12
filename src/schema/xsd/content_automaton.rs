//! Deterministic content-model automaton.
//!
//! Compiles a complex type's particle tree ([`Particle`]) into a Glushkov
//! automaton at schema-compile time. XSD's Unique Particle Attribution rule
//! guarantees valid content models are 1-unambiguous, so the Glushkov
//! automaton is deterministic: validation is one transition lookup per child
//! element, and a state that has two transitions on the same name is a UPA
//! violation in the schema itself.
//!
//! Occurrence bounds are handled in two ways:
//! - **Leaves** (elements, wildcards) keep `{min,max}` as a counter on a
//!   self-loop — one position regardless of bounds.
//! - **Groups** (sequence/choice) are expanded structurally:
//!   `G{2,4}` becomes `G G (G (G)?)?` with fresh positions per copy, which is
//!   what makes "the whole sequence repeats as a unit" come out right.
//!   Expansion is capped; beyond the cap the bound degrades gracefully to
//!   unbounded (lenient, never a false error).
//!
//! `xs:all` is not regular and is validated by the existing count-based
//! path; types whose content contains `xs:all` get no automaton.

use std::sync::Arc;

use rustc_hash::FxHashSet;

use crate::schema::types::{ElementDef, Particle, WildcardConstraint};

/// Cap for structural expansion of group occurrence bounds. A group with
/// `minOccurs`/`maxOccurs` beyond this is treated as unbounded above the
/// cap (lenient: may accept too many, never rejects valid content).
const EXPAND_CAP: u32 = 16;

/// What a position in the automaton matches.
#[derive(Debug, Clone)]
pub enum PosMatcher {
    /// An element particle: matched by qualified or local name, including
    /// substitution-group members.
    Element {
        /// All names that select this position (the declared name, its
        /// local part, and substitution member names + their local parts)
        names: FxHashSet<String>,
        /// Only the names as declared (qualified): used for UPA overlap
        /// checks, where local-part fallbacks would cause false positives
        /// on same-local-name elements from different namespaces
        decl_names: FxHashSet<String>,
        /// Identity of the source particle in the schema: positions created
        /// by occurrence expansion of the same particle share one id, so
        /// the UPA check doesn't flag a particle as competing with its own
        /// expansion copies
        particle_id: u32,
        /// The declared element particle (for nested validation context)
        def: Arc<ElementDef>,
    },
    /// An `xs:any` wildcard, matched by namespace.
    Wildcard(Arc<WildcardConstraint>),
}

impl PosMatcher {
    fn matches(&self, name: &str, local: &str, ns: Option<&str>) -> bool {
        match self {
            PosMatcher::Element { names, .. } => names.contains(name) || names.contains(local),
            PosMatcher::Wildcard(wc) => wc.matches(ns),
        }
    }

    /// A human-readable label for "expected ..." error messages.
    fn label(&self) -> String {
        match self {
            PosMatcher::Element { def, .. } => format!("'{}'", def.name),
            PosMatcher::Wildcard(_) => "any element (wildcard)".to_string(),
        }
    }

    /// The bare particle name (no quoting), for callers that need to
    /// cross-reference instance child names.
    fn plain_name(&self) -> String {
        match self {
            PosMatcher::Element { def, .. } => def.name.clone(),
            PosMatcher::Wildcard(_) => "*".to_string(),
        }
    }
}

/// Occurrence bounds enforced by a leaf position's self-loop counter.
#[derive(Debug, Clone, Copy)]
struct Occur {
    min: u32,
    max: Option<u32>,
}

/// A compiled, deterministic content-model automaton.
///
/// States are `0` (start) and `p + 1` for each position `p`. The runtime
/// state is an [`AutomatonState`]: current state plus the repeat count of
/// the current position.
#[derive(Debug)]
pub struct ContentAutomaton {
    /// Matcher per position (positions are 0-based here; state = pos + 1)
    matchers: Vec<PosMatcher>,
    /// Leaf occurrence bounds per position
    occurs: Vec<Occur>,
    /// Outgoing transitions per state (state 0 = start): target positions
    transitions: Vec<Vec<u32>>,
    /// Whether each state is accepting
    accepting: Vec<bool>,
    /// First UPA violation detected during construction, if any
    pub upa_violation: Option<String>,
}

/// Runtime cursor into a [`ContentAutomaton`].
///
/// Occurrence ranges on nested particles make the automaton ambiguous in a
/// way UPA does not forbid (`(item{2,4}){2,3}`: the fourth `item` may
/// belong to the second group iteration or extend the first), so the
/// cursor tracks the *set* of feasible `(state, run)` configurations. Run
/// counts are normalized, keeping the set tiny — for unambiguous models it
/// stays at one entry.
#[derive(Debug, Clone)]
pub struct AutomatonState {
    /// Feasible (state, consecutive-run) pairs; state 0 = start
    configs: smallvec::SmallVec<[(u32, u32); 2]>,
}

impl Default for AutomatonState {
    fn default() -> Self {
        Self {
            configs: smallvec::smallvec![(0, 0)],
        }
    }
}

/// Why content cannot end in the current state.
#[derive(Debug)]
pub enum FinishError {
    /// The current particle has not reached its `minOccurs`.
    TooFew {
        /// The particle's display label
        name: String,
        /// Its `minOccurs`
        min: u32,
        /// Consecutive occurrences seen
        found: u32,
    },
    /// Required particles are still missing.
    Missing {
        /// Plain names of the particles that could continue the content
        expected: Vec<String>,
    },
}

/// Outcome of feeding one child element to the automaton.
#[derive(Debug)]
pub enum StepResult {
    /// The child is admitted by the content model.
    Matched,
    /// The child matches the current particle but exceeds its `maxOccurs`.
    TooMany {
        /// The exceeded bound
        max: u32,
    },
    /// No transition admits this child here.
    NotExpected {
        /// Labels of the particles that would have been accepted here
        expected: Vec<String>,
    },
}

impl ContentAutomaton {
    /// Normalizes a run count so equivalent configurations collapse: once a
    /// position with no upper bound has met its minimum, further repeats
    /// are indistinguishable.
    fn norm_run(&self, pos: usize, run: u32) -> u32 {
        let occ = self.occurs[pos];
        match occ.max {
            None => run.min(occ.min.max(1)),
            Some(_) => run,
        }
    }

    /// Feeds one child element; advances the configuration set.
    ///
    /// `name` is the child's name as written (possibly prefixed), `local`
    /// its local part, `ns` its namespace URI if known.
    pub fn step(
        &self,
        st: &mut AutomatonState,
        name: &str,
        local: &str,
        ns: Option<&str>,
    ) -> StepResult {
        let mut next: smallvec::SmallVec<[(u32, u32); 2]> = smallvec::SmallVec::new();
        let mut push = |state: u32, run: u32| {
            if !next.contains(&(state, run)) {
                next.push((state, run));
            }
        };
        let mut blocked_max: Option<u32> = None;

        for &(state, run) in &st.configs {
            for &target in &self.transitions[state as usize] {
                if !self.matchers[target as usize].matches(name, local, ns) {
                    continue;
                }
                if target + 1 == state {
                    // Another repeat of the current particle.
                    let occ = self.occurs[target as usize];
                    match occ.max {
                        Some(max) if run >= max => blocked_max = Some(max),
                        _ => push(state, self.norm_run(target as usize, run + 1)),
                    }
                } else {
                    // Moving on: the particle we leave must have met its
                    // minOccurs for this path to be feasible.
                    if state > 0 && run < self.occurs[(state - 1) as usize].min {
                        continue;
                    }
                    push(target + 1, 1);
                }
            }
        }

        if !next.is_empty() {
            st.configs = next;
            return StepResult::Matched;
        }
        if let Some(max) = blocked_max {
            return StepResult::TooMany { max };
        }

        // Nothing matched: collect what would have been acceptable from
        // every live configuration.
        let mut expected: Vec<String> = Vec::new();
        for &(state, run) in &st.configs {
            for &target in &self.transitions[state as usize] {
                let feasible = if target + 1 == state {
                    true
                } else {
                    state == 0 || run >= self.occurs[(state - 1) as usize].min
                };
                if feasible {
                    let label = self.matchers[target as usize].label();
                    if !expected.contains(&label) {
                        expected.push(label);
                    }
                }
            }
        }
        StepResult::NotExpected { expected }
    }

    /// Checks that the content can end with the current configuration set.
    pub fn finish(&self, st: &AutomatonState) -> Result<(), FinishError> {
        let mut too_few: Option<FinishError> = None;
        for &(state, run) in &st.configs {
            if state == 0 {
                if self.accepting[0] {
                    return Ok(());
                }
                continue;
            }
            let pos = (state - 1) as usize;
            if self.accepting[state as usize] {
                if run >= self.occurs[pos].min {
                    return Ok(());
                }
                too_few.get_or_insert(FinishError::TooFew {
                    name: self.matchers[pos].label(),
                    min: self.occurs[pos].min,
                    found: run,
                });
            }
        }
        if let Some(err) = too_few {
            return Err(err);
        }
        // Not acceptable anywhere: report what could continue the content.
        let mut expected: Vec<String> = Vec::new();
        for &(state, run) in &st.configs {
            for &target in &self.transitions[state as usize] {
                let feasible = if target + 1 == state {
                    true
                } else {
                    state == 0 || run >= self.occurs[(state - 1) as usize].min
                };
                if feasible {
                    let name = self.matchers[target as usize].plain_name();
                    if !expected.contains(&name) {
                        expected.push(name);
                    }
                }
            }
        }
        Err(FinishError::Missing { expected })
    }

    /// Whether empty content is acceptable.
    pub fn accepts_empty(&self) -> bool {
        self.accepting[0]
    }
}

// =========================================================================
// Construction
// =========================================================================

/// Normalized regex node over positions.
enum Node {
    Empty,
    Leaf(usize),
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Opt(Box<Node>),
    Star(Box<Node>),
}

/// Builder state: positions created so far.
struct Builder<'a> {
    matchers: Vec<PosMatcher>,
    occurs: Vec<Occur>,
    /// substitution member lookup: head element name -> member names
    subst: &'a dyn Fn(&str) -> Vec<String>,
    /// Source-particle identity for expansion copies (UPA bookkeeping):
    /// maps a particle's address-independent path to one id. Incremented
    /// per *source* leaf; expansion copies reuse the current id via
    /// `current_particle_id`.
    next_particle_id: u32,
    /// When set, leaves take this id instead of a fresh one (used while
    /// instantiating expansion copies of the same source particle).
    current_particle_id: Option<u32>,
}

impl Builder<'_> {
    fn fresh_particle_id(&mut self) -> u32 {
        match self.current_particle_id {
            Some(id) => id,
            None => {
                let id = self.next_particle_id;
                self.next_particle_id += 1;
                id
            }
        }
    }

    fn leaf_element(&mut self, def: &ElementDef) -> Node {
        if def.max_occurs == Some(0) {
            return Node::Empty;
        }
        let mut names = FxHashSet::default();
        let mut add = |n: &str| {
            names.insert(n.to_string());
            if let Some((_, local)) = n.split_once(':') {
                names.insert(local.to_string());
            }
        };
        add(&def.name);
        for member in (self.subst)(&def.name) {
            add(&member);
        }
        // UPA comparison uses only the directly declared name: substitution
        // members and local-part fallbacks lack the namespace information
        // needed to tell genuine competition from same-local-name elements
        // in different namespaces (conservative: misses substitution-based
        // UPA violations, never rejects a valid schema for them).
        let mut decl_names = FxHashSet::default();
        decl_names.insert(def.name.clone());
        let min = def.min_occurs;
        let pos = self.matchers.len();
        let particle_id = self.fresh_particle_id();
        self.matchers.push(PosMatcher::Element {
            names,
            decl_names,
            particle_id,
            def: Arc::new(def.clone()),
        });
        self.occurs.push(Occur {
            min: min.max(1),
            max: def.max_occurs,
        });
        if min == 0 {
            Node::Opt(Box::new(Node::Leaf(pos)))
        } else {
            Node::Leaf(pos)
        }
    }

    fn leaf_wildcard(&mut self, wc: &WildcardConstraint) -> Node {
        if wc.max_occurs == Some(0) {
            return Node::Empty;
        }
        let min = wc.min_occurs;
        let pos = self.matchers.len();
        self.matchers
            .push(PosMatcher::Wildcard(Arc::new(wc.clone())));
        self.occurs.push(Occur {
            min: min.max(1),
            max: wc.max_occurs,
        });
        if min == 0 {
            Node::Opt(Box::new(Node::Leaf(pos)))
        } else {
            Node::Leaf(pos)
        }
    }

    /// Builds a node for one occurrence of the particle (fresh positions).
    fn build_once(&mut self, particle: &Particle) -> Option<Node> {
        match particle {
            Particle::Element(def) => Some(self.leaf_element(def)),
            Particle::Wildcard(wc) => Some(self.leaf_wildcard(wc)),
            Particle::Sequence { items, .. } => {
                let nodes: Vec<Node> = items.iter().filter_map(|p| self.build(p)).collect();
                Some(Node::Seq(nodes))
            }
            Particle::Choice { items, .. } => {
                let nodes: Vec<Node> = items.iter().filter_map(|p| self.build(p)).collect();
                if nodes.is_empty() {
                    Some(Node::Empty)
                } else {
                    Some(Node::Alt(nodes))
                }
            }
            // xs:all is not regular; the caller refuses to build an
            // automaton for content containing it.
            Particle::All { .. } => None,
        }
    }

    /// Builds a node for the particle including its group occurrence bounds.
    fn build(&mut self, particle: &Particle) -> Option<Node> {
        let (min, max) = match particle {
            // Leaf bounds are handled by the position's counter.
            Particle::Element(_) | Particle::Wildcard(_) => {
                return self.build_once(particle);
            }
            Particle::Sequence { min, max, .. } | Particle::Choice { min, max, .. } => (*min, *max),
            Particle::All { .. } => return None,
        };

        if max == Some(0) {
            return Some(Node::Empty);
        }
        if min == 1 && max == Some(1) {
            return self.build_once(particle);
        }

        // Structural expansion with fresh positions per copy. Copies of
        // the same source particle replay the same particle-id sequence so
        // the UPA check doesn't see copies as competing particles.
        let id_base = self.next_particle_id;
        let copy = |b: &mut Self| -> Option<Node> {
            b.next_particle_id = id_base;
            b.build_once(particle)
        };
        let req = min.min(EXPAND_CAP);
        let mut parts: Vec<Node> = Vec::new();
        for _ in 0..req {
            parts.push(copy(self)?);
        }
        match max {
            None => {
                // {min,unbounded}: min copies then a star.
                let star = copy(self)?;
                parts.push(Node::Star(Box::new(star)));
            }
            Some(m) if m > min && m - min <= EXPAND_CAP && min <= EXPAND_CAP => {
                // {min,max}: nested optional copies keep determinism:
                // G{1,3} = G (G (G)?)?
                let mut tail: Option<Node> = None;
                for _ in 0..(m - min) {
                    let c = copy(self)?;
                    let inner = match tail.take() {
                        Some(t) => Node::Seq(vec![c, t]),
                        None => c,
                    };
                    tail = Some(Node::Opt(Box::new(inner)));
                }
                if let Some(t) = tail {
                    parts.push(t);
                }
            }
            Some(m) if m > min => {
                // Bound too large to expand: degrade to unbounded (lenient).
                let star = copy(self)?;
                parts.push(Node::Star(Box::new(star)));
            }
            _ => {} // max == min: exactly the required copies
        }
        let node = Node::Seq(parts);
        if min == 0 {
            // min==0 with copies built only for the optional tail
            Some(Node::Opt(Box::new(node)))
        } else {
            Some(node)
        }
    }
}

/// Glushkov sets for a node.
struct Sets {
    nullable: bool,
    first: Vec<usize>,
    last: Vec<usize>,
}

fn glushkov(node: &Node, follow: &mut Vec<Vec<usize>>) -> Sets {
    match node {
        Node::Empty => Sets {
            nullable: true,
            first: vec![],
            last: vec![],
        },
        Node::Leaf(p) => Sets {
            nullable: false,
            first: vec![*p],
            last: vec![*p],
        },
        Node::Opt(inner) => {
            let s = glushkov(inner, follow);
            Sets {
                nullable: true,
                ..s
            }
        }
        Node::Star(inner) => {
            let s = glushkov(inner, follow);
            for &l in &s.last {
                for &f in &s.first {
                    if !follow[l].contains(&f) {
                        follow[l].push(f);
                    }
                }
            }
            Sets {
                nullable: true,
                first: s.first,
                last: s.last,
            }
        }
        Node::Seq(items) => {
            let mut nullable = true;
            let mut first: Vec<usize> = vec![];
            let mut last: Vec<usize> = vec![];
            for item in items {
                let s = glushkov(item, follow);
                for &l in &last {
                    for &f in &s.first {
                        if !follow[l].contains(&f) {
                            follow[l].push(f);
                        }
                    }
                }
                if nullable {
                    first.extend(&s.first);
                }
                if s.nullable {
                    last.extend(&s.last);
                } else {
                    last = s.last;
                }
                nullable &= s.nullable;
            }
            Sets {
                nullable,
                first,
                last,
            }
        }
        Node::Alt(items) => {
            let mut nullable = false;
            let mut first: Vec<usize> = vec![];
            let mut last: Vec<usize> = vec![];
            for item in items {
                let s = glushkov(item, follow);
                nullable |= s.nullable;
                first.extend(s.first);
                last.extend(s.last);
            }
            Sets {
                nullable,
                first,
                last,
            }
        }
    }
}

/// Builds the automaton for a particle tree, or `None` when the content
/// model is not automaton-friendly (contains `xs:all`).
///
/// `subst` maps a head element name to its (transitive) substitution
/// member names.
pub fn build_automaton(
    particle: &Particle,
    subst: &dyn Fn(&str) -> Vec<String>,
) -> Option<ContentAutomaton> {
    let mut b = Builder {
        matchers: Vec::new(),
        occurs: Vec::new(),
        subst,
        next_particle_id: 0,
        current_particle_id: None,
    };
    let root = b.build(particle)?;

    let n = b.matchers.len();
    let mut follow: Vec<Vec<usize>> = vec![Vec::new(); n];
    let sets = glushkov(&root, &mut follow);

    // A structural self-transition (the position follows itself through a
    // repeated group, e.g. `(<element maxOccurs="1"/>)*`) means the leaf's
    // own bound resets on every group iteration; a single run counter
    // cannot tell iterations apart, so the upper bound is dropped
    // (lenient). The lower bound still applies to each uninterrupted run.
    for (p, occ) in b.occurs.iter_mut().enumerate() {
        if follow[p].contains(&p) {
            occ.max = None;
        }
    }

    // transitions[state]: state 0 = start (first set), state p+1 = follow(p).
    let mut transitions: Vec<Vec<u32>> = Vec::with_capacity(n + 1);
    transitions.push(sets.first.iter().map(|&p| p as u32).collect());
    for f in &follow {
        transitions.push(f.iter().map(|&p| p as u32).collect());
    }

    // Self-loops for repeatable leaves (counter-based).
    for (p, occ) in b.occurs.iter().enumerate() {
        if occ.max != Some(1) && !transitions[p + 1].contains(&(p as u32)) {
            // Repeats come first so the counter path wins ties with
            // follow-set successors of the same name (which would be a UPA
            // violation anyway).
            transitions[p + 1].insert(0, p as u32);
        }
    }

    let mut accepting = vec![false; n + 1];
    accepting[0] = sets.nullable;
    for &l in &sets.last {
        accepting[l + 1] = true;
    }

    let upa_violation = check_upa(&b.matchers, &b.occurs, &transitions);

    Some(ContentAutomaton {
        matchers: b.matchers,
        occurs: b.occurs,
        transitions,
        accepting,
        upa_violation,
    })
}

/// Detects Unique Particle Attribution violations: two element transitions
/// from one state that can match the same name. (Element/wildcard overlap
/// is also a 1.0 violation but needs reliable namespace info on element
/// declarations, so it is not flagged here.)
fn check_upa(
    matchers: &[PosMatcher],
    occurs: &[Occur],
    transitions: &[Vec<u32>],
) -> Option<String> {
    for (state, targets) in transitions.iter().enumerate() {
        // A self-loop (more repeats of the current particle) and an exit
        // transition are only simultaneously feasible when the particle's
        // occurrence range is variable: with min == max the counter fully
        // determines whether the next match repeats or moves on.
        let self_pos = state.checked_sub(1).map(|p| p as u32);
        let fixed_count = self_pos
            .map(|p| {
                let occ = occurs[p as usize];
                occ.max == Some(occ.min)
            })
            .unwrap_or(false);
        for (i, &a) in targets.iter().enumerate() {
            for &bpos in &targets[i + 1..] {
                if a == bpos {
                    continue;
                }
                if fixed_count && (Some(a) == self_pos || Some(bpos) == self_pos) {
                    continue;
                }
                if let (
                    PosMatcher::Element {
                        decl_names: na,
                        particle_id: pa,
                        def: da,
                        ..
                    },
                    PosMatcher::Element {
                        decl_names: nb,
                        particle_id: pb,
                        ..
                    },
                ) = (&matchers[a as usize], &matchers[bpos as usize])
                    && pa != pb
                    && na.intersection(nb).next().is_some()
                {
                    return Some(format!(
                        "content model violates Unique Particle Attribution: \
                         element '{}' can match more than one particle",
                        da.name
                    ));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
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
    fn all_yields_no_automaton() {
        let p = Particle::All {
            min: 1,
            elements: vec![],
        };
        assert!(build_automaton(&p, &no_subst).is_none());
    }
}
