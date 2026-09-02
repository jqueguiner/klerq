//! klerq-calc — the spreadsheet engine (MS Excel analog).
//!
//! A [`Sheet`] holds cells addressed in A1 notation. A cell is a literal
//! (number / text) or a `=formula`. Formulas support:
//! - arithmetic `+ - * /`, parentheses, unary minus
//! - cell references (`A1`) and ranges (`A1:B3`)
//! - functions `SUM`, `AVERAGE`, `MIN`, `MAX`, `COUNT`
//!
//! Evaluation is recursive with **cycle detection**, so `A1=B1`, `B1=A1`
//! yields a [`CalcError::Cycle`] instead of hanging.
//!
//! Built TDD-first — see the `tests` module (written before the engine).

mod parser;

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use parser::{parse, Expr};

/// A cell's stored content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Cell {
    Empty,
    Number(f64),
    Text(String),
    Formula(String),
}

/// Zero-based cell address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Addr {
    pub col: u32,
    pub row: u32,
}

#[derive(Debug, Error, PartialEq)]
pub enum CalcError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("circular reference at {0}")]
    Cycle(String),
    #[error("#VALUE! ({0})")]
    Value(String),
    #[error("unknown function: {0}")]
    UnknownFn(String),
}

/// Parse an A1 reference like "AB12" into a zero-based [`Addr`].
pub fn parse_a1(s: &str) -> Option<Addr> {
    let s = s.trim();
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = s.split_at(split);
    if letters.is_empty() || digits.is_empty() {
        return None;
    }
    let mut col: u32 = 0;
    for c in letters.chars() {
        if !c.is_ascii_alphabetic() {
            return None;
        }
        col = col * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
    }
    let row: u32 = digits.parse().ok()?;
    if row == 0 {
        return None;
    }
    Some(Addr {
        col: col - 1,
        row: row - 1,
    })
}

/// The spreadsheet grid.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sheet {
    // JSON maps require string keys, so (col,row) tuple keys are stored on disk
    // as a sorted list of entries — deterministic and diff-friendly.
    #[serde(with = "cells_serde")]
    cells: HashMap<(u32, u32), Cell>,
}

mod cells_serde {
    use super::Cell;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::HashMap;

    #[derive(Serialize, Deserialize)]
    struct Entry {
        col: u32,
        row: u32,
        cell: Cell,
    }

    pub fn serialize<S: Serializer>(
        map: &HashMap<(u32, u32), Cell>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let mut v: Vec<Entry> = map
            .iter()
            .map(|(&(col, row), cell)| Entry {
                col,
                row,
                cell: cell.clone(),
            })
            .collect();
        v.sort_by_key(|e| (e.row, e.col));
        v.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<HashMap<(u32, u32), Cell>, D::Error> {
        let v: Vec<Entry> = Vec::deserialize(d)?;
        Ok(v.into_iter().map(|e| ((e.col, e.row), e.cell)).collect())
    }
}

impl Sheet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a cell from raw user input. Leading `=` marks a formula; otherwise
    /// the input is parsed as a number if possible, else stored as text.
    pub fn set(&mut self, a1: &str, input: &str) {
        let addr = parse_a1(a1).expect("valid A1 address");
        let cell = if let Some(f) = input.strip_prefix('=') {
            Cell::Formula(f.trim().to_string())
        } else if let Ok(n) = input.trim().parse::<f64>() {
            Cell::Number(n)
        } else if input.is_empty() {
            Cell::Empty
        } else {
            Cell::Text(input.to_string())
        };
        self.cells.insert((addr.col, addr.row), cell);
    }

    /// Raw stored cell.
    pub fn raw(&self, a1: &str) -> Cell {
        let addr = match parse_a1(a1) {
            Some(a) => a,
            None => return Cell::Empty,
        };
        self.cells
            .get(&(addr.col, addr.row))
            .cloned()
            .unwrap_or(Cell::Empty)
    }

    /// Inclusive bounding box of non-empty cells as `(max_col, max_row)`,
    /// or `None` when the sheet is empty. Used to bound CSV/table export.
    pub fn extent(&self) -> Option<(u32, u32)> {
        self.cells
            .iter()
            .filter(|(_, c)| !matches!(c, Cell::Empty))
            .map(|(&(c, r), _)| (c, r))
            .reduce(|(mc, mr), (c, r)| (mc.max(c), mr.max(r)))
    }

    /// Evaluate a cell to a number (the common case for formulas).
    pub fn eval_number(&self, a1: &str) -> Result<f64, CalcError> {
        let addr = parse_a1(a1).ok_or_else(|| CalcError::Parse(a1.to_string()))?;
        let mut visiting = HashSet::new();
        self.eval_addr(addr, &mut visiting)
    }

