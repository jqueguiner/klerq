//! Shared OOXML plumbing: OPC (zip) container read/write + XML text escaping.

use std::io::{Cursor, Read, Write};

use zip::write::{SimpleFileOptions, ZipWriter};

use crate::OoxmlError;

/// Build an OPC package (a zip) from `(path, contents)` parts.
pub(crate) fn zip_write(files: &[(&str, String)]) -> Vec<u8> {
    let mut cur = Cursor::new(Vec::new());
    let mut zw = ZipWriter::new(&mut cur);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in files {
        zw.start_file(*name, opts).expect("zip start_file");
        zw.write_all(data.as_bytes()).expect("zip write");
    }
    zw.finish().expect("zip finish");
    cur.into_inner()
}

/// Read one part out of an OPC package as UTF-8 text.
pub(crate) fn zip_read(bytes: &[u8], name: &str) -> Result<String, OoxmlError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec()))
        .map_err(|e| OoxmlError::Zip(e.to_string()))?;
    let mut f = zip
        .by_name(name)
        .map_err(|_| OoxmlError::Missing(name.to_string()))?;
    let mut s = String::new();
    f.read_to_string(&mut s)
        .map_err(|e| OoxmlError::Zip(e.to_string()))?;
    Ok(s)
}

/// True when the package contains `name`.
#[cfg(test)]
pub(crate) fn zip_has(bytes: &[u8], name: &str) -> bool {
    zip::ZipArchive::new(Cursor::new(bytes.to_vec()))
        .map(|mut z| z.by_name(name).is_ok())
        .unwrap_or(false)
}

/// Escape text for an XML text node / attribute value.
pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

/// Prefix the standard XML declaration.
pub(crate) fn decl(body: &str) -> String {
    format!("{XML_DECL}{body}")
}
