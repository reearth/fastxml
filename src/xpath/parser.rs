//! XPath expression parser.
//!
//! Parses tokenized XPath expressions into an abstract syntax tree (AST).

use crate::error::{Error, Result};
use super::lexer::{Lexer, Token};

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
    pub fn descendant_or_self_any() -> Self {
        Self {
            axis: Axis::DescendantOrSelf,
            node_test: NodeTest::Any,
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

/// XPath expression parser.
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    /// Creates a new parser from an XPath expression string.
    pub fn new(xpath: &str) -> Result<Self> {
        let mut lexer = Lexer::new(xpath);
        let tokens = lexer.tokenize()?;
        Ok(Self { tokens, pos: 0 })
    }

    /// Parses the expression.
    pub fn parse(&mut self) -> Result<Expr> {
        self.parse_union_expr()
    }

    fn current(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) {
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
    }

    fn expect(&mut self, expected: &Token) -> Result<()> {
        if self.current() == expected {
            self.advance();
            Ok(())
        } else {
            Err(Error::XPathSyntax(format!(
                "expected {:?}, found {:?}",
                expected,
                self.current()
            )))
        }
    }

    fn parse_union_expr(&mut self) -> Result<Expr> {
        // Use the additive expression chain which properly handles
        // function calls, paths, and arithmetic expressions
        self.parse_additive_expr()
    }

    fn parse_path_expr(&mut self) -> Result<PathExpr> {
        let mut absolute = false;
        let mut steps = Vec::new();

        // Handle leading / or //
        match self.current() {
            Token::Slash => {
                absolute = true;
                self.advance();
            }
            Token::DoubleSlash => {
                absolute = true;
                self.advance();
                // // is shorthand for /descendant-or-self::node()/
                steps.push(Step::descendant_or_self_any());
            }
            _ => {}
        }

        // Parse steps
        if !matches!(self.current(), Token::Eof | Token::Pipe | Token::RightBracket | Token::RightParen) {
            steps.push(self.parse_step()?);

            while matches!(self.current(), Token::Slash | Token::DoubleSlash) {
                if matches!(self.current(), Token::DoubleSlash) {
                    self.advance();
                    steps.push(Step::descendant_or_self_any());
                } else {
                    self.advance();
                }

                if !matches!(self.current(), Token::Eof | Token::Pipe | Token::RightBracket | Token::RightParen) {
                    steps.push(self.parse_step()?);
                }
            }
        }

        Ok(PathExpr { absolute, steps })
    }

    fn parse_step(&mut self) -> Result<Step> {
        // Handle abbreviated syntax
        match self.current() {
            Token::Dot => {
                self.advance();
                return Ok(Step {
                    axis: Axis::SelfNode,
                    node_test: NodeTest::Node,
                    predicates: Vec::new(),
                });
            }
            Token::DoubleDot => {
                self.advance();
                return Ok(Step {
                    axis: Axis::Parent,
                    node_test: NodeTest::Node,
                    predicates: Vec::new(),
                });
            }
            Token::At => {
                self.advance();
                let node_test = self.parse_node_test()?;
                let predicates = self.parse_predicates()?;
                return Ok(Step {
                    axis: Axis::Attribute,
                    node_test,
                    predicates,
                });
            }
            _ => {}
        }

        // Check for axis specifier
        let axis = self.parse_axis()?;
        let node_test = self.parse_node_test()?;
        let predicates = self.parse_predicates()?;

        Ok(Step {
            axis,
            node_test,
            predicates,
        })
    }

    fn parse_axis(&mut self) -> Result<Axis> {
        let axis = match self.current() {
            Token::ChildAxis => Some(Axis::Child),
            Token::DescendantAxis => Some(Axis::Descendant),
            Token::ParentAxis => Some(Axis::Parent),
            Token::SelfAxis => Some(Axis::SelfNode),
            Token::DescendantOrSelfAxis => Some(Axis::DescendantOrSelf),
            Token::AncestorAxis => Some(Axis::Ancestor),
            Token::AncestorOrSelfAxis => Some(Axis::AncestorOrSelf),
            Token::FollowingSiblingAxis => Some(Axis::FollowingSibling),
            Token::PrecedingSiblingAxis => Some(Axis::PrecedingSibling),
            Token::FollowingAxis => Some(Axis::Following),
            Token::PrecedingAxis => Some(Axis::Preceding),
            Token::AttributeAxis => Some(Axis::Attribute),
            Token::NamespaceAxis => Some(Axis::Namespace),
            _ => None,
        };

        if let Some(axis) = axis {
            self.advance();
            self.expect(&Token::DoubleColon)?;
            Ok(axis)
        } else {
            // Default axis is child
            Ok(Axis::Child)
        }
    }

    fn parse_node_test(&mut self) -> Result<NodeTest> {
        match self.current() {
            Token::Asterisk => {
                self.advance();
                Ok(NodeTest::Any)
            }
            Token::Name(name) => {
                let name = name.clone();
                self.advance();
                if let Some((prefix, local)) = name.split_once(':') {
                    Ok(NodeTest::QName {
                        prefix: prefix.to_string(),
                        local: local.to_string(),
                    })
                } else {
                    Ok(NodeTest::Name(name))
                }
            }
            Token::TextFn => {
                self.advance();
                self.expect(&Token::LeftParen)?;
                self.expect(&Token::RightParen)?;
                Ok(NodeTest::Text)
            }
            Token::NodeFn => {
                self.advance();
                self.expect(&Token::LeftParen)?;
                self.expect(&Token::RightParen)?;
                Ok(NodeTest::Node)
            }
            _ => Err(Error::XPathSyntax(format!(
                "expected node test, found {:?}",
                self.current()
            ))),
        }
    }

    fn parse_predicates(&mut self) -> Result<Vec<Predicate>> {
        let mut predicates = Vec::new();

        while matches!(self.current(), Token::LeftBracket) {
            self.advance();
            let pred = self.parse_predicate()?;
            predicates.push(pred);
            self.expect(&Token::RightBracket)?;
        }

        Ok(predicates)
    }

    fn parse_predicate(&mut self) -> Result<Predicate> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Result<Predicate> {
        let mut left = self.parse_and_expr()?;

        while matches!(self.current(), Token::Or) {
            self.advance();
            let right = self.parse_and_expr()?;
            left = Predicate::Or(Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_and_expr(&mut self) -> Result<Predicate> {
        let mut left = self.parse_primary_predicate()?;

        while matches!(self.current(), Token::And) {
            self.advance();
            let right = self.parse_primary_predicate()?;
            left = Predicate::And(Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_primary_predicate(&mut self) -> Result<Predicate> {
        // Handle not()
        if matches!(self.current(), Token::Not) {
            self.advance();
            self.expect(&Token::LeftParen)?;
            let inner = self.parse_predicate()?;
            self.expect(&Token::RightParen)?;
            return Ok(Predicate::Not(Box::new(inner)));
        }

        // Handle parenthesized expression
        if matches!(self.current(), Token::LeftParen) {
            self.advance();
            let inner = self.parse_predicate()?;
            self.expect(&Token::RightParen)?;
            return Ok(inner);
        }

        // Parse expression and check for comparison
        let left = self.parse_expr_value()?;

        let op = match self.current() {
            Token::Equals => Some(ComparisonOp::Equal),
            Token::NotEquals => Some(ComparisonOp::NotEqual),
            Token::LessThan => Some(ComparisonOp::LessThan),
            Token::LessOrEqual => Some(ComparisonOp::LessOrEqual),
            Token::GreaterThan => Some(ComparisonOp::GreaterThan),
            Token::GreaterOrEqual => Some(ComparisonOp::GreaterOrEqual),
            _ => None,
        };

        if let Some(op) = op {
            self.advance();
            let right = self.parse_expr_value()?;
            Ok(Predicate::Comparison {
                left: Box::new(left),
                op,
                right: Box::new(right),
            })
        } else {
            // Could be position predicate or boolean expression
            if let Expr::Number(n) = &left {
                Ok(Predicate::Position(*n as usize))
            } else {
                Ok(Predicate::Expr(Box::new(left)))
            }
        }
    }

    fn parse_expr_value(&mut self) -> Result<Expr> {
        // Handle unary minus
        if matches!(self.current(), Token::Minus) {
            self.advance();
            let inner = self.parse_expr_value()?;
            return Ok(Expr::Negate(Box::new(inner)));
        }

        match self.current() {
            Token::String(s) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::String(s))
            }
            Token::Number(n) => {
                let n = *n;
                self.advance();
                Ok(Expr::Number(n))
            }
            // All function tokens
            Token::NameFn | Token::TextFn | Token::LocalNameFn | Token::NamespaceUriFn |
            Token::ContainsFn | Token::StartsWithFn | Token::Not |
            // New functions
            Token::StringFn | Token::ConcatFn | Token::SubstringFn |
            Token::SubstringBeforeFn | Token::SubstringAfterFn |
            Token::StringLengthFn | Token::NormalizeSpaceFn | Token::TranslateFn |
            Token::PositionFn | Token::LastFn | Token::CountFn | Token::IdFn |
            Token::TrueFn | Token::FalseFn | Token::BooleanFn | Token::LangFn |
            Token::NumberFn | Token::SumFn | Token::FloorFn | Token::CeilingFn | Token::RoundFn => {
                self.parse_function_call()
            }
            Token::LeftParen => {
                self.advance();
                let inner = self.parse_additive_expr()?;
                self.expect(&Token::RightParen)?;
                Ok(inner)
            }
            Token::Name(_) | Token::Slash | Token::DoubleSlash | Token::Dot | Token::At |
            Token::Asterisk => {
                let path = self.parse_path_expr()?;
                Ok(Expr::Path(path))
            }
            _ => Err(Error::XPathSyntax(format!(
                "expected expression value, found {:?}",
                self.current()
            ))),
        }
    }

    /// Parses an additive expression: expr ('+' | '-') expr
    fn parse_additive_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplicative_expr()?;

        loop {
            match self.current() {
                Token::Plus => {
                    self.advance();
                    let right = self.parse_multiplicative_expr()?;
                    left = Expr::Add(Box::new(left), Box::new(right));
                }
                Token::Minus => {
                    self.advance();
                    let right = self.parse_multiplicative_expr()?;
                    left = Expr::Subtract(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }

        Ok(left)
    }

    /// Parses a multiplicative expression: expr ('*' | 'div' | 'mod') expr
    fn parse_multiplicative_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary_expr()?;

        loop {
            match self.current() {
                // Note: Asterisk is tricky - could be multiply or node test
                // In expression context after a value, it's multiply
                Token::Div => {
                    self.advance();
                    let right = self.parse_unary_expr()?;
                    left = Expr::Divide(Box::new(left), Box::new(right));
                }
                Token::Mod => {
                    self.advance();
                    let right = self.parse_unary_expr()?;
                    left = Expr::Modulo(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }

        Ok(left)
    }

    /// Parses a unary expression: '-' expr | primary
    fn parse_unary_expr(&mut self) -> Result<Expr> {
        if matches!(self.current(), Token::Minus) {
            self.advance();
            let inner = self.parse_unary_expr()?;
            Ok(Expr::Negate(Box::new(inner)))
        } else {
            self.parse_primary_expr()
        }
    }

    /// Parses a primary expression (path, literal, function, or parenthesized)
    fn parse_primary_expr(&mut self) -> Result<Expr> {
        match self.current() {
            Token::String(s) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::String(s))
            }
            Token::Number(n) => {
                let n = *n;
                self.advance();
                Ok(Expr::Number(n))
            }
            Token::LeftParen => {
                self.advance();
                let inner = self.parse_additive_expr()?;
                self.expect(&Token::RightParen)?;
                Ok(inner)
            }
            // Function calls
            Token::NameFn | Token::TextFn | Token::LocalNameFn | Token::NamespaceUriFn |
            Token::ContainsFn | Token::StartsWithFn | Token::Not |
            Token::StringFn | Token::ConcatFn | Token::SubstringFn |
            Token::SubstringBeforeFn | Token::SubstringAfterFn |
            Token::StringLengthFn | Token::NormalizeSpaceFn | Token::TranslateFn |
            Token::PositionFn | Token::LastFn | Token::CountFn | Token::IdFn |
            Token::TrueFn | Token::FalseFn | Token::BooleanFn | Token::LangFn |
            Token::NumberFn | Token::SumFn | Token::FloorFn | Token::CeilingFn | Token::RoundFn => {
                self.parse_function_call()
            }
            // Path expressions
            Token::Name(_) | Token::Slash | Token::DoubleSlash | Token::Dot | Token::At |
            Token::Asterisk => {
                let path = self.parse_path_expr()?;
                Ok(Expr::Path(path))
            }
            _ => Err(Error::XPathSyntax(format!(
                "expected primary expression, found {:?}",
                self.current()
            ))),
        }
    }

    fn parse_function_call(&mut self) -> Result<Expr> {
        let name = match self.current() {
            // Node set functions
            Token::NameFn => "name",
            Token::LocalNameFn => "local-name",
            Token::NamespaceUriFn => "namespace-uri",
            Token::PositionFn => "position",
            Token::LastFn => "last",
            Token::CountFn => "count",
            Token::IdFn => "id",

            // String functions
            Token::TextFn => "text",
            Token::StringFn => "string",
            Token::ConcatFn => "concat",
            Token::ContainsFn => "contains",
            Token::StartsWithFn => "starts-with",
            Token::SubstringFn => "substring",
            Token::SubstringBeforeFn => "substring-before",
            Token::SubstringAfterFn => "substring-after",
            Token::StringLengthFn => "string-length",
            Token::NormalizeSpaceFn => "normalize-space",
            Token::TranslateFn => "translate",

            // Boolean functions
            Token::Not => "not",
            Token::TrueFn => "true",
            Token::FalseFn => "false",
            Token::BooleanFn => "boolean",
            Token::LangFn => "lang",

            // Number functions
            Token::NumberFn => "number",
            Token::SumFn => "sum",
            Token::FloorFn => "floor",
            Token::CeilingFn => "ceiling",
            Token::RoundFn => "round",

            _ => return Err(Error::XPathSyntax("expected function".into())),
        };
        let name = name.to_string();
        self.advance();

        self.expect(&Token::LeftParen)?;

        let mut args = Vec::new();
        if !matches!(self.current(), Token::RightParen) {
            args.push(self.parse_expr_value()?);
            while matches!(self.current(), Token::Comma) {
                self.advance();
                args.push(self.parse_expr_value()?);
            }
        }

        self.expect(&Token::RightParen)?;

        Ok(Expr::Function { name, args })
    }
}

/// Parses an XPath expression string into an AST.
pub fn parse_xpath(xpath: &str) -> Result<Expr> {
    let mut parser = Parser::new(xpath)?;
    parser.parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_path() {
        let expr = parse_xpath("/root/child").unwrap();
        if let Expr::Path(path) = expr {
            assert!(path.absolute);
            assert_eq!(path.steps.len(), 2);
            assert_eq!(path.steps[0].node_test, NodeTest::Name("root".into()));
            assert_eq!(path.steps[1].node_test, NodeTest::Name("child".into()));
        } else {
            panic!("expected Path");
        }
    }

    #[test]
    fn test_descendant() {
        let expr = parse_xpath("//element").unwrap();
        if let Expr::Path(path) = expr {
            assert!(path.absolute);
            // Should have descendant-or-self step + element step
            assert_eq!(path.steps.len(), 2);
            assert_eq!(path.steps[0].axis, Axis::DescendantOrSelf);
        } else {
            panic!("expected Path");
        }
    }

    #[test]
    fn test_predicate_name() {
        let expr = parse_xpath("//*[name()='Building']").unwrap();
        if let Expr::Path(path) = expr {
            assert_eq!(path.steps.len(), 2);
            let step = &path.steps[1];
            assert_eq!(step.predicates.len(), 1);
            if let Predicate::Comparison { op, .. } = &step.predicates[0] {
                assert_eq!(*op, ComparisonOp::Equal);
            } else {
                panic!("expected comparison predicate");
            }
        } else {
            panic!("expected Path");
        }
    }

    #[test]
    fn test_logical_or() {
        let expr = parse_xpath("//*[(name()='A' or name()='B')]").unwrap();
        if let Expr::Path(path) = expr {
            let step = &path.steps[1];
            assert!(!step.predicates.is_empty());
            assert!(matches!(&step.predicates[0], Predicate::Or(_, _)));
        } else {
            panic!("expected Path");
        }
    }

    #[test]
    fn test_not_predicate() {
        let expr = parse_xpath("//*[not(name()='Window')]").unwrap();
        if let Expr::Path(path) = expr {
            let step = &path.steps[1];
            assert!(!step.predicates.is_empty());
            assert!(matches!(&step.predicates[0], Predicate::Not(_)));
        } else {
            panic!("expected Path");
        }
    }

    #[test]
    fn test_namespaced_path() {
        let expr = parse_xpath("/gml:root/gml:child").unwrap();
        if let Expr::Path(path) = expr {
            assert_eq!(path.steps.len(), 2);
            assert_eq!(path.steps[0].node_test, NodeTest::QName {
                prefix: "gml".into(),
                local: "root".into(),
            });
        } else {
            panic!("expected Path");
        }
    }

    #[test]
    fn test_child_axis() {
        let expr = parse_xpath("./child::*").unwrap();
        if let Expr::Path(path) = expr {
            assert!(!path.absolute);
            assert_eq!(path.steps.len(), 2);
            assert_eq!(path.steps[1].axis, Axis::Child);
            assert_eq!(path.steps[1].node_test, NodeTest::Any);
        } else {
            panic!("expected Path");
        }
    }

    #[test]
    fn test_text_node() {
        let expr = parse_xpath("/root/text()").unwrap();
        if let Expr::Path(path) = expr {
            assert_eq!(path.steps.len(), 2);
            assert_eq!(path.steps[1].node_test, NodeTest::Text);
        } else {
            panic!("expected Path");
        }
    }
}
