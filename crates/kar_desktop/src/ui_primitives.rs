//! Shared typography, surfaces and icons for the Kar workbench.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex, OnceLock},
};

use gpui::{prelude::*, *};
use kui::{
    components::pro_chrome::ProPalette,
    primitives::pro_icons::{pro_app_artwork, pro_svg},
};

use crate::types::{OfficeKind, runtime_brand_svg, runtime_display_name};

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
pub const MACOS_TRAFFIC_LIGHT_PAD: f32 = 76.0;
pub const SIDEBAR_W: f32 = 257.0;
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
        14.0
    } else {
        MACOS_TRAFFIC_LIGHT_PAD
    }
}


pub const FS_TAG: f32 = 11.0;
pub const FS_META: f32 = 12.0;
pub const FS_UI: f32 = 14.0;
pub const FS_READ: f32 = 15.0;
pub const FS_TITLE: f32 = 18.0;
pub const FS_HERO: f32 = 30.0;

pub const LH_META: f32 = 16.0;
pub const LH_UI: f32 = 20.0;
pub const LH_READ: f32 = 22.5;


pub const CTRL_H: f32 = 32.0;
pub const ROW_H: f32 = 36.0;
pub const ICON: f32 = 16.0;
pub const ICON_SM: f32 = 14.0;
pub const ICON_XS: f32 = 12.0;

pub const R_CTRL: f32 = 8.0;
pub const R_CARD: f32 = 8.0;
pub const R_HERO: f32 = 8.0;
pub const R_PILL: f32 = 999.0;

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

const SVG_TRAY_FOLDER: &str = r#"<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"></path>"#;

const SVG_FOLDER_PLUS: &str = r#"<path d="M12 10v6m-3-3h6" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"></path><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"></path>"#;

const SVG_FOLDER_GIT2: &str = r#"<path d="M9 20H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H20a2 2 0 0 1 2 2v5" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"></path><circle cx="13" cy="12" r="2" stroke="currentColor" stroke-width="1.75"></circle><path d="M18 19c-2.8 0-5-2.2-5-5v8" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"></path><circle cx="20" cy="19" r="2" stroke="currentColor" stroke-width="1.75"></circle>"#;

const SVG_SQUARE_PEN: &str = r#"<path d="M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"></path><path d="M18.375 2.625a1 1 0 0 1 3 3l-9.013 9.014a2 2 0 0 1-.853.505l-2.873.84a.5.5 0 0 1-.62-.62l.84-2.873a2 2 0 0 1 .506-.852z" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"></path>"#;

const SVG_LIST_FILTER: &str = r#"<path d="M3 6h18M7 12h10m-7 6h4" stroke="currentColor" stroke-width="1.85" stroke-linecap="round" stroke-linejoin="round"></path>"#;

const SVG_PAPERCLIP: &str = r#"<path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"></path>"#;

const SVG_PANEL_BOTTOM: &str = r#"<rect width="18" height="18" x="3" y="3" rx="2" stroke="currentColor" stroke-width="1.8"></rect><path d="M3 15h18" stroke="currentColor" stroke-width="1.8"></path>"#;

const SVG_PANEL_RIGHT: &str = r#"<rect width="18" height="18" x="3" y="3" rx="2" stroke="currentColor" stroke-width="1.8"></rect><path d="M15 3v18" stroke="currentColor" stroke-width="1.8"></path>"#;

const SVG_SQUARE_MENU: &str = r#"<rect width="18" height="18" x="3" y="3" rx="2" stroke="currentColor" stroke-width="1.8"></rect><path d="M7 8h10M7 12h10M7 16h10" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"></path>"#;

const SVG_FILE_DIFF: &str = r#"<rect width="18" height="18" x="3" y="3" rx="2" stroke="currentColor" stroke-width="1.8"></rect><path d="M12 7v6m-3-3h6m-6 6h6" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"></path>"#;

const SVG_WORKFLOWS_FORK: &str = r#"<circle cx="6" cy="6" r="2.2" stroke="currentColor" stroke-width="1.8"></circle><circle cx="18" cy="6" r="2.2" stroke="currentColor" stroke-width="1.8"></circle><circle cx="6" cy="18" r="2.2" stroke="currentColor" stroke-width="1.8"></circle><path d="M6 8.2v7.6M18 8.2c0 4.5-12 3.1-12 7.6" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"></path>"#;

