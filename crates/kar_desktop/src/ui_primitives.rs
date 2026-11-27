//! Shared typography, surfaces and icons for the Kar workbench.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex, OnceLock},
};

use gpui::{prelude::*, *};
use kui::{
    components::pro_chrome::ProPalette,
    primitives::pro_icons::{ProIcon, pro_app_artwork, pro_artwork, pro_icon, pro_symbol},
};

pub(crate) mod material;
pub(crate) use material::{frosted_card, frosted_composer, frosted_panel};

use crate::types::{OfficeKind, runtime_brand_id, runtime_brand_svg};

// ── UI Locale (`中` / `EN`) ──────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UiLocale {
    #[default]
    Zh,
    En,
}

impl UiLocale {
    pub fn toggle(self) -> Self {
        match self {
            Self::Zh => Self::En,
            Self::En => Self::Zh,
        }
    }

    pub fn pick<'a>(self, zh: &'a str, en: &'a str) -> &'a str {
        match self {
            Self::Zh => zh,
            Self::En => en,
        }
    }

    pub fn short_badge(self) -> &'static str {
        match self {
            Self::Zh => "中",
            Self::En => "EN",
        }
    }
}

// ── Desktop workspace geometry in logical pixels ─

pub const HEADER_H: f32 = 52.0;
pub const TITLEBAR_EDGE_INSET: f32 = 14.0;
pub const TITLEBAR_BUTTON_GAP: f32 = 4.0;
// Keep a 6 pt top inset above the 28 pt toolbar buttons.
pub(crate) const MACOS_TITLEBAR_CENTER_Y: f32 = 20.;
// The system controls end at 72 pt; 12 more to the button slot equalizes the visible icon gaps.
pub const MACOS_TRAFFIC_LIGHT_PAD: f32 = 84.;
pub const SIDEBAR_W: f32 = 257.0;
pub(crate) const DIVIDER_HIT_WIDTH: f32 = 13.;
pub(crate) const FOLDED_GRIP_HIT_WIDTH: f32 = 28.;
pub const SHEET_GUTTER: f32 = 1.0;
pub const DOC_MIN_W: f32 = 380.0;
pub const THREAD_W: f32 = 736.0;
pub const THREAD_PAD: f32 = 24.0;
pub const DETAILS_CARD_W: f32 = 276.0;
pub const TERMINAL_DEFAULT_H: f32 = 280.0;
pub const TERMINAL_MIN_H: f32 = 176.0;

/// Fullscreen-aware left padding for macOS window controls.
pub fn macos_traffic_pad(window: &Window) -> f32 {
    if !cfg!(target_os = "macos") || window.is_fullscreen() {
        TITLEBAR_EDGE_INSET
    } else {
        MACOS_TRAFFIC_LIGHT_PAD
    }
}

pub(crate) fn titlebar_center_y(window: &Window) -> f32 {
    if cfg!(target_os = "macos") && !window.is_fullscreen() {
        MACOS_TITLEBAR_CENTER_Y
    } else {
        HEADER_H / 2.
    }
}

pub(crate) fn titlebar_row(window: &Window) -> Div {
    div()
        .h(px(HEADER_H))
        .pb(px(HEADER_H - 2. * titlebar_center_y(window)))
        .window_control_area(WindowControlArea::Drag)
        .flex_none()
        .flex()
        .items_center()
}


pub const FS_TAG: f32 = 11.0;
pub const FS_META: f32 = 12.0;
pub const FS_CONTROL: f32 = 13.0;
pub const FS_UI: f32 = 14.0;
pub const FS_READ: f32 = 15.0;
pub const FS_SECTION: f32 = 16.0;
pub const FS_TITLE: f32 = 18.0;
pub const FS_METRIC: f32 = 20.0;
pub const FS_HERO: f32 = 22.0;

pub const LH_META: f32 = 16.0;
pub const LH_CONTROL: f32 = 18.0;
pub const LH_UI: f32 = 20.0;
pub const LH_METRIC: f32 = 26.0;
pub const LH_READ: f32 = 22.5;


pub const CTRL_H: f32 = 32.0;
pub const CTRL_SM_H: f32 = 28.0;
pub const CTRL_XS_H: f32 = 24.0;
pub const ROW_H: f32 = 36.0;
pub const ICON: f32 = 16.0;
pub const ICON_SM: f32 = 14.0;
pub const ICON_XS: f32 = 12.0;

pub const R_XS: f32 = 4.0;
pub const R_SM: f32 = 6.0;
pub const R_CTRL: f32 = 8.0;
pub const R_CARD: f32 = 12.0;
pub const R_HERO: f32 = 12.0;
pub const R_MODAL: f32 = 16.0;
pub const R_PILL: f32 = 999.0;

/// Global optional named `kantor_theme::Palette` override (`None` = signature Kwrok Monochrome).
#[derive(Clone, Debug, Default)]
pub struct ActiveNamedTheme(pub Option<kantor_theme::Palette>);
impl Global for ActiveNamedTheme {}

/// Global user typography & code font preferences applied across Kwrok.
#[derive(Clone, Debug)]
pub struct ActiveTypography {
    pub ui_delta_px: f32,
    pub code_font_size: f32,
    pub code_font_family: Option<SharedString>,
}
impl Default for ActiveTypography {
    fn default() -> Self {
        Self {
            ui_delta_px: 0.0,
            code_font_size: FS_CONTROL,
            code_font_family: None,
        }
    }
}
impl Global for ActiveTypography {}

pub fn scaled_fs(base: f32, cx: &App) -> f32 {
    let delta = cx
        .try_global::<ActiveTypography>()
        .map_or(0.0, |t| t.ui_delta_px);
    (base + delta).clamp(10.0, 36.0)
}

pub fn effective_code_font_size(cx: &App) -> f32 {
    cx.try_global::<ActiveTypography>()
        .map_or(FS_CONTROL, |t| t.code_font_size)
}

// ── KUI Sculpted Vector Icons & Artwork (`assistant_home.rs` + `pro_icons.rs`) ─

const HAND_APPROVAL_SVG: &str = r#"<path d="M52.5305 32.6557V24.8463C52.5305 19.8766 57.5001 19.8766 57.5001 19.8766C60.2448 19.8766 62.4698 22.1016 62.4698 24.8463V42.027C62.4698 53.3194 53.3154 62.4735 42.0233 62.4735H34.2388M22.7127 39.7552L15.94 31.8537C14.8314 30.5602 13.2129 29.8159 11.5094 29.8159H11.1235C8.11379 29.8159 5.67395 32.2558 5.67395 35.2655C5.67395 36.3412 5.99241 37.393 6.58919 38.2881L16.4118 53.0221C20.3482 58.9269 26.9753 62.4735 34.0719 62.4735" stroke="currentColor" stroke-width="4.7" stroke-linecap="round" stroke-linejoin="round"></path><path d="M52.534 24.1378V16.3284C52.534 13.5837 50.3091 11.3588 47.5644 11.3588C44.8197 11.3588 42.5948 13.5837 42.5948 16.3284V29.8174" stroke="currentColor" stroke-width="4.7" stroke-linecap="round" stroke-linejoin="round"></path><path d="M42.5958 29.8198V10.6513C42.5958 7.90661 40.3708 5.68163 37.6262 5.68163C34.8815 5.68163 32.6565 7.90661 32.6565 10.6513V28.4" stroke="currentColor" stroke-width="4.7" stroke-linecap="round" stroke-linejoin="round"></path><path d="M32.6576 28.3975V16.3284C32.6576 13.5837 30.4326 11.3588 27.6879 11.3588C24.9433 11.3588 22.7183 13.5837 22.7183 16.3284V39.7567" stroke="currentColor" stroke-width="4.7" stroke-linecap="round" stroke-linejoin="round"></path>"#;

