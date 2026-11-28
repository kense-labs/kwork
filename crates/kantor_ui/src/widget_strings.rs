//! The gpui_kit widgets' own text (input context menu, dialog buttons,
//! placeholders, date picker...) in the interface language.
//!
//! gpui-component looks its strings up with rust-i18n (`t!("Input.Cut")`,
//! English in its `locales/ui.yml`, no Indonesian). [`init`] sets its locale
//! to `id` and plugs in a backend that answers those keys from Kantor's
//! catalog (`i18n/widgets.tsv`), so no vendored copy is needed. Keys left
//! out here fall back to gpui-component's English.

use std::borrow::Cow;

use crate::i18n::{self, Lang, tk};

/// gpui-component key, the English it stands for (a catalog key), and the
/// names of its `%{…}` arguments in the order of the English `{}`s.
pub const WIDGET_STRINGS: &[(&str, &str, &[&str])] = &[
    // Text inputs: context menu, clear and password buttons.
    ("Input.Cut", tk("Cut"), &[]),
    ("Input.Copy", tk("Copy"), &[]),
    ("Input.Paste", tk("Paste"), &[]),
    ("Input.Select All", tk("Select All"), &[]),
    ("Input.Clear", tk("Clear"), &[]),
    ("Input.Show Password", tk("Show password"), &[]),
    ("Input.Hide Password", tk("Hide password"), &[]),
    ("Input.Decrement", tk("Decrease"), &[]),
    ("Input.Increment", tk("Increase"), &[]),
    ("Input.Replace", tk("Replace"), &[]),
    ("Input.Replace All", tk("Replace All"), &[]),
    // Dialogs.
    ("Dialog.ok", tk("OK"), &[]),
    ("Dialog.cancel", tk("Cancel"), &[]),
    // Pickers and lists.
    ("Select.placeholder", tk("Please select"), &[]),
    ("ComboBox.placeholder", tk("Please select"), &[]),
    ("Combobox.placeholder", tk("Please select"), &[]),
    ("ComboBox.search_placeholder", tk("Search…"), &[]),
    ("ComboBox.empty", tk("No results"), &[]),
    ("List.search_placeholder", tk("Search…"), &[]),
    ("Command.placeholder", tk("Type a command or search…"), &[]),
    ("Command.empty", tk("No results"), &[]),
    ("DropdownButton.more_options", tk("More options"), &[]),
    ("Table.SortBy", tk("Sort by {}"), &["column"]),
    ("Pagination.previous", tk("Previous"), &[]),
    ("Pagination.next", tk("Next"), &[]),
    ("Settings.search_placeholder", tk("Search…"), &[]),
    ("Settings.no_results", tk("No matching settings"), &[]),
    (
        "Settings.no_results_description",
        tk("Try a different keyword."),
        &[],
    ),
    ("Settings.Reset All", tk("Reset All"), &[]),
    // Colour picker.
    ("ColorPicker.Palette", tk("Palette"), &[]),
    ("ColorPicker.Hue", tk("Hue"), &[]),
    ("ColorPicker.Saturation", tk("Saturation"), &[]),
    ("ColorPicker.Lightness", tk("Lightness"), &[]),
    ("ColorPicker.Alpha", tk("Opacity"), &[]),
    // Panels and tabs.
    ("Dock.Unnamed", tk("Unnamed"), &[]),
    ("Dock.Close", tk("Close"), &[]),
    ("Dock.Zoom In", tk("Zoom In"), &[]),
    ("Dock.Zoom Out", tk("Zoom Out"), &[]),
    ("Dock.Collapse", tk("Collapse"), &[]),
    ("Dock.Expand", tk("Expand"), &[]),
    // Date picker and calendar.
    ("DatePicker.placeholder", tk("Select date"), &[]),
    ("DatePicker.time", tk("Time"), &[]),
    ("Calendar.week.0", tk("Su"), &[]),
    ("Calendar.week.1", tk("Mo"), &[]),
    ("Calendar.week.2", tk("Tu"), &[]),
    ("Calendar.week.3", tk("We"), &[]),
    ("Calendar.week.4", tk("Th"), &[]),
    ("Calendar.week.5", tk("Fr"), &[]),
    ("Calendar.week.6", tk("Sa"), &[]),
    ("Calendar.month.January", tk("January"), &[]),
    ("Calendar.month.February", tk("February"), &[]),
    ("Calendar.month.March", tk("March"), &[]),
    ("Calendar.month.April", tk("April"), &[]),
    ("Calendar.month.May", tk("May"), &[]),
    ("Calendar.month.June", tk("June"), &[]),
    ("Calendar.month.July", tk("July"), &[]),
    ("Calendar.month.August", tk("August"), &[]),
    ("Calendar.month.September", tk("September"), &[]),
    ("Calendar.month.October", tk("October"), &[]),
    ("Calendar.month.November", tk("November"), &[]),
    ("Calendar.month.December", tk("December"), &[]),
];

/// The locale gpui-component is switched to for Indonesian / Simplified Chinese.
const LOCALE: &str = "id";
const ZH_LOCALE: &str = "zh";

/// Namespace of Kantor's keys in the rust-i18n extension (the backend sees
/// `kantor.Input.Cut`).
const NAMESPACE: &str = "kantor";

