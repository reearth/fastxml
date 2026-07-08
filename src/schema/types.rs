//! XSD schema type definitions.

use std::sync::Arc;

use indexmap::IndexMap as IndexMapBase;

/// Insertion-ordered map with a fast non-cryptographic hasher; schema
/// lookups happen per element/attribute during validation, so hashing speed
/// matters more than HashDoS resistance here.
pub type IndexMap<K, V> = IndexMapBase<K, V, rustc_hash::FxBuildHasher>;

/// Unordered map with the same fast non-cryptographic hasher, used for the
/// per-schema caches and namespace/substitution tables consulted per element
/// (C6): the built-in `HashMap` SipHash cost showed up in per-element cache
/// lookups such as `ns_type_children_cache`.
type HashMap<K, V> = rustc_hash::FxHashMap<K, V>;

/// A namespace-qualified name for collision-free lookups.
///
/// Uses (namespace_uri, local_name) instead of "prefix:local_name"
/// to avoid non-deterministic prefix collisions when multiple namespaces
/// define types/elements with the same local name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NsName {
    /// Namespace URI (empty string for no-namespace).
    ///
    /// Stored as `Arc<str>` so cloning a key (which happens per cache
    /// insertion and, once the readers are flipped, per element lookup) is a
    /// pointer bump rather than a heap copy.
    pub namespace_uri: Arc<str>,
    /// Local name.
    pub local_name: Arc<str>,
}

impl NsName {
    /// Creates a new NsName.
    pub fn new(namespace_uri: impl Into<Arc<str>>, local_name: impl Into<Arc<str>>) -> Self {
        Self {
            namespace_uri: namespace_uri.into(),
            local_name: local_name.into(),
        }
    }
}

/// Borrowed key for probing [`NsName`]-keyed [`IndexMap`]s without allocating.
///
/// The instance side of validation holds `(&str, &str)`; building an owned
/// `NsName` per element lookup would put two `Arc<str>` allocations on the
/// hot path. This mirror type hashes exactly like `NsName` (both hash the
/// `str` contents field-by-field in the same order) and implements
/// [`indexmap::Equivalent`], so `IndexMap::get` accepts it directly.
#[derive(Debug, Clone, Copy)]
pub struct NsNameRef<'a> {
    /// Namespace URI (empty string for no-namespace).
    pub namespace_uri: &'a str,
    /// Local name.
    pub local_name: &'a str,
}

impl std::hash::Hash for NsNameRef<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Must match NsName's derived Hash: Arc<str> delegates to str.
        self.namespace_uri.hash(state);
        self.local_name.hash(state);
    }
}

impl indexmap::Equivalent<NsName> for NsNameRef<'_> {
    fn equivalent(&self, key: &NsName) -> bool {
        *key.namespace_uri == *self.namespace_uri && *key.local_name == *self.local_name
    }
}

/// Content model type for an element (used in caches).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentModelType {
    /// Sequence - all elements in order, each with their own min/max occurs
    #[default]
    Sequence,
    /// Choice - exactly one of the elements must be present
    Choice,
    /// All - all elements must be present, but in any order
    All,
    /// Empty - no child elements allowed
    Empty,
}

/// Pre-computed child element constraints for a type.
///
/// This struct caches the flattened inheritance chain for a complex type,
/// eliminating the need to traverse the inheritance hierarchy at validation time.
#[derive(Debug, Clone)]
pub struct FlattenedChildren {
    /// Child element constraints (name -> (min_occurs, max_occurs))
    pub constraints: HashMap<String, (u32, Option<u32>)>,
    /// Content model type
    pub content_model_type: ContentModelType,
    /// Ordered element names for sequence validation.
    /// Using Arc<[String]> to make cloning cheap (pointer copy instead of deep clone).
    pub ordered_elements: Arc<[String]>,
    /// Element wildcard (xs:any) in the content model, if present
    pub wildcard: Option<WildcardConstraint>,
    /// Deterministic content-model automaton built from the (inheritance-
    /// merged) particle tree; None when the content contains xs:all or the
    /// base chain could not be resolved (count-based fallback is used).
    pub automaton: Option<Arc<crate::schema::xsd::content_automaton::ContentAutomaton>>,
}

impl FlattenedChildren {
    /// Creates a new empty FlattenedChildren.
    pub fn new() -> Self {
        Self {
            constraints: HashMap::default(),
            content_model_type: ContentModelType::Empty,
            ordered_elements: Arc::from([]),
            wildcard: None,
            automaton: None,
        }
    }

    /// Creates a FlattenedChildren with the given content model type.
    pub fn with_content_model(content_model_type: ContentModelType) -> Self {
        Self {
            constraints: HashMap::default(),
            content_model_type,
            ordered_elements: Arc::from([]),
            wildcard: None,
            automaton: None,
        }
    }
}

impl Default for FlattenedChildren {
    fn default() -> Self {
        Self::new()
    }
}

