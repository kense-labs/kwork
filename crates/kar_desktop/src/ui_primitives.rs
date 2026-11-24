//! Kense architectural monochrome tokens, elevation shadows, and KUI visual primitives.

use gpui::{prelude::*, *};
use kui::{
    components::pro_chrome::ProPalette,
    primitives::pro_icons::{ProIcon, pro_app_artwork, pro_icon, pro_svg},
};

use crate::types::OfficeKind;

/// Height of the top window / workspace chrome bar (`44px` compact desktop cadence).
pub const HEADER_H: f32 = 44.0;
/// Left inset required on macOS to clear native traffic lights (`76px`).
pub const MACOS_TRAFFIC_LIGHT_PAD: f32 = 76.0;

const SVG_APPROVAL_HAND: &str = r#"<path d="M8.5 10.5V4.75a1.25 1.25 0 0 1 2.5 0V9.5m0-5.25a1.25 1.25 0 0 1 2.5 0V9.5m0-4.5a1.25 1.25 0 0 1 2.5 0v5.5m0-3.25a1.25 1.25 0 0 1 2.5 0v6.25a6.5 6.5 0 0 1-6.5 6.5h-1.2a6.3 6.3 0 0 1-5.12-2.62l-2.18-3.03a1.32 1.32 0 0 1 2.02-1.68L7.5 14.5V6.75a1.25 1.25 0 0 1 2.5 0" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"></path>"#;

const SVG_TRAY_FOLDER: &str = r#"<path d="M2.5 7C2.5 5.62 3.62 4.5 5 4.5H8.5C9.2 4.5 9.86 4.83 10.28 5.39L11.12 6.5H19C20.38 6.5 21.5 7.62 21.5 9V17C21.5 18.38 20.38 19.5 19 19.5H5C3.62 19.5 2.5 18.38 2.5 17V7Z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"></path>"#;

const SVG_DOC_CHIP: &str = r#"<path d="M8 7L16 7" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8"></path><path d="M8 11L12 11" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8"></path><path d="M13 21.5V21C13 18.17 13 16.76 13.88 15.88C14.76 15 16.17 15 19 15H19.5M20 13.34V10C20 6.23 20 4.34 18.83 3.17C17.66 2 15.77 2 12 2C8.23 2 6.34 2 5.17 3.17C4 4.34 4 6.23 4 10V14.54C4 17.79 4 19.41 4.89 20.51C5.07 20.73 5.27 20.93 5.49 21.11C6.59 22 8.21 22 11.46 22C12.16 22 12.51 22 12.84 21.89C13.34 21.66 13.59 21.41 14.09 20.91L18.83 16.17C19.41 15.59 19.7 15.3 19.85 14.94C20 14.57 20 14.16 20 13.34Z" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8"></path>"#;

const SVG_SHEET_CHIP: &str = r#"<rect x="3" y="4" width="18" height="16" rx="3" stroke="currentColor" stroke-width="1.8"></rect><path d="M3 9.5H21M9.5 9.5V20" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"></path>"#;

const SVG_SLIDE_CHIP: &str = r#"<path d="M12 18V22M10 18L6 22M14 18L18 22" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8"></path><rect x="2.5" y="3" width="19" height="15" rx="3" stroke="currentColor" stroke-width="1.8"></rect><path d="M8 13V10M12 13V7.5M16 13V11" stroke="currentColor" stroke-linecap="round" stroke-width="1.8"></path>"#;

/// Kense architectural monochrome palette paired with KUI `ProPalette`.
#[derive(Clone, Copy, Debug)]
pub struct StudioPalette {
    pub pro: ProPalette,
    pub dark: bool,
    /// Outer window & sidebar canvas (`#F6F6F7` in Light, `#0C0C0E` in Dark).
    pub stage: Hsla,
    pub stage_hover: Hsla,
    pub stage_active: Hsla,
    /// Main elevated rounded-12px inset workspace surface (`#FFFFFF` / `#131316`).
    pub inset_bg: Hsla,
    /// Subtle Office host header wash (`#FAFAFB` / `#16161A`).
    pub artifact_bg: Hsla,
    /// Elevated inner cards, write deck, and floating popovers (`#FFFFFF` / `#1B1B1F`).
    pub card_bg: Hsla,
    /// Monochrome outer tray around the composer (`#F4F4F5` / `#18181C`).
    pub tray_bg: Hsla,
    /// Subtle surface tint for badges, pills, and quiet strips.
    pub subtle_bg: Hsla,
    /// Conversational user bubble (`#F4F4F5` / `#222227`).
    pub user_bubble: Hsla,
    /// Primary Kense monochrome ink accent (`#171717` / `#F4F4F5`).
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_soft: Hsla,
    pub accent_ink: Hsla,
    /// Primary tactile button fill & foreground.
    pub primary_bg: Hsla,
    pub on_primary: Hsla,
    /// Architectural hairlines & borders.
    pub border: Hsla,
    pub border_soft: Hsla,
    pub border_strong: Hsla,
    /// Typography ink hierarchy.
    pub ink: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
}

