//! klerq-calc — the spreadsheet engine (MS Excel analog).
//!
//! A [`Sheet`] holds cells addressed in A1 notation. A cell is a literal
//! (number / text) or a `=formula`. Formulas support:
//! - arithmetic `+ - * /`, parentheses, unary minus
//! - comparisons `= <> < > <= >=` (yield 1/0), usable in `IF`
//! - cell references (`A1`) and ranges (`A1:B3`)
//! - a large function library ([`FUNCTION_NAMES`], dispatched in `functions.rs`):
//!   math/trig/hyperbolic, rounding, combinatorics & special (`GAMMA`, `ERF`),
//!   statistics (`MEDIAN`, `STDEV`, `PERCENTILE`, `MODE`, …), logic
//!   (`IF`/`IFS`/`IFERROR` lazy, `AND`/`OR`/`XOR`/`NOT`), bitwise, financial
//!   (`PMT`, `FV`, `PV`, `RATE`, `NPER`, `IPMT`, `NPV`, `IRR`, `MIRR`, `DDB`, …)
//!   and forecasting/regression (`SLOPE`, `INTERCEPT`, `FORECAST`, `TREND`,
//!   `CORREL`, `RSQ`, `GROWTH`, …).
//!
//! Evaluation is recursive with **cycle detection**, so `A1=B1`, `B1=A1`
//! yields a [`CalcError::Cycle`] instead of hanging.
//!
//! Built TDD-first — see the `tests` module (written before the engine).

mod functions;
mod parser;

