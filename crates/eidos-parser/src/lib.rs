//! Lexer, recursive-descent parser, and error type for the tpt-eidos MVK
//! surface language. Pure `std`; no external crates.

mod ast;
pub use ast::*;

#[derive(Clone, Debug, PartialEq)]
pub enum ParseError {
    UnexpectedEof,
    UnexpectedToken(String),
    InvalidNumber(String),
    Message(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::UnexpectedEof => write!(f, "unexpected end of input"),
            ParseError::UnexpectedToken(s) => write!(f, "unexpected token: {s}"),
            ParseError::InvalidNumber(s) => write!(f, "invalid number literal: {s}"),
            ParseError::Message(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Lexical tokens.
#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Num(f64),
    Fn,
    Type,
    Requires,
    Ensures,
    Effects,
    Let,
    If,
    Else,
    Return,
    As,
    Array,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Colon,
    Eq,
    Arrow,
    Pipe,
    Dot,
    Le,
    Ge,
    EqEq,
    Ne,
    Lt,
    Gt,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    And,
    Or,
    Not,
}

fn is_base_type(s: &str) -> bool {
    matches!(
        s,
        "f64" | "f32" | "i64" | "i32" | "i8" | "u64" | "u32" | "u8" | "bool" | "char" | "Unit"
    )
}

fn keyword(s: &str) -> Option<Tok> {
    Some(match s {
        "fn" => Tok::Fn,
        "type" => Tok::Type,
        "requires" => Tok::Requires,
        "ensures" => Tok::Ensures,
        "effects" => Tok::Effects,
        "let" => Tok::Let,
        "if" => Tok::If,
        "else" => Tok::Else,
        "return" => Tok::Return,
        "as" => Tok::As,
        "Array" => Tok::Array,
        _ => return None,
    })
}

struct Lexer;

impl Lexer {
    fn run(src: &str) -> Result<Vec<Tok>, ParseError> {
        let chars: Vec<char> = src.chars().collect();
        let mut i = 0;
        let mut toks = Vec::new();
        while i < chars.len() {
            let c = chars[i];
            if c.is_whitespace() {
                i += 1;
                continue;
            }
            if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            if c.is_ascii_digit()
                || (c == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit())
            {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                let v: f64 = s
                    .parse()
                    .map_err(|_| ParseError::InvalidNumber(s.clone()))?;
                toks.push(Tok::Num(v));
                continue;
            }
            if c.is_alphabetic() || c == '_' {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                toks.push(keyword(&s).unwrap_or(Tok::Ident(s)));
                continue;
            }
            let (t, consumed) = match c {
                '(' => (Tok::LParen, 1),
                ')' => (Tok::RParen, 1),
                '{' => (Tok::LBrace, 1),
                '}' => (Tok::RBrace, 1),
                '[' => (Tok::LBracket, 1),
                ']' => (Tok::RBracket, 1),
                ',' => (Tok::Comma, 1),
                ';' => (Tok::Semi, 1),
                ':' => (Tok::Colon, 1),
                '|' => (Tok::Pipe, 1),
                '.' => (Tok::Dot, 1),
                '+' => (Tok::Plus, 1),
                '-' if i + 1 < chars.len() && chars[i + 1] == '>' => (Tok::Arrow, 2),
                '!' if i + 1 < chars.len() && chars[i + 1] == '=' => (Tok::Ne, 2),
                '!' => (Tok::Not, 1),
                '-' => (Tok::Minus, 1),
                '*' => (Tok::Star, 1),
                '/' => (Tok::Slash, 1),
                '%' => (Tok::Percent, 1),
                '=' if i + 1 < chars.len() && chars[i + 1] == '=' => (Tok::EqEq, 2),
                '=' => (Tok::Eq, 1),
                '<' if i + 1 < chars.len() && chars[i + 1] == '=' => (Tok::Le, 2),
                '<' => (Tok::Lt, 1),
                '>' if i + 1 < chars.len() && chars[i + 1] == '=' => (Tok::Ge, 2),
                '>' => (Tok::Gt, 1),
                '&' if i + 1 < chars.len() && chars[i + 1] == '&' => (Tok::And, 2),
                '&' => return Err(ParseError::UnexpectedToken("&".into())),
                _ => return Err(ParseError::UnexpectedToken(c.to_string())),
            };
            i += consumed;
            toks.push(t);
        }
        Ok(toks)
    }
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn new(toks: Vec<Tok>) -> Self {
        Parser { toks, pos: 0 }
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn peek2(&self) -> Option<&Tok> {
        self.toks.get(self.pos + 1)
    }

    fn advance(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, t: &Tok) -> Result<(), ParseError> {
        match self.peek() {
            Some(x) if x == t => {
                self.pos += 1;
                Ok(())
            }
            Some(x) => Err(ParseError::UnexpectedToken(format!(
                "{x:?} (expected {t:?})"
            ))),
            None => Err(ParseError::UnexpectedEof),
        }
    }

    fn eat_ident(&mut self) -> Result<String, ParseError> {
        match self.advance() {
            Some(Tok::Ident(s)) => Ok(s),
            Some(t) => Err(ParseError::UnexpectedToken(format!(
                "{t:?} (expected identifier)"
            ))),
            None => Err(ParseError::UnexpectedEof),
        }
    }

    fn parse_module(&mut self) -> Result<Module, ParseError> {
        let mut items = Vec::new();
        while self.peek().is_some() {
            items.push(self.parse_item()?);
        }
        Ok(Module { items })
    }

    fn parse_item(&mut self) -> Result<Item, ParseError> {
        match self.peek() {
            Some(Tok::Type) => {
                self.advance();
                let name = self.eat_ident()?;
                self.eat(&Tok::Eq)?;
                let ty = self.parse_type()?;
                if self.peek() == Some(&Tok::Semi) {
                    self.advance();
                }
                Ok(Item::TypeAlias { name, ty })
            }
            Some(Tok::Fn) => {
                self.advance();
                let name = self.eat_ident()?;
                self.eat(&Tok::LParen)?;
                let mut params = Vec::new();
                if self.peek() != Some(&Tok::RParen) {
                    loop {
                        let pname = self.eat_ident()?;
                        self.eat(&Tok::Colon)?;
                        let pty = self.parse_type()?;
                        params.push((pname, pty));
                        if self.peek() == Some(&Tok::Comma) {
                            self.advance();
                            continue;
                        }
                        break;
                    }
                }
                self.eat(&Tok::RParen)?;
                self.eat(&Tok::Arrow)?;
                let ret = self.parse_type()?;
                let mut requires = None;
                let mut ensures = None;
                let mut effects = Vec::new();
                if self.peek() == Some(&Tok::Requires) {
                    self.advance();
                    requires = Some(self.parse_expr()?);
                }
                if self.peek() == Some(&Tok::Ensures) {
                    self.advance();
                    self.eat(&Tok::Pipe)?;
                    let b = self.eat_ident()?;
                    self.eat(&Tok::Pipe)?;
                    let body = self.parse_expr()?;
                    ensures = Some(Expr::Lambda {
                        params: vec![b],
                        body: Box::new(body),
                    });
                }
                if self.peek() == Some(&Tok::Effects) {
                    self.advance();
                    self.eat(&Tok::LBracket)?;
                    let mut effs = Vec::new();
                    if self.peek() != Some(&Tok::RBracket) {
                        loop {
                            effs.push(self.eat_ident()?);
                            if self.peek() == Some(&Tok::Comma) {
                                self.advance();
                                continue;
                            }
                            break;
                        }
                    }
                    self.eat(&Tok::RBracket)?;
                    effects = effs;
                }
                self.eat(&Tok::LBrace)?;
                let body = self.parse_expr()?;
                if self.peek() == Some(&Tok::Semi) {
                    self.advance();
                }
                self.eat(&Tok::RBrace)?;
                Ok(Item::Fn(Box::new(Fun {
                    name,
                    params,
                    ret,
                    requires,
                    ensures,
                    effects,
                    body,
                })))
            }
            _ => Err(ParseError::UnexpectedToken(format!(
                "{:?} (expected item)",
                self.peek()
            ))),
        }
    }

    fn parse_type(&mut self) -> Result<Type, ParseError> {
        if self.peek() == Some(&Tok::LBrace) {
            self.advance();
            let bind = self.eat_ident()?;
            self.eat(&Tok::Colon)?;
            let ty = self.parse_type()?;
            self.eat(&Tok::Pipe)?;
            let pred = self.parse_expr()?;
            self.eat(&Tok::RBrace)?;
            return Ok(Type::Refine {
                bind,
                ty: Box::new(ty),
                pred: Box::new(pred),
            });
        }
        if self.peek() == Some(&Tok::Array) && self.peek2() == Some(&Tok::Lt) {
            self.advance();
            self.advance();
            let inner = self.parse_type()?;
            self.eat(&Tok::Comma)?;
            let n = match self.advance() {
                Some(Tok::Num(n)) => {
                    if n.fract() != 0.0 {
                        return Err(ParseError::Message(
                            "Array length must be an integer".into(),
                        ));
                    }
                    n as u64
                }
                Some(t) => {
                    return Err(ParseError::UnexpectedToken(format!(
                        "{t:?} (expected length)"
                    )))
                }
                None => return Err(ParseError::UnexpectedEof),
            };
            self.eat(&Tok::Gt)?;
            return Ok(Type::Array(Box::new(inner), n));
        }
        match self.advance() {
            Some(Tok::Ident(s)) => Ok(if is_base_type(&s) {
                Type::Base(s)
            } else {
                Type::Named(s)
            }),
            Some(t) => Err(ParseError::UnexpectedToken(format!(
                "{t:?} (expected type)"
            ))),
            None => Err(ParseError::UnexpectedEof),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_let_if_return()
    }

    fn parse_let_if_return(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Some(Tok::Let) => {
                self.advance();
                let name = self.eat_ident()?;
                self.eat(&Tok::Eq)?;
                let value = self.parse_expr()?;
                self.eat(&Tok::Semi)?;
                let body = self.parse_expr()?;
                Ok(Expr::Let {
                    name,
                    value: Box::new(value),
                    body: Box::new(body),
                })
            }
            Some(Tok::If) => {
                self.advance();
                let cond = self.parse_expr()?;
                self.eat(&Tok::LBrace)?;
                let then = self.parse_expr()?;
                if self.peek() == Some(&Tok::Semi) {
                    self.advance();
                }
                self.eat(&Tok::RBrace)?;
                self.eat(&Tok::Else)?;
                self.eat(&Tok::LBrace)?;
                let els = self.parse_expr()?;
                if self.peek() == Some(&Tok::Semi) {
                    self.advance();
                }
                self.eat(&Tok::RBrace)?;
                Ok(Expr::If {
                    cond: Box::new(cond),
                    then: Box::new(then),
                    els: Box::new(els),
                })
            }
            Some(Tok::Return) => {
                self.advance();
                let e = self.parse_expr()?;
                if self.peek() == Some(&Tok::Semi) {
                    self.advance();
                }
                Ok(Expr::Return(Box::new(e)))
            }
            _ => self.parse_or(),
        }
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut a = self.parse_and()?;
        while self.peek() == Some(&Tok::Or) {
            self.advance();
            let b = self.parse_and()?;
            a = Expr::Bin {
                op: BinOp::Or,
                a: Box::new(a),
                b: Box::new(b),
            };
        }
        Ok(a)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut a = self.parse_cmp()?;
        while self.peek() == Some(&Tok::And) {
            self.advance();
            let b = self.parse_cmp()?;
            a = Expr::Bin {
                op: BinOp::And,
                a: Box::new(a),
                b: Box::new(b),
            };
        }
        Ok(a)
    }

    fn parse_cmp(&mut self) -> Result<Expr, ParseError> {
        let a = self.parse_add()?;
        let op = match self.peek() {
            Some(Tok::Lt) => BinOp::Lt,
            Some(Tok::Gt) => BinOp::Gt,
            Some(Tok::Le) => BinOp::Le,
            Some(Tok::Ge) => BinOp::Ge,
            Some(Tok::EqEq) => BinOp::Eq,
            Some(Tok::Ne) => BinOp::Ne,
            _ => return Ok(a),
        };
        self.advance();
        let b = self.parse_add()?;
        Ok(Expr::Bin {
            op,
            a: Box::new(a),
            b: Box::new(b),
        })
    }

    fn parse_add(&mut self) -> Result<Expr, ParseError> {
        let mut a = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => BinOp::Add,
                Some(Tok::Minus) => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let b = self.parse_mul()?;
            a = Expr::Bin {
                op,
                a: Box::new(a),
                b: Box::new(b),
            };
        }
        Ok(a)
    }

    fn parse_mul(&mut self) -> Result<Expr, ParseError> {
        let mut a = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Star) => BinOp::Mul,
                Some(Tok::Slash) => BinOp::Div,
                Some(Tok::Percent) => BinOp::Rem,
                _ => break,
            };
            self.advance();
            let b = self.parse_unary()?;
            a = Expr::Bin {
                op,
                a: Box::new(a),
                b: Box::new(b),
            };
        }
        Ok(a)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Some(Tok::Minus) => {
                self.advance();
                let a = self.parse_unary()?;
                Ok(Expr::Un {
                    op: UnOp::Neg,
                    a: Box::new(a),
                })
            }
            Some(Tok::Not) => {
                self.advance();
                let a = self.parse_unary()?;
                Ok(Expr::Un {
                    op: UnOp::Not,
                    a: Box::new(a),
                })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut e = self.parse_primary()?;
        loop {
            match self.peek() {
                Some(Tok::Dot) => {
                    self.advance();
                    let name = self.eat_ident()?;
                    let mut args = Vec::new();
                    if self.peek() == Some(&Tok::LParen) {
                        self.advance();
                        if self.peek() != Some(&Tok::RParen) {
                            loop {
                                args.push(self.parse_expr()?);
                                if self.peek() == Some(&Tok::Comma) {
                                    self.advance();
                                    continue;
                                }
                                break;
                            }
                        }
                        self.eat(&Tok::RParen)?;
                    }
                    e = Expr::Method {
                        recv: Box::new(e),
                        name,
                        args,
                    };
                }
                Some(Tok::LParen) => {
                    self.advance();
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.peek() == Some(&Tok::Comma) {
                                self.advance();
                                continue;
                            }
                            break;
                        }
                    }
                    self.eat(&Tok::RParen)?;
                    e = Expr::Call {
                        func: match e {
                            Expr::Var(f) => f,
                            _ => {
                                return Err(ParseError::Message(
                                    "call target must be a name".into(),
                                ))
                            }
                        },
                        args,
                    };
                }
                Some(Tok::As) => {
                    self.advance();
                    let ty = self.parse_type()?;
                    e = Expr::Cast {
                        value: Box::new(e),
                        ty: Box::new(ty),
                    };
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.advance() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Ident(s)) if s == "true" => Ok(Expr::Bool(true)),
            Some(Tok::Ident(s)) if s == "false" => Ok(Expr::Bool(false)),
            Some(Tok::Ident(s)) => {
                if s == "Array" {
                    return Err(ParseError::Message(
                        "Array<T,N> used as a value is not supported".into(),
                    ));
                }
                Ok(Expr::Var(s))
            }
            Some(Tok::LBracket) => {
                let mut elems = Vec::new();
                if self.peek() != Some(&Tok::RBracket) {
                    loop {
                        elems.push(self.parse_expr()?);
                        if self.peek() == Some(&Tok::Comma) {
                            self.advance();
                            continue;
                        }
                        break;
                    }
                }
                self.eat(&Tok::RBracket)?;
                Ok(Expr::ArrayLit(elems))
            }
            Some(Tok::LBrace) => {
                let mut fields = Vec::new();
                if self.peek() != Some(&Tok::RBrace) {
                    loop {
                        let fname = self.eat_ident()?;
                        self.eat(&Tok::Colon)?;
                        let fval = self.parse_expr()?;
                        fields.push((fname, fval));
                        if self.peek() == Some(&Tok::Comma) {
                            self.advance();
                            continue;
                        }
                        break;
                    }
                }
                self.eat(&Tok::RBrace)?;
                Ok(Expr::Record(fields))
            }
            Some(Tok::LParen) => {
                let e = self.parse_expr()?;
                self.eat(&Tok::RParen)?;
                Ok(e)
            }
            Some(Tok::Pipe) => {
                let mut params = Vec::new();
                if self.peek() != Some(&Tok::Pipe) {
                    loop {
                        if self.peek() == Some(&Tok::LParen) {
                            self.advance();
                            let mut inner = Vec::new();
                            if self.peek() != Some(&Tok::RParen) {
                                loop {
                                    inner.push(self.eat_ident()?);
                                    if self.peek() == Some(&Tok::Comma) {
                                        self.advance();
                                        continue;
                                    }
                                    break;
                                }
                            }
                            self.eat(&Tok::RParen)?;
                            params.extend(inner);
                        } else {
                            params.push(self.eat_ident()?);
                        }
                        if self.peek() == Some(&Tok::Comma) {
                            self.advance();
                            continue;
                        }
                        break;
                    }
                }
                self.eat(&Tok::Pipe)?;
                let body = self.parse_expr()?;
                Ok(Expr::Lambda {
                    params,
                    body: Box::new(body),
                })
            }
            Some(t) => Err(ParseError::UnexpectedToken(format!(
                "{t:?} (expected primary)"
            ))),
            None => Err(ParseError::UnexpectedEof),
        }
    }
}

/// Parse tpt-eidos source into a `Module`.
pub fn parse(source: &str) -> Result<Module, ParseError> {
    let toks = Lexer::run(source)?;
    let mut p = Parser::new(toks);
    let m = p.parse_module()?;
    if p.peek().is_some() {
        return Err(ParseError::UnexpectedToken(format!("{:?}", p.peek())));
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_array_type() {
        let m = parse("type T = Array<f64, 3>;").unwrap();
        assert_eq!(
            m.items,
            vec![Item::TypeAlias {
                name: "T".into(),
                ty: Type::Array(Box::new(Type::Base("f64".into())), 3),
            }]
        );
    }

    #[test]
    fn parse_refine() {
        let m = parse("type P = { x: f64 | x > 0.0 };").unwrap();
        match &m.items[0] {
            Item::TypeAlias {
                ty: Type::Refine { bind, .. },
                ..
            } => assert_eq!(bind, "x"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_fn_with_division() {
        let src = "fn f(a: f64) -> f64 requires a > 0.0 { return a / a; }";
        let m = parse(src).unwrap();
        assert!(matches!(m.items[0], Item::Fn(_)));
    }
}