impl StudioPalette {
    pub fn get(cx: &App) -> Self {
        let pro = ProPalette::get(cx);
        if pro.dark {
            Self {
                pro,
                dark: true,
                stage: rgb(0x0c0c0e).into(),
                stage_hover: hsla(0., 0., 1., 0.05),
                stage_active: hsla(0., 0., 1., 0.085),
                inset_bg: rgb(0x131316).into(),
                artifact_bg: rgb(0x16161a).into(),
                card_bg: rgb(0x1b1b1f).into(),
                tray_bg: rgb(0x18181c).into(),
                subtle_bg: rgb(0x1c1c21).into(),
                user_bubble: rgb(0x222227).into(),
                accent: rgb(0xfafafa).into(),
                accent_hover: rgb(0xe4e4e7).into(),
                accent_soft: hsla(0., 0., 1., 0.08),
                accent_ink: rgb(0xf4f4f5).into(),
                primary_bg: rgb(0xf4f4f5).into(),
                on_primary: rgb(0x09090b).into(),
                border: hsla(0., 0., 1., 0.08),
                border_soft: hsla(0., 0., 1., 0.05),
                border_strong: hsla(0., 0., 1., 0.14),
                ink: rgb(0xfafafa).into(),
                text: rgb(0xa1a1aa).into(),
                muted: rgb(0x71717a).into(),
                faint: rgb(0x52525b).into(),
            }
        } else {
            Self {
                pro,
                dark: false,
                stage: rgb(0xf6f6f7).into(),
                stage_hover: hsla(0., 0., 0., 0.04),
                stage_active: hsla(0., 0., 0., 0.065),
                inset_bg: rgb(0xffffff).into(),
                artifact_bg: rgb(0xfafafb).into(),
                card_bg: rgb(0xffffff).into(),
                tray_bg: rgb(0xf4f4f5).into(),
                subtle_bg: rgb(0xf4f4f5).into(),
                user_bubble: rgb(0xf4f4f5).into(),
                accent: rgb(0x171717).into(),
                accent_hover: rgb(0x27272a).into(),
                accent_soft: hsla(0., 0., 0., 0.055),
                accent_ink: rgb(0x171717).into(),
                primary_bg: rgb(0x171717).into(),
                on_primary: rgb(0xffffff).into(),
                border: hsla(0., 0., 0., 0.08),
                border_soft: hsla(0., 0., 0., 0.048),
                border_strong: hsla(0., 0., 0., 0.13),
                ink: rgb(0x09090b).into(),
                text: rgb(0x52525b).into(),
                muted: rgb(0x71717a).into(),
                faint: rgb(0xa1a1aa).into(),
            }
        }
    }

