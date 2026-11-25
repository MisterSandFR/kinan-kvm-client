//! Minimal translation table. Falls back to English when a key is missing.
use std::collections::HashMap;

pub struct I18n {
    language: String,
    strings: HashMap<&'static str, HashMap<&'static str, &'static str>>,
}

const EN: &str = include_str!("../assets/i18n/en.json");
const FR: &str = include_str!("../assets/i18n/fr.json");
const ZH: &str = include_str!("../assets/i18n/zh-CN.json");

const SUPPORTED: &[(&str, &str)] = &[("en", EN), ("fr", FR), ("zh-CN", ZH)];

impl I18n {
    pub fn new() -> Self {
        let mut strings = HashMap::new();
        for (tag, raw) in SUPPORTED {
            let parsed: HashMap<String, String> = match serde_json::from_str(raw) {
                Ok(m) => m,
                Err(_) => continue,
            };
            // Leak strings so the UI closures can hold `&'static str`.
            let leaked: HashMap<&'static str, &'static str> = parsed
                .into_iter()
                .map(|(k, v)| {
                    let key: &'static str = Box::leak(k.into_boxed_str());
                    let val: &'static str = Box::leak(v.into_boxed_str());
                    (key, val)
                })
                .collect();
            strings.insert(*tag, leaked);
        }
        Self {
            language: "en".into(),
            strings,
        }
    }

    pub fn available_languages(&self) -> Vec<&'static str> {
        vec!["en", "fr", "zh-CN"]
    }

    pub fn set_language(&mut self, tag: &str) {
        self.language = if self.strings.contains_key(tag) {
            tag.into()
        } else {
            "en".into()
        };
    }

    #[allow(dead_code)]
    pub fn current(&self) -> &str {
        &self.language
    }

    pub fn auto_detect(&mut self) {
        let locale = sys_locale::get_locale().unwrap_or_else(|| "en".into());
        let tag: &str = if locale.starts_with("fr") {
            "fr"
        } else if locale.starts_with("zh") {
            "zh-CN"
        } else {
            "en"
        };
        self.set_language(tag);
    }

    /// Translate `key`. Missing keys are returned verbatim so bugs stay visible.
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        if let Some(bag) = self.strings.get(self.language.as_str()) {
            if let Some(v) = bag.get(key) {
                return v;
            }
        }
        if let Some(bag) = self.strings.get("en") {
            if let Some(v) = bag.get(key) {
                return v;
            }
        }
        key
    }
}

impl Default for I18n {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::I18n;

    #[test]
    fn english_translation_is_the_default() {
        let mut i = I18n::new();
        i.set_language("en");
        assert_eq!(i.t("app.title"), "Kinan KVM Client");
    }

    #[test]
    fn french_overrides_english() {
        let mut i = I18n::new();
        i.set_language("en");
        let en = i.t("action.connect").to_string();
        i.set_language("fr");
        let fr = i.t("action.connect").to_string();
        assert_ne!(en, fr);
    }

    #[test]
    fn missing_key_returns_itself() {
        let i = I18n::new();
        assert_eq!(i.t("totally.unknown.key"), "totally.unknown.key");
    }
}
