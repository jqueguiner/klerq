//! WordprocessingML (`.docx`) read/write for [`TextDocument`].

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use klerq_writer::{Align, Paragraph, Run, RunStyle, TextDocument};

use crate::common::{decl, xml_escape, zip_read, zip_write};
use crate::OoxmlError;

const CONTENT_TYPES: &str = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn run_xml(run: &Run) -> String {
    let mut rpr = String::new();
    if run.style.bold {
        rpr.push_str("<w:b/>");
    }
    if run.style.italic {
        rpr.push_str("<w:i/>");
    }
    if run.style.underline {
        rpr.push_str("<w:u w:val=\"single\"/>");
    }
    let rpr = if rpr.is_empty() {
        String::new()
    } else {
        format!("<w:rPr>{rpr}</w:rPr>")
    };
    format!(
        "<w:r>{rpr}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
        xml_escape(&run.text)
    )
}

fn paragraph_xml(p: &Paragraph) -> String {
    let runs: String = p.runs.iter().map(run_xml).collect();
    format!("<w:p>{runs}</w:p>")
}

/// Serialize a [`TextDocument`] to `.docx` bytes.
pub fn write_docx(doc: &TextDocument) -> Vec<u8> {
    let body: String = doc.paragraphs.iter().map(paragraph_xml).collect();
    let document = decl(&format!(
        "<w:document xmlns:w=\"{W_NS}\"><w:body>{body}<w:sectPr/></w:body></w:document>"
    ));
    zip_write(&[
        ("[Content_Types].xml", decl(CONTENT_TYPES)),
        ("_rels/.rels", decl(ROOT_RELS)),
        ("word/document.xml", document),
    ])
}

/// Parse `.docx` bytes into a [`TextDocument`].
pub fn read_docx(bytes: &[u8]) -> Result<TextDocument, OoxmlError> {
    let xml = zip_read(bytes, "word/document.xml")?;
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(false);

    let mut doc = TextDocument::new();
    let mut runs: Vec<Run> = Vec::new();
    let mut cur_style = RunStyle::default();
    let mut cur_text = String::new();
    let mut in_run = false;
    let mut in_text = false;

    loop {
        match reader
            .read_event()
            .map_err(|e| OoxmlError::Xml(e.to_string()))?
        {
            Event::Start(e) | Event::Empty(e) => match e.name().as_ref() {
                b"w:p" => {
                    runs = Vec::new();
                }
                b"w:r" => {
                    in_run = true;
                    cur_style = RunStyle::default();
                    cur_text = String::new();
                }
                b"w:b" => cur_style.bold = true,
                b"w:i" => cur_style.italic = true,
                b"w:u" => cur_style.underline = true,
                b"w:t" => in_text = true,
                _ => {}
            },
            Event::Text(e) if in_text => {
                cur_text.push_str(&e.unescape().map_err(|e| OoxmlError::Xml(e.to_string()))?);
            }
            Event::End(e) => match e.name().as_ref() {
                b"w:t" => in_text = false,
                b"w:r" if in_run => {
                    runs.push(Run {
                        text: std::mem::take(&mut cur_text),
                        style: cur_style.clone(),
                    });
                    in_run = false;
                }
                b"w:p" => {
                    doc.paragraphs.push(Paragraph {
                        runs: std::mem::take(&mut runs),
                        align: Align::default(),
                    });
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::zip_has;

    #[test]
    fn docx_is_a_valid_opc_package() {
        let doc = TextDocument::new();
        let bytes = write_docx(&doc);
        assert_eq!(&bytes[..2], b"PK"); // zip magic
        assert!(zip_has(&bytes, "[Content_Types].xml"));
        assert!(zip_has(&bytes, "word/document.xml"));
    }

    #[test]
    fn docx_roundtrips_text_and_formatting() {
        let mut doc = TextDocument::new();
        doc.paragraphs.push(Paragraph::new("plain paragraph"));
        doc.paragraphs.push(Paragraph {
            runs: vec![Run {
                text: "bold & <fancy>".into(),
                style: RunStyle {
                    bold: true,
                    italic: true,
                    underline: false,
                },
            }],
            align: Align::default(),
        });

        let bytes = write_docx(&doc);
        let back = read_docx(&bytes).unwrap();

        assert_eq!(back.paragraphs.len(), 2);
        assert_eq!(back.paragraphs[0].text(), "plain paragraph");
        assert_eq!(back.paragraphs[1].text(), "bold & <fancy>"); // escaping survives
        assert!(back.paragraphs[1].runs[0].style.bold);
        assert!(back.paragraphs[1].runs[0].style.italic);
        assert!(!back.paragraphs[1].runs[0].style.underline);
        assert!(!back.paragraphs[0].runs[0].style.bold);
    }

    #[test]
    fn read_docx_rejects_non_package() {
        assert!(read_docx(b"not a zip").is_err());
    }
}
