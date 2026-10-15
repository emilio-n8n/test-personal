//! Parameters: named numbers a feature reads, and the expressions that compute them.
//!
//! A parameter is a leaf. An expression reads parameters — and other expressions — so a dimension
//! can be `width` while `width` is driven by something else. The evaluation is a DFS with a cycle
//! check, so `a = b + 1` and `b = a` is a reported error rather than a stack overflow.
//!
//! Expressions are deliberately small: numbers, `+ - * / ^`, parentheses, unary minus, and named
//! parameters. There is no scripting language here on purpose — a modelling document is meant to be
//! readable and to fail in a way that can be explained, and every feature it does not have is a
//! thing that cannot go wrong in it.

use std::collections::BTreeMap;

use crate::error::ModelError;

/// A parsed arithmetic expression over named parameters.
///
/// The tree is kept, not flattened to a number, because a parameter's *value* changes on every edit
/// while its *shape* does not: re-evaluating must not re-parse, and a parse error must survive
/// until the next time the value is asked for rather than losing the text that caused it.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Number(f64),
    Name(String),
    Neg(Box<Expr>),
    Bin { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

impl BinOp {
    fn apply(self, a: f64, b: f64) -> Option<f64> {
        let v = match self {
            BinOp::Add => a + b,
            BinOp::Sub => a - b,
            BinOp::Mul => a * b,
            BinOp::Div => a / b,
            BinOp::Pow => a.powf(b),
        };
        v.is_finite().then_some(v)
    }

    /// What this operator is called in the UI and in errors.
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Pow => "^",
        }
    }
}

/// Every name an expression mentions, in the order first seen.
pub fn names_of(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Number(_) => {}
        Expr::Name(n) => {
            if !out.contains(n) {
                out.push(n.clone());
            }
        }
        Expr::Neg(e) => names_of(e, out),
        Expr::Bin { lhs, rhs, .. } => {
            names_of(lhs, out);
            names_of(rhs, out);
        }
    }
}

/// Parse an expression. Returns the reason in full, because the text is all the user has to go on.
pub fn parse(text: &str) -> Result<Expr, ModelError> {
    let mut p = Parser { chars: text.chars().collect(), at: 0 };
    let e = p.expression()?;
    p.skip_spaces();
    if p.at < p.chars.len() {
        return Err(ModelError::Expression(format!("unexpected {:?} in {text:?}", p.chars[p.at])));
    }
    Ok(e)
}

struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn skip_spaces(&mut self) {
        while self.at < self.chars.len() && self.chars[self.at].is_whitespace() {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_spaces();
        self.chars.get(self.at).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expression(&mut self) -> Result<Expr, ModelError> {
        let mut lhs = self.term()?;
        loop {
            let op = match self.peek() {
                Some('+') => BinOp::Add,
                Some('-') => BinOp::Sub,
                _ => return Ok(lhs),
            };
            self.at += 1;
            let rhs = self.term()?;
            lhs = Expr::Bin { op, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
    }

    fn term(&mut self) -> Result<Expr, ModelError> {
        let mut lhs = self.power()?;
        loop {
            let op = match self.peek() {
                Some('*') => BinOp::Mul,
                Some('/') => BinOp::Div,
                _ => return Ok(lhs),
            };
            self.at += 1;
            let rhs = self.power()?;
            lhs = Expr::Bin { op, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
    }

    fn power(&mut self) -> Result<Expr, ModelError> {
        let base = self.unary()?;
        if self.eat('^') {
            // Right-associative, so `2^3^2` is 2^(3^2) as every calculator agrees.
            let exponent = self.power()?;
            return Ok(Expr::Bin { op: BinOp::Pow, lhs: Box::new(base), rhs: Box::new(exponent) });
        }
        Ok(base)
    }

    fn unary(&mut self) -> Result<Expr, ModelError> {
        if self.eat('-') {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat('+') {
            return self.unary();
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<Expr, ModelError> {
        self.skip_spaces();
        let Some(c) = self.peek() else {
            return Err(ModelError::Expression("the expression ends too early".into()));
        };
        if c == '(' {
            self.at += 1;
            let e = self.expression()?;
            if !self.eat(')') {
                return Err(ModelError::Expression("a ( is never closed".into()));
            }
            return Ok(e);
        }
        if c.is_ascii_digit() || c == '.' {
            let start = self.at;
            while self.at < self.chars.len() && (self.chars[self.at].is_ascii_digit() || self.chars[self.at] == '.') {
                self.at += 1;
            }
            // `2e-3` and `1.5E3`, but not `2e` or `2em`.
            if self.at < self.chars.len() && (self.chars[self.at] == 'e' || self.chars[self.at] == 'E') {
                let save = self.at;
                self.at += 1;
                if self.eat('-') || self.chars.get(self.at).is_some_and(char::is_ascii_digit) {
                    while self.at < self.chars.len() && self.chars[self.at].is_ascii_digit() {
                        self.at += 1;
                    }
                } else {
                    self.at = save;
                }
            }
            let text: String = self.chars[start..self.at].iter().collect();
            return text.parse::<f64>().map(Expr::Number).map_err(|_| ModelError::Expression(format!("{text:?} is not a number")));
        }
        if c.is_alphabetic() || c == '_' {
            let start = self.at;
            while self.at < self.chars.len() && (self.chars[self.at].is_alphanumeric() || self.chars[self.at] == '_') {
                self.at += 1;
            }
            return Ok(Expr::Name(self.chars[start..self.at].iter().collect()));
        }
        Err(ModelError::Expression(format!("{c:?} is not something an expression can use")))
    }
}

/// Evaluate an expression against a table of already-known values.
pub fn evaluate(expr: &Expr, values: &BTreeMap<String, f64>) -> Result<f64, ModelError> {
    match expr {
        Expr::Number(n) => Ok(*n),
        Expr::Name(n) => values.get(n).copied().ok_or_else(|| ModelError::Expression(format!("{n:?} has no value yet"))),
        Expr::Neg(e) => Ok(-evaluate(e, values)?),
        Expr::Bin { op, lhs, rhs } => {
            let (a, b) = (evaluate(lhs, values)?, evaluate(rhs, values)?);
            op.apply(a, b).ok_or_else(|| ModelError::Expression(format!("{} gives {a} {} {b}, which is not a number", op.symbol(), op.symbol())))
        }
    }
}
