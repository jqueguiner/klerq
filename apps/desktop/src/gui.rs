//! Klerq desktop GUI — an `egui`/`eframe` front-end over the tested
//! [`klerq_desktop::Workspace`]. All document logic lives in the (unit-tested)
//! library; this file is a thin, stateful view layer.
//!
//! Run with: `cargo run --bin klerq-gui`

use eframe::egui;
use klerq_desktop::Workspace;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 740.0])
            .with_min_inner_size([820.0, 560.0])
            .with_title("Klerq"),
        ..Default::default()
    };
    eframe::run_native(
        "Klerq",
        options,
        Box::new(|cc| Ok(Box::new(KlerqApp::new(cc)))),
    )
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Writer,
    Calc,
    Slides,
    Plugins,
}

const COLS: usize = 8; // A..H
const ROWS: usize = 20; // 1..20

const SAMPLE_PLUGIN: &str = "// Community plugin (JavaScript). Define transform(text).\n\
// The sandbox has no fetch / require / filesystem.\n\
function transform(text) {\n\
    return text.toUpperCase();\n\
}";

struct KlerqApp {
    ws: Workspace,
    tab: Tab,
    dark: bool,
    // Writer
    new_para: String,
    sel_para: usize,
    // Calc
    sel_col: usize,
    sel_row: usize,
    cell_buf: String,
    // Slides
    sel_slide: usize,
    slide_title: String,
    shape_text: String,
    // Plugins
    plugin_src: String,
    plugin_input: String,
    plugin_out: String,
    // File I/O feedback
    file_status: String,
}

impl KlerqApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut ws = Workspace::new();
        // Seed with a little content so the app looks alive on first launch.
        ws.write_paragraph("Welcome to Klerq — the Rust-native office suite.");
        ws.write_paragraph("Edit here, crunch numbers in Calc, present in Slides.");
        ws.set_cell("A1", "Item");
        ws.set_cell("B1", "Qty");
        ws.set_cell("A2", "Widgets");
        ws.set_cell("B2", "10");
        ws.set_cell("A3", "Gadgets");
        ws.set_cell("B3", "15");
        ws.set_cell("A4", "Total");
        ws.set_cell("B4", "=SUM(B2:B3)");
        ws.add_slide("Klerq");
        ws.add_text_box(0, "One suite. Every platform.");
        ws.add_slide("Roadmap");

        let app = Self {
            ws,
            tab: Tab::Writer,
            dark: true,
            new_para: String::new(),
            sel_para: 0,
            sel_col: 0,
            sel_row: 0,
            cell_buf: String::new(),
            sel_slide: 0,
            slide_title: String::new(),
            shape_text: String::new(),
            plugin_src: SAMPLE_PLUGIN.to_string(),
            plugin_input: "make me loud".to_string(),
            plugin_out: String::new(),
            file_status: String::new(),
        };
        apply_theme(&cc.egui_ctx, app.dark);
        app
    }

    fn addr(&self, col: usize, row: usize) -> String {
        format!("{}{}", (b'A' + col as u8) as char, row + 1)
    }
}

fn accent() -> egui::Color32 {
    egui::Color32::from_rgb(99, 102, 241) // indigo-500
}

fn apply_theme(ctx: &egui::Context, dark: bool) {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.selection.bg_fill = accent();
    visuals.hyperlink_color = accent();
    visuals.widgets.hovered.rounding = egui::Rounding::same(8.0);
    visuals.widgets.active.rounding = egui::Rounding::same(8.0);
    visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
    visuals.window_rounding = egui::Rounding::same(10.0);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    ctx.set_style(style);
}

impl eframe::App for KlerqApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.menu_bar(ctx);
        self.side_rail(ctx);
        self.status_bar(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            let rtl = self.ws.is_rtl();
            let layout = if rtl {
                egui::Layout::top_down(egui::Align::Max)
            } else {
                egui::Layout::top_down(egui::Align::Min)
            };
            ui.with_layout(layout, |ui| match self.tab {
                Tab::Writer => self.writer_view(ui),
                Tab::Calc => self.calc_view(ui),
                Tab::Slides => self.slides_view(ui),
                Tab::Plugins => self.plugins_view(ui),
            });
        });
    }
}

