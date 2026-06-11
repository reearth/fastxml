//! Syntax checker for XSD regular expressions (XSD 1.0 Part 2, Appendix F).
//!
//! Used at schema compile time: a pattern facet whose value is not a valid
//! XSD regular expression makes the schema invalid. This is independent of
//! the runtime translation to Rust regex syntax — a pattern can be valid XSD
//! that the Rust engine cannot express (it is then skipped at validation
//! time), but invalid XSD must reject the schema.

/// Checks that `pattern` is a syntactically valid XSD regular expression.
pub(crate) fn check_xsd_regex(pattern: &str) -> Result<(), String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut p = Checker { chars, pos: 0 };
    p.reg_exp()?;
    if p.pos != p.chars.len() {
        return Err(format!(
            "unexpected '{}' at position {}",
            p.chars[p.pos], p.pos
        ));
    }
    Ok(())
}

struct Checker {
    chars: Vec<char>,
    pos: usize,
}

impl Checker {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// regExp ::= branch ( '|' branch )*
    fn reg_exp(&mut self) -> Result<(), String> {
        self.branch()?;
        while self.eat('|') {
            self.branch()?;
        }
        Ok(())
    }

    /// branch ::= piece*
    fn branch(&mut self) -> Result<(), String> {
        loop {
            match self.peek() {
                None | Some('|') | Some(')') => return Ok(()),
                _ => self.piece()?,
            }
        }
    }

    /// piece ::= atom quantifier?
    fn piece(&mut self) -> Result<(), String> {
        self.atom()?;
        self.quantifier()
    }