/// A compiled XSD schema.
///
/// Components are keyed by namespace-qualified [`NsName`] — the owning
/// schema document's target namespace plus the local name — so same-local
/// globals in different namespaces never collide and schema-document prefix
/// conventions do not leak into the compiled artifact (C5 cutover).
#[derive(Debug, Clone)]
pub struct CompiledSchema {
    /// Target namespace
    pub target_namespace: Option<String>,

    // === Namespace-qualified component maps (collision-free) ===
    /// Global element definitions keyed by `(target namespace, local name)`.
    pub elements_ns: IndexMap<NsName, ElementDef>,
    /// Type definitions keyed by `(target namespace, local name)`.
    pub types_ns: IndexMap<NsName, TypeDef>,
    /// Top-level attribute definitions keyed by `(target namespace, local name)`.
    pub attributes_ns: IndexMap<NsName, AttributeDef>,

    /// Substitution groups (head element name -> list of substitute element names)
    pub substitution_groups: HashMap<String, Vec<String>>,

    // === Namespace resolution ===
    /// Namespace URI to prefix mapping (uri -> prefix).
    /// Used to resolve element lookups when XML uses different prefix than schema.
    pub namespace_prefixes: HashMap<String, String>,

    // === Namespace-aware indices ===
    /// Prefix to namespace URI mapping (prefix -> uri).
    pub prefix_namespaces: HashMap<String, String>,

    /// Namespace-aware type children cache: (namespace_uri, local_name) -> FlattenedChildren.
    ///
    /// This is the primary cache for type children lookups. It uses namespace URIs
    /// instead of prefixes to avoid cross-namespace collisions.
    pub ns_type_children_cache: HashMap<NsName, Arc<FlattenedChildren>>,

    /// Pre-computed transitive substitution groups (head -> all members).
    ///
    /// This cache stores all transitive members of each substitution group head,
    /// including indirect members (e.g., if A has member B, and B has member C,
    /// this stores [B, C] for A).
    pub transitive_substitution_groups: HashMap<String, Arc<Vec<String>>>,

    /// Reverse lookup: substitution group member -> head element.
    ///
    /// This allows O(1) lookup of which substitution group head an element belongs to.
    pub substitution_group_heads: HashMap<String, String>,
}

impl CompiledSchema {
    /// Creates a new empty schema.
    pub fn new() -> Self {
        Self {
            target_namespace: None,
            elements_ns: IndexMap::default(),
            types_ns: IndexMap::default(),
            attributes_ns: IndexMap::default(),
            substitution_groups: HashMap::default(),
            // Namespace resolution
            namespace_prefixes: HashMap::default(),
            prefix_namespaces: HashMap::default(),
            // Namespace-aware caches
            ns_type_children_cache: HashMap::default(),
            transitive_substitution_groups: HashMap::default(),
            substitution_group_heads: HashMap::default(),
        }
    }

    /// Creates a schema with a target namespace.
    pub fn with_namespace(namespace: impl Into<String>) -> Self {
        Self {
            target_namespace: Some(namespace.into()),
            ..Self::new()
        }
    }

    /// Compatibility shim: looks up an element definition by a prefixed or
    /// bare qualified-name string.
    ///
    /// Deprecated in favor of [`element_ns`](Self::element_ns) /
    /// [`element_ns_any`](Self::element_ns_any) with a resolved namespace
    /// URI. Resolution: a prefixed name resolves its prefix through the
    /// schema's accumulated prefix table; an unprefixed name tries the
    /// target namespace, then no-namespace; both fall back to an any-
    /// namespace local-name scan (the compiled reference strings this shim
    /// serves are not always prefix-qualified — see `resolve_qname`'s
    /// type-cache gating).
    #[doc(hidden)]
    pub fn get_element(&self, qname: &str) -> Option<&ElementDef> {
        if let Some((prefix, local)) = qname.split_once(':') {
            if let Some(uri) = self.prefix_namespaces.get(prefix)
                && let Some(elem) = self.element_ns(uri, local)
            {
                return Some(elem);
            }
            return self.element_ns_any(local);
        }
        if let Some(tns) = self.target_namespace.as_deref()
            && let Some(elem) = self.element_ns(tns, qname)
        {
            return Some(elem);
        }
        if let Some(elem) = self.element_ns("", qname) {
            return Some(elem);
        }
        self.element_ns_any(qname)
    }

    /// Compatibility shim: looks up an element definition by namespace URI
    /// and local name, with an any-namespace local-name scan as last resort.
    ///
    /// Prefer [`element_ns`](Self::element_ns) (strict) — the trailing scan
    /// here is leniency retained for legacy fallback paths.
    #[doc(hidden)]
    pub fn get_element_by_ns(&self, namespace_uri: &str, local_name: &str) -> Option<&ElementDef> {
        if let Some(elem) = self.element_ns(namespace_uri, local_name) {
            return Some(elem);
        }
        self.element_ns_any(local_name)
    }

