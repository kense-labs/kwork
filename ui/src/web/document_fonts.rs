//! The macOS system text face and optical sizes used by document headings.

use super::*;
use crate::font_preferences::FontPreferences;
use gpui::{FontFallbacks, Global, SharedString};
use js_sys::{Array, Uint8Array};
use std::collections::{HashMap, HashSet};
use wasm_bindgen_futures::JsFuture;

impl Global for FontPreferences {}

pub(super) fn preferences(cx: &App) -> &FontPreferences {
    cx.global::<FontPreferences>()
}
pub(super) fn code_size(cx: &App) -> f32 {
    preferences(cx).code_font_size as f32
}
pub(super) fn terminal_size(cx: &App) -> f32 {
    preferences(cx).terminal_font_size as f32
}

#[wasm_bindgen(raw_module = "../document-fonts.js")]
extern "C" {
    #[wasm_bindgen(js_name = prepareDocumentFonts)]
    fn prepare_document_fonts(source: Uint8Array) -> js_sys::Promise;
}

struct DocumentFonts {
    loaded: bool,
    chinese: Option<FontFallbacks>,
    requested: HashSet<String>,
    installed: HashMap<String, SharedString>,
}

impl Global for DocumentFonts {}

pub(super) fn prepare(cx: &mut Context<DshApp>) {
    cx.set_global(FontPreferences::default());
    if !bridge::is_mac() || cx.try_global::<DocumentFonts>().is_some() {
        return;
    }
    cx.set_global(DocumentFonts {
        loaded: false,
        chinese: None,
        requested: HashSet::new(),
        installed: HashMap::new(),
    });
    cx.spawn(async move |this, cx| {
        let Some(source) = bridge::font_data("document_system_font").await else {
            return;
        };
        let Ok(result) = JsFuture::from(prepare_document_fonts(source)).await else {
            return;
        };
        let fonts = Array::from(&result)
            .iter()
            .map(|value| Uint8Array::new(&value).to_vec().into())
            .collect();
        let _ = this.update(cx, |_, cx| {
            let system = cx.text_system();
            if system.add_fonts(fonts).is_ok()
                && system
                    .all_font_names()
                    .iter()
                    .any(|name| name == "DSH System 17")
            {
                let installed = system
                    .all_font_names()
                    .into_iter()
                    .map(|name| (name.to_lowercase(), name.into()))
                    .collect();
                cx.global_mut::<DocumentFonts>().loaded = true;
                cx.global_mut::<DocumentFonts>().installed = installed;
                cx.refresh_windows();
                cx.notify();
            }
        });
    })
    .detach();
    cx.spawn(async move |this, cx| {
        let Some(source) = bridge::font_data("chinese_system_font").await else {
            return;
        };
        let _ = this.update(cx, |_, cx| {
            let system = cx.text_system();
            if system.add_fonts(vec![source.to_vec().into()]).is_ok()
                && system
                    .all_font_names()
                    .iter()
                    .any(|name| name == "PingFang SC")
            {
                let installed = system
                    .all_font_names()
                    .into_iter()
                    .map(|name| (name.to_lowercase(), name.into()))
                    .collect();
                cx.global_mut::<DocumentFonts>().chinese = Some(FontFallbacks::from_fonts(vec![
                    "PingFang SC".into(),
                    "KUI Input Sans".into(),
                ]));
                cx.global_mut::<DocumentFonts>().installed = installed;
                cx.refresh_windows();
                cx.notify();
            }
        });
    })
    .detach();
}

pub(crate) fn interface_family(cx: &App) -> Option<SharedString> {
    family(17., cx)
}

pub(super) fn interface_fallbacks(cx: &App) -> Option<FontFallbacks> {
    let custom = installed_families(&preferences(cx).text_font_family, cx);
    if custom.is_empty() {
        return cx.try_global::<DocumentFonts>()?.chinese.clone();
    }
    Some(fallbacks(custom, cx))
}

pub(super) fn interface_style(w: &Window, cx: &App) -> gpui::TextStyle {
    let mut style = w.text_style();
    if let Some(family) = interface_family(cx) {
        style.font_family = family;
    }
    if let Some(fallbacks) = interface_fallbacks(cx) {
        style.font_fallbacks = Some(fallbacks);
    }
    style
}

pub(super) fn family(size: f32, cx: &App) -> Option<SharedString> {
    if let Some(family) = installed_families(&preferences(cx).text_font_family, cx).first() {
        return Some(family.clone());
    }
    cx.try_global::<DocumentFonts>()?.loaded.then(|| {
        let optical = size.round().clamp(17., 25.) as u8;
        format!("DSH System {optical}").into()
    })
}

