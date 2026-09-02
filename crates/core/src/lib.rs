//! klerq-core — shared document primitives for every Klerq app.
//!
//! Provides:
//! - [`Value`]: portable scalar value used across writer/calc/slides.
//! - [`Command`]: reversible edit operation (apply / undo).
//! - [`CommandStack`]: undo/redo history that drives every editor.
//!
//! No I/O, no UI. Built TDD-first; see the `tests` module and `tests/`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Portable scalar value shared across document models.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum Value {
    Empty,
    Bool(bool),
    Number(f64),
    Text(String),
}

impl Value {
    /// Numeric coercion used by formula/layout engines. Text→parse, Bool→0/1.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Empty => Some(0.0),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Value::Number(n) => Some(*n),
            Value::Text(t) => t.trim().parse::<f64>().ok(),
        }
    }
}

/// Errors surfaced by the command stack.
#[derive(Debug, Error, PartialEq)]
pub enum CommandError {
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("nothing to redo")]
    NothingToRedo,
}

/// A reversible edit against a document of type `D`.
///
/// Implementors MUST guarantee `undo(apply(state)) == state` for the same
/// command instance — this invariant is what makes the [`CommandStack`] sound.
pub trait Command<D> {
    /// Human-readable label (localization key) for history UIs.
    fn label(&self) -> &str;
    /// Apply the change to the document.
    fn apply(&mut self, doc: &mut D);
    /// Reverse the change previously applied.
    fn undo(&mut self, doc: &mut D);
}

/// Undo/redo history. Applying a new command clears the redo stack.
pub struct CommandStack<D> {
    undo_stack: Vec<Box<dyn Command<D>>>,
    redo_stack: Vec<Box<dyn Command<D>>>,
}

impl<D> Default for CommandStack<D> {
    fn default() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }
}

impl<D> CommandStack<D> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply `cmd` to `doc`, push it onto the undo stack, clear redo history.
    pub fn execute(&mut self, mut cmd: Box<dyn Command<D>>, doc: &mut D) {
        cmd.apply(doc);
        self.undo_stack.push(cmd);
        self.redo_stack.clear();
    }

    /// Reverse the most recent command.
    pub fn undo(&mut self, doc: &mut D) -> Result<(), CommandError> {
        let mut cmd = self.undo_stack.pop().ok_or(CommandError::NothingToUndo)?;
        cmd.undo(doc);
        self.redo_stack.push(cmd);
        Ok(())
    }

    /// Re-apply the most recently undone command.
    pub fn redo(&mut self, doc: &mut D) -> Result<(), CommandError> {
        let mut cmd = self.redo_stack.pop().ok_or(CommandError::NothingToRedo)?;
        cmd.apply(doc);
        self.undo_stack.push(cmd);
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal doc + command used to prove the stack invariants (TDD fixtures).
    #[derive(Default)]
    struct Counter {
        value: i64,
    }

    struct Add(i64);
    impl Command<Counter> for Add {
        fn label(&self) -> &str {
            "cmd.add"
        }
        fn apply(&mut self, doc: &mut Counter) {
            doc.value += self.0;
        }
        fn undo(&mut self, doc: &mut Counter) {
            doc.value -= self.0;
        }
    }

    #[test]
    fn value_as_number_coerces() {
        assert_eq!(Value::Empty.as_number(), Some(0.0));
        assert_eq!(Value::Bool(true).as_number(), Some(1.0));
        assert_eq!(Value::Number(3.5).as_number(), Some(3.5));
        assert_eq!(Value::Text("42".into()).as_number(), Some(42.0));
        assert_eq!(Value::Text("nope".into()).as_number(), None);
    }

    #[test]
    fn execute_applies_and_records() {
        let mut doc = Counter::default();
        let mut stack = CommandStack::new();
        stack.execute(Box::new(Add(5)), &mut doc);
        assert_eq!(doc.value, 5);
        assert!(stack.can_undo());
        assert!(!stack.can_redo());
    }

    #[test]
    fn undo_then_redo_round_trips() {
        let mut doc = Counter::default();
        let mut stack = CommandStack::new();
        stack.execute(Box::new(Add(5)), &mut doc);
        stack.execute(Box::new(Add(3)), &mut doc);
        assert_eq!(doc.value, 8);

        stack.undo(&mut doc).unwrap();
        assert_eq!(doc.value, 5);
        stack.undo(&mut doc).unwrap();
        assert_eq!(doc.value, 0);
        assert_eq!(stack.undo(&mut doc), Err(CommandError::NothingToUndo));

        stack.redo(&mut doc).unwrap();
        assert_eq!(doc.value, 5);
        stack.redo(&mut doc).unwrap();
        assert_eq!(doc.value, 8);
        assert_eq!(stack.redo(&mut doc), Err(CommandError::NothingToRedo));
    }

    #[test]
    fn new_command_clears_redo() {
        let mut doc = Counter::default();
        let mut stack = CommandStack::new();
        stack.execute(Box::new(Add(1)), &mut doc);
        stack.undo(&mut doc).unwrap();
        assert!(stack.can_redo());
        stack.execute(Box::new(Add(9)), &mut doc);
        assert!(!stack.can_redo());
        assert_eq!(doc.value, 9);
    }
}