const CHIP_KNOWLEDGE_SVG: &str = r#"<path d="M3 12C3 7.75736 3 5.63604 4.31802 4.31802C5.63604 3 7.75736 3 12 3C16.2426 3 18.364 3 19.682 4.31802C21 5.63604 21 7.75736 21 12C21 16.2426 21 18.364 19.682 19.682C18.364 21 16.2426 21 12 21C7.75736 21 5.63604 21 4.31802 19.682C3 18.364 3 16.2426 3 12Z" stroke="currentColor" stroke-width="1.9"></path><path d="M3 12H21" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M11 7.5L17 7.5" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M7.125 7.5H7M7.25 7.5C7.25 7.63807 7.13807 7.75 7 7.75C6.86193 7.75 6.75 7.63807 6.75 7.5C6.75 7.36193 6.86193 7.25 7 7.25C7.13807 7.25 7.25 7.36193 7.25 7.5Z" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M11 16.5L17 16.5" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M7.125 16.5H7M7.25 16.5C7.25 16.6381 7.13807 16.75 7 16.75C6.86193 16.75 6.75 16.6381 6.75 16.5C6.75 16.3619 6.86193 16.25 7 16.25C7.13807 16.25 7.25 16.3619 7.25 16.5Z" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path>"#;

const CHIP_DOCUMENT_SVG: &str = r#"<path d="M8 7L16 7" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M8 11L12 11" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M13 21.5V21C13 18.1716 13 16.7574 13.8787 15.8787C14.7574 15 16.1716 15 19 15H19.5M20 13.3431V10C20 6.22876 20 4.34315 18.8284 3.17157C17.6569 2 15.7712 2 12 2C8.22877 2 6.34315 2 5.17157 3.17157C4 4.34314 4 6.22876 4 10L4 14.5442C4 17.7892 4 19.4117 4.88607 20.5107C5.06508 20.7327 5.26731 20.9349 5.48933 21.1139C6.58831 22 8.21082 22 11.4558 22C12.1614 22 12.5141 22 12.8372 21.886C12.9044 21.8623 12.9702 21.835 13.0345 21.8043C13.3436 21.6564 13.593 21.407 14.0919 20.9081L18.8284 16.1716C19.4065 15.5935 19.6955 15.3045 19.8478 14.9369C20 14.5694 20 14.1606 20 13.3431Z" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path>"#;

const CHIP_PRESENTATION_SVG: &str = r#"<path d="M12 18V22" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M10 18L5 22" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M14 18L19 22" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M2.5 12C2.5 14.8284 2.5 16.2426 3.37868 17.1213C4.25736 18 5.67157 18 8.5 18H15.5C18.3284 18 19.7426 18 20.6213 17.1213C21.5 16.2426 21.5 14.8284 21.5 12V8C21.5 5.17157 21.5 3.75736 20.6213 2.87868C19.7426 2 18.3284 2 15.5 2H8.5C5.67157 2 4.25736 2 3.37868 2.87868C2.5 3.75736 2.5 5.17157 2.5 8V12Z" stroke="currentColor" stroke-width="1.9"></path><path d="M8 13V10M12 13V7M16 13V11" stroke="currentColor" stroke-linecap="round" stroke-width="1.9"></path>"#;

const CHIP_TABLE_SVG: &str = r#"<path d="M20 10C20 10 22.375 11.8222 21.375 14.6667C20.375 17.5111 21.375 19.4074 22 20M20 10H4M20 10V4M4 10C4 10 1.625 11.8222 2.625 14.6667C3.625 17.5111 2.625 19.4074 2 20M4 10V4M22 4H20.75H20M2 4H3.25H4M4 4H20" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M11 7H13" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path>"#;

const CHIP_RESEARCH_SVG: &str = r#"<path d="M16.1995 2.62118C17.0064 2.153 17.4099 1.91892 17.8053 2.02539C18.2007 2.13186 18.4336 2.53732 18.8995 3.34823L20.3819 5.92852C20.8478 6.73943 21.0807 7.14489 20.9747 7.54225C20.8688 7.93961 20.4653 8.1737 19.6584 8.64188L16.235 10.6282C15.4281 11.0964 15.0247 11.3305 14.6293 11.224C14.2339 11.1175 14.0009 10.7121 13.5351 9.90115L12.0527 7.32085C11.5868 6.50994 11.3539 6.10449 11.4598 5.70713C11.5658 5.30977 11.9692 5.07568 12.7761 4.6075L16.1995 2.62118Z" stroke="currentColor" stroke-width="1.9"></path><path d="M11.5585 6.46075L14.0292 10.7613L10.6058 12.7476C9.79886 13.2157 9.39541 13.4498 9.00001 13.3434C8.60461 13.2369 8.37167 12.8314 7.9058 12.0205L7.41168 11.1604C6.94581 10.3495 6.71288 9.94406 6.81882 9.5467C6.92477 9.14934 7.32822 8.91525 8.13513 8.44707L11.5585 6.46075Z" stroke="currentColor" stroke-width="1.9"></path><path d="M6.91755 10.3003L8.39993 12.8806L5.40444 14.6186C5.00566 14.85 4.80627 14.9657 4.61138 14.9915C4.35152 15.0259 4.08871 14.9551 3.88077 14.7948C3.72482 14.6745 3.6097 14.4741 3.37947 14.0734C3.14923 13.6726 3.03412 13.4722 3.00846 13.2764C2.97425 13.0152 3.04467 12.7511 3.20422 12.5421C3.32389 12.3854 3.52328 12.2697 3.92206 12.0384L6.91755 10.3003Z" stroke="currentColor" stroke-width="1.9"></path><path d="M7.5 22L12 12L16.5 22" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path>"#;

const CHIP_VISUALIZE_SVG: &str = r#"<path d="M8 9V17" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M13 5V17" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M18 13V17" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M3 3V13C3 16.7712 3 18.6569 4.17157 19.8284C5.34315 21 7.22876 21 11 21H21" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path>"#;

const ART_DOC_SVG: &str = r#"<path d="M3.3 1h3.9L10 3.8v6.4c0 .72-.58 1.3-1.3 1.3H3.3c-.72 0-1.3-.58-1.3-1.3V2.3C2 1.58 2.58 1 3.3 1Z" fill="currentColor" opacity="0.45"></path><path d="M7.2 1v1.8c0 .55.45 1 1 1H10Z" fill="currentColor"></path><path d="M4 6.6h4M4 8.6h2.6" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"></path>"#;

const ART_SHEET_SVG: &str = r#"<path d="M3.3 1h3.9L10 3.8v6.4c0 .72-.58 1.3-1.3 1.3H3.3c-.72 0-1.3-.58-1.3-1.3V2.3C2 1.58 2.58 1 3.3 1Z" fill="currentColor" opacity="0.45"></path><path d="M7.2 1v1.8c0 .55.45 1 1 1H10Z" fill="currentColor"></path><rect x="3.4" y="5.6" width="2.3" height="1.6" rx="0.4" fill="currentColor"></rect><rect x="6.3" y="5.6" width="2.3" height="1.6" rx="0.4" fill="currentColor"></rect><rect x="3.4" y="7.8" width="2.3" height="1.6" rx="0.4" fill="currentColor"></rect><rect x="6.3" y="7.8" width="2.3" height="1.6" rx="0.4" fill="currentColor"></rect>"#;

const ART_SLIDE_SVG: &str = r#"<rect x="1.5" y="1.8" width="9" height="6.8" rx="1.3" fill="currentColor" opacity="0.45"></rect><path d="M4.2 6.2V4.7M6 6.2V3.5M7.8 6.2V5.1" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"></path><path d="M6 8.6v2M4.2 10.6L6 8.8l1.8 1.8" stroke="currentColor" stroke-width="1.05" stroke-linecap="round" stroke-linejoin="round"></path>"#;

