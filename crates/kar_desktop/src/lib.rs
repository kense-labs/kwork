//! Pure-Rust GPUI desktop workbench combining KUI chrome, Kar chat SDK, and Kantor Office editors.

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
        badge::{Badge, BadgeTone},
        input::Input,
        pro_chrome::{ProBtnStyle, ProPalette, pro_btn},
        resizable_panels::{Pane, ResizablePanels},
        theme_switch::{ThemeChange, ThemeSwitch},
    },
    primitives::{control::UiLanguage, pro_icons::ProIcon},
};

use crate::ui_primitives::{HEADER_H, render_kar_brand_lockup};

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

/// Apply a cohesive Light or Dark theme across both KUI (`kui::Theme`) and Kantor (`kantor_theme`).
pub fn apply_unified_theme(mode: ThemeMode, cx: &mut App) {
    kui::Theme::global_mut(cx).set_mode(mode);
    let palette = match mode {
        ThemeMode::Light => kantor_theme::Palette {
            dark: false,
            background: 0xffffff,
            dark_background: 0xf8f8f8,
            lighter_background: 0xf4f4f5,
            foreground: 0x111113,
            dark_foreground: 0x63636b,
            accent: 0x171717,
            selection: 0xe4e4e7,
            muted: 0x73737a,
            red: 0xd92d20,
            green: 0x16a34a,
            yellow: 0xd97706,
            blue: 0x245bc4,
            magenta: 0x7142b6,
            cyan: 0x08776d,
        },
        ThemeMode::Dark => kantor_theme::Palette {
            dark: true,
            background: 0x141416,
            dark_background: 0x0e0e10,
            lighter_background: 0x1c1c1f,
            foreground: 0xf4f4f5,
            dark_foreground: 0x9ca3af,
            accent: 0xe4e4e7,
            selection: 0x27272a,
            muted: 0x71717a,
            red: 0xf97066,
            green: 0x4ade80,
            yellow: 0xfbbf24,
            blue: 0x60a5fa,
            magenta: 0xbc9cff,
            cyan: 0x4fc3b4,
        },
    };
    kantor_ui::tokens::apply_palette(&palette, cx);
    kantor_ui::fonts::apply(None, cx);
    cx.refresh_windows();
}

/// Stage 1 integration smoke view: renders real KUI controls alongside a real Kantor `DocsView`
/// inside a single GPUI window, proving unified `gpui-pre 0.3.11` type compatibility.
pub struct SmokeIntegrationView {
    panels: Entity<ResizablePanels>,
    theme_switch: Entity<ThemeSwitch>,
    badge: Entity<Badge>,
    input: Entity<Input>,
    docs_view: Entity<DocsView>,
    clicks: usize,
}

impl SmokeIntegrationView {
    pub fn new(sample_docx: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme_switch = cx.new(|_| ThemeSwitch::new("smoke-theme-switch").with_icon_only(true));
        cx.subscribe(&theme_switch, |_, _, ev: &ThemeChange, cx| {
            apply_unified_theme(ev.next, cx);
            cx.notify();
        })
        .detach();

        let badge = cx.new(|_| Badge::new("gpui-pre 0.3.11 统一类型树", BadgeTone::Neutral));
        let input = cx.new(|cx| {
            Input::new("smoke-kui-input", "新增段落内容 (KUI Input)", window, cx)
                .with_value("来自同窗 KUI 控件的原生联动段落", window, cx)
                .with_placeholder("输入段落文本并插入右侧 Kantor Word 画布…", cx)
        });
        let docs_view = cx.new(|cx| DocsView::new(sample_docx, window, cx));

        let weak_left = cx.weak_entity();
        let right_docs = docs_view.clone();

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
                    .with_limits(280., Some(480.)),
                    Pane::new("kantor-pane", "Kantor DocsView", 0.66, move |_, _, _| {
                        div().size_full().child(right_docs.clone())
                    })
                    .with_limits(420., None),
                ],
                cx,
            )
        });

        Self {
            panels,
            theme_switch,
            badge,
            input,
            docs_view,
            clicks: 0,
        }
    }

    fn render_left_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = ProPalette::get(cx);
        let para_count = self.docs_view.read(cx).agent_read(|doc, _| doc.len());

        div()
            .size_full()
            .bg(p.side_tint)
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(HEADER_H))
                    .px(px(14.))
                    .border_b_1()
                    .border_color(p.line)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(render_kar_brand_lockup(&p, cx))
                    .child(self.theme_switch.clone()),
            )
            .child(
                div()
                    .p(px(18.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.ink)
                                    .child("同窗原生集成验证"),
                            )
                            .child(self.badge.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .text_color(p.text)
                            .child(format!(
                                "左侧 KUI 组件库与右侧 Kantor DocsView 共享唯一 gpui-pre 0.3.11 实例。当前文档共 {para_count} 个段落，已联动写入 {} 次。",
                                self.clicks
                            )),
                    )
                    .child(
                        div()
                            .p(px(14.))
                            .rounded(px(10.))
                            .bg(p.card)
                            .border_1()
                            .border_color(p.line)
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(self.input.clone())
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .child(pro_btn(
                                        "smoke-insert-btn",
                                        "插入段落至右侧 Word",
                                        Some(ProIcon::Plus),
                                        ProBtnStyle::Primary,
                                        false,
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
                                            this.badge.update(cx, |b, cx| {
                                                b.set(
                                                    format!("已联动 #{next}"),
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
}

impl Render for SmokeIntegrationView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.panels.clone())
    }
}
