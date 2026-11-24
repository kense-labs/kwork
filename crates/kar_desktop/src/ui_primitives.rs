//! Claude-inspired warm editorial design tokens, elevation shadows, and KUI visual primitives.

use gpui::{prelude::*, *};
use kui::{
    components::pro_chrome::ProPalette,
    primitives::pro_icons::{ProIcon, pro_app_artwork, pro_icon, pro_svg},
};

use crate::types::OfficeKind;

/// Height of the top window / workspace chrome bar (matches Claude Desktop `--frame-row-h: 48px`).
pub const HEADER_H: f32 = 48.0;
/// Left inset required on macOS to clear native traffic lights (`--df-traffic-light-spacer: 76px`).
pub const MACOS_TRAFFIC_LIGHT_PAD: f32 = 76.0;

const SVG_SUN: &str = r#"<circle cx="12" cy="12" r="4" stroke="currentColor" stroke-width="1.85"></circle><path d="M12 2.2V4.2M12 19.8V21.8M5.07 5.07L6.48 6.48M17.52 17.52L18.93 18.93M2.2 12H4.2M19.8 12H21.8M5.07 18.93L6.48 17.52M17.52 6.48L18.93 5.07" stroke="currentColor" stroke-linecap="round" stroke-width="1.85"></path>"#;

const SVG_MOON: &str = r#"<path d="M20.4 14.45C19.28 14.95 18.04 15.23 16.74 15.23C11.82 15.23 7.83 11.24 7.83 6.32C7.83 5.02 8.11 3.78 8.61 2.66C4.92 3.8 2.24 7.24 2.24 11.3C2.24 16.3 6.29 20.35 11.29 20.35C15.35 20.35 18.79 17.67 20.4 14.45Z" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.85"></path>"#;

/// Warm geometric 8-ray editorial spark mark inspired by Claude's conversational emblem.
const SVG_SPARK_EMBLEM: &str = r#"<path d="M12 2.1L13.55 8.35L18.95 5.05L15.65 10.45L21.9 12L15.65 13.55L18.95 18.95L13.55 15.65L12 21.9L10.45 15.65L5.05 18.95L8.35 13.55L2.1 12L8.35 10.45L5.05 5.05L10.45 8.35L12 2.1Z" fill="currentColor"></path>"#;

/// Approval hand outline from KUI `AssistantHomeScreen`.
const SVG_APPROVAL_HAND: &str = r#"<path d="M8.5 10.5V4.75a1.25 1.25 0 0 1 2.5 0V9.5m0-5.25a1.25 1.25 0 0 1 2.5 0V9.5m0-4.5a1.25 1.25 0 0 1 2.5 0v5.5m0-3.25a1.25 1.25 0 0 1 2.5 0v6.25a6.5 6.5 0 0 1-6.5 6.5h-1.2a6.3 6.3 0 0 1-5.12-2.62l-2.18-3.03a1.32 1.32 0 0 1 2.02-1.68L7.5 14.5V6.75a1.25 1.25 0 0 1 2.5 0" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"></path>"#;

/// Soft filled folder glyph for composer context tray.
const SVG_TRAY_FOLDER: &str = r#"<path d="M2.25 6.8C2.25 5.42 3.37 4.3 4.75 4.3H8.4C9.12 4.3 9.8 4.64 10.23 5.22L11.12 6.4H19.25C20.63 6.4 21.75 7.52 21.75 8.9V17.2C21.75 18.58 20.63 19.7 19.25 19.7H4.75C3.37 19.7 2.25 18.58 2.25 17.2V6.8Z" fill="currentColor" fill-opacity="0.18" stroke="currentColor" stroke-width="1.65" stroke-linejoin="round"></path>"#;