const SVG_FOLDER_GIT2: &str = r#"<path d="M9 20H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H20a2 2 0 0 1 2 2v5" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"></path><circle cx="13" cy="12" r="2" stroke="currentColor" stroke-width="1.75"></circle><path d="M18 19c-2.8 0-5-2.2-5-5v8" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"></path><circle cx="20" cy="19" r="2" stroke="currentColor" stroke-width="1.75"></circle>"#;

const SVG_LIST_FILTER: &str = r#"<path d="M3 6h18M7 12h10m-7 6h4" stroke="currentColor" stroke-width="1.85" stroke-linecap="round" stroke-linejoin="round"></path>"#;

const SVG_SQUARE_MENU: &str = r#"<rect width="18" height="18" x="3" y="3" rx="2" stroke="currentColor" stroke-width="1.8"></rect><path d="M7 8h10M7 12h10M7 16h10" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"></path>"#;

const SVG_FILE_DIFF: &str = r#"<rect width="18" height="18" x="3" y="3" rx="2" stroke="currentColor" stroke-width="1.8"></rect><path d="M12 7v6m-3-3h6m-6 6h6" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"></path>"#;

const SVG_WORKFLOWS_FORK: &str = r#"<circle cx="6" cy="6" r="2.2" stroke="currentColor" stroke-width="1.8"></circle><circle cx="18" cy="6" r="2.2" stroke="currentColor" stroke-width="1.8"></circle><circle cx="6" cy="18" r="2.2" stroke="currentColor" stroke-width="1.8"></circle><path d="M6 8.2v7.6M18 8.2c0 4.5-12 3.1-12 7.6" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"></path>"#;

const SVG_CHART_COLUMNS: &str = r#"<path d="M5 20v-6m7 6V4m7 16v-9" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"></path>"#;

const SVG_CIRCLE_CHECK: &str = r#"<circle cx="12" cy="12" r="9" stroke="currentColor" stroke-width="1.8"></circle><path d="m8.5 12.2 2.3 2.3 4.8-4.8" stroke="currentColor" stroke-width="1.85" stroke-linecap="round" stroke-linejoin="round"></path>"#;

const SVG_MONITOR_MACHINE: &str = r#"<rect width="18" height="12" x="3" y="4" rx="2" stroke="currentColor" stroke-width="1.75"></rect><path d="M8 20h8m-4-4v4" stroke="currentColor" stroke-width="1.75" stroke-linecap="round"></path>"#;

const SVG_GIT_COMMIT_CIRCLE: &str = r#"<circle cx="12" cy="12" r="3.5" stroke="currentColor" stroke-width="1.8"></circle><path d="M3 12h5.5m7 0H21" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"></path>"#;

const SVG_DOC_CHIP: &str = r#"<path d="M8 7L16 7" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8"></path><path d="M8 11L12 11" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8"></path><path d="M13 21.5V21C13 18.17 13 16.76 13.88 15.88C14.76 15 16.17 15 19 15H19.5M20 13.34V10C20 6.23 20 4.34 18.83 3.17C17.66 2 15.77 2 12 2C8.23 2 6.34 2 5.17 3.17C4 4.34 4 6.23 4 10V14.54C4 17.79 4 19.41 4.89 20.51C5.07 20.73 5.27 20.93 5.49 21.11C6.59 22 8.21 22 11.46 22C12.16 22 12.51 22 12.84 21.89C13.34 21.66 13.59 21.41 14.09 20.91L18.83 16.17C19.41 15.59 19.7 15.3 19.85 14.94C20 14.57 20 14.16 20 13.34Z" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8"></path>"#;

const SVG_SHEET_CHIP: &str = r#"<rect x="3" y="4" width="18" height="16" rx="3" stroke="currentColor" stroke-width="1.8"></rect><path d="M3 9.5H21M9.5 9.5V20" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"></path>"#;

const SVG_SLIDE_CHIP: &str = r#"<path d="M12 18V22M10 18L6 22M14 18L18 22" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8"></path><rect x="2.5" y="3" width="19" height="15" rx="3" stroke="currentColor" stroke-width="1.8"></rect><path d="M8 13V10M12 13V7.5M16 13V11" stroke="currentColor" stroke-linecap="round" stroke-width="1.8"></path>"#;

const SVG_RADAR_LINK: &str = r#"<circle cx="12" cy="12" r="2.3" stroke="currentColor" stroke-width="1.9"></circle><path d="M7.75 16.25C5.4 13.9 5.4 10.1 7.75 7.75M16.25 7.75C18.6 10.1 18.6 13.9 16.25 16.25" stroke="currentColor" stroke-linecap="round" stroke-width="1.9"></path><path d="M4.6 19.4C0.5 15.3 0.5 8.7 4.6 4.6M19.4 4.6C23.5 8.7 23.5 15.3 19.4 19.4" stroke="currentColor" stroke-linecap="round" stroke-width="1.9"></path>"#;

const SVG_SCHEDULE_REPEAT: &str = r#"<path d="M16.4 3L17.4 3.98C17.84 4.41 18.06 4.63 17.98 4.81C17.91 5 17.59 5 16.96 5H9.2C5.22 5 2 8.13 2 12C2 13.49 2.48 14.87 3.29 16" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M7.6 21L6.6 20.02C6.16 19.59 5.94 19.37 6.02 19.19C6.09 19 6.41 19 7.04 19H14.8C18.78 19 22 15.87 22 12C22 10.51 21.52 9.13 20.71 8" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M12 8.5V12L14.2 13.4" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path>"#;

const SVG_BRIEF_CARDS: &str = r#"<rect x="3" y="3.5" width="18" height="17" rx="4" stroke="currentColor" stroke-width="1.9"></rect><path d="M7.5 15.5V11.5M12 15.5V8.5M16.5 15.5V13" stroke="currentColor" stroke-linecap="round" stroke-width="1.9"></path>"#;

const SVG_STEER_MERGE: &str = r#"<path d="M6 19V5M6 5L3 8M6 5L9 8" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M18 19C18 14.5 15 12 10.5 12H6" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><circle cx="18" cy="19" r="2" stroke="currentColor" stroke-width="1.9"></circle>"#;

const SVG_BG_LAYERS: &str = r#"<path d="M12 3L21 7.8L12 12.6L3 7.8L12 3Z" stroke="currentColor" stroke-linejoin="round" stroke-width="1.85"></path><path d="M3 12.5L12 17.3L21 12.5" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.85"></path><path d="M3 16.8L12 21.6L21 16.8" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.85"></path>"#;

const SVG_SKILLS_PUZZLE: &str = r#"<path d="M12 3L14.2 7.6L19.2 8.3L15.6 11.8L16.5 16.8L12 14.4L7.5 16.8L8.4 11.8L4.8 8.3L9.8 7.6L12 3Z" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.85"></path><path d="M5 20.5H19" stroke="currentColor" stroke-linecap="round" stroke-width="1.85"></path>"#;

const SVG_USAGE_PULSE: &str = r#"<path d="M5 20v-6m7 6V4m7 16v-9" stroke="currentColor" stroke-width="1.95" stroke-linecap="round"></path>"#;

const SVG_HANDOFF_RELAY: &str = r#"<path d="M4 9H17.5L14 5.5" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M20 15H6.5L10 18.5" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path>"#;

const SVG_TERMINAL_LOGS: &str = r#"<rect x="3" y="4" width="18" height="16" rx="3" stroke="currentColor" stroke-width="1.85"></rect><path d="M7.5 9.5L10.5 12L7.5 14.5" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.85"></path><path d="M12.5 14.5H16.5" stroke="currentColor" stroke-linecap="round" stroke-width="1.85"></path>"#;

