//! klerq-ai — pluggable LLM access for Klerq.
//!
//! Supports **OpenAI**, **Anthropic**, **Gemini**, and any **OpenAI-compatible**
//! endpoint (set a custom base URL). Configuration — including the API key — is
//! stored locally via [`AiConfig::save_to`] / [`AiConfig::load_from`].
//!
//! The network-free pieces are fully unit-tested: request construction
//! ([`build_request`]) and response parsing ([`parse_response`]). The actual
//! HTTP round-trip ([`chat`]) uses the pure-Rust `ureq` client so it runs on
//! every target.
//!
//! The headline feature is [`suggest_formula`]: describe what you want in plain
//! language and get back a Klerq spreadsheet formula.
//!
//! Built TDD-first — see the `tests` module.

pub mod google;

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("no API key configured")]
    NoKey,
    #[error("http error: {0}")]
    Http(String),
    #[error("could not parse provider response: {0}")]
    Parse(String),
    #[error("io error: {0}")]
    Io(String),
}

/// Supported LLM providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    OpenAI,
    Anthropic,
    Gemini,
    /// Any OpenAI-compatible server (LM Studio, Ollama, vLLM, OpenRouter, …).
    /// Requires a custom `base_url` on the config.
    OpenAiCompatible,
}

impl Provider {
    pub fn default_model(&self) -> &'static str {
        match self {
            Provider::OpenAI | Provider::OpenAiCompatible => "gpt-4o-mini",
            Provider::Anthropic => "claude-opus-4-8",
            Provider::Gemini => "gemini-2.0-flash",
        }
    }

    pub fn default_base(&self) -> &'static str {
        match self {
            Provider::OpenAI => "https://api.openai.com",
            Provider::Anthropic => "https://api.anthropic.com",
            Provider::Gemini => "https://generativelanguage.googleapis.com",
            Provider::OpenAiCompatible => "http://localhost:1234",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Provider::OpenAI => "OpenAI",
            Provider::Anthropic => "Anthropic",
            Provider::Gemini => "Gemini",
            Provider::OpenAiCompatible => "OpenAI-compatible",
        }
    }

    /// All providers, for building a picker.
    pub fn all() -> [Provider; 4] {
        [
            Provider::OpenAI,
            Provider::Anthropic,
            Provider::Gemini,
            Provider::OpenAiCompatible,
        ]
    }
}

/// Stored AI configuration. `api_key` is redacted in `Debug` output so it never
/// leaks into logs.
#[derive(Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub provider: Provider,
    pub model: String,
    pub api_key: String,
    /// Overrides the provider's default base URL (required for OpenAiCompatible).
    #[serde(default)]
    pub base_url: Option<String>,
}

impl fmt::Debug for AiConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AiConfig")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("api_key", &redact(&self.api_key))
            .field("base_url", &self.base_url)
            .finish()
    }
}

fn redact(key: &str) -> String {
    if key.is_empty() {
        "<none>".to_string()
    } else if key.len() <= 6 {
        "***".to_string()
    } else {
        format!("{}…{}", &key[..3], &key[key.len() - 2..])
    }
}

impl AiConfig {
    pub fn new(provider: Provider, api_key: impl Into<String>) -> Self {
        Self {
            provider,
            model: provider.default_model().to_string(),
            api_key: api_key.into(),
            base_url: None,
        }
    }

    fn base(&self) -> String {
        self.base_url
            .clone()
            .unwrap_or_else(|| self.provider.default_base().to_string())
    }

