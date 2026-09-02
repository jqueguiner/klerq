//! Google Sheets connector.
//!
//! **What is possible.** Google Sheets' live collaborative editing runs over a
//! private, undocumented protocol — no third-party client can join that session.
//! What third parties *can* do:
//! - Read a **public** sheet with zero auth via its CSV export endpoint.
//! - Read/write via the official **Sheets API v4** with an access token / key.
//!
//! So Klerq "opens" a sheet from its link (read now), polls for near-real-time
//! updates, and writes back through the v4 API when given a token. The
//! URL-parsing and request-building here are pure and unit-tested; the network
//! calls use `ureq`.

use crate::{AiError, HttpRequest};

/// A parsed reference to a Google spreadsheet + tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetRef {
    pub id: String,
    /// Tab id (`gid`); defaults to `"0"` (first tab).
    pub gid: String,
}

/// Parse a Google Sheets share/edit URL into a [`SheetRef`].
pub fn parse_sheet_url(url: &str) -> Option<SheetRef> {
    // .../spreadsheets/d/<ID>/...
    let after = url.split("/d/").nth(1)?;
    let id = after
        .split(['/', '?', '#'])
        .next()
        .filter(|s| !s.is_empty())?
        .to_string();

    // gid may appear as #gid=N or ?gid=N or &gid=N
    let gid = url
        .rsplit_once("gid=")
        .map(|(_, rest)| {
            rest.split(['&', '#', '/'])
                .next()
                .unwrap_or("0")
                .to_string()
        })
        .unwrap_or_else(|| "0".to_string());

    Some(SheetRef { id, gid })
}

/// CSV export URL for a public sheet (no auth required).
pub fn csv_export_url(r: &SheetRef) -> String {
    format!(
        "https://docs.google.com/spreadsheets/d/{}/export?format=csv&gid={}",
        r.id, r.gid
    )
}

/// Build a Sheets API v4 `values.get` request. `auth` is either an API key
/// (public data) appended as `?key=` or, if it looks like an OAuth token,
/// sent as a Bearer header.
pub fn build_values_get(id: &str, range: &str, auth: &str) -> HttpRequest {
    let looks_like_key = !auth.contains('.') && auth.starts_with("AIza");
    if looks_like_key {
        HttpRequest {
            url: format!(
                "https://sheets.googleapis.com/v4/spreadsheets/{id}/values/{range}?key={auth}"
            ),
            headers: vec![],
            body: String::new(),
        }
    } else {
        HttpRequest {
            url: format!("https://sheets.googleapis.com/v4/spreadsheets/{id}/values/{range}"),
            headers: vec![("Authorization".into(), format!("Bearer {auth}"))],
            body: String::new(),
        }
    }
}

/// Build a Sheets API v4 `values.update` request (needs an OAuth access token
/// with write scope). `values` is a row-major grid written from `range`'s
/// top-left, interpreted like typed user input (formulas kept).
pub fn build_values_update(
    id: &str,
    range: &str,
    values: &[Vec<String>],
    token: &str,
) -> HttpRequest {
    let rows: Vec<serde_json::Value> = values
        .iter()
        .map(|row| serde_json::Value::Array(row.iter().map(|c| serde_json::json!(c)).collect()))
        .collect();
    let body = serde_json::json!({ "range": range, "majorDimension": "ROWS", "values": rows });
    HttpRequest {
        url: format!(
            "https://sheets.googleapis.com/v4/spreadsheets/{id}/values/{range}?valueInputOption=USER_ENTERED"
        ),
        headers: vec![
            ("Authorization".into(), format!("Bearer {token}")),
            ("Content-Type".into(), "application/json".into()),
        ],
        body: body.to_string(),
    }
}

/// Fetch a public sheet's contents as CSV (read-now / poll).
pub fn fetch_public_csv(r: &SheetRef) -> Result<String, AiError> {
    crate::http_get(&csv_export_url(r))
}

/// Push a values grid to a sheet via the v4 API (real network PUT).
pub fn push_values(
    id: &str,
    range: &str,
    values: &[Vec<String>],
    token: &str,
) -> Result<(), AiError> {
    let req = build_values_update(id, range, values, token);
    let mut request = ureq::put(&req.url);
    for (k, v) in &req.headers {
        request = request.set(k, v);
    }
    request
        .send_string(&req.body)
        .map_err(|e| AiError::Http(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_edit_url_with_hash_gid() {
        let r = parse_sheet_url("https://docs.google.com/spreadsheets/d/1AbC_dEF123/edit#gid=456")
            .unwrap();
        assert_eq!(r.id, "1AbC_dEF123");
        assert_eq!(r.gid, "456");
    }

    #[test]
    fn parses_url_with_query_gid_and_defaults() {
        let r =
            parse_sheet_url("https://docs.google.com/spreadsheets/d/XYZ/edit?usp=sharing").unwrap();
        assert_eq!(r.id, "XYZ");
        assert_eq!(r.gid, "0"); // default tab
    }

    #[test]
    fn rejects_non_sheet_url() {
        assert!(parse_sheet_url("https://example.com/foo").is_none());
    }

    #[test]
    fn builds_csv_export_url() {
        let r = SheetRef {
            id: "ID1".into(),
            gid: "7".into(),
        };
        assert_eq!(
            csv_export_url(&r),
            "https://docs.google.com/spreadsheets/d/ID1/export?format=csv&gid=7"
        );
    }

    #[test]
    fn values_get_with_api_key_uses_query() {
        let req = build_values_get("SID", "A1:Z100", "AIzaSyExampleKey");
        assert!(req.url.contains("/values/A1:Z100?key=AIzaSyExampleKey"));
        assert!(req.headers.is_empty());
    }

    #[test]
    fn values_get_with_token_uses_bearer() {
        let req = build_values_get("SID", "A1", "ya29.aB.cD"); // token-looking
        assert!(req.url.ends_with("/values/A1"));
        assert_eq!(
            req.headers
                .iter()
                .find(|(k, _)| k == "Authorization")
                .map(|(_, v)| v.as_str()),
            Some("Bearer ya29.aB.cD")
        );
    }

    #[test]
    fn values_update_builds_body() {
        let vals = vec![
            vec!["Item".into(), "Qty".into()],
            vec!["Widget".into(), "=1+1".into()],
        ];
        let req = build_values_update("SID", "A1", &vals, "ya29.tok");
        assert!(req.url.contains("valueInputOption=USER_ENTERED"));
        assert!(req.body.contains("\"=1+1\""));
        assert!(req.body.contains("\"majorDimension\":\"ROWS\""));
        assert_eq!(
            req.headers
                .iter()
                .find(|(k, _)| k == "Authorization")
                .map(|(_, v)| v.as_str()),
            Some("Bearer ya29.tok")
        );
    }
}