const SVG_SIDE_QUESTION: &str = r#"<path d="M12 21C16.9706 21 21 17.1944 21 12.5C21 7.80558 16.9706 4 12 4C7.02944 4 3 7.80558 3 12.5C3 14.46 3.7 16.26 4.88 17.7L3.8 20.8L7.4 19.85C8.76 20.58 10.33 21 12 21Z" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.85"></path><path d="M9.8 10.2C9.8 8.98 10.78 8 12 8C13.22 8 14.2 8.98 14.2 10.2C14.2 11.15 13.6 11.95 12.75 12.26C12.3 12.42 12 12.82 12 13.3V13.6" stroke="currentColor" stroke-linecap="round" stroke-width="1.85"></path><circle cx="12" cy="16.4" r="0.95" fill="currentColor"></circle>"#;

// ── Authentic KUI `ProPalette` + `AppShellLayout::Inset` Surface Hierarchy ──

#[derive(Clone, Copy, Debug)]
pub struct StudioPalette {
    /// KUI's palette for status accents and KUI internal widgets.
    pub pro: ProPalette,
    text_delta: f32,
    pub dark: bool,
    pub stage: Hsla,
    pub surface: Hsla,
    pub tray: Hsla,
    pub card: Hsla,
    pub message_surface: Hsla,
    pub sidebar_selected: Hsla,
    pub sidebar_hover: Hsla,
    pub soft: Hsla,
    pub soft_hover: Hsla,
    pub hover: Hsla,
    pub active: Hsla,
    pub line: Hsla,
    pub line_soft: Hsla,
    pub line_strong: Hsla,
    pub ink: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    pub primary: Hsla,
    pub on_primary: Hsla,
    pub brand_action: Hsla,
    pub on_brand_action: Hsla,
}

impl StudioPalette {
    pub fn get(cx: &App) -> Self {
        let pro = ProPalette::get(cx);
        let text_delta = cx.try_global::<ActiveTypography>().map_or(0., |t| t.ui_delta_px);
        if let Some(named) = cx
            .try_global::<ActiveNamedTheme>()
            .and_then(|g| g.0.as_ref())
        {
            let kt = kantor_ui::tokens::Tokens::from_palette(named);
            let dark = named.dark;
            let ink = kt.fg;
            return Self {
                pro,
                text_delta,
                dark,
                stage: kt.bg,
                surface: kt.bg,
                tray: kt.chrome,
                card: kt.surface,
                message_surface: kt.selected,
                sidebar_selected: kt.elevated,
                sidebar_hover: kt.hover,
                soft: kt.chrome,
                soft_hover: ink.opacity(0.10),
                hover: ink.opacity(0.06),
                active: ink.opacity(0.11),
                line: kt.border,
                line_soft: kt.border.opacity(0.72),
                line_strong: kt.border_strong,
                ink,
                text: kt.fg_muted,
                muted: kt.fg_dim,
                faint: kt.fg_dim.opacity(0.82),
                primary: kt.accent,
                on_primary: kt.accent_fg,
                brand_action: kt.accent,
                on_brand_action: kt.accent_fg,
            };
        }
        let dark = pro.dark;
        let tone = |light, night| Hsla::from(rgb(if dark { night } else { light }));
        let ink = tone(0x18181b, 0xf5f5f5);
        let card = tone(0xffffff, 0x0e0e0e);
        let muted = tone(0x5f5f68, 0xa3a3a3);
        let line = tone(0xe4e4e7, 0x18181b);
        let primary = tone(0x1447e6, 0x4779e9);
        Self {
            pro,
            text_delta,
            dark,
            stage: tone(0xfafafa, 0x050505),
            surface: tone(0xfcfcfc, 0x09090b),
            tray: tone(0xf4f4f5, 0x121215),
            card,
            message_surface: tone(0xebebed, 0x212124),
            sidebar_selected: tone(0xffffff, 0x19191d),
            sidebar_hover: tone(0xfdfdfd, 0x131316),
            soft: tone(0xf4f4f5, 0x18181b),
            soft_hover: ink.opacity(0.09),
            hover: ink.opacity(0.055),
            active: ink.opacity(0.10),
            line,
            line_soft: tone(0xebebef, 0x1d1d22),
            line_strong: tone(0xd4d4d8, 0x34343a),
            ink,
            text: tone(0x3f3f46, 0xd4d4d8),
            muted,
            faint: tone(0x61616b, 0x8e8e96),
            primary,
            on_primary: white(),
            brand_action: primary,
            on_brand_action: white(),
        }
    }

    pub fn font_size(&self, base: f32) -> Pixels {
        px(base + self.text_delta)
    }

    pub fn line_height(&self, base: f32) -> Pixels {
        px(base * (1. + self.text_delta / FS_UI))
    }

    /// Popover & modal glass elevation shadow.
    pub fn popover_shadow(&self) -> Vec<BoxShadow> {
        let (far, near) = if self.dark { (0.32, 0.15) } else { (0.10, 0.035) };
        vec![
            BoxShadow {
                color: hsla(0., 0., 0., far),
                offset: point(px(0.), px(7.)),
                blur_radius: px(24.),
                spread_radius: px(-5.),
                inset: false,
            },
            BoxShadow {
                color: hsla(0., 0., 0., near),
                offset: point(px(0.), px(2.)),
                blur_radius: px(6.),
                spread_radius: px(0.),
                inset: false,
            },
        ]
    }
}

/// A quiet empty state shared by workspace panes and settings.
pub fn pane_empty(icon: AnyElement, title: &str, detail: &str, s: &StudioPalette) -> Div {
    div()
        .w_full()
        .px(px(24.))
        .py(px(36.))
        .flex()
        .flex_col()
        .items_center()
        .text_center()
        .child(
            div()
                .size(px(40.))
                .rounded(px(R_CARD))
                .bg(s.soft)
                .flex()
                .items_center()
                .justify_center()
                .child(icon),
        )
        .child(
            div()
                .mt(px(12.))
                .text_size(s.font_size(FS_TITLE))
                .font_weight(FontWeight::MEDIUM)
                .text_color(s.ink)
                .child(title.to_owned()),
        )
        .child(
            div()
                .mt(px(6.))
                .max_w(px(320.))
                .text_size(s.font_size(FS_UI))
                .line_height(s.line_height(LH_UI))
                .text_color(s.muted)
                .child(detail.to_owned()),
        )
}

/// The meaning a small coloured mark carries.
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
            Self::Neutral => s.muted,
            Self::Success => s.pro.green_ink,
            Self::Info | Self::Accent => s.text,
            Self::Warning => s.pro.amber_ink,
            Self::Danger => s.pro.red_ink,
        }
    }
}

/// Tabular numerals (`tnum`) font features for metrics, counters, and percentages.
pub fn tnum_features() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

/// The UI font family shared across KUI, Kar Desktop, and Kantor chrome.
pub fn ui_font(cx: &App) -> SharedString {
    gpui_kit::component::Theme::global(cx).font_family.clone()
}

/// The monospace family for commands and code, honoring user overrides.
pub fn mono_font(cx: &App) -> SharedString {
    gpui_kit::component::Theme::global(cx).mono_font_family.clone()
}

pub fn render_approval_hand(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 68.155 68.155", HAND_APPROVAL_SVG, side, color)
}