fn candidates(value: &str) -> impl Iterator<Item = String> + '_ {
    value
        .split(',')
        .map(|name| {
            let name = name.trim().trim_matches('"');
            match name.to_ascii_lowercase().as_str() {
                "serif" | "ui-serif" => "Times New Roman",
                "sans-serif" | "ui-sans-serif" => "Arial",
                "monospace" | "ui-monospace" => "Menlo",
                "system-ui" | "ui-rounded" | "-apple-system" | "blinkmacsystemfont" => {
                    "DSH System 17"
                }
                "cursive" => "Apple Chancery",
                "fantasy" => "Papyrus",
                "emoji" => "Apple Color Emoji",
                "math" => "STIX Two Math",
                "fangsong" => "FangSong",
                _ => name,
            }
            .to_owned()
        })
        .filter(|name| !name.is_empty())
}

fn installed_families(value: &str, cx: &App) -> Vec<SharedString> {
    let Some(fonts) = cx.try_global::<DocumentFonts>() else {
        return Vec::new();
    };
    candidates(value)
        .filter_map(|name| {
            if name == "DSH System 17" && fonts.loaded {
                Some(name.into())
            } else {
                fonts.installed.get(&name.to_lowercase()).cloned()
            }
        })
        .collect()
}

fn fallbacks(mut names: Vec<SharedString>, cx: &App) -> FontFallbacks {
    if cx
        .try_global::<DocumentFonts>()
        .is_some_and(|fonts| fonts.chinese.is_some())
    {
        names.push("PingFang SC".into());
    }
    names.push("KUI Input Sans".into());
    FontFallbacks::from_fonts(names.into_iter().map(|name| name.to_string()).collect())
}

pub(super) fn code_family(cx: &App) -> SharedString {
    installed_families(&preferences(cx).code_font_family, cx)
        .into_iter()
        .next()
        .unwrap_or_else(|| default_monospace().into())
}

pub(super) fn code_fallbacks(cx: &App) -> FontFallbacks {
    let mut names = installed_families(&preferences(cx).code_font_family, cx);
    names.push(default_monospace().into());
    fallbacks(names, cx)
}

fn default_monospace() -> &'static str {
    if bridge::is_mac() {
        "Menlo"
    } else if bridge::is_windows() {
        "Cascadia Mono"
    } else {
        "monospace"
    }
}

pub(super) fn terminal_family(cx: &App) -> SharedString {
    installed_families(&preferences(cx).terminal_font_family, cx)
        .into_iter()
        .next()
        .unwrap_or_else(|| default_monospace().into())
}

pub(super) fn terminal_fallbacks(cx: &App) -> FontFallbacks {
    let mut names = installed_families(&preferences(cx).terminal_font_family, cx);
    names.push(default_monospace().into());
    fallbacks(names, cx)
}

pub(super) fn apply_preferences(next: FontPreferences, cx: &mut Context<DshApp>) {
    let next = next.normalized();
    cx.set_global(next.clone());
    if bridge::is_mac() {
        let installed = cx
            .text_system()
            .all_font_names()
            .into_iter()
            .map(|name| (name.to_lowercase(), name.into()))
            .collect();
        cx.global_mut::<DocumentFonts>().installed = installed;
        let names = [
            &next.text_font_family,
            &next.code_font_family,
            &next.terminal_font_family,
        ]
        .into_iter()
        .flat_map(|value| candidates(value))
        .collect::<HashSet<_>>();
        for name in names {
            let fonts = cx.global_mut::<DocumentFonts>();
            if fonts.installed.contains_key(&name.to_lowercase())
                || !fonts.requested.insert(name.clone())
                || name == "DSH System 17"
            {
                continue;
            }
            cx.spawn(async move |this, cx| {
                let Some(source) =
                    bridge::font_data_args("named_system_font", &json!({"family":name})).await
                else {
                    return;
                };
                let _ = this.update(cx, |_, cx| {
                    let system = cx.text_system();
                    if system.add_fonts(vec![source.to_vec().into()]).is_ok() {
                        let installed = system
                            .all_font_names()
                            .into_iter()
                            .map(|name| (name.to_lowercase(), name.into()))
                            .collect();
                        cx.global_mut::<DocumentFonts>().installed = installed;
                        cx.refresh_windows();
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }
    cx.refresh_windows();
    cx.notify();
}
