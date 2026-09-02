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

// ================= Google SSO (OAuth 2.0, desktop PKCE) =================

use sha2::{Digest, Sha256};

/// URL-safe base64 without padding.
fn b64url(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(T[((n >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(T[(n & 63) as usize] as char);
        }
    }
    out
}

/// Minimal percent-encoding for query/form values.
fn pe(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// OAuth client configuration for a Google "Desktop app" credential.
#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    /// Google desktop clients issue a secret; PKCE still applies.
    pub client_secret: String,
    /// e.g. `http://127.0.0.1:8585`
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

impl OAuthConfig {
    /// Sensible defaults for Sheets read/write on a loopback port.
    pub fn sheets(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        port: u16,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            redirect_uri: format!("http://127.0.0.1:{port}"),
            scopes: vec!["https://www.googleapis.com/auth/spreadsheets".to_string()],
        }
    }
}

/// Tokens returned by the OAuth token endpoint.
#[derive(Debug, Clone, Default)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
}

/// Generate a random PKCE code verifier (43 chars, url-safe).
pub fn gen_verifier() -> String {
    let mut b = [0u8; 32];
    let _ = getrandom::getrandom(&mut b);
    b64url(&b)
}

/// S256 PKCE challenge for a verifier.
pub fn challenge_of(verifier: &str) -> String {
    let mut h = Sha256::new();
    h.update(verifier.as_bytes());
    b64url(&h.finalize())
}

/// Build the authorization URL the user opens to sign in (SSO).
pub fn auth_url(cfg: &OAuthConfig, challenge: &str, state: &str) -> String {
    format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent&state={}",
        pe(&cfg.client_id),
        pe(&cfg.redirect_uri),
        pe(&cfg.scopes.join(" ")),
        pe(challenge),
        pe(state),
    )
}

/// Build the code→token exchange request (form-encoded POST).
pub fn build_token_exchange(cfg: &OAuthConfig, code: &str, verifier: &str) -> HttpRequest {
    let body = format!(
        "code={}&client_id={}&client_secret={}&redirect_uri={}&grant_type=authorization_code&code_verifier={}",
        pe(code),
        pe(&cfg.client_id),
        pe(&cfg.client_secret),
        pe(&cfg.redirect_uri),
        pe(verifier),
    );
    HttpRequest {
        url: "https://oauth2.googleapis.com/token".into(),
        headers: vec![(
            "Content-Type".into(),
            "application/x-www-form-urlencoded".into(),
        )],
        body,
    }
}

/// Build a refresh-token request.
pub fn build_token_refresh(cfg: &OAuthConfig, refresh_token: &str) -> HttpRequest {
    let body = format!(
        "client_id={}&client_secret={}&grant_type=refresh_token&refresh_token={}",
        pe(&cfg.client_id),
        pe(&cfg.client_secret),
        pe(refresh_token),
    );
    HttpRequest {
        url: "https://oauth2.googleapis.com/token".into(),
        headers: vec![(
            "Content-Type".into(),
            "application/x-www-form-urlencoded".into(),
        )],
        body,
    }
}

/// Parse a token endpoint response.
pub fn parse_token_response(raw: &str) -> Result<TokenSet, AiError> {
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| AiError::Parse(e.to_string()))?;
    let access = v["access_token"]
        .as_str()
        .ok_or_else(|| AiError::Parse(format!("no access_token: {raw}")))?;
    Ok(TokenSet {
        access_token: access.to_string(),
        refresh_token: v["refresh_token"].as_str().map(|s| s.to_string()),
        expires_in: v["expires_in"].as_u64().unwrap_or(0),
    })
}

