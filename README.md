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

## What people hate about Office — and how Klerq answers it

Built from real gripes (Reddit / HN / forums) about Word, Excel and PowerPoint:

| Common complaint | Klerq |
|---|---|
| Word "helpfully" auto-corrects/reformats against you | Explicit styles, **no forced autocorrect**; AI edits only when *you* click |
| PowerPoint is tedious; decks are text-heavy & generic | **AI deck generator** from a topic → punchy bullets |
| Excel is powerful but bloated; **no dark mode**, no custom shortcuts | Dark mode built in, fast Rust engine, **211 functions** incl. AI formula builder |
| Subscriptions, telemetry, cloud lock-in | **Free, open-source (MIT/Apache), offline-first**, keys stay local |
| Weak cross-platform / format fidelity | Windows/macOS/Linux/ARM, real `.docx/.xlsx/.pptx` + native formats |
| Painful real-time collaboration | **CRDT sync** — converges with no server or lock |

AI lives in **all three apps** (Writer, Slides, Calc), with your choice of
provider (OpenAI/Anthropic/Gemini/local) and keys stored on your machine.

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

- **Writer** — add/select paragraphs, bold/italic/underline, alignment,
  undo/redo. **AI assistant**: Summarize, Rewrite, Continue, Shorten, Expand,
  Fix grammar, Professional/Casual tone — one click, then replace or append.
- **Calc** — clickable A1–H20 grid, a formula bar (`=SUM(B2:B3)`), live recalc,
  `#ERR` on bad refs/cycles. **211 functions** (clickable palette): the full
  standard set (math/trig/stats/financial/forecasting) **plus disruptive
  primitives Excel doesn't ship** — neural-net activations (SIGMOID, RELU, GELU,
  SWISH, MISH…), ML metrics & similarity (MSE, RMSE, R2, COSINE, EUCLID, KLDIV,
  CROSSENTROPY…), quant finance (CAGR, SHARPE, SORTINO, MAXDRAWDOWN, EWMA, BETA),
  information stats (ENTROPY, GINI, LOGSUMEXP, IQR, MAD), shaping (CLAMP, LERP,
  REMAP, SMOOTHSTEP), geo (HAVERSINE), number theory (ISPRIME, FIB, POPCOUNT).
- **Slides** — slide list, a slide canvas that renders text-box shapes, add slides
  and boxes. **AI deck generator**: type a topic → a full 5–8 slide deck (title +
  punchy bullets), no template wrangling.
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

**Microsoft 365 / Excel Online.** Via the **Microsoft Graph API** (more open than
Google): resolve a workbook **share link** to a driveItem, then **read and write**
worksheet ranges (`PATCH` keeps formulas), open workbook **sessions**, and
subscribe to **change-notification webhooks** for near-real-time push. Paste the
link + a Graph OAuth token in the AI tab, pick a worksheet/range, then Open or
Push. (Joining Microsoft's live Fluid co-auth session isn't a public path, but
Graph gives genuine bidirectional sync.)

**Google Sheets + Google SSO.** **Sign in with Google** (OAuth 2.0 desktop flow —
browser SSO, PKCE with S256, loopback redirect) to read/write your **private**
sheets. Or paste a link to **open a public sheet** (CSV export, zero auth) with
**auto-poll** for near-real-time reads. Write the grid back via Sheets API v4
(uses your signed-in token, or a pasted one).
(Google's *live* editing runs over a private, undocumented protocol no
third-party client can join — so this is read-now + poll + API write, not a join
of Google's realtime session.) For true multi-user realtime, use Klerq's own
CRDT sync below.

**Real-time collaboration (Google-Docs-style).** `klerq-sync` gives documents
CRDT-based live sync — every replica edits locally and converges with no central
server or locking. Calc uses a last-writer-wins grid (Lamport-stamped, site
tiebreak); text uses a Logoot sequence CRDT so concurrent inserts interleave
deterministically. Ops are `serde`-serializable, so any transport works; the AI
tab has an ops-exchange panel (a WebSocket relay automates it). Convergence is
unit-tested (concurrent conflicts, out-of-order and duplicate delivery).

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
