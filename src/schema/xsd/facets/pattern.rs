//! Translation of XSD regular expressions to Rust `regex` syntax.

use regex::Regex;

/// XML NameStartChar set, as Rust regex character class members.
const NAME_START_CHARS: &str = ":A-Z_a-z\\x{C0}-\\x{D6}\\x{D8}-\\x{F6}\\x{F8}-\\x{2FF}\\x{370}-\\x{37D}\\x{37F}-\\x{1FFF}\\x{200C}-\\x{200D}\\x{2070}-\\x{218F}\\x{2C00}-\\x{2FEF}\\x{3001}-\\x{D7FF}\\x{F900}-\\x{FDCF}\\x{FDF0}-\\x{FFFD}\\x{10000}-\\x{EFFFF}";

/// XML NameChar set (NameStartChar plus `- . 0-9 · ̀-ͯ ‿-⁀`).
const NAME_CHARS: &str = "\\-.0-9\\x{B7}\\x{300}-\\x{36F}\\x{203F}-\\x{2040}:A-Z_a-z\\x{C0}-\\x{D6}\\x{D8}-\\x{F6}\\x{F8}-\\x{2FF}\\x{370}-\\x{37D}\\x{37F}-\\x{1FFF}\\x{200C}-\\x{200D}\\x{2070}-\\x{218F}\\x{2C00}-\\x{2FEF}\\x{3001}-\\x{D7FF}\\x{F900}-\\x{FDCF}\\x{FDF0}-\\x{FFFD}\\x{10000}-\\x{EFFFF}";

/// XSD `\s` set: exactly space, tab, newline, carriage return (unlike Rust's
/// Unicode-aware `\s`).
const XSD_SPACE_CHARS: &str = " \\t\\n\\r";

/// XSD `\w` is "everything except punctuation, separators, and other"
/// (`[#x0-#x10FFFF] - [\p{P}\p{Z}\p{C}]`), which is wider than Rust's `\w`.
const XSD_NON_WORD_CHARS: &str = "\\p{P}\\p{Z}\\p{C}";

/// Translates an XSD regular expression to Rust regex syntax:
///
/// - `\i` / `\I` / `\c` / `\C` (XML name character escapes) are expanded to
///   explicit character classes,
/// - `\s` / `\S` / `\w` / `\W` are redefined to XSD's sets,
/// - `\p{IsBlock}` Unicode block escapes become explicit code-point ranges,
/// - character class subtraction `[a-z-[aeiou]]` becomes Rust's
///   `[a-z--[aeiou]]` difference syntax,
/// - `^` and `$` are literal characters in XSD; escape them outside classes.
fn translate_xsd_pattern(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut chars = pattern.chars().peekable();
    let mut class_depth = 0usize;

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let Some(&next) = chars.peek() else {
                    out.push('\\');
                    break;
                };
                match next {
                    'i' => {
                        chars.next();
                        if class_depth > 0 {
                            out.push_str(NAME_START_CHARS);
                        } else {
                            out.push('[');
                            out.push_str(NAME_START_CHARS);
                            out.push(']');
                        }
                    }
                    'c' => {
                        chars.next();
                        if class_depth > 0 {
                            out.push_str(NAME_CHARS);
                        } else {
                            out.push('[');
                            out.push_str(NAME_CHARS);
                            out.push(']');
                        }
                    }
                    'I' => {
                        chars.next();
                        // Negated classes can't be embedded inside another
                        // class; only translate at top level.
                        if class_depth == 0 {
                            out.push_str("[^");
                            out.push_str(NAME_START_CHARS);
                            out.push(']');
                        }
                    }
                    'C' => {
                        chars.next();
                        if class_depth == 0 {
                            out.push_str("[^");
                            out.push_str(NAME_CHARS);
                            out.push(']');
                        }
                    }
                    's' => {
                        chars.next();
                        if class_depth > 0 {
                            out.push_str(XSD_SPACE_CHARS);
                        } else {
                            out.push('[');
                            out.push_str(XSD_SPACE_CHARS);
                            out.push(']');
                        }
                    }
                    'S' => {
                        chars.next();
                        if class_depth == 0 {
                            out.push_str("[^");
                            out.push_str(XSD_SPACE_CHARS);
                            out.push(']');
                        }
                    }
                    'w' => {
                        chars.next();
                        if class_depth == 0 {
                            out.push_str("[^");
                            out.push_str(XSD_NON_WORD_CHARS);
                            out.push(']');
                        } else {
                            // Approximation: cannot negate inside a class
                            out.push_str("\\w");
                        }
                    }
                    'W' => {
                        chars.next();
                        if class_depth > 0 {
                            out.push_str(XSD_NON_WORD_CHARS);
                        } else {
                            out.push('[');
                            out.push_str(XSD_NON_WORD_CHARS);
                            out.push(']');
                        }
                    }
                    'p' | 'P' => {
                        chars.next();
                        translate_property_escape(next == 'P', &mut chars, class_depth, &mut out);
                    }
                    _ => {
                        out.push('\\');
                        out.push(next);
                        chars.next();
                    }
                }
            }
            '[' => {
                class_depth += 1;
                out.push('[');
            }
            ']' => {
                class_depth = class_depth.saturating_sub(1);
                out.push(']');
            }
            '-' if class_depth > 0 && chars.peek() == Some(&'[') => {
                // XSD class subtraction → Rust class difference
                out.push_str("--");
            }
            '^' if class_depth == 0 => out.push_str("\\^"),
            '$' if class_depth == 0 => out.push_str("\\$"),
            _ => out.push(ch),
        }
    }

    out
}

