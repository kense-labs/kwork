//! Pure-Rust GPUI desktop workbench combining KUI components, Kense architectural monochrome,
//! Kar Rust SDK (`ChatStore`/`ChatSession`/`HttpChatFeed`/`ChatReplica`/`serve_mcp_tools`),
//! and Kantor Office editors.

#![forbid(unsafe_code)]

extern crate gpui_kit as gpui;

mod aster;
pub mod brief;
pub mod client_store;
pub mod coedit;
mod engine;
pub mod hardware;
pub mod forge_devices_voice;
pub mod kar_bridge;
mod kremote;
pub mod mcp_office;
mod onboarding_audio;
pub mod office_host;
pub mod prefs;
pub mod pty_terminal;
pub mod qr;
pub mod registry;
pub mod remote_link;
pub mod schedules;
pub mod studio_hub;
pub mod types;
pub mod turn_review;
pub mod ui_agents;
pub mod ui_brief;
pub mod ui_chat;
pub mod ui_files_gallery;
pub mod ui_git;
pub mod ui_handoff_logs;
pub mod ui_inbox_tasks;
pub mod ui_motion;
pub mod ui_office;
mod ui_onboarding;
mod ui_onboarding_film;
mod ui_onboarding_providers;
pub mod ui_primitives;
pub mod ui_remote;
pub mod ui_schedules;
pub mod ui_settings;
pub mod ui_settings_panels;
pub mod ui_sidebar;
pub mod ui_skills_mcp;
pub mod ui_terminal;
pub mod ui_usage;
pub mod workbench;

use std::path::PathBuf;

use docs_ui::DocsView;
use gpui::{prelude::*, *};
use kui::{
    ThemeMode,
    components::{
        ai_spark::{AiSpark, SparkStatus, SparkTone},
        badge::{Badge, BadgeTone},
        copy_button::CopyButton,
        input::Input,
        resizable_panels::{Pane, ResizablePanels},
        theme_switch::{ThemeChange, ThemeSwitch},
    },
    foundation::AccentPalette,
    primitives::control::UiLanguage,
};

use crate::ui_primitives::{
    FS_META, FS_READ, FS_UI, LH_UI, R_CARD, StudioPalette, macos_traffic_pad,
    render_kar_brand_lockup, render_office_chip, titlebar_row, ui_font,
};

pub const APP_NAME: &str = "Kwrok";

struct SystemCodeFont(SharedString);
impl Global for SystemCodeFont {}

/// Initialize shared GPUI Kit, Kantor Office editors, and KUI foundation globals.
pub fn init_runtime_ui(cx: &mut App) {
    kantor_ui::set_embedded_host(true);
    kantor_ui::i18n::set_lang(kantor_ui::i18n::Lang::Zh);
    gpui_kit::init(cx);
    kantor_ui::widget_strings::init();
    kantor_ui::find_bar::init(cx);
    sheets_ui::init(cx);
    docs_ui::init(cx);
    slides_ui::init(cx);
    pdf_ui::init(cx);
    kui::init(cx);
    cx.set_global(kui::primitives::field::SingleLineFieldMetrics {
        height: ui_primitives::CTRL_H,
        radius: ui_primitives::R_CTRL,
        text_size: ui_primitives::FS_CONTROL,
        label_size: ui_primitives::FS_META,
    });
    ui_primitives::material::init_window_glass(cx);
    cx.set_global(ui_primitives::ActiveNamedTheme::default());
    cx.set_global(ui_primitives::ActiveTypography::default());
    ui_chat::init_project_picker_keys(cx);
    workbench::init_palette_keys(cx);
    #[cfg(target_os = "macos")]
    cx.set_reduce_motion(
        objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion(),
    );
    cx.set_global(UiLanguage::Chinese);
    apply_unified_theme(system_theme(cx.window_appearance()), cx);
}

pub(crate) fn system_theme(appearance: WindowAppearance) -> ThemeMode {
    match appearance {
        WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
        WindowAppearance::Light | WindowAppearance::VibrantLight => ThemeMode::Light,
    }
}

fn apply_ui_language(locale: ui_primitives::UiLocale, cx: &mut App) {
    let (kit, office) = match locale {
        ui_primitives::UiLocale::En => (UiLanguage::English, kantor_ui::i18n::Lang::En),
        ui_primitives::UiLocale::Zh => (UiLanguage::Chinese, kantor_ui::i18n::Lang::Zh),
    };
    cx.set_global(kit);
    kantor_ui::i18n::set_lang(office);
    cx.refresh_windows();
}

