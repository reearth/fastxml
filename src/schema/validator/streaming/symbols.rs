//! Interned symbol table for streaming validation.
//!
//! Element and attribute names recur constantly while streaming a large XML
//! document. Instead of hashing/allocating their strings on every start tag,
//! the validator interns each distinct name once into a [`SymbolId`] (a dense
//! `u32` index) and thereafter compares integers.
//!
//! One table holds *both* qualified names (`gml:Point`) and bare local names
//! (`Point`). A prefixed qname's record links to its local symbol, which is
//! interned eagerly, so a single `intern` of the qname yields both the qname
//! symbol and — via [`SymRecord::local`] — its local symbol without a second
//! hash probe.

use std::sync::Arc;

use rustc_hash::FxHashMap;

/// A dense, comparable handle for an interned name.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub(crate) struct SymbolId(pub u32);

/// The interned data for one symbol.
pub(crate) struct SymRecord {
    /// The name text (pooled `Arc<str>`, shared with callers that still need
    /// a string, e.g. error messages and identity constraints).
    pub text: Arc<str>,
    /// The symbol of this name's local part. For a bare (unprefixed) name this
    /// is the symbol itself; for a prefixed qname it is the eagerly-interned
    /// local-name symbol.
    pub local: SymbolId,
    /// Whether the name carried a namespace prefix (`prefix:local`).
    pub has_prefix: bool,
}

/// Interns names into [`SymbolId`]s. See the module docs.
#[derive(Default)]
pub(crate) struct SymbolTable {
    map: FxHashMap<Box<str>, SymbolId>,
    records: Vec<SymRecord>,
    /// Interned `(namespace-symbol, local-symbol)` pairs. Lets a type's
    /// resolved `(namespace, local)` identity map to a single dense symbol
    /// without allocating a combined string per element — the key is two
    /// integers. Shares the [`SymbolId`] space with `records`, so a pair
    /// symbol is distinct from every name symbol.
    pairs: FxHashMap<(u32, u32), SymbolId>,
}

impl SymbolTable {
    /// Interns `s`, returning its symbol. Idempotent: the same string always
    /// maps to the same [`SymbolId`]. A prefixed qname eagerly interns its
    /// local part so [`SymRecord::local`] is available with no extra probe.
    pub fn intern(&mut self, s: &str) -> SymbolId {
        if let Some(&id) = self.map.get(s) {
            return id;
        }
        // Eagerly intern the local part of a prefixed qname. This borrows
        // `self` mutably and must complete before we allocate the new id.
        let local_sym = match s.split_once(':') {
            Some((p, l)) if !p.is_empty() && !l.is_empty() => Some(self.intern(l)),
            _ => None,
        };
        let id = SymbolId(self.records.len() as u32);
        let (local, has_prefix) = match local_sym {
            Some(l) => (l, true),
            None => (id, false),
        };
        let text: Arc<str> = Arc::from(s);
        self.records.push(SymRecord {
            text,
            local,
            has_prefix,
        });
        self.map.insert(Box::from(s), id);
        id
    }

    /// Interns the `(namespace-symbol, local-symbol)` pair into a single dense
    /// symbol, idempotently. Used to give a type its resolved
    /// `(namespace, local)` identity as one comparable integer key — immune to
    /// the bare-local-name collisions that plague the raw type-reference
    /// string (e.g. two `ct-A` types in different namespaces). No allocation:
    /// the key is an integer pair.
    pub fn intern_pair(&mut self, ns: SymbolId, local: SymbolId) -> SymbolId {
        if let Some(&id) = self.pairs.get(&(ns.0, local.0)) {
            return id;
        }
        let id = SymbolId(self.records.len() as u32);
        // Synthetic record so pair symbols share the id space with names and
        // stay globally unique. The text mirrors the local part; it is never
        // read back for a pair symbol (pairs are used only as cache keys).
        let text = self.records[local.0 as usize].text.clone();
        self.records.push(SymRecord {
            text,
            local,
            has_prefix: false,
        });
        self.pairs.insert((ns.0, local.0), id);
        id
    }

    /// Returns the pooled `Arc<str>` text for `id`.
    #[inline]
    pub fn arc(&self, id: SymbolId) -> &Arc<str> {
        &self.records[id.0 as usize].text
    }

    /// Returns the record for `id`.
    #[inline]
    #[allow(dead_code)]
    pub fn record(&self, id: SymbolId) -> &SymRecord {
        &self.records[id.0 as usize]
    }

    /// Returns the local-name symbol of `id`.
    #[inline]
    pub fn local(&self, id: SymbolId) -> SymbolId {
        self.records[id.0 as usize].local
    }

    /// Whether `id`'s name carried a namespace prefix.
    #[inline]
    #[allow(dead_code)]
    pub fn has_prefix(&self, id: SymbolId) -> bool {
        self.records[id.0 as usize].has_prefix
    }

    /// Number of interned symbols (used by the binding layer to size runtime
    /// tables and to distinguish schema-declared symbols from document-only
    /// ones once pre-registration has run).
    #[inline]
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.records.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_is_idempotent() {
        let mut t = SymbolTable::default();
        let a = t.intern("gml:Point");
        let b = t.intern("gml:Point");
        assert_eq!(a, b);
    }

    #[test]
    fn prefixed_links_to_local() {
        let mut t = SymbolTable::default();
        let q = t.intern("gml:Point");
        let l = t.intern("Point");
        assert_eq!(t.local(q), l);
        assert!(t.has_prefix(q));
        assert!(!t.has_prefix(l));
        assert_eq!(t.local(l), l);
    }

    #[test]
    fn arc_is_pooled() {
        let mut t = SymbolTable::default();
        let a = t.intern("dem:lod");
        let b = t.intern("dem:lod");
        assert!(Arc::ptr_eq(t.arc(a), t.arc(b)));
        assert_eq!(t.arc(a).as_ref(), "dem:lod");
    }

    #[test]
    fn unprefixed_local_is_self() {
        let mut t = SymbolTable::default();
        let a = t.intern("root");
        assert_eq!(t.local(a), a);
        assert!(!t.has_prefix(a));
    }
}