    /// Compatibility shim: looks up a type definition by a prefixed or bare
    /// qualified-name string. See [`get_element`](Self::get_element) for the
    /// resolution rules.
    #[doc(hidden)]
    pub fn get_type(&self, qname: &str) -> Option<&TypeDef> {
        if let Some((prefix, local)) = qname.split_once(':') {
            if let Some(uri) = self.prefix_namespaces.get(prefix)
                && let Some(typ) = self.type_ns(uri, local)
            {
                return Some(typ);
            }
            return self.type_ns_any(local);
        }
        if let Some(tns) = self.target_namespace.as_deref()
            && let Some(typ) = self.type_ns(tns, qname)
        {
            return Some(typ);
        }
        if let Some(typ) = self.type_ns("", qname) {
            return Some(typ);
        }
        self.type_ns_any(qname)
    }

    /// Looks up a global element by namespace URI and local name in the
    /// collision-free [`elements_ns`](Self::elements_ns) map.
    ///
    /// Strict resolution: tries the exact `(namespace, local)` key, then a
    /// chameleon fallback under the no-namespace `""` (an included chameleon
    /// schema's components adopt the includer's target namespace, but may also
    /// have been registered under `""`). It deliberately does NOT scan across
    /// other namespaces — that leniency lives in
    /// [`element_ns_any`](Self::element_ns_any) and is only appropriate when
    /// the instance namespace could not be resolved at all.
    pub fn element_ns(&self, namespace_uri: &str, local_name: &str) -> Option<&ElementDef> {
        // Borrowed-key probes: no per-lookup allocation on the hot path.
        if let Some(e) = self.elements_ns.get(&NsNameRef {
            namespace_uri,
            local_name,
        }) {
            return Some(e);
        }
        if !namespace_uri.is_empty()
            && let Some(e) = self.elements_ns.get(&NsNameRef {
                namespace_uri: "",
                local_name,
            })
        {
            return Some(e);
        }
        None
    }

    /// Leniency fallback: the first global element whose local name matches,
    /// regardless of namespace. Only appropriate when the instance element's
    /// namespace could not be resolved (no in-scope binding); a cleanly
    /// resolved namespace that misses [`element_ns`](Self::element_ns) must
    /// stay a miss, or invalid instances would be silently accepted.
    pub fn element_ns_any(&self, local_name: &str) -> Option<&ElementDef> {
        self.elements_ns
            .iter()
            .find(|(k, _)| &*k.local_name == local_name)
            .map(|(_, e)| e)
    }

    /// Looks up a type by namespace URI and local name in the collision-free
    /// [`types_ns`](Self::types_ns) map (exact key, then chameleon `""`).
    pub fn type_ns(&self, namespace_uri: &str, local_name: &str) -> Option<&TypeDef> {
        if let Some(t) = self.types_ns.get(&NsNameRef {
            namespace_uri,
            local_name,
        }) {
            return Some(t);
        }
        if !namespace_uri.is_empty()
            && let Some(t) = self.types_ns.get(&NsNameRef {
                namespace_uri: "",
                local_name,
            })
        {
            return Some(t);
        }
        None
    }

    /// Leniency fallback for types: first type with a matching local name in
    /// any namespace. See [`element_ns_any`](Self::element_ns_any).
    pub fn type_ns_any(&self, local_name: &str) -> Option<&TypeDef> {
        self.types_ns
            .iter()
            .find(|(k, _)| &*k.local_name == local_name)
            .map(|(_, t)| t)
    }

    /// Looks up a top-level attribute by namespace URI and local name in the
    /// collision-free [`attributes_ns`](Self::attributes_ns) map.
    pub fn attribute_ns(&self, namespace_uri: &str, local_name: &str) -> Option<&AttributeDef> {
        if let Some(a) = self.attributes_ns.get(&NsNameRef {
            namespace_uri,
            local_name,
        }) {
            return Some(a);
        }
        if !namespace_uri.is_empty()
            && let Some(a) = self.attributes_ns.get(&NsNameRef {
                namespace_uri: "",
                local_name,
            })
        {
            return Some(a);
        }
        None
    }

    /// Resolves a compile-time-resolved reference: the ns-qualified map first
    /// (collision-free), then the legacy string lookup as fallback.
    pub fn type_by_ref(&self, ns: Option<&NsName>, name: &str) -> Option<&TypeDef> {
        if let Some(n) = ns
            && let Some(t) = self.type_ns(&n.namespace_uri, &n.local_name)
        {
            return Some(t);
        }
        self.get_type(name)
    }

    /// Resolves a complex type's base-type definition, hopping via the
    /// compile-time resolved `base_ns` when it corresponds to the base
    /// reference, with the legacy string lookup as fallback.
    ///
    /// The base reference lives either in the content model
    /// (`ComplexExtension` / `SimpleContent`) or in `base_type`; both are
    /// compiled from the same QName, so `base_ns` applies whenever the
    /// strings agree (built-in GML types set only the content-model string).
    pub fn complex_base_def(&self, c: &ComplexType) -> Option<&TypeDef> {
        let base = match &c.content {
            ContentModel::ComplexExtension { base_type, .. } => Some(base_type.as_str()),
            ContentModel::SimpleContent { base_type } => Some(base_type.as_str()),
            _ => c.base_type.as_deref(),
        }?;
        let ns = c
            .base_ns
            .as_ref()
            .filter(|_| c.base_type.as_deref() == Some(base));
        self.type_by_ref(ns, base)
    }