/// Apply signature Kwrok monochrome OR any named `kantor_theme` palette + typography across KUI & Kantor.
pub fn apply_unified_theme(mode: ThemeMode, cx: &mut App) {
    let named = cx
        .try_global::<ui_primitives::ActiveNamedTheme>()
        .and_then(|g| g.0.clone());
    apply_palette_and_mode(mode, named, cx);
}

pub fn apply_theme_and_typography(
    fallback_mode: ThemeMode,
    prefs: &prefs::DesktopPreferences,
    cx: &mut App,
) {
    cx.set_global(ui_primitives::ActiveTypography {
        ui_delta_px: prefs.ui_font_scale.delta_px(),
        code_font_size: prefs.code_font_size.clamp(11.0, 18.0),
        code_font_family: prefs
            .code_font_family
            .as_deref()
            .and_then(|family| installed_code_font(family, cx)),
    });
    let resolved_palette = prefs
        .named_theme_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|id| {
            let tid = kantor_theme::catalog::ThemeId::parse(id);
            kantor_theme::catalog::resolve(&tid)
        });
    let effective_mode = match &resolved_palette {
        Some(p) => {
            if p.dark {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            }
        }
        None => fallback_mode,
    };
    cx.set_global(ui_primitives::ActiveNamedTheme(resolved_palette.clone()));
    apply_palette_and_mode(effective_mode, resolved_palette, cx);
}

fn installed_code_font(family: &str, cx: &App) -> Option<SharedString> {
    kantor_ui::fonts::installed(cx)
        .iter()
        .find(|name| name.eq_ignore_ascii_case(family.trim()))
        .map(|name| SharedString::from(name.clone()))
}

fn apply_palette_and_mode(
    mode: ThemeMode,
    named_palette: Option<kantor_theme::Palette>,
    cx: &mut App,
) {
    {
        let theme = kui::Theme::global_mut(cx);
        theme.set_mode(mode);
        theme.set_accent(AccentPalette::default());
    }

    cx.set_global(ui_primitives::ActiveNamedTheme(named_palette.clone()));
    let shell = ui_primitives::StudioPalette::get(cx);
    {
        let theme = kui::Theme::global_mut(cx);
        let colors = &mut theme.tokens.colors;
        colors.background = shell.surface;
        colors.surface = shell.card;
        colors.surface_muted = shell.soft;
        colors.foreground = shell.ink;
        colors.text_secondary = shell.text;
        colors.text_muted = shell.muted;
        colors.border = shell.line;
        colors.border_strong = shell.line_strong;
        colors.accent = shell.brand_action;
        colors.accent_foreground = shell.on_brand_action;
        colors.accent_subtle = shell.brand_action.opacity(0.15);
        colors.focus_outline = shell.brand_action;
    }

    let palette = named_palette.unwrap_or_else(|| {
        let rgb24 = |color: Hsla| u32::from(color.to_rgb()) >> 8;
        let colors = &kui::Theme::global(cx).tokens.colors;
        kantor_theme::Palette {
            dark: shell.dark,
            background: rgb24(shell.surface),
            dark_background: rgb24(shell.tray),
            lighter_background: rgb24(shell.card),
            foreground: rgb24(shell.ink),
            dark_foreground: rgb24(shell.text),
            accent: rgb24(shell.brand_action),
            selection: rgb24(shell.message_surface),
            muted: rgb24(shell.muted),
            red: rgb24(colors.danger),
            green: rgb24(colors.success),
            yellow: rgb24(colors.warning),
            blue: rgb24(shell.brand_action),
            magenta: if shell.dark { 0xc084fc } else { 0x7c3aed },
            cyan: if shell.dark { 0x2dd4bf } else { 0x0d9488 },
        }
    });
    kantor_ui::tokens::apply_palette(&palette, cx);
    kantor_ui::fonts::apply(None, cx);
    if !cx.has_global::<SystemCodeFont>() {
        let family = gpui_kit::component::Theme::global(cx).mono_font_family.clone();
        cx.set_global(SystemCodeFont(family));
    }
    let typography = cx.default_global::<ui_primitives::ActiveTypography>();
    let size = px(typography.code_font_size);
    let text_offset = typography.ui_delta_px;
    let mono = typography.code_font_family.clone()
        .unwrap_or_else(|| cx.global::<SystemCodeFont>().0.clone());
    gpui_kit::component::Theme::update(cx, |theme| {
        theme.mono_font_family = mono.clone();
        theme.mono_font_size = size;
    });
    let kui_theme = kui::Theme::global_mut(cx);
    kui_theme.text_size_offset = text_offset;
    kui_theme.code_typography = Some((mono, size));
    cx.refresh_windows();
}

