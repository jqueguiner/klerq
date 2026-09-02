//! Klerq desktop GUI — an `egui`/`eframe` front-end over the tested
//! [`klerq_desktop::Workspace`]. All document logic lives in the (unit-tested)
//! library; this file is a thin, stateful view layer.
//!
//! Run with: `cargo run --bin klerq-gui`

use eframe::egui;
use klerq_ai::Provider;
use klerq_calc::FUNCTION_NAMES;
use klerq_desktop::Workspace;
use klerq_writer::Align;

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
    Ai,
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
    // AI
    ai_prompt: String,
    ai_answer: String,
    ai_status: String,
    csv_url: String,
    data_paste: String,
    // Collaboration
    collab_buf: String,
    collab_status: String,
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
        // Restore saved AI provider config, if any.
        ws.load_ai(std::path::Path::new("."));

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
            ai_prompt: "sum of A1 to A10".to_string(),
            ai_answer: String::new(),
            ai_status: String::new(),
            csv_url: String::new(),
            data_paste: String::new(),
            collab_buf: String::new(),
            collab_status: String::new(),
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
                Tab::Ai => self.ai_view(ui),
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
        // Mirror the rail to the right for right-to-left languages.
        let panel = if self.ws.is_rtl() {
            egui::SidePanel::right("rail")
        } else {
            egui::SidePanel::left("rail")
        };
        panel.exact_width(184.0).resizable(false).show(ctx, |ui| {
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
                (Tab::Ai, "🤖", "AI".to_string()),
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
            let has_sel = self.sel_para < self.ws.writer.paragraphs.len();
            if ui.button("𝗕 Bold").clicked() && has_sel {
                self.ws.toggle_bold(self.sel_para);
            }
            if ui.button("𝘐 Italic").clicked() && has_sel {
                self.ws.toggle_italic(self.sel_para);
            }
            if ui.button("U̲ Under").clicked() && has_sel {
                self.ws.toggle_underline(self.sel_para);
            }
            ui.separator();
            if ui.button("⯇").on_hover_text("Align left").clicked() && has_sel {
                self.ws.set_align(self.sel_para, Align::Left);
            }
            if ui.button("≡").on_hover_text("Align center").clicked() && has_sel {
                self.ws.set_align(self.sel_para, Align::Center);
            }
            if ui.button("⯈").on_hover_text("Align right").clicked() && has_sel {
                self.ws.set_align(self.sel_para, Align::Right);
            }
        });
        ui.add_space(8.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            let count = self.ws.writer.paragraphs.len();
            for i in 0..count {
                let para = &self.ws.writer.paragraphs[i];
                let style = para
                    .runs
                    .first()
                    .map(|r| r.style.clone())
                    .unwrap_or_default();
                let align = para.align;
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
                        if style.bold {
                            text = text.strong();
                        }
                        if style.italic {
                            text = text.italics();
                        }
                        if style.underline {
                            text = text.underline();
                        }
                        let cross = match align {
                            Align::Left | Align::Justify => egui::Align::LEFT,
                            Align::Center => egui::Align::Center,
                            Align::Right => egui::Align::RIGHT,
                        };
                        let clicked = ui
                            .with_layout(egui::Layout::top_down(cross), |ui| {
                                ui.add(egui::Label::new(text).sense(egui::Sense::click()))
                                    .clicked()
                            })
                            .inner;
                        if clicked {
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
        egui::CollapsingHeader::new(format!("ƒ Functions ({})", FUNCTION_NAMES.len()))
            .id_source("fn-palette")
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for name in FUNCTION_NAMES {
                        if ui
                            .selectable_label(false, egui::RichText::new(*name).monospace().small())
                            .on_hover_text("Click to start this formula")
                            .clicked()
                        {
                            self.cell_buf = format!("={name}(");
                        }
                    }
                });
            });
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
                // Route through collab so the edit is broadcastable.
                self.ws.collab_set_cell(&sel_addr, &self.cell_buf);
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

    // ---- AI ----
    fn ai_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("🤖 AI assistant");
        ui.label(
            egui::RichText::new("Configure a provider, then ask for a formula in plain language.")
                .small()
                .weak(),
        );
        ui.add_space(8.0);

        // ----- Provider settings -----
        egui::CollapsingHeader::new("Provider settings")
            .default_open(!self.ws.ai.has_key())
            .show(ui, |ui| {
                egui::Grid::new("ai-settings")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Provider");
                        egui::ComboBox::from_id_source("ai-provider")
                            .selected_text(self.ws.ai.provider.label())
                            .show_ui(ui, |ui| {
                                for p in Provider::all() {
                                    if ui
                                        .selectable_label(self.ws.ai.provider == p, p.label())
                                        .clicked()
                                    {
                                        self.ws.ai.provider = p;
                                        self.ws.ai.model = p.default_model().to_string();
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label("Model");
                        ui.text_edit_singleline(&mut self.ws.ai.model);
                        ui.end_row();

                        ui.label("API key");
                        ui.add(egui::TextEdit::singleline(&mut self.ws.ai.api_key).password(true));
                        ui.end_row();

                        ui.label("Custom base URL");
                        let mut base = self.ws.ai.base_url.clone().unwrap_or_default();
                        let hint = self.ws.ai.provider.default_base();
                        if ui
                            .add(egui::TextEdit::singleline(&mut base).hint_text(hint))
                            .changed()
                        {
                            self.ws.ai.base_url = if base.trim().is_empty() {
                                None
                            } else {
                                Some(base)
                            };
                        }
                        ui.end_row();
                    });
                ui.horizontal(|ui| {
                    if ui.button("💾 Save config").clicked() {
                        self.ai_status = match self.ws.save_ai(std::path::Path::new(".")) {
                            Ok(()) => "Saved AI config to ./klerq-ai.json".into(),
                            Err(e) => format!("Save failed: {e}"),
                        };
                    }
                    ui.label(
                        egui::RichText::new("Key stored locally in plain JSON — keep it private.")
                            .small()
                            .weak(),
                    );
                });
            });

        ui.add_space(10.0);

        // ----- Formula chat -----
        ui.label(egui::RichText::new("Ask for a formula").strong());
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.ai_prompt)
                    .hint_text("e.g. average of B2:B20 if A2:A20 > 0")
                    .desired_width(420.0),
            );
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("✨ Suggest formula").strong())
                        .fill(accent()),
                )
                .clicked()
            {
                match self.ws.suggest_formula(&self.ai_prompt) {
                    Ok(formula) => {
                        self.ai_answer = formula;
                        self.ai_status = "Suggestion ready".into();
                    }
                    Err(e) => {
                        self.ai_answer.clear();
                        self.ai_status = format!("Error: {e}");
                    }
                }
            }
        });
        if !self.ai_answer.is_empty() {
            ui.add_space(6.0);
            egui::Frame::group(ui.style())
                .fill(ui.style().visuals.extreme_bg_color)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new(&self.ai_answer).monospace().size(15.0));
                });
            let target = self.addr(self.sel_col, self.sel_row);
            if ui.button(format!("⇤ Insert into {target}")).clicked() {
                self.ws.set_cell(&target, &self.ai_answer);
                self.cell_buf = self.ai_answer.clone();
                self.tab = Tab::Calc;
            }
        }

        ui.add_space(12.0);

        // ----- Data connection: import from a URL (CSV / JSON / XML autodetected) -----
        ui.label(egui::RichText::new("Data connection — import from URL").strong());
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.csv_url)
                    .hint_text("https://…/data.csv | .json | .xml")
                    .desired_width(420.0),
            );
            if ui.button("⭳ Import").clicked() && !self.csv_url.trim().is_empty() {
                match self.ws.import_url(&self.csv_url) {
                    Ok(rows) => {
                        self.ai_status = format!("Imported {rows} rows into Calc");
                        self.tab = Tab::Calc;
                    }
                    Err(e) => self.ai_status = format!("Import failed: {e}"),
                }
            }
        });

        ui.add_space(8.0);
        ui.label(egui::RichText::new("…or paste CSV / JSON / XML").strong());
        ui.add(
            egui::TextEdit::multiline(&mut self.data_paste)
                .hint_text("paste rows, a JSON array of objects, or XML records")
                .desired_rows(5)
                .desired_width(f32::INFINITY)
                .code_editor(),
        );
        ui.horizontal(|ui| {
            if ui.button("Import CSV").clicked() && !self.data_paste.trim().is_empty() {
                let n = self.ws.import_csv_text(&self.data_paste);
                self.ai_status = format!("Imported {n} CSV rows");
                self.tab = Tab::Calc;
            }
            if ui.button("Import JSON").clicked() && !self.data_paste.trim().is_empty() {
                self.ai_status = match self.ws.import_json_text(&self.data_paste) {
                    Ok(n) => {
                        self.tab = Tab::Calc;
                        format!("Imported {n} JSON records")
                    }
                    Err(e) => format!("JSON import failed: {e}"),
                };
            }
            if ui.button("Import XML").clicked() && !self.data_paste.trim().is_empty() {
                self.ai_status = match self.ws.import_xml_text(&self.data_paste) {
                    Ok(n) => {
                        self.tab = Tab::Calc;
                        format!("Imported {n} XML records")
                    }
                    Err(e) => format!("XML import failed: {e}"),
                };
            }
        });

        if !self.ai_status.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(&self.ai_status).weak());
        }

        ui.add_space(12.0);
        ui.separator();
        // ----- Real-time collaboration (CRDT) -----
        ui.label(
            egui::RichText::new(format!(
                "🔗 Collaboration — replica #{}",
                self.ws.collab_site()
            ))
            .strong(),
        );
        ui.label(
            egui::RichText::new(
                "Calc edits converge across replicas (Google-Docs-style). Exchange the ops \
                 below over any channel; a WebSocket relay automates it.",
            )
            .small()
            .weak(),
        );
        ui.horizontal(|ui| {
            if ui.button("⇧ Export my edits").clicked() {
                self.collab_buf = self.ws.collab_export();
                self.collab_status = "Copied local ops — send to a collaborator".into();
            }
            if ui.button("⇩ Apply ops from box").clicked() && !self.collab_buf.trim().is_empty() {
                self.collab_status = match self.ws.collab_import(&self.collab_buf) {
                    Ok(n) => {
                        self.tab = Tab::Calc;
                        format!("Merged {n} remote op(s)")
                    }
                    Err(e) => format!("Merge failed: {e}"),
                };
            }
        });
        ui.add(
            egui::TextEdit::multiline(&mut self.collab_buf)
                .hint_text("collaboration ops (JSON) — paste a peer's here, then Apply")
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .code_editor(),
        );
        if !self.collab_status.is_empty() {
            ui.label(egui::RichText::new(&self.collab_status).weak());
        }
    }
}
