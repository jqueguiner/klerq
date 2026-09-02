//! Microsoft 365 / Excel Online connector via the **Microsoft Graph API**.
//!
//! Unlike Google, Microsoft exposes an official read/write surface for workbooks
//! on OneDrive / SharePoint:
//! - Resolve a **share link** to a `driveItem` (`/shares/{token}/driveItem`).
//! - Read/write worksheet ranges (`.../workbook/worksheets('S')/range(...)`),
//!   with `PATCH` writing values/formulas.
//! - Persistent **workbook sessions** for consistent edits.
//! - **Change-notification webhooks** (subscriptions) for near-real-time push.
//!
//! Joining Microsoft's *live co-authoring* (Fluid Framework) session is still not
//! a public path, but Graph gives real bidirectional sync. All URL/base64/request
//! building here is pure and unit-tested; the network calls use `ureq`.

use serde_json::Value;

use crate::{AiError, HttpRequest};

const GRAPH: &str = "https://graph.microsoft.com/v1.0";

fn base64_std(input: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Encode a sharing URL into a Graph `shares` token (`u!` + url-safe base64).
pub fn encode_share_url(url: &str) -> String {
    let b = base64_std(url.as_bytes());
    let t = b.trim_end_matches('=').replace('/', "_").replace('+', "-");
    format!("u!{t}")
}

fn bearer(url: String, token: &str) -> HttpRequest {
    HttpRequest {
        url,
        headers: vec![("Authorization".into(), format!("Bearer {token}"))],
        body: String::new(),
    }
}

/// GET the `driveItem` behind a sharing link.
pub fn build_shared_item_get(share_url: &str, token: &str) -> HttpRequest {
    bearer(
        format!("{GRAPH}/shares/{}/driveItem", encode_share_url(share_url)),
        token,
    )
}

/// GET a worksheet range's values/formulas.
pub fn build_range_get(
    drive_id: &str,
    item_id: &str,
    worksheet: &str,
    address: &str,
    token: &str,
) -> HttpRequest {
    bearer(
        format!(
            "{GRAPH}/drives/{drive_id}/items/{item_id}/workbook/worksheets('{worksheet}')/range(address='{address}')"
        ),
        token,
    )
}

/// PATCH a worksheet range with a values grid (formulas kept as typed).
pub fn build_range_update(
    drive_id: &str,
    item_id: &str,
    worksheet: &str,
    address: &str,
    values: &[Vec<String>],
    token: &str,
) -> HttpRequest {
    let rows: Vec<Value> = values
        .iter()
        .map(|row| Value::Array(row.iter().map(|c| serde_json::json!(c)).collect()))
        .collect();
    HttpRequest {
        url: format!(
            "{GRAPH}/drives/{drive_id}/items/{item_id}/workbook/worksheets('{worksheet}')/range(address='{address}')"
        ),
        headers: vec![
            ("Authorization".into(), format!("Bearer {token}")),
            ("Content-Type".into(), "application/json".into()),
        ],
        body: serde_json::json!({ "values": rows }).to_string(),
    }
}

/// POST a persistent/non-persistent workbook session.
pub fn build_create_session(
    drive_id: &str,
    item_id: &str,
    persist: bool,
    token: &str,
) -> HttpRequest {
    HttpRequest {
        url: format!("{GRAPH}/drives/{drive_id}/items/{item_id}/workbook/createSession"),
        headers: vec![
            ("Authorization".into(), format!("Bearer {token}")),
            ("Content-Type".into(), "application/json".into()),
        ],
        body: serde_json::json!({ "persistChanges": persist }).to_string(),
    }
}

/// Build a change-notification subscription (webhook) for near-real-time push.
pub fn build_subscription(
    resource: &str,
    notification_url: &str,
    expiry_iso: &str,
    token: &str,
) -> HttpRequest {
    HttpRequest {
        url: format!("{GRAPH}/subscriptions"),
        headers: vec![
            ("Authorization".into(), format!("Bearer {token}")),
            ("Content-Type".into(), "application/json".into()),
        ],
        body: serde_json::json!({
            "changeType": "updated",
            "notificationUrl": notification_url,
            "resource": resource,
            "expirationDateTime": expiry_iso,
        })
        .to_string(),
    }
}

/// Turn a Graph range response into a row-major string grid.
pub fn parse_range_values(raw: &str) -> Result<Vec<Vec<String>>, AiError> {
    let v: Value = serde_json::from_str(raw).map_err(|e| AiError::Parse(e.to_string()))?;
    let rows = v["values"]
        .as_array()
        .ok_or_else(|| AiError::Parse("no 'values' array".into()))?;
    Ok(rows
        .iter()
        .map(|row| {
            row.as_array()
                .map(|cells| cells.iter().map(cell_string).collect())
                .unwrap_or_default()
        })
        .collect())
}

fn cell_string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// ---- network (ureq) ----

fn http(method: &str, url: &str, token: &str, body: Option<&str>) -> Result<String, AiError> {
    let mut req = ureq::request(method, url).set("Authorization", &format!("Bearer {token}"));
    let resp = match body {
        Some(b) => {
            req = req.set("Content-Type", "application/json");
            req.send_string(b)
        }
        None => req.call(),
    }
    .map_err(|e| AiError::Http(e.to_string()))?;
    resp.into_string().map_err(|e| AiError::Http(e.to_string()))
}

/// Resolve a sharing link to `(drive_id, item_id)`.
pub fn resolve_shared_item(share_url: &str, token: &str) -> Result<(String, String), AiError> {
    let req = build_shared_item_get(share_url, token);
    let raw = http("GET", &req.url, token, None)?;
    let v: Value = serde_json::from_str(&raw).map_err(|e| AiError::Parse(e.to_string()))?;
    let item = v["id"]
        .as_str()
        .ok_or_else(|| AiError::Parse("no item id".into()))?;
    let drive = v["parentReference"]["driveId"]
        .as_str()
        .ok_or_else(|| AiError::Parse("no driveId".into()))?;
    Ok((drive.to_string(), item.to_string()))
}

/// Read a worksheet range as a string grid.
pub fn get_range_values(
    drive_id: &str,
    item_id: &str,
    worksheet: &str,
    address: &str,
    token: &str,
) -> Result<Vec<Vec<String>>, AiError> {
    let req = build_range_get(drive_id, item_id, worksheet, address, token);
    let raw = http("GET", &req.url, token, None)?;
    parse_range_values(&raw)
}

/// Write a string grid to a worksheet range.
pub fn update_range(
    drive_id: &str,
    item_id: &str,
    worksheet: &str,
    address: &str,
    values: &[Vec<String>],
    token: &str,
) -> Result<(), AiError> {
    let req = build_range_update(drive_id, item_id, worksheet, address, values, token);
    http("PATCH", &req.url, token, Some(&req.body))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64_std(b"Man"), "TWFu");
        assert_eq!(base64_std(b"Ma"), "TWE=");
        assert_eq!(base64_std(b"M"), "TQ==");
    }

    #[test]
    fn share_url_encodes_url_safe() {
        let t = encode_share_url("https://onedrive.live.com/redir?resid=ABC!12&authKey=x/y+z");
        assert!(t.starts_with("u!"));
        assert!(!t.contains('='));
        assert!(!t.contains('/'));
        assert!(!t.contains('+'));
    }

    #[test]
    fn shared_item_get_targets_graph_shares() {
        let req = build_shared_item_get("https://x/y", "tok");
        assert!(req
            .url
            .starts_with("https://graph.microsoft.com/v1.0/shares/u!"));
        assert!(req.url.ends_with("/driveItem"));
        assert_eq!(
            req.headers
                .iter()
                .find(|(k, _)| k == "Authorization")
                .map(|(_, v)| v.as_str()),
            Some("Bearer tok")
        );
    }

    #[test]
    fn range_get_url_shape() {
        let req = build_range_get("DR", "IT", "Sheet1", "A1:B5", "tok");
        assert!(req
            .url
            .contains("/drives/DR/items/IT/workbook/worksheets('Sheet1')"));
        assert!(req.url.contains("range(address='A1:B5')"));
    }

    #[test]
    fn range_update_builds_values_body() {
        let vals = vec![vec!["Item".into(), "=1+1".into()]];
        let req = build_range_update("DR", "IT", "Sheet1", "A1:B1", &vals, "tok");
        assert!(req.url.contains("worksheets('Sheet1')"));
        assert!(req.body.contains("\"values\""));
        assert!(req.body.contains("\"=1+1\""));
        assert_eq!(
            req.headers
                .iter()
                .find(|(k, _)| k == "Content-Type")
                .map(|(_, v)| v.as_str()),
            Some("application/json")
        );
    }

    #[test]
    fn create_session_body() {
        let req = build_create_session("DR", "IT", true, "tok");
        assert!(req.url.ends_with("/workbook/createSession"));
        assert!(req.body.contains("\"persistChanges\":true"));
    }

    #[test]
    fn subscription_body() {
        let req = build_subscription(
            "/me/drive/root",
            "https://hook",
            "2030-01-01T00:00:00Z",
            "tok",
        );
        assert!(req.url.ends_with("/subscriptions"));
        assert!(req.body.contains("\"changeType\":\"updated\""));
        assert!(req.body.contains("https://hook"));
    }

    #[test]
    fn parses_range_values_of_mixed_types() {
        let raw = r#"{"address":"Sheet1!A1:B2","values":[["Item",10],["Widget",true]]}"#;
        let grid = parse_range_values(raw).unwrap();
        assert_eq!(grid, vec![vec!["Item", "10"], vec!["Widget", "true"]]);
    }

    #[test]
    fn parse_range_bad_json_errors() {
        assert!(parse_range_values("nope").is_err());
        assert!(parse_range_values("{}").is_err());
    }
}