/// Read a private sheet's range via the Sheets API v4 (needs a token).
pub fn get_values(id: &str, range: &str, token: &str) -> Result<Vec<Vec<String>>, AiError> {
    let req = build_values_get(id, range, token);
    let mut request = ureq::get(&req.url);
    for (k, v) in &req.headers {
        request = request.set(k, v);
    }
    let raw = request
        .call()
        .map_err(|e| AiError::Http(e.to_string()))?
        .into_string()
        .map_err(|e| AiError::Http(e.to_string()))?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| AiError::Parse(e.to_string()))?;
    let rows = v["values"].as_array().cloned().unwrap_or_default();
    Ok(rows
        .iter()
        .map(|row| {
            row.as_array()
                .map(|cs| {
                    cs.iter()
                        .map(|c| match c {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Null => String::new(),
                            other => other.to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect())
}

/// Interactive desktop sign-in: open the browser (Google SSO), catch the
/// loopback redirect, and exchange the code for tokens. Blocking.
pub fn run_loopback_login(cfg: &OAuthConfig) -> Result<TokenSet, AiError> {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let port: u16 = cfg
        .redirect_uri
        .rsplit(':')
        .next()
        .and_then(|p| p.trim_end_matches('/').parse().ok())
        .ok_or_else(|| AiError::Http("redirect_uri must be http://127.0.0.1:PORT".into()))?;
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| AiError::Http(format!("cannot bind {port}: {e}")))?;

    let verifier = gen_verifier();
    let challenge = challenge_of(&verifier);
    let state = gen_verifier();
    let url = auth_url(cfg, &challenge, &state);
    open_browser(&url);

    let (mut stream, _) = listener
        .accept()
        .map_err(|e| AiError::Http(e.to_string()))?;
    let mut buf = [0u8; 4096];
    let n = stream
        .read(&mut buf)
        .map_err(|e| AiError::Http(e.to_string()))?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let query = request
        .split_whitespace()
        .nth(1)
        .and_then(|path| path.split_once('?').map(|(_, q)| q.to_string()))
        .unwrap_or_default();

    let mut code = String::new();
    let mut got_state = String::new();
    for pair in query.split('&') {
        if let Some(v) = pair.strip_prefix("code=") {
            code = pe_decode(v);
        } else if let Some(v) = pair.strip_prefix("state=") {
            got_state = pe_decode(v);
        }
    }
    let _ = stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<h2>Klerq: signed in. You can close this tab.</h2>",
    );

    if got_state != state {
        return Err(AiError::Http("OAuth state mismatch".into()));
    }
    if code.is_empty() {
        return Err(AiError::Http("no authorization code returned".into()));
    }

    let req = build_token_exchange(cfg, &code, &verifier);
    let raw = ureq::post(&req.url)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&req.body)
        .map_err(|e| AiError::Http(e.to_string()))?
        .into_string()
        .map_err(|e| AiError::Http(e.to_string()))?;
    parse_token_response(&raw)
}

fn pe_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn open_browser(url: &str) {
    let (cmd, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else if cfg!(target_os = "windows") {
        ("cmd", vec!["/C", "start", "", url])
    } else {
        ("xdg-open", vec![url])
    };
    let _ = std::process::Command::new(cmd).args(args).spawn();
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

    // ---- OAuth / SSO ----

    #[test]
    fn pkce_challenge_matches_rfc7636_vector() {
        // RFC 7636 Appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            challenge_of(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn auth_url_has_pkce_and_scope() {
        let cfg = OAuthConfig::sheets("cid.apps", "secret", 8585);
        let url = auth_url(&cfg, "CHAL", "STATE");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("client_id=cid.apps"));
        assert!(url.contains("code_challenge=CHAL"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fspreadsheets"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8585"));
        assert!(url.contains("access_type=offline"));
    }

    #[test]
    fn token_exchange_body_is_form_encoded() {
        let cfg = OAuthConfig::sheets("cid", "sec", 8585);
        let req = build_token_exchange(&cfg, "AUTHCODE", "VERIFIER");
        assert_eq!(req.url, "https://oauth2.googleapis.com/token");
        assert!(req.body.contains("grant_type=authorization_code"));
        assert!(req.body.contains("code=AUTHCODE"));
        assert!(req.body.contains("code_verifier=VERIFIER"));
        assert_eq!(
            req.headers
                .iter()
                .find(|(k, _)| k == "Content-Type")
                .map(|(_, v)| v.as_str()),
            Some("application/x-www-form-urlencoded")
        );
    }

    #[test]
    fn refresh_body_uses_refresh_grant() {
        let cfg = OAuthConfig::sheets("cid", "sec", 8585);
        let req = build_token_refresh(&cfg, "RT");
        assert!(req.body.contains("grant_type=refresh_token"));
        assert!(req.body.contains("refresh_token=RT"));
    }

    #[test]
    fn parses_token_response() {
        let raw = r#"{"access_token":"ya29.abc","refresh_token":"1//rt","expires_in":3599}"#;
        let t = parse_token_response(raw).unwrap();
        assert_eq!(t.access_token, "ya29.abc");
        assert_eq!(t.refresh_token.as_deref(), Some("1//rt"));
        assert_eq!(t.expires_in, 3599);
    }

    #[test]
    fn verifier_is_random_and_urlsafe() {
        let a = gen_verifier();
        let b = gen_verifier();
        assert_ne!(a, b);
        assert!(!a.contains('+') && !a.contains('/') && !a.contains('='));
    }
}
