use std::{collections::BTreeMap, sync::OnceLock};
pub const LANGUAGES: [(&str, &str); 6] = [
    ("en", "English"),
    ("ru", "Русский"),
    ("fr", "Français"),
    ("de", "Deutsch"),
    ("es", "Español"),
    ("it", "Italiano"),
];
fn catalog() -> &'static BTreeMap<String, [String; 6]> {
    static DATA: OnceLock<BTreeMap<String, [String; 6]>> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../locales/catalog.json"))
            .expect("validated translation catalog")
    })
}
#[derive(Clone, Copy)]
pub struct Text {
    index: usize,
}
impl Text {
    pub fn new(language: &str) -> Self {
        Self {
            index: LANGUAGES
                .iter()
                .position(|(id, _)| *id == language)
                .unwrap_or(0),
        }
    }
    pub fn get(self, key: &str) -> &'static str {
        catalog()
            .get(key)
            .map(|row| row[self.index].as_str())
            .unwrap_or("…")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_six_catalogs_are_complete_and_fallback_is_english() {
        assert!(catalog().len() > 100);
        for (key, row) in catalog() {
            for value in row {
                assert!(!value.trim().is_empty(), "missing {key}");
            }
        }
        assert_eq!(Text::new("unknown").get("save"), "Save");
    }
}
