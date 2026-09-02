//! PresentationML (`.pptx`) read/write for [`Presentation`].
//!
//! A Klerq-subset of PresentationML: each slide is a `spTree` of shapes whose
//! `<a:t>` runs carry text. Convention: the first shape is the slide title, the
//! rest are text boxes. This round-trips losslessly in Klerq; full PowerPoint
//! compliance (slide masters/layouts) is tracked as future work in PLAN.md.

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use klerq_slides::{Presentation, Shape, Slide};

use crate::common::{decl, xml_escape, zip_read, zip_write};
use crate::OoxmlError;

const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const P_NS: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

const ROOT_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#;

fn text_shape(text: &str) -> String {
    format!(
        "<p:sp><p:txBody><a:bodyPr/><a:p><a:r><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp>",
        xml_escape(text)
    )
}

fn slide_xml(slide: &Slide) -> String {
    let mut tree = text_shape(&slide.title); // first shape = title
    for shape in &slide.shapes {
        tree.push_str(&text_shape(&shape.text));
    }
    decl(&format!(
        "<p:sld xmlns:a=\"{A_NS}\" xmlns:p=\"{P_NS}\" xmlns:r=\"{R_NS}\"><p:cSld><p:spTree>{tree}</p:spTree></p:cSld></p:sld>"
    ))
}

fn content_types(n: usize) -> String {
    let mut overrides = String::from(
        "<Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>",
    );
    for i in 1..=n {
        overrides.push_str(&format!(
            "<Override PartName=\"/ppt/slides/slide{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
        ));
    }
    decl(&format!(
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>{overrides}</Types>"
    ))
}

fn presentation_xml(n: usize) -> String {
    let mut ids = String::new();
    for i in 1..=n {
        ids.push_str(&format!("<p:sldId id=\"{}\" r:id=\"rId{i}\"/>", 255 + i));
    }
    decl(&format!(
        "<p:presentation xmlns:a=\"{A_NS}\" xmlns:p=\"{P_NS}\" xmlns:r=\"{R_NS}\"><p:sldIdLst>{ids}</p:sldIdLst></p:presentation>"
    ))
}

fn presentation_rels(n: usize) -> String {
    let mut rels = String::new();
    for i in 1..=n {
        rels.push_str(&format!(
            "<Relationship Id=\"rId{i}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide{i}.xml\"/>"
        ));
    }
    decl(&format!(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{rels}</Relationships>"
    ))
}

/// Serialize a [`Presentation`] to `.pptx` bytes.
pub fn write_pptx(deck: &Presentation) -> Vec<u8> {
    let n = deck.slides.len();
    let mut parts: Vec<(String, String)> = vec![
        ("[Content_Types].xml".into(), content_types(n)),
        ("_rels/.rels".into(), decl(ROOT_RELS)),
        ("ppt/presentation.xml".into(), presentation_xml(n)),
        (
            "ppt/_rels/presentation.xml.rels".into(),
            presentation_rels(n),
        ),
    ];
    for (i, slide) in deck.slides.iter().enumerate() {
        parts.push((format!("ppt/slides/slide{}.xml", i + 1), slide_xml(slide)));
    }
    let refs: Vec<(&str, String)> = parts.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    zip_write(&refs)
}

/// Collect the `<a:t>` text runs of one slide part, in document order.
fn slide_texts(xml: &str) -> Result<Vec<String>, OoxmlError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut texts = Vec::new();
    let mut cur = String::new();
    let mut in_t = false;
    loop {
        match reader
            .read_event()
            .map_err(|e| OoxmlError::Xml(e.to_string()))?
        {
            Event::Start(e) if e.name().as_ref() == b"a:t" => {
                in_t = true;
                cur.clear();
            }
            Event::Text(e) if in_t => {
                cur.push_str(&e.unescape().map_err(|e| OoxmlError::Xml(e.to_string()))?);
            }
            Event::End(e) if e.name().as_ref() == b"a:t" => {
                in_t = false;
                texts.push(std::mem::take(&mut cur));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(texts)
}

/// Parse `.pptx` bytes into a [`Presentation`].
pub fn read_pptx(bytes: &[u8]) -> Result<Presentation, OoxmlError> {
    // The first slide must exist for the package to be a presentation.
    let first = zip_read(bytes, "ppt/slides/slide1.xml")?;
    let mut deck = Presentation::new();
    let mut i = 1;
    let mut xml = first;
    loop {
        let texts = slide_texts(&xml)?;
        let mut iter = texts.into_iter();
        let title = iter.next().unwrap_or_default();
        let mut slide = Slide::new(title);
        for t in iter {
            slide.shapes.push(Shape::text_box(t));
        }
        deck.slides.push(slide);

        i += 1;
        match zip_read(bytes, &format!("ppt/slides/slide{i}.xml")) {
            Ok(next) => xml = next,
            Err(_) => break,
        }
    }
    Ok(deck)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::zip_has;

    #[test]
    fn pptx_is_a_valid_opc_package() {
        let mut deck = Presentation::new();
        deck.slides.push(Slide::new("Only"));
        let bytes = write_pptx(&deck);
        assert_eq!(&bytes[..2], b"PK");
        assert!(zip_has(&bytes, "ppt/presentation.xml"));
        assert!(zip_has(&bytes, "ppt/slides/slide1.xml"));
    }

    #[test]
    fn pptx_roundtrips_titles_and_boxes() {
        let mut deck = Presentation::new();
        let mut s1 = Slide::new("Welcome & Intro");
        s1.shapes.push(Shape::text_box("first point"));
        s1.shapes.push(Shape::text_box("second point"));
        deck.slides.push(s1);
        deck.slides.push(Slide::new("End"));

        let bytes = write_pptx(&deck);
        let back = read_pptx(&bytes).unwrap();

        assert_eq!(back.slides.len(), 2);
        assert_eq!(back.slides[0].title, "Welcome & Intro"); // escaping survives
        assert_eq!(back.slides[0].shapes.len(), 2);
        assert_eq!(back.slides[0].shapes[1].text, "second point");
        assert_eq!(back.slides[1].title, "End");
        assert_eq!(back.slides[1].shapes.len(), 0);
    }

    #[test]
    fn read_pptx_rejects_non_package() {
        assert!(read_pptx(b"nope").is_err());
    }
}
