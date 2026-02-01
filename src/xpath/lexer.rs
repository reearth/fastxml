//! XPath expression tokenizer.

use crate::error::{Error, Result};

/// XPath token types.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// Forward slash `/`
    Slash,
    /// Double slash `//`
    DoubleSlash,
    /// Dot `.`
    Dot,
    /// Double dot `..`
    DoubleDot,
    /// At sign `@`
    At,
    /// Asterisk `*`
    Asterisk,
    /// Left parenthesis `(`
    LeftParen,
    /// Right parenthesis `)`
    RightParen,
    /// Left bracket `[`
    LeftBracket,
    /// Right bracket `]`
    RightBracket,
    /// Comma `,`
    Comma,
    /// Pipe `|`
    Pipe,
    /// Plus `+`
    Plus,
    /// Minus `-`
    Minus,
    /// Equals `=`
    Equals,
    /// Not equals `!=`
    NotEquals,
    /// Less than `<`
    LessThan,
    /// Less than or equal `<=`
    LessOrEqual,
    /// Greater than `>`
    GreaterThan,
    /// Greater than or equal `>=`
    GreaterOrEqual,
    /// `and` keyword
    And,
    /// `or` keyword
    Or,
    /// `not` keyword (function)
    Not,
    /// `name` function
    NameFn,
    /// `text` function
    TextFn,
    /// `local-name` function
    LocalNameFn,
    /// `namespace-uri` function
    NamespaceUriFn,
    /// `contains` function
    ContainsFn,
    /// `starts-with` function
    StartsWithFn,
    /// `child` axis
    ChildAxis,
    /// `descendant` axis
    DescendantAxis,
    /// `parent` axis
    ParentAxis,
    /// `self` axis
    SelfAxis,
    /// `descendant-or-self` axis
    DescendantOrSelfAxis,
    /// `ancestor` axis
    AncestorAxis,
    /// `following-sibling` axis
    FollowingSiblingAxis,
    /// `preceding-sibling` axis
    PrecedingSiblingAxis,
    /// Double colon `::`
    DoubleColon,
    /// Name (element name, possibly with prefix)
    Name(String),
    /// String literal
    String(String),
    /// Number literal
    Number(f64),
    /// End of input
    Eof,
}

/// XPath tokenizer.
pub struct Lexer<'a> {
    input: &'a str,
    pos: usize,
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
}

