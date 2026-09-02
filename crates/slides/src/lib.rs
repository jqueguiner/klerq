//! klerq-slides — the presentation model (MS PowerPoint analog).
//!
//! A [`Presentation`] is an ordered list of [`Slide`]s; each slide has a title
//! and a set of [`Shape`]s (text boxes, rectangles, images…). Edits are
//! [`core::Command`]s so undo/redo is shared with the rest of the suite.
//!
//! Built TDD-first — see the `tests` module.

use klerq_core::Command;
use serde::{Deserialize, Serialize};

/// Kind of shape placed on a slide.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ShapeKind {
    TextBox,
    Rectangle,
    Ellipse,
    Image,
}

/// A positioned shape (EMU-agnostic units; renderer decides scale).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub kind: ShapeKind,
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Shape {
    pub fn text_box(text: impl Into<String>) -> Self {
        Self {
            kind: ShapeKind::TextBox,
            text: text.into(),
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 40.0,
        }
    }
}

/// One slide.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Slide {
    pub title: String,
    pub shapes: Vec<Shape>,
}

impl Slide {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            shapes: Vec::new(),
        }
    }
}

/// The whole deck.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Presentation {
    pub slides: Vec<Slide>,
}

impl Presentation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.slides.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slides.is_empty()
    }
}

// ----- Commands -----

/// Append a slide with the given title.
pub struct AddSlide {
    title: String,
}

impl AddSlide {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
        }
    }
}

impl Command<Presentation> for AddSlide {
    fn label(&self) -> &str {
        "slides.add_slide"
    }
    fn apply(&mut self, doc: &mut Presentation) {
        doc.slides.push(Slide::new(self.title.clone()));
    }
    fn undo(&mut self, doc: &mut Presentation) {
        doc.slides.pop();
    }
}

/// Add a shape to slide `index`.
pub struct AddShape {
    index: usize,
    shape: Shape,
}

impl AddShape {
    pub fn new(index: usize, shape: Shape) -> Self {
        Self { index, shape }
    }
}

impl Command<Presentation> for AddShape {
    fn label(&self) -> &str {
        "slides.add_shape"
    }
    fn apply(&mut self, doc: &mut Presentation) {
        if let Some(slide) = doc.slides.get_mut(self.index) {
            slide.shapes.push(self.shape.clone());
        }
    }
    fn undo(&mut self, doc: &mut Presentation) {
        if let Some(slide) = doc.slides.get_mut(self.index) {
            slide.shapes.pop();
        }
    }
}

/// Move a slide from one position to another (reorder), reversibly.
pub struct MoveSlide {
    from: usize,
    to: usize,
}

impl MoveSlide {
    pub fn new(from: usize, to: usize) -> Self {
        Self { from, to }
    }
}

impl Command<Presentation> for MoveSlide {
    fn label(&self) -> &str {
        "slides.move_slide"
    }
    fn apply(&mut self, doc: &mut Presentation) {
        if self.from < doc.slides.len() && self.to < doc.slides.len() {
            let s = doc.slides.remove(self.from);
            doc.slides.insert(self.to, s);
        }
    }
    fn undo(&mut self, doc: &mut Presentation) {
        if self.to < doc.slides.len() && self.from < doc.slides.len() {
            let s = doc.slides.remove(self.to);
            doc.slides.insert(self.from, s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klerq_core::CommandStack;

    #[test]
    fn new_deck_is_empty() {
        assert!(Presentation::new().is_empty());
    }

    #[test]
    fn add_slide_is_undoable() {
        let mut deck = Presentation::new();
        let mut stack = CommandStack::new();
        stack.execute(Box::new(AddSlide::new("Intro")), &mut deck);
        stack.execute(Box::new(AddSlide::new("Body")), &mut deck);
        assert_eq!(deck.len(), 2);
        assert_eq!(deck.slides[0].title, "Intro");

        stack.undo(&mut deck).unwrap();
        assert_eq!(deck.len(), 1);
    }

    #[test]
    fn add_shape_to_slide() {
        let mut deck = Presentation::new();
        let mut stack = CommandStack::new();
        stack.execute(Box::new(AddSlide::new("S1")), &mut deck);
        stack.execute(
            Box::new(AddShape::new(0, Shape::text_box("Hello"))),
            &mut deck,
        );
        assert_eq!(deck.slides[0].shapes.len(), 1);
        assert_eq!(deck.slides[0].shapes[0].text, "Hello");

        stack.undo(&mut deck).unwrap();
        assert_eq!(deck.slides[0].shapes.len(), 0);
    }

    #[test]
    fn move_slide_reorders_and_undoes() {
        let mut deck = Presentation::new();
        let mut stack = CommandStack::new();
        for t in ["A", "B", "C"] {
            stack.execute(Box::new(AddSlide::new(t)), &mut deck);
        }
        stack.execute(Box::new(MoveSlide::new(0, 2)), &mut deck);
        let titles: Vec<_> = deck.slides.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["B", "C", "A"]);

        stack.undo(&mut deck).unwrap();
        let titles: Vec<_> = deck.slides.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["A", "B", "C"]);
    }

    #[test]
    fn deck_serializes_roundtrip() {
        let mut deck = Presentation::new();
        deck.slides.push(Slide::new("only"));
        let json = serde_json::to_string(&deck).unwrap();
        let back: Presentation = serde_json::from_str(&json).unwrap();
        assert_eq!(deck, back);
    }
}