    fn quantifier(&mut self) -> Result<(), String> {
        match self.peek() {
            Some('?') | Some('*') | Some('+') => {
                self.pos += 1;
                Ok(())
            }
            Some('{') => {
                self.pos += 1;
                let min = self.digits()?;
                let max = if self.eat(',') {
                    if self.peek() == Some('}') {
                        None // {n,} unbounded
                    } else {
                        Some(self.digits()?)
                    }
                } else {
                    Some(min)
                };
                if !self.eat('}') {
                    return Err("unterminated quantifier (missing '}')".to_string());
                }
                if let Some(max) = max
                    && min > max
                {
                    return Err(format!(
                        "quantifier minimum {} exceeds maximum {}",
                        min, max
                    ));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn digits(&mut self) -> Result<u64, String> {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == start {
            return Err("expected digits in quantifier".to_string());
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        // Saturate huge counts; the value itself is syntactically fine.
        Ok(text.parse::<u64>().unwrap_or(u64::MAX))
    }

    /// atom ::= Char | charClass | ( '(' regExp ')' )
    fn atom(&mut self) -> Result<(), String> {
        match self.peek() {
            Some('(') => {
                self.pos += 1;
                self.reg_exp()?;
                if !self.eat(')') {
                    return Err("unbalanced '(' (missing ')')".to_string());
                }
                Ok(())
            }
            Some('[') => {
                self.pos += 1;
                self.char_class_expr()
            }
            Some('\\') => {
                self.pos += 1;
                self.escape(true)
            }
            Some('.') => {
                self.pos += 1;
                Ok(())
            }
            // Metacharacters that cannot start an atom
            Some(c @ ('?' | '*' | '+' | '{' | '}' | ']')) => {
                Err(format!("unescaped metacharacter '{}'", c))
            }
            Some(_) => {
                self.pos += 1;
                Ok(())
            }
            None => Err("expected an atom".to_string()),
        }
    }

    /// An escape after `\`. With `allow_multi`, multi-character and category
    /// escapes are allowed (atoms and class members); range endpoints allow
    /// only single-character escapes.
    fn escape(&mut self, allow_multi: bool) -> Result<(), String> {
        let Some(c) = self.bump() else {
            return Err("trailing '\\'".to_string());
        };
        match c {
            // SingleCharEsc
            'n' | 'r' | 't' | '\\' | '|' | '.' | '?' | '*' | '+' | '(' | ')' | '{' | '}' | '-'
            | '[' | ']' | '^' => Ok(()),
            // MultiCharEsc
            's' | 'S' | 'i' | 'I' | 'c' | 'C' | 'd' | 'D' | 'w' | 'W' if allow_multi => Ok(()),
            // catEsc / complEsc
            'p' | 'P' if allow_multi => {
                if !self.eat('{') {
                    return Err(format!("'\\{}' must be followed by '{{name}}'", c));
                }
                let start = self.pos;
                while matches!(self.peek(), Some(ch) if ch != '}') {
                    self.pos += 1;
                }
                if !self.eat('}') {
                    return Err("unterminated character property (missing '}')".to_string());
                }
                let name: String = self.chars[start..self.pos - 1].iter().collect();
                check_char_property(&name)
            }
            other => Err(format!("invalid escape '\\{}'", other)),
        }
    }

    /// charClassExpr ::= '[' charGroup ']' (the '[' is already consumed)
    fn char_class_expr(&mut self) -> Result<(), String> {
        self.eat('^'); // negCharGroup

        let mut members = 0usize;

        // A leading '-' is a literal member.
        if self.eat('-') {
            members += 1;
        }
        loop {
            match self.peek() {
                None => return Err("unbalanced '[' (missing ']')".to_string()),
                Some(']') => {
                    self.pos += 1;
                    if members == 0 {
                        return Err("empty character class".to_string());
                    }
                    return Ok(());
                }
                Some('-') => {
                    self.pos += 1;
                    match self.peek() {
                        // Class subtraction: '-' '[' charClassExpr ']' ends the class
                        Some('[') => {
                            self.pos += 1;
                            self.char_class_expr()?;
                            if !self.eat(']') {
                                return Err(
                                    "class subtraction must end the character class".to_string()
                                );
                            }
                            if members == 0 {
                                return Err("empty character class".to_string());
                            }
                            return Ok(());
                        }
                        // Otherwise '-' is a literal member (XmlCharIncDash)
                        _ => {
                            members += 1;
                        }
                    }
                }
                _ => {
                    self.class_member()?;
                    members += 1;
                }
            }
        }
    }

    /// One positive character group member: a char or escape, optionally the
    /// start of a range.
    fn class_member(&mut self) -> Result<(), String> {
        // First endpoint
        let single = match self.peek() {
            Some('\\') => {
                self.pos += 1;
                let escape_char = self.peek();
                self.escape(true)?;
                // Multi-char / category escapes cannot be range endpoints
                !matches!(
                    escape_char,
                    Some('s' | 'S' | 'i' | 'I' | 'c' | 'C' | 'd' | 'D' | 'w' | 'W' | 'p' | 'P')
                )
            }
            Some('[') => return Err("unescaped '[' in character class".to_string()),
            Some(_) => {
                self.pos += 1;
                true
            }
            None => return Err("unbalanced '[' (missing ']')".to_string()),
        };

        // Range?
        if self.peek() == Some('-')
            && !matches!(self.chars.get(self.pos + 1), Some(']') | Some('[') | None)
        {
            self.pos += 1; // consume '-'
            if !single {
                return Err("character range start must be a single character".to_string());
            }
            // Second endpoint must be a single char or single-char escape
            match self.peek() {
                Some('\\') => {
                    self.pos += 1;
                    self.escape(false).map_err(|_| {
                        "character range end must be a single character".to_string()
                    })?;
                }
                Some(']') | None => {
                    return Err("missing character range end".to_string());
                }
                Some(_) => {
                    self.pos += 1;
                }
            }
        }
        Ok(())
    }
}

/// Validates a `\p{...}` character property name: a Unicode general
/// category, or an `Is`-prefixed block name (block names themselves are not
/// checked against the Unicode database).
fn check_char_property(name: &str) -> Result<(), String> {
    const CATEGORIES: &[&str] = &[
        "L", "Lu", "Ll", "Lt", "Lm", "Lo", "M", "Mn", "Mc", "Me", "N", "Nd", "Nl", "No", "P", "Pc",
        "Pd", "Ps", "Pe", "Pi", "Pf", "Po", "Z", "Zs", "Zl", "Zp", "S", "Sm", "Sc", "Sk", "So",
        "C", "Cc", "Cf", "Co", "Cn",
    ];
    if CATEGORIES.contains(&name) {
        return Ok(());
    }
    if let Some(block) = name.strip_prefix("Is") {
        if !block.is_empty() && block.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Ok(());
        }
    }
    Err(format!("invalid character property '{}'", name))
}

#[cfg(test)]
mod tests {
    use super::check_xsd_regex;

    #[test]
    fn accepts_valid_patterns() {
        for p in [
            "abc",
            "a|b|c",
            "(a{2})*",
            "a*b{2,4}c{0}",
            "[a-z-[aeiou]]",
            "\\d{1,5}\\s([A-Z][a-z]{1,20}\\s){4}",
            "\\p{Lu}*",
            "\\p{IsBasicLatin}+",
            "[\\-+]?[0-9]+",
            "[a-c-]",
            "[-a-c]",
            "[-]",
            "a[-]?c",
            "[a-z--[b-z]]",
            "foo\\?bar",
            "\\i\\c*",
            "[\\n\\r\\t\\\\\\|\\.\\-\\^\\?\\*\\+\\{\\}\\[\\]\\(\\)]*",
        ] {
            assert!(check_xsd_regex(p).is_ok(), "should accept: {p}");
        }
    }

    #[test]
    fn rejects_invalid_patterns() {
        for p in [
            "{5",
            "{5,",
            "{5,6",
            "a{5,2}",
            "(abc",
            "abc)",
            "[abc",
            "\\u0041",
            "\\x41",
            "[\\u0554-\\u0557]",
            "[a-\\d]*",
            "[c-\\S]",
            "\\p{klsak",
            "\\p{Xx}",
            "[\\p]",
            "\\pfoo",
            "(\\p{",
            "a\\",
            "*a",
            "?",
        ] {
            assert!(check_xsd_regex(p).is_err(), "should reject: {p}");
        }
    }
}