const SVG_CHART_COLUMNS: &str = r#"<path d="M5 20v-6m7 6V4m7 16v-9" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"></path>"#;

const SVG_ARROW_UP: &str = r#"<path d="M12 19V5m-6 6 6-6 6 6" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"></path>"#;

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

const SVG_GIT_BRANCH: &str = r#"<line x1="6" x2="6" y1="3" y2="15" stroke="currentColor" stroke-width="1.85" stroke-linecap="round"></line><circle cx="18" cy="6" r="3" stroke="currentColor" stroke-width="1.85"></circle><circle cx="6" cy="18" r="3" stroke="currentColor" stroke-width="1.85"></circle><path d="M18 9a9 9 0 0 1-9 9" stroke="currentColor" stroke-width="1.85" stroke-linecap="round" stroke-linejoin="round"></path>"#;

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
        let dark = pro.dark;
        let tone = |light, night| Hsla::from(rgb(if dark { night } else { light }));
        let ink = tone(0x18181b, 0xf5f5f5);
        let card = tone(0xffffff, 0x0e0e0e);
        let muted = tone(0x71717a, 0xa3a3a3);
        let line = tone(0xe4e4e7, 0x141414);
        let primary = tone(0x1447e6, 0x4779e9);
        Self {
            pro,
            dark,
            stage: tone(0xfafafa, 0x000000),
            surface: tone(0xfdfdfd, 0x000000),
            tray: tone(0xf4f4f5, 0x111111),
            card,
            message_surface: tone(0xebebed, 0x212121),
            sidebar_selected: tone(0xffffff, 0x191919),
            sidebar_hover: tone(0xfdfdfd, 0x131313),
            soft: tone(0xf4f4f5, 0x171717),
            soft_hover: ink.opacity(0.09),
            hover: ink.opacity(0.055),
            active: ink.opacity(0.10),
            line,
            line_soft: tone(0xebebeb, 0x1a1a1a),
            line_strong: tone(0xd4d4d8, 0x323232),
            ink,
            text: tone(0x3f3f46, 0xd4d4d4),
            muted,
            faint: tone(0x75757e, 0x909090),
            primary,
            on_primary: white(),
            brand_action: primary,
            on_brand_action: white(),
        }
    }

    /// Popover & modal glass elevation shadow.
    pub fn popover_shadow(&self) -> Vec<BoxShadow> {
        let (far, near) = if self.dark { (0.68, 0.34) } else { (0.12, 0.045) };
        vec![
            BoxShadow {
                color: hsla(0., 0., 0., far),
                offset: point(px(0.), px(16.)),
                blur_radius: px(42.),
                spread_radius: px(-10.),
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

// ── T3 Code `ProjectMonogram` (`ProjectMonogram.tsx` + `projectIconColors.ts`) ─

/// Compact project identity shared by the breadcrumb, project picker and thread list.
pub fn project_monogram_letters(name: &str) -> String {
    let mut words = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty());
    let Some(first_word) = words.next() else {
        return "PR".into();
    };
    let mut chars = first_word.chars();
    let first = chars.next().unwrap();
    if matches!(first as u32, 0x3040..=0x30ff | 0x3400..=0x9fff | 0xac00..=0xd7af | 0x20000..=0x323af)
    {
        return first.to_string();
    }
    let second = chars
        .clone()
        .find(|c| c.is_numeric())
        .or_else(|| words.next_back().and_then(|w| w.chars().next()))
        .or_else(|| chars.next_back())
        .unwrap_or(first);
    first
        .to_uppercase()
        .chain(second.to_uppercase())
        .take(2)
        .collect()
}

pub fn project_monogram_colors(name: &str, dark: bool) -> (Hsla, Hsla) {
    const PALETTE: &[(u32, u32)] = &[
        (0x4a5565, 0x99a1af), // gray
        (0xe7000b, 0xff6467), // red
        (0xf54900, 0xff8904), // orange
        (0xe17100, 0xffb900), // amber
        (0xd08700, 0xfdc700), // yellow
        (0x5ea500, 0x9ae600), // lime
        (0x00a63e, 0x05df72), // green
        (0x009966, 0x00d492), // emerald
        (0x009689, 0x00d5be), // teal
        (0x0092b8, 0x00d3f2), // cyan
        (0x0084d1, 0x00bcff), // sky
        (0x155dfc, 0x51a2ff), // blue
        (0x4f39f6, 0x7c86ff), // indigo
        (0x7f22fe, 0xa684ff), // violet
        (0x9810fa, 0xc27aff), // purple
        (0xc800de, 0xed6aff), // fuchsia
        (0xe60076, 0xfb64b6), // pink
        (0xec003f, 0xff637e), // rose
    ];
    let name = name.trim();
    let key = if name.is_empty() { "project" } else { name };
    let index = key
        .chars()
        .flat_map(char::to_lowercase)
        .fold(0, |acc, c| (acc * 31 + c as usize) % PALETTE.len());
    let (light, night) = PALETTE[index];
    let ink: Hsla = rgb(if dark { night } else { light }).into();
    (ink, ink.opacity(0.14))
}

/// Render T3 Code's signature `16x16` (`rounded-[25%]`) two-letter `ProjectMonogram` tile.
pub fn render_project_monogram(
    project_name: &str,
    side: f32,
    s: &StudioPalette,
    cx: &App,
) -> Div {
    let letters = project_monogram_letters(project_name);
    let (fg, bg) = project_monogram_colors(project_name, s.dark);
    div()
        .size(px(side))
        .flex_none()
        .rounded(px((side * 0.25).max(3.5)))
        .bg(bg)
        .flex()
        .items_center()
        .justify_center()
        .font_family(mono_font(cx))
        .text_size(px(side * 8.25 / 16.))
        .font_weight(FontWeight::BOLD)
        .text_color(fg)
        .child(letters)
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
                .text_size(px(FS_TITLE))
                .font_weight(FontWeight::MEDIUM)
                .text_color(s.ink)
                .child(title.to_owned()),
        )
        .child(
            div()
                .mt(px(6.))
                .max_w(px(320.))
                .text_size(px(FS_UI))
                .line_height(px(LH_UI))
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

/// The monospace family for commands and code.
pub fn mono_font(cx: &App) -> SharedString {
    gpui_kit::component::Theme::global(cx).mono_font_family.clone()
}

pub fn render_approval_hand(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 68.155 68.155", HAND_APPROVAL_SVG, side, color)
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
    img(image).size(px(side)).flex_none().into_any_element()
}

/// Trio of sculpted Kantor Office vector marks (Word, Excel, PPT) for the two-tier composer tray.
pub fn render_kantor_tray_marks(s: &StudioPalette) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(4.5))
        .child(pro_svg("0 0 12 12", ART_DOC_SVG, 12.5, s.pro.blue))
        .child(pro_svg("0 0 12 12", ART_SHEET_SVG, 12.5, s.pro.green))
        .child(pro_svg("0 0 12 12", ART_SLIDE_SVG, 12.5, s.pro.orange))
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
        pro_svg("0 0 24 24", svg, side, color)
    }
}