    /// Subtle 2-layer architectural shadow for the rounded-12px inset workspace.
    pub fn inset_shadows(&self) -> Vec<BoxShadow> {
        if self.dark {
            vec![
                BoxShadow {
                    color: hsla(0., 0., 0., 0.42),
                    offset: point(px(0.), px(4.)),
                    blur_radius: px(16.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: hsla(0., 0., 0., 0.24),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(2.5),
                    spread_radius: px(0.),
                    inset: false,
                },
            ]
        } else {
            vec![
                BoxShadow {
                    color: hsla(0., 0., 0., 0.045),
                    offset: point(px(0.), px(3.)),
                    blur_radius: px(12.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: hsla(0., 0., 0., 0.025),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(2.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ]
        }
    }

    /// Clean low-amplitude shadow for elevated inner cards.
    pub fn composer_shadows(&self) -> Vec<BoxShadow> {
        if self.dark {
            vec![BoxShadow {
                color: hsla(0., 0., 0., 0.28),
                offset: point(px(0.), px(2.)),
                blur_radius: px(8.),
                spread_radius: px(0.),
                inset: false,
            }]
        } else {
            vec![BoxShadow {
                color: hsla(0., 0., 0., 0.035),
                offset: point(px(0.), px(1.5)),
                blur_radius: px(5.),
                spread_radius: px(0.),
                inset: false,
            }]
        }
    }

    /// Floating shadow for popovers, command desk, and modal sheets.
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
                    color: hsla(0., 0., 0., 0.11),
                    offset: point(px(0.), px(10.)),
                    blur_radius: px(26.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: hsla(0., 0., 0., 0.045),
                    offset: point(px(0.), px(2.)),
                    blur_radius: px(5.),
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
            Self::Accent => s.ink,
        }
    }

    pub fn text_color(self, s: &StudioPalette) -> Hsla {
        match self {
            Self::Neutral => s.text,
            Self::Success | Self::Info | Self::Accent => s.ink,
            Self::Warning => s.pro.amber_ink,
            Self::Danger => s.pro.red_ink,
        }
    }
}

pub fn render_approval_hand(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_APPROVAL_HAND, side, color)
}

pub fn render_tray_folder(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_TRAY_FOLDER, side, color)
}

pub fn render_doc_outline_icon(kind: OfficeKind, side: f32, color: Hsla) -> AnyElement {
    let path = match kind {
        OfficeKind::Word => SVG_DOC_CHIP,
        OfficeKind::Excel => SVG_SHEET_CHIP,
        OfficeKind::Ppt => SVG_SLIDE_CHIP,
    };
    pro_svg("0 0 24 24", path, side, color)
}

/// Official Kense mark + `Kar` lockup (`BRAND.md` monochrome spec).
pub fn render_kar_brand_lockup(s: &StudioPalette, cx: &App) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(7.))
        .when_some(pro_app_artwork("kense", 20., cx), |d, mark| d.child(mark))
        .child(
            div()
                .text_size(px(14.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(s.ink)
                .child("Kar"),
        )
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

/// Understated capsule status pill (`22px` height) — shown only for non-default states.
pub fn render_status_pill(
    label: impl Into<SharedString>,
    tone: DotTone,
    s: &StudioPalette,
) -> Div {
    div()
        .h(px(22.))
        .px(px(8.5))
        .rounded_full()
        .bg(s.subtle_bg)
        .border_1()
        .border_color(s.border)
        .flex_none()
        .flex()
        .items_center()
        .gap(px(5.5))
        .child(
            div()
                .size(px(5.5))
                .rounded_full()
                .bg(tone.dot_color(s)),
        )
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(tone.text_color(s))
                .whitespace_nowrap()
                .child(label.into()),
        )
}

/// Subtle micro-tag explicitly marking isolated demo/sample state without loud banner noise.
pub fn render_demo_pill(label: impl Into<SharedString>, s: &StudioPalette) -> Div {
    div()
        .h(px(19.))
        .px(px(6.))
        .rounded(px(5.))
        .bg(s.pro.amber_soft)
        .border_1()
        .border_color(s.pro.amber.opacity(0.24))
        .flex_none()
        .flex()
        .items_center()
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
            s.pro.accent_info.opacity(0.20),
            if compact { "W" } else { "DOCX" },
        ),
        OfficeKind::Excel => (
            s.pro.green_soft,
            s.pro.green_ink,
            s.pro.green.opacity(0.22),
            if compact { "X" } else { "XLSX" },
        ),
        OfficeKind::Ppt => (
            s.pro.amber_soft,
            s.pro.amber_ink,
            s.pro.amber.opacity(0.24),
            if compact { "P" } else { "PPTX" },
        ),
    };

    div()
        .h(px(if compact { 17. } else { 18. }))
        .min_w(px(if compact { 17. } else { 34. }))
        .px(px(if compact { 4. } else { 5.5 }))
        .rounded(px(4.5))
        .bg(bg)
        .border_1()
        .border_color(border)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(9.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg)
        .child(text)
}

/// Subtle keyboard shortcut badge (`⌘↵`, `⌘K`).
pub fn render_keycap(keys: &'static str, s: &StudioPalette) -> Div {
    div()
        .h(px(19.))
        .px(px(5.))
        .rounded(px(4.5))
        .bg(s.subtle_bg)
        .border_1()
        .border_color(s.border)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(10.))
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
