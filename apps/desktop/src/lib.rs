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
use klerq_writer::{
    Align, InsertParagraph, SetAlign, TextDocument, ToggleBold, ToggleItalic, ToggleUnderline,
};

/// Format a float for display: integers without a trailing `.0`.
pub fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// Every bundled locale, embedded at build time. The first entry is the
/// fallback locale. Add a language by dropping a `.ftl` file under `locales/`
/// and adding one line here.
pub const LOCALES: &[(&str, &str)] = &[
    ("en-US", include_str!("../../../locales/en-US/klerq.ftl")),
    ("fr-FR", include_str!("../../../locales/fr-FR/klerq.ftl")),
    ("es-ES", include_str!("../../../locales/es-ES/klerq.ftl")),
    ("de-DE", include_str!("../../../locales/de-DE/klerq.ftl")),
    ("it-IT", include_str!("../../../locales/it-IT/klerq.ftl")),
    ("pt-BR", include_str!("../../../locales/pt-BR/klerq.ftl")),
    ("ru-RU", include_str!("../../../locales/ru-RU/klerq.ftl")),
    ("ja-JP", include_str!("../../../locales/ja-JP/klerq.ftl")),
    ("zh-CN", include_str!("../../../locales/zh-CN/klerq.ftl")),
    ("ko-KR", include_str!("../../../locales/ko-KR/klerq.ftl")),
    ("hi-IN", include_str!("../../../locales/hi-IN/klerq.ftl")),
    ("tr-TR", include_str!("../../../locales/tr-TR/klerq.ftl")),
    ("ar-SA", include_str!("../../../locales/ar-SA/klerq.ftl")),
    ("he-IL", include_str!("../../../locales/he-IL/klerq.ftl")),
];

/// Message keys every locale must define (used by the parity test).
pub const UI_KEYS: &[&str] = &[
    "app-title",
    "app-tagline",
    "menu-file",
    "menu-edit",
    "menu-view",
    "menu-help",
    "action-new",
    "action-open",
    "action-save",
    "action-undo",
    "action-redo",
    "app-writer",
    "app-calc",
    "app-slides",
    "status-words",
    "status-ready",
    "greeting",
];

/// A live Klerq session: one document per app plus localization.
pub struct Workspace {
    pub locale: Localizer,
    pub writer: TextDocument,
    pub calc: Sheet,
    pub slides: Presentation,
    /// AI provider configuration (key, model, base URL).
    pub ai: klerq_ai::AiConfig,
    /// Real-time collaboration session (CRDT) for Calc.
    pub collab: klerq_sync::Session,
    writer_stack: CommandStack<TextDocument>,
    slides_stack: CommandStack<Presentation>,
}

/// Convert a zero-based (col,row) to an A1 address.
fn col_row_to_a1(col: u32, row: u32) -> String {
    let mut c = col + 1;
    let mut letters = String::new();
    while c > 0 {
        let rem = (c - 1) % 26;
        letters.insert(0, (b'A' + rem as u8) as char);
        c = (c - 1) / 26;
    }
    format!("{letters}{}", row + 1)
}