    /// Resolves a simple type's base-type definition (ns-first, string
    /// fallback). The synthetic `list(...)`/`union(...)` markers carry no
    /// `base_ns` and miss the string lookup too, returning `None` as before.
    pub fn simple_base_def(&self, s: &SimpleType) -> Option<&TypeDef> {
        let base = s.base_type.as_deref()?;
        self.type_by_ref(s.base_ns.as_ref(), base)
    }

    /// Resolves a prefixed type reference to a NsName using prefix_namespaces.
    ///
    /// "bldg:WallSurfaceType" → NsName("http://...building...", "WallSurfaceType")
    /// "WallSurfaceType" (no prefix) → NsName(target_namespace, "WallSurfaceType")
    pub fn resolve_type_ref_to_ns(&self, type_ref: &str) -> Option<NsName> {
        if let Some((prefix, local)) = type_ref.split_once(':') {
            let ns_uri = self.prefix_namespaces.get(prefix)?;
            Some(NsName::new(ns_uri.clone(), local))
        } else {
            // No prefix - use target namespace
            let ns = self.target_namespace.as_deref().unwrap_or("");
            Some(NsName::new(ns, type_ref))
        }
    }

    /// Looks up type children cache by namespace URI and local name.
    pub fn get_ns_type_children(
        &self,
        namespace_uri: &str,
        local_name: &str,
    ) -> Option<&Arc<FlattenedChildren>> {
        self.ns_type_children_cache
            .get(&NsName::new(namespace_uri, local_name))
    }

    /// Looks up a type by namespace URI and local name.
    pub fn get_type_by_ns(&self, namespace_uri: &str, local_name: &str) -> Option<&TypeDef> {
        // Compatibility shim over the ns-qualified map; the trailing
        // any-namespace scan preserves the legacy bare-local fallback.
        if let Some(typ) = self.type_ns(namespace_uri, local_name) {
            return Some(typ);
        }
        self.type_ns_any(local_name)
    }

    /// Renders an [`NsName`] as `prefix:local` using the schema's
    /// namespace-to-prefix table, for diagnostics. Falls back to the bare
    /// local name when the namespace has no (non-empty) prefix.
    pub fn display_name(&self, name: &NsName) -> String {
        if !name.namespace_uri.is_empty()
            && let Some(prefix) = self.namespace_prefixes.get(&*name.namespace_uri)
            && !prefix.is_empty()
        {
            return format!("{}:{}", prefix, name.local_name);
        }
        name.local_name.to_string()
    }
}

impl Default for CompiledSchema {
    fn default() -> Self {
        Self::new()
    }
}

/// An element definition.
#[derive(Debug, Clone)]
pub struct ElementDef {
    /// Element name
    pub name: String,
    /// Type reference (name of a type definition)
    pub type_ref: Option<String>,
    /// Namespace-resolved form of [`type_ref`](Self::type_ref), computed at
    /// compile time against the owning document's bindings. The string
    /// `type_ref` is kept verbatim for message stability; this parallel field
    /// carries the collision-free `(namespace, local)` for lookups.
    pub type_ns: Option<NsName>,
    /// For an element *reference* (`<xs:element ref="q:name"/>`), the resolved
    /// namespace of the referenced global element. The `name` field holds the
    /// bare local for message stability; this carries the namespace.
    pub ref_ns: Option<NsName>,
    /// Inline type definition
    pub inline_type: Option<TypeDef>,
    /// Minimum occurrences
    pub min_occurs: u32,
    /// Maximum occurrences (None = unbounded)
    pub max_occurs: Option<u32>,
    /// Whether this element is abstract
    pub is_abstract: bool,
    /// Substitution group head
    pub substitution_group: Option<String>,
    /// Namespace-resolved form of
    /// [`substitution_group`](Self::substitution_group).
    pub substitution_ns: Option<NsName>,
    /// Whether the element is nillable
    pub nillable: bool,
    /// Default value applied when the element is empty
    pub default: Option<String>,
    /// Fixed value the element content must match
    pub fixed: Option<String>,
    /// Identity constraints (unique, key, keyref)
    pub constraints: Vec<CompiledConstraint>,
}

