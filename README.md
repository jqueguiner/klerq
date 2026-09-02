<div align="center">

# Klerq

**The Rust-native, plugin-friendly, fully-translatable office suite.**

Writer · Calc · Slides — one shared engine, every platform.

[![CI](https://github.com/jqueguiner/klerq/actions/workflows/ci.yml/badge.svg)](https://github.com/jqueguiner/klerq/actions/workflows/ci.yml)
&nbsp;·&nbsp; License: MIT OR Apache-2.0
&nbsp;·&nbsp; Built test-first (TDD)

</div>

---

Klerq is an open-source office suite written entirely in **Rust**. It aims to be
a familiar analog to the Microsoft Office apps — a word processor (**Writer**), a
spreadsheet (**Calc**) and a presentation tool (**Slides**) — while being:

- **Cross-platform by construction** — Windows, macOS (Apple Silicon + Intel),
  Linux, and ARM. Every dependency is pure Rust, so it builds and runs the same
  everywhere. No native toolchain lock-in.
- **Extensible in JavaScript** — the community writes plugins in JS, run in a
  sandboxed [`boa`](https://github.com/boa-dev/boa) engine with **no** ambient
  filesystem/network access. A plugin can only touch the API Klerq injects.
- **Translatable into any language** — every user-facing string is a
  [Project Fluent](https://projectfluent.org) key. Locales load at runtime,
  right-to-left scripts (Arabic, Hebrew, Farsi, Urdu…) are first-class.
- **Test-driven** — tests are written before the implementation. The whole
  workspace is green under `cargo test`, `cargo clippy -D warnings`, and
  `cargo fmt --check`.

## Workspace layout

```
klerq/
├── crates/
│   ├── core/     klerq-core    shared document model + command stack (undo/redo)
│   ├── i18n/     klerq-i18n    Fluent localization, RTL-aware, fallback chain
│   ├── plugin/   klerq-plugin  sandboxed JavaScript plugin host (boa)
│   ├── writer/   klerq-writer  word processor: paragraphs, runs, styles
│   ├── calc/     klerq-calc    spreadsheet: cells, formula parser/evaluator, recalc
│   └── slides/   klerq-slides  presentations: slides, shapes, ordering
├── apps/
│   └── desktop/  klerq-desktop shell binary composing every engine
├── locales/      en-US, fr-FR  Fluent translation files (add your language here)
└── PLAN.md       the phased, TDD delivery plan
```

## Quick start

```bash
# build + run every test in the workspace
cargo test

# launch the graphical desktop app (egui)
cargo run --bin klerq-gui

# or run the text-mode demo (exercises Writer, Calc, Slides, i18n and a JS plugin)
cargo run --bin klerq
```

## The desktop app (`klerq-gui`)

A native `egui`/`eframe` window with a left app-rail (Writer · Calc · Slides ·
Plugins), a localized menu bar, dark/light theme, and a live language switcher:

- **Writer** — add/select paragraphs, toggle bold, undo/redo (Enter to add).
- **Calc** — clickable A1–H20 grid, a formula bar (`=SUM(B2:B3)`), live recalc,
  `#ERR` on bad refs/cycles.
- **Slides** — slide list, a slide canvas that renders text-box shapes, add slides
  and boxes.
- **Plugins** — a JS code editor + input; run a community `transform(text)` in the
  sandbox and see the output.
- **Language** — switch en-US ⇄ fr-FR from the rail; every label re-localizes and
  RTL locales flip layout.

The entire view layer sits on the unit-tested `Workspace` API — logic is tested,
the GUI is thin.

Expected demo output:

```
Klerq
The Rust-native, plugin-friendly office suite
Writer: Klerq is a Rust-native office suite. / It ships Writer, Calc and Slides. (12 words)
Calc:   A3 = SUM(A1:A2) = 30
Slides: 2 slides
Plugin: PLUGINS WORK
i18n:   Bienvenue dans Klerq, Ada !
```

## Writing a plugin (JavaScript)

A plugin is a JSON manifest plus a JS source. Define a global `transform`:

```js
// manifest: {"name":"shouty","version":"1.0.0","permissions":[]}
function transform(text) {
  return text.toUpperCase();   // read `klerq.version`, `klerq.pluginName` too
}
```

The host runs it in isolation — `fetch`, `require`, and filesystem access simply
do not exist in the sandbox.

## Adding a language

Copy `locales/en-US/klerq.ftl` to `locales/<your-locale>/klerq.ftl`, translate
the values (keep the keys), and register it with
`Localizer::add_locale("<locale>", ftl_source)`. RTL is detected automatically.

## Status

Foundation is in place and fully tested (see [`PLAN.md`](PLAN.md) for the phase
breakdown). Next up: the `egui` desktop front-end and native file formats.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at your
option.