    fn eval_addr(&self, addr: Addr, visiting: &mut HashSet<(u32, u32)>) -> Result<f64, CalcError> {
        let key = (addr.col, addr.row);
        if !visiting.insert(key) {
            return Err(CalcError::Cycle(a1_of(addr)));
        }
        let result = match self.cells.get(&key).cloned().unwrap_or(Cell::Empty) {
            Cell::Empty => Ok(0.0),
            Cell::Number(n) => Ok(n),
            Cell::Text(t) => t
                .trim()
                .parse::<f64>()
                .map_err(|_| CalcError::Value(format!("text {t:?}"))),
            Cell::Formula(src) => {
                let expr = parse(&src).map_err(CalcError::Parse)?;
                self.eval_expr(&expr, visiting)
            }
        };
        visiting.remove(&key);
        result
    }

    fn eval_expr(&self, expr: &Expr, visiting: &mut HashSet<(u32, u32)>) -> Result<f64, CalcError> {
        match expr {
            Expr::Number(n) => Ok(*n),
            Expr::Ref(a1) => {
                let addr = parse_a1(a1).ok_or_else(|| CalcError::Parse(a1.clone()))?;
                self.eval_addr(addr, visiting)
            }
            Expr::Neg(inner) => Ok(-self.eval_expr(inner, visiting)?),
            Expr::BinOp { op, lhs, rhs } => {
                let l = self.eval_expr(lhs, visiting)?;
                let r = self.eval_expr(rhs, visiting)?;
                Ok(match op {
                    '+' => l + r,
                    '-' => l - r,
                    '*' => l * r,
                    '/' => l / r,
                    _ => unreachable!(),
                })
            }
            Expr::Call { name, args } => self.eval_call(name, args, visiting),
            Expr::Range(a, b) => {
                // A bare range only makes sense inside a function; evaluating it
                // as a scalar is an error.
                Err(CalcError::Value(format!("range {a}:{b} outside function")))
            }
        }
    }

    fn eval_call(
        &self,
        name: &str,
        args: &[Expr],
        visiting: &mut HashSet<(u32, u32)>,
    ) -> Result<f64, CalcError> {
        let values = self.flatten_args(args, visiting)?;
        let up = name.to_ascii_uppercase();
        match up.as_str() {
            "SUM" => Ok(values.iter().sum()),
            "COUNT" => Ok(values.len() as f64),
            "AVERAGE" => {
                if values.is_empty() {
                    Err(CalcError::Value("AVERAGE of empty range".into()))
                } else {
                    Ok(values.iter().sum::<f64>() / values.len() as f64)
                }
            }
            "MIN" => values
                .iter()
                .cloned()
                .reduce(f64::min)
                .ok_or_else(|| CalcError::Value("MIN of empty range".into())),
            "MAX" => values
                .iter()
                .cloned()
                .reduce(f64::max)
                .ok_or_else(|| CalcError::Value("MAX of empty range".into())),
            _ => Err(CalcError::UnknownFn(name.to_string())),
        }
    }

    /// Expand args (scalars + ranges) into a flat list of numbers.
    fn flatten_args(
        &self,
        args: &[Expr],
        visiting: &mut HashSet<(u32, u32)>,
    ) -> Result<Vec<f64>, CalcError> {
        let mut out = Vec::new();
        for arg in args {
            match arg {
                Expr::Range(a, b) => {
                    let start = parse_a1(a).ok_or_else(|| CalcError::Parse(a.clone()))?;
                    let end = parse_a1(b).ok_or_else(|| CalcError::Parse(b.clone()))?;
                    let (c0, c1) = (start.col.min(end.col), start.col.max(end.col));
                    let (r0, r1) = (start.row.min(end.row), start.row.max(end.row));
                    for col in c0..=c1 {
                        for row in r0..=r1 {
                            out.push(self.eval_addr(Addr { col, row }, visiting)?);
                        }
                    }
                }
                other => out.push(self.eval_expr(other, visiting)?),
            }
        }
        Ok(out)
    }
}