pub fn render_folder_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_TRAY_FOLDER, side, color)
}

pub fn render_folder_plus_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_FOLDER_PLUS, side, color)
}

pub fn render_folder_git_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_FOLDER_GIT2, side, color)
}

pub fn render_square_pen_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_SQUARE_PEN, side, color)
}

pub fn render_list_filter_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_LIST_FILTER, side, color)
}

pub fn render_paperclip_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_PAPERCLIP, side, color)
}

pub fn render_panel_bottom_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_PANEL_BOTTOM, side, color)
}

pub fn render_panel_right_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_PANEL_RIGHT, side, color)
}

pub fn render_square_menu_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_SQUARE_MENU, side, color)
}

pub fn render_file_diff_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_FILE_DIFF, side, color)
}

pub fn render_workflows_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_WORKFLOWS_FORK, side, color)
}

pub fn render_chart_columns_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_CHART_COLUMNS, side, color)
}

pub fn render_arrow_up_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_ARROW_UP, side, color)
}

pub fn render_circle_check_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_CIRCLE_CHECK, side, color)
}

pub fn render_machine_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_MONITOR_MACHINE, side, color)
}

pub fn render_git_commit_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_GIT_COMMIT_CIRCLE, side, color)
}

pub fn render_radar_link_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_RADAR_LINK, side, color)
}

pub fn render_schedule_repeat_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_SCHEDULE_REPEAT, side, color)
}

pub fn render_brief_chart_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_BRIEF_CARDS, side, color)
}

pub fn render_steer_merge_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_STEER_MERGE, side, color)
}

pub fn render_bg_layers_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_BG_LAYERS, side, color)
}

