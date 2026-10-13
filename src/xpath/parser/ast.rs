//! XPath AST (Abstract Syntax Tree) types.

/// XPath axis specifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// `child::` - direct children
    Child,
    /// `descendant::` - all descendants
    Descendant,
    /// `parent::` - direct parent
    Parent,
    /// `self::` - the node itself
    SelfNode,
    /// `descendant-or-self::` - self and all descendants
    DescendantOrSelf,
    /// `ancestor::` - all ancestors
    Ancestor,
    /// `ancestor-or-self::` - self and all ancestors
    AncestorOrSelf,
    /// `following-sibling::` - following siblings
    FollowingSibling,
    /// `preceding-sibling::` - preceding siblings
    PrecedingSibling,
    /// `following::` - all following nodes in document order
    Following,
    /// `preceding::` - all preceding nodes in document order
    Preceding,
    /// `attribute::` - attributes
    Attribute,
    /// `namespace::` - namespace nodes
    Namespace,
}

/// Node test in a step.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeTest {
    /// `*`: any node of the axis's principal node type (elements, or
    /// attributes on the `attribute` axis, or namespaces on the `namespace`
    /// axis). Use [`NodeTest::Node`] for any node of any type.
    Any,
    /// Match nodes with this name
    Name(String),
    /// Match nodes with prefix and local name
    QName {
        /// Namespace prefix
        prefix: String,
        /// Local name
        local: String,
    },
    /// Match text nodes `text()`
    Text,
    /// Match any node `node()`
    Node,
    /// Match comment nodes `comment()`
    Comment,
    /// Match processing instructions `processing-instruction()`, optionally
    /// only those with the given target (`processing-instruction('target')`)
    ProcessingInstruction(Option<String>),
}

/// A predicate expression.
///
/// The parser reads a predicate with the same grammar as any [`Expr`] and then
/// lifts its top-level structure into this form: `or` / `and` / comparison /
/// a one-argument `not()` become the matching variant, a positive integer
/// literal becomes [`Predicate::Position`], and anything else is
/// [`Predicate::Expr`]. Operands of `and` / `or` / `not()` are lifted the same
/// way except that a number there is a boolean, never a position.
#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    /// Comparison: left op right
    Comparison {
        /// Left operand
        left: Box<Expr>,
        /// Comparison operator
        op: ComparisonOp,
        /// Right operand
        right: Box<Expr>,
    },
    /// Logical AND
    And(Box<Predicate>, Box<Predicate>),
    /// Logical OR
    Or(Box<Predicate>, Box<Predicate>),
    /// Logical NOT
    Not(Box<Predicate>),
    /// Position predicate (number)
    Position(usize),
    /// An expression used as boolean test
    Expr(Box<Expr>),
}

/// Comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonOp {
    /// `=` equality
    Equal,
    /// `!=` inequality
    NotEqual,
    /// `<` less than
    LessThan,
    /// `<=` less than or equal
    LessOrEqual,
    /// `>` greater than
    GreaterThan,
    /// `>=` greater than or equal
    GreaterOrEqual,
}

/// XPath expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Path expression (sequence of steps)
    Path(PathExpr),
    /// String literal
    String(String),
    /// Number literal
    Number(f64),
    /// Variable reference ($name)
    Variable(String),
    /// Function call
    Function {
        /// Function name
        name: String,
        /// Function arguments
        args: Vec<Expr>,
    },
    /// Union (`a | b | ...`). Each operand must evaluate to a node-set.
    Union(Vec<Expr>),
    /// Comparison (`left op right`) with `=`, `!=`, `<`, `<=`, `>`, `>=`
    Comparison {
        /// Left operand
        left: Box<Expr>,
        /// Comparison operator
        op: ComparisonOp,
        /// Right operand
        right: Box<Expr>,
    },
    /// Logical AND (`left and right`)
    And(Box<Expr>, Box<Expr>),
    /// Logical OR (`left or right`)
    Or(Box<Expr>, Box<Expr>),
    /// Filter expression: a primary expression followed by predicates,
    /// e.g. `(//a)[1]` or `$nodes[@id]`. The predicates filter the node-set
    /// in document order.
    Filter {
        /// The filtered expression (must evaluate to a node-set)
        expr: Box<Expr>,
        /// Predicates, applied in order
        predicates: Vec<Predicate>,
    },
    /// A location path that continues from a filter expression,
    /// e.g. `(//a)/text()` or `id('x')//b`.
    PathFrom {
        /// The starting expression (must evaluate to a node-set)
        base: Box<Expr>,
        /// The steps after the first `/` or `//`, with the same conventions as
        /// [`PathExpr::steps`] (`//` is a `descendant-or-self::node()` step)
        steps: Vec<Step>,
    },
    /// Addition (left + right)
    Add(Box<Expr>, Box<Expr>),
    /// Subtraction (left - right)
    Subtract(Box<Expr>, Box<Expr>),
    /// Multiplication (left * right)
    Multiply(Box<Expr>, Box<Expr>),
    /// Division (left div right)
    Divide(Box<Expr>, Box<Expr>),
    /// Modulo (left mod right)
    Modulo(Box<Expr>, Box<Expr>),
    /// Unary negation (-expr)
    Negate(Box<Expr>),
}

/// A location path (absolute or relative).
#[derive(Debug, Clone, PartialEq)]
pub struct PathExpr {
    /// Whether the path starts with `/`
    pub absolute: bool,
    /// The steps in the path
    pub steps: Vec<Step>,
}

/// A single step in a location path.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// The axis
    pub axis: Axis,
    /// The node test
    pub node_test: NodeTest,
    /// Predicates
    pub predicates: Vec<Predicate>,
}

impl Step {
    /// Creates a child axis step with the given name.
    pub fn child(name: &str) -> Self {
        Self {
            axis: Axis::Child,
            node_test: Self::parse_name(name),
            predicates: Vec::new(),
        }
    }

    /// Creates a descendant-or-self step matching any node.
    /// This is used for the // abbreviation in XPath, which is
    /// defined as /descendant-or-self::node()/ per the spec.
    pub fn descendant_or_self_any() -> Self {
        Self {
            axis: Axis::DescendantOrSelf,
            // Use NodeTest::Node to match all nodes (not just elements)
            // This is required because // in XPath is /descendant-or-self::node()/
            node_test: NodeTest::Node,
            predicates: Vec::new(),
        }
    }

    fn parse_name(name: &str) -> NodeTest {
        if name == "*" {
            NodeTest::Any
        } else if let Some((prefix, local)) = name.split_once(':') {
            NodeTest::QName {
                prefix: prefix.to_string(),
                local: local.to_string(),
            }
        } else {
            NodeTest::Name(name.to_string())
        }
    }
}