impl ElementDef {
    /// Creates a new element definition.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_ref: None,
            type_ns: None,
            ref_ns: None,
            inline_type: None,
            min_occurs: 1,
            max_occurs: Some(1),
            is_abstract: false,
            substitution_group: None,
            substitution_ns: None,
            nillable: false,
            default: None,
            fixed: None,
            constraints: Vec::new(),
        }
    }

    /// Sets the type reference.
    pub fn with_type(mut self, type_ref: impl Into<String>) -> Self {
        self.type_ref = Some(type_ref.into());
        self
    }

    /// Sets the occurrence bounds.
    pub fn with_occurs(mut self, min: u32, max: Option<u32>) -> Self {
        self.min_occurs = min;
        self.max_occurs = max;
        self
    }

    /// Makes this element optional (minOccurs=0).
    pub fn optional(mut self) -> Self {
        self.min_occurs = 0;
        self
    }

    /// Makes this element repeatable (maxOccurs=unbounded).
    pub fn unbounded(mut self) -> Self {
        self.max_occurs = None;
        self
    }
}

/// A type definition.
#[derive(Debug, Clone)]
pub enum TypeDef {
    /// Simple type (string, integer, etc.)
    Simple(SimpleType),
    /// Complex type (elements, attributes)
    Complex(ComplexType),
}

/// A simple type definition.
#[derive(Debug, Clone)]
pub struct SimpleType {
    /// Type name
    pub name: String,
    /// Base type (restriction or extension)
    pub base_type: Option<String>,
    /// Namespace-resolved form of [`base_type`](Self::base_type). `None` for
    /// the synthetic `list(...)`/`union(...)` markers, which are not QNames.
    pub base_ns: Option<NsName>,
    /// Enumeration values
    pub enumeration: Vec<String>,
    /// Pattern restriction
    pub pattern: Option<String>,
    /// Exact length restriction
    pub length: Option<u32>,
    /// Min length restriction
    pub min_length: Option<u32>,
    /// Max length restriction
    pub max_length: Option<u32>,
    /// Minimum value (inclusive)
    pub min_inclusive: Option<String>,
    /// Maximum value (inclusive)
    pub max_inclusive: Option<String>,
    /// Minimum value (exclusive)
    pub min_exclusive: Option<String>,
    /// Maximum value (exclusive)
    pub max_exclusive: Option<String>,
    /// Total digits restriction
    pub total_digits: Option<u32>,
    /// Fraction digits restriction
    pub fraction_digits: Option<u32>,
    /// Item type for list types (`<xs:list itemType="..."/>`)
    pub item_type: Option<String>,
    /// Namespace-resolved form of [`item_type`](Self::item_type).
    pub item_ns: Option<NsName>,
    /// Member types for union types (`<xs:union memberTypes="..."/>`)
    pub member_types: Vec<String>,
    /// Namespace-resolved form of each entry in
    /// [`member_types`](Self::member_types) (index-aligned; `None` where the
    /// member reference could not be resolved).
    pub member_ns: Vec<Option<NsName>>,
    /// Whitespace normalization declared by a whiteSpace facet
    pub white_space: Option<WhiteSpace>,
    /// Explicit timezone requirement (XSD 1.1 explicitTimezone facet)
    pub explicit_timezone: Option<ExplicitTimezone>,
}

/// explicitTimezone facet values (XSD 1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplicitTimezone {
    /// The value must carry a timezone
    Required,
    /// The value must not carry a timezone
    Prohibited,
    /// Either is fine
    Optional,
}

/// Whitespace normalization mode declared by an `xs:whiteSpace` facet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhiteSpace {
    /// Preserve all whitespace as-is
    Preserve,
    /// Replace tab/newline/carriage-return with spaces
    Replace,
    /// Collapse whitespace runs and trim
    Collapse,
}

impl SimpleType {
    /// Creates a new simple type.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            base_type: None,
            base_ns: None,
            enumeration: Vec::new(),
            pattern: None,
            length: None,
            min_length: None,
            max_length: None,
            min_inclusive: None,
            max_inclusive: None,
            min_exclusive: None,
            max_exclusive: None,
            total_digits: None,
            fraction_digits: None,
            item_type: None,
            item_ns: None,
            member_types: Vec::new(),
            member_ns: Vec::new(),
            white_space: None,
            explicit_timezone: None,
        }
    }

    /// Creates a string type.
    pub fn string() -> Self {
        Self::new("string").with_base("xs:string")
    }

    /// Creates an integer type.
    pub fn integer() -> Self {
        Self::new("integer").with_base("xs:integer")
    }

    /// Sets the base type.
    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base_type = Some(base.into());
        self
    }

    /// Sets the whiteSpace facet.
    pub fn with_white_space(mut self, ws: WhiteSpace) -> Self {
        self.white_space = Some(ws);
        self
    }
}

/// How a type is derived from its base type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivationMethod {
    /// Derived by extension
    Extension,
    /// Derived by restriction
    Restriction,
}

/// Derivation methods blocked by a `block` attribute.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlockSet {
    /// `block` includes `extension` (or `#all`)
    pub extension: bool,
    /// `block` includes `restriction` (or `#all`)
    pub restriction: bool,
}

impl BlockSet {
    /// Whether the given derivation method is blocked.
    pub fn blocks(&self, method: DerivationMethod) -> bool {
        match method {
            DerivationMethod::Extension => self.extension,
            DerivationMethod::Restriction => self.restriction,
        }
    }
}