pub fn render_git_branch_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_GIT_BRANCH, side, color)
}

pub fn render_skills_puzzle_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_SKILLS_PUZZLE, side, color)
}

pub fn render_usage_pulse_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_USAGE_PULSE, side, color)
}

pub fn render_handoff_relay_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_HANDOFF_RELAY, side, color)
}

pub fn render_terminal_logs_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_TERMINAL_LOGS, side, color)
}

pub fn render_side_question_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_SIDE_QUESTION, side, color)
}

/// Line icon for a kind of document.
pub fn render_doc_outline_icon(kind: OfficeKind, side: f32, color: Hsla) -> AnyElement {
    let path = match kind {
        OfficeKind::Word => SVG_DOC_CHIP,
        OfficeKind::Excel => SVG_SHEET_CHIP,
        OfficeKind::Ppt => SVG_SLIDE_CHIP,
    };
    pro_svg("0 0 24 24", path, side, color)
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
    let background = if s.dark { 0x040404 } else { 0xf8f8f8 };
    let height = 78.;
    // Keep the atlas sampling fringe outside the visible strip on Retina displays.
    let bleed = 4.;
    let crest = GradientFloor::new("sidebar-brand-floor")
        .top_anchored()
        .with_height(px(height + 2. * bleed))
        .with_radius(px(0.))
        .with_speed(5.4)
        .with_background(rgb(background).into())
        .with_colors(stops.map(|color| rgb(color).into()))
        .paused(!running);
    // One clipped image keeps Metal's atlas filtering away from visible tile edges.
    div()
        .absolute()
        .inset_0()
        .overflow_hidden()
        .child(
            img(grain.clone())
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
                .top_0()
                .left_0()
                .right_0()
                .h(px(height))
                .overflow_hidden()
                .child(
                    div()
                        .absolute()
                        .top(px(-bleed))
                        .left(px(-bleed))
                        .right(px(-bleed))
                        .when(s.dark, |d| d.opacity(0.43))
                        .child(crest),
                ),
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
                .text_size(px(14.))
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

/// Render an agent's official vector brand mark from `kar::brands::runtime_brands()` (Retina SVG
/// cached per theme), falling back to KUI `pro_app_artwork` or a clean monogram tile.
pub fn render_runtime_mark(runtime_id: &str, side: f32, s: &StudioPalette, cx: &App) -> AnyElement {
    static BRAND_CACHE: BrandImageCache = LazyLock::new(|| Mutex::new(HashMap::new()));

    // Prefer KUI's crisp Claude starburst vector over CLI pixel-mascot for `"claude"`.
    if runtime_id == "claude"
        && let Some(el) = pro_app_artwork("claude", side, cx)
    {
        return el;
    }

    if let Some(data_uri) = runtime_brand_svg(runtime_id, s.dark) {
        let cached = BRAND_CACHE
            .lock()
            .ok()
            .and_then(|mut map| {
                let key = (runtime_id.to_owned(), s.dark);
                if let Some(existing) = map.get(&key) {
                    return Some(existing.clone());
                }
                let svg_bytes = decode_percent_svg_data_uri(data_uri, s.dark)?;
                let image = Arc::new(Image::from_bytes(ImageFormat::Svg, svg_bytes));
                map.insert(key, image.clone());
                Some(image)
            });
        if let Some(image) = cached {
            return img(image).size(px(side)).flex_none().into_any_element();
        }
    }

    let brand_key = match runtime_id {
        "codex" => Some("openai"),
        "claude" => Some("claude"),
        "grok" => Some("grok"),
        "qwen" => Some("qwen"),
        "antigravity" => Some("gemini"),
        _ => None,
    };
    if let Some(key) = brand_key
        && let Some(el) = pro_app_artwork(key, side, cx)
    {
        return el;
    }

    let initial = runtime_display_name(runtime_id)
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    div()
        .size(px(side))
        .flex_none()
        .rounded(px((side * 0.28).round()))
        .bg(s.active)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px((side * 0.60).round()))
        .font_weight(FontWeight::MEDIUM)
        .text_color(s.ink)
        .child(initial)
        .into_any_element()
}

/// Subtle keyboard shortcut badge (`⌘K`, `ESC`, `↵`).
pub fn render_kbd(text: impl Into<SharedString>, s: &StudioPalette) -> Div {
    div()
        .flex_none()
        .h(px(19.))
        .px(px(5.5))
        .rounded(px(4.5))
        .bg(s.soft)
        .border_1()
        .border_color(s.line_soft)
        .flex()
        .items_center()
        .justify_center()
        .font_features(tnum_features())
        .text_size(px(10.5))
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
                .text_size(px(FS_META))
                .text_color(tone.text_color(s))
                .whitespace_nowrap()
                .child(label.into()),
        )
}