/// KUI's signature glossy sky-blue folder artwork (`assistant_home.rs::tray_folder_icon`).
pub fn render_tray_folder_artwork(side: f32) -> AnyElement {
    static FOLDER_IMG: OnceLock<Arc<Image>> = OnceLock::new();
    const MARKUP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 67 67" width="48" height="48" fill="none"><path d="M11.17 15.35v15.36h44.67v-6.98c0-3.86-3.13-6.98-6.98-6.98H33.5l-3.93-5.89a5.58 5.58 0 0 0-4.64-2.49h-6.78c-3.86 0-6.98 3.13-6.98 6.98Z" fill="#00A1FF"></path><path d="M33.5 16.75l-3.93-5.89a5.58 5.58 0 0 0-4.64-2.49h-6.78c-3.86 0-6.98 3.13-6.98 6.98v15.36h44.67v-6.98c0-3.86-3.13-6.98-6.98-6.98H33.5Zm0 0h-9.77" stroke="#00A1FF" stroke-width="4.19" stroke-linecap="round" stroke-linejoin="round"></path><path d="M6.59 42.38c-1.05-5.26-1.57-7.89-.08-9.67l.31-.34c1.66-1.66 4.59-1.66 10.45-1.66h32.46c5.86 0 8.79 0 10.45 1.66l.31.34c1.49 1.78.97 4.41-.08 9.67-1.51 7.56-2.26 11.34-5.1 13.66l-.6.46c-3.01 2.13-7.23 2.13-15.66 2.13h-11.1c-8.43 0-12.64 0-15.66-2.13l-.6-.46c-2.84-2.32-3.6-6.1-5.1-13.66Z" fill="#3CC7FF" stroke="#3CC7FF" stroke-width="4.19"></path></svg>"##;
    let image = FOLDER_IMG
        .get_or_init(|| {
            Arc::new(Image::from_bytes(
                ImageFormat::Svg,
                MARKUP.as_bytes().to_vec(),
            ))
        })
        .clone();
    pro_artwork(image, side).into_any_element()
}

const ART_PDF_SVG: &str = r#"<path d="M3.3 1h3.9L10 3.8v6.4c0 .72-.58 1.3-1.3 1.3H3.3c-.72 0-1.3-.58-1.3-1.3V2.3C2 1.58 2.58 1 3.3 1Z" fill="currentColor" opacity="0.45"></path><path d="M7.2 1v1.8c0 .55.45 1 1 1H10Z" fill="currentColor"></path><path d="M4.1 8.8c1.2-1.8 2.6-3.1 3.8-3.1.5 0 .6.5.2 1-1 1.2-3.4 2.1-4 2.1Z" stroke="currentColor" stroke-width="0.95" stroke-linecap="round" stroke-linejoin="round"></path>"#;

const SVG_PDF_CHIP: &str = r#"<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"></path><path d="M14 2v6h6" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"></path><path d="M9 17c1.5-2.8 4.2-4.8 6-4.5.7.1.8.9.1 1.5-1.6 1.4-4.8 2.6-6.1 3Z" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"></path>"#;

/// Quartet of sculpted Kantor Office vector marks (Word, Excel, PPT, PDF) for the two-tier composer tray.
pub fn render_kantor_tray_marks(s: &StudioPalette) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(4.5))
        .child(pro_symbol("0 0 12 12", ART_DOC_SVG, 12.5, s.pro.blue))
        .child(pro_symbol("0 0 12 12", ART_SHEET_SVG, 12.5, s.pro.green))
        .child(pro_symbol("0 0 12 12", ART_SLIDE_SVG, 12.5, s.pro.orange))
        .child(pro_symbol("0 0 12 12", ART_PDF_SVG, 12.5, s.pro.red))
}

/// Sculpted KUI starter chip icon categories (`assistant_home.rs:147-169`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StarterChipIcon {
    Knowledge,
    Document,
    Table,
    Presentation,
    Research,
    Visualize,
}

impl StarterChipIcon {
    pub fn render(self, side: f32, color: Hsla) -> AnyElement {
        let svg = match self {
            Self::Knowledge => CHIP_KNOWLEDGE_SVG,
            Self::Document => CHIP_DOCUMENT_SVG,
            Self::Table => CHIP_TABLE_SVG,
            Self::Presentation => CHIP_PRESENTATION_SVG,
            Self::Research => CHIP_RESEARCH_SVG,
            Self::Visualize => CHIP_VISUALIZE_SVG,
        };
        pro_symbol("0 0 24 24", svg, side, color)
    }
}

pub fn render_folder_icon(side: f32, color: Hsla) -> AnyElement {
    pro_icon(ProIcon::Folder, side, color)
}

pub fn render_folder_plus_icon(side: f32, color: Hsla) -> AnyElement {
    pro_icon(ProIcon::FolderPlus, side, color)
}

pub fn render_folder_git_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_FOLDER_GIT2, side, color)
}

pub fn render_square_pen_icon(side: f32, color: Hsla) -> AnyElement {
    pro_icon(ProIcon::NewChat, side, color)
}

pub fn render_list_filter_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_LIST_FILTER, side, color)
}

pub fn render_paperclip_icon(side: f32, color: Hsla) -> AnyElement {
    pro_icon(ProIcon::Paperclip, side, color)
}

pub fn render_square_menu_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_SQUARE_MENU, side, color)
}

pub fn render_file_diff_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_FILE_DIFF, side, color)
}

pub fn render_workflows_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_WORKFLOWS_FORK, side, color)
}

pub fn render_chart_columns_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_CHART_COLUMNS, side, color)
}

pub fn render_arrow_up_icon(side: f32, color: Hsla) -> AnyElement {
    pro_icon(ProIcon::ArrowUp, side, color)
}

pub fn render_circle_check_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_CIRCLE_CHECK, side, color)
}

pub fn render_machine_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_MONITOR_MACHINE, side, color)
}

pub fn render_git_commit_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_GIT_COMMIT_CIRCLE, side, color)
}

pub fn render_radar_link_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_RADAR_LINK, side, color)
}

pub fn render_schedule_repeat_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_SCHEDULE_REPEAT, side, color)
}

pub fn render_brief_chart_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_BRIEF_CARDS, side, color)
}

pub fn render_steer_merge_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_STEER_MERGE, side, color)
}

pub fn render_bg_layers_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_BG_LAYERS, side, color)
}

pub fn render_git_branch_icon(side: f32, color: Hsla) -> AnyElement {
    pro_icon(ProIcon::GitBranch, side, color)
}

pub fn render_skills_puzzle_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_SKILLS_PUZZLE, side, color)
}

pub fn render_usage_pulse_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_USAGE_PULSE, side, color)
}

pub fn render_handoff_relay_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_HANDOFF_RELAY, side, color)
}

pub fn render_terminal_logs_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_TERMINAL_LOGS, side, color)
}

pub fn render_side_question_icon(side: f32, color: Hsla) -> AnyElement {
    pro_symbol("0 0 24 24", SVG_SIDE_QUESTION, side, color)
}

/// Line icon for a kind of document.
pub fn render_doc_outline_icon(kind: OfficeKind, side: f32, color: Hsla) -> AnyElement {
    let path = match kind {
        OfficeKind::Word => SVG_DOC_CHIP,
        OfficeKind::Excel => SVG_SHEET_CHIP,
        OfficeKind::Ppt => SVG_SLIDE_CHIP,
        OfficeKind::Pdf => SVG_PDF_CHIP,
    };
    pro_symbol("0 0 24 24", path, side, color)
}

