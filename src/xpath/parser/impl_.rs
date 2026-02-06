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

    /// Parses the expression.
    pub fn parse(&mut self) -> Result<Expr> {
        self.parse_union_expr()
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
            // Function keywords can also be used as variable names
            Token::NameFn => "name".to_string(),
            Token::TextFn => "text".to_string(),
            Token::LocalNameFn => "local-name".to_string(),
            Token::NamespaceUriFn => "namespace-uri".to_string(),
            Token::ContainsFn => "contains".to_string(),
            Token::StartsWithFn => "starts-with".to_string(),
            Token::Not => "not".to_string(),
            Token::StringFn => "string".to_string(),
            Token::ConcatFn => "concat".to_string(),
            Token::SubstringFn => "substring".to_string(),
            Token::SubstringBeforeFn => "substring-before".to_string(),
            Token::SubstringAfterFn => "substring-after".to_string(),
            Token::StringLengthFn => "string-length".to_string(),
            Token::NormalizeSpaceFn => "normalize-space".to_string(),
            Token::TranslateFn => "translate".to_string(),
            Token::BooleanFn => "boolean".to_string(),
            Token::NumberFn => "number".to_string(),
            Token::SumFn => "sum".to_string(),
            Token::FloorFn => "floor".to_string(),
            Token::CeilingFn => "ceiling".to_string(),
            Token::RoundFn => "round".to_string(),
            Token::CountFn => "count".to_string(),
            Token::LastFn => "last".to_string(),
            Token::PositionFn => "position".to_string(),
            Token::TrueFn => "true".to_string(),
            Token::FalseFn => "false".to_string(),
            _ => {
                return Err(XPathSyntaxError::UnexpectedToken {
                    found: Some(self.current().clone()),
                    expected: "variable name after $".to_string(),
                }
                .into());
            }
        };
        self.advance();
        Ok(name)
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

        // Parse steps
        if !matches!(
            self.current(),
            Token::Eof | Token::Pipe | Token::RightBracket | Token::RightParen
        ) {
            steps.push(self.parse_step()?);

            while matches!(self.current(), Token::Slash | Token::DoubleSlash) {
                if matches!(self.current(), Token::DoubleSlash) {
                    self.advance();
                    steps.push(Step::descendant_or_self_any());
                } else {
                    self.advance();
                }

                if !matches!(
                    self.current(),
                    Token::Eof | Token::Pipe | Token::RightBracket | Token::RightParen
                ) {
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
            // Function tokens can also be used as names in node test context
            // e.g., @id, @name, @count, etc. should be treated as attribute names
            Token::IdFn => {
                self.advance();
                Ok(NodeTest::Name("id".to_string()))
            }
            Token::NameFn => {
                self.advance();
                Ok(NodeTest::Name("name".to_string()))
            }
            Token::CountFn => {
                self.advance();
                Ok(NodeTest::Name("count".to_string()))
            }
            Token::LastFn => {
                self.advance();
                Ok(NodeTest::Name("last".to_string()))
            }
            Token::PositionFn => {
                self.advance();
                Ok(NodeTest::Name("position".to_string()))
            }
            Token::StringFn => {
                self.advance();
                Ok(NodeTest::Name("string".to_string()))
            }
            Token::NumberFn => {
                self.advance();
                Ok(NodeTest::Name("number".to_string()))
            }
            Token::BooleanFn => {
                self.advance();
                Ok(NodeTest::Name("boolean".to_string()))
            }
            Token::SumFn => {
                self.advance();
                Ok(NodeTest::Name("sum".to_string()))
            }
            Token::TrueFn => {
                self.advance();
                Ok(NodeTest::Name("true".to_string()))
            }
            Token::FalseFn => {
                self.advance();
                Ok(NodeTest::Name("false".to_string()))
            }
            Token::FloorFn => {
                self.advance();
                Ok(NodeTest::Name("floor".to_string()))
            }
            Token::CeilingFn => {
                self.advance();
                Ok(NodeTest::Name("ceiling".to_string()))
            }
            Token::RoundFn => {
                self.advance();
                Ok(NodeTest::Name("round".to_string()))
            }
            Token::Not => {
                self.advance();
                Ok(NodeTest::Name("not".to_string()))
            }
            Token::LangFn => {
                self.advance();
                Ok(NodeTest::Name("lang".to_string()))
            }
            Token::ContainsFn => {
                self.advance();
                Ok(NodeTest::Name("contains".to_string()))
            }
            Token::StartsWithFn => {
                self.advance();
                Ok(NodeTest::Name("starts-with".to_string()))
            }
            Token::ConcatFn => {
                self.advance();
                Ok(NodeTest::Name("concat".to_string()))
            }
            Token::SubstringFn => {
                self.advance();
                Ok(NodeTest::Name("substring".to_string()))
            }
            Token::SubstringBeforeFn => {
                self.advance();
                Ok(NodeTest::Name("substring-before".to_string()))
            }
            Token::SubstringAfterFn => {
                self.advance();
                Ok(NodeTest::Name("substring-after".to_string()))
            }
            Token::StringLengthFn => {
                self.advance();
                Ok(NodeTest::Name("string-length".to_string()))
            }
            Token::NormalizeSpaceFn => {
                self.advance();
                Ok(NodeTest::Name("normalize-space".to_string()))
            }
            Token::TranslateFn => {
                self.advance();
                Ok(NodeTest::Name("translate".to_string()))
            }
            Token::LocalNameFn => {
                self.advance();
                Ok(NodeTest::Name("local-name".to_string()))
            }
            Token::NamespaceUriFn => {
                self.advance();
                Ok(NodeTest::Name("namespace-uri".to_string()))
            }
            _ => Err(XPathSyntaxError::UnexpectedToken {
                found: Some(self.current().clone()),
                expected: "node test".to_string(),
            }
            .into()),
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
            // Could be position predicate or boolean expression
            if let Expr::Number(n) = &left {
                Ok(Predicate::Position(*n as usize))
            } else {
                Ok(Predicate::Expr(Box::new(left)))
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
            Token::Dollar => {
                self.advance();
                // Accept Name tokens and function name keywords as variable names
                let var_name = self.extract_variable_name()?;
                Ok(Expr::Variable(var_name))
            }
            Token::Name(name) => {
                // Check if this is a function call (name followed by '(')
                if self.peek() == Some(&Token::LeftParen) {
                    // This is an unknown function call
                    let fn_name = name.clone();
                    self.advance(); // consume name
                    self.advance(); // consume '('

                    let mut args = Vec::new();
                    if !matches!(self.current(), Token::RightParen) {
                        args.push(self.parse_expr_value()?);
                        while matches!(self.current(), Token::Comma) {
                            self.advance();
                            args.push(self.parse_expr_value()?);
                        }
                    }

                    self.expect(&Token::RightParen)?;

                    Ok(Expr::Function { name: fn_name, args })
                } else {
                    let path = self.parse_path_expr()?;
                    Ok(Expr::Path(path))
                }
            }
            Token::Slash | Token::DoubleSlash | Token::Dot | Token::At | Token::Asterisk |
            // All axis tokens for paths like ancestor::*, preceding-sibling::node(), etc.
            Token::ChildAxis | Token::DescendantAxis | Token::ParentAxis | Token::SelfAxis |
            Token::DescendantOrSelfAxis | Token::AncestorAxis | Token::AncestorOrSelfAxis |
            Token::FollowingSiblingAxis | Token::PrecedingSiblingAxis |
            Token::FollowingAxis | Token::PrecedingAxis |
            Token::AttributeAxis | Token::NamespaceAxis => {
                let path = self.parse_path_expr()?;
                Ok(Expr::Path(path))
            }
            _ => Err(XPathSyntaxError::UnexpectedToken {
                found: Some(self.current().clone()),
                expected: "expression value".to_string(),
            }.into()),
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
            // Function calls
            Token::NameFn
            | Token::TextFn
            | Token::LocalNameFn
            | Token::NamespaceUriFn
            | Token::ContainsFn
            | Token::StartsWithFn
            | Token::Not
            | Token::StringFn
            | Token::ConcatFn
            | Token::SubstringFn
            | Token::SubstringBeforeFn
            | Token::SubstringAfterFn
            | Token::StringLengthFn
            | Token::NormalizeSpaceFn
            | Token::TranslateFn
            | Token::PositionFn
            | Token::LastFn
            | Token::CountFn
            | Token::IdFn
            | Token::TrueFn
            | Token::FalseFn
            | Token::BooleanFn
            | Token::LangFn
            | Token::NumberFn
            | Token::SumFn
            | Token::FloorFn
            | Token::CeilingFn
            | Token::RoundFn => self.parse_function_call(),
            // Path expressions or unknown function calls
            Token::Name(name) => {
                // Check if this is a function call (name followed by '(')
                if self.peek() == Some(&Token::LeftParen) {
                    // This is an unknown function call
                    let fn_name = name.clone();
                    self.advance(); // consume name
                    self.advance(); // consume '('

                    let mut args = Vec::new();
                    if !matches!(self.current(), Token::RightParen) {
                        args.push(self.parse_expr_value()?);
                        while matches!(self.current(), Token::Comma) {
                            self.advance();
                            args.push(self.parse_expr_value()?);
                        }
                    }

                    self.expect(&Token::RightParen)?;

                    Ok(Expr::Function {
                        name: fn_name,
                        args,
                    })
                } else {
                    let path = self.parse_path_expr()?;
                    Ok(Expr::Path(path))
                }
            }
            Token::Slash | Token::DoubleSlash | Token::Dot | Token::At | Token::Asterisk |
            // All axis tokens for paths like ancestor::*, preceding-sibling::node(), etc.
            Token::ChildAxis | Token::DescendantAxis | Token::ParentAxis | Token::SelfAxis |
            Token::DescendantOrSelfAxis | Token::AncestorAxis | Token::AncestorOrSelfAxis |
            Token::FollowingSiblingAxis | Token::PrecedingSiblingAxis |
            Token::FollowingAxis | Token::PrecedingAxis |
            Token::AttributeAxis | Token::NamespaceAxis => {
                let path = self.parse_path_expr()?;
                Ok(Expr::Path(path))
            }
            _ => Err(XPathSyntaxError::UnexpectedToken {
                found: Some(self.current().clone()),
                expected: "primary expression".to_string(),
            }
            .into()),
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

            _ => {
                return Err(XPathSyntaxError::UnexpectedToken {
                    found: Some(self.current().clone()),
                    expected: "function".to_string(),
                }
                .into());
            }
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