/// A complex type definition.
#[derive(Debug, Clone)]
pub struct ComplexType {
    /// Type name
    pub name: String,
    /// Base type (for extension or restriction)
    pub base_type: Option<String>,
    /// Namespace-resolved form of [`base_type`](Self::base_type), computed at
    /// compile time against the owning document's bindings.
    pub base_ns: Option<NsName>,
    /// How this type derives from `base_type`
    pub derivation: Option<DerivationMethod>,
    /// Derivation methods blocked for xsi:type substitution
    pub block: BlockSet,
    /// Content model
    pub content: ContentModel,
    /// Attribute definitions
    pub attributes: Vec<AttributeDef>,
    /// Element wildcard (xs:any) anywhere in the content model
    pub wildcard: Option<WildcardConstraint>,
    /// Attribute wildcard (xs:anyAttribute)
    pub attr_wildcard: Option<WildcardConstraint>,
    /// Whether this type is abstract
    pub is_abstract: bool,
    /// Whether content is mixed (text + elements)
    pub mixed: bool,
    /// Nested particle tree of this type's own content (excluding the
    /// base chain), used to build the content-model automaton
    pub particle: Option<std::sync::Arc<Particle>>,
}

impl ComplexType {
    /// Creates a new complex type.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            base_type: None,
            base_ns: None,
            derivation: None,
            block: BlockSet::default(),
            content: ContentModel::Empty,
            attributes: Vec::new(),
            wildcard: None,
            attr_wildcard: None,
            is_abstract: false,
            mixed: false,
            particle: None,
        }
    }

    /// Creates a complex type with sequence content.
    pub fn sequence(name: impl Into<String>, elements: Vec<ElementDef>) -> Self {
        Self {
            name: name.into(),
            base_type: None,
            base_ns: None,
            derivation: None,
            block: BlockSet::default(),
            content: ContentModel::Sequence(elements),
            attributes: Vec::new(),
            wildcard: None,
            attr_wildcard: None,
            is_abstract: false,
            mixed: false,
            particle: None,
        }
    }
}

/// A compiled content-model particle preserving the nested compositor
/// structure. [`ContentModel`] flattens compositors for the legacy
/// count-based checks; this tree is what the content-model automaton is
/// built from, so cross-compositor rules (choice totals, sequence-as-unit
/// occurrence counting) can be enforced.
#[derive(Debug, Clone)]
pub enum Particle {
    /// An element declaration (occurrence bounds live on the def)
    Element(ElementDef),
    /// An `xs:any` wildcard (occurrence bounds live on the constraint)
    Wildcard(WildcardConstraint),
    /// `xs:sequence`
    Sequence {
        /// Minimum occurrences of the whole group
        min: u32,
        /// Maximum occurrences of the whole group (None = unbounded)
        max: Option<u32>,
        /// Child particles in order
        items: Vec<Particle>,
    },
    /// `xs:choice`
    Choice {
        /// Minimum occurrences of the whole group
        min: u32,
        /// Maximum occurrences of the whole group (None = unbounded)
        max: Option<u32>,
        /// Alternative particles
        items: Vec<Particle>,
    },
    /// `xs:all` (validated by counting, not by the automaton)
    All {
        /// Minimum occurrences of the group (0 or 1)
        min: u32,
        /// Member elements
        elements: Vec<ElementDef>,
    },
}

/// Content model for complex types.
#[derive(Debug, Clone)]
pub enum ContentModel {
    /// Empty content
    Empty,
    /// Sequence of elements (in order)
    Sequence(Vec<ElementDef>),
    /// Choice of elements (one of)
    Choice(Vec<ElementDef>),
    /// All (unordered)
    All(Vec<ElementDef>),
    /// Simple content (text with optional extension)
    SimpleContent {
        /// Base type for the content
        base_type: String,
    },
    /// Complex content extension
    ComplexExtension {
        /// Base type being extended
        base_type: String,
        /// Additional child elements
        elements: Vec<ElementDef>,
    },
    /// Any content (wildcard)
    Any {
        /// Target namespace for wildcard
        namespace: Option<String>,
        /// How to process the content
        process_contents: ProcessContents,
    },
}

/// A compiled element wildcard (`xs:any`).
#[derive(Debug, Clone)]
pub struct WildcardConstraint {
    /// Namespace constraint
    pub namespace: WildcardNamespace,
    /// How matched content is processed
    pub process_contents: ProcessContents,
    /// Minimum number of matched elements
    pub min_occurs: u32,
    /// Maximum number of matched elements (None = unbounded)
    pub max_occurs: Option<u32>,
    /// The schema's target namespace (used by `##other`)
    pub target_namespace: Option<String>,
}

impl WildcardConstraint {
    /// Whether an element in `ns` is admitted by this wildcard.
    pub fn matches(&self, ns: Option<&str>) -> bool {
        match &self.namespace {
            WildcardNamespace::Any => true,
            WildcardNamespace::Other => {
                // ##other: a namespace other than the target namespace;
                // absent namespaces are not admitted.
                match ns {
                    Some(ns) => Some(ns) != self.target_namespace.as_deref(),
                    None => false,
                }
            }
            WildcardNamespace::List(uris) => uris
                .iter()
                .any(|u| (u.is_empty() && ns.is_none()) || Some(u.as_str()) == ns),
        }
    }
}

