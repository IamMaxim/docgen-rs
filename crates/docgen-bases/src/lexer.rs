//! Tokenizer for the Bases expression language. Produces a flat token stream the
//! Pratt parser consumes. Tolerant: an unrecognized character becomes an `Error`
//! token the parser surfaces, rather than panicking.

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Number(f64),
    /// A string literal's contents (quotes stripped, escapes resolved).
    Str(String),
    Ident(String),
    /// A `/pattern/flags` regex literal (pattern, flags).
    Regex(String, String),
    True,
    False,
    Null,
    // Punctuation / operators.
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    AndAnd,
    OrOr,
    Bang,
    Dot,
    Comma,
    LParen,
    RParen,
    LBracket,
    RBracket,
    /// A lexing error (unexpected char); carries the offending char.
    Error(char),
    Eof,
}

/// Tokenize `src`. Whitespace is skipped. A `/` is a regex literal only when a
/// regex could legally start there (start of input or after an operator/`(`/`,`/
/// `[`); otherwise it is division.
pub fn tokenize(src: &str) -> Vec<Token> {
    let chars: Vec<char> = src.chars().collect();
    let mut tokens: Vec<Token> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '0'..='9' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                match text.parse::<f64>() {
                    Ok(n) => tokens.push(Token::Number(n)),
                    Err(_) => tokens.push(Token::Error(c)),
                }
            }
            '"' | '\'' => {
                let quote = c;
                i += 1;
                let mut s = String::new();
                while i < chars.len() && chars[i] != quote {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                        s.push(match chars[i] {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            '\\' => '\\',
                            '"' => '"',
                            '\'' => '\'',
                            other => other,
                        });
                    } else {
                        s.push(chars[i]);
                    }
                    i += 1;
                }
                if i < chars.len() {
                    i += 1; // closing quote
                }
                tokens.push(Token::Str(s));
            }
            c if c.is_ascii_alphabetic() || c == '_' || c == '$' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '$')
                {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                tokens.push(match word.as_str() {
                    "true" => Token::True,
                    "false" => Token::False,
                    "null" => Token::Null,
                    _ => Token::Ident(word),
                });
            }
            '/' if regex_allowed(&tokens) => {
                // Regex literal /pattern/flags.
                i += 1;
                let mut pat = String::new();
                let mut in_class = false;
                while i < chars.len() && (chars[i] != '/' || in_class) {
                    match chars[i] {
                        '\\' if i + 1 < chars.len() => {
                            pat.push(chars[i]);
                            pat.push(chars[i + 1]);
                            i += 2;
                            continue;
                        }
                        '[' => in_class = true,
                        ']' => in_class = false,
                        _ => {}
                    }
                    pat.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() {
                    i += 1; // closing '/'
                }
                let flag_start = i;
                while i < chars.len() && chars[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let flags: String = chars[flag_start..i].iter().collect();
                tokens.push(Token::Regex(pat, flags));
            }
            _ => {
                let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
                let (tok, len) = match two.as_str() {
                    "==" => (Token::EqEq, 2),
                    "!=" => (Token::NotEq, 2),
                    "<=" => (Token::LtEq, 2),
                    ">=" => (Token::GtEq, 2),
                    "&&" => (Token::AndAnd, 2),
                    "||" => (Token::OrOr, 2),
                    _ => match c {
                        '+' => (Token::Plus, 1),
                        '-' => (Token::Minus, 1),
                        '*' => (Token::Star, 1),
                        '/' => (Token::Slash, 1),
                        '%' => (Token::Percent, 1),
                        '<' => (Token::Lt, 1),
                        '>' => (Token::Gt, 1),
                        '!' => (Token::Bang, 1),
                        '.' => (Token::Dot, 1),
                        ',' => (Token::Comma, 1),
                        '(' => (Token::LParen, 1),
                        ')' => (Token::RParen, 1),
                        '[' => (Token::LBracket, 1),
                        ']' => (Token::RBracket, 1),
                        other => (Token::Error(other), 1),
                    },
                };
                tokens.push(tok);
                i += len;
            }
        }
    }
    tokens.push(Token::Eof);
    tokens
}

/// A `/` starts a regex literal when the previous significant token is not a
/// value/close-paren/ident (i.e. where a *prefix* is expected). At the start of
/// input, or after an operator / `(` / `,` / `[`, a regex is allowed.
fn regex_allowed(tokens: &[Token]) -> bool {
    match tokens.last() {
        None => true,
        Some(t) => matches!(
            t,
            Token::Plus
                | Token::Minus
                | Token::Star
                | Token::Slash
                | Token::Percent
                | Token::EqEq
                | Token::NotEq
                | Token::Lt
                | Token::Gt
                | Token::LtEq
                | Token::GtEq
                | Token::AndAnd
                | Token::OrOr
                | Token::Bang
                | Token::LParen
                | Token::Comma
                | Token::LBracket
                | Token::Dot
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_strings_idents() {
        let t = tokenize(r#"foo == "bar" + 3.5"#);
        assert_eq!(
            t,
            vec![
                Token::Ident("foo".into()),
                Token::EqEq,
                Token::Str("bar".into()),
                Token::Plus,
                Token::Number(3.5),
                Token::Eof
            ]
        );
    }

    #[test]
    fn single_quoted_string() {
        let t = tokenize("'hello world'");
        assert_eq!(t[0], Token::Str("hello world".into()));
    }

    #[test]
    fn keywords() {
        assert_eq!(tokenize("true")[0], Token::True);
        assert_eq!(tokenize("false")[0], Token::False);
        assert_eq!(tokenize("null")[0], Token::Null);
    }

    #[test]
    fn two_char_operators() {
        let t = tokenize("a && b || c >= d <= e != f");
        assert!(t.contains(&Token::AndAnd));
        assert!(t.contains(&Token::OrOr));
        assert!(t.contains(&Token::GtEq));
        assert!(t.contains(&Token::LtEq));
        assert!(t.contains(&Token::NotEq));
    }

    #[test]
    fn member_and_call() {
        let t = tokenize("file.hasTag(\"x\")");
        assert_eq!(
            t,
            vec![
                Token::Ident("file".into()),
                Token::Dot,
                Token::Ident("hasTag".into()),
                Token::LParen,
                Token::Str("x".into()),
                Token::RParen,
                Token::Eof
            ]
        );
    }

    #[test]
    fn regex_literal_vs_division() {
        // After `=` position (start) a regex is allowed.
        let t = tokenize("/ab.c/gi");
        assert_eq!(t[0], Token::Regex("ab.c".into(), "gi".into()));
        // Between two idents, `/` is division.
        let t2 = tokenize("price / age");
        assert!(t2.contains(&Token::Slash));
        assert!(!t2.iter().any(|x| matches!(x, Token::Regex(_, _))));
    }

    #[test]
    fn regex_with_char_class_containing_slash() {
        let t = tokenize(r"/[a/b]/");
        assert_eq!(t[0], Token::Regex("[a/b]".into(), "".into()));
    }

    #[test]
    fn escapes_in_string() {
        let t = tokenize(r#""a\"b\n""#);
        assert_eq!(t[0], Token::Str("a\"b\n".into()));
    }
}