/// Translates a `\p{...}` / `\P{...}` property escape. General categories
/// pass through to Rust's engine; `Is...` Unicode block names (which Rust
/// does not support) are expanded to explicit code-point ranges.
fn translate_property_escape(
    negated: bool,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    class_depth: usize,
    out: &mut String,
) {
    // Collect "{...}" if present; otherwise pass through verbatim.
    if chars.peek() != Some(&'{') {
        out.push('\\');
        out.push(if negated { 'P' } else { 'p' });
        return;
    }
    let mut name = String::new();
    chars.next(); // consume '{'
    for c in chars.by_ref() {
        if c == '}' {
            break;
        }
        name.push(c);
    }

    if let Some(block) = name.strip_prefix("Is") {
        if let Some(ranges) = unicode_block_ranges(block) {
            if class_depth > 0 {
                // Embed the ranges as members (negation unsupported here)
                if !negated {
                    out.push_str(ranges);
                } else {
                    // Force a compile failure rather than wrong semantics
                    out.push_str("\\P{unsupported-in-class}");
                }
            } else {
                out.push('[');
                if negated {
                    out.push('^');
                }
                out.push_str(ranges);
                out.push(']');
            }
        } else {
            // Unknown block: emit something uncompilable so the pattern is
            // skipped instead of misinterpreted.
            out.push_str("\\p{unknown-block}");
        }
    } else {
        // General category / script: Rust regex understands these natively.
        out.push('\\');
        out.push(if negated { 'P' } else { 'p' });
        out.push('{');
        out.push_str(&name);
        out.push('}');
    }
}

