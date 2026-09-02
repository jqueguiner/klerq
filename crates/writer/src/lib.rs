//! klerq-writer — the word-processing document model (MS Word analog).
//!
//! A [`TextDocument`] is a list of [`Paragraph`]s; each paragraph holds styled
//! [`Run`]s of text. Edits go through [`core::Command`] implementations so the
//! shared undo/redo stack works uniformly across the suite.
//!
//! Built TDD-first — see the `tests` module.

use klerq_core::Command;
use serde::{Deserialize, Serialize};

/// Inline character formatting.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// A contiguous span of text sharing one style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
}

impl Run {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: RunStyle::default(),
        }
    }
}

/// Paragraph alignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

/// A paragraph: styled runs plus block-level style.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Paragraph {
    pub runs: Vec<Run>,
    pub align: Align,
}

impl Paragraph {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            runs: vec![Run::plain(text)],
            align: Align::default(),
        }
    }

    /// Concatenated plain text of the paragraph.
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

/// The whole document.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TextDocument {
    pub paragraphs: Vec<Paragraph>,
}

impl TextDocument {
    pub fn new() -> Self {
        Self::default()
    }

    /// Full plain text, paragraphs joined by newlines.
    pub fn plain_text(&self) -> String {
        self.paragraphs
            .iter()
            .map(|p| p.text())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Whitespace-delimited word count across the document.
    pub fn word_count(&self) -> usize {
        self.paragraphs
            .iter()
            .flat_map(|p| p.text().split_whitespace().map(|_| ()).collect::<Vec<_>>())
            .count()
    }
}

// ----- Commands (reversible edits) -----

/// Append a paragraph to the end of the document.
pub struct InsertParagraph {
    para: Paragraph,
}

impl InsertParagraph {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            para: Paragraph::new(text),
        }
    }
}

impl Command<TextDocument> for InsertParagraph {
    fn label(&self) -> &str {
        "writer.insert_paragraph"
    }
    fn apply(&mut self, doc: &mut TextDocument) {
        doc.paragraphs.push(self.para.clone());
    }
    fn undo(&mut self, doc: &mut TextDocument) {
        doc.paragraphs.pop();
    }
}

/// Toggle bold on every run of paragraph `index`, remembering prior state.
pub struct ToggleBold {
    index: usize,
    prev: Vec<bool>,
}

impl ToggleBold {
    pub fn new(index: usize) -> Self {
        Self {
            index,
            prev: Vec::new(),
        }
    }
}

impl Command<TextDocument> for ToggleBold {
    fn label(&self) -> &str {
        "writer.toggle_bold"
    }
    fn apply(&mut self, doc: &mut TextDocument) {
        if let Some(p) = doc.paragraphs.get_mut(self.index) {
            self.prev = p.runs.iter().map(|r| r.style.bold).collect();
            for r in &mut p.runs {
                r.style.bold = !r.style.bold;
            }
        }
    }
    fn undo(&mut self, doc: &mut TextDocument) {
        if let Some(p) = doc.paragraphs.get_mut(self.index) {
            for (r, was) in p.runs.iter_mut().zip(self.prev.iter()) {
                r.style.bold = *was;
            }
        }
    }
}

/// Toggle italic on every run of paragraph `index`.
pub struct ToggleItalic {
    index: usize,
    prev: Vec<bool>,
}

impl ToggleItalic {
    pub fn new(index: usize) -> Self {
        Self {
            index,
            prev: Vec::new(),
        }
    }
}

impl Command<TextDocument> for ToggleItalic {
    fn label(&self) -> &str {
        "writer.toggle_italic"
    }
    fn apply(&mut self, doc: &mut TextDocument) {
        if let Some(p) = doc.paragraphs.get_mut(self.index) {
            self.prev = p.runs.iter().map(|r| r.style.italic).collect();
            for r in &mut p.runs {
                r.style.italic = !r.style.italic;
            }
        }
    }
    fn undo(&mut self, doc: &mut TextDocument) {
        if let Some(p) = doc.paragraphs.get_mut(self.index) {
            for (r, was) in p.runs.iter_mut().zip(self.prev.iter()) {
                r.style.italic = *was;
            }
        }
    }
}

/// Toggle underline on every run of paragraph `index`.
pub struct ToggleUnderline {
    index: usize,
    prev: Vec<bool>,
}

impl ToggleUnderline {
    pub fn new(index: usize) -> Self {
        Self {
            index,
            prev: Vec::new(),
        }
    }
}

