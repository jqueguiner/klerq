//! SpreadsheetML (`.xlsx`) read/write for [`Sheet`].
//!
//! Cells are written as inline strings (`t="inlineStr"`), numbers (`<v>`), or
//! formulas (`<f>` + a cached `<v>` value). No shared-string table — inline
//! strings keep the single worksheet self-contained.

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use klerq_calc::{Cell, Sheet};

use crate::common::{decl, xml_escape, zip_read, zip_write};
use crate::OoxmlError;

const CONTENT_TYPES: &str = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;

const WORKBOOK: &str = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#;

const WORKBOOK_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;

const S_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

fn col_letters(mut col: u32) -> String {
    let mut s = String::new();
    col += 1;
    while col > 0 {
        let rem = (col - 1) % 26;
        s.insert(0, (b'A' + rem as u8) as char);
        col = (col - 1) / 26;
    }
    s
}

fn num_str(n: f64) -> String {
    // format! already drops the trailing .0 for whole floats.
    format!("{n}")
}

fn cell_xml(sheet: &Sheet, addr: &str) -> Option<String> {
    match sheet.raw(addr) {
        Cell::Empty => None,
        Cell::Number(n) => Some(format!("<c r=\"{addr}\"><v>{}</v></c>", num_str(n))),
        Cell::Text(t) => Some(format!(
            "<c r=\"{addr}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
            xml_escape(&t)
        )),
        Cell::Formula(f) => {
            let cached = sheet.eval_number(addr).unwrap_or(0.0);
            Some(format!(
                "<c r=\"{addr}\"><f>{}</f><v>{}</v></c>",
                xml_escape(&f),
                num_str(cached)
            ))
        }
    }
}

/// Serialize a [`Sheet`] to `.xlsx` bytes.
pub fn write_xlsx(sheet: &Sheet) -> Vec<u8> {
    let mut sheet_data = String::new();
    if let Some((max_col, max_row)) = sheet.extent() {
        for row in 0..=max_row {
            let mut cells = String::new();
            for col in 0..=max_col {
                let addr = format!("{}{}", col_letters(col), row + 1);
                if let Some(c) = cell_xml(sheet, &addr) {
                    cells.push_str(&c);
                }
            }
            if !cells.is_empty() {
                sheet_data.push_str(&format!("<row r=\"{}\">{cells}</row>", row + 1));
            }
        }
    }
    let worksheet = decl(&format!(
        "<worksheet xmlns=\"{S_NS}\"><sheetData>{sheet_data}</sheetData></worksheet>"
    ));
    zip_write(&[
        ("[Content_Types].xml", decl(CONTENT_TYPES)),
        ("_rels/.rels", decl(ROOT_RELS)),
        ("xl/workbook.xml", decl(WORKBOOK)),
        ("xl/_rels/workbook.xml.rels", decl(WORKBOOK_RELS)),
        ("xl/worksheets/sheet1.xml", worksheet),
    ])
}

#[derive(PartialEq)]
enum In {
    None,
    Formula,
    Value,
    Text,
}

/// Parse `.xlsx` bytes into a [`Sheet`].
pub fn read_xlsx(bytes: &[u8]) -> Result<Sheet, OoxmlError> {
    let xml = zip_read(bytes, "xl/worksheets/sheet1.xml")?;
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(false);

    let mut sheet = Sheet::new();
    let mut addr = String::new();
    let mut is_inline = false;
    let mut formula: Option<String> = None;
    let mut value: Option<String> = None;
    let mut inline: Option<String> = None;
    let mut state = In::None;

    loop {
        match reader
            .read_event()
            .map_err(|e| OoxmlError::Xml(e.to_string()))?
        {
            Event::Start(e) => match e.name().as_ref() {
                b"c" => {
                    addr.clear();
                    is_inline = false;
                    formula = None;
                    value = None;
                    inline = None;
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"r" => {
                                addr = String::from_utf8_lossy(&attr.value).into_owned();
                            }
                            b"t" => {
                                is_inline = attr.value.as_ref() == b"inlineStr";
                            }
                            _ => {}
                        }
                    }
                }
                b"f" => state = In::Formula,
                b"v" => state = In::Value,
                b"t" => state = In::Text,
                _ => {}
            },
            Event::Text(e) => {
                let text = e.unescape().map_err(|e| OoxmlError::Xml(e.to_string()))?;
                match state {
                    In::Formula => formula = Some(text.into_owned()),
                    In::Value => value = Some(text.into_owned()),
                    In::Text => inline = Some(text.into_owned()),
                    In::None => {}
                }
            }
            Event::End(e) => match e.name().as_ref() {
                b"f" | b"v" | b"t" => state = In::None,
                b"c" if !addr.is_empty() => {
                    if let Some(f) = &formula {
                        sheet.set(&addr, &format!("={f}"));
                    } else if is_inline {
                        sheet.set(&addr, inline.as_deref().unwrap_or(""));
                    } else if let Some(v) = &value {
                        sheet.set(&addr, v);
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(sheet)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::zip_has;

    #[test]
    fn xlsx_is_a_valid_opc_package() {
        let bytes = write_xlsx(&Sheet::new());
        assert_eq!(&bytes[..2], b"PK");
        assert!(zip_has(&bytes, "xl/workbook.xml"));
        assert!(zip_has(&bytes, "xl/worksheets/sheet1.xml"));
    }

    #[test]
    fn xlsx_roundtrips_numbers_text_formulas() {
        let mut s = Sheet::new();
        s.set("A1", "Item");
        s.set("B1", "10");
        s.set("B2", "=B1*2");
        s.set("C1", "a & b");

        let bytes = write_xlsx(&s);
        let back = read_xlsx(&bytes).unwrap();

        assert_eq!(back.raw("A1"), Cell::Text("Item".into()));
        assert_eq!(back.eval_number("B1").unwrap(), 10.0);
        assert_eq!(back.raw("B2"), Cell::Formula("B1*2".into()));
        assert_eq!(back.eval_number("B2").unwrap(), 20.0); // formula recomputes
        assert_eq!(back.raw("C1"), Cell::Text("a & b".into())); // escaping survives
    }

    #[test]
    fn xlsx_handles_multi_letter_columns() {
        let mut s = Sheet::new();
        s.set("AA1", "42");
        let back = read_xlsx(&write_xlsx(&s)).unwrap();
        assert_eq!(back.eval_number("AA1").unwrap(), 42.0);
    }

    #[test]
    fn read_xlsx_rejects_non_package() {
        assert!(read_xlsx(b"nope").is_err());
    }
}