/// Marks built-in sample content, so it is never mistaken for the person's own.
pub fn render_sample_tag(s: &StudioPalette) -> Div {
    div()
        .flex_none()
        .h(px(18.))
        .px(px(6.))
        .rounded(px(4.5))
        .bg(s.soft)
        .flex()
        .items_center()
        .text_size(px(10.5))
        .text_color(s.muted)
        .whitespace_nowrap()
        .child("Demo")
}

/// Mini file-language pill (`[RS]`, `[TS]`, `[MD]`, `[W]`, `[X]`, `[P]`) matching T3 Code's `FileTagChip`.
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
        "TOML" | "JSON" | "YAML" | "YML" => ("CFG", s.muted, s.soft),
        "MD" => ("MD", s.pro.accent_info, s.pro.accent_soft),
        _ => ("FILE", s.muted, s.soft),
    };
    div()
        .h(px(15.))
        .px(px(3.5))
        .rounded(px(3.5))
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

/// The W / X / P square that says what kind of document something is.
pub fn render_office_chip(kind: OfficeKind, s: &StudioPalette) -> Div {
    let (bg, fg, letter) = match kind {
        OfficeKind::Word => (s.pro.accent_soft, s.pro.accent_info, "W"),
        OfficeKind::Excel => (s.pro.green_soft, s.pro.green_ink, "X"),
        OfficeKind::Ppt => (s.pro.amber_soft, s.pro.amber_ink, "P"),
    };
    div()
        .size(px(ICON))
        .flex_none()
        .rounded(px(4.))
        .bg(bg)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(10.))
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

const SVG_VIEW_GRID: &str = r##"<path d="M3.89 9.66C4.39 10 5.1 10 6.5 10C7.9 10 8.61 10 9.11 9.66C9.52 9.33 10 8.61 10 6.5C10 4.39 9.52 3.67 9.11 3.34C8.61 3 7.9 3 6.5 3C5.1 3 4.39 3 3.89 3.34C3.48 3.67 3 4.39 3 6.5C3 8.61 3.48 9.33 3.89 9.66Z" stroke="currentColor" stroke-linejoin="round" stroke-width="1.9"></path><path d="M14.89 9.66C15.39 10 16.1 10 17.5 10C18.9 10 19.61 10 20.11 9.66C20.52 9.33 21 8.61 21 6.5C21 4.39 20.52 3.67 20.11 3.34C19.61 3 18.9 3 17.5 3C16.1 3 15.39 3 14.89 3.34C14.48 3.67 14 4.39 14 6.5C14 8.61 14.48 9.33 14.89 9.66Z" stroke="currentColor" stroke-linejoin="round" stroke-width="1.9"></path><path d="M3.89 20.66C4.39 21 5.1 21 6.5 21C7.9 21 8.61 21 9.11 20.66C9.52 20.33 10 19.61 10 17.5C10 15.39 9.52 14.67 9.11 14.34C8.61 14 7.9 14 6.5 14C5.1 14 4.39 14 3.89 14.34C3.48 14.67 3 15.39 3 17.5C3 19.61 3.48 20.33 3.89 20.66Z" stroke="currentColor" stroke-linejoin="round" stroke-width="1.9"></path><path d="M14.89 20.66C15.39 21 16.1 21 17.5 21C18.9 21 19.61 21 20.11 20.66C20.52 20.33 21 19.61 21 17.5C21 15.39 20.52 14.67 20.11 14.34C19.61 14 18.9 14 17.5 14C16.1 14 15.39 14 14.89 14.34C14.48 14.67 14 15.39 14 17.5C14 19.61 14.48 20.33 14.89 20.66Z" stroke="currentColor" stroke-linejoin="round" stroke-width="1.9"></path>"##;

const SVG_VIEW_LIST: &str = r##"<path d="M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"></path>"##;

