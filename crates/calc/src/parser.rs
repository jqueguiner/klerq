//! Formula parser: tokenizer + recursive-descent parser producing an [`Expr`]
//! AST. Grammar (lowest→highest precedence):
//!
//! ```text
//! expr    := term (('+' | '-') term)*
//! term    := factor (('*' | '/') factor)*
//! factor  := '-' factor | primary
//! primary := number
//!          | '(' expr ')'
//!          | ident '(' args? ')'      // function call
//!          | ident ':' ident          // range
//!          | ident                    // cell reference
//! args    := expr (',' expr)*
//! ```

/// Formula AST node.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Ref(String),
    Range(String, String),
    Neg(Box<Expr>),
    BinOp {
        op: char,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Punct(char),
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let mut toks = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            let n = s.parse::<f64>().map_err(|_| format!("bad number {s:?}"))?;
            toks.push(Tok::Num(n));
        } else if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                i += 1;
            }
            toks.push(Tok::Ident(chars[start..i].iter().collect()));
        } else if "+-*/(),:".contains(c) {
            toks.push(Tok::Punct(c));
            i += 1;
        } else {
            return Err(format!("unexpected char {c:?}"));
        }
    }
    Ok(toks)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat_punct(&mut self, c: char) -> Result<(), String> {
        match self.next() {
            Some(Tok::Punct(p)) if p == c => Ok(()),
            other => Err(format!("expected {c:?}, found {other:?}")),
        }
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let mut lhs = self.term()?;
        while let Some(Tok::Punct(op @ ('+' | '-'))) = self.peek().cloned() {
            self.pos += 1;
            let rhs = self.term()?;
            lhs = Expr::BinOp {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn term(&mut self) -> Result<Expr, String> {
        let mut lhs = self.factor()?;
        while let Some(Tok::Punct(op @ ('*' | '/'))) = self.peek().cloned() {
            self.pos += 1;
            let rhs = self.factor()?;
            lhs = Expr::BinOp {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn factor(&mut self) -> Result<Expr, String> {
        if let Some(Tok::Punct('-')) = self.peek() {
            self.pos += 1;
            return Ok(Expr::Neg(Box::new(self.factor()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Number(n)),
            Some(Tok::Punct('(')) => {
                let e = self.expr()?;
                self.eat_punct(')')?;
                Ok(e)
            }
            Some(Tok::Ident(name)) => match self.peek() {
                Some(Tok::Punct('(')) => {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::Punct(')')) {
                        args.push(self.expr()?);
                        while self.peek() == Some(&Tok::Punct(',')) {
                            self.pos += 1;
                            args.push(self.expr()?);
                        }
                    }
                    self.eat_punct(')')?;
                    Ok(Expr::Call { name, args })
                }
                Some(Tok::Punct(':')) => {
                    self.pos += 1;
                    match self.next() {
                        Some(Tok::Ident(end)) => Ok(Expr::Range(name, end)),
                        other => Err(format!("expected range end, found {other:?}")),
                    }
                }
                _ => Ok(Expr::Ref(name)),
            },
            other => Err(format!("unexpected token {other:?}")),
        }
    }
}

/// Parse a formula body (without the leading `=`) into an [`Expr`].
pub fn parse(src: &str) -> Result<Expr, String> {
    let toks = tokenize(src)?;
    if toks.is_empty() {
        return Err("empty formula".into());
    }
    let mut p = Parser { toks, pos: 0 };
    let e = p.expr()?;
    if p.pos != p.toks.len() {
        return Err(format!("trailing tokens from position {}", p.pos));
    }
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_number() {
        assert_eq!(parse("42").unwrap(), Expr::Number(42.0));
    }

    #[test]
    fn parses_precedence() {
        // 2+3*4 => 2 + (3*4)
        let e = parse("2+3*4").unwrap();
        assert_eq!(
            e,
            Expr::BinOp {
                op: '+',
                lhs: Box::new(Expr::Number(2.0)),
                rhs: Box::new(Expr::BinOp {
                    op: '*',
                    lhs: Box::new(Expr::Number(3.0)),
                    rhs: Box::new(Expr::Number(4.0)),
                }),
            }
        );
    }

    #[test]
    fn parses_ref_and_range() {
        assert_eq!(parse("A1").unwrap(), Expr::Ref("A1".into()));
        assert_eq!(
            parse("A1:B2").unwrap(),
            Expr::Range("A1".into(), "B2".into())
        );
    }

    #[test]
    fn parses_call() {
        assert_eq!(
            parse("SUM(A1:A3, 5)").unwrap(),
            Expr::Call {
                name: "SUM".into(),
                args: vec![Expr::Range("A1".into(), "A3".into()), Expr::Number(5.0)],
            }
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("2 +").is_err());
        assert!(parse("(1").is_err());
        assert!(parse("").is_err());
    }
}