impl Workspace {
    /// Build a workspace with every bundled locale registered (fallback first).
    pub fn new() -> Self {
        let (fb_loc, fb_src) = LOCALES[0];
        let mut locale = Localizer::new(fb_loc, fb_src).expect("valid fallback ftl");
        for (loc, src) in &LOCALES[1..] {
            locale.add_locale(loc, src).expect("valid ftl");
        }
        Self {
            locale,
            writer: TextDocument::new(),
            calc: Sheet::new(),
            slides: Presentation::new(),
            ai: klerq_ai::AiConfig::new(klerq_ai::Provider::OpenAI, ""),
            collab: klerq_sync::Session::new(std::process::id() as u64),
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

    /// Toggle italic on a Writer paragraph (undoable).
    pub fn toggle_italic(&mut self, index: usize) {
        self.writer_stack
            .execute(Box::new(ToggleItalic::new(index)), &mut self.writer);
    }

    /// Toggle underline on a Writer paragraph (undoable).
    pub fn toggle_underline(&mut self, index: usize) {
        self.writer_stack
            .execute(Box::new(ToggleUnderline::new(index)), &mut self.writer);
    }

    /// Set alignment of a Writer paragraph (undoable).
    pub fn set_align(&mut self, index: usize, align: Align) {
        self.writer_stack
            .execute(Box::new(SetAlign::new(index, align)), &mut self.writer);
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

    /// Export all three documents to MS Office files in `dir`
    /// (`klerq.docx` / `klerq.xlsx` / `klerq.pptx`). Returns the written paths.
    pub fn export_ooxml(&self, dir: &std::path::Path) -> std::io::Result<Vec<std::path::PathBuf>> {
        use klerq_ooxml::{write_docx, write_pptx, write_xlsx, EXT_DOCX, EXT_PPTX, EXT_XLSX};
        let jobs: [(&str, Vec<u8>); 3] = [
            (EXT_DOCX, write_docx(&self.writer)),
            (EXT_XLSX, write_xlsx(&self.calc)),
            (EXT_PPTX, write_pptx(&self.slides)),
        ];
        let mut out = Vec::new();
        for (ext, data) in jobs {
            let path = dir.join(format!("klerq.{ext}"));
            std::fs::write(&path, data)?;
            out.push(path);
        }
        Ok(out)
    }

    /// Import whichever MS Office files exist in `dir`, replacing the matching
    /// documents. Returns how many were imported.
    pub fn import_ooxml(&mut self, dir: &std::path::Path) -> usize {
        use klerq_ooxml::{read_docx, read_pptx, read_xlsx, EXT_DOCX, EXT_PPTX, EXT_XLSX};
        let mut n = 0;
        if let Ok(b) = std::fs::read(dir.join(format!("klerq.{EXT_DOCX}"))) {
            if let Ok(d) = read_docx(&b) {
                self.writer = d;
                n += 1;
            }
        }
        if let Ok(b) = std::fs::read(dir.join(format!("klerq.{EXT_XLSX}"))) {
            if let Ok(d) = read_xlsx(&b) {
                self.calc = d;
                n += 1;
            }
        }
        if let Ok(b) = std::fs::read(dir.join(format!("klerq.{EXT_PPTX}"))) {
            if let Ok(d) = read_pptx(&b) {
                self.slides = d;
                n += 1;
            }
        }
        n
    }

    /// Persist the AI configuration (provider, key, model) to `dir/klerq-ai.json`.
    pub fn save_ai(&self, dir: &std::path::Path) -> Result<(), String> {
        self.ai
            .save_to(&dir.join("klerq-ai.json"))
            .map_err(|e| e.to_string())
    }

    /// Load AI configuration from `dir/klerq-ai.json` if present.
    pub fn load_ai(&mut self, dir: &std::path::Path) -> bool {
        match klerq_ai::AiConfig::load_from(&dir.join("klerq-ai.json")) {
            Ok(cfg) => {
                self.ai = cfg;
                true
            }
            Err(_) => false,
        }
    }

    /// Ask the configured AI to build a Calc formula for a natural-language
    /// request, grounded in the real function library.
    pub fn suggest_formula(&self, prompt: &str) -> Result<String, String> {
        klerq_ai::suggest_formula(&self.ai, prompt, klerq_calc::FUNCTION_NAMES)
            .map_err(|e| e.to_string())
    }

    /// Free-form AI chat (returns the assistant's reply).
    pub fn ai_chat(&self, prompt: &str) -> Result<String, String> {
        klerq_ai::chat(&self.ai, None, &[klerq_ai::Msg::user(prompt)]).map_err(|e| e.to_string())
    }

    /// Import CSV from a URL (a "data connection") into Calc, replacing the sheet.
    pub fn import_csv_url(&mut self, url: &str) -> Result<usize, String> {
        let body = klerq_ai::http_get(url).map_err(|e| e.to_string())?;
        self.calc = klerq_format::calc_from_csv(&body);
        Ok(body.lines().filter(|l| !l.is_empty()).count())
    }

    /// Import CSV text (paste / file) into Calc, replacing the sheet.
    pub fn import_csv_text(&mut self, csv: &str) -> usize {
        self.calc = klerq_format::calc_from_csv(csv);
        csv.lines().filter(|l| !l.is_empty()).count()
    }

    /// Import JSON text (array of objects, `{data:[…]}`, …) into Calc.
    pub fn import_json_text(&mut self, json: &str) -> Result<usize, String> {
        let sheet = klerq_format::json_to_sheet(json).map_err(|e| e.to_string())?;
        self.calc = sheet;
        Ok(self.calc.extent().map(|(_, r)| r as usize).unwrap_or(0))
    }

    /// Import XML text (records = children of the root) into Calc.
    pub fn import_xml_text(&mut self, xml: &str) -> Result<usize, String> {
        let sheet = klerq_format::xml_to_sheet(xml).map_err(|e| e.to_string())?;
        self.calc = sheet;
        Ok(self.calc.extent().map(|(_, r)| r as usize).unwrap_or(0))
    }

    /// Import from a URL, auto-detecting CSV / JSON / XML by content.
    pub fn import_url(&mut self, url: &str) -> Result<usize, String> {
        let body = klerq_ai::http_get(url).map_err(|e| e.to_string())?;
        let trimmed = body.trim_start();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            self.import_json_text(&body)
        } else if trimmed.starts_with('<') {
            self.import_xml_text(&body)
        } else {
            Ok(self.import_csv_text(&body))
        }
    }

    /// This replica's collaboration id.
    pub fn collab_site(&self) -> u64 {
        self.collab.site
    }

    /// Edit a cell AND record a broadcastable collaboration op.
    pub fn collab_set_cell(&mut self, a1: &str, input: &str) {
        self.calc.set(a1, input);
        if let Some(addr) = klerq_calc::parse_a1(a1) {
            let value = if input.is_empty() {
                None
            } else {
                Some(input.to_string())
            };
            self.collab.set_cell(addr.col, addr.row, value);
        }
    }

    /// Serialize local edits to JSON to send to collaborators.
    pub fn collab_export(&mut self) -> String {
        self.collab.export_ops()
    }

    /// Apply collaborators' edits (JSON ops) and mirror them into the live sheet.
    pub fn collab_import(&mut self, json: &str) -> Result<usize, String> {
        let n = self.collab.import_ops(json)?;
        let cells: Vec<(u32, u32, String)> = self
            .collab
            .calc
            .cells()
            .map(|(c, r, s)| (c, r, s.to_string()))
            .collect();
        for (c, r, s) in cells {
            self.calc.set(&col_row_to_a1(c, r), &s);
        }
        Ok(n)
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
    fn writer_formatting_through_workspace() {
        let mut ws = Workspace::new();
        ws.write_paragraph("format me");
        ws.toggle_italic(0);
        ws.toggle_underline(0);
        ws.set_align(0, Align::Center);
        let run = &ws.writer.paragraphs[0].runs[0];
        assert!(run.style.italic);
        assert!(run.style.underline);
        assert_eq!(ws.writer.paragraphs[0].align, Align::Center);
        assert!(ws.undo_writer()); // undo align
        assert_eq!(ws.writer.paragraphs[0].align, Align::Left);
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
    fn every_bundled_locale_loads() {
        let ws = Workspace::new();
        // new() would panic on any invalid .ftl; assert the full set registered.
        assert_eq!(ws.locales().len(), LOCALES.len());
        assert!(ws.locales().len() >= 14);
    }

    #[test]
    fn all_locales_define_every_key() {
        // Parity: each locale's source must define every UI key — no gaps.
        for (loc, src) in LOCALES {
            for key in UI_KEYS {
                assert!(
                    src.contains(&format!("{key} =")),
                    "locale {loc} missing key {key}"
                );
            }
        }
    }

    #[test]
    fn rtl_locales_flip() {
        let mut ws = Workspace::new();
        assert!(!ws.is_rtl());
        ws.set_locale("ar-SA");
        assert!(ws.is_rtl());
        ws.set_locale("he-IL");
        assert!(ws.is_rtl());
        ws.set_locale("de-DE");
        assert!(!ws.is_rtl());
    }

    #[test]
    fn sample_translations_resolve() {
        let mut ws = Workspace::new();
        ws.set_locale("de-DE");
        assert_eq!(ws.t("action-save"), "Speichern");
        ws.set_locale("ja-JP");
        assert_eq!(ws.t("action-open"), "開く");
        ws.set_locale("zh-CN");
        assert_eq!(ws.t("menu-file"), "文件");
    }

    #[test]
    fn ooxml_export_import_roundtrips() {
        let dir = std::env::temp_dir().join(format!("klerq-ooxml-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut ws = Workspace::new();
        ws.write_paragraph("office interop works");
        ws.set_cell("A1", "5");
        ws.set_cell("A2", "=A1*4");
        ws.add_slide("Deck title");
        assert_eq!(ws.export_ooxml(&dir).unwrap().len(), 3);

        let mut ws2 = Workspace::new();
        assert_eq!(ws2.import_ooxml(&dir), 3);
        assert_eq!(ws2.writer.plain_text(), "office interop works");
        assert_eq!(ws2.calc.eval_number("A2").unwrap(), 20.0);
        assert_eq!(ws2.slides.slides[0].title, "Deck title");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ai_config_persists_through_workspace() {
        let dir = std::env::temp_dir().join(format!("klerq-ws-ai-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut ws = Workspace::new();
        ws.ai = klerq_ai::AiConfig::new(klerq_ai::Provider::Gemini, "g-key");
        ws.ai.model = "gemini-2.0-flash".into();
        ws.save_ai(&dir).unwrap();

        let mut ws2 = Workspace::new();
        assert!(ws2.load_ai(&dir));
        assert_eq!(ws2.ai.provider, klerq_ai::Provider::Gemini);
        assert_eq!(ws2.ai.api_key, "g-key");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn suggest_formula_without_key_errors() {
        let ws = Workspace::new(); // empty key
        assert!(ws.suggest_formula("sum of A1 to A10").is_err());
    }

    #[test]
    fn csv_text_import_into_calc() {
        let mut ws = Workspace::new();
        let rows = ws.import_csv_text("Item,Qty\nWidgets,10\nTotal,=B2+5");
        assert_eq!(rows, 3);
        assert_eq!(ws.calc.eval_number("B2").unwrap(), 10.0);
        assert_eq!(ws.calc.eval_number("B3").unwrap(), 15.0);
    }

    #[test]
    fn json_text_import_into_calc() {
        let mut ws = Workspace::new();
        ws.import_json_text(r#"[{"city":"Paris","pop":2100000},{"city":"Rome","pop":2800000}]"#)
            .unwrap();
        assert_eq!(ws.calc.raw("A1"), klerq_calc::Cell::Text("city".into()));
        assert_eq!(ws.calc.eval_number("B2").unwrap(), 2100000.0);
        assert_eq!(ws.calc.eval_number("B3").unwrap(), 2800000.0);
    }

    #[test]
    fn xml_text_import_into_calc() {
        let mut ws = Workspace::new();
        ws.import_xml_text(
            "<rows><r><name>Ada</name><n>1</n></r><r><name>Bo</name><n>2</n></r></rows>",
        )
        .unwrap();
        assert_eq!(ws.calc.raw("A1"), klerq_calc::Cell::Text("name".into()));
        assert_eq!(ws.calc.eval_number("B3").unwrap(), 2.0);
    }

    #[test]
    fn collab_edits_sync_between_two_workspaces() {
        let mut alice = Workspace::new();
        let mut bob = Workspace::new();

        alice.collab_set_cell("A1", "10");
        alice.collab_set_cell("A2", "=A1*2");
        let wire = alice.collab_export();

        let n = bob.collab_import(&wire).unwrap();
        assert_eq!(n, 2);
        // Bob's live sheet reflects Alice's edits and recomputes formulas.
        assert_eq!(bob.calc.eval_number("A1").unwrap(), 10.0);
        assert_eq!(bob.calc.eval_number("A2").unwrap(), 20.0);
    }

    #[test]
    fn a1_conversion_roundtrips() {
        assert_eq!(col_row_to_a1(0, 0), "A1");
        assert_eq!(col_row_to_a1(26, 0), "AA1");
        assert_eq!(col_row_to_a1(1, 4), "B5");
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