/// A fixed paper grain and a warm KUI crest anchored to the top of the sidebar.
pub(crate) fn sidebar_texture(s: &StudioPalette, width: f32, running: bool, window: &Window) -> Div {
    use kui::components::gradient_floor::GradientFloor;

    static GRAIN: [OnceLock<Arc<RenderImage>>; 2] = [const { OnceLock::new() }; 2];
    let grain = GRAIN[s.dark as usize].get_or_init(|| {
        let noise = image::RgbaImage::from_fn(384, 1024, |x, y| {
            let n =
                (x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263)).wrapping_mul(2246822519);
            let shade = |v: u32| {
                let offset = (v & 7) as u8 / 2;
                if s.dark { 2 + offset } else { 247 + offset }
            };
            image::Rgba([shade(n >> 24), shade(n >> 16), shade(n >> 8), 255])
        });
        Arc::new(RenderImage::new([image::Frame::new(noise)]))
    });
    let stops = if s.dark {
        [0xb56a3b, 0xf07b4b, 0xa64553]
    } else {
        [0xffb967, 0xfe875a, 0xdc5b62]
    };
    // Bleed the luminous top and sides past the bounds to avoid texture atlas
    // fringe. The far edge is intrinsically transparent over the same paper grain.
    let bleed = 4.;
    let crest = GradientFloor::new("sidebar-brand-floor")
        .top_anchored()
        .over_backdrop()
        .with_height(px(96.))
        .with_radius(px(0.))
        .with_speed(5.4)
        .with_background(rgb(if s.dark { 0x040404 } else { 0xf8f8f8 }).into())
        .with_colors(stops.map(|color| rgb(color).into()))
        .paused(!running);
    // One clipped image keeps Metal's atlas filtering away from visible tile edges.
    div()
        .absolute()
        .inset_0()
        .overflow_hidden()
        .child(
            img(grain.clone())
                .opacity(if material::native_glass() { 0.14 } else { 0.50 })
                .absolute()
                .left(px(-2.))
                .top(px(-2.))
                .w(px(width.max(384.) + 4.))
                .h(px(f32::from(window.viewport_size().height).max(1024.) + 4.))
                .object_fit(ObjectFit::Fill),
        )
        .child(
            div()
                .absolute()
                .top(px(-bleed))
                .left(px(-bleed))
                .right(px(-bleed))
                .when(s.dark, |d| d.opacity(0.43))
                .child(crest),
        )
}

/// Kense emblem and desktop product wordmark.
pub fn render_kar_brand_lockup(s: &StudioPalette, cx: &App) -> Div {
    kar_brand_on(s.ink, cx)
}

pub(crate) fn kar_brand_on(ink: Hsla, cx: &App) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .when_some(pro_app_artwork("kense", 14., cx), |d, mark| d.child(mark))
        .child(
            div()
                .text_size(px(scaled_fs(FS_UI, cx)))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(ink)
                .child(crate::APP_NAME),
        )
}

fn decode_percent_svg_data_uri(data_uri: &str, dark_bg: bool) -> Option<Vec<u8>> {
    let raw = data_uri.strip_prefix("data:image/svg+xml,")?;
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex_val = |b: u8| -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    };
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let mut svg = String::from_utf8(out).ok()?;
    let fallback_hex = if dark_bg { "#f4f4f5" } else { "#18181b" };
    svg = svg
        .replace(r#"height="1em""#, r#"height="48""#)
        .replace(r#"width="1em""#, r#"width="48""#)
        .replace("currentColor", fallback_hex);
    Some(svg.into_bytes())
}

type BrandImageCache = LazyLock<Mutex<HashMap<(String, bool), Arc<Image>>>>;

/// Draw the same provider identity at every size, using GPUI's device-scale SVG cache.
pub fn render_runtime_mark(runtime_id: &str, side: f32, s: &StudioPalette, _cx: &App) -> AnyElement {
    static BRAND_CACHE: BrandImageCache = LazyLock::new(|| Mutex::new(HashMap::new()));

    let brand_id = runtime_brand_id(runtime_id);
    if let Some(data_uri) = runtime_brand_svg(brand_id, s.dark) {
        let mut images = BRAND_CACHE.lock().unwrap_or_else(|err| err.into_inner());
        let key = (brand_id.to_owned(), s.dark);
        let image = images.entry(key).or_insert_with(|| {
            let bytes = decode_percent_svg_data_uri(data_uri, s.dark).expect("bundled runtime SVG");
            Arc::new(Image::from_bytes(ImageFormat::Svg, bytes))
        });
        return pro_artwork(image.clone(), side).into_any_element();
    }
    pro_icon(ProIcon::Terminal, side, s.muted)
}

/// Subtle keyboard shortcut badge (`⌘K`, `ESC`, `↵`).
pub fn render_kbd(text: impl Into<SharedString>, s: &StudioPalette) -> Div {
    div()
        .flex_none()
        .h(px(20.))
        .px(px(6.))
        .rounded(px(R_XS))
        .bg(s.soft)
        .border_1()
        .border_color(s.line_soft)
        .flex()
        .items_center()
        .justify_center()
        .font_features(tnum_features())
        .text_size(s.font_size(FS_TAG))
        .text_color(s.muted)
        .whitespace_nowrap()
        .child(text.into())
}

/// A dot and a word for a state worth mentioning. No box around it.
pub fn render_status(label: impl Into<SharedString>, tone: DotTone, s: &StudioPalette) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .size(px(6.))
                .flex_none()
                .rounded_full()
                .bg(tone.dot_color(s)),
        )
        .child(
            div()
                .text_size(s.font_size(FS_META))
                .text_color(tone.text_color(s))
                .whitespace_nowrap()
                .child(label.into()),
        )
}

/// Marks built-in sample content, so it is never mistaken for the person's own.
pub fn render_sample_tag(s: &StudioPalette) -> Div {
    div()
        .flex_none()
        .h(px(20.))
        .px(px(6.))
        .rounded(px(R_XS))
        .bg(s.soft)
        .flex()
        .items_center()
        .text_size(s.font_size(FS_TAG))
        .text_color(s.muted)
        .whitespace_nowrap()
        .child("Demo")
}

/// Mini file-language pill (`[RS]`, `[TS]`, `[MD]`, `[W]`, `[X]`, `[P]`, `[PDF]`) matching T3 Code's `FileTagChip`.
pub fn render_file_ext_badge(ext_or_path: &str, s: &StudioPalette, cx: &App) -> Div {
    let ext = ext_or_path
        .rsplit('.')
        .next()
        .unwrap_or(ext_or_path)
        .to_ascii_uppercase();
    let (short, fg, bg) = match ext.as_str() {
        "RS" => ("RS", rgb(0xea580c).into(), hsla(21. / 360., 0.9, 0.48, 0.15)),
        "TS" | "TSX" => ("TS", rgb(0x3b82f6).into(), hsla(217. / 360., 0.9, 0.60, 0.16)),
        "JS" | "JSX" => ("JS", rgb(0xeab308).into(), hsla(48. / 360., 0.9, 0.47, 0.16)),
        "PY" => ("PY", rgb(0x0ea5e9).into(), hsla(199. / 360., 0.9, 0.48, 0.16)),
        "DOCX" | "DOC" => ("W", s.pro.accent_info, s.pro.accent_soft),
        "XLSX" | "XLS" => ("X", s.pro.green_ink, s.pro.green_soft),
        "PPTX" | "PPT" => ("P", s.pro.amber_ink, s.pro.amber_soft),
        "PDF" => ("PDF", s.pro.red_ink, s.pro.red_soft),
        "TOML" | "JSON" | "YAML" | "YML" => ("CFG", s.muted, s.soft),
        "MD" => ("MD", s.pro.accent_info, s.pro.accent_soft),
        _ => ("FILE", s.muted, s.soft),
    };
    div()
        .h(px(16.))
        .px(px(4.))
        .rounded(px(R_XS))
        .bg(bg)
        .flex()
        .items_center()
        .justify_center()
        .font_family(mono_font(cx))
        .text_size(px(9.0))
        .font_weight(FontWeight::BOLD)
        .text_color(fg)
        .child(short)
}

/// The W / X / P / F square that says what kind of document something is.
pub fn render_office_chip(kind: OfficeKind, s: &StudioPalette) -> Div {
    let (bg, fg, letter) = match kind {
        OfficeKind::Word => (s.pro.accent_soft, s.pro.accent_info, "W"),
        OfficeKind::Excel => (s.pro.green_soft, s.pro.green_ink, "X"),
        OfficeKind::Ppt => (s.pro.amber_soft, s.pro.amber_ink, "P"),
        OfficeKind::Pdf => (s.pro.red_soft, s.pro.red_ink, "F"),
    };
    div()
        .size(px(ICON))
        .flex_none()
        .rounded(px(R_XS))
        .bg(bg)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(FS_TAG))
        .font_weight(FontWeight::BOLD)
        .text_color(fg)
        .child(letter)
}