fn format_widget_template(translated: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(translated.len() + 8);
    let mut rest = translated;
    let mut names = args.iter();
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        if let Some(n) = names.next() {
            out.push_str("%{");
            out.push_str(n);
            out.push('}');
        }
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// The Indonesian for a gpui-component key, with its `{}`s turned back into
/// the `%{name}` arguments rust-i18n fills in.
pub fn indonesian(key: &str) -> Option<String> {
    let (_, en, args) = WIDGET_STRINGS.iter().find(|(k, _, _)| *k == key)?;
    let id = i18n::indonesian(en)?;
    Some(format_widget_template(id, args))
}

/// The Simplified Chinese for a gpui-component key, with its `{}`s turned back into
/// the `%{name}` arguments rust-i18n fills in.
pub fn chinese(key: &str) -> Option<String> {
    let (_, en, args) = WIDGET_STRINGS.iter().find(|(k, _, _)| *k == key)?;
    let zh = i18n::chinese(en)?;
    Some(format_widget_template(zh, args))
}

/// Answers gpui-component's lookups in the `id` and `zh` locales from the catalog.
struct Backend;

impl rust_i18n::Backend for Backend {
    fn available_locales(&self) -> Vec<Cow<'_, str>> {
        vec![Cow::Borrowed(LOCALE), Cow::Borrowed(ZH_LOCALE)]
    }

    fn translate(&self, locale: &str, key: &str) -> Option<Cow<'_, str>> {
        let key = key.strip_prefix(NAMESPACE)?.strip_prefix('.')?;
        match locale {
            LOCALE => indonesian(key).map(Cow::Owned),
            ZH_LOCALE => chinese(key).map(Cow::Owned),
            _ => None,
        }
    }

    fn messages_for_locale(&self, locale: &str) -> Option<Vec<(Cow<'_, str>, Cow<'_, str>)>> {
        match locale {
            LOCALE => Some(
                WIDGET_STRINGS
                    .iter()
                    .filter_map(|(k, _, _)| {
                        let id = indonesian(k)?;
                        Some((Cow::Owned(format!("{NAMESPACE}.{k}")), Cow::Owned(id)))
                    })
                    .collect(),
            ),
            ZH_LOCALE => Some(
                WIDGET_STRINGS
                    .iter()
                    .filter_map(|(k, _, _)| {
                        let zh = chinese(k)?;
                        Some((Cow::Owned(format!("{NAMESPACE}.{k}")), Cow::Owned(zh)))
                    })
                    .collect(),
            ),
            _ => None,
        }
    }
}

/// Put the widgets in the interface language. Call once after `gpui_kit::init`,
/// and call [`sync_locale`] whenever the host switches UI language at runtime.
pub fn init() {
    sync_locale();
}

/// Synchronize `gpui_kit::component` locale with [`i18n::lang`].
pub fn sync_locale() {
    match i18n::lang() {
        Lang::Id => {
            install();
            gpui_kit::component::set_locale(LOCALE);
        }
        Lang::Zh => {
            install();
            gpui_kit::component::set_locale(ZH_LOCALE);
        }
        Lang::En => {
            gpui_kit::component::set_locale("en");
        }
    }
}

/// Hand gpui-component the catalog backend (once per process).
fn install() {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    static BACKEND: Backend = Backend;
    INSTALLED.call_once(|| {
        gpui_kit::component::_rust_i18n_extend(&BACKEND, NAMESPACE);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[std::prelude::v1::test]
    fn every_widget_string_has_indonesian() {
        let missing: Vec<&str> = WIDGET_STRINGS
            .iter()
            .filter(|(k, _, _)| indonesian(k).is_none())
            .map(|(_, en, _)| *en)
            .collect();
        assert!(missing.is_empty(), "no Indonesian for {missing:?}");
    }

    #[std::prelude::v1::test]
    fn arguments_keep_rust_i18n_names() {
        assert_eq!(
            indonesian("Table.SortBy").as_deref(),
            Some("Urutkan menurut %{column}")
        );
        for (k, en, args) in WIDGET_STRINGS {
            assert_eq!(en.matches("{}").count(), args.len(), "{k}");
        }
    }

    /// Through gpui-component's own lookup, as its `t!` does.
    #[std::prelude::v1::test]
    fn gpui_component_finds_the_indonesian() {
        install();
        let tr = |l, k| gpui_kit::component::_rust_i18n_translate(l, k).into_owned();
        assert_eq!(tr("id", "Input.Cut"), "Potong");
        assert_eq!(tr("id", "Dialog.cancel"), "Batal");
        assert_eq!(tr("en", "Input.Cut"), "Cut");
        // Not in Kantor's list: gpui-component's English.
        assert_eq!(tr("id", "Diff.NoChanges"), "No changes");
    }

    #[std::prelude::v1::test]
    fn backend_answers_namespaced_keys_in_id_only() {
        use rust_i18n::Backend as _;
        assert_eq!(
            Backend.translate("id", "kantor.Input.Paste").as_deref(),
            Some("Tempel")
        );
        assert_eq!(Backend.translate("en", "kantor.Input.Paste"), None);
        assert_eq!(Backend.translate("id", "Input.Paste"), None);
    }
}
