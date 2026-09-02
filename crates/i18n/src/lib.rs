//! opensuite-i18n — localization for the whole suite.
//!
//! Wraps [Project Fluent](https://projectfluent.org) so any language can be
//! added at runtime from `.ftl` sources. Features:
//! - Register a bundle per locale ([`Localizer::add_locale`]).
//! - Translate keys with named args ([`Localizer::t`] / [`Localizer::t_args`]).
//! - Fallback chain to a default locale when a key is missing.
//! - RTL awareness ([`Localizer::is_rtl`]) for Arabic/Hebrew/Farsi/Urdu/…
//!
//! Built TDD-first — see the `tests` module.

use std::collections::BTreeMap;

use fluent::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use thiserror::Error;
use unic_langid::LanguageIdentifier;

/// Languages written right-to-left (by primary language subtag).
const RTL_LANGS: &[&str] = &["ar", "he", "fa", "ur", "ps", "syr", "dv", "yi"];

#[derive(Debug, Error)]
pub enum I18nError {
    #[error("invalid locale identifier: {0}")]
    InvalidLocale(String),
    #[error("failed to parse Fluent resource for {0}")]
    ParseError(String),
    #[error("unknown locale: {0}")]
    UnknownLocale(String),
}

/// Runtime localization registry.
pub struct Localizer {
    bundles: BTreeMap<String, FluentBundle<FluentResource>>,
    current: String,
    fallback: String,
}

impl Localizer {
    /// Create a localizer whose default/fallback locale is `fallback`
    /// (e.g. `"en-US"`), seeded with that locale's `.ftl` source.
    pub fn new(fallback: &str, ftl_source: &str) -> Result<Self, I18nError> {
        let mut me = Self {
            bundles: BTreeMap::new(),
            current: fallback.to_string(),
            fallback: fallback.to_string(),
        };
        me.add_locale(fallback, ftl_source)?;
        Ok(me)
    }

    /// Register (or replace) a locale from Fluent source text.
    pub fn add_locale(&mut self, locale: &str, ftl_source: &str) -> Result<(), I18nError> {
        let langid: LanguageIdentifier = locale
            .parse()
            .map_err(|_| I18nError::InvalidLocale(locale.to_string()))?;
        let res = FluentResource::try_new(ftl_source.to_string())
            .map_err(|_| I18nError::ParseError(locale.to_string()))?;
        let mut bundle = FluentBundle::new(vec![langid]);
        // Isolating marks confuse plain-terminal assertions; disable them.
        bundle.set_use_isolating(false);
        bundle
            .add_resource(res)
            .map_err(|_| I18nError::ParseError(locale.to_string()))?;
        self.bundles.insert(locale.to_string(), bundle);
        Ok(())
    }

    /// Switch the active locale. Must already be registered.
    pub fn set_locale(&mut self, locale: &str) -> Result<(), I18nError> {
        if self.bundles.contains_key(locale) {
            self.current = locale.to_string();
            Ok(())
        } else {
            Err(I18nError::UnknownLocale(locale.to_string()))
        }
    }

    /// Every registered locale, sorted.
    pub fn available_locales(&self) -> Vec<String> {
        self.bundles.keys().cloned().collect()
    }

    /// Active locale tag.
    pub fn current_locale(&self) -> &str {
        &self.current
    }

    /// True when the active locale is written right-to-left.
    pub fn is_rtl(&self) -> bool {
        Self::locale_is_rtl(&self.current)
    }

    /// True when `locale`'s primary language subtag is right-to-left.
    pub fn locale_is_rtl(locale: &str) -> bool {
        let lang = locale.split(['-', '_']).next().unwrap_or("").to_lowercase();
        RTL_LANGS.contains(&lang.as_str())
    }

    /// Translate `key` with no arguments.
    pub fn t(&self, key: &str) -> String {
        self.t_args(key, &[])
    }

    /// Translate `key`, substituting `args` (name, value) pairs.
    /// Falls back to the default locale, then to the raw key.
    pub fn t_args(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut fargs = FluentArgs::new();
        for (k, v) in args {
            fargs.set(*k, FluentValue::from(*v));
        }
        self.format_in(&self.current, key, &fargs)
            .or_else(|| self.format_in(&self.fallback, key, &fargs))
            .unwrap_or_else(|| key.to_string())
    }

    fn format_in(&self, locale: &str, key: &str, args: &FluentArgs) -> Option<String> {
        let bundle = self.bundles.get(locale)?;
        let msg = bundle.get_message(key)?;
        let pattern = msg.value()?;
        let mut errors = Vec::new();
        let out = bundle.format_pattern(pattern, Some(args), &mut errors);
        if errors.is_empty() {
            Some(out.into_owned())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: &str = "app-title = OpenSuite\ngreeting = Hello, { $name }!\nsave = Save";
    const FR: &str = "app-title = OpenSuite\ngreeting = Bonjour, { $name } !\nsave = Enregistrer";
    const AR: &str = "save = حفظ";

    fn loc() -> Localizer {
        let mut l = Localizer::new("en-US", EN).unwrap();
        l.add_locale("fr-FR", FR).unwrap();
        l.add_locale("ar-SA", AR).unwrap();
        l
    }

    #[test]
    fn translates_simple_key() {
        let l = loc();
        assert_eq!(l.t("save"), "Save");
    }

    #[test]
    fn translates_with_args() {
        let l = loc();
        assert_eq!(l.t_args("greeting", &[("name", "Ada")]), "Hello, Ada!");
    }

    #[test]
    fn switches_locale() {
        let mut l = loc();
        l.set_locale("fr-FR").unwrap();
        assert_eq!(l.t("save"), "Enregistrer");
        assert_eq!(l.t_args("greeting", &[("name", "Ada")]), "Bonjour, Ada !");
    }

    #[test]
    fn falls_back_to_default_for_missing_key() {
        let mut l = loc();
        l.set_locale("ar-SA").unwrap(); // AR only defines `save`
        assert_eq!(l.t("app-title"), "OpenSuite"); // from en-US fallback
    }

    #[test]
    fn unknown_key_returns_key() {
        let l = loc();
        assert_eq!(l.t("does-not-exist"), "does-not-exist");
    }

    #[test]
    fn rtl_detection() {
        let mut l = loc();
        assert!(!l.is_rtl());
        l.set_locale("ar-SA").unwrap();
        assert!(l.is_rtl());
        assert!(Localizer::locale_is_rtl("he-IL"));
        assert!(!Localizer::locale_is_rtl("en-US"));
    }

    #[test]
    fn lists_available_locales() {
        let l = loc();
        assert_eq!(l.available_locales(), vec!["ar-SA", "en-US", "fr-FR"]);
    }

    #[test]
    fn set_unknown_locale_errors() {
        let mut l = loc();
        assert!(l.set_locale("zz-ZZ").is_err());
    }
}
