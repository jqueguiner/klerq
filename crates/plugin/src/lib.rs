//! opensuite-plugin — run community plugins written in JavaScript.
//!
//! Uses the pure-Rust [`boa_engine`] interpreter so plugins run on every target
//! OS/arch with no native dependency. The engine ships **no** ambient I/O
//! (`fetch`, `require`, filesystem, network are all absent), so a plugin is
//! sandboxed by construction — it can only touch the API Klerq injects.
//!
//! A plugin is a [`PluginManifest`] (JSON) plus a JS source that may define a
//! global `transform(text)` function and read the injected `klerq` object.
//!
//! Built TDD-first — see the `tests` module.

use boa_engine::{Context, Source};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Plugin metadata, parsed from a JSON manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// Requested capabilities. Empty = pure compute (the safe default).
    #[serde(default)]
    pub permissions: Vec<String>,
}

impl PluginManifest {
    pub fn from_json(json: &str) -> Result<Self, PluginError> {
        serde_json::from_str(json).map_err(|e| PluginError::Manifest(e.to_string()))
    }
}

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("invalid manifest: {0}")]
    Manifest(String),
    #[error("javascript error: {0}")]
    Js(String),
}

/// Host that loads and runs one plugin inside an isolated JS context.
pub struct PluginHost {
    context: Context,
    manifest: PluginManifest,
}

impl PluginHost {
    /// Create a host for `manifest`, inject the stable `opensuite` API, and
    /// evaluate the plugin `source`.
    pub fn load(manifest: PluginManifest, source: &str) -> Result<Self, PluginError> {
        let mut context = Context::default();
        // Stable, minimal API surface exposed to every plugin.
        let api = format!(
            "globalThis.klerq = {{ version: \"{}\", pluginName: \"{}\" }};",
            env!("CARGO_PKG_VERSION"),
            manifest.name.replace('"', "")
        );
        context
            .eval(Source::from_bytes(&api))
            .map_err(|e| PluginError::Js(e.to_string()))?;
        context
            .eval(Source::from_bytes(source))
            .map_err(|e| PluginError::Js(e.to_string()))?;
        Ok(Self { context, manifest })
    }

    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    /// Evaluate an arbitrary JS expression and return it as a string.
    pub fn eval_string(&mut self, expr: &str) -> Result<String, PluginError> {
        let val = self
            .context
            .eval(Source::from_bytes(expr))
            .map_err(|e| PluginError::Js(e.to_string()))?;
        val.to_string(&mut self.context)
            .map(|s| s.to_std_string_escaped())
            .map_err(|e| PluginError::Js(e.to_string()))
    }

    /// Call the plugin's global `transform(text)` with `input`, return result.
    pub fn call_transform(&mut self, input: &str) -> Result<String, PluginError> {
        // JSON-encode the input so it is a safe JS string literal.
        let literal = serde_json::to_string(input).map_err(|e| PluginError::Js(e.to_string()))?;
        let expr = format!("String(transform({literal}))");
        self.eval_string(&expr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{
        "name": "shouty",
        "version": "1.0.0",
        "description": "uppercases text",
        "permissions": []
    }"#;

    fn host(source: &str) -> PluginHost {
        let m = PluginManifest::from_json(MANIFEST).unwrap();
        PluginHost::load(m, source).unwrap()
    }

    #[test]
    fn parses_manifest() {
        let m = PluginManifest::from_json(MANIFEST).unwrap();
        assert_eq!(m.name, "shouty");
        assert_eq!(m.version, "1.0.0");
        assert!(m.permissions.is_empty());
    }

    #[test]
    fn runs_transform_plugin() {
        let mut h = host("function transform(s){ return s.toUpperCase(); }");
        assert_eq!(h.call_transform("hello").unwrap(), "HELLO");
    }

    #[test]
    fn plugin_can_read_injected_api() {
        let mut h = host("");
        assert_eq!(h.eval_string("klerq.pluginName").unwrap(), "shouty");
        assert!(!h.eval_string("klerq.version").unwrap().is_empty());
    }

    #[test]
    fn transform_input_is_escaped_safely() {
        // Quotes / newlines in input must not break out of the JS literal.
        let mut h = host("function transform(s){ return s + s.length; }");
        assert_eq!(h.call_transform("a\"b").unwrap(), "a\"b3");
    }

    #[test]
    fn sandbox_has_no_network() {
        // `fetch` is not defined in the sandbox → calling it throws.
        let mut h = host("");
        assert!(h.eval_string("fetch('http://evil')").is_err());
    }

    #[test]
    fn sandbox_has_no_require() {
        let mut h = host("");
        assert!(h.eval_string("require('fs')").is_err());
    }

    #[test]
    fn bad_manifest_errors() {
        assert!(PluginManifest::from_json("{ not json").is_err());
    }

    #[test]
    fn js_syntax_error_surfaces() {
        let m = PluginManifest::from_json(MANIFEST).unwrap();
        assert!(PluginHost::load(m, "function (( {").is_err());
    }
}