    pub fn has_key(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    pub fn save_to(&self, path: &std::path::Path) -> Result<(), AiError> {
        let json = serde_json::to_string_pretty(self).map_err(|e| AiError::Io(e.to_string()))?;
        std::fs::write(path, json).map_err(|e| AiError::Io(e.to_string()))
    }

    pub fn load_from(path: &std::path::Path) -> Result<Self, AiError> {
        let text = std::fs::read_to_string(path).map_err(|e| AiError::Io(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| AiError::Io(e.to_string()))
    }
}

/// A chat message.
#[derive(Debug, Clone)]
pub struct Msg {
    pub role: String,
    pub content: String,
}

impl Msg {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

/// A ready-to-send HTTP request (kept separate from sending so it is testable).
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Build the provider-specific HTTP request for a chat completion.
pub fn build_request(cfg: &AiConfig, system: Option<&str>, msgs: &[Msg]) -> HttpRequest {
    let base = cfg.base();
    match cfg.provider {
        Provider::OpenAI | Provider::OpenAiCompatible => {
            let mut messages = Vec::new();
            if let Some(s) = system {
                messages.push(json!({"role": "system", "content": s}));
            }
            for m in msgs {
                messages.push(json!({"role": m.role, "content": m.content}));
            }
            HttpRequest {
                url: format!("{base}/v1/chat/completions"),
                headers: vec![
                    ("Authorization".into(), format!("Bearer {}", cfg.api_key)),
                    ("Content-Type".into(), "application/json".into()),
                ],
                body: json!({"model": cfg.model, "messages": messages}).to_string(),
            }
        }
        Provider::Anthropic => {
            let messages: Vec<Value> = msgs
                .iter()
                .map(|m| json!({"role": m.role, "content": m.content}))
                .collect();
            let mut body = json!({
                "model": cfg.model,
                "max_tokens": 1024,
                "messages": messages,
            });
            if let Some(s) = system {
                body["system"] = json!(s);
            }
            HttpRequest {
                url: format!("{base}/v1/messages"),
                headers: vec![
                    ("x-api-key".into(), cfg.api_key.clone()),
                    ("anthropic-version".into(), "2023-06-01".into()),
                    ("Content-Type".into(), "application/json".into()),
                ],
                body: body.to_string(),
            }
        }
        Provider::Gemini => {
            let contents: Vec<Value> = msgs
                .iter()
                .map(|m| {
                    let role = if m.role == "assistant" {
                        "model"
                    } else {
                        "user"
                    };
                    json!({"role": role, "parts": [{"text": m.content}]})
                })
                .collect();
            let mut body = json!({ "contents": contents });
            if let Some(s) = system {
                body["systemInstruction"] = json!({"parts": [{"text": s}]});
            }
            HttpRequest {
                url: format!(
                    "{base}/v1beta/models/{}:generateContent?key={}",
                    cfg.model, cfg.api_key
                ),
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: body.to_string(),
            }
        }
    }
}

/// Extract the assistant's text from a raw provider JSON response.
pub fn parse_response(provider: Provider, raw: &str) -> Result<String, AiError> {
    let v: Value = serde_json::from_str(raw).map_err(|e| AiError::Parse(e.to_string()))?;
    let text = match provider {
        Provider::OpenAI | Provider::OpenAiCompatible => {
            v["choices"][0]["message"]["content"].as_str()
        }
        Provider::Anthropic => v["content"][0]["text"].as_str(),
        Provider::Gemini => v["candidates"][0]["content"]["parts"][0]["text"].as_str(),
    };
    text.map(|s| s.to_string())
        .ok_or_else(|| AiError::Parse(format!("no content in response: {raw}")))
}

/// Send a chat request and return the assistant's reply. Performs real network
/// I/O via `ureq`.
pub fn chat(cfg: &AiConfig, system: Option<&str>, msgs: &[Msg]) -> Result<String, AiError> {
    if !cfg.has_key() && cfg.provider != Provider::OpenAiCompatible {
        return Err(AiError::NoKey);
    }
    let req = build_request(cfg, system, msgs);
    let mut request = ureq::post(&req.url);
    for (k, v) in &req.headers {
        request = request.set(k, v);
    }
    let resp = request
        .send_string(&req.body)
        .map_err(|e| AiError::Http(e.to_string()))?;
    let raw = resp
        .into_string()
        .map_err(|e| AiError::Http(e.to_string()))?;
    parse_response(cfg.provider, &raw)
}

/// System prompt that turns the model into a Klerq formula assistant.
pub fn formula_system_prompt(functions: &[&str]) -> String {
    format!(
        "You are a spreadsheet formula assistant for Klerq (an Excel-like app). \
         Given a request, reply with ONE Klerq formula and nothing else. The \
         formula MUST start with '='. Use A1-style references and ranges (A1:B3). \
         Available functions: {}. If a request cannot be a formula, reply with a \
         short '#' comment.",
        functions.join(", ")
    )
}

/// Extract a clean formula from a model reply: first line starting with `=`,
/// stripping code fences/backticks.
pub fn extract_formula(reply: &str) -> String {
    for line in reply.lines() {
        let t = line.trim().trim_matches('`').trim();
        if let Some(rest) = t.strip_prefix('=') {
            return format!("={}", rest.trim());
        }
    }
    reply.trim().trim_matches('`').to_string()
}

/// Ask the configured model to build a formula for `prompt`.
pub fn suggest_formula(
    cfg: &AiConfig,
    prompt: &str,
    functions: &[&str],
) -> Result<String, AiError> {
    let system = formula_system_prompt(functions);
    let reply = chat(cfg, Some(&system), &[Msg::user(prompt)])?;
    Ok(extract_formula(&reply))
}

/// Fetch a URL's body as text — used for "import data from a connection".
pub fn http_get(url: &str) -> Result<String, AiError> {
    ureq::get(url)
        .call()
        .map_err(|e| AiError::Http(e.to_string()))?
        .into_string()
        .map_err(|e| AiError::Http(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_header<'a>(req: &'a HttpRequest, name: &str) -> Option<&'a str> {
        req.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn openai_request_shape() {
        let cfg = AiConfig::new(Provider::OpenAI, "sk-test-key");
        let req = build_request(&cfg, Some("be terse"), &[Msg::user("2+2?")]);
        assert_eq!(req.url, "https://api.openai.com/v1/chat/completions");
        assert_eq!(
            find_header(&req, "authorization"),
            Some("Bearer sk-test-key")
        );
        assert!(req.body.contains("\"gpt-4o-mini\""));
        assert!(req.body.contains("be terse"));
        assert!(req.body.contains("2+2?"));
    }

    #[test]
    fn anthropic_request_shape() {
        let cfg = AiConfig::new(Provider::Anthropic, "ak-123");
        let req = build_request(&cfg, Some("sys"), &[Msg::user("hi")]);
        assert_eq!(req.url, "https://api.anthropic.com/v1/messages");
        assert_eq!(find_header(&req, "x-api-key"), Some("ak-123"));
        assert_eq!(find_header(&req, "anthropic-version"), Some("2023-06-01"));
        assert!(req.body.contains("\"max_tokens\""));
        assert!(req.body.contains("\"system\":\"sys\""));
    }

    #[test]
    fn gemini_request_puts_key_in_query() {
        let cfg = AiConfig::new(Provider::Gemini, "g-key");
        let req = build_request(&cfg, None, &[Msg::user("hello")]);
        assert!(req.url.contains(":generateContent?key=g-key"));
        assert!(req.url.contains("gemini-2.0-flash"));
        assert!(req.body.contains("\"parts\""));
    }

    #[test]
    fn openai_compatible_uses_custom_base() {
        let mut cfg = AiConfig::new(Provider::OpenAiCompatible, "local");
        cfg.base_url = Some("https://my.host:8000".into());
        cfg.model = "llama-3".into();
        let req = build_request(&cfg, None, &[Msg::user("x")]);
        assert_eq!(req.url, "https://my.host:8000/v1/chat/completions");
        assert!(req.body.contains("llama-3"));
    }

    #[test]
    fn parse_openai_response() {
        let raw = r#"{"choices":[{"message":{"role":"assistant","content":"=SUM(A1:A2)"}}]}"#;
        assert_eq!(
            parse_response(Provider::OpenAI, raw).unwrap(),
            "=SUM(A1:A2)"
        );
    }

    #[test]
    fn parse_anthropic_response() {
        let raw = r#"{"content":[{"type":"text","text":"=AVERAGE(B1:B9)"}]}"#;
        assert_eq!(
            parse_response(Provider::Anthropic, raw).unwrap(),
            "=AVERAGE(B1:B9)"
        );
    }

    #[test]
    fn parse_gemini_response() {
        let raw = r#"{"candidates":[{"content":{"parts":[{"text":"=MAX(A1:A5)"}]}}]}"#;
        assert_eq!(
            parse_response(Provider::Gemini, raw).unwrap(),
            "=MAX(A1:A5)"
        );
    }

    #[test]
    fn parse_error_on_bad_json() {
        assert!(parse_response(Provider::OpenAI, "not json").is_err());
        assert!(parse_response(Provider::OpenAI, "{}").is_err());
    }

    #[test]
    fn extract_formula_strips_fences() {
        assert_eq!(extract_formula("```\n=SUM(A1:A2)\n```"), "=SUM(A1:A2)");
        assert_eq!(
            extract_formula("Here you go:\n=IF(A1>0, 1, 0)"),
            "=IF(A1>0, 1, 0)"
        );
        assert_eq!(extract_formula("`=A1*2`"), "=A1*2");
    }

    #[test]
    fn system_prompt_lists_functions() {
        let p = formula_system_prompt(&["SUM", "IF", "VLOOKUP"]);
        assert!(p.contains("SUM, IF, VLOOKUP"));
        assert!(p.contains("start with '='"));
    }

    #[test]
    fn config_redacts_key_in_debug() {
        let cfg = AiConfig::new(Provider::OpenAI, "sk-supersecret-abcdef");
        let dbg = format!("{cfg:?}");
        assert!(!dbg.contains("supersecret"));
        assert!(dbg.contains("sk-")); // shows a hint, not the whole key
    }

    #[test]
    fn config_roundtrips_to_disk() {
        let dir = std::env::temp_dir().join(format!("klerq-ai-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ai.json");
        let mut cfg = AiConfig::new(Provider::Anthropic, "ak-42");
        cfg.model = "claude-opus-4-8".into();
        cfg.save_to(&path).unwrap();

        let back = AiConfig::load_from(&path).unwrap();
        assert_eq!(back.provider, Provider::Anthropic);
        assert_eq!(back.api_key, "ak-42");
        assert_eq!(back.model, "claude-opus-4-8");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn chat_without_key_errors() {
        let cfg = AiConfig::new(Provider::OpenAI, "");
        assert!(matches!(
            chat(&cfg, None, &[Msg::user("hi")]),
            Err(AiError::NoKey)
        ));
    }
}
