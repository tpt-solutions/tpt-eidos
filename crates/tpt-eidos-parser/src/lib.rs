//! Lexer, recursive-descent parser, and error type for the tpt-eidos MVK
//! surface language. Pure `std`; no external crates.

mod ast;
pub use ast::*;

#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub span: Option<Span>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ParseErrorKind {
    UnexpectedEof,
    UnexpectedToken(String),
    InvalidNumber(String),
    Message(String),
}

impl ParseError {
    pub fn unexpected_eof(span: Option<Span>) -> Self {
        ParseError {
            kind: ParseErrorKind::UnexpectedEof,
            span,
        }
    }
    pub fn unexpected_token(msg: impl Into<String>, span: Option<Span>) -> Self {
        ParseError {
            kind: ParseErrorKind::UnexpectedToken(msg.into()),
            span,
        }
    }
    pub fn invalid_number(msg: impl Into<String>, span: Option<Span>) -> Self {
        ParseError {
            kind: ParseErrorKind::InvalidNumber(msg.into()),
            span,
        }
    }
    pub fn message(msg: impl Into<String>, span: Option<Span>) -> Self {
        ParseError {
            kind: ParseErrorKind::Message(msg.into()),
            span,
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            ParseErrorKind::UnexpectedEof => write!(f, "unexpected end of input"),
            ParseErrorKind::UnexpectedToken(s) => write!(f, "unexpected token: {s}"),
            ParseErrorKind::InvalidNumber(s) => write!(f, "invalid number literal: {s}"),
            ParseErrorKind::Message(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Lexical tokens, each paired with its byte offset in the source.
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
    Linear,
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
    DocComment(String),
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
        "linear" => Tok::Linear,
        _ => return None,
    })
}

/// A token paired with its byte offset in the source text.
#[derive(Clone, Debug, PartialEq)]
struct SpannedTok {
    tok: Tok,
    pos: usize,
}

struct Lexer;

impl Lexer {
    fn run(src: &str) -> Result<Vec<SpannedTok>, ParseError> {
        let chars: Vec<char> = src.chars().collect();
        let mut byte_pos = 0;
        let mut char_idx = 0;
        let mut toks = Vec::new();
        while char_idx < chars.len() {
            let c = chars[char_idx];
            if c.is_whitespace() {
                byte_pos += c.len_utf8();
                char_idx += 1;
                continue;
            }
            if c == '/' && char_idx + 1 < chars.len() && chars[char_idx + 1] == '/' {
                let is_doc = char_idx + 2 < chars.len()
                    && chars[char_idx + 2] == '/'
                    && !(char_idx + 3 < chars.len() && chars[char_idx + 3] == '/');
                let start = byte_pos;
                // skip past `//` (or `///`)
                let prefix_len = if is_doc { 3 } else { 2 };
                for _ in 0..prefix_len {
                    byte_pos += chars[char_idx].len_utf8();
                    char_idx += 1;
                }
                // skip one leading space after `///` if present
                if is_doc && char_idx < chars.len() && chars[char_idx] == ' ' {
                    byte_pos += 1;
                    char_idx += 1;
                }
                let text_start_idx = char_idx;
                while char_idx < chars.len() && chars[char_idx] != '\n' {
                    byte_pos += chars[char_idx].len_utf8();
                    char_idx += 1;
                }
                if is_doc {
                    let text: String = chars[text_start_idx..char_idx].iter().collect();
                    let pos = start;
                    toks.push(SpannedTok {
                        tok: Tok::DocComment(text),
                        pos,
                    });
                }
                continue;
            }
            if c.is_ascii_digit()
                || (c == '.' && char_idx + 1 < chars.len() && chars[char_idx + 1].is_ascii_digit())
            {
                let start = byte_pos;
                while char_idx < chars.len()
                    && (chars[char_idx].is_ascii_digit() || chars[char_idx] == '.')
                {
                    byte_pos += chars[char_idx].len_utf8();
                    char_idx += 1;
                }
                let s: String = chars[(char_idx - (byte_pos - start))..char_idx]
                    .iter()
                    .collect();
                let v: f64 = s.parse().map_err(|_| {
                    ParseError::invalid_number(
                        s.clone(),
                        Some(Span {
                            lo: start,
                            hi: byte_pos,
                        }),
                    )
                })?;
                toks.push(SpannedTok {
                    tok: Tok::Num(v),
                    pos: start,
                });
                continue;
            }
            if c.is_alphabetic() || c == '_' {
                let start = byte_pos;
                while char_idx < chars.len()
                    && (chars[char_idx].is_alphanumeric() || chars[char_idx] == '_')
                {
                    byte_pos += chars[char_idx].len_utf8();
                    char_idx += 1;
                }
                let s: String = chars[(char_idx - (byte_pos - start))..char_idx]
                    .iter()
                    .collect();
                let tok = keyword(&s).unwrap_or(Tok::Ident(s));
                toks.push(SpannedTok { tok, pos: start });
                continue;
            }
            let start = byte_pos;
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
                '-' if char_idx + 1 < chars.len() && chars[char_idx + 1] == '>' => (Tok::Arrow, 2),
                '!' if char_idx + 1 < chars.len() && chars[char_idx + 1] == '=' => (Tok::Ne, 2),
                '!' => (Tok::Not, 1),
                '-' => (Tok::Minus, 1),
                '*' => (Tok::Star, 1),
                '/' => (Tok::Slash, 1),
                '%' => (Tok::Percent, 1),
                '=' if char_idx + 1 < chars.len() && chars[char_idx + 1] == '=' => (Tok::EqEq, 2),
                '=' => (Tok::Eq, 1),
                '<' if char_idx + 1 < chars.len() && chars[char_idx + 1] == '=' => (Tok::Le, 2),
                '<' => (Tok::Lt, 1),
                '>' if char_idx + 1 < chars.len() && chars[char_idx + 1] == '=' => (Tok::Ge, 2),
                '>' => (Tok::Gt, 1),
                '&' if char_idx + 1 < chars.len() && chars[char_idx + 1] == '&' => (Tok::And, 2),
                '&' => {
                    return Err(ParseError::unexpected_token(
                        "&",
                        Some(Span {
                            lo: start,
                            hi: start + 1,
                        }),
                    ))
                }
                _ => {
                    return Err(ParseError::unexpected_token(
                        c.to_string(),
                        Some(Span {
                            lo: start,
                            hi: start + c.len_utf8(),
                        }),
                    ))
                }
            };
            byte_pos += consumed;
            char_idx += consumed;
            toks.push(SpannedTok { tok: t, pos: start });
        }
        Ok(toks)
    }
}