/// Render a zero-based address back to A1 (used in error messages).
fn a1_of(addr: Addr) -> String {
    let mut col = addr.col + 1;
    let mut letters = String::new();
    while col > 0 {
        let rem = (col - 1) % 26;
        letters.insert(0, (b'A' + rem as u8) as char);
        col = (col - 1) / 26;
    }
    format!("{letters}{}", addr.row + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a1_parsing() {
        assert_eq!(parse_a1("A1"), Some(Addr { col: 0, row: 0 }));
        assert_eq!(parse_a1("B3"), Some(Addr { col: 1, row: 2 }));
        assert_eq!(parse_a1("Z1"), Some(Addr { col: 25, row: 0 }));
        assert_eq!(parse_a1("AA1"), Some(Addr { col: 26, row: 0 }));
        assert_eq!(parse_a1("A0"), None);
        assert_eq!(parse_a1("1"), None);
    }

    #[test]
    fn a1_roundtrip() {
        for s in ["A1", "B3", "Z9", "AA1", "AB12"] {
            let a = parse_a1(s).unwrap();
            assert_eq!(a1_of(a), s);
        }
    }

    #[test]
    fn literals_and_text() {
        let mut s = Sheet::new();
        s.set("A1", "42");
        s.set("A2", "hello");
        assert_eq!(s.eval_number("A1").unwrap(), 42.0);
        assert_eq!(s.raw("A2"), Cell::Text("hello".into()));
        assert_eq!(s.eval_number("A3").unwrap(), 0.0); // empty
    }

    #[test]
    fn arithmetic_and_precedence() {
        let mut s = Sheet::new();
        s.set("A1", "=2+3*4");
        assert_eq!(s.eval_number("A1").unwrap(), 14.0);
        s.set("A2", "=(2+3)*4");
        assert_eq!(s.eval_number("A2").unwrap(), 20.0);
        s.set("A3", "=-5+2");
        assert_eq!(s.eval_number("A3").unwrap(), -3.0);
    }

    #[test]
    fn cell_references_chain() {
        let mut s = Sheet::new();
        s.set("A1", "10");
        s.set("A2", "20");
        s.set("A3", "=A1+A2");
        s.set("A4", "=A3*2");
        assert_eq!(s.eval_number("A3").unwrap(), 30.0);
        assert_eq!(s.eval_number("A4").unwrap(), 60.0);
    }

    #[test]
    fn functions_over_ranges() {
        let mut s = Sheet::new();
        s.set("A1", "1");
        s.set("A2", "2");
        s.set("A3", "3");
        s.set("A4", "4");
        s.set("B1", "=SUM(A1:A4)");
        s.set("B2", "=AVERAGE(A1:A4)");
        s.set("B3", "=MIN(A1:A4)");
        s.set("B4", "=MAX(A1:A4)");
        s.set("B5", "=COUNT(A1:A4)");
        assert_eq!(s.eval_number("B1").unwrap(), 10.0);
        assert_eq!(s.eval_number("B2").unwrap(), 2.5);
        assert_eq!(s.eval_number("B3").unwrap(), 1.0);
        assert_eq!(s.eval_number("B4").unwrap(), 4.0);
        assert_eq!(s.eval_number("B5").unwrap(), 4.0);
    }

    #[test]
    fn function_with_mixed_args() {
        let mut s = Sheet::new();
        s.set("A1", "5");
        s.set("B1", "=SUM(A1, 10, 2*5)");
        assert_eq!(s.eval_number("B1").unwrap(), 25.0);
    }

    #[test]
    fn recalculates_on_dependency_change() {
        let mut s = Sheet::new();
        s.set("A1", "10");
        s.set("A2", "=A1*2");
        assert_eq!(s.eval_number("A2").unwrap(), 20.0);
        s.set("A1", "100");
        assert_eq!(s.eval_number("A2").unwrap(), 200.0);
    }

    #[test]
    fn detects_direct_cycle() {
        let mut s = Sheet::new();
        s.set("A1", "=B1");
        s.set("B1", "=A1");
        assert!(matches!(s.eval_number("A1"), Err(CalcError::Cycle(_))));
    }

    #[test]
    fn detects_self_cycle() {
        let mut s = Sheet::new();
        s.set("A1", "=A1+1");
        assert!(matches!(s.eval_number("A1"), Err(CalcError::Cycle(_))));
    }

    #[test]
    fn extent_bounds_non_empty_cells() {
        let mut s = Sheet::new();
        assert_eq!(s.extent(), None);
        s.set("A1", "1");
        s.set("C3", "9");
        s.set("B2", ""); // empty → ignored
        assert_eq!(s.extent(), Some((2, 2))); // C3 => col2,row2
    }

    #[test]
    fn unknown_function_errors() {
        let mut s = Sheet::new();
        s.set("A1", "=BOGUS(1,2)");
        assert!(matches!(s.eval_number("A1"), Err(CalcError::UnknownFn(_))));
    }
}
