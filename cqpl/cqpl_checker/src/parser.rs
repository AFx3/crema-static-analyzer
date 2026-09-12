use crate::ast::{
    LabelPredicate, MayPredicate, PathFormula, PathQuantifier, StateFormula,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenKind {
    Ident(String),
    LParen,
    RParen,
    LBracket,
    RBracket,
    Dot,
    Bang,
    And,
    Or,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    offset: usize,
}

pub fn parse_query(input: &str) -> Result<StateFormula, String> {
    let cleaned = strip_comments(input);
    let tokens = lex(&cleaned)?;
    if tokens.is_empty() {
        return Err("empty CQPL query".into());
    }
    let mut p = Parser { tokens, pos: 0 };
    let formula = p.parse_state()?;
    if let Some(tok) = p.peek() {
        return Err(format!(
            "unexpected token {:?} at byte {} after complete formula",
            tok.kind, tok.offset
        ));
    }
    Ok(formula)
}

fn strip_comments(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let hash = line.find('#');
            let slash = line.find("//");
            let cut = match (hash, slash) {
                (Some(a), Some(b)) => a.min(b),
                (Some(a), None) => a,
                (None, Some(b)) => b,
                (None, None) => line.len(),
            };
            &line[..cut]
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn lex(input: &str) -> Result<Vec<Token>, String> {
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let offset = i;
        match c {
            '(' => { out.push(Token { kind: TokenKind::LParen, offset }); i += 1; }
            ')' => { out.push(Token { kind: TokenKind::RParen, offset }); i += 1; }
            '[' => { out.push(Token { kind: TokenKind::LBracket, offset }); i += 1; }
            ']' => { out.push(Token { kind: TokenKind::RBracket, offset }); i += 1; }
            '.' => { out.push(Token { kind: TokenKind::Dot, offset }); i += 1; }
            '!' => { out.push(Token { kind: TokenKind::Bang, offset }); i += 1; }
            '&' if i + 1 < bytes.len() && bytes[i + 1] as char == '&' => {
                out.push(Token { kind: TokenKind::And, offset }); i += 2;
            }
            '|' if i + 1 < bytes.len() && bytes[i + 1] as char == '|' => {
                out.push(Token { kind: TokenKind::Or, offset }); i += 2;
            }
            _ if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    let d = bytes[i] as char;
                    if d.is_ascii_alphanumeric() || d == '_' {
                        i += 1;
                    } else {
                        break;
                    }
                }
                out.push(Token {
                    kind: TokenKind::Ident(input[start..i].to_string()),
                    offset,
                });
            }
            _ => return Err(format!("invalid character {:?} at byte {}", c, i)),
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> { self.tokens.get(self.pos) }

    fn bump(&mut self) -> Option<Token> {
        let tok = self.tokens.get(self.pos).cloned();
        if tok.is_some() { self.pos += 1; }
        tok
    }

    fn peek_ident(&self, name: &str) -> bool {
        matches!(self.peek(), Some(Token { kind: TokenKind::Ident(s), .. }) if s.eq_ignore_ascii_case(name))
    }

    fn consume_ident(&mut self, name: &str) -> bool {
        if self.peek_ident(name) { self.pos += 1; true } else { false }
    }

    fn expect_ident_any(&mut self, what: &str) -> Result<String, String> {
        match self.bump() {
            Some(Token { kind: TokenKind::Ident(s), .. }) => Ok(s),
            Some(t) => Err(format!("expected {}, found {:?} at byte {}", what, t.kind, t.offset)),
            None => Err(format!("expected {}, found end of input", what)),
        }
    }

    fn consume_simple(&mut self, kind: TokenKind) -> bool {
        if self.peek().map(|t| &t.kind) == Some(&kind) { self.pos += 1; true } else { false }
    }

    fn expect_simple(&mut self, kind: TokenKind, desc: &str) -> Result<(), String> {
        if self.consume_simple(kind.clone()) { Ok(()) } else {
            match self.peek() {
                Some(t) => Err(format!("expected {}, found {:?} at byte {}", desc, t.kind, t.offset)),
                None => Err(format!("expected {}, found end of input", desc)),
            }
        }
    }

    fn parse_state(&mut self) -> Result<StateFormula, String> { self.parse_or() }

    fn parse_or(&mut self) -> Result<StateFormula, String> {
        let mut lhs = self.parse_and()?;
        while self.consume_simple(TokenKind::Or) {
            let rhs = self.parse_and()?;
            lhs = StateFormula::Or(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<StateFormula, String> {
        let mut lhs = self.parse_unary()?;
        while self.consume_simple(TokenKind::And) {
            let rhs = self.parse_unary()?;
            lhs = StateFormula::And(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<StateFormula, String> {
        if self.consume_simple(TokenKind::Bang) || self.consume_ident("not") {
            return Ok(StateFormula::Not(Box::new(self.parse_unary()?)));
        }

        if self.consume_ident("exists") {
            let logic_var = self.expect_ident_any("logical variable after 'exists'")?;
            self.expect_simple(TokenKind::Dot, "'.' after quantified variable")?;
            let body = self.parse_state()?;
            return Ok(StateFormula::Exists { logic_var, body: Box::new(body) });
        }
        if self.consume_ident("forall") {
            let logic_var = self.expect_ident_any("logical variable after 'forall'")?;
            self.expect_simple(TokenKind::Dot, "'.' after quantified variable")?;
            let body = self.parse_state()?;
            return Ok(StateFormula::ForAll { logic_var, body: Box::new(body) });
        }

        for (name, q, ctor) in [
            ("EX", PathQuantifier::Exists, "X"),
            ("AX", PathQuantifier::ForAll, "X"),
            ("EF", PathQuantifier::Exists, "F"),
            ("AF", PathQuantifier::ForAll, "F"),
            ("EG", PathQuantifier::Exists, "G"),
            ("AG", PathQuantifier::ForAll, "G"),
        ] {
            if self.consume_ident(name) {
                let arg = Box::new(self.parse_unary()?);
                let formula = match ctor {
                    "X" => PathFormula::Next(arg),
                    "F" => PathFormula::Eventually(arg),
                    "G" => PathFormula::Globally(arg),
                    _ => unreachable!(),
                };
                return Ok(StateFormula::Path { quantifier: q, formula });
            }
        }

        if self.consume_ident("E") {
            return self.parse_explicit_path(PathQuantifier::Exists);
        }
        if self.consume_ident("A") {
            return self.parse_explicit_path(PathQuantifier::ForAll);
        }

        self.parse_primary()
    }

    fn parse_explicit_path(&mut self, quantifier: PathQuantifier) -> Result<StateFormula, String> {
        if self.consume_simple(TokenKind::LBracket) {
            let lhs = self.parse_state()?;
            if !self.consume_ident("U") {
                return Err("expected 'U' in bracketed path formula".into());
            }
            let rhs = self.parse_state()?;
            self.expect_simple(TokenKind::RBracket, "']' after until formula")?;
            return Ok(StateFormula::Path {
                quantifier,
                formula: PathFormula::Until(Box::new(lhs), Box::new(rhs)),
            });
        }

        let parenthesized = self.consume_simple(TokenKind::LParen);
        let formula = if self.consume_ident("X") {
            PathFormula::Next(Box::new(self.parse_state()?))
        } else if self.consume_ident("F") {
            PathFormula::Eventually(Box::new(self.parse_state()?))
        } else if self.consume_ident("G") {
            PathFormula::Globally(Box::new(self.parse_state()?))
        } else {
            PathFormula::State(Box::new(self.parse_state()?))
        };
        if parenthesized {
            self.expect_simple(TokenKind::RParen, "')' after path formula")?;
        }
        Ok(StateFormula::Path { quantifier, formula })
    }

    fn parse_primary(&mut self) -> Result<StateFormula, String> {
        if self.consume_simple(TokenKind::LParen) {
            let f = self.parse_state()?;
            self.expect_simple(TokenKind::RParen, "')'")?;
            return Ok(f);
        }

        let pred = self.expect_ident_any("CQPL predicate or parenthesized formula")?;
        self.expect_simple(TokenKind::LParen, "'(' after predicate")?;
        let logic_var = self.expect_ident_any("logical variable as predicate argument")?;
        self.expect_simple(TokenKind::RParen, "')' after predicate argument")?;

        let lower = pred.to_ascii_lowercase();
        match lower.as_str() {
            "alloc" => Ok(StateFormula::May { predicate: MayPredicate::Alloc, logic_var }),
            "drop" => Ok(StateFormula::May { predicate: MayPredicate::Drop, logic_var }),
            "own_forg" | "ownforg" => Ok(StateFormula::May { predicate: MayPredicate::OwnForg, logic_var }),
            "alloc_l" => Ok(StateFormula::Label { predicate: LabelPredicate::Alloc, logic_var }),
            "drop_l" => Ok(StateFormula::Label { predicate: LabelPredicate::Drop, logic_var }),
            "read_l" => Ok(StateFormula::Label { predicate: LabelPredicate::Read, logic_var }),
            "write_l" => Ok(StateFormula::Label { predicate: LabelPredicate::Write, logic_var }),
            "use_l" => Ok(StateFormula::Label { predicate: LabelPredicate::Use, logic_var }),
            _ => Err(format!("unknown CQPL predicate '{pred}'")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_theoretical_leak_formula() {
        let q = parse_query("exists x. E(F (alloc(x) && EX EG !drop(x)))").unwrap();
        assert!(q.free_vars().is_empty());
    }

    #[test]
    fn parses_double_free_formula_with_until() {
        let q = parse_query(
            "exists x. EF (alloc(x) && EX EF (drop_l(x) && EX E[(!alloc_l(x)) U drop_l(x)]))"
        ).unwrap();
        assert!(q.free_vars().is_empty());
    }

    #[test]
    fn reports_open_logic_variable() {
        let q = parse_query("EF use_l(x)").unwrap();
        assert!(q.free_vars().contains("x"));
    }
}