struct Parser {
    toks: Vec<SpannedTok>,
    pos: usize,
    depth: usize,
}

const MAX_PARSE_DEPTH: usize = 64;

impl Parser {
    fn new(toks: Vec<SpannedTok>) -> Self {
        Parser {
            toks,
            pos: 0,
            depth: 0,
        }
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|st| &st.tok)
    }

    fn peek2(&self) -> Option<&Tok> {
        self.toks.get(self.pos + 1).map(|st| &st.tok)
    }

    fn tok_pos(&self) -> usize {
        self.toks.get(self.pos).map_or(0, |st| st.pos)
    }

    fn advance(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).map(|st| st.tok.clone());
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn advance_pos(&mut self) -> (Tok, usize) {
        let st = self.toks.get(self.pos).cloned();
        if let Some(st) = st {
            self.pos += 1;
            (st.tok, st.pos)
        } else {
            (Tok::Fn, 0)
        }
    }

    fn eat(&mut self, t: &Tok) -> Result<(), ParseError> {
        match self.peek() {
            Some(x) if x == t => {
                self.pos += 1;
                Ok(())
            }
            Some(x) => {
                let pos = self.tok_pos();
                Err(ParseError::unexpected_token(
                    format!("{x:?} (expected {t:?})"),
                    Some(Span {
                        lo: pos,
                        hi: pos + 1,
                    }),
                ))
            }
            None => Err(ParseError::unexpected_eof(None)),
        }
    }

    fn eat_ident(&mut self) -> Result<String, ParseError> {
        match self.advance() {
            Some(Tok::Ident(s)) => Ok(s),
            Some(t) => {
                let pos = self.tok_pos();
                Err(ParseError::unexpected_token(
                    format!("{t:?} (expected identifier)"),
                    Some(Span {
                        lo: pos,
                        hi: pos + 1,
                    }),
                ))
            }
            None => Err(ParseError::unexpected_eof(None)),
        }
    }

    fn wrap(&self, kind: ExprKind, start: usize) -> Expr {
        Expr {
            kind,
            span: Span {
                lo: start,
                hi: self.tok_pos(),
            },
        }
    }

    fn parse_module(&mut self) -> Result<Module, ParseError> {
        let mut items = Vec::new();
        while self.peek().is_some() {
            let mut doc_lines: Vec<String> = Vec::new();
            while matches!(self.peek(), Some(Tok::DocComment(_))) {
                if let Some(Tok::DocComment(s)) = self.advance() {
                    doc_lines.push(s);
                }
            }
            let doc = if doc_lines.is_empty() {
                None
            } else {
                Some(doc_lines.join("\n"))
            };
            items.push(self.parse_item(doc)?);
        }
        Ok(Module { items })
    }

    fn parse_item(&mut self, doc: Option<String>) -> Result<Item, ParseError> {
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
                    let kw_pos = self.tok_pos();
                    self.advance();
                    self.eat(&Tok::Pipe)?;
                    let b = self.eat_ident()?;
                    self.eat(&Tok::Pipe)?;
                    let body = self.parse_expr()?;
                    ensures = Some(Expr {
                        kind: ExprKind::Lambda {
                            params: vec![Pattern::Var(b)],
                            body: Box::new(body),
                        },
                        span: Span {
                            lo: kw_pos,
                            hi: self.tok_pos(),
                        },
                    });
                }
                if self.peek() == Some(&Tok::Effects) {
                    self.advance();
                    self.eat(&Tok::LBracket)?;
                    let mut effs = Vec::new();
                    if self.peek() != Some(&Tok::RBracket) {
                        loop {
                            let name = self.eat_ident()?;
                            let budget = if self.peek() == Some(&Tok::Lt) {
                                self.advance();
                                let val = match self.advance() {
                                    Some(Tok::Num(n)) => n,
                                    Some(t) => {
                                        let pos = self.tok_pos();
                                        return Err(ParseError::unexpected_token(
                                            format!("{t:?} (expected number)"),
                                            Some(Span {
                                                lo: pos,
                                                hi: pos + 1,
                                            }),
                                        ));
                                    }
                                    None => return Err(ParseError::unexpected_eof(None)),
                                };
                                let unit_name = self.eat_ident()?;
                                let unit = match unit_name.as_str() {
                                    "us" => ast::TimeUnit::Us,
                                    "ms" => ast::TimeUnit::Ms,
                                    "s" => ast::TimeUnit::S,
                                    _ => {
                                        return Err(ParseError::message(
                                            format!(
                                                "unknown time unit: {unit_name} (expected us, ms, or s)"
                                            ),
                                            None,
                                        ))
                                    }
                                };
                                self.eat(&Tok::Gt)?;
                                Some((val, unit))
                            } else {
                                None
                            };
                            effs.push(ast::Effect { name, budget });
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
                    doc,
                })))
            }
            _ => {
                let pos = self.tok_pos();
                Err(ParseError::unexpected_token(
                    format!("{:?} (expected item)", self.peek()),
                    Some(Span {
                        lo: pos,
                        hi: pos + 1,
                    }),
                ))
            }
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
            let start = self.tok_pos();
            self.advance();
            self.advance();
            let inner = self.parse_type()?;
            self.eat(&Tok::Comma)?;
            let n = match self.advance() {
                Some(Tok::Num(n)) => {
                    if !n.is_finite() || n.fract() != 0.0 || n < 0.0 || n > u64::MAX as f64 {
                        return Err(ParseError::message(
                            "Array length must be a non-negative integer in range",
                            Some(Span {
                                lo: start,
                                hi: self.tok_pos(),
                            }),
                        ));
                    }
                    n as u64
                }
                Some(t) => {
                    let pos = self.tok_pos();
                    return Err(ParseError::unexpected_token(
                        format!("{t:?} (expected length)"),
                        Some(Span {
                            lo: pos,
                            hi: pos + 1,
                        }),
                    ));
                }
                None => return Err(ParseError::unexpected_eof(None)),
            };
            self.eat(&Tok::Gt)?;
            return Ok(Type::Array(Box::new(inner), n));
        }
        if self.peek() == Some(&Tok::Linear) {
            self.advance();
            let inner = self.parse_type()?;
            return Ok(Type::Linear(Box::new(inner)));
        }
        match self.advance() {
            Some(Tok::Ident(s)) => Ok(if is_base_type(&s) {
                Type::Base(s)
            } else {
                Type::Named(s)
            }),
            Some(t) => {
                let pos = self.tok_pos();
                Err(ParseError::unexpected_token(
                    format!("{t:?} (expected type)"),
                    Some(Span {
                        lo: pos,
                        hi: pos + 1,
                    }),
                ))
            }
            None => Err(ParseError::unexpected_eof(None)),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.depth += 1;
        if self.depth > MAX_PARSE_DEPTH {
            self.depth -= 1;
            let pos = self.tok_pos();
            return Err(ParseError::message(
                "maximum parse depth exceeded",
                Some(Span {
                    lo: pos,
                    hi: pos + 1,
                }),
            ));
        }
        let start = self.tok_pos();
        let r = self.parse_let_if_return();
        self.depth -= 1;
        r.map(|e| Expr {
            kind: e.kind,
            span: Span {
                lo: start,
                hi: self.tok_pos(),
            },
        })
    }

    fn parse_let_if_return(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Some(Tok::Let) => {
                let start = self.tok_pos();
                self.advance();
                let name = self.eat_ident()?;
                self.eat(&Tok::Eq)?;
                let value = self.parse_expr()?;
                self.eat(&Tok::Semi)?;
                let body = self.parse_expr()?;
                Ok(self.wrap(
                    ExprKind::Let {
                        name,
                        value: Box::new(value),
                        body: Box::new(body),
                    },
                    start,
                ))
            }
            Some(Tok::If) => {
                let start = self.tok_pos();
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
                Ok(self.wrap(
                    ExprKind::If {
                        cond: Box::new(cond),
                        then: Box::new(then),
                        els: Box::new(els),
                    },
                    start,
                ))
            }
            Some(Tok::Return) => {
                let start = self.tok_pos();
                self.advance();
                let e = self.parse_expr()?;
                if self.peek() == Some(&Tok::Semi) {
                    self.advance();
                }
                Ok(self.wrap(ExprKind::Return(Box::new(e)), start))
            }
            _ => self.parse_or(),
        }
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut a = self.parse_and()?;
        while self.peek() == Some(&Tok::Or) {
            let start = a.span.lo;
            self.advance();
            let b = self.parse_and()?;
            a = self.wrap(
                ExprKind::Bin {
                    op: BinOp::Or,
                    a: Box::new(a),
                    b: Box::new(b),
                },
                start,
            );
        }
        Ok(a)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut a = self.parse_cmp()?;
        while self.peek() == Some(&Tok::And) {
            let start = a.span.lo;
            self.advance();
            let b = self.parse_cmp()?;
            a = self.wrap(
                ExprKind::Bin {
                    op: BinOp::And,
                    a: Box::new(a),
                    b: Box::new(b),
                },
                start,
            );
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
        let start = a.span.lo;
        self.advance();
        let b = self.parse_add()?;
        Ok(self.wrap(
            ExprKind::Bin {
                op,
                a: Box::new(a),
                b: Box::new(b),
            },
            start,
        ))
    }

    fn parse_add(&mut self) -> Result<Expr, ParseError> {
        let mut a = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => BinOp::Add,
                Some(Tok::Minus) => BinOp::Sub,
                _ => break,
            };
            let start = a.span.lo;
            self.advance();
            let b = self.parse_mul()?;
            a = self.wrap(
                ExprKind::Bin {
                    op,
                    a: Box::new(a),
                    b: Box::new(b),
                },
                start,
            );
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
            let start = a.span.lo;
            self.advance();
            let b = self.parse_unary()?;
            a = self.wrap(
                ExprKind::Bin {
                    op,
                    a: Box::new(a),
                    b: Box::new(b),
                },
                start,
            );
        }
        Ok(a)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Some(Tok::Minus) => {
                let start = self.tok_pos();
                self.advance();
                let a = self.parse_unary()?;
                Ok(self.wrap(
                    ExprKind::Un {
                        op: UnOp::Neg,
                        a: Box::new(a),
                    },
                    start,
                ))
            }
            Some(Tok::Not) => {
                let start = self.tok_pos();
                self.advance();
                let a = self.parse_unary()?;
                Ok(self.wrap(
                    ExprKind::Un {
                        op: UnOp::Not,
                        a: Box::new(a),
                    },
                    start,
                ))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut e = self.parse_primary()?;
        loop {
            match self.peek() {
                Some(Tok::Dot) => {
                    let start = e.span.lo;
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
                    e = self.wrap(
                        ExprKind::Method {
                            recv: Box::new(e),
                            name,
                            args,
                        },
                        start,
                    );
                }
                Some(Tok::LParen) => {
                    let start = e.span.lo;
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
                    let func = match &e.kind {
                        ExprKind::Var(f) => f.clone(),
                        _ => {
                            return Err(ParseError::message(
                                "call target must be a name",
                                Some(e.span),
                            ))
                        }
                    };
                    e = self.wrap(ExprKind::Call { func, args }, start);
                }
                Some(Tok::As) => {
                    let start = e.span.lo;
                    self.advance();
                    let ty = self.parse_type()?;
                    e = self.wrap(
                        ExprKind::Cast {
                            value: Box::new(e),
                            ty: Box::new(ty),
                        },
                        start,
                    );
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let (tok, start) = self.advance_pos();
        match tok {
            Tok::Num(n) => Ok(self.wrap(ExprKind::Num(n), start)),
            Tok::Ident(s) if s == "true" => Ok(self.wrap(ExprKind::Bool(true), start)),
            Tok::Ident(s) if s == "false" => Ok(self.wrap(ExprKind::Bool(false), start)),
            Tok::Ident(s) => {
                if s == "Array" {
                    return Err(ParseError::message(
                        "Array<T,N> used as a value is not supported",
                        Some(Span {
                            lo: start,
                            hi: start + s.len(),
                        }),
                    ));
                }
                Ok(self.wrap(ExprKind::Var(s), start))
            }
            Tok::LBracket => {
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
                Ok(self.wrap(ExprKind::ArrayLit(elems), start))
            }
            Tok::LBrace => {
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
                Ok(self.wrap(ExprKind::Record(fields), start))
            }
            Tok::LParen => {
                let e = self.parse_expr()?;
                self.eat(&Tok::RParen)?;
                Ok(e)
            }
            Tok::Pipe => {
                let mut params = Vec::new();
                if self.peek() != Some(&Tok::Pipe) {
                    loop {
                        if self.peek() == Some(&Tok::LParen) {
                            params.push(self.parse_param_pattern()?);
                        } else {
                            params.push(Pattern::Var(self.eat_ident()?));
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
                Ok(self.wrap(
                    ExprKind::Lambda {
                        params,
                        body: Box::new(body),
                    },
                    start,
                ))
            }
            t => {
                let pos = self.tok_pos();
                Err(ParseError::unexpected_token(
                    format!("{t:?} (expected primary)"),
                    Some(Span {
                        lo: pos,
                        hi: pos + 1,
                    }),
                ))
            }
        }
    }

    fn parse_param_pattern(&mut self) -> Result<Pattern, ParseError> {
        if self.peek() == Some(&Tok::LParen) {
            self.advance();
            let mut inner = Vec::new();
            if self.peek() != Some(&Tok::RParen) {
                loop {
                    inner.push(self.parse_param_pattern()?);
                    if self.peek() == Some(&Tok::Comma) {
                        self.advance();
                        continue;
                    }
                    break;
                }
            }
            self.eat(&Tok::RParen)?;
            Ok(Pattern::Tuple(inner))
        } else {
            Ok(Pattern::Var(self.eat_ident()?))
        }
    }
}

/// Parse tpt-eidos source into a `Module`.
pub fn parse(source: &str) -> Result<Module, ParseError> {
    let toks = Lexer::run(source)?;
    let mut p = Parser::new(toks);
    let m = p.parse_module()?;
    if p.peek().is_some() {
        let pos = p.tok_pos();
        return Err(ParseError::unexpected_token(
            format!("{:?}", p.peek()),
            Some(Span {
                lo: pos,
                hi: pos + 1,
            }),
        ));
    }
    Ok(m)
}

/// Parse a single expression. Wraps the source in a trivial function, parses
/// the module, and extracts the return expression.
pub fn parse_expr(source: &str) -> Result<Expr, ParseError> {
    let m = parse(&format!("fn _() -> f64 {{ return {source}; }}"))?;
    if let Item::Fn(f) = &m.items[0] {
        if let ExprKind::Return(e) = &f.body.kind {
            return Ok((**e).clone());
        }
    }
    Err(ParseError::message("internal: parse_expr failed", None))
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

    #[test]
    fn error_unexpected_eof() {
        let e = parse("fn f(a: f64").unwrap_err();
        assert!(matches!(e.kind, ParseErrorKind::UnexpectedEof));
        assert!(e.to_string().contains("end of input"));
    }

    #[test]
    fn error_unexpected_token() {
        let e = parse("fn f() -> f64 { return 1 + * 2; }").unwrap_err();
        assert!(matches!(e.kind, ParseErrorKind::UnexpectedToken(_)));
        assert!(e.to_string().contains("unexpected token"));
    }

    #[test]
    fn error_invalid_number() {
        let e = parse("fn f() -> f64 { return 1.2.3; }").unwrap_err();
        assert!(matches!(e.kind, ParseErrorKind::InvalidNumber(ref s) if s == "1.2.3"));
        assert!(e.to_string().contains("invalid number literal"));
    }

    #[test]
    fn error_array_length_out_of_range() {
        let e = parse("type T = Array<f64, 200000000000000000000>;").unwrap_err();
        assert!(e.to_string().contains("Array length"), "got: {e}");
    }

    #[test]
    fn precedence_mul_over_add() {
        let m = parse("fn f() -> f64 { return 1.0 + 2.0 * 3.0; }").unwrap();
        if let Item::Fn(f) = &m.items[0] {
            if let ExprKind::Return(e) = &f.body.kind {
                if let ExprKind::Bin { op, b, .. } = &e.kind {
                    assert_eq!(*op, BinOp::Add);
                    assert!(matches!(&b.kind, ExprKind::Bin { op: BinOp::Mul, .. }));
                    return;
                }
            }
        }
        panic!("expected (1 + (2 * 3))");
    }

    #[test]
    fn precedence_not_over_eq() {
        let m = parse("fn f() -> bool { return ! a == b; }").unwrap();
        if let Item::Fn(f) = &m.items[0] {
            if let ExprKind::Return(e) = &f.body.kind {
                if let ExprKind::Bin { op, a, .. } = &e.kind {
                    assert_eq!(*op, BinOp::Eq);
                    assert!(matches!(&a.kind, ExprKind::Un { op: UnOp::Not, .. }));
                    return;
                }
            }
        }
        panic!("expected ((!a) == b)");
    }

    #[test]
    fn add_is_left_associative() {
        let m = parse("fn f() -> f64 { return 1.0 - 2.0 - 3.0; }").unwrap();
        if let Item::Fn(f) = &m.items[0] {
            if let ExprKind::Return(e) = &f.body.kind {
                if let ExprKind::Bin { op, a, .. } = &e.kind {
                    assert_eq!(*op, BinOp::Sub);
                    assert!(matches!(&a.kind, ExprKind::Bin { op: BinOp::Sub, .. }));
                    return;
                }
            }
        }
        panic!("expected ((1 - 2) - 3)");
    }

    #[test]
    fn parses_lambda_with_tuple_pattern() {
        let e = parse_expr("|(a, b)| a + b").unwrap();
        match &e.kind {
            ExprKind::Lambda { params, .. } => {
                assert_eq!(
                    params,
                    &vec![Pattern::Tuple(vec![
                        Pattern::Var("a".into()),
                        Pattern::Var("b".into())
                    ])]
                );
            }
            other => panic!("expected lambda, got {other:?}"),
        }
    }

    #[test]
    fn parses_nested_tuple_param_pattern() {
        let e = parse_expr("|(a, (b, c))| a + b + c").unwrap();
        match &e.kind {
            ExprKind::Lambda { params, .. } => assert_eq!(
                params,
                &vec![Pattern::Tuple(vec![
                    Pattern::Var("a".into()),
                    Pattern::Tuple(vec![Pattern::Var("b".into()), Pattern::Var("c".into())])
                ])]
            ),
            other => panic!("expected lambda, got {other:?}"),
        }
    }

    #[test]
    fn parses_effects_list() {
        let m = parse("fn f() -> f64 effects [Pure, IO] { return 1.0; }").unwrap();
        match &m.items[0] {
            Item::Fn(f) => {
                assert_eq!(f.effects.len(), 2);
                assert_eq!(f.effects[0].name, "Pure");
                assert_eq!(f.effects[0].budget, None);
                assert_eq!(f.effects[1].name, "IO");
                assert_eq!(f.effects[1].budget, None);
            }
            other => panic!("expected fn, got {other:?}"),
        }
    }

    #[test]
    fn parses_parameterized_effects() {
        let m = parse("fn f() -> f64 effects [RealTime<2ms>] { return 1.0; }").unwrap();
        match &m.items[0] {
            Item::Fn(f) => {
                assert_eq!(f.effects.len(), 1);
                assert_eq!(f.effects[0].name, "RealTime");
                assert_eq!(f.effects[0].budget, Some((2.0, ast::TimeUnit::Ms)));
            }
            other => panic!("expected fn, got {other:?}"),
        }
    }

    #[test]
    fn parse_expr_entry_point() {
        let e = parse_expr("1.0 + 2.0").unwrap();
        assert!(matches!(&e.kind, ExprKind::Bin { op: BinOp::Add, .. }));
    }

    #[test]
    fn parse_expr_rejects_non_expression() {
        assert!(parse_expr("fn f() -> f64 { return 1.0; }").is_err());
    }

    #[test]
    fn rejects_deeply_nested_expression() {
        let nested = format!("{}{}{}", "(".repeat(2000), "1.0", ")".repeat(2000));
        assert!(parse_expr(&nested).is_err());
    }

    #[test]
    fn parse_errors_carry_span_info() {
        let e = parse("fn f() -> f64 { return 1 + * 2; }").unwrap_err();
        assert!(e.span.is_some(), "parse errors should carry span info");
        let span = e.span.unwrap();
        assert!(span.lo > 0, "span should point into the source");
    }

    #[test]
    fn parse_error_eof_has_span() {
        let e = parse("fn f(a: f64").unwrap_err();
        // UnexpectedEof from eat() may not have a span since there's no token to point at
        // but the kind is correct
        assert!(matches!(e.kind, ParseErrorKind::UnexpectedEof));
    }

    #[test]
    fn spans_track_byte_offsets() {
        let e = parse_expr("1.0 + 2.0").unwrap();
        assert!(e.span.lo < e.span.hi, "span must have positive width");
        match &e.kind {
            ExprKind::Bin { a, b, .. } => {
                assert!(a.span.lo < a.span.hi, "a must have positive width");
                assert!(b.span.lo < b.span.hi, "b must have positive width");
                assert_eq!(
                    a.span.lo, e.span.lo,
                    "lhs should start at same position as full expression"
                );
                assert!(b.span.lo > a.span.hi, "rhs should start after lhs ends");
            }
            other => panic!("expected bin, got {other:?}"),
        }
    }
}
