//! The additional font roles from Harness ui-theme/theme-settings.ts.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FontPreferences {
    pub code_font_size: i32,
    pub terminal_font_size: i32,
    pub text_font_family: String,
    pub code_font_family: String,
    pub terminal_font_family: String,
}

impl Default for FontPreferences {
    fn default() -> Self {
        Self {
            code_font_size: 11,
            terminal_font_size: 13,
            text_font_family: String::new(),
            code_font_family: String::new(),
            terminal_font_family: String::new(),
        }
    }
}

impl FontPreferences {
    pub fn normalized(mut self) -> Self {
        self.code_font_size = self.code_font_size.clamp(10, 16);
        self.terminal_font_size = self.terminal_font_size.clamp(10, 20);
        for family in [
            &mut self.text_font_family,
            &mut self.code_font_family,
            &mut self.terminal_font_family,
        ] {
            *family = normalize_family(family);
        }
        self
    }
}

// Same comma-separated normalization as upstream normalizeFontFamily.
pub fn normalize_family(value: &str) -> String {
    const GENERIC: &[&str] = &[
        "serif",
        "sans-serif",
        "monospace",
        "cursive",
        "fantasy",
        "system-ui",
        "ui-serif",
        "ui-sans-serif",
        "ui-monospace",
        "ui-rounded",
        "emoji",
        "math",
        "fangsong",
        "-apple-system",
        "blinkmacsystemfont",
    ];
    value
        .split(',')
        .filter_map(|name| {
            let name: String = name
                .chars()
                .filter(|c| !matches!(c, '"' | '\'' | '\\' | '<' | '>') && !c.is_ascii_control())
                .collect();
            let name = name
                .split(|c: char| c == '\u{feff}' || c.is_whitespace() && c != '\u{85}')
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if name.is_empty() {
                None
            } else if GENERIC.contains(&name.to_ascii_lowercase().as_str()) {
                Some(name)
            } else {
                Some(format!("\"{name}\""))
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn font_lists_keep_order_and_normalize_idempotently() {
        let value = "  'Arial' , \"Courier   New\", ui-monospace,, <bad>\\\n";
        let expected = "\"Arial\", \"Courier New\", ui-monospace, \"bad\"";
        assert_eq!(normalize_family(value), expected);
        assert_eq!(normalize_family(expected), expected);
    }
    #[test]
    fn missing_roles_keep_defaults_and_role_bounds_are_independent() {
        let prefs: FontPreferences =
            serde_json::from_str(r#"{"codeFontSize":90,"textFontFamily":"Arial"}"#).unwrap();
        let prefs = prefs.normalized();
        assert_eq!(prefs.code_font_size, 16);
        assert_eq!(prefs.terminal_font_size, 13);
        assert_eq!(prefs.text_font_family, "\"Arial\"");
    }
}