/// A document's name as people know it, without the markers used internally for samples.
pub fn clean_display_doc_name(raw_name: &str) -> String {
    let stripped = raw_name.strip_prefix("[演示] ").unwrap_or(raw_name);
    match stripped {
        "demo_coedit_schedule.docx" => "schedule.docx".into(),
        "demo_coedit_budget.xlsx" => "budget.xlsx".into(),
        "demo_coedit_launch.pptx" => "launch.pptx".into(),
        other => other.to_owned(),
    }
}

// ── KUI Application Screen Vector Artwork (`files.rs`, `agent_skills.rs`, `settings_models.rs`) ─

const SVG_SKILL_FEATHER: &str = r#"<path d="M5.07579 17C4.08939 4.54502 12.9123 1.0121 19.9734 2.22417C20.2585 6.35185 18.2389 7.89748 14.3926 8.61125C15.1353 9.38731 16.4477 10.3639 16.3061 11.5847C16.2054 12.4534 15.6154 12.8797 14.4355 13.7322C11.8497 15.6004 8.85421 16.7785 5.07579 17Z" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M4 22C4 15.5 7.84848 12.1818 10.5 10" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path>"#;

const SVG_SKILL_REPORT: &str = r#"<path d="M10.5 8H18.5M10.5 12H13M18.5 12H16M10.5 16H13M18.5 16H16" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M7 7.5H6C4.11 7.5 3.17 7.5 2.59 8.09C2 8.67 2 9.61 2 11.5V18C2 19.38 3.12 20.5 4.5 20.5C5.88 20.5 7 19.38 7 18V7.5Z" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M16 3.5H11C8.8 3.5 7 4.7 7 7.5V18C7 19.38 5.88 20.5 4.5 20.5H16C18.83 20.5 20.24 20.5 21.12 19.62C22 18.74 22 17.33 22 14.5V9.5C22 6.67 22 5.26 21.12 4.38C20.24 3.5 18.83 3.5 16 3.5Z" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path>"#;

const SVG_SKILL_GLOBE: &str = r#"<path d="M22 12C22 6.48 17.52 2 12 2C6.48 2 2 6.48 2 12C2 17.52 6.48 22 12 22" stroke="currentColor" stroke-linecap="round" stroke-width="1.9"></path><path d="M19.9 19.93L22 22M21.11 17.05C21.11 19.28 19.29 21.09 17.05 21.09C14.82 21.09 13 19.28 13 17.05C13 14.81 14.82 13 17.05 13C19.29 13 21.11 14.81 21.11 17.05Z" stroke="currentColor" stroke-linecap="round" stroke-width="1.9"></path><path d="M2.5 12h9M12 2.5c2.2 2.4 3.3 5.5 3.3 8.5" stroke="currentColor" stroke-linecap="round" stroke-width="1.8"></path>"#;

const SVG_SKILL_PR: &str = r#"<path d="M6 8V16" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M18 16V12C18 9.17 18 7.76 17.12 6.88C16.24 6 14.83 6 12 6H11M11 6L13.5 3.5M11 6L13.5 8.5" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><circle cx="6" cy="18" r="2" stroke="currentColor" stroke-width="1.9"></circle><circle cx="6" cy="6" r="2" stroke="currentColor" stroke-width="1.9"></circle><circle cx="18" cy="18" r="2" stroke="currentColor" stroke-width="1.9"></circle>"#;

const SVG_SKILL_CHART: &str = r#"<path d="M4 15V19M12 9V19M20 13V19M22 22H2" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><circle cx="4" cy="10" r="2" stroke="currentColor" stroke-width="1.9"></circle><circle cx="12" cy="4" r="2" stroke="currentColor" stroke-width="1.9"></circle><circle cx="20" cy="8" r="2" stroke="currentColor" stroke-width="1.9"></circle><path d="M5.6 8.8L10.4 5.2M13.8 4.9L18.2 7.1" stroke="currentColor" stroke-width="1.8"></path>"#;

pub fn render_grid_view_icon(side: f32, color: Hsla) -> AnyElement {
    pro_icon(ProIcon::Grid, side, color)
}

pub fn render_list_view_icon(side: f32, color: Hsla) -> AnyElement {
    pro_icon(ProIcon::List, side, color)
}

