//! Structured-data import: JSON and XML → a tabular [`Table`] → a Calc [`Sheet`].
//!
//! - JSON: an array of objects becomes rows × the union of keys; an array of
//!   scalars becomes a one-column table; a `{ "data": [...] }`-style wrapper is
//!   unwrapped automatically; any other object becomes a key/value table.
//! - XML: each direct child of the root is a record; that record's child
//!   elements (leaf text) and attributes (`@name`) become columns.
//!
//! Nested JSON values are kept as compact JSON strings so nothing is lost.
//!
//! Built TDD-first — see the `tests` module.

use klerq_calc::Sheet;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use serde_json::Value;

use crate::{col_to_letters, FormatError};

/// A simple string table: a header row plus data rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

fn scalar(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        // Nested arrays/objects: keep as compact JSON so no data is dropped.
        other => other.to_string(),
    }
}

/// Parse JSON text into a [`Table`].
pub fn json_to_table(s: &str) -> Result<Table, FormatError> {
    let v: Value = serde_json::from_str(s).map_err(|e| FormatError::Json(e.to_string()))?;
    Ok(value_to_table(&v))
}

fn value_to_table(v: &Value) -> Table {
    match v {
        Value::Array(arr) => {
            let has_objects = arr.iter().any(|x| x.is_object());
            if has_objects {
                let mut headers: Vec<String> = Vec::new();
                for item in arr {
                    if let Value::Object(map) = item {
                        for k in map.keys() {
                            if !headers.contains(k) {
                                headers.push(k.clone());
                            }
                        }
                    }
                }
                let rows = arr
                    .iter()
                    .map(|item| {
                        headers
                            .iter()
                            .map(|h| item.get(h).map(scalar).unwrap_or_default())
                            .collect()
                    })
                    .collect();
                Table { headers, rows }
            } else {
                Table {
                    headers: vec!["value".into()],
                    rows: arr.iter().map(|x| vec![scalar(x)]).collect(),
                }
            }
        }
        Value::Object(map) => {
            // Unwrap a common `{ "field": [ ... ] }` envelope.
            if map.len() == 1 {
                if let Some(inner @ Value::Array(_)) = map.values().next() {
                    return value_to_table(inner);
                }
            }
            Table {
                headers: vec!["key".into(), "value".into()],
                rows: map
                    .iter()
                    .map(|(k, val)| vec![k.clone(), scalar(val)])
                    .collect(),
            }
        }
        other => Table {
            headers: vec!["value".into()],
            rows: vec![vec![scalar(other)]],
        },
    }
}