pub use functions::FUNCTION_NAMES;

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
            Expr::Cmp { op, lhs, rhs } => {
                let l = self.eval_expr(lhs, visiting)?;
                let r = self.eval_expr(rhs, visiting)?;
                let truth = match op.as_str() {
                    "=" => l == r,
                    "<>" => l != r,
                    "<" => l < r,
                    ">" => l > r,
                    "<=" => l <= r,
                    ">=" => l >= r,
                    _ => unreachable!(),
                };
                Ok(if truth { 1.0 } else { 0.0 })
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
        let up = name.to_ascii_uppercase();

        // ----- Lazy functions (need the AST, not just evaluated values) -----

        // IF: only the taken branch is evaluated, so the untaken branch may
        // reference erroring/cyclic cells without failing the whole formula.
        if up == "IF" {
            if args.len() != 3 {
                return Err(CalcError::Value("IF needs 3 arguments".into()));
            }
            let cond = self.eval_expr(&args[0], visiting)?;
            let branch = if cond != 0.0 { &args[1] } else { &args[2] };
            return self.eval_expr(branch, visiting);
        }
        // IFS: pairs of (cond, value); return the first value whose cond is true.
        if up == "IFS" {
            let mut i = 0;
            while i + 1 < args.len() {
                if self.eval_expr(&args[i], visiting)? != 0.0 {
                    return self.eval_expr(&args[i + 1], visiting);
                }
                i += 2;
            }
            return Err(CalcError::Value("IFS: no condition matched".into()));
        }
        // IFERROR(value, fallback): swallow an error in `value`.
        if up == "IFERROR" {
            if args.len() != 2 {
                return Err(CalcError::Value("IFERROR needs 2 arguments".into()));
            }
            return match self.eval_expr(&args[0], visiting) {
                Ok(v) => Ok(v),
                Err(_) => self.eval_expr(&args[1], visiting),
            };
        }

        // ----- Registry functions over the flattened value list -----
        let values = self.flatten_args(args, visiting)?;
        match functions::call(&up, &values) {
            Some(Ok(v)) => Ok(v),
            Some(Err(e)) => Err(CalcError::Value(format!("{up}: {e}"))),
            None => Err(CalcError::UnknownFn(name.to_string())),
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
    fn comparison_operators() {
        let mut s = Sheet::new();
        s.set("A1", "5");
        s.set("B1", "=A1>3");
        s.set("B2", "=A1<3");
        s.set("B3", "=A1=5");
        s.set("B4", "=A1<>5");
        s.set("B5", "=A1>=5");
        s.set("B6", "=A1<=4");
        assert_eq!(s.eval_number("B1").unwrap(), 1.0);
        assert_eq!(s.eval_number("B2").unwrap(), 0.0);
        assert_eq!(s.eval_number("B3").unwrap(), 1.0);
        assert_eq!(s.eval_number("B4").unwrap(), 0.0);
        assert_eq!(s.eval_number("B5").unwrap(), 1.0);
        assert_eq!(s.eval_number("B6").unwrap(), 0.0);
    }

    #[test]
    fn if_function_picks_branch() {
        let mut s = Sheet::new();
        s.set("A1", "12");
        s.set("A2", "=IF(A1>10, 100, 0)");
        s.set("A3", "=IF(A1>100, 100, 0)");
        assert_eq!(s.eval_number("A2").unwrap(), 100.0);
        assert_eq!(s.eval_number("A3").unwrap(), 0.0);
    }

    #[test]
    fn if_is_lazy_untaken_branch_not_evaluated() {
        let mut s = Sheet::new();
        s.set("C1", "=C1"); // self-cyclic cell
        s.set("A1", "=IF(1, 42, C1)"); // false branch (C1) must not be touched
        assert_eq!(s.eval_number("A1").unwrap(), 42.0);
    }

    #[test]
    fn logical_functions() {
        let mut s = Sheet::new();
        s.set("A1", "1");
        s.set("A2", "0");
        assert_eq!(s.eval_number("A1").unwrap(), 1.0);
        s.set("B1", "=AND(A1, 1, 5)");
        s.set("B2", "=AND(A1, A2)");
        s.set("B3", "=OR(A2, 0, 3)");
        s.set("B4", "=NOT(A2)");
        assert_eq!(s.eval_number("B1").unwrap(), 1.0);
        assert_eq!(s.eval_number("B2").unwrap(), 0.0);
        assert_eq!(s.eval_number("B3").unwrap(), 1.0);
        assert_eq!(s.eval_number("B4").unwrap(), 1.0);
    }

    #[test]
    fn math_functions() {
        let mut s = Sheet::new();
        s.set("A1", "=ABS(-7)");
        s.set("A2", "=ROUND(1.23456, 2)");
        s.set("A3", "=MOD(10, 3)");
        s.set("A4", "=POWER(2, 10)");
        s.set("A5", "=SQRT(144)");
        s.set("A6", "=INT(3.9)");
        s.set("A7", "=PRODUCT(2, 3, 4)");
        s.set("A8", "=CEILING(4.1)");
        s.set("A9", "=FLOOR(4.9)");
        assert_eq!(s.eval_number("A1").unwrap(), 7.0);
        assert_eq!(s.eval_number("A2").unwrap(), 1.23);
        assert_eq!(s.eval_number("A3").unwrap(), 1.0);
        assert_eq!(s.eval_number("A4").unwrap(), 1024.0);
        assert_eq!(s.eval_number("A5").unwrap(), 12.0);
        assert_eq!(s.eval_number("A6").unwrap(), 3.0);
        assert_eq!(s.eval_number("A7").unwrap(), 24.0);
        assert_eq!(s.eval_number("A8").unwrap(), 5.0);
        assert_eq!(s.eval_number("A9").unwrap(), 4.0);
    }

    #[test]
    fn nested_if_with_functions() {
        let mut s = Sheet::new();
        s.set("A1", "85");
        // grade: >=90 -> 4, >=80 -> 3, else 0
        s.set("A2", "=IF(A1>=90, 4, IF(A1>=80, 3, 0))");
        assert_eq!(s.eval_number("A2").unwrap(), 3.0);
    }

    #[test]
    fn function_registry_is_large_and_all_recognized() {
        // Every advertised name must dispatch (never "unknown function").
        assert!(
            FUNCTION_NAMES.len() >= 200,
            "only {} functions",
            FUNCTION_NAMES.len()
        );
        for name in FUNCTION_NAMES {
            let got = functions::call(name, &[1.0, 2.0, 3.0, 4.0]);
            assert!(
                got.is_some(),
                "function {name} not recognized by dispatcher"
            );
        }
    }

    #[test]
    fn trig_and_math_functions() {
        let mut s = Sheet::new();
        s.set("A1", "=SIN(0)");
        s.set("A2", "=COS(0)");
        s.set("A3", "=DEGREES(PI())");
        s.set("A4", "=LOG(8, 2)");
        s.set("A5", "=GCD(12, 18)");
        s.set("A6", "=LCM(4, 6)");
        s.set("A7", "=EXP(0)");
        assert_eq!(s.eval_number("A1").unwrap(), 0.0);
        assert_eq!(s.eval_number("A2").unwrap(), 1.0);
        assert!((s.eval_number("A3").unwrap() - 180.0).abs() < 1e-9);
        assert!((s.eval_number("A4").unwrap() - 3.0).abs() < 1e-9);
        assert_eq!(s.eval_number("A5").unwrap(), 6.0);
        assert_eq!(s.eval_number("A6").unwrap(), 12.0);
        assert_eq!(s.eval_number("A7").unwrap(), 1.0);
    }

    #[test]
    fn statistics_functions() {
        let mut s = Sheet::new();
        for (i, v) in [2, 4, 4, 4, 5, 5, 7, 9].iter().enumerate() {
            s.set(&format!("A{}", i + 1), &v.to_string());
        }
        s.set("B1", "=AVERAGE(A1:A8)");
        s.set("B2", "=MEDIAN(A1:A8)");
        s.set("B3", "=STDEVP(A1:A8)");
        s.set("B4", "=VARP(A1:A8)");
        s.set("B5", "=MODE(A1:A8)");
        assert_eq!(s.eval_number("B1").unwrap(), 5.0);
        assert_eq!(s.eval_number("B2").unwrap(), 4.5);
        assert!((s.eval_number("B3").unwrap() - 2.0).abs() < 1e-9);
        assert_eq!(s.eval_number("B4").unwrap(), 4.0);
        assert_eq!(s.eval_number("B5").unwrap(), 4.0);
    }

    #[test]
    fn financial_functions() {
        let mut s = Sheet::new();
        // Loan: 5%/yr over 10 yrs, PV 1000.
        s.set("A1", "=PMT(0.05, 10, 1000)");
        s.set("A2", "=FV(0.05, 10, -100)");
        s.set("A3", "=NPV(0.1, 100, 100, 100)");
        s.set("A4", "=SLN(1000, 100, 10)");
        assert!((s.eval_number("A1").unwrap() - -129.504575).abs() < 1e-3);
        assert!((s.eval_number("A2").unwrap() - 1257.78925).abs() < 1e-2);
        assert!((s.eval_number("A3").unwrap() - 248.685).abs() < 1e-2);
        assert_eq!(s.eval_number("A4").unwrap(), 90.0);
    }

    #[test]
    fn forecasting_functions() {
        // Perfect line y = 2x + 1 over x = 1..4  → known_y then known_x.
        let mut s = Sheet::new();
        s.set("A1", "=SLOPE(3, 5, 7, 9, 1, 2, 3, 4)");
        s.set("A2", "=INTERCEPT(3, 5, 7, 9, 1, 2, 3, 4)");
        s.set("A3", "=RSQ(3, 5, 7, 9, 1, 2, 3, 4)");
        s.set("A4", "=FORECAST(10, 3, 5, 7, 9, 1, 2, 3, 4)"); // y at x=10
        assert!((s.eval_number("A1").unwrap() - 2.0).abs() < 1e-9);
        assert!((s.eval_number("A2").unwrap() - 1.0).abs() < 1e-9);
        assert!((s.eval_number("A3").unwrap() - 1.0).abs() < 1e-9);
        assert!((s.eval_number("A4").unwrap() - 21.0).abs() < 1e-9);
    }

    #[test]
    fn disruptive_functions_not_in_excel() {
        let mut s = Sheet::new();
        // ML activations
        s.set("A1", "=RELU(-3)");
        s.set("A2", "=SIGMOID(0)");
        // shaping
        s.set("A3", "=CLAMP(15, 0, 10)");
        s.set("A4", "=LERP(0, 10, 0.5)");
        s.set("A5", "=REMAP(5, 0, 10, 0, 100)");
        // info stats
        s.set("A6", "=ENTROPY(1, 1, 1, 1)"); // uniform 4 → ln 4
                                             // number theory
        s.set("A7", "=ISPRIME(97)");
        s.set("A8", "=FIB(10)");
        s.set("A9", "=POPCOUNT(7)");
        // geo: London → Paris ≈ 344 km
        s.set("A10", "=HAVERSINE(51.5074, -0.1278, 48.8566, 2.3522)");
        // paired ML metrics: MSE of [1,2] vs [1,3] = (0+1)/2 = 0.5
        s.set("A11", "=MSE(1, 2, 1, 3)");
        // cosine of identical vectors = 1
        s.set("A12", "=COSINE(1, 2, 1, 2)");
        // quant: CAGR 100→200 over 2y ≈ 0.4142
        s.set("A13", "=CAGR(100, 200, 2)");

        assert_eq!(s.eval_number("A1").unwrap(), 0.0);
        assert_eq!(s.eval_number("A2").unwrap(), 0.5);
        assert_eq!(s.eval_number("A3").unwrap(), 10.0);
        assert_eq!(s.eval_number("A4").unwrap(), 5.0);
        assert_eq!(s.eval_number("A5").unwrap(), 50.0);
        assert!((s.eval_number("A6").unwrap() - 4f64.ln()).abs() < 1e-9);
        assert_eq!(s.eval_number("A7").unwrap(), 1.0);
        assert_eq!(s.eval_number("A8").unwrap(), 55.0);
        assert_eq!(s.eval_number("A9").unwrap(), 3.0);
        assert!((s.eval_number("A10").unwrap() - 344.0).abs() < 5.0);
        assert_eq!(s.eval_number("A11").unwrap(), 0.5);
        assert!((s.eval_number("A12").unwrap() - 1.0).abs() < 1e-9);
        assert!((s.eval_number("A13").unwrap() - 0.414213).abs() < 1e-4);
    }

    #[test]
    fn unknown_function_errors() {
        let mut s = Sheet::new();
        s.set("A1", "=BOGUS(1,2)");
        assert!(matches!(s.eval_number("A1"), Err(CalcError::UnknownFn(_))));
    }
}