/// Cohesive Claude-style warm spatial palette paired with KUI `ProPalette`.
#[derive(Clone, Copy, Debug)]
pub struct StudioPalette {
    pub pro: ProPalette,
    pub dark: bool,
    /// Outer window & sidebar canvas (`#F2EFE9` warm stone in Light, `#141413` warm obsidian in Dark).
    pub stage: Hsla,
    pub stage_hover: Hsla,
    pub stage_active: Hsla,
    /// Main elevated rounded-14px inset workspace surface (`#FCFAF7` / `#1C1B19`).
    pub inset_bg: Hsla,
    /// Right-hand Claude Artifacts-style Office host frame (`#F5F2EB` / `#171615`).
    pub artifact_bg: Hsla,
    /// Elevated inner cards, write deck, and floating popovers (`#FFFFFF` / `#242320`).
    pub card_bg: Hsla,
    /// Soft outer tray around the composer & segmented controls (`#EFECE4` / `#151413`).
    pub tray_bg: Hsla,
    /// Subtle surface tint for badges, pills, and quiet strips.
    pub subtle_bg: Hsla,
    /// Warm conversational user bubble (`#ECE7DC` / `#2B2926`).
    pub user_bubble: Hsla,
    /// Claude-inspired warm terracotta brand spark (`#C96442` / `#D97757`).
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_soft: Hsla,
    pub accent_ink: Hsla,
    /// Primary tactile button fill (`#1F1E1C` / `#F2EFE9`) & foreground.
    pub primary_bg: Hsla,
    pub on_primary: Hsla,
    /// Warm hairlines & borders.
    pub border: Hsla,
    pub border_soft: Hsla,
    pub border_strong: Hsla,
    /// Editorial ink hierarchy.
    pub ink: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
}

impl StudioPalette {
    pub fn get(cx: &App) -> Self {
        let pro = ProPalette::get(cx);
        if pro.dark {
            // Exact Claude Desktop Dark tokens (`--bg-300: #141413`, `--bg-200: #1f1e1d`,
            // `--bg-100: #262624`, `--bg-000: #30302e`, `--accent-brand: #d97757`)
            Self {
                pro,
                dark: true,
                stage: rgb(0x141413).into(),
                stage_hover: hsla(45. / 360., 0.12, 0.88, 0.055),
                stage_active: rgb(0x262624).into(),
                inset_bg: rgb(0x1f1e1d).into(),
                artifact_bg: rgb(0x191817).into(),
                card_bg: rgb(0x30302e).into(),
                tray_bg: rgb(0x262624).into(),
                subtle_bg: rgb(0x262624).into(),
                user_bubble: rgb(0x2d2c29).into(),
                accent: rgb(0xd97757).into(),
                accent_hover: rgb(0xe48667).into(),
                accent_soft: hsla(15. / 360., 0.63, 0.60, 0.16),
                accent_ink: rgb(0xf3a389).into(),
                primary_bg: rgb(0xd97757).into(),
                on_primary: rgb(0xffffff).into(),
                border: hsla(51. / 360., 0.16, 0.85, 0.10),
                border_soft: hsla(51. / 360., 0.16, 0.85, 0.06),
                border_strong: hsla(51. / 360., 0.16, 0.85, 0.17),
                ink: rgb(0xfaf9f5).into(),
                text: rgb(0xc2c0b6).into(),
                muted: rgb(0x918f86).into(),
                faint: rgb(0x63625c).into(),
            }
        } else {
            // Exact Claude Desktop Light tokens (`--bg-200: #f5f4ed`, `--bg-100: #faf9f5`,
            // `--bg-300: #f0eee6`, `--bg-000: #ffffff`, `--brand-000: #c6613f`, `--accent-brand: #d97757`)
            Self {
                pro,
                dark: false,
                stage: rgb(0xf4f3ec).into(),
                stage_hover: hsla(48. / 360., 0.18, 0.18, 0.05),
                stage_active: rgb(0xe9e6dc).into(),
                inset_bg: rgb(0xfaf9f5).into(),
                artifact_bg: rgb(0xf5f4ed).into(),
                card_bg: rgb(0xffffff).into(),
                tray_bg: rgb(0xf0eee6).into(),
                subtle_bg: rgb(0xf3f1e9).into(),
                user_bubble: rgb(0xf0eee6).into(),
                accent: rgb(0xc6613f).into(),
                accent_hover: rgb(0xb35333).into(),
                accent_soft: hsla(15. / 360., 0.58, 0.54, 0.12),
                accent_ink: rgb(0xa34829).into(),
                primary_bg: rgb(0xc6613f).into(),
                on_primary: rgb(0xffffff).into(),
                border: hsla(30. / 360., 0.08, 0.12, 0.095),
                border_soft: hsla(30. / 360., 0.08, 0.12, 0.055),
                border_strong: hsla(30. / 360., 0.08, 0.12, 0.16),
                ink: rgb(0x141413).into(),
                text: rgb(0x3d3d3a).into(),
                muted: rgb(0x73726c).into(),
                faint: rgb(0x9e9c94).into(),
            }
        }
    }

