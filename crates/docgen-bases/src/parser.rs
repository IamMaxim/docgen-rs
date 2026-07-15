//! A Pratt (precedence-climbing) parser turning a token stream into an [`Expr`].
//! Tolerant: on a syntax error it returns `Err(BaseError::Expr)`; callers render
//! that as an inline error rather than failing the build.

use crate::ast::{BinaryOp, Expr, UnaryOp};
use crate::lexer::{tokenize, Token};

/// Parse a whole expression string. Trailing tokens after a complete expression
/// are an error.
pub fn parse(src: &str) -> Result<Expr, String> {
    let tokens = tokenize(src);
    let mut p = Parser { tokens, pos: 0 };
    let expr = p.expr(0)?;
    if !matches!(p.peek(), Token::Eof) {
        return Err(format!("unexpected trailing token: {:?}", p.peek()));
    }
    Ok(expr)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

/// Binding powers for binary operators (higher binds tighter). `||` < `&&` <
/// comparison < additive < multiplicative.
fn infix_bp(tok: &Token) -> Option<(BinaryOp, u8, u8)> {
    Some(match tok {
        Token::OrOr => (BinaryOp::Or, 1, 2),
        Token::AndAnd => (BinaryOp::And, 3, 4),
        Token::EqEq => (BinaryOp::Eq, 5, 6),
        Token::NotEq => (BinaryOp::NotEq, 5, 6),
        Token::Lt => (BinaryOp::Lt, 7, 8),
        Token::Gt => (BinaryOp::Gt, 7, 8),
        Token::LtEq => (BinaryOp::LtEq, 7, 8),
        Token::GtEq => (BinaryOp::GtEq, 7, 8),
        Token::Plus => (BinaryOp::Add, 9, 10),
        Token::Minus => (BinaryOp::Sub, 9, 10),
        Token::Star => (BinaryOp::Mul, 11, 12),
        Token::Slash => (BinaryOp::Div, 11, 12),
        Token::Percent => (BinaryOp::Mod, 11, 12),
        _ => return None,
    })
}

impl Parser {
    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn next(&mut self) -> Token {
        let t = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        t
    }

    fn eat(&mut self, want: &Token) -> Result<(), String> {
        if self.peek() == want {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected {:?}, found {:?}", want, self.peek()))
        }
    }

    /// Parse an expression with binding power >= `min_bp`.
    fn expr(&mut self, min_bp: u8) -> Result<Expr, String> {
        let mut lhs = self.prefix()?;
        loop {
            // Postfix: member `.name`, index `[...]`, call `(...)`.
            match self.peek() {
                Token::Dot => {
                    self.pos += 1;
                    let name = match self.next() {
                        Token::Ident(n) => n,
                        // Allow keywords as member names too (rare but harmless).
                        Token::True => "true".to_string(),
                        Token::False => "false".to_string(),
                        Token::Null => "null".to_string(),
                        other => return Err(format!("expected member name, found {other:?}")),
                    };
                    lhs = Expr::Member(Box::new(lhs), name);
                    continue;
                }
                Token::LBracket => {
                    self.pos += 1;
                    let idx = self.expr(0)?;
                    self.eat(&Token::RBracket)?;
                    lhs = Expr::Index(Box::new(lhs), Box::new(idx));
                    continue;
                }
                Token::LParen => {
                    self.pos += 1;
                    let args = self.args()?;
                    self.eat(&Token::RParen)?;
                    lhs = Expr::Call(Box::new(lhs), args);
                    continue;
                }
                _ => {}
            }
            // Infix binary operators.
            let Some((op, lbp, rbp)) = infix_bp(self.peek()) else {
                break;
            };
            if lbp < min_bp {
                break;
            }
            self.pos += 1;
            let rhs = self.expr(rbp)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn args(&mut self) -> Result<Vec<Expr>, String> {
        let mut out = Vec::new();
        // An empty argument/element list (`f()` or `[]`).
        if matches!(self.peek(), Token::RParen | Token::RBracket) {
            return Ok(out);
        }
        loop {
            out.push(self.expr(0)?);
            match self.peek() {
                Token::Comma => {
                    self.pos += 1;
                    // Tolerate a trailing comma before the closing delimiter.
                    if matches!(self.peek(), Token::RParen | Token::RBracket) {
                        break;
                    }
                }
                _ => break,
            }
        }
        Ok(out)
    }

    fn prefix(&mut self) -> Result<Expr, String> {
        match self.next() {
            Token::Number(n) => Ok(Expr::Number(n)),
            Token::Str(s) => Ok(Expr::Str(s)),
            Token::Regex(p, f) => Ok(Expr::Regex(p, f)),
            Token::Ident(name) => Ok(Expr::Ident(name)),
            Token::True => Ok(Expr::Bool(true)),
            Token::False => Ok(Expr::Bool(false)),
            Token::Null => Ok(Expr::Null),
            Token::Bang => Ok(Expr::Unary(UnaryOp::Not, Box::new(self.expr(13)?))),
            Token::Minus => Ok(Expr::Unary(UnaryOp::Neg, Box::new(self.expr(13)?))),
            Token::LParen => {
                let e = self.expr(0)?;
                self.eat(&Token::RParen)?;
                Ok(e)
            }
            Token::LBracket => {
                // A list literal `[a, b, c]`.
                let items = self.args()?;
                self.eat(&Token::RBracket)?;
                // Represent as a call to the built-in `list`, so eval reuses one path.
                Ok(Expr::Call(Box::new(Expr::Ident("__list".into())), items))
            }
            other => Err(format!("unexpected token: {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::BinaryOp;

    #[test]
    fn precedence_mul_over_add() {
        let e = parse("1 + 2 * 3").unwrap();
        // 1 + (2 * 3)
        assert_eq!(
            e,
            Expr::Binary(
                BinaryOp::Add,
                Box::new(Expr::Number(1.0)),
                Box::new(Expr::Binary(
                    BinaryOp::Mul,
                    Box::new(Expr::Number(2.0)),
                    Box::new(Expr::Number(3.0)),
                )),
            )
        );
    }

    #[test]
    fn comparison_below_arithmetic() {
        let e = parse("a + 1 == b").unwrap();
        // (a + 1) == b
        assert!(matches!(e, Expr::Binary(BinaryOp::Eq, _, _)));
    }

    #[test]
    fn and_below_or_binding() {
        // a || b && c  ==>  a || (b && c)
        let e = parse("a || b && c").unwrap();
        match e {
            Expr::Binary(BinaryOp::Or, l, r) => {
                assert_eq!(*l, Expr::Ident("a".into()));
                assert!(matches!(*r, Expr::Binary(BinaryOp::And, _, _)));
            }
            other => panic!("expected Or at top, got {other:?}"),
        }
    }

    #[test]
    fn member_call_chain() {
        let e = parse("file.hasTag(\"x\")").unwrap();
        match e {
            Expr::Call(callee, args) => {
                assert_eq!(args.len(), 1);
                assert_eq!(
                    *callee,
                    Expr::Member(Box::new(Expr::Ident("file".into())), "hasTag".into())
                );
            }
            other => panic!("expected call, got {other:?}"),
        }
    }

    #[test]
    fn method_on_call_result() {
        // link("a").linksTo(x) — call on a member of a call.
        let e = parse("link(\"a\").linksTo(x)").unwrap();
        assert!(matches!(e, Expr::Call(_, _)));
    }

    #[test]
    fn not_and_neg() {
        assert!(matches!(parse("!a").unwrap(), Expr::Unary(UnaryOp::Not, _)));
        assert!(matches!(parse("-5").unwrap(), Expr::Unary(UnaryOp::Neg, _)));
    }

    #[test]
    fn index_access() {
        let e = parse("categories[0]").unwrap();
        assert!(matches!(e, Expr::Index(_, _)));
    }

    #[test]
    fn list_literal_becomes_list_call() {
        let e = parse("[1, 2, 3]").unwrap();
        match e {
            Expr::Call(callee, args) => {
                assert_eq!(*callee, Expr::Ident("__list".into()));
                assert_eq!(args.len(), 3);
            }
            other => panic!("expected list call, got {other:?}"),
        }
    }

    #[test]
    fn grouping() {
        let e = parse("(1 + 2) * 3").unwrap();
        assert!(matches!(e, Expr::Binary(BinaryOp::Mul, _, _)));
    }

    #[test]
    fn trailing_token_is_error() {
        assert!(parse("1 2").is_err());
    }

    #[test]
    fn negation_of_call() {
        // !file.inFolder("Misc")
        let e = parse("!file.inFolder(\"Misc\")").unwrap();
        assert!(matches!(e, Expr::Unary(UnaryOp::Not, _)));
    }
}