/// Code-point ranges (as Rust regex class members) for the Unicode 3.1
/// blocks XSD 1.0 block escapes refer to.
fn unicode_block_ranges(block: &str) -> Option<&'static str> {
    Some(match block {
        "BasicLatin" => "\\x{0000}-\\x{007F}",
        "Latin-1Supplement" => "\\x{0080}-\\x{00FF}",
        "LatinExtended-A" => "\\x{0100}-\\x{017F}",
        "LatinExtended-B" => "\\x{0180}-\\x{024F}",
        "IPAExtensions" => "\\x{0250}-\\x{02AF}",
        "SpacingModifierLetters" => "\\x{02B0}-\\x{02FF}",
        "CombiningDiacriticalMarks" => "\\x{0300}-\\x{036F}",
        "Greek" => "\\x{0370}-\\x{03FF}",
        "Cyrillic" => "\\x{0400}-\\x{04FF}",
        "Armenian" => "\\x{0530}-\\x{058F}",
        "Hebrew" => "\\x{0590}-\\x{05FF}",
        "Arabic" => "\\x{0600}-\\x{06FF}",
        "Syriac" => "\\x{0700}-\\x{074F}",
        "Thaana" => "\\x{0780}-\\x{07BF}",
        "Devanagari" => "\\x{0900}-\\x{097F}",
        "Bengali" => "\\x{0980}-\\x{09FF}",
        "Gurmukhi" => "\\x{0A00}-\\x{0A7F}",
        "Gujarati" => "\\x{0A80}-\\x{0AFF}",
        "Oriya" => "\\x{0B00}-\\x{0B7F}",
        "Tamil" => "\\x{0B80}-\\x{0BFF}",
        "Telugu" => "\\x{0C00}-\\x{0C7F}",
        "Kannada" => "\\x{0C80}-\\x{0CFF}",
        "Malayalam" => "\\x{0D00}-\\x{0D7F}",
        "Sinhala" => "\\x{0D80}-\\x{0DFF}",
        "Thai" => "\\x{0E00}-\\x{0E7F}",
        "Lao" => "\\x{0E80}-\\x{0EFF}",
        "Tibetan" => "\\x{0F00}-\\x{0FFF}",
        "Myanmar" => "\\x{1000}-\\x{109F}",
        "Georgian" => "\\x{10A0}-\\x{10FF}",
        "HangulJamo" => "\\x{1100}-\\x{11FF}",
        "Ethiopic" => "\\x{1200}-\\x{137F}",
        "Cherokee" => "\\x{13A0}-\\x{13FF}",
        "UnifiedCanadianAboriginalSyllabics" => "\\x{1400}-\\x{167F}",
        "Ogham" => "\\x{1680}-\\x{169F}",
        "Runic" => "\\x{16A0}-\\x{16FF}",
        "Khmer" => "\\x{1780}-\\x{17FF}",
        "Mongolian" => "\\x{1800}-\\x{18AF}",
        "LatinExtendedAdditional" => "\\x{1E00}-\\x{1EFF}",
        "GreekExtended" => "\\x{1F00}-\\x{1FFF}",
        "GeneralPunctuation" => "\\x{2000}-\\x{206F}",
        "SuperscriptsandSubscripts" => "\\x{2070}-\\x{209F}",
        "CurrencySymbols" => "\\x{20A0}-\\x{20CF}",
        "CombiningMarksforSymbols" => "\\x{20D0}-\\x{20FF}",
        "LetterlikeSymbols" => "\\x{2100}-\\x{214F}",
        "NumberForms" => "\\x{2150}-\\x{218F}",
        "Arrows" => "\\x{2190}-\\x{21FF}",
        "MathematicalOperators" => "\\x{2200}-\\x{22FF}",
        "MiscellaneousTechnical" => "\\x{2300}-\\x{23FF}",
        "ControlPictures" => "\\x{2400}-\\x{243F}",
        "OpticalCharacterRecognition" => "\\x{2440}-\\x{245F}",
        "EnclosedAlphanumerics" => "\\x{2460}-\\x{24FF}",
        "BoxDrawing" => "\\x{2500}-\\x{257F}",
        "BlockElements" => "\\x{2580}-\\x{259F}",
        "GeometricShapes" => "\\x{25A0}-\\x{25FF}",
        "MiscellaneousSymbols" => "\\x{2600}-\\x{26FF}",
        "Dingbats" => "\\x{2700}-\\x{27BF}",
        "BraillePatterns" => "\\x{2800}-\\x{28FF}",
        "CJKRadicalsSupplement" => "\\x{2E80}-\\x{2EFF}",
        "KangxiRadicals" => "\\x{2F00}-\\x{2FDF}",
        "IdeographicDescriptionCharacters" => "\\x{2FF0}-\\x{2FFF}",
        "CJKSymbolsandPunctuation" => "\\x{3000}-\\x{303F}",
        "Hiragana" => "\\x{3040}-\\x{309F}",
        "Katakana" => "\\x{30A0}-\\x{30FF}",
        "Bopomofo" => "\\x{3100}-\\x{312F}",
        "HangulCompatibilityJamo" => "\\x{3130}-\\x{318F}",
        "Kanbun" => "\\x{3190}-\\x{319F}",
        "BopomofoExtended" => "\\x{31A0}-\\x{31BF}",
        "EnclosedCJKLettersandMonths" => "\\x{3200}-\\x{32FF}",
        "CJKCompatibility" => "\\x{3300}-\\x{33FF}",
        "CJKUnifiedIdeographsExtensionA" => "\\x{3400}-\\x{4DBF}",
        "CJKUnifiedIdeographs" => "\\x{4E00}-\\x{9FFF}",
        "YiSyllables" => "\\x{A000}-\\x{A48F}",
        "YiRadicals" => "\\x{A490}-\\x{A4CF}",
        "HangulSyllables" => "\\x{AC00}-\\x{D7AF}",
        "PrivateUse" => "\\x{E000}-\\x{F8FF}\\x{F0000}-\\x{FFFFD}\\x{100000}-\\x{10FFFD}",
        "CJKCompatibilityIdeographs" => "\\x{F900}-\\x{FAFF}",
        "AlphabeticPresentationForms" => "\\x{FB00}-\\x{FB4F}",
        "ArabicPresentationForms-A" => "\\x{FB50}-\\x{FDFF}",
        "CombiningHalfMarks" => "\\x{FE20}-\\x{FE2F}",
        "CJKCompatibilityForms" => "\\x{FE30}-\\x{FE4F}",
        "SmallFormVariants" => "\\x{FE50}-\\x{FE6F}",
        "ArabicPresentationForms-B" => "\\x{FE70}-\\x{FEFE}",
        "Specials" => "\\x{FEFF}\\x{FFF0}-\\x{FFFD}",
        "HalfwidthandFullwidthForms" => "\\x{FF00}-\\x{FFEF}",
        "OldItalic" => "\\x{10300}-\\x{1032F}",
        "Gothic" => "\\x{10330}-\\x{1034F}",
        "Deseret" => "\\x{10400}-\\x{1044F}",
        "ByzantineMusicalSymbols" => "\\x{1D000}-\\x{1D0FF}",
        "MusicalSymbols" => "\\x{1D100}-\\x{1D1FF}",
        "MathematicalAlphanumericSymbols" => "\\x{1D400}-\\x{1D7FF}",
        "CJKUnifiedIdeographsExtensionB" => "\\x{20000}-\\x{2A6DF}",
        "CJKCompatibilityIdeographsSupplement" => "\\x{2F800}-\\x{2FA1F}",
        "Tags" => "\\x{E0000}-\\x{E007F}",
        _ => return None,
    })
}

/// Compiles an XSD pattern to an anchored Rust [`Regex`], translating
/// XSD-specific constructs first. Returns `None` (with a log) when the
/// pattern uses constructs the regex engine cannot express.
pub(super) fn compile_xsd_pattern(pattern: &str) -> Option<Regex> {
    let translated = translate_xsd_pattern(pattern);
    let anchored = format!("^(?:{})$", translated);
    match Regex::new(&anchored) {
        Ok(regex) => Some(regex),
        Err(e) => {
            tracing::warn!("Unsupported XSD pattern '{}': {}", pattern, e);
            None
        }
    }
}
