//! Pure-Rust GPUI desktop workbench combining KUI components, Kense architectural monochrome,
//! Kar Rust SDK (`ChatStore`/`ChatSession`/`HttpChatFeed`/`ChatReplica`), and Kantor Office editors.

extern crate gpui_kit as gpui;

pub mod coedit;
pub mod kar_bridge;
pub mod office_host;
pub mod types;
pub mod ui_chat;
pub mod ui_office;
pub mod ui_primitives;
pub mod ui_sidebar;
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
    foundation::{ColorTokens, RadiusTokens},
    primitives::{control::UiLanguage, pro_icons::ProIcon},
};

use crate::ui_primitives::{
    HEADER_H, MACOS_TRAFFIC_LIGHT_PAD, StudioPalette, render_kar_brand_lockup, render_office_chip,
};

/// Initialize shared GPUI Kit, Kantor Office editors, and KUI foundation globals.
pub fn init_runtime_ui(cx: &mut App) {
    gpui_kit::init(cx);
    kantor_ui::widget_strings::init();
    kantor_ui::find_bar::init(cx);
    sheets_ui::init(cx);
    docs_ui::init(cx);
    slides_ui::init(cx);
    kui::init(cx);
    cx.set_global(UiLanguage::Chinese);
    apply_unified_theme(ThemeMode::Light, cx);
}

/// Apply authentic Kense architectural monochrome tokens (`DESIGN.md` & `BRAND.md`) across KUI and Kantor.
pub fn apply_unified_theme(mode: ThemeMode, cx: &mut App) {
    {
        let theme = kui::Theme::global_mut(cx);
        theme.set_mode(mode);
        theme.tokens.radius = RadiusTokens {
            sm: px(6.),
            md: px(8.),
            lg: px(12.),
            panel: px(12.),
            full: px(9999.),
        };
        theme.tokens.colors = match mode {
            ThemeMode::Light => ColorTokens {
                background: rgb(0xf6f6f7).into(),
                surface: rgb(0xffffff).into(),
                surface_muted: rgb(0xf4f4f5).into(),
                foreground: rgb(0x09090b).into(),
                text_secondary: rgb(0x52525b).into(),
                text_muted: rgb(0x71717a).into(),
                border: rgb(0xe4e4e7).into(),
                border_strong: rgb(0xd4d4d8).into(),
                accent: rgb(0x171717).into(),
                accent_foreground: rgb(0xffffff).into(),
                accent_subtle: rgb(0xf4f4f5).into(),
                danger: rgb(0xdc2626).into(),
                danger_foreground: rgb(0xffffff).into(),
                success: rgb(0x16a34a).into(),
                warning: rgb(0xd97706).into(),
                focus_outline: rgb(0x18181b).into(),
            },
            ThemeMode::Dark => ColorTokens {
                background: rgb(0x0c0c0e).into(),
                surface: rgb(0x18181c).into(),
                surface_muted: rgb(0x131316).into(),
                foreground: rgb(0xfafafa).into(),
                text_secondary: rgb(0xa1a1aa).into(),
                text_muted: rgb(0x71717a).into(),
                border: rgb(0x27272a).into(),
                border_strong: rgb(0x3f3f46).into(),
                accent: rgb(0xf4f4f5).into(),
                accent_foreground: rgb(0x09090b).into(),
                accent_subtle: rgb(0x27272a).into(),
                danger: rgb(0xf87171).into(),
                danger_foreground: rgb(0x09090b).into(),
                success: rgb(0x4ade80).into(),
                warning: rgb(0xfbbf24).into(),
                focus_outline: rgb(0xa1a1aa).into(),
            },
        };
    }

    let palette = match mode {
        ThemeMode::Light => kantor_theme::Palette {
            dark: false,
            background: 0xffffff,
            dark_background: 0xf4f4f5,
            lighter_background: 0xfafafb,
            foreground: 0x09090b,
            dark_foreground: 0x52525b,
            accent: 0x18181b,
            selection: 0xe4e4e7,
            muted: 0x71717a,
            red: 0xdc2626,
            green: 0x16a34a,
            yellow: 0xd97706,
            blue: 0x2563eb,
            magenta: 0x7c3aed,
            cyan: 0x0d9488,
        },
        ThemeMode::Dark => kantor_theme::Palette {
            dark: true,
            background: 0x131316,
            dark_background: 0x0c0c0e,
            lighter_background: 0x1b1b1f,
            foreground: 0xfafafa,
            dark_foreground: 0xa1a1aa,
            accent: 0x60a5fa,
            selection: 0x27272a,
            muted: 0x71717a,
            red: 0xf87171,
            green: 0x4ade80,
            yellow: 0xfbbf24,
            blue: 0x60a5fa,
            magenta: 0xc084fc,
            cyan: 0x2dd4bf,
        },
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
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(16.), px(15.))),
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
/// inside our Kense monochrome Inset frame, proving unified `gpui-pre 0.3.11` type compatibility.
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
            Input::new("smoke-kui-input", "新增段落内容 (KUI Input)", window, cx)
                .with_value("来自同窗 KUI 组件库的原生联动段落", window, cx)
                .with_placeholder("输入段落文本并插入右侧 Kantor Word 画布…", cx)
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
                "Kar KUI + Kantor Split",
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
                    .window_control_area(WindowControlArea::Drag)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(render_kar_brand_lockup(&s, cx))
                    .child(self.theme_switch.clone()),
            )
            .child(
                div()
                    .px(px(14.))
                    .py(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .p(px(14.))
                            .rounded(px(12.))
                            .bg(s.card_bg)
                            .border_1()
                            .border_color(s.border)
                            .shadow(s.composer_shadows())
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
                                            .gap(px(7.))
                                            .child(self.spark.clone())
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(s.ink)
                                                    .child("同窗原生集成验证"),
                                            ),
                                    )
                                    .child(self.badge.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .line_height(px(19.))
                                    .text_color(s.text)
                                    .child(format!(
                                        "KUI 原生控件（AiSpark / Input / Badge / CopyButton / ThemeSwitch）与 Kantor DocsView 共享同一 gpui-pre 0.3.11 实例。当前共 {para_count} 个段落，已联动 {} 次。",
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
            .bg(s.stage)
            .p(px(6.))
            .pl_0()
            .child(
                div()
                    .size_full()
                    .rounded(px(12.))
                    .bg(s.inset_bg)
                    .border_1()
                    .border_color(s.border)
                    .shadow(s.inset_shadows())
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(40.))
                            .px(px(12.))
                            .border_b_1()
                            .border_color(s.border_soft)
                            .bg(s.artifact_bg)
                            .window_control_area(WindowControlArea::Drag)
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(7.))
                                    .child(render_office_chip(
                                        crate::types::OfficeKind::Word,
                                        true,
                                        &s,
                                    ))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.ink)
                                            .child("Kantor DocsView"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
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
                    ),
            )
            .into_any_element()
    }
}

impl Render for SmokeIntegrationView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = StudioPalette::get(cx);
        div().size_full().bg(s.stage).child(self.panels.clone())
    }
}
