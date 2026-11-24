//! Shared Kense-grade visual primitives, status pills, format chips, and provider marks.

use gpui::{prelude::*, *};
use kui::{
    components::pro_chrome::ProPalette,
    primitives::pro_icons::{ProIcon, pro_app_artwork, pro_icon, pro_svg},
};

use crate::types::OfficeKind;

pub const HEADER_H: f32 = 44.0;

const SVG_SUN: &str = r#"<circle cx="12" cy="12" r="4" stroke="currentColor" stroke-width="1.9"></circle><path d="M12 2V4M12 20V22M4.93 4.93L6.34 6.34M17.66 17.66L19.07 19.07M2 12H4M20 12H22M4.93 19.07L6.34 17.66M17.66 6.34L19.07 4.93" stroke="currentColor" stroke-linecap="round" stroke-width="1.9"></path>"#;

const SVG_MOON: &str = r#"<path d="M20.5 14.469C19.3635 14.9756 18.1066 15.2571 16.7848 15.2571C11.7897 15.2571 7.74286 11.2103 7.74286 6.21519C7.74286 4.89342 8.02437 3.63647 8.53096 2.5C4.78998 3.65848 2.07143 7.14366 2.07143 11.2658C2.07143 16.3323 6.17782 20.4387 11.2443 20.4387C15.3665 20.4387 18.8516 17.7201 20.5 14.469Z" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path>"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DotTone {
    Neutral,
    Success,
    Info,
    Warning,
    Danger,
}

impl DotTone {
    pub fn dot_color(self, p: &ProPalette) -> Hsla {
        match self {
            Self::Neutral => p.faint,
            Self::Success => p.green,
            Self::Info => p.accent_info,
            Self::Warning => p.amber,
            Self::Danger => p.red,
        }
    }

    pub fn text_color(self, p: &ProPalette) -> Hsla {
        match self {
            Self::Neutral => p.text,
            Self::Success => p.ink,
            Self::Info => p.ink,
            Self::Warning => p.amber_ink,
            Self::Danger => p.red_ink,
        }
    }
}

/// Brand lockup adhering to KUI `DESIGN.md`: `#171717` / `#f4f4f5` brand tile + clean typography.
pub fn render_kar_brand_lockup(p: &ProPalette, cx: &App) -> Div {
    let tile_bg: Hsla = if p.dark {
        rgb(0xf4f4f5).into()
    } else {
        rgb(0x171717).into()
    };
    let tile_fg: Hsla = if p.dark {
        rgb(0x111113).into()
    } else {
        rgb(0xffffff).into()
    };

    let mark = pro_app_artwork("kense", 13., cx).unwrap_or_else(|| {
        div()
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(tile_fg)
            .child("K")
            .into_any_element()
    });

    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .size(px(24.))
                .rounded(px(6.))
                .bg(tile_bg)
                .flex()
                .items_center()
                .justify_center()
                .child(mark),
        )
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(5.))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.strong)
                        .child("Kar"),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(p.muted)
                        .child("Studio"),
                ),
        )
}

pub fn render_theme_glyph(dark: bool, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", if dark { SVG_SUN } else { SVG_MOON }, 14.5, color)
}

/// Map a Kar runtime identifier to KUI's official provider artwork or clean fallback glyph.
pub fn render_runtime_mark(runtime_id: &str, side: f32, p: &ProPalette, cx: &App) -> AnyElement {
    let brand_key = match runtime_id {
        "codex" => Some("openai"),
        "claude" => Some("claude"),
        "grok" => Some("grok"),
        "qwen" => Some("qwen"),
        "gemini" => Some("gemini"),
        _ => None,
    };
    if let Some(key) = brand_key
        && let Some(el) = pro_app_artwork(key, side, cx)
    {
        return el;
    }
    pro_icon(ProIcon::Terminal, side, p.muted)
}

/// Understated capsule status pill with a 6px semantic indicator dot.
pub fn render_status_pill(
    label: impl Into<SharedString>,
    tone: DotTone,
    p: &ProPalette,
) -> Div {
    div()
        .h(px(24.))
        .px(px(9.))
        .rounded_full()
        .bg(p.subtle)
        .border_1()
        .border_color(p.line)
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .size(px(6.))
                .rounded_full()
                .bg(tone.dot_color(p)),
        )
        .child(
            div()
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(tone.text_color(p))
                .whitespace_nowrap()
                .child(label.into()),
        )
}

/// Subtle micro-tag explicitly marking isolated demo/sample state without loud banner noise.
pub fn render_demo_pill(label: impl Into<SharedString>, p: &ProPalette) -> Div {
    div()
        .h(px(20.))
        .px(px(6.5))
        .rounded(px(5.))
        .bg(p.amber_soft)
        .border_1()
        .border_color(p.amber.opacity(0.28))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(4.))
        .text_size(px(10.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.amber_ink)
        .whitespace_nowrap()
        .child(label.into())
}

/// Crisp, restrained Office format badge (`DOCX`, `XLSX`, `PPTX`).
pub fn render_office_chip(kind: OfficeKind, compact: bool, p: &ProPalette) -> Div {
    let (bg, fg, border, text) = match kind {
        OfficeKind::Word => (
            p.accent_soft,
            p.accent_info,
            p.accent_info.opacity(0.22),
            if compact { "W" } else { "DOCX" },
        ),
        OfficeKind::Excel => (
            p.green_soft,
            p.green_ink,
            p.green.opacity(0.24),
            if compact { "X" } else { "XLSX" },
        ),
        OfficeKind::Ppt => (
            p.amber_soft,
            p.amber_ink,
            p.amber.opacity(0.26),
            if compact { "P" } else { "PPTX" },
        ),
    };

    div()
        .h(px(if compact { 17. } else { 18. }))
        .px(px(if compact { 4.5 } else { 5.5 }))
        .rounded(px(4.5))
        .bg(bg)
        .border_1()
        .border_color(border)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(10.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .child(text)
}

/// Subtle keyboard shortcut badge (`⌘↵`).
pub fn render_keycap(keys: &'static str, p: &ProPalette) -> Div {
    div()
        .h(px(20.))
        .px(px(5.5))
        .rounded(px(5.))
        .bg(p.chip)
        .border_1()
        .border_color(p.line)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(10.5))
        .text_color(p.muted)
        .child(keys)
}

/// Clean human-friendly document title (removes raw temp prefixes in demo tabs while keeping clarity).
pub fn clean_display_doc_name(raw_name: &str) -> String {
    let stripped = raw_name
        .strip_prefix("[演示] ")
        .unwrap_or(raw_name);
    match stripped {
        "demo_coedit_schedule.docx" => "schedule.docx".into(),
        "demo_coedit_budget.xlsx" => "budget.xlsx".into(),
        "demo_coedit_launch.pptx" => "launch.pptx".into(),
        other => other.to_owned(),
    }
}
