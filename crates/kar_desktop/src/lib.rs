//! Pure-Rust GPUI desktop workbench combining KUI components, Kense architectural monochrome,
//! Kar Rust SDK (`ChatStore`/`ChatSession`/`HttpChatFeed`/`ChatReplica`/`serve_mcp_tools`),
//! and Kantor Office editors.

#![forbid(unsafe_code)]

extern crate gpui_kit as gpui;

pub mod brief;
pub mod client_store;
pub mod coedit;
pub mod kar_bridge;
pub mod mcp_office;
pub mod office_host;
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
pub mod ui_primitives;
pub mod ui_remote;
pub mod ui_schedules;
pub mod ui_settings;
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
        pro_chrome::{ProBtnStyle, pro_btn},
        resizable_panels::{Pane, ResizablePanels},
        theme_switch::{ThemeChange, ThemeSwitch},
    },
    primitives::{control::UiLanguage, pro_icons::ProIcon},
};

use crate::ui_primitives::{
    FS_META, FS_READ, FS_UI, HEADER_H, LH_UI, MACOS_TRAFFIC_LIGHT_PAD, R_CARD, StudioPalette,
    render_kar_brand_lockup, render_office_chip, ui_font,
};

pub const APP_NAME: &str = "Kwrok";

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
    kui::init(cx);
    ui_primitives::material::init_window_glass(cx);
    ui_chat::init_project_picker_keys(cx);
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

/// Apply authentic Kense architectural monochrome tokens (`DESIGN.md` & `ProPalette`) across KUI and Kantor.
pub fn apply_unified_theme(mode: ThemeMode, cx: &mut App) {
    {
        let theme = kui::Theme::global_mut(cx);
        theme.set_mode(mode);
    }

    let shell = ui_primitives::StudioPalette::get(cx);
    let colors = &mut kui::Theme::global_mut(cx).tokens.colors;
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
    colors.focus_outline = shell.brand_action;

    let rgb24 = |color: Hsla| u32::from(color.to_rgb()) >> 8;
    let palette = kantor_theme::Palette {
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
    };
    kantor_ui::tokens::apply_palette(&palette, cx);
    kantor_ui::fonts::apply(None, cx);
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
            traffic_light_position: Some(point(px(16.), px(19.))),
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
                    Pane::new("kui-pane", "KUI Controls", 0.34, move |_, _, cx| {
                        weak_left
                            .upgrade()
                            .map(|e| e.update(cx, |this, cx| this.render_left_pane(cx)))
                            .unwrap_or_else(|| div().into_any_element())
                    })
                    .with_limits(300., Some(460.)),
                    Pane::new("kantor-pane", "Kantor DocsView", 0.66, move |_, _, cx| {
                        weak_right
                            .upgrade()
                            .map(|e| e.update(cx, |this, cx| this.render_right_pane(cx)))
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

    fn render_left_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let s = StudioPalette::get(cx);
        let para_count = self.docs_view.read(cx).agent_read(|doc, _| doc.len());

        div()
            .size_full()
            .bg(s.stage)
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(HEADER_H))
                    .pl(px(if cfg!(target_os = "macos") {
                        MACOS_TRAFFIC_LIGHT_PAD
                    } else {
                        16.
                    }))
                    .pr(px(12.))
                    .border_b_1()
                    .border_color(s.line_soft)
                    .window_control_area(WindowControlArea::Drag)
                    .flex()
                    .items_center()
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
                                    .child(pro_btn(
                                        "smoke-insert-btn",
                                        "插入段落至 Word",
                                        Some(ProIcon::Plus),
                                        ProBtnStyle::Primary,
                                        true,
                                        cx,
                                        |this, window, cx| {
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
                                        },
                                    )),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_right_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let s = StudioPalette::get(cx);
        div()
            .size_full()
            .bg(s.surface)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .h(px(HEADER_H))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(s.line_soft)
                    .bg(s.stage)
                    .window_control_area(WindowControlArea::Drag)
                    .flex()
                    .items_center()
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