pub fn render_inbox_tray_icon(side: f32, color: Hsla) -> AnyElement {
    pro_icon(ProIcon::Inbox, side, color)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillCardGlyph {
    Feather,
    Report,
    Globe,
    PullRequest,
    Chart,
    Slides,
}

impl SkillCardGlyph {
    pub fn from_name(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        if lower.contains("git") || lower.contains("pr") || lower.contains("commit") || lower.contains("review") {
            Self::PullRequest
        } else if lower.contains("web") || lower.contains("search") || lower.contains("browser") || lower.contains("fetch") {
            Self::Globe
        } else if lower.contains("sheet") || lower.contains("excel") || lower.contains("data") || lower.contains("chart") || lower.contains("brief") {
            Self::Chart
        } else if lower.contains("slide") || lower.contains("ppt") || lower.contains("deck") {
            Self::Slides
        } else if lower.contains("doc") || lower.contains("word") || lower.contains("report") || lower.contains("plan") {
            Self::Report
        } else {
            Self::Feather
        }
    }

    pub fn render_tile(self, side: f32, accent: Hsla) -> Div {
        let svg = match self {
            Self::Feather => SVG_SKILL_FEATHER,
            Self::Report => SVG_SKILL_REPORT,
            Self::Globe => SVG_SKILL_GLOBE,
            Self::PullRequest => SVG_SKILL_PR,
            Self::Chart => SVG_SKILL_CHART,
            Self::Slides => SVG_SLIDE_CHIP,
        };
        div()
            .size(px(side))
            .flex_none()
            .rounded(px((side * 0.28).clamp(6., 9.)))
            .bg(accent.opacity(0.14))
            .border_1()
            .border_color(accent.opacity(0.24))
            .flex()
            .items_center()
            .justify_center()
            .child(pro_symbol("0 0 24 24", svg, (side * 0.52).round(), accent))
    }
}

/// A quiet frame for the same artwork shown in the sidebar and composer.
pub fn render_provider_color_tile(
    runtime_id: &str,
    side: f32,
    s: &StudioPalette,
    cx: &App,
) -> Div {
    let glyph_side = (side * 0.67).round();

    div()
        .size(px(side))
        .flex_none()
        .rounded(px((side * 0.27).clamp(6., 9.)))
        .bg(s.card)
        .border_1()
        .border_color(s.line)
        .flex()
        .items_center()
        .justify_center()
        .child(render_runtime_mark(runtime_id, glyph_side, s, cx))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MiniDocPreviewKind {
    Document,
    Spreadsheet,
    Presentation,
    Pdf,
    Code,
}

/// KUI `files.rs` miniature `52px` sculpted paper document preview with overhanging format badge.
pub fn render_mini_doc_artwork(
    badge: &'static str,
    tag_color: Hsla,
    kind: MiniDocPreviewKind,
    s: &StudioPalette,
) -> Div {
    let mut page = div()
        .w(px(52.))
        .pt(px(10.))
        .px(px(9.))
        .pb(px(20.))
        .rounded(px(6.))
        .bg(s.card)
        .border_1()
        .border_color(s.line_strong)
        .flex()
        .flex_col()
        .gap(px(6.));

    match kind {
        MiniDocPreviewKind::Spreadsheet => {
            let mut rows = div().flex().flex_col().gap(px(4.5));
            for r in 0..4usize {
                let cell_bg = if r == 0 {
                    s.ink.opacity(0.24)
                } else {
                    s.ink.opacity(0.11)
                };
                rows = rows.child(
                    div()
                        .flex()
                        .gap(px(3.))
                        .child(div().flex_1().h(px(2.)).rounded(px(1.)).bg(cell_bg))
                        .child(div().flex_1().h(px(2.)).rounded(px(1.)).bg(cell_bg))
                        .child(div().flex_1().h(px(2.)).rounded(px(1.)).bg(cell_bg)),
                );
            }
            page = page.child(rows);
        }
        MiniDocPreviewKind::Presentation => {
            page = page
                .child(
                    div()
                        .w_full()
                        .h(px(15.))
                        .rounded(px(2.5))
                        .bg(tag_color.opacity(0.15))
                        .px(px(5.))
                        .pb(px(3.))
                        .flex()
                        .items_end()
                        .gap(px(3.))
                        .child(div().w(px(5.)).h(px(6.)).rounded(px(1.)).bg(tag_color.opacity(0.65)))
                        .child(div().w(px(5.)).h(px(10.)).rounded(px(1.)).bg(tag_color))
                        .child(div().w(px(5.)).h(px(7.5)).rounded(px(1.)).bg(tag_color.opacity(0.75))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(div().w_full().h(px(2.)).rounded(px(1.)).bg(s.ink.opacity(0.14)))
                        .child(div().w(px(22.)).h(px(2.)).rounded(px(1.)).bg(s.ink.opacity(0.10))),
                );
        }
        MiniDocPreviewKind::Pdf => {
            page = page
                .child(
                    div()
                        .w_full()
                        .h(px(12.))
                        .rounded(px(2.5))
                        .bg(tag_color.opacity(0.16))
                        .flex()
                        .items_center()
                        .px(px(4.))
                        .child(div().w(px(16.)).h(px(2.5)).rounded(px(1.)).bg(tag_color)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(div().w_full().h(px(2.)).rounded(px(1.)).bg(s.ink.opacity(0.14)))
                        .child(div().w(px(24.)).h(px(2.)).rounded(px(1.)).bg(s.ink.opacity(0.11)))
                        .child(div().w(px(16.)).h(px(2.)).rounded(px(1.)).bg(tag_color.opacity(0.5))),
                );
        }
        MiniDocPreviewKind::Document | MiniDocPreviewKind::Code => {
            page = page.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.5))
                    .child(div().w(px(20.)).h(px(2.5)).rounded(px(1.)).bg(s.ink.opacity(0.22)))
                    .child(div().w_full().h(px(2.)).rounded(px(1.)).bg(s.ink.opacity(0.11)))
                    .child(div().w_full().h(px(2.)).rounded(px(1.)).bg(s.ink.opacity(0.11)))
                    .child(div().w(px(24.)).h(px(2.)).rounded(px(1.)).bg(s.ink.opacity(0.11)))
                    .child(div().w(px(17.)).h(px(2.)).rounded(px(1.)).bg(s.ink.opacity(0.11))),
            );
        }
    }

    div().relative().w(px(52.)).ml(px(8.)).child(page).child(
        div()
            .absolute()
            .left(px(-8.))
            .bottom(px(8.))
            .px(px(4.5))
            .py(px(2.))
            .rounded(px(3.5))
            .bg(tag_color)
            .text_size(px(8.5))
            .line_height(px(10.))
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(0xffffff))
            .child(badge),
    )
}

/// Common reading column for workspace collections.
pub(crate) fn workspace_page() -> Div {
    div()
        .w_full()
        .max_w(px(960.))
        .mx_auto()
        .p(px(28.))
        .flex()
        .flex_col()
        .gap(px(24.))
}

/// Unified container shell shared across all Settings tabs (`max_w = 880`, `p = 24`, `gap = 20`).
pub fn settings_page_shell() -> Div {
    div()
        .w_full()
        .max_w(px(880.))
        .mx_auto()
        .p(px(24.))
        .flex()
        .flex_col()
        .gap(px(20.))
}

/// Content cards share their finish across the conversation and settings.
pub fn card_box(s: &StudioPalette) -> Div {
    frosted_card(s)
}

/// Quiet nested container inside a `card_box` (`s.tray` + `1px s.line_soft` + `R_CTRL`).
pub fn sub_card_box(s: &StudioPalette) -> Div {
    div()
        .rounded(px(R_CTRL))
        .bg(s.tray)
        .border_1()
        .border_color(s.line_soft)
}

/// Section title above a `card_box` inside Settings or Workspace panels.
pub fn settings_section_title(label: impl Into<SharedString>, s: &StudioPalette) -> Div {
    div()
        .px(px(4.))
        .pb(px(8.))
        .text_color(s.ink)
        .text_size(s.font_size(FS_SECTION))
        .font_weight(FontWeight::SEMIBOLD)
        .child(label.into())
}

/// Hairline separator between rows inside a `card_box`.
pub fn settings_divider(s: &StudioPalette) -> Div {
    div().mx(px(16.)).h(px(1.)).bg(s.line_soft)
}

/// Standard 2-column (or responsive stacked) setting row inside a `card_box`.
pub fn settings_row_item(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    control: AnyElement,
    stacked: bool,
    s: &StudioPalette,
) -> Div {
    div()
        .min_h(px(64.))
        .px(px(16.))
        .py(px(12.))
        .flex()
        .map(|d| {
            if stacked {
                d.flex_col().items_start().gap(px(8.))
            } else {
                d.flex_wrap().items_center().justify_between().gap(px(12.))
            }
        })
        .child(
            div()
                .map(|d| {
                    if stacked {
                        d.w_full()
                    } else {
                        d.flex_1().flex_basis(px(180.)).min_w_0()
                    }
                })
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(
                    div()
                        .text_size(s.font_size(FS_UI))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.ink)
                        .child(title.into()),
                )
                .child(
                    div()
                        .text_size(s.font_size(FS_META))
                        .line_height(s.line_height(17.))
                        .text_color(s.muted)
                        .child(description.into()),
                ),
        )
        .child(div().max_w_full().flex_none().child(control))
}

/// Keep a labelled field and its action together at a readable width.
pub(crate) fn settings_input_action_row() -> Div {
    div()
        .w_full()
        .max_w(px(520.))
        .flex()
        .items_end()
        .gap(px(8.))
}

/// A single-line editor with an optional leading search icon.
pub fn search_input_box(
    id: impl Into<ElementId>,
    editor: Entity<kui::primitives::editor::Editor>,
    focused: bool,
    with_search_icon: bool,
    s: &StudioPalette,
) -> Stateful<Div> {
    let focus_editor = editor.clone();
    div()
        .id(id)
        .h(px(CTRL_H))
        .px(px(10.))
        .rounded(px(R_CTRL))
        .bg(s.card)
        .border_1()
        .border_color(if focused { s.line_strong } else { s.line })
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_text()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            let h = focus_editor.focus_handle(cx);
            window.focus(&h, cx);
        })
        .when(with_search_icon, |d| {
            d.child(pro_icon(ProIcon::Search, ICON_SM, s.muted))
        })
        .child(div().flex_1().min_w_0().h(px(20.)).child(editor))
}

pub(crate) fn section_heading(label: &'static str, count: Option<usize>, s: &StudioPalette) -> Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(8.))
        .pb(px(10.))
        .border_b_1()
        .border_color(s.line)
        .child(
            div()
                .text_size(s.font_size(FS_SECTION))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(s.ink)
                .child(label),
        )
        .when_some(count, |d, n| {
            d.child(
                div()
                    .text_size(s.font_size(FS_META))
                    .text_color(s.muted)
                    .child(n.to_string()),
            )
        })
}