impl<'a> Lexer<'a> {
    /// Creates a new lexer for the given XPath expression.
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            chars: input.char_indices().peekable(),
        }
    }

    /// Tokenizes the entire expression.
    pub fn tokenize(&mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();

        loop {
            let token = self.next_token()?;
            if token == Token::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }

        Ok(tokens)
    }

    /// Returns the next token.
    pub fn next_token(&mut self) -> Result<Token> {
        self.skip_whitespace();

        let Some(&(pos, ch)) = self.chars.peek() else {
            return Ok(Token::Eof);
        };

        self.pos = pos;

        match ch {
            '/' => {
                self.advance();
                if self.peek_char() == Some('/') {
                    self.advance();
                    Ok(Token::DoubleSlash)
                } else {
                    Ok(Token::Slash)
                }
            }
            '.' => {
                self.advance();
                if self.peek_char() == Some('.') {
                    self.advance();
                    Ok(Token::DoubleDot)
                } else {
                    Ok(Token::Dot)
                }
            }
            '@' => {
                self.advance();
                Ok(Token::At)
            }
            '*' => {
                self.advance();
                Ok(Token::Asterisk)
            }
            '(' => {
                self.advance();
                Ok(Token::LeftParen)
            }
            ')' => {
                self.advance();
                Ok(Token::RightParen)
            }
            '[' => {
                self.advance();
                Ok(Token::LeftBracket)
            }
            ']' => {
                self.advance();
                Ok(Token::RightBracket)
            }
            ',' => {
                self.advance();
                Ok(Token::Comma)
            }
            '|' => {
                self.advance();
                Ok(Token::Pipe)
            }
            '+' => {
                self.advance();
                Ok(Token::Plus)
            }
            '-' => {
                self.advance();
                Ok(Token::Minus)
            }
            '=' => {
                self.advance();
                Ok(Token::Equals)
            }
            '!' => {
                self.advance();
                if self.peek_char() == Some('=') {
                    self.advance();
                    Ok(Token::NotEquals)
                } else {
                    Err(Error::XPathSyntax(format!("unexpected character '!' at position {}", pos)))
                }
            }
            '<' => {
                self.advance();
                if self.peek_char() == Some('=') {
                    self.advance();
                    Ok(Token::LessOrEqual)
                } else {
                    Ok(Token::LessThan)
                }
            }
            '>' => {
                self.advance();
                if self.peek_char() == Some('=') {
                    self.advance();
                    Ok(Token::GreaterOrEqual)
                } else {
                    Ok(Token::GreaterThan)
                }
            }
            ':' => {
                self.advance();
                if self.peek_char() == Some(':') {
                    self.advance();
                    Ok(Token::DoubleColon)
                } else {
                    Err(Error::XPathSyntax(format!("unexpected ':' at position {}", pos)))
                }
            }
            '\'' | '"' => {
                self.read_string()
            }
            c if c.is_ascii_digit() => {
                self.read_number()
            }
            c if is_name_start_char(c) => {
                self.read_name_or_keyword()
            }
            _ => {
                Err(Error::XPathSyntax(format!("unexpected character '{}' at position {}", ch, pos)))
            }
        }
    }

    fn advance(&mut self) -> Option<char> {
        self.chars.next().map(|(_, c)| c)
    }

    fn peek_char(&mut self) -> Option<char> {
        self.chars.peek().map(|(_, c)| *c)
    }

    fn skip_whitespace(&mut self) {
        while let Some(&(_, ch)) = self.chars.peek() {
            if ch.is_whitespace() {
                self.chars.next();
            } else {
                break;
            }
        }
    }

    fn read_string(&mut self) -> Result<Token> {
        let quote = self.advance().unwrap(); // ' or "
        let start = self.chars.peek().map(|(i, _)| *i).unwrap_or(self.input.len());
        let mut end = start;

        while let Some(&(pos, ch)) = self.chars.peek() {
            if ch == quote {
                end = pos;
                self.advance();
                break;
            }
            self.advance();
        }

        let s = &self.input[start..end];
        Ok(Token::String(s.to_string()))
    }

    fn read_number(&mut self) -> Result<Token> {
        let start = self.chars.peek().map(|(i, _)| *i).unwrap_or(self.input.len());
        let mut end = start;
        let mut has_dot = false;

        while let Some(&(pos, ch)) = self.chars.peek() {
            if ch.is_ascii_digit() {
                end = pos + 1;
                self.advance();
            } else if ch == '.' && !has_dot {
                // Check if next char is a digit (to distinguish from .. and ./path)
                let next_is_digit = {
                    let mut chars = self.input[pos + 1..].chars();
                    chars.next().is_some_and(|c| c.is_ascii_digit())
                };
                if next_is_digit {
                    has_dot = true;
                    end = pos + 1;
                    self.advance();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let s = &self.input[start..end];
        let num: f64 = s.parse().map_err(|_| {
            Error::XPathSyntax(format!("invalid number '{}'", s))
        })?;

        Ok(Token::Number(num))
    }

    fn read_name_or_keyword(&mut self) -> Result<Token> {
        let start = self.chars.peek().map(|(i, _)| *i).unwrap_or(self.input.len());
        let mut end = start;

        while let Some(&(pos, ch)) = self.chars.peek() {
            if is_name_char_no_colon(ch) {
                end = pos + ch.len_utf8();
                self.advance();
            } else if ch == ':' {
                // Check if this is "::" (axis separator) or just ":" (namespace separator)
                let remaining = &self.input[pos..];
                if remaining.starts_with("::") {
                    // This is "::", stop here - the name ends before the ::
                    break;
                } else {
                    // Single ":" - part of QName (e.g., gml:root)
                    end = pos + 1;
                    self.advance();
                }
            } else {
                break;
            }
        }

        let name = &self.input[start..end];

        // Check if next chars are "::" to determine if this is an axis
        let is_axis = {
            let remaining = &self.input[end..];
            remaining.starts_with("::")
        };

        // Check for keywords and functions
        let token = match name {
            "and" => Token::And,
            "or" => Token::Or,
            "not" => Token::Not,
            "name" => Token::NameFn,
            "text" => Token::TextFn,
            "local-name" => Token::LocalNameFn,
            "namespace-uri" => Token::NamespaceUriFn,
            "contains" => Token::ContainsFn,
            "starts-with" => Token::StartsWithFn,
            // Only return axis tokens if followed by ::
            "child" if is_axis => Token::ChildAxis,
            "descendant" if is_axis => Token::DescendantAxis,
            "parent" if is_axis => Token::ParentAxis,
            "self" if is_axis => Token::SelfAxis,
            "descendant-or-self" if is_axis => Token::DescendantOrSelfAxis,
            "ancestor" if is_axis => Token::AncestorAxis,
            "following-sibling" if is_axis => Token::FollowingSiblingAxis,
            "preceding-sibling" if is_axis => Token::PrecedingSiblingAxis,
            _ => Token::Name(name.to_string()),
        };

        Ok(token)
    }
}

fn is_name_start_char(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_name_char_no_colon(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '.'
}

#[allow(dead_code)]
fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == ':'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_path() {
        let mut lexer = Lexer::new("/root/child");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens, vec![
            Token::Slash,
            Token::Name("root".into()),
            Token::Slash,
            Token::Name("child".into()),
            Token::Eof,
        ]);
    }

    #[test]
    fn test_double_slash() {
        let mut lexer = Lexer::new("//element");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens, vec![
            Token::DoubleSlash,
            Token::Name("element".into()),
            Token::Eof,
        ]);
    }

    #[test]
    fn test_predicate() {
        let mut lexer = Lexer::new("//*[name()='Building']");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens, vec![
            Token::DoubleSlash,
            Token::Asterisk,
            Token::LeftBracket,
            Token::NameFn,
            Token::LeftParen,
            Token::RightParen,
            Token::Equals,
            Token::String("Building".into()),
            Token::RightBracket,
            Token::Eof,
        ]);
    }

    #[test]
    fn test_namespaced() {
        let mut lexer = Lexer::new("/gml:root/gml:child");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens, vec![
            Token::Slash,
            Token::Name("gml:root".into()),
            Token::Slash,
            Token::Name("gml:child".into()),
            Token::Eof,
        ]);
    }

    #[test]
    fn test_logical_operators() {
        let mut lexer = Lexer::new("[name()='A' or name()='B']");
        let tokens = lexer.tokenize().unwrap();
        assert!(tokens.contains(&Token::Or));
    }

    #[test]
    fn test_axis() {
        let mut lexer = Lexer::new("./child::*");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens, vec![
            Token::Dot,
            Token::Slash,
            Token::ChildAxis,
            Token::DoubleColon,
            Token::Asterisk,
            Token::Eof,
        ]);
    }
}
