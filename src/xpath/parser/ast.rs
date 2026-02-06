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
    /// Match any node `*`
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
}

/// A predicate expression.
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
    /// Union of paths (path1 | path2)
    Union(Vec<PathExpr>),
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
