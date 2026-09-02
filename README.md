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
│   ├── slides/   klerq-slides  presentations: slides, shapes, ordering
│   ├── format/   klerq-format  save/load: native .klw/.klc/.kls + text/CSV/outline
│   ├── ooxml/    klerq-ooxml   MS Office interop: .docx / .xlsx / .pptx
│   └── ai/       klerq-ai      LLM providers (OpenAI/Anthropic/Gemini/custom) + formula assistant
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
  `#ERR` on bad refs/cycles. **211 functions** (clickable palette): the full
  standard set (math/trig/stats/financial/forecasting) **plus disruptive
  primitives Excel doesn't ship** — neural-net activations (SIGMOID, RELU, GELU,
  SWISH, MISH…), ML metrics & similarity (MSE, RMSE, R2, COSINE, EUCLID, KLDIV,
  CROSSENTROPY…), quant finance (CAGR, SHARPE, SORTINO, MAXDRAWDOWN, EWMA, BETA),
  information stats (ENTROPY, GINI, LOGSUMEXP, IQR, MAD), shaping (CLAMP, LERP,
  REMAP, SMOOTHSTEP), geo (HAVERSINE), number theory (ISPRIME, FIB, POPCOUNT).
- **Slides** — slide list, a slide canvas that renders text-box shapes, add slides
  and boxes.
- **Plugins** — a JS code editor + input; run a community `transform(text)` in the
  sandbox and see the output.
- **AI** — configure a provider (**OpenAI**, **Anthropic**, **Gemini**, or any
  **OpenAI-compatible** endpoint via a custom base URL), store the API key
  locally, then describe a calculation in plain language and get a Klerq formula
  back — grounded in the real function library — to insert into the selected
  cell. Also imports CSV from a URL ("data connection") straight into Calc.
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

## Files & formats

Save/Open in the File menu writes the three open documents as **native** Klerq
files — `klerq.klw` (Writer), `klerq.klc` (Calc), `klerq.kls` (Slides) — a
versioned JSON envelope that round-trips losslessly (formulas preserved). The
`klerq-format` crate also does interop: Writer ⇄ plain text, Calc ⇄ CSV
(evaluated values out, `=formulas` in), Slides ⇄ Markdown-ish outline.

**Structured data import.** The AI tab imports **CSV, JSON and XML** into Calc —
paste it or pull from a URL (format auto-detected). A JSON array of objects
becomes rows × the union of keys (`{data:[…]}` envelopes are unwrapped); XML
records (children of the root) become rows with element/attribute columns.

**MS Office (OOXML).** File ▸ Export/Import MS Office reads and writes real
`.docx` / `.xlsx` / `.pptx` — genuine OPC zip packages built with pure-Rust
`zip` + `quick-xml` (so they work on every target, ARM included):
- **.docx** — paragraphs and runs with bold / italic / underline.
- **.xlsx** — inline-string, number and formula cells (`<f>` + cached value).
- **.pptx** — slide titles and text boxes (a Klerq OOXML subset that round-trips
  in Klerq; full PowerPoint master/layout fidelity is the next phase).

## Languages

Klerq ships **14 locales** out of the box — English, French, Spanish, German,
Italian, Portuguese (BR), Russian, Japanese, Simplified Chinese, Korean, Hindi,
Turkish, plus right-to-left **Arabic** and **Hebrew** (the whole UI mirrors for
RTL). Switch live from the rail; a parity test enforces that every locale
defines every UI key.

To add a language: copy `locales/en-US/klerq.ftl` to
`locales/<your-locale>/klerq.ftl`, translate the values (keep the keys), and add
one line to the `LOCALES` table in `apps/desktop/src/lib.rs`. RTL is detected
automatically from the locale tag.

## Status

Foundation is in place and fully tested (see [`PLAN.md`](PLAN.md) for the phase
breakdown). Next up: the `egui` desktop front-end and native file formats.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at your
option.