impl KlerqApp {
    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button(self.ws.t("menu-file"), |ui| {
                    let _ = ui.button(self.ws.t("action-new"));
                    if ui.button(self.ws.t("action-open")).clicked() {
                        let n = self.ws.load_all(std::path::Path::new("."));
                        self.file_status = format!("Opened {n} document(s) from ./klerq.*");
                        ui.close_menu();
                    }
                    if ui.button(self.ws.t("action-save")).clicked() {
                        self.file_status = match self.ws.save_all(std::path::Path::new(".")) {
                            Ok(p) => format!("Saved {} files (klerq.klw/.klc/.kls)", p.len()),
                            Err(e) => format!("Save failed: {e}"),
                        };
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Export to MS Office…").clicked() {
                        self.file_status = match self.ws.export_ooxml(std::path::Path::new(".")) {
                            Ok(p) => format!("Exported {} files (klerq.docx/.xlsx/.pptx)", p.len()),
                            Err(e) => format!("Export failed: {e}"),
                        };
                        ui.close_menu();
                    }
                    if ui.button("Import from MS Office…").clicked() {
                        let n = self.ws.import_ooxml(std::path::Path::new("."));
                        self.file_status = format!("Imported {n} MS Office file(s) from ./klerq.*");
                        ui.close_menu();
                    }
                });
                ui.menu_button(self.ws.t("menu-edit"), |ui| {
                    if ui.button(self.ws.t("action-undo")).clicked() {
                        self.ws.undo_writer();
                    }
                    if ui.button(self.ws.t("action-redo")).clicked() {
                        self.ws.redo_writer();
                    }
                });
                ui.menu_button(self.ws.t("menu-view"), |ui| {
                    if ui.button("Toggle dark / light").clicked() {
                        self.dark = !self.dark;
                        apply_theme(ui.ctx(), self.dark);
                    }
                });
                ui.menu_button(self.ws.t("menu-help"), |ui| {
                    ui.label("Klerq — MIT OR Apache-2.0");
                    ui.hyperlink_to(
                        "github.com/jqueguiner/klerq",
                        "https://github.com/jqueguiner/klerq",
                    );
                });
            });
        });
    }

    fn side_rail(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("rail")
            .exact_width(184.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("Klerq")
                        .size(24.0)
                        .strong()
                        .color(accent()),
                );
                ui.label(egui::RichText::new(self.ws.t("app-tagline")).small().weak());
                ui.add_space(12.0);

                let items = [
                    (Tab::Writer, "📝", self.ws.t("app-writer")),
                    (Tab::Calc, "🔢", self.ws.t("app-calc")),
                    (Tab::Slides, "🖼", self.ws.t("app-slides")),
                    (Tab::Plugins, "🧩", "Plugins".to_string()),
                ];
                for (tab, icon, label) in items {
                    let selected = self.tab == tab;
                    let text = egui::RichText::new(format!("{icon}  {label}")).size(15.0);
                    if ui
                        .add_sized(
                            [ui.available_width(), 34.0],
                            egui::SelectableLabel::new(selected, text),
                        )
                        .clicked()
                    {
                        self.tab = tab;
                    }
                }

                ui.add_space(16.0);
                ui.separator();
                ui.label(egui::RichText::new("Language").small().weak());
                let current = self.ws.locale.current_locale().to_string();
                egui::ComboBox::from_id_source("locale")
                    .selected_text(current.clone())
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for loc in self.ws.locales() {
                            if ui.selectable_label(loc == current, &loc).clicked() {
                                self.ws.set_locale(&loc);
                            }
                        }
                    });
                if self.ws.is_rtl() {
                    ui.label(egui::RichText::new("RTL layout").small().color(accent()));
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.add_space(8.0);
                    if ui
                        .button(if self.dark { "☀ Light" } else { "🌙 Dark" })
                        .clicked()
                    {
                        self.dark = !self.dark;
                        apply_theme(ui.ctx(), self.dark);
                    }
                });
            });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("● {}", self.ws.t("status-ready")))
                        .color(egui::Color32::from_rgb(52, 199, 89)),
                );
                ui.separator();
                ui.label(self.ws.status()); // localized word count
                if !self.file_status.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new(&self.file_status).weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(self.ws.locale.current_locale()).weak());
                });
            });
        });
    }

    // ---- Writer ----
    fn writer_view(&mut self, ui: &mut egui::Ui) {
        ui.heading(format!("📝 {}", self.ws.t("app-writer")));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.new_para)
                    .hint_text("Type a new paragraph…")
                    .desired_width(360.0),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("＋ Add").clicked() || enter) && !self.new_para.trim().is_empty() {
                let text = std::mem::take(&mut self.new_para);
                self.ws.write_paragraph(&text);
                self.sel_para = self.ws.writer.paragraphs.len().saturating_sub(1);
            }
            ui.separator();
            ui.add_enabled_ui(self.ws.can_undo_writer(), |ui| {
                if ui.button("↶ Undo").clicked() {
                    self.ws.undo_writer();
                }
            });
            ui.add_enabled_ui(self.ws.can_redo_writer(), |ui| {
                if ui.button("↷ Redo").clicked() {
                    self.ws.redo_writer();
                }
            });
            if ui.button("𝗕 Bold").clicked() && self.sel_para < self.ws.writer.paragraphs.len() {
                self.ws.toggle_bold(self.sel_para);
            }
        });
        ui.add_space(8.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            let count = self.ws.writer.paragraphs.len();
            for i in 0..count {
                let para = &self.ws.writer.paragraphs[i];
                let bold = para.runs.first().map(|r| r.style.bold).unwrap_or(false);
                let selected = i == self.sel_para;
                egui::Frame::group(ui.style())
                    .fill(if selected {
                        accent().linear_multiply(0.12)
                    } else {
                        ui.style().visuals.faint_bg_color
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let mut text = egui::RichText::new(para.text()).size(15.0);
                        if bold {
                            text = text.strong();
                        }
                        if ui
                            .add(egui::Label::new(text).sense(egui::Sense::click()))
                            .clicked()
                        {
                            self.sel_para = i;
                        }
                    });
            }
        });
    }

    // ---- Calc ----
    fn calc_view(&mut self, ui: &mut egui::Ui) {
        ui.heading(format!("🔢 {}", self.ws.t("app-calc")));
        ui.add_space(6.0);
        let sel_addr = self.addr(self.sel_col, self.sel_row);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&sel_addr).monospace().strong());
            ui.label("=");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.cell_buf)
                    .hint_text("value or =SUM(A1:A2)")
                    .desired_width(420.0),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("✓ Set").clicked() || enter {
                self.ws.set_cell(&sel_addr, &self.cell_buf);
            }
        });
        ui.add_space(8.0);
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("calc-grid")
                .striped(true)
                .min_col_width(72.0)
                .spacing(egui::vec2(2.0, 2.0))
                .show(ui, |ui| {
                    ui.label(""); // corner
                    for c in 0..COLS {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new(((b'A' + c as u8) as char).to_string())
                                    .strong(),
                            );
                        });
                    }
                    ui.end_row();
                    for r in 0..ROWS {
                        ui.label(egui::RichText::new((r + 1).to_string()).strong());
                        for c in 0..COLS {
                            let addr = self.addr(c, r);
                            let selected = c == self.sel_col && r == self.sel_row;
                            let disp = self.ws.cell_display(&addr);
                            let resp = ui.add_sized(
                                [72.0, 22.0],
                                egui::SelectableLabel::new(
                                    selected,
                                    egui::RichText::new(disp).monospace(),
                                ),
                            );
                            if resp.clicked() {
                                self.sel_col = c;
                                self.sel_row = r;
                                self.cell_buf = self.ws.cell_input(&addr);
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    }

    // ---- Slides ----
    fn slides_view(&mut self, ui: &mut egui::Ui) {
        ui.heading(format!("🖼 {}", self.ws.t("app-slides")));
        ui.add_space(6.0);
        ui.horizontal_top(|ui| {
            // Slide list
            ui.vertical(|ui| {
                ui.set_width(210.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.slide_title)
                            .hint_text("Slide title")
                            .desired_width(130.0),
                    );
                    if ui.button("＋").clicked() && !self.slide_title.trim().is_empty() {
                        let t = std::mem::take(&mut self.slide_title);
                        self.ws.add_slide(&t);
                        self.sel_slide = self.ws.slides.len().saturating_sub(1);
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for i in 0..self.ws.slides.len() {
                        let title = &self.ws.slides.slides[i].title;
                        let label = format!("{}. {}", i + 1, title);
                        if ui
                            .add_sized(
                                [ui.available_width(), 28.0],
                                egui::SelectableLabel::new(i == self.sel_slide, label),
                            )
                            .clicked()
                        {
                            self.sel_slide = i;
                        }
                    }
                });
            });
            ui.separator();
            // Slide canvas
            ui.vertical(|ui| {
                if self.sel_slide < self.ws.slides.len() {
                    let idx = self.sel_slide;
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.shape_text)
                                .hint_text("Text box content")
                                .desired_width(260.0),
                        );
                        if ui.button("＋ Text box").clicked() && !self.shape_text.trim().is_empty()
                        {
                            let t = std::mem::take(&mut self.shape_text);
                            self.ws.add_text_box(idx, &t);
                        }
                    });
                    ui.add_space(6.0);
                    let canvas = egui::vec2(ui.available_width().min(620.0), 340.0);
                    egui::Frame::canvas(ui.style())
                        .fill(egui::Color32::from_gray(if self.dark { 30 } else { 245 }))
                        .rounding(egui::Rounding::same(10.0))
                        .show(ui, |ui| {
                            ui.set_min_size(canvas);
                            ui.vertical(|ui| {
                                ui.add_space(10.0);
                                ui.label(
                                    egui::RichText::new(&self.ws.slides.slides[idx].title)
                                        .size(26.0)
                                        .strong()
                                        .color(accent()),
                                );
                                ui.add_space(8.0);
                                for shape in &self.ws.slides.slides[idx].shapes {
                                    egui::Frame::group(ui.style())
                                        .stroke(egui::Stroke::new(1.0_f32, accent()))
                                        .show(ui, |ui| {
                                            ui.label(egui::RichText::new(&shape.text).size(16.0));
                                        });
                                }
                            });
                        });
                } else {
                    ui.label("No slide selected.");
                }
            });
        });
    }

    // ---- Plugins ----
    fn plugins_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("🧩 Plugins — community JavaScript");
        ui.label(
            egui::RichText::new("Sandboxed (boa engine): no fetch, no require, no filesystem. Define transform(text).")
                .small()
                .weak(),
        );
        ui.add_space(8.0);
        ui.add(
            egui::TextEdit::multiline(&mut self.plugin_src)
                .code_editor()
                .desired_rows(10)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Input:");
            ui.add(egui::TextEdit::singleline(&mut self.plugin_input).desired_width(320.0));
            if ui
                .add(egui::Button::new(egui::RichText::new("▶ Run").strong()).fill(accent()))
                .clicked()
            {
                self.plugin_out = match self.ws.run_plugin(&self.plugin_src, &self.plugin_input) {
                    Ok(out) => out,
                    Err(e) => format!("error: {e}"),
                };
            }
        });
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Output").small().weak());
        egui::Frame::group(ui.style())
            .fill(ui.style().visuals.extreme_bg_color)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(egui::RichText::new(&self.plugin_out).monospace());
            });
    }
}
