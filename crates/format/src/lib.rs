//! klerq-format — reading and writing Klerq documents.
//!
//! Two layers:
//! - **Native format**: a versioned JSON envelope (`{ klerq, kind, version, doc }`)
//!   that losslessly round-trips each document model. File extensions:
//!   `.klw` (Writer), `.klc` (Calc), `.kls` (Slides).
//! - **Interop**: plain-text for Writer, CSV for Calc (evaluated values in / raw
//!   formulas out), and a text outline for Slides. This is the bridge toward
//!   full MS Office (OOXML) import/export, tracked in PLAN.md Phase 8.
//!
//! Built TDD-first — see the `tests` module (written before the code).

use klerq_calc::{parse_a1, Cell, Sheet};
use klerq_slides::{Presentation, Shape, Slide};
use klerq_writer::{Paragraph, TextDocument};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Bumped when the on-disk envelope shape changes incompatibly.
pub const FORMAT_VERSION: u32 = 1;

/// Native file extensions per document kind.
pub const EXT_WRITER: &str = "klw";
pub const EXT_CALC: &str = "klc";
pub const EXT_SLIDES: &str = "kls";

#[derive(Debug, Error, PartialEq)]
pub enum FormatError {
    #[error("json error: {0}")]
    Json(String),
    #[error("wrong document kind: expected {expected}, found {found}")]
    WrongKind { expected: String, found: String },
    #[error("unsupported format version: {0}")]
    UnsupportedVersion(u32),
}

/// Versioned on-disk envelope wrapping a document body.
#[derive(Debug, Serialize, Deserialize)]
struct Envelope<T> {
    klerq: String,
    kind: String,
    version: u32,
    doc: T,
}

fn save<T: Serialize>(kind: &str, doc: &T) -> String {
    let env = Envelope {
        klerq: "klerq".to_string(),
        kind: kind.to_string(),
        version: FORMAT_VERSION,
        doc,
    };
    // Pretty output: documents are meant to be diff-friendly in version control.
    serde_json::to_string_pretty(&env).expect("serializable document")
}

fn load<T: for<'de> Deserialize<'de>>(kind: &str, s: &str) -> Result<T, FormatError> {
    // Read the envelope generically first so kind/version are validated before
    // we try to shape the body into `T` (gives a precise WrongKind error).
    let env: Envelope<serde_json::Value> =
        serde_json::from_str(s).map_err(|e| FormatError::Json(e.to_string()))?;
    if env.version > FORMAT_VERSION {
        return Err(FormatError::UnsupportedVersion(env.version));
    }
    if env.kind != kind {
        return Err(FormatError::WrongKind {
            expected: kind.to_string(),
            found: env.kind,
        });
    }
    serde_json::from_value(env.doc).map_err(|e| FormatError::Json(e.to_string()))
}

// ---- Native (lossless) ----

pub fn save_writer(doc: &TextDocument) -> String {
    save("writer", doc)
}
pub fn load_writer(s: &str) -> Result<TextDocument, FormatError> {
    load("writer", s)
}
pub fn save_calc(sheet: &Sheet) -> String {
    save("calc", sheet)
}
pub fn load_calc(s: &str) -> Result<Sheet, FormatError> {
    load("calc", s)
}
pub fn save_slides(deck: &Presentation) -> String {
    save("slides", deck)
}
pub fn load_slides(s: &str) -> Result<Presentation, FormatError> {
    load("slides", s)
}

// ---- Interop ----

/// Format a float without a trailing `.0` for whole numbers.
fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// Writer → plain text (paragraphs joined by newlines).
pub fn writer_to_text(doc: &TextDocument) -> String {
    doc.plain_text()
}

/// Plain text → Writer (one paragraph per line).
pub fn writer_from_text(text: &str) -> TextDocument {
    let mut doc = TextDocument::new();
    for line in text.split('\n') {
        doc.paragraphs.push(Paragraph::new(line));
    }
    doc
}

fn col_to_letters(mut col: u32) -> String {
    let mut s = String::new();
    col += 1;
    while col > 0 {
        let rem = (col - 1) % 26;
        s.insert(0, (b'A' + rem as u8) as char);
        col = (col - 1) / 26;
    }
    s
}

