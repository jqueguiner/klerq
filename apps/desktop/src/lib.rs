//! klerq-desktop — the shell that composes every Klerq engine.
//!
//! This library layer is UI-toolkit-agnostic so it can be driven by tests, a
//! CLI, or (later) an `egui` front-end. It wires localization, the three
//! document engines and the JS plugin host into one [`Workspace`].

use klerq_calc::Sheet;
use klerq_core::CommandStack;
use klerq_i18n::Localizer;
use klerq_plugin::{PluginHost, PluginManifest};
use klerq_slides::{AddSlide, Presentation};
use klerq_writer::{InsertParagraph, TextDocument};

/// Base (fallback) locale source, embedded at build time.
pub const EN_US_FTL: &str = include_str!("../../../locales/en-US/klerq.ftl");
/// French locale source.
pub const FR_FR_FTL: &str = include_str!("../../../locales/fr-FR/klerq.ftl");

/// A live Klerq session: one document per app plus localization.
pub struct Workspace {
    pub locale: Localizer,
    pub writer: TextDocument,
    pub calc: Sheet,
    pub slides: Presentation,
    writer_stack: CommandStack<TextDocument>,
    slides_stack: CommandStack<Presentation>,
}

impl Workspace {
    /// Build a workspace with English + French locales registered.
    pub fn new() -> Self {
        let mut locale = Localizer::new("en-US", EN_US_FTL).expect("valid en-US ftl");
        locale
            .add_locale("fr-FR", FR_FR_FTL)
            .expect("valid fr-FR ftl");
        Self {
            locale,
            writer: TextDocument::new(),
            calc: Sheet::new(),
            slides: Presentation::new(),
            writer_stack: CommandStack::new(),
            slides_stack: CommandStack::new(),
        }
    }

    /// Add a paragraph to the Writer document (undoable).
    pub fn write_paragraph(&mut self, text: &str) {
        self.writer_stack
            .execute(Box::new(InsertParagraph::new(text)), &mut self.writer);
    }

    /// Add a slide to the deck (undoable).
    pub fn add_slide(&mut self, title: &str) {
        self.slides_stack
            .execute(Box::new(AddSlide::new(title)), &mut self.slides);
    }

    /// Undo the last Writer edit.
    pub fn undo_writer(&mut self) -> bool {
        self.writer_stack.undo(&mut self.writer).is_ok()
    }

    /// One-line localized status summarizing the session.
    pub fn status(&self) -> String {
        let words = self.writer.word_count().to_string();
        self.locale.t_args("status-words", &[("count", &words)])
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

/// Run a text-mode demo of every engine and return a human-readable report.
/// Used by `main` and by the smoke test — proving the crates compose.
pub fn run_demo() -> String {
    let mut ws = Workspace::new();

    // Writer
    ws.write_paragraph("Klerq is a Rust-native office suite.");
    ws.write_paragraph("It ships Writer, Calc and Slides.");

    // Calc
    ws.calc.set("A1", "10");
    ws.calc.set("A2", "20");
    ws.calc.set("A3", "=SUM(A1:A2)");
    let sum = ws.calc.eval_number("A3").unwrap_or(f64::NAN);

    // Slides
    ws.add_slide("Welcome");
    ws.add_slide("Roadmap");

    // Plugin (community JavaScript)
    let manifest =
        PluginManifest::from_json(r#"{"name":"shouty","version":"1.0.0","permissions":[]}"#)
            .unwrap();
    let mut plugin =
        PluginHost::load(manifest, "function transform(s){ return s.toUpperCase(); }").unwrap();
    let shouted = plugin.call_transform("plugins work").unwrap();

    let mut report = String::new();
    report.push_str(&format!("{}\n", ws.locale.t("app-title")));
    report.push_str(&format!("{}\n", ws.locale.t("app-tagline")));
    report.push_str(&format!(
        "Writer: {} ({})\n",
        ws.writer.plain_text().replace('\n', " / "),
        ws.status()
    ));
    report.push_str(&format!("Calc:   A3 = SUM(A1:A2) = {sum}\n"));
    report.push_str(&format!("Slides: {} slides\n", ws.slides.len()));
    report.push_str(&format!("Plugin: {shouted}\n"));

    // Show localization by switching to French.
    ws.locale.set_locale("fr-FR").unwrap();
    report.push_str(&format!(
        "i18n:   {}\n",
        ws.locale.t_args("greeting", &[("name", "Ada")])
    ));
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_composes_all_engines() {
        let mut ws = Workspace::new();
        ws.write_paragraph("hello world here");
        ws.calc.set("A1", "=2+2");
        ws.add_slide("Deck");

        assert_eq!(ws.writer.word_count(), 3);
        assert_eq!(ws.calc.eval_number("A1").unwrap(), 4.0);
        assert_eq!(ws.slides.len(), 1);
    }

    #[test]
    fn undo_flows_through_workspace() {
        let mut ws = Workspace::new();
        ws.write_paragraph("one");
        ws.write_paragraph("two");
        assert_eq!(ws.writer.paragraphs.len(), 2);
        assert!(ws.undo_writer());
        assert_eq!(ws.writer.paragraphs.len(), 1);
    }

    #[test]
    fn status_is_localized() {
        let mut ws = Workspace::new();
        ws.write_paragraph("a b c");
        assert_eq!(ws.status(), "3 words");
        ws.locale.set_locale("fr-FR").unwrap();
        assert_eq!(ws.status(), "3 mots");
    }

    #[test]
    fn demo_runs_end_to_end() {
        let report = run_demo();
        assert!(report.contains("Klerq"));
        assert!(report.contains("SUM(A1:A2) = 30"));
        assert!(report.contains("PLUGINS WORK"));
        assert!(report.contains("Bienvenue")); // French greeting
    }
}
