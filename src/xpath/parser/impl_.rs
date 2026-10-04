//! XPath expression parser implementation.

use crate::error::Result;
use crate::xpath::error::XPathSyntaxError;
use crate::xpath::lexer::{Lexer, Token};

use super::ast::{Axis, ComparisonOp, Expr, NodeTest, PathExpr, Predicate, Step};

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

    /// Parses the expression. The whole input must be consumed: trailing
    /// tokens are a syntax error rather than being silently ignored.
    pub fn parse(&mut self) -> Result<Expr> {
        let expr = self.parse_union_expr()?;
        if !matches!(self.current(), Token::Eof) {
            return Err(XPathSyntaxError::UnexpectedToken {
                found: Some(self.current().clone()),
                expected: "end of expression".to_string(),
            }
            .into());
        }
        Ok(expr)
    }

    fn current(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos + 1)
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
            Err(XPathSyntaxError::UnexpectedToken {
                found: Some(self.current().clone()),
                expected: format!("{:?}", expected),
            }
            .into())
        }
    }

    /// Extracts a variable name from the current token.
    /// Accepts both Name tokens and function keyword tokens (e.g., name, text, position).
    fn extract_variable_name(&mut self) -> Result<String> {
        let name = match self.current() {
            Token::Name(n) => n.clone(),
            // Keywords can also be used as variable names
            token => match keyword_name(token) {
                Some(name) => name.to_string(),
                None => {
                    return Err(XPathSyntaxError::UnexpectedToken {
                        found: Some(self.current().clone()),
                        expected: "variable name after $".to_string(),
                    }
                    .into());
                }
            },
        };
        self.advance();
        Ok(name)
    }

    /// Whether the current token can start a location step.
    ///
    /// Per XPath 1.0 §3.7, keyword tokens (`div`, `and`, `count`, `text`, ...)
    /// that are not in operator position or followed by `(` are plain names.
    fn at_step_start(&self) -> bool {
        match self.current() {
            Token::Dot
            | Token::DoubleDot
            | Token::At
            | Token::Asterisk
            | Token::Name(_)
            | Token::ChildAxis
            | Token::DescendantAxis
            | Token::ParentAxis
            | Token::SelfAxis
            | Token::DescendantOrSelfAxis
            | Token::AncestorAxis
            | Token::AncestorOrSelfAxis
            | Token::FollowingSiblingAxis
            | Token::PrecedingSiblingAxis
            | Token::FollowingAxis
            | Token::PrecedingAxis
            | Token::AttributeAxis
            | Token::NamespaceAxis => true,
            token => keyword_name(token).is_some(),
        }
    }

    fn parse_union_expr(&mut self) -> Result<Expr> {
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

        // Parse steps. A bare `/` has none; a leading `//` needs one.
        let leading_double_slash = !steps.is_empty();
        if leading_double_slash || self.at_step_start() {
            steps.push(self.parse_step()?);

            while matches!(self.current(), Token::Slash | Token::DoubleSlash) {
                if matches!(self.current(), Token::DoubleSlash) {
                    self.advance();
                    steps.push(Step::descendant_or_self_any());
                } else {
                    self.advance();
                }

                // A step is mandatory after an inner `/` or `//`.
                steps.push(self.parse_step()?);
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
            // `text()` / `node()` are node type tests only when followed by
            // `(`; otherwise `text` / `node` are element names (§3.7).
            Token::TextFn if self.peek() == Some(&Token::LeftParen) => {
                self.advance();
                self.expect(&Token::LeftParen)?;
                self.expect(&Token::RightParen)?;
                Ok(NodeTest::Text)
            }
            Token::NodeFn if self.peek() == Some(&Token::LeftParen) => {
                self.advance();
                self.expect(&Token::LeftParen)?;
                self.expect(&Token::RightParen)?;
                Ok(NodeTest::Node)
            }
            // Operator and function keywords are plain names in a node test
            // (e.g. `//div`, `@id`, `@name`, `self::and`).
            token => match keyword_name(token) {
                Some(name) => {
                    let name = name.to_string();
                    self.advance();
                    Ok(NodeTest::Name(name))
                }
                None => Err(XPathSyntaxError::UnexpectedToken {
                    found: Some(self.current().clone()),
                    expected: "node test".to_string(),
                }
                .into()),
            },
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
        // Handle not() (without `(`, `not` is an element name)
        if matches!(self.current(), Token::Not) && self.peek() == Some(&Token::LeftParen) {
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
        let left = self.parse_predicate_additive_expr()?;

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
            let right = self.parse_predicate_additive_expr()?;
            Ok(Predicate::Comparison {
                left: Box::new(left),
                op,
                right: Box::new(right),
            })
        } else {
            // `[n]` abbreviates `[position() = n]`. Only a positive integer
            // can equal a position; any other number (`[0]`, `[1.5]`) stays an
            // expression, which selects nothing instead of being truncated.
            match &left {
                Expr::Number(n) if *n >= 1.0 && n.fract() == 0.0 && *n <= u32::MAX as f64 => {
                    Ok(Predicate::Position(*n as usize))
                }
                _ => Ok(Predicate::Expr(Box::new(left))),
            }
        }
    }

    /// Parses an additive expression inside predicates: expr ('+' | '-') expr
    fn parse_predicate_additive_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_predicate_multiplicative_expr()?;

        loop {
            match self.current() {
                Token::Plus => {
                    self.advance();
                    let right = self.parse_predicate_multiplicative_expr()?;
                    left = Expr::Add(Box::new(left), Box::new(right));
                }
                Token::Minus => {
                    self.advance();
                    let right = self.parse_predicate_multiplicative_expr()?;
                    left = Expr::Subtract(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }

        Ok(left)
    }

    /// Parses a multiplicative expression inside predicates: expr ('*' | 'div' | 'mod') expr
    fn parse_predicate_multiplicative_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_expr_value()?;

        loop {
            match self.current() {
                Token::Asterisk => {
                    self.advance();
                    let right = self.parse_expr_value()?;
                    left = Expr::Multiply(Box::new(left), Box::new(right));
                }
                Token::Div => {
                    self.advance();
                    let right = self.parse_expr_value()?;
                    left = Expr::Divide(Box::new(left), Box::new(right));
                }
                Token::Mod => {
                    self.advance();
                    let right = self.parse_expr_value()?;
                    left = Expr::Modulo(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }

        Ok(left)
    }

    /// Parses an operand inside the predicate sub-grammar.
    fn parse_expr_value(&mut self) -> Result<Expr> {
        self.parse_unary_expr()
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
                // After a complete operand, `*` is the multiply operator
                // (§3.7); as a name test it was already consumed by the path.
                Token::Asterisk => {
                    self.advance();
                    let right = self.parse_unary_expr()?;
                    left = Expr::Multiply(Box::new(left), Box::new(right));
                }
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

    /// Parses a unary expression: '-' expr | union
    fn parse_unary_expr(&mut self) -> Result<Expr> {
        if matches!(self.current(), Token::Minus) {
            self.advance();
            let inner = self.parse_unary_expr()?;
            Ok(Expr::Negate(Box::new(inner)))
        } else {
            self.parse_path_union_expr()
        }
    }

    /// Parses a union expression: path ('|' path)*
    fn parse_path_union_expr(&mut self) -> Result<Expr> {
        let first = self.parse_primary_expr()?;

        // Check if there's a union operator
        if !matches!(self.current(), Token::Pipe) {
            return Ok(first);
        }

        // Extract the path from first expression
        let first_path = match first {
            Expr::Path(p) => p,
            _ => {
                // Union operator requires path expressions
                return Err(XPathSyntaxError::UnexpectedToken {
                    found: Some(self.current().clone()),
                    expected: "path expression for union".to_string(),
                }
                .into());
            }
        };

        let mut paths = vec![first_path];

        while matches!(self.current(), Token::Pipe) {
            self.advance();
            let next = self.parse_primary_expr()?;
            match next {
                Expr::Path(p) => paths.push(p),
                _ => {
                    return Err(XPathSyntaxError::UnexpectedToken {
                        found: Some(self.current().clone()),
                        expected: "path expression for union".to_string(),
                    }
                    .into());
                }
            }
        }

        Ok(Expr::Union(paths))
    }

    /// Parses a primary expression (path, literal, function, or parenthesized)
    fn parse_primary_expr(&mut self) -> Result<Expr> {
        let followed_by_paren = self.peek() == Some(&Token::LeftParen);
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
            Token::Dollar => {
                self.advance();
                // Accept Name tokens and function name keywords as variable names
                let var_name = self.extract_variable_name()?;
                Ok(Expr::Variable(var_name))
            }
            Token::LeftParen => {
                self.advance();
                let inner = self.parse_additive_expr()?;
                self.expect(&Token::RightParen)?;
                Ok(inner)
            }
            // Unknown function call (name followed by '(')
            Token::Name(name) if followed_by_paren => {
                let name = name.clone();
                self.advance(); // consume name
                self.parse_function_args(name)
            }
            // Built-in function call. `text(` / `node(` are node type tests,
            // which start a location path instead.
            token if followed_by_paren && function_name(token).is_some() => {
                self.parse_function_call()
            }
            Token::Slash | Token::DoubleSlash => Ok(Expr::Path(self.parse_path_expr()?)),
            _ if self.at_step_start() => Ok(Expr::Path(self.parse_path_expr()?)),
            _ => Err(XPathSyntaxError::UnexpectedToken {
                found: Some(self.current().clone()),
                expected: "primary expression".to_string(),
            }
            .into()),
        }
    }

    fn parse_function_call(&mut self) -> Result<Expr> {
        let Some(name) = function_name(self.current()) else {
            return Err(XPathSyntaxError::UnexpectedToken {
                found: Some(self.current().clone()),
                expected: "function".to_string(),
            }
            .into());
        };
        self.advance();
        self.parse_function_args(name.to_string())
    }

    /// Parses `( [Expr (',' Expr)*] )` after a function name.
    fn parse_function_args(&mut self, name: String) -> Result<Expr> {
        self.expect(&Token::LeftParen)?;

        let mut args = Vec::new();
        if !matches!(self.current(), Token::RightParen) {
            args.push(self.parse_union_expr()?);
            while matches!(self.current(), Token::Comma) {
                self.advance();
                args.push(self.parse_union_expr()?);
            }
        }

        self.expect(&Token::RightParen)?;

        Ok(Expr::Function { name, args })
    }
}

/// The function a keyword token names in a function call. Operator names and
/// the node types `text` / `node` are not functions: `text()` is a node test.
fn function_name(token: &Token) -> Option<&'static str> {
    match token {
        Token::And | Token::Or | Token::Div | Token::Mod | Token::TextFn | Token::NodeFn => None,
        token => keyword_name(token),
    }
}

/// The source spelling of a keyword token. Outside operator / function-call
/// position every keyword is an ordinary name (XPath 1.0 §3.7).
fn keyword_name(token: &Token) -> Option<&'static str> {
    Some(match token {
        // Operator names
        Token::And => "and",
        Token::Or => "or",
        Token::Div => "div",
        Token::Mod => "mod",

        // Node set functions
        Token::NameFn => "name",
        Token::LocalNameFn => "local-name",
        Token::NamespaceUriFn => "namespace-uri",
        Token::PositionFn => "position",
        Token::LastFn => "last",
        Token::CountFn => "count",
        Token::IdFn => "id",

        // String functions
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

        // Node types
        Token::TextFn => "text",
        Token::NodeFn => "node",

        _ => return None,
    })
}

/// Parses an XPath expression string into an AST.
pub fn parse_xpath(xpath: &str) -> Result<Expr> {
    let mut parser = Parser::new(xpath)?;
    parser.parse()
}