fn csv_escape(field: &str) -> String {
    if field.contains([',', '"', '\n']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// One cell's display value: text as-is, formulas/numbers evaluated, `#ERR` on
/// failure, empty string for blank cells.
fn cell_value(sheet: &Sheet, a1: &str) -> String {
    match sheet.raw(a1) {
        Cell::Empty => String::new(),
        Cell::Text(t) => t,
        _ => match sheet.eval_number(a1) {
            Ok(n) => fmt_num(n),
            Err(_) => "#ERR".to_string(),
        },
    }
}

/// Calc → CSV of **evaluated** values (what you see in the grid).
pub fn calc_to_csv(sheet: &Sheet) -> String {
    let Some((max_col, max_row)) = sheet.extent() else {
        return String::new();
    };
    let mut out = String::new();
    for row in 0..=max_row {
        let mut fields = Vec::with_capacity((max_col + 1) as usize);
        for col in 0..=max_col {
            let a1 = format!("{}{}", col_to_letters(col), row + 1);
            fields.push(csv_escape(&cell_value(sheet, &a1)));
        }
        out.push_str(&fields.join(","));
        out.push('\n');
    }
    out
}

/// Split a single CSV line honoring double-quoted fields.
fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
            '"' => in_quotes = true,
            ',' if !in_quotes => {
                fields.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

/// CSV → Calc. Each non-empty field becomes a cell; `=…` fields become formulas.
pub fn calc_from_csv(csv: &str) -> Sheet {
    let mut sheet = Sheet::new();
    for (row, line) in csv.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        for (col, field) in split_csv_line(line).into_iter().enumerate() {
            if field.is_empty() {
                continue;
            }
            let a1 = format!("{}{}", col_to_letters(col as u32), row + 1);
            debug_assert!(parse_a1(&a1).is_some());
            sheet.set(&a1, &field);
        }
    }
    sheet
}

/// Slides → a Markdown-ish text outline.
pub fn slides_to_outline(deck: &Presentation) -> String {
    let mut out = String::new();
    for (i, slide) in deck.slides.iter().enumerate() {
        out.push_str(&format!("# Slide {}: {}\n", i + 1, slide.title));
        for shape in &slide.shapes {
            if !shape.text.is_empty() {
                out.push_str(&format!("- {}\n", shape.text));
            }
        }
    }
    out
}

/// Text outline → Slides (`# ` starts a slide, `- ` adds a text box).
pub fn slides_from_outline(text: &str) -> Presentation {
    let mut deck = Presentation::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# ") {
            // Drop an optional "Slide N: " prefix if present.
            let title = rest.splitn(2, ": ").last().unwrap_or(rest).to_string();
            deck.slides.push(Slide::new(title));
        } else if let Some(rest) = line.strip_prefix("- ") {
            if let Some(slide) = deck.slides.last_mut() {
                slide.shapes.push(Shape::text_box(rest));
            }
        }
    }
    deck
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- native round-trips ----

    #[test]
    fn writer_native_roundtrip() {
        let mut doc = TextDocument::new();
        doc.paragraphs.push(Paragraph::new("hello"));
        doc.paragraphs.push(Paragraph::new("world"));
        let saved = save_writer(&doc);
        assert!(saved.contains("\"kind\": \"writer\""));
        assert_eq!(load_writer(&saved).unwrap(), doc);
    }

    #[test]
    fn calc_native_roundtrip_preserves_formulas() {
        let mut s = Sheet::new();
        s.set("A1", "10");
        s.set("A2", "=A1*2");
        let saved = save_calc(&s);
        let back = load_calc(&saved).unwrap();
        assert_eq!(back.raw("A2"), Cell::Formula("A1*2".to_string()));
        assert_eq!(back.eval_number("A2").unwrap(), 20.0);
    }

    #[test]
    fn slides_native_roundtrip() {
        let mut deck = Presentation::new();
        deck.slides.push(Slide::new("Intro"));
        let saved = save_slides(&deck);
        assert_eq!(load_slides(&saved).unwrap(), deck);
    }

    #[test]
    fn wrong_kind_is_rejected() {
        let doc = TextDocument::new();
        let saved = save_writer(&doc);
        let err = load_calc(&saved).unwrap_err();
        assert!(matches!(err, FormatError::WrongKind { .. }));
    }

    #[test]
    fn future_version_is_rejected() {
        let doc = TextDocument::new();
        let saved = save_writer(&doc).replace("\"version\": 1", "\"version\": 999");
        assert_eq!(
            load_writer(&saved).unwrap_err(),
            FormatError::UnsupportedVersion(999)
        );
    }

    #[test]
    fn corrupt_json_errors() {
        assert!(matches!(
            load_writer("{not json"),
            Err(FormatError::Json(_))
        ));
    }

    // ---- interop ----

    #[test]
    fn writer_text_roundtrip() {
        let doc = writer_from_text("line one\nline two");
        assert_eq!(doc.paragraphs.len(), 2);
        assert_eq!(writer_to_text(&doc), "line one\nline two");
    }

    #[test]
    fn calc_exports_evaluated_csv() {
        let mut s = Sheet::new();
        s.set("A1", "Item");
        s.set("B1", "Qty");
        s.set("A2", "Widgets");
        s.set("B2", "10");
        s.set("A3", "Total");
        s.set("B3", "=B2+5");
        let csv = calc_to_csv(&s);
        assert_eq!(csv, "Item,Qty\nWidgets,10\nTotal,15\n");
    }

    #[test]
    fn csv_escapes_special_fields() {
        let mut s = Sheet::new();
        s.set("A1", "a,b");
        s.set("B1", "quote\"x");
        let csv = calc_to_csv(&s);
        assert_eq!(csv, "\"a,b\",\"quote\"\"x\"\n");
    }

    #[test]
    fn csv_import_then_recalc() {
        let sheet = calc_from_csv("Item,Qty\nWidgets,10\nTotal,=B2+5");
        assert_eq!(sheet.raw("A1"), Cell::Text("Item".into()));
        assert_eq!(sheet.eval_number("B2").unwrap(), 10.0);
        assert_eq!(sheet.eval_number("B3").unwrap(), 15.0); // formula imported
    }

    #[test]
    fn csv_roundtrips_quoted_fields() {
        let sheet = calc_from_csv("\"a,b\",plain\n");
        assert_eq!(sheet.raw("A1"), Cell::Text("a,b".into()));
        assert_eq!(sheet.raw("B1"), Cell::Text("plain".into()));
    }

    #[test]
    fn slides_outline_roundtrip() {
        let mut deck = Presentation::new();
        let mut s = Slide::new("Welcome");
        s.shapes.push(Shape::text_box("first point"));
        s.shapes.push(Shape::text_box("second point"));
        deck.slides.push(s);
        deck.slides.push(Slide::new("End"));

        let outline = slides_to_outline(&deck);
        let back = slides_from_outline(&outline);
        assert_eq!(back.slides.len(), 2);
        assert_eq!(back.slides[0].title, "Welcome");
        assert_eq!(back.slides[0].shapes.len(), 2);
        assert_eq!(back.slides[0].shapes[1].text, "second point");
        assert_eq!(back.slides[1].title, "End");
    }
}