/// Namespace constraint of a wildcard.
#[derive(Debug, Clone)]
pub enum WildcardNamespace {
    /// `##any`
    Any,
    /// `##other` (any namespace other than the target namespace)
    Other,
    /// An explicit list of namespace URIs (the empty string means
    /// "no namespace", from `##local`)
    List(Vec<String>),
}

/// How to process wildcard content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessContents {
    /// Strict - must validate
    Strict,
    /// Lax - validate if schema available
    Lax,
    /// Skip - don't validate
    Skip,
}

/// An attribute definition.
#[derive(Debug, Clone)]
pub struct AttributeDef {
    /// Attribute name
    pub name: String,
    /// Type reference
    pub type_ref: Option<String>,
    /// Namespace-resolved form of [`type_ref`](Self::type_ref).
    pub type_ns: Option<NsName>,
    /// For an attribute *reference* (`<xs:attribute ref="q:name"/>`), the
    /// resolved namespace of the referenced global attribute. `name` keeps
    /// the bare local for message stability.
    pub ref_ns: Option<NsName>,
    /// Inline simple type definition
    pub inline_type: Option<Box<SimpleType>>,
    /// Whether the attribute is required
    pub required: bool,
    /// Default value
    pub default: Option<String>,
    /// Fixed value
    pub fixed: Option<String>,
    /// Whether this is a reference to a globally declared attribute
    pub is_ref: bool,
}

impl AttributeDef {
    /// Creates a new attribute definition.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_ref: None,
            type_ns: None,
            ref_ns: None,
            inline_type: None,
            required: false,
            default: None,
            fixed: None,
            is_ref: false,
        }
    }

    /// Creates a required attribute.
    pub fn required(name: impl Into<String>) -> Self {
        Self {
            required: true,
            ..Self::new(name)
        }
    }

    /// Sets the type reference.
    pub fn with_type(mut self, type_ref: impl Into<String>) -> Self {
        self.type_ref = Some(type_ref.into());
        self
    }

    /// Sets a default value.
    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self
    }
}

/// Type of compiled identity constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledConstraintType {
    /// Values must be unique (null allowed)
    Unique,
    /// Values must be unique and non-null
    Key,
    /// Values must reference an existing key
    KeyRef,
}

/// A compiled identity constraint ready for validation.
#[derive(Debug, Clone)]
pub struct CompiledConstraint {
    /// Constraint name
    pub name: String,
    /// Type of constraint
    pub constraint_type: CompiledConstraintType,
    /// XPath selector expression (for selecting scope)
    pub selector_xpath: String,
    /// XPath field expressions (for selecting key values)
    pub field_xpaths: Vec<String>,
    /// For keyref: the key being referenced
    pub refer: Option<String>,
}

impl CompiledConstraint {
    /// Creates a new compiled unique constraint.
    pub fn unique(name: impl Into<String>, selector: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            constraint_type: CompiledConstraintType::Unique,
            selector_xpath: selector.into(),
            field_xpaths: Vec::new(),
            refer: None,
        }
    }

    /// Creates a new compiled key constraint.
    pub fn key(name: impl Into<String>, selector: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            constraint_type: CompiledConstraintType::Key,
            selector_xpath: selector.into(),
            field_xpaths: Vec::new(),
            refer: None,
        }
    }

    /// Creates a new compiled keyref constraint.
    pub fn keyref(
        name: impl Into<String>,
        selector: impl Into<String>,
        refer: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            constraint_type: CompiledConstraintType::KeyRef,
            selector_xpath: selector.into(),
            field_xpaths: Vec::new(),
            refer: Some(refer.into()),
        }
    }

    /// Adds a field XPath expression.
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field_xpaths.push(field.into());
        self
    }

    /// Adds multiple field XPath expressions.
    pub fn with_fields(mut self, fields: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.field_xpaths.extend(fields.into_iter().map(Into::into));
        self
    }
}

/// Built-in XSD types.
pub mod builtin {
    /// Built-in string type name.
    pub const STRING: &str = "xs:string";
    /// Built-in integer type name.
    pub const INTEGER: &str = "xs:integer";
    /// Built-in decimal type name.
    pub const DECIMAL: &str = "xs:decimal";
    /// Built-in boolean type name.
    pub const BOOLEAN: &str = "xs:boolean";
    /// Built-in date type name.
    pub const DATE: &str = "xs:date";
    /// Built-in dateTime type name.
    pub const DATE_TIME: &str = "xs:dateTime";
    /// Built-in double type name.
    pub const DOUBLE: &str = "xs:double";
    /// Built-in float type name.
    pub const FLOAT: &str = "xs:float";
    /// Built-in anyURI type name.
    pub const ANY_URI: &str = "xs:anyURI";
    /// Built-in ID type name.
    pub const ID: &str = "xs:ID";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_element_with_local_name() {
        let mut schema = CompiledSchema::new();
        schema.elements_ns.insert(
            crate::schema::types::NsName::new("", "ReliefFeature"),
            ElementDef::new("ReliefFeature"),
        );

        // Local name lookup should work
        assert!(schema.get_element("ReliefFeature").is_some());
    }

