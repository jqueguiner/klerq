# Klerq — Plan

Open-source, cross-platform office suite in Rust. MS-Office-like. Community JS
plugins. Fully translatable. TDD-first.

Repo: `github.com/jqueguiner/klerq` (public, MIT OR Apache-2.0 dual license).

## Goals

1. **Multi-OS / multi-arch**: Windows, macOS (Apple Silicon + Intel), Linux, ARM, x86_64.
2. **Office suite**: Writer (docs), Calc (spreadsheet), Slides (presentations) — MS Word/Excel/PowerPoint analogs.
3. **All in Rust** for core engine + shared model.
4. **Community plugins in JavaScript** — sandboxed JS runtime, stable plugin API.
5. **Full i18n** — every user-facing string translatable, any language (RTL supported).
6. **TDD** — tests written before implementation, every crate.

## Non-negotiable constraints

- No feature code lands without a test proving it (red → green → refactor).
- Pure-Rust dependencies preferred (portability across all targets).
- CI matrix must build+test on all target OS/arch.

## Architecture — Cargo workspace

```
klerq/
├── Cargo.toml                  # workspace
├── crates/
│   ├── core/     klerq-core    # shared document model, commands, undo/redo
│   ├── i18n/     klerq-i18n    # Fluent localization, locale registry, RTL
│   ├── plugin/   klerq-plugin  # JS runtime host (boa), plugin API, sandbox
│   ├── writer/   klerq-writer  # text document engine (paragraphs, runs, styles)
│   ├── calc/     klerq-calc    # spreadsheet engine (cells, formulas, deps)
│   ├── slides/   klerq-slides  # presentation engine (slides, shapes)
│   ├── format/   klerq-format  # native save/load + text/CSV/outline interop
│   └── ooxml/    klerq-ooxml   # MS Office .docx/.xlsx/.pptx read/write
├── apps/
│   └── desktop/  klerq-desktop # shell binary tying crates together (`klerq`)
├── locales/                    # .ftl translation files per language (en-US, fr-FR)
└── .github/workflows/ci.yml    # cross-platform test matrix
```

## Technology choices

| Concern            | Choice                    | Why |
|--------------------|---------------------------|-----|
| JS engine          | `boa_engine` (pure Rust)  | No native deps → builds on every target incl. ARM |
| i18n               | `fluent` (project-fluent) | Industry-standard, plural/gender, RTL |
| Serialization      | `serde` + JSON            | Portable doc format |
| GUI (later phase)  | `egui`/`eframe`           | Pure-Rust, cross-platform, wgpu backend |
| Testing            | built-in `#[test]`        | TDD core |

## Phased delivery (each phase = tests first)

- **Phase 0 — Scaffold** ✅ workspace, CI matrix, licenses, README.
- **Phase 1 — core** ✅ document model + command stack + undo/redo. (5 tests)
- **Phase 2 — i18n** ✅ localizer, locale registry, fallback, RTL. (8 tests)
- **Phase 3 — plugin** ✅ JS host, manifest, API surface, sandbox. (8 tests)
- **Phase 4 — writer** ✅ paragraphs/runs/styles + edit commands. (4 tests)
- **Phase 5 — calc** ✅ cells, formula parse+eval, dependency recalc, cycles. (16 tests)
- **Phase 6 — slides** ✅ slides/shapes model + reorder. (5 tests)
- **Phase 6.5 — desktop shell** ✅ `Workspace` composes all engines; text demo. (11 tests)
- **Phase 7 — egui front-end** ✅ windowed `klerq-gui`: app-rail, Writer/Calc/Slides/Plugins views, formula bar + live grid, slide canvas, JS plugin runner, dark/light, live locale switch + RTL.
- **Phase 8 — file formats** ✅ `klerq-format`: versioned native save/load (`.klw`/`.klc`/`.kls`) with kind+version guards; interop — Writer↔plain-text, Calc↔CSV (evaluated out, formulas in), Slides↔outline. Wired into the GUI File menu (Save/Open). (13 tests)
- **Phase 9 — OOXML interop** ✅ `klerq-ooxml`: read/write `.docx` (paragraphs + bold/italic/underline), `.xlsx` (inline-string/number/formula cells), `.pptx` (slide titles + text boxes). Real OPC zip + XML via pure-Rust `zip`+`quick-xml`. Wired into GUI File ▸ Export/Import MS Office. (13 tests)
- **Phase 10 — PPTX full compliance** ⏳ slide masters/layouts for full PowerPoint fidelity; shared-strings table for XLSX.

- **Calc engine (ongoing)** ✅ comparisons `= <> < > <= >=`, lazy `IF`, `AND/OR/NOT`, `PRODUCT`, `ABS/INT/ROUND/MOD/POWER/SQRT/CEILING/FLOOR` added on top of SUM/AVERAGE/MIN/MAX/COUNT.
- **i18n (ongoing)** ✅ 14 bundled locales incl RTL Arabic + Hebrew, parity-tested; GUI rail mirrors under RTL.
- **Calc functions (ongoing)** ✅ 148-function library (math/trig/stats/financial/forecasting) via `functions.rs` registry + GUI palette.
- **AI (ongoing)** ✅ `klerq-ai` crate: OpenAI/Anthropic/Gemini/OpenAI-compatible providers, locally-stored config+key, request-build + response-parse tested; GUI AI tab: NL→formula assistant grounded in the function library, plus CSV-from-URL data import.

**Current status:** 93 tests green; `cargo test`, `cargo clippy -D warnings`,
`cargo fmt --check` all pass. GUI builds on all desktop targets.

## Definition of done (per crate)

1. Public API documented.
2. Tests written first, all green.
3. `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check` pass.
4. Builds in CI on all OS/arch targets.

## CI target matrix

- ubuntu-latest — x86_64-unknown-linux-gnu
- ubuntu-latest — aarch64-unknown-linux-gnu (via `cross` + qemu)
- macos-latest — aarch64-apple-darwin (Apple Silicon)
- macos-13 — x86_64-apple-darwin (Intel)
- windows-latest — x86_64-pc-windows-msvc