const SVG_SKILL_FEATHER: &str = r#"<path d="M5.07579 17C4.08939 4.54502 12.9123 1.0121 19.9734 2.22417C20.2585 6.35185 18.2389 7.89748 14.3926 8.61125C15.1353 9.38731 16.4477 10.3639 16.3061 11.5847C16.2054 12.4534 15.6154 12.8797 14.4355 13.7322C11.8497 15.6004 8.85421 16.7785 5.07579 17Z" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M4 22C4 15.5 7.84848 12.1818 10.5 10" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path>"#;

const SVG_SKILL_REPORT: &str = r#"<path d="M10.5 8H18.5M10.5 12H13M18.5 12H16M10.5 16H13M18.5 16H16" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M7 7.5H6C4.11 7.5 3.17 7.5 2.59 8.09C2 8.67 2 9.61 2 11.5V18C2 19.38 3.12 20.5 4.5 20.5C5.88 20.5 7 19.38 7 18V7.5Z" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M16 3.5H11C8.8 3.5 7 4.7 7 7.5V18C7 19.38 5.88 20.5 4.5 20.5H16C18.83 20.5 20.24 20.5 21.12 19.62C22 18.74 22 17.33 22 14.5V9.5C22 6.67 22 5.26 21.12 4.38C20.24 3.5 18.83 3.5 16 3.5Z" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path>"#;

const SVG_SKILL_GLOBE: &str = r#"<path d="M22 12C22 6.48 17.52 2 12 2C6.48 2 2 6.48 2 12C2 17.52 6.48 22 12 22" stroke="currentColor" stroke-linecap="round" stroke-width="1.9"></path><path d="M19.9 19.93L22 22M21.11 17.05C21.11 19.28 19.29 21.09 17.05 21.09C14.82 21.09 13 19.28 13 17.05C13 14.81 14.82 13 17.05 13C19.29 13 21.11 14.81 21.11 17.05Z" stroke="currentColor" stroke-linecap="round" stroke-width="1.9"></path><path d="M2.5 12h9M12 2.5c2.2 2.4 3.3 5.5 3.3 8.5" stroke="currentColor" stroke-linecap="round" stroke-width="1.8"></path>"#;

const SVG_SKILL_PR: &str = r#"<path d="M6 8V16" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><path d="M18 16V12C18 9.17 18 7.76 17.12 6.88C16.24 6 14.83 6 12 6H11M11 6L13.5 3.5M11 6L13.5 8.5" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><circle cx="6" cy="18" r="2" stroke="currentColor" stroke-width="1.9"></circle><circle cx="6" cy="6" r="2" stroke="currentColor" stroke-width="1.9"></circle><circle cx="18" cy="18" r="2" stroke="currentColor" stroke-width="1.9"></circle>"#;

const SVG_SKILL_CHART: &str = r#"<path d="M4 15V19M12 9V19M20 13V19M22 22H2" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"></path><circle cx="4" cy="10" r="2" stroke="currentColor" stroke-width="1.9"></circle><circle cx="12" cy="4" r="2" stroke="currentColor" stroke-width="1.9"></circle><circle cx="20" cy="8" r="2" stroke="currentColor" stroke-width="1.9"></circle><path d="M5.6 8.8L10.4 5.2M13.8 4.9L18.2 7.1" stroke="currentColor" stroke-width="1.8"></path>"#;

const SVG_INBOX_TRAY: &str = r#"<path d="M3 13h4.2a1.5 1.5 0 0 1 1.3.75l.9 1.5a1.5 1.5 0 0 0 1.3.75h2.6a1.5 1.5 0 0 0 1.3-.75l.9-1.5A1.5 1.5 0 0 1 16.8 13H21" stroke="currentColor" stroke-width="1.85" stroke-linecap="round" stroke-linejoin="round"></path><path d="M5.4 4.8 3.2 12.2A4 4 0 0 0 3 13.4V17a3 3 0 0 0 3 3h12a3 3 0 0 0 3-3v-3.6a4 4 0 0 0-.2-1.2L18.6 4.8A2.5 2.5 0 0 0 16.2 3H7.8a2.5 2.5 0 0 0-2.4 1.8Z" stroke="currentColor" stroke-width="1.85" stroke-linecap="round" stroke-linejoin="round"></path>"#;

pub fn render_grid_view_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_VIEW_GRID, side, color)
}

pub fn render_list_view_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_VIEW_LIST, side, color)
}

