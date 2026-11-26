//! A labelled source excerpt; the copy action awaits the host clipboard.
use crate::{
    components::copy_button::CopyButton,
    primitives::{
        family::{Paint, Slide},
        glyph::{Glyph, icon, transformed},
        nav::*,
    },
};
use gpui::{prelude::*, *};
struct SourceHeader {
    filename: SharedString,
    language: SharedString,
    copy: Entity<CopyButton>,
}

pub struct CodeBlock {
    header: Option<SourceHeader>,
    source: SharedString,
    max_lines: usize,
    lines: usize,
    open: bool,
    fold: Slide,
}
impl CodeBlock {
    pub fn new(
        file: impl Into<SharedString>,
        language: impl Into<SharedString>,
        source: impl Into<SharedString>,
        cx: &mut App,
    ) -> Self {
        let mut block = Self::source_only(source);
        let copy = cx.new(|cx| {
            CopyButton::new("copy", block.source.clone(), tr(cx, "Copy code", "复制代码"))
        });
        block.header = Some(SourceHeader { filename: file.into(), language: language.into(), copy });
        block
    }

    /// Embed the source in a pane that provides its own file title and actions.
    pub fn source_only(source: impl Into<SharedString>) -> Self {
        let source = source.into();
        Self {
            header: None,
            lines: source.split('\n').count(),
            source,
            open: false,
            max_lines: 0,
            fold: Slide::new(0.),
        }
    }

    pub fn max_lines(mut self, max: usize) -> Self {
        self.max_lines = max;
        self
    }

    pub fn source(&self) -> SharedString {
        self.source.clone()
    }
}
impl Render for CodeBlock {
    fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Paint::get(cx);
        let foldable = self.max_lines > 0 && self.max_lines < self.lines;
        let a = self.fold.get(w, cx).clamp(0., 1.);
        let text = if self.open {
            tr(cx, "Show less", "收起")
        } else {
            tr(cx, "Show all", "显示全部")
        };
        panel(cx)
            .w_full()
            .when(self.header.is_some(), |block| block.max_w(px(536.)))
            .flex_none()
            .when_some(self.header.as_ref(), |block, header| block.child(
                row()
                    .p_3()
                    .gap_2()
                    .child(icon(Glyph::FileText, 16., p.faint))
                    .child(label(header.filename.clone(), 14., p.ink).flex_1().min_w_0().truncate())
                    .child(label(header.language.clone(), 12., p.faint).max_w(relative(0.24)).truncate())
                    .child(div().flex_none().child(header.copy.clone())),
            ))
            .child(
                div()
                    .id("source")
                    .w_full()
                    .min_w_0()
                    .tab_stop(true)
                    .overflow_x_scroll()
                    .overflow_y_hidden()
                    .when(foldable, |d| {
                        d.h(px(33.
                            + 20.
                                * (self.max_lines as f32
                                    + (self.lines - self.max_lines) as f32 * a)))
                    })
                    .p_4()
                    .when(self.header.is_some(), |d| d.border_t_1())
                    .border_color(p.line)
                    .bg(p.stage)
                    .font_family("monospace")
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .flex()
                    .child(div().flex_none().whitespace_nowrap().child(self.source.clone())),
            )
            .when(foldable, |b| {
                b.child(
                    button("toggle-source", text, cx)
                        .aria_expanded(self.open)
                        .child(text)
                        .child(transformed(
                            Glyph::Down,
                            14.,
                            p.secondary,
                            a * std::f32::consts::PI,
                            1.,
                            (0., 0.),
                            1.,
                        ))
                        .on_click(cx.listener(|s, _, _, cx| {
                            s.open = !s.open;
                            s.fold.duration(if s.open { 1. } else { 0. }, 0.32, cx);
                            cx.notify()
                        })),
                )
            })
    }
}