    #[test]
    fn test_get_element_with_qualified_name() {
        let mut schema = CompiledSchema::new();
        schema.elements_ns.insert(
            crate::schema::types::NsName::new("", "ReliefFeature"),
            ElementDef::new("ReliefFeature"),
        );

        // Qualified name lookup should fall back to local name
        assert!(
            schema.get_element("dem:ReliefFeature").is_some(),
            "Should find 'ReliefFeature' when looking up 'dem:ReliefFeature'"
        );
    }

    #[test]
    fn test_get_type_with_local_name() {
        let mut schema = CompiledSchema::new();
        schema.types_ns.insert(
            crate::schema::types::NsName::new("", "AbstractCityObjectType"),
            TypeDef::Complex(ComplexType::new("AbstractCityObjectType")),
        );

        // Local name lookup should work
        assert!(schema.get_type("AbstractCityObjectType").is_some());
    }

    #[test]
    fn test_get_type_with_qualified_name() {
        let mut schema = CompiledSchema::new();
        schema.types_ns.insert(
            crate::schema::types::NsName::new("", "AbstractCityObjectType"),
            TypeDef::Complex(ComplexType::new("AbstractCityObjectType")),
        );

        // Qualified name lookup should fall back to local name
        assert!(
            schema.get_type("core:AbstractCityObjectType").is_some(),
            "Should find 'AbstractCityObjectType' when looking up 'core:AbstractCityObjectType'"
        );
    }

    #[test]
    fn test_get_type_not_found() {
        let schema = CompiledSchema::new();
        assert!(schema.get_type("NonExistentType").is_none());
        assert!(schema.get_type("prefix:NonExistentType").is_none());
    }

    #[test]
    fn test_get_element_not_found() {
        let schema = CompiledSchema::new();
        assert!(schema.get_element("NonExistentElement").is_none());
        assert!(schema.get_element("prefix:NonExistentElement").is_none());
    }

    // === Namespace-qualified accessor semantics (C3) ===

    /// Two globals sharing a local name in different namespaces must not
    /// collide on the namespace-keyed map (the wildG031 class).
    #[test]
    fn ns_maps_resolve_same_local_in_different_namespaces() {
        const NS_A: &str = "http://example.com/a";
        const NS_B: &str = "http://example.com/b";
        let mut schema = CompiledSchema::new();
        let mut a = ElementDef::new("value");
        a.type_ref = Some("a:AType".into());
        let mut b = ElementDef::new("value");
        b.type_ref = Some("b:BType".into());
        schema.elements_ns.insert(NsName::new(NS_A, "value"), a);
        schema.elements_ns.insert(NsName::new(NS_B, "value"), b);

        // Each namespace resolves to its OWN declaration, not the other's.
        assert_eq!(
            schema
                .element_ns(NS_A, "value")
                .unwrap()
                .type_ref
                .as_deref(),
            Some("a:AType")
        );
        assert_eq!(
            schema
                .element_ns(NS_B, "value")
                .unwrap()
                .type_ref
                .as_deref(),
            Some("b:BType")
        );
    }

    /// A cleanly-resolved namespace that misses (with no chameleon `""`
    /// entry) must stay a miss — `element_ns` must NOT scan other namespaces
    /// (amendment #1: the any-ns scan can only hide "not declared" errors).
    #[test]
    fn ns_element_lookup_does_not_leak_across_namespaces() {
        const NS_A: &str = "http://example.com/a";
        const NS_C: &str = "http://example.com/c";
        let mut schema = CompiledSchema::new();
        schema
            .elements_ns
            .insert(NsName::new(NS_A, "value"), ElementDef::new("value"));

        // Wrong namespace -> strict miss (no cross-namespace leak).
        assert!(schema.element_ns(NS_C, "value").is_none());
        // The leniency scan (only for unresolvable instance namespaces) DOES
        // find it by local name.
        assert!(schema.element_ns_any("value").is_some());
    }

    /// Chameleon-include fallback: a component registered under the
    /// no-namespace `""` is reachable from a qualified lookup.
    #[test]
    fn ns_type_lookup_chameleon_fallback() {
        const NS_A: &str = "http://example.com/a";
        let mut schema = CompiledSchema::new();
        schema.types_ns.insert(
            NsName::new("", "ChameleonType"),
            TypeDef::Complex(ComplexType::new("ChameleonType")),
        );
        assert!(schema.type_ns(NS_A, "ChameleonType").is_some());
        assert!(schema.type_ns("", "ChameleonType").is_some());
        assert!(schema.type_ns(NS_A, "Missing").is_none());
    }
}