/// Parse XML text into a [`Table`] (records = direct children of the root).
pub fn xml_to_table(xml: &str) -> Result<Table, FormatError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut headers: Vec<String> = Vec::new();
    let mut records: Vec<Vec<(String, String)>> = Vec::new();
    let mut depth = 0i32;
    let mut cur: Option<Vec<(String, String)>> = None;
    let mut field: Option<String> = None;

    let push_header = |headers: &mut Vec<String>, k: &str| {
        if !headers.iter().any(|h| h == k) {
            headers.push(k.to_string());
        }
    };

    loop {
        match reader
            .read_event()
            .map_err(|e| FormatError::Xml(e.to_string()))?
        {
            Event::Start(e) => {
                depth += 1;
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if depth == 2 {
                    // Record start: seed with its attributes.
                    let mut rec = Vec::new();
                    for attr in e.attributes().flatten() {
                        let k = format!("@{}", String::from_utf8_lossy(attr.key.as_ref()));
                        let val = String::from_utf8_lossy(&attr.value).into_owned();
                        push_header(&mut headers, &k);
                        rec.push((k, val));
                    }
                    cur = Some(rec);
                } else if depth == 3 {
                    field = Some(name);
                }
            }
            Event::Text(e) => {
                if depth == 3 {
                    if let (Some(rec), Some(f)) = (cur.as_mut(), field.as_ref()) {
                        let val = e
                            .unescape()
                            .map_err(|e| FormatError::Xml(e.to_string()))?
                            .into_owned();
                        push_header(&mut headers, f);
                        rec.push((f.clone(), val));
                    }
                }
            }
            Event::Empty(e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if depth == 2 {
                    // Self-closing field inside a record.
                    if let Some(rec) = cur.as_mut() {
                        push_header(&mut headers, &name);
                        rec.push((name, String::new()));
                    }
                } else if depth == 1 {
                    records.push(Vec::new()); // empty self-closing record
                }
            }
            Event::End(_) => {
                if depth == 2 {
                    if let Some(rec) = cur.take() {
                        records.push(rec);
                    }
                } else if depth == 3 {
                    field = None;
                }
                depth -= 1;
            }
            Event::Eof => break,
            _ => {}
        }
    }

    let rows = records
        .iter()
        .map(|rec| {
            headers
                .iter()
                .map(|h| {
                    rec.iter()
                        .find(|(k, _)| k == h)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect();

    Ok(Table { headers, rows })
}

/// Lay a [`Table`] into a fresh [`Sheet`]: header row then data rows.
pub fn table_to_sheet(t: &Table) -> Sheet {
    let mut sheet = Sheet::new();
    for (c, h) in t.headers.iter().enumerate() {
        if !h.is_empty() {
            sheet.set(&format!("{}1", col_to_letters(c as u32)), h);
        }
    }
    for (r, row) in t.rows.iter().enumerate() {
        for (c, val) in row.iter().enumerate() {
            if !val.is_empty() {
                sheet.set(&format!("{}{}", col_to_letters(c as u32), r + 2), val);
            }
        }
    }
    sheet
}

/// Import JSON text straight into a [`Sheet`].
pub fn json_to_sheet(s: &str) -> Result<Sheet, FormatError> {
    Ok(table_to_sheet(&json_to_table(s)?))
}

/// Import XML text straight into a [`Sheet`].
pub fn xml_to_sheet(s: &str) -> Result<Sheet, FormatError> {
    Ok(table_to_sheet(&xml_to_table(s)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use klerq_calc::Cell;

    #[test]
    fn json_array_of_objects_to_table() {
        let t = json_to_table(r#"[{"name":"Ada","age":36},{"name":"Bo","age":40}]"#).unwrap();
        assert_eq!(t.headers, vec!["name", "age"]);
        assert_eq!(t.rows, vec![vec!["Ada", "36"], vec!["Bo", "40"]]);
    }

    #[test]
    fn json_union_of_keys_and_missing_fields() {
        let t = json_to_table(r#"[{"a":1},{"a":2,"b":3}]"#).unwrap();
        assert_eq!(t.headers, vec!["a", "b"]);
        assert_eq!(t.rows, vec![vec!["1", ""], vec!["2", "3"]]);
    }

    #[test]
    fn json_unwraps_data_envelope() {
        let t = json_to_table(r#"{"data":[{"x":1},{"x":2}]}"#).unwrap();
        assert_eq!(t.headers, vec!["x"]);
        assert_eq!(t.rows.len(), 2);
    }

    #[test]
    fn json_nested_value_kept_as_json() {
        let t = json_to_table(r#"[{"id":1,"tags":["a","b"]}]"#).unwrap();
        assert_eq!(t.rows[0][1], "[\"a\",\"b\"]");
    }

    #[test]
    fn json_object_becomes_key_value_table() {
        let t = json_to_table(r#"{"host":"db1","port":5432}"#).unwrap();
        assert_eq!(t.headers, vec!["key", "value"]);
        assert!(t
            .rows
            .contains(&vec!["host".to_string(), "db1".to_string()]));
    }

    #[test]
    fn json_import_into_sheet() {
        let s = json_to_sheet(r#"[{"name":"Ada","qty":10},{"name":"Bo","qty":20}]"#).unwrap();
        assert_eq!(s.raw("A1"), Cell::Text("name".into()));
        assert_eq!(s.raw("B1"), Cell::Text("qty".into()));
        assert_eq!(s.raw("A2"), Cell::Text("Ada".into()));
        assert_eq!(s.eval_number("B2").unwrap(), 10.0);
        assert_eq!(s.eval_number("B3").unwrap(), 20.0);
    }

    #[test]
    fn json_bad_input_errors() {
        assert!(json_to_table("{not json").is_err());
    }

    #[test]
    fn xml_records_to_table() {
        let xml = r#"<rows>
            <row><name>Ada</name><age>36</age></row>
            <row><name>Bo</name><age>40</age></row>
        </rows>"#;
        let t = xml_to_table(xml).unwrap();
        assert_eq!(t.headers, vec!["name", "age"]);
        assert_eq!(t.rows, vec![vec!["Ada", "36"], vec!["Bo", "40"]]);
    }

    #[test]
    fn xml_attributes_become_columns() {
        let xml =
            r#"<items><item id="1"><qty>10</qty></item><item id="2"><qty>20</qty></item></items>"#;
        let t = xml_to_table(xml).unwrap();
        assert_eq!(t.headers, vec!["@id", "qty"]);
        assert_eq!(t.rows[0], vec!["1", "10"]);
        assert_eq!(t.rows[1], vec!["2", "20"]);
    }

    #[test]
    fn xml_import_into_sheet() {
        let xml = r#"<data><r><city>Paris</city><pop>2100000</pop></r></data>"#;
        let s = xml_to_sheet(xml).unwrap();
        assert_eq!(s.raw("A1"), Cell::Text("city".into()));
        assert_eq!(s.raw("A2"), Cell::Text("Paris".into()));
        assert_eq!(s.eval_number("B2").unwrap(), 2100000.0);
    }
}