impl Command<TextDocument> for ToggleUnderline {
    fn label(&self) -> &str {
        "writer.toggle_underline"
    }
    fn apply(&mut self, doc: &mut TextDocument) {
        if let Some(p) = doc.paragraphs.get_mut(self.index) {
            self.prev = p.runs.iter().map(|r| r.style.underline).collect();
            for r in &mut p.runs {
                r.style.underline = !r.style.underline;
            }
        }
    }
    fn undo(&mut self, doc: &mut TextDocument) {
        if let Some(p) = doc.paragraphs.get_mut(self.index) {
            for (r, was) in p.runs.iter_mut().zip(self.prev.iter()) {
                r.style.underline = *was;
            }
        }
    }
}

/// Set the alignment of paragraph `index`, remembering the previous value.
pub struct SetAlign {
    index: usize,
    align: Align,
    prev: Align,
}

impl SetAlign {
    pub fn new(index: usize, align: Align) -> Self {
        Self {
            index,
            align,
            prev: Align::default(),
        }
    }
}

impl Command<TextDocument> for SetAlign {
    fn label(&self) -> &str {
        "writer.set_align"
    }
    fn apply(&mut self, doc: &mut TextDocument) {
        if let Some(p) = doc.paragraphs.get_mut(self.index) {
            self.prev = p.align;
            p.align = self.align;
        }
    }
    fn undo(&mut self, doc: &mut TextDocument) {
        if let Some(p) = doc.paragraphs.get_mut(self.index) {
            p.align = self.prev;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klerq_core::CommandStack;

    #[test]
    fn empty_document_has_no_words() {
        let doc = TextDocument::new();
        assert_eq!(doc.word_count(), 0);
        assert_eq!(doc.plain_text(), "");
    }

    #[test]
    fn insert_paragraph_is_undoable() {
        let mut doc = TextDocument::new();
        let mut stack = CommandStack::new();
        stack.execute(Box::new(InsertParagraph::new("hello world")), &mut doc);
        stack.execute(Box::new(InsertParagraph::new("second line")), &mut doc);
        assert_eq!(doc.paragraphs.len(), 2);
        assert_eq!(doc.word_count(), 4);
        assert_eq!(doc.plain_text(), "hello world\nsecond line");

        stack.undo(&mut doc).unwrap();
        assert_eq!(doc.paragraphs.len(), 1);
        stack.redo(&mut doc).unwrap();
        assert_eq!(doc.paragraphs.len(), 2);
    }

    #[test]
    fn toggle_bold_round_trips() {
        let mut doc = TextDocument::new();
        let mut stack = CommandStack::new();
        stack.execute(Box::new(InsertParagraph::new("bold me")), &mut doc);
        assert!(!doc.paragraphs[0].runs[0].style.bold);

        stack.execute(Box::new(ToggleBold::new(0)), &mut doc);
        assert!(doc.paragraphs[0].runs[0].style.bold);

        stack.undo(&mut doc).unwrap();
        assert!(!doc.paragraphs[0].runs[0].style.bold);
    }

    #[test]
    fn italic_and_underline_toggle_round_trip() {
        let mut doc = TextDocument::new();
        let mut stack = CommandStack::new();
        stack.execute(Box::new(InsertParagraph::new("style me")), &mut doc);

        stack.execute(Box::new(ToggleItalic::new(0)), &mut doc);
        stack.execute(Box::new(ToggleUnderline::new(0)), &mut doc);
        assert!(doc.paragraphs[0].runs[0].style.italic);
        assert!(doc.paragraphs[0].runs[0].style.underline);

        stack.undo(&mut doc).unwrap(); // undo underline
        assert!(!doc.paragraphs[0].runs[0].style.underline);
        assert!(doc.paragraphs[0].runs[0].style.italic);
        stack.undo(&mut doc).unwrap(); // undo italic
        assert!(!doc.paragraphs[0].runs[0].style.italic);
    }

    #[test]
    fn set_align_is_undoable() {
        let mut doc = TextDocument::new();
        let mut stack = CommandStack::new();
        stack.execute(Box::new(InsertParagraph::new("centered")), &mut doc);
        assert_eq!(doc.paragraphs[0].align, Align::Left);

        stack.execute(Box::new(SetAlign::new(0, Align::Center)), &mut doc);
        assert_eq!(doc.paragraphs[0].align, Align::Center);
        stack.execute(Box::new(SetAlign::new(0, Align::Right)), &mut doc);
        assert_eq!(doc.paragraphs[0].align, Align::Right);

        stack.undo(&mut doc).unwrap();
        assert_eq!(doc.paragraphs[0].align, Align::Center);
        stack.undo(&mut doc).unwrap();
        assert_eq!(doc.paragraphs[0].align, Align::Left);
    }

    #[test]
    fn document_serializes_roundtrip() {
        let mut doc = TextDocument::new();
        doc.paragraphs.push(Paragraph::new("persist me"));
        let json = serde_json::to_string(&doc).unwrap();
        let back: TextDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
    }
}
