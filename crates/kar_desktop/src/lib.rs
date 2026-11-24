//! Pure-Rust GPUI desktop workbench combining KUI chrome, Kar chat SDK, and Kantor Office editors.

extern crate gpui_kit as gpui;

use std::path::PathBuf;

use docs_ui::DocsView;
use gpui::{prelude::*, *};
use kui::components::{
    badge::{Badge, BadgeTone},
    button::Button,
    input::Input,
    resizable_panels::{Pane, ResizablePanels},
};

/// Initialize shared GPUI Kit, Kantor Office editors, and KUI foundation globals.
pub fn init_runtime_ui(cx: &mut App) {
    gpui_kit::init(cx);
    kantor_ui::widget_strings::init();
    let palette =
        kantor_theme::catalog::resolve(&kantor_theme::catalog::ThemeId::parse("light"));
    kantor_ui::tokens::apply_palette(&palette, cx);
    kantor_ui::fonts::apply(None, cx);
    kantor_ui::find_bar::init(cx);
    sheets_ui::init(cx);
    docs_ui::init(cx);
    slides_ui::init(cx);
    kui::init(cx);
}

/// Stage 1 integration smoke view: renders real KUI controls alongside a real Kantor `DocsView`
/// inside a single GPUI window, proving unified `gpui-pre 0.3.11` type compatibility.
pub struct SmokeIntegrationView {
    panels: Entity<ResizablePanels>,
}

impl SmokeIntegrationView {
    pub fn new(sample_docx: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let badge = cx.new(|_| Badge::new("GPUI 0.3.11 Unified", BadgeTone::Success));
        let input = cx.new(|cx| {
            Input::new("smoke-kui-input", "Kar 指令输入 (KUI Input)", window, cx)
                .with_placeholder("同一窗口内 KUI Input 与 Kantor DocsView 联合渲染", cx)
        });
        let docs_view = cx.new(|cx| DocsView::new(sample_docx, window, cx));
        let clicks = std::rc::Rc::new(std::cell::Cell::new(0usize));

        let left_badge = badge.clone();
        let left_input = input.clone();
        let left_docs = docs_view.clone();
        let right_docs = docs_view.clone();

        let panels = cx.new(|cx| {
            ResizablePanels::new(
                "smoke-split",
                "Kar KUI + Kantor Split",
                vec![
                    Pane::new("kui-pane", "KUI Controls", 0.38, move |_, _, cx| {
                        let count = clicks.get();
                        let clicks_ref = clicks.clone();
                        let badge_for_click = left_badge.clone();
                        let docs_for_click = left_docs.clone();
                        let para_count = left_docs.read(cx).agent_read(|doc, _| doc.len());
                        div()
                            .size_full()
                            .p_4()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .bg(rgb(0xfafafa))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(0x111111))
                                            .child("Kar Desktop · 集成验证"),
                                    )
                                    .child(left_badge.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(rgb(0x52525b))
                                    .child(format!(
                                        "右侧 Kantor DocsView 当前段落数：{para_count} · KUI 点击计数：{count}"
                                    )),
                            )
                            .child(left_input.clone())
                            .child(
                                Button::new("smoke-kui-btn")
                                    .primary()
                                    .label("KUI Button → 向右侧 Kantor Word 插入段落")
                                    .on_click(move |_, window, cx| {
                                        let next = clicks_ref.get() + 1;
                                        clicks_ref.set(next);
                                        badge_for_click.update(cx, |b, cx| {
                                            b.set(
                                                format!("已联动 #{next}"),
                                                BadgeTone::Info,
                                                cx,
                                            );
                                        });
                                        docs_for_click.update(cx, |v, cx| {
                                            let _ = v.agent_edit(
                                                "KUI 控件插入段落",
                                                window,
                                                cx,
                                                |doc, sel| {
                                                    let items = vec![
                                                        docs_engine::agent::NewParagraph {
                                                            text: format!(
                                                                "来自同窗 KUI Button 的第 {next} 次联动写入"
                                                            ),
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
                                                            anchor: docs_engine::Position::new(
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
                                    }),
                            )
                    })
                    .with_limits(260., Some(560.)),
                    Pane::new("kantor-pane", "Kantor DocsView", 0.62, move |_, _, _| {
                        div().size_full().child(right_docs.clone())
                    })
                    .with_limits(360., None),
                ],
                cx,
            )
        });

        Self { panels }
    }
}

impl Render for SmokeIntegrationView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.panels.clone())
    }
}