pub fn render_inbox_tray_icon(side: f32, color: Hsla) -> AnyElement {
    pro_svg("0 0 24 24", SVG_INBOX_TRAY, side, color)
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
            .child(pro_svg("0 0 24 24", svg, (side * 0.52).round(), accent))
    }
}

/// KUI `settings_models.rs` colorful provider logo tile (`Anthropic` coral, `Gemini` gradient, `Mistral` pixel, + Kar SVGs).
pub fn render_provider_color_tile(
    runtime_id: &str,
    side: f32,
    s: &StudioPalette,
    cx: &App,
) -> Div {
    static SPECIAL: [OnceLock<Arc<Image>>; 3] = [const { OnceLock::new() }; 3];
    let lower = runtime_id.to_ascii_lowercase();
    let special = if lower.contains("claude") || lower.contains("anthropic") {
        Some((
            0,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48" viewBox="0 0 24 24"><path fill="#D97757" d="m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z"/></svg>"##,
        ))
    } else if lower.contains("gemini") || lower.contains("antigravity") || lower.contains("google") {
        Some((
            1,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48" viewBox="0 0 24 24"><defs><linearGradient id="g" x1="0" y1="24" x2="24" y2="0" gradientUnits="userSpaceOnUse"><stop stop-color="#1C7DFF"/><stop offset="0.52" stop-color="#1C69FF"/><stop offset="1" stop-color="#B04DFF"/></linearGradient></defs><path fill="url(#g)" d="M11.04 19.32Q12 21.51 12 24q0-2.49.93-4.68.96-2.19 2.58-3.81t3.81-2.55Q21.51 12 24 12q-2.49 0-4.68-.93a12.3 12.3 0 0 1-3.81-2.58 12.3 12.3 0 0 1-2.58-3.81Q12 2.49 12 0q0 2.49-.96 4.68-.93 2.19-2.55 3.81a12.3 12.3 0 0 1-3.81 2.58Q2.49 12 0 12q2.49 0 4.68.96 2.19.93 3.81 2.55t2.55 3.81"/></svg>"##,
        ))
    } else if lower.contains("mistral") || lower.contains("vibe") {
        Some((
            2,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48" viewBox="-2 -2 29 29"><rect x="0" y="0" width="5" height="5" fill="#FFD800"/><rect x="20" y="0" width="5" height="5" fill="#FFD800"/><rect x="0" y="5" width="5" height="5" fill="#FFAF00"/><rect x="5" y="5" width="5" height="5" fill="#FFAF00"/><rect x="15" y="5" width="5" height="5" fill="#FFAF00"/><rect x="20" y="5" width="5" height="5" fill="#FFAF00"/><rect x="0" y="10" width="5" height="5" fill="#FF8205"/><rect x="5" y="10" width="5" height="5" fill="#FF8205"/><rect x="10" y="10" width="5" height="5" fill="#FF8205"/><rect x="15" y="10" width="5" height="5" fill="#FF8205"/><rect x="20" y="10" width="5" height="5" fill="#FF8205"/><rect x="0" y="15" width="5" height="5" fill="#FA500F"/><rect x="10" y="15" width="5" height="5" fill="#FA500F"/><rect x="20" y="15" width="5" height="5" fill="#FA500F"/><rect x="0" y="20" width="5" height="5" fill="#E10500"/><rect x="20" y="20" width="5" height="5" fill="#E10500"/></svg>"##,
        ))
    } else {
        None
    };

    let glyph_side = (side * 0.54).round().max(14.);
    let inner: AnyElement = if let Some((slot, markup)) = special {
        let image = SPECIAL[slot]
            .get_or_init(|| {
                Arc::new(Image::from_bytes(
                    ImageFormat::Svg,
                    markup.as_bytes().to_vec(),
                ))
            })
            .clone();
        img(image).size(px(glyph_side)).flex_none().into_any_element()
    } else {
        render_runtime_mark(runtime_id, glyph_side, s, cx)
    };

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
        .child(inner)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MiniDocPreviewKind {
    Document,
    Spreadsheet,
    Presentation,
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
        .p(px(32.))
        .flex()
        .flex_col()
        .gap(px(28.))
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
                .text_size(px(FS_UI))
                .font_weight(FontWeight::MEDIUM)
                .text_color(s.ink)
                .child(label),
        )
        .when_some(count, |d, n| {
            d.child(
                div()
                    .text_size(px(FS_META))
                    .text_color(s.muted)
                    .child(n.to_string()),
            )
        })
}
