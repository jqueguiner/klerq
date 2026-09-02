//! klerq-ooxml — MS Office interop: read/write `.docx`, `.xlsx`, `.pptx`.
//!
//! OOXML files are OPC packages — zip containers of XML parts. This crate builds
//! and parses those packages with pure-Rust deps (`zip` + `quick-xml`), so it
//! runs on every Klerq target including ARM.
//!
//! Coverage (built TDD-first, see each module's tests):
//! - **DOCX** — paragraphs + runs with bold/italic/underline. Office-compatible.
//! - **XLSX** — inline-string / number / formula cells. Office-compatible.
//! - **PPTX** — slide titles + text boxes. A Klerq OOXML subset that round-trips
//!   in Klerq; full PowerPoint master/layout compliance is future work.

mod common;
mod docx;
mod pptx;
mod xlsx;

use thiserror::Error;

pub use docx::{read_docx, write_docx};
pub use pptx::{read_pptx, write_pptx};
pub use xlsx::{read_xlsx, write_xlsx};

/// MS Office file extensions.
pub const EXT_DOCX: &str = "docx";
pub const EXT_XLSX: &str = "xlsx";
pub const EXT_PPTX: &str = "pptx";

#[derive(Debug, Error)]
pub enum OoxmlError {
    #[error("zip/container error: {0}")]
    Zip(String),
    #[error("missing package part: {0}")]
    Missing(String),
    #[error("xml error: {0}")]
    Xml(String),
}