/// Build the unified native macOS transparent-titlebar `WindowOptions` shared across windows.
pub fn studio_window_options(
    title: impl Into<SharedString>,
    window_size: Size<Pixels>,
    min_size: Size<Pixels>,
    cx: &App,
) -> WindowOptions {
    WindowOptions {
        app_id: Some("kar-desktop".into()),
        window_background: if ui_primitives::material::native_glass() {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Opaque
        },
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: true,
            // Align the right edge of the three lights with AppKit's window-sharing pill.
            traffic_light_position: Some(point(
                px(12.),
                px(ui_primitives::MACOS_TITLEBAR_CENTER_Y - 7.),
            )),
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            window_size,
            cx,
        ))),
        window_min_size: Some(min_size),
        ..Default::default()
    }
}

/// Stage 1 integration smoke view: renders real KUI controls (`ResizablePanels`, `AiSpark`,
/// `ThemeSwitch`, `Badge`, `Input`, `CopyButton`, `pro_btn`) alongside a real Kantor `DocsView`
/// inside an edge-to-edge Kense monochrome shell, proving unified `gpui-pre 0.3.11` compatibility.
pub struct SmokeIntegrationView {
    panels: Entity<ResizablePanels>,
    spark: Entity<AiSpark>,
    theme_switch: Entity<ThemeSwitch>,
    badge: Entity<Badge>,
    input: Entity<Input>,
    copy_btn: Entity<CopyButton>,
    docs_view: Entity<DocsView>,
    clicks: usize,
}

impl SmokeIntegrationView {
    pub fn new(sample_docx: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let spark = cx.new(|_| {
            AiSpark::new("smoke-ai-spark")
                .with_size(18.)
                .with_tone(SparkTone::Foreground)
                .with_status(SparkStatus::Idle)
        });
        let theme_switch = cx.new(|_| {
            ThemeSwitch::new("smoke-theme-switch")
                .with_icon_only(true)
                .with_compact(true)
        });
        cx.subscribe(&theme_switch, |_, _, ev: &ThemeChange, cx| {
            apply_unified_theme(ev.next, cx);
            cx.notify();
        })
        .detach();

        let badge = cx.new(|_| Badge::new("gpui-pre 0.3.11", BadgeTone::Success));
        let input = cx.new(|cx| {
            Input::new("smoke-kui-input", "新增段落内容", window, cx)
                .with_value("来自同窗 KUI 组件库的原生联动段落", window, cx)
                .with_placeholder("输入段落文本并写入右侧 Kantor Word 画布…", cx)
        });
        let copy_btn = cx.new(|_| {
            CopyButton::new(
                "smoke-copy-tree",
                "gpui-pre v0.3.11 (KUI + Kantor unified)",
                "复制依赖信息",
            )
            .with_compact(true)
        });
        let docs_view = cx.new(|cx| DocsView::new(sample_docx, window, cx));

        let weak_left = cx.weak_entity();
        let weak_right = cx.weak_entity();

        let panels = cx.new(|cx| {
            ResizablePanels::new(
                "smoke-split",
                "Kwrok KUI + Kantor Split",
                vec![
                    Pane::new("kui-pane", "KUI Controls", 0.34, move |_, window, cx| {
                        weak_left
                            .upgrade()
                            .map(|e| e.update(cx, |this, cx| this.render_left_pane(window, cx)))
                            .unwrap_or_else(|| div().into_any_element())
                    })
                    .with_limits(300., Some(460.)),
                    Pane::new("kantor-pane", "Kantor DocsView", 0.66, move |_, window, cx| {
                        weak_right
                            .upgrade()
                            .map(|e| e.update(cx, |this, cx| this.render_right_pane(window, cx)))
                            .unwrap_or_else(|| div().into_any_element())
                    })
                    .with_limits(440., None),
                ],
                cx,
            )
        });

        Self {
            panels,
            spark,
            theme_switch,
            badge,
            input,
            copy_btn,
            docs_view,
            clicks: 0,
        }
    }