    /// Soft 2-layer elevation shadow for the main rounded-14px inset workspace.
    pub fn inset_shadows(&self) -> Vec<BoxShadow> {
        if self.dark {
            vec![
                BoxShadow {
                    color: hsla(0., 0., 0., 0.42),
                    offset: point(px(0.), px(4.)),
                    blur_radius: px(18.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: hsla(0., 0., 0., 0.24),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(3.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ]
        } else {
            vec![
                BoxShadow {
                    color: hsla(32. / 360., 0.22, 0.12, 0.065),
                    offset: point(px(0.), px(4.)),
                    blur_radius: px(16.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: hsla(32. / 360., 0.20, 0.12, 0.035),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(2.5),
                    spread_radius: px(0.),
                    inset: false,
                },
            ]
        }
    }

    /// Tactile card shadow for the double-deck composer write card & artifact cards.
    pub fn composer_shadows(&self) -> Vec<BoxShadow> {
        if self.dark {
            vec![BoxShadow {
                color: hsla(0., 0., 0., 0.34),
                offset: point(px(0.), px(3.)),
                blur_radius: px(12.),
                spread_radius: px(0.),
                inset: false,
            }]
        } else {
            vec![
                BoxShadow {
                    color: hsla(32. / 360., 0.24, 0.14, 0.055),
                    offset: point(px(0.), px(3.)),
                    blur_radius: px(12.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: hsla(32. / 360., 0.18, 0.14, 0.03),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(2.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ]
        }
    }

    /// Floating elevation shadow for popovers, command palette, and drawers.
    pub fn popover_shadows(&self) -> Vec<BoxShadow> {
        if self.dark {
            vec![
                BoxShadow {
                    color: hsla(0., 0., 0., 0.56),
                    offset: point(px(0.), px(10.)),
                    blur_radius: px(28.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: hsla(0., 0., 0., 0.32),
                    offset: point(px(0.), px(2.)),
                    blur_radius: px(6.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ]
        } else {
            vec![
                BoxShadow {
                    color: hsla(32. / 360., 0.24, 0.10, 0.14),
                    offset: point(px(0.), px(10.)),
                    blur_radius: px(28.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: hsla(32. / 360., 0.18, 0.10, 0.06),
                    offset: point(px(0.), px(2.)),
                    blur_radius: px(6.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ]
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DotTone {
    Neutral,
    Success,
    Info,
    Warning,
    Danger,
    Accent,
}

impl DotTone {
    pub fn dot_color(self, s: &StudioPalette) -> Hsla {
        match self {
            Self::Neutral => s.faint,
            Self::Success => s.pro.green,
            Self::Info => s.pro.accent_info,
            Self::Warning => s.pro.amber,
            Self::Danger => s.pro.red,
            Self::Accent => s.accent,
        }
    }

    pub fn text_color(self, s: &StudioPalette) -> Hsla {
        match self {
            Self::Neutral => s.text,
            Self::Success | Self::Info => s.ink,
            Self::Warning => s.pro.amber_ink,
            Self::Danger => s.pro.red_ink,
            Self::Accent => s.accent_ink,
        }
    }
}

pub fn render_claude_spark(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_SPARK_EMBLEM, side, color)
}

pub fn render_approval_hand(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_APPROVAL_HAND, side, color)
}

pub fn render_tray_folder(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_TRAY_FOLDER, side, color)
}

/// Editorial Kar Studio lockup with warm terracotta spark emblem.
pub fn render_kar_brand_lockup(s: &StudioPalette, _cx: &App) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .size(px(24.))
                .rounded(px(7.))
                .bg(s.accent_soft)
                .border_1()
                .border_color(s.accent.opacity(0.24))
                .flex()
                .items_center()
                .justify_center()
                .child(render_claude_spark(14., s.accent)),
        )
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(4.5))
                .child(
                    div()
                        .text_size(px(14.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(s.ink)
                        .child("Kar"),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(s.muted)
                        .child("Studio"),
                ),
        )
}

pub fn render_theme_glyph(dark: bool, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", if dark { SVG_SUN } else { SVG_MOON }, 14.5, color)
}

/// Map a Kar runtime identifier to KUI's official provider artwork or clean fallback glyph.
pub fn render_runtime_mark(runtime_id: &str, side: f32, s: &StudioPalette, cx: &App) -> AnyElement {
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
    pro_icon(ProIcon::Terminal, side, s.muted)
}

/// Understated capsule status pill with a 6px semantic indicator dot.
pub fn render_status_pill(
    label: impl Into<SharedString>,
    tone: DotTone,
    s: &StudioPalette,
) -> Div {
    div()
        .h(px(24.))
        .px(px(9.5))
        .rounded_full()
        .bg(s.subtle_bg)
        .border_1()
        .border_color(s.border)
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .size(px(6.))
                .rounded_full()
                .bg(tone.dot_color(s)),
        )
        .child(
            div()
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(tone.text_color(s))
                .whitespace_nowrap()
                .child(label.into()),
        )
}

/// Subtle micro-tag explicitly marking isolated demo/sample state without loud banner noise.
pub fn render_demo_pill(label: impl Into<SharedString>, s: &StudioPalette) -> Div {
    div()
        .h(px(20.))
        .px(px(6.5))
        .rounded(px(5.5))
        .bg(s.pro.amber_soft)
        .border_1()
        .border_color(s.pro.amber.opacity(0.28))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(4.))
        .text_size(px(10.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(s.pro.amber_ink)
        .whitespace_nowrap()
        .child(label.into())
}

/// Crisp Office format badge (`DOCX`, `XLSX`, `PPTX` or compact `W`, `X`, `P`).
pub fn render_office_chip(kind: OfficeKind, compact: bool, s: &StudioPalette) -> Div {
    let (bg, fg, border, text) = match kind {
        OfficeKind::Word => (
            s.pro.accent_soft,
            s.pro.accent_info,
            s.pro.accent_info.opacity(0.22),
            if compact { "W" } else { "DOCX" },
        ),
        OfficeKind::Excel => (
            s.pro.green_soft,
            s.pro.green_ink,
            s.pro.green.opacity(0.24),
            if compact { "X" } else { "XLSX" },
        ),
        OfficeKind::Ppt => (
            s.accent_soft,
            s.accent_ink,
            s.accent.opacity(0.26),
            if compact { "P" } else { "PPTX" },
        ),
    };

    div()
        .h(px(if compact { 18. } else { 19. }))
        .min_w(px(if compact { 18. } else { 36. }))
        .px(px(if compact { 4.5 } else { 6. }))
        .rounded(px(5.))
        .bg(bg)
        .border_1()
        .border_color(border)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg)
        .child(text)
}

/// Subtle keyboard shortcut badge (`⌘↵`, `⌘K`).
pub fn render_keycap(keys: &'static str, s: &StudioPalette) -> Div {
    div()
        .h(px(20.))
        .px(px(5.5))
        .rounded(px(5.))
        .bg(s.subtle_bg)
        .border_1()
        .border_color(s.border)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(10.5))
        .text_color(s.muted)
        .child(keys)
}

/// Clean human-friendly document title (removes raw temp prefixes in demo tabs while keeping clarity).
pub fn clean_display_doc_name(raw_name: &str) -> String {
    let stripped = raw_name.strip_prefix("[演示] ").unwrap_or(raw_name);
    match stripped {
        "demo_coedit_schedule.docx" => "schedule.docx".into(),
        "demo_coedit_budget.xlsx" => "budget.xlsx".into(),
        "demo_coedit_launch.pptx" => "launch.pptx".into(),
        other => other.to_owned(),
    }
}
