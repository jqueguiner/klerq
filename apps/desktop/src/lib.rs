//! klerq-desktop — the shell that composes every Klerq engine.
//!
//! This library layer is UI-toolkit-agnostic so it can be driven by tests, a
//! CLI, or (later) an `egui` front-end. It wires localization, the three
//! document engines and the JS plugin host into one [`Workspace`].

use klerq_calc::Sheet;
use klerq_core::CommandStack;
use klerq_i18n::Localizer;
use klerq_plugin::{PluginHost, PluginManifest};
use klerq_slides::{AddShape, AddSlide, Presentation, Shape};
use klerq_writer::{InsertParagraph, TextDocument, ToggleBold};

/// Format a float for display: integers without a trailing `.0`.
pub fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

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

    /// Redo the last undone Writer edit.
    pub fn redo_writer(&mut self) -> bool {
        self.writer_stack.redo(&mut self.writer).is_ok()
    }

    /// Toggle bold on a Writer paragraph (undoable).
    pub fn toggle_bold(&mut self, index: usize) {
        self.writer_stack
            .execute(Box::new(ToggleBold::new(index)), &mut self.writer);
    }

    pub fn can_undo_writer(&self) -> bool {
        self.writer_stack.can_undo()
    }

    pub fn can_redo_writer(&self) -> bool {
        self.writer_stack.can_redo()
    }

    /// Set a Calc cell from raw user input (`42`, `text`, or `=SUM(A1:A2)`).
    pub fn set_cell(&mut self, a1: &str, input: &str) {
        self.calc.set(a1, input);
    }

    /// What a cell should show in the grid: evaluated number, text, or `#ERR`.
    pub fn cell_display(&self, a1: &str) -> String {
        match self.calc.raw(a1) {
            klerq_calc::Cell::Empty => String::new(),
            klerq_calc::Cell::Text(t) => t,
            _ => match self.calc.eval_number(a1) {
                Ok(n) => fmt_num(n),
                Err(_) => "#ERR".to_string(),
            },
        }
    }

    /// Raw text to show when editing a cell (formula prefixed with `=`).
    pub fn cell_input(&self, a1: &str) -> String {
        match self.calc.raw(a1) {
            klerq_calc::Cell::Empty => String::new(),
            klerq_calc::Cell::Text(t) => t,
            klerq_calc::Cell::Number(n) => fmt_num(n),
            klerq_calc::Cell::Formula(f) => format!("={f}"),
        }
    }

    /// Add a text box to slide `index` (undoable).
    pub fn add_text_box(&mut self, index: usize, text: &str) {
        self.slides_stack.execute(
            Box::new(AddShape::new(index, Shape::text_box(text))),
            &mut self.slides,
        );
    }

    /// Run a JS plugin's `transform(input)` once, returning its output or error.
    pub fn run_plugin(&self, source: &str, input: &str) -> Result<String, String> {
        let manifest =
            PluginManifest::from_json(r#"{"name":"inline","version":"0.0.0","permissions":[]}"#)
                .map_err(|e| e.to_string())?;
        let mut host = PluginHost::load(manifest, source).map_err(|e| e.to_string())?;
        host.call_transform(input).map_err(|e| e.to_string())
    }

    /// Locales available to switch between.
    pub fn locales(&self) -> Vec<String> {
        self.locale.available_locales()
    }

    /// Switch UI language.
    pub fn set_locale(&mut self, locale: &str) -> bool {
        self.locale.set_locale(locale).is_ok()
    }

    /// Is the current UI language right-to-left?
    pub fn is_rtl(&self) -> bool {
        self.locale.is_rtl()
    }

    /// Translate a UI key in the current language.
    pub fn t(&self, key: &str) -> String {
        self.locale.t(key)
    }

    /// Save all three documents into `dir` as native Klerq files
    /// (`klerq.klw` / `.klc` / `.kls`). Returns the written paths.
    pub fn save_all(&self, dir: &std::path::Path) -> std::io::Result<Vec<std::path::PathBuf>> {
        use klerq_format::{save_calc, save_slides, save_writer, EXT_CALC, EXT_SLIDES, EXT_WRITER};
        let mut out = Vec::new();
        let jobs: [(&str, String); 3] = [
            (EXT_WRITER, save_writer(&self.writer)),
            (EXT_CALC, save_calc(&self.calc)),
            (EXT_SLIDES, save_slides(&self.slides)),
        ];
        for (ext, data) in jobs {
            let path = dir.join(format!("klerq.{ext}"));
            std::fs::write(&path, data)?;
            out.push(path);
        }
        Ok(out)
    }

    /// Load whichever native Klerq files exist in `dir`, replacing the current
    /// documents. Returns how many documents were loaded.
    pub fn load_all(&mut self, dir: &std::path::Path) -> usize {
        use klerq_format::{load_calc, load_slides, load_writer, EXT_CALC, EXT_SLIDES, EXT_WRITER};
        let mut loaded = 0;
        if let Ok(t) = std::fs::read_to_string(dir.join(format!("klerq.{EXT_WRITER}"))) {
            if let Ok(d) = load_writer(&t) {
                self.writer = d;
                loaded += 1;
            }
        }
        if let Ok(t) = std::fs::read_to_string(dir.join(format!("klerq.{EXT_CALC}"))) {
            if let Ok(d) = load_calc(&t) {
                self.calc = d;
                loaded += 1;
            }
        }
        if let Ok(t) = std::fs::read_to_string(dir.join(format!("klerq.{EXT_SLIDES}"))) {
            if let Ok(d) = load_slides(&t) {
                self.slides = d;
                loaded += 1;
            }
        }
        loaded
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
    fn cell_display_and_input_roundtrip() {
        let mut ws = Workspace::new();
        ws.set_cell("A1", "10");
        ws.set_cell("A2", "20");
        ws.set_cell("A3", "=SUM(A1:A2)");
        ws.set_cell("B1", "hello");
        assert_eq!(ws.cell_display("A3"), "30");
        assert_eq!(ws.cell_input("A3"), "=SUM(A1:A2)");
        assert_eq!(ws.cell_display("B1"), "hello");
        assert_eq!(ws.cell_display("Z9"), "");
    }

    #[test]
    fn cell_display_reports_errors() {
        let mut ws = Workspace::new();
        ws.set_cell("A1", "=A1"); // self cycle
        assert_eq!(ws.cell_display("A1"), "#ERR");
    }

    #[test]
    fn redo_and_bold_through_workspace() {
        let mut ws = Workspace::new();
        ws.write_paragraph("hi there");
        ws.toggle_bold(0);
        assert!(ws.writer.paragraphs[0].runs[0].style.bold);
        assert!(ws.undo_writer());
        assert!(!ws.writer.paragraphs[0].runs[0].style.bold);
        assert!(ws.redo_writer());
        assert!(ws.writer.paragraphs[0].runs[0].style.bold);
    }

    #[test]
    fn add_text_box_to_slide() {
        let mut ws = Workspace::new();
        ws.add_slide("S1");
        ws.add_text_box(0, "box text");
        assert_eq!(ws.slides.slides[0].shapes[0].text, "box text");
    }

    #[test]
    fn run_plugin_through_workspace() {
        let ws = Workspace::new();
        let out = ws
            .run_plugin(
                "function transform(s){return s.split('').reverse().join('');}",
                "abc",
            )
            .unwrap();
        assert_eq!(out, "cba");
    }

    #[test]
    fn locale_helpers() {
        let mut ws = Workspace::new();
        assert!(ws.locales().contains(&"fr-FR".to_string()));
        assert!(ws.set_locale("fr-FR"));
        assert_eq!(ws.t("action-save"), "Enregistrer");
        assert!(!ws.is_rtl());
    }

    #[test]
    fn fmt_num_trims_integers() {
        assert_eq!(fmt_num(30.0), "30");
        assert_eq!(fmt_num(2.5), "2.5");
        assert_eq!(fmt_num(-3.0), "-3");
    }

    #[test]
    fn save_all_then_load_all_roundtrips() {
        let dir = std::env::temp_dir().join(format!("klerq-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut ws = Workspace::new();
        ws.write_paragraph("persisted paragraph");
        ws.set_cell("A1", "7");
        ws.set_cell("A2", "=A1*3");
        ws.add_slide("Saved deck");
        let paths = ws.save_all(&dir).unwrap();
        assert_eq!(paths.len(), 3);

        // Fresh session loads the files back.
        let mut ws2 = Workspace::new();
        assert_eq!(ws2.load_all(&dir), 3);
        assert_eq!(ws2.writer.plain_text(), "persisted paragraph");
        assert_eq!(ws2.calc.eval_number("A2").unwrap(), 21.0);
        assert_eq!(ws2.slides.slides[0].title, "Saved deck");

        std::fs::remove_dir_all(&dir).ok();
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