    fn render_left_pane(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let s = StudioPalette::get(cx);
        let para_count = self.docs_view.read(cx).agent_read(|doc, _| doc.len());

        div()
            .size_full()
            .bg(s.stage)
            .flex()
            .flex_col()
            .child(
                titlebar_row(window)
                    .pl(px(macos_traffic_pad(window)))
                    .pr(px(12.))
                    .border_b_1()
                    .border_color(s.line_soft)
                    .justify_between()
                    .child(render_kar_brand_lockup(&s, cx))
                    .child(self.theme_switch.clone()),
            )
            .child(
                div()
                    .px(px(16.))
                    .py(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .p(px(16.))
                            .rounded(px(R_CARD))
                            .bg(s.card)
                            .border_1()
                            .border_color(s.line)
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(self.spark.clone())
                                            .child(
                                                div()
                                                    .text_size(px(FS_READ))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(s.ink)
                                                    .child("同窗原生集成验证"),
                                            ),
                                    )
                                    .child(self.badge.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(FS_UI))
                                    .line_height(px(LH_UI))
                                    .text_color(s.text)
                                    .child(format!(
                                        "KUI 原生控件与 Kantor DocsView 共享同一 gpui-pre 0.3.11 实例。当前共 {para_count} 个段落，已联动 {} 次。",
                                        self.clicks
                                    )),
                            )
                            .child(self.input.clone())
                            .child(
                                div()
                                    .pt(px(2.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(8.))
                                    .child(self.copy_btn.clone())
                                    .child(
                                        ui_motion::primary_button(
                                            "smoke-insert-btn",
                                            "插入段落至 Word",
                                            &s,
                                        )
                                        .on_press(cx, |this, window, cx| {
                                            this.clicks += 1;
                                            let next = this.clicks;
                                            let custom =
                                                this.input.read(cx).value(cx).trim().to_owned();
                                            let text = if custom.is_empty() {
                                                format!("来自 KUI 控件的第 {next} 次同窗写入")
                                            } else {
                                                format!("{custom} (#{next})")
                                            };
                                            this.spark.update(cx, |sp, cx| {
                                                sp.set_status(SparkStatus::Done, cx);
                                            });
                                            this.badge.update(cx, |b, cx| {
                                                b.set(
                                                    format!("已写入 #{next}"),
                                                    BadgeTone::Success,
                                                    cx,
                                                );
                                            });
                                            this.docs_view.update(cx, |v, cx| {
                                                let _ = v.agent_edit(
                                                    "KUI 控件插入段落",
                                                    window,
                                                    cx,
                                                    |doc, sel| {
                                                        let items = vec![
                                                            docs_engine::agent::NewParagraph {
                                                                text,
                                                                style: None,
                                                                list: None,
                                                                level: 0,
                                                            },
                                                        ];
                                                        let after = doc.len().checked_sub(1);
                                                        let r = doc
                                                            .insert_paragraphs(after, &items, sel)
                                                            .map_err(anyhow::Error::msg)?;
                                                        let end =
                                                            doc.paragraph(r.end - 1).text.len();
                                                        Ok((
                                                            (),
                                                            docs_engine::Selection {
                                                                anchor:
                                                                    docs_engine::Position::new(
                                                                        r.start, 0,
                                                                    ),
                                                                head: docs_engine::Position::new(
                                                                    r.end - 1,
                                                                    end,
                                                                ),
                                                            },
                                                        ))
                                                    },
                                                );
                                            });
                                            cx.notify();
                                        }),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_right_pane(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let s = StudioPalette::get(cx);
        div()
            .size_full()
            .bg(s.surface)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                titlebar_row(window)
                    .px(px(12.))
                    .border_b_1()
                    .border_color(s.line_soft)
                    .bg(s.stage)
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(render_office_chip(crate::types::OfficeKind::Word, &s))
                            .child(
                                div()
                                    .text_size(px(FS_UI))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child("Kantor DocsView"),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .text_color(s.muted)
                            .child("纯 Rust · 原生 GPUI"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.docs_view.clone()),
            )
            .into_any_element()
    }
}

impl Render for SmokeIntegrationView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = StudioPalette::get(cx);
        div()
            .size_full()
            .bg(s.stage)
            .font_family(ui_font(cx))
            .text_color(s.ink)
            .children(kantor_ui::overlay::menu_shield(window))
            .child(self.panels.clone())
    }
}
