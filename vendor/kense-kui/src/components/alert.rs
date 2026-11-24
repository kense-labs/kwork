//! Persistent feedback with an optional recovery action.
use crate::primitives::{
    family::{Paint, PhaseClock, ease},
    glyph::{Glyph, transformed},
    presentation::*,
};
use gpui::{prelude::*, *};
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Success,
    Warning,
    Error,
}
pub struct Alert {
    title: ChangingText,
    body: ChangingText,
    previous_tone: Severity,
    severity: Severity,
    action: Option<SharedString>,
    compact_inline: bool,
    clock: PhaseClock,
}
pub struct AlertAction;
impl EventEmitter<AlertAction> for Alert {}
impl Alert {
    pub fn new(
        title: impl Into<SharedString>,
        body: impl Into<SharedString>,
        severity: Severity,
    ) -> Self {
        Self {
            title: ChangingText::new(title),
            body: ChangingText::new(body),
            previous_tone: severity,
            severity,
            action: None,
            compact_inline: false,
            clock: Default::default(),
        }
    }
    pub fn with_action(mut self, label: impl Into<SharedString>) -> Self {
        self.action = Some(label.into());
        self
    }
    pub fn with_compact_inline(mut self, compact: bool) -> Self {
        self.compact_inline = compact;
        self
    }
    pub fn replace(&mut self, next: Self, cx: &mut Context<Self>) {
        self.title.set(next.title.current, 1., cx);
        self.body.set(next.body.current, 1., cx);
        self.previous_tone = self.severity;
        self.severity = next.severity;
        self.action = next.action;
        if next.compact_inline {
            self.compact_inline = true;
        }
        self.clock.restart(cx);
        cx.notify()
    }
}
impl Render for Alert {
    fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Paint::get(cx);
        let color = |severity| -> Hsla {
            match severity {
                Severity::Success => p.success,
                Severity::Error => p.danger,
                Severity::Warning => rgb(0xc49343).into(),
                Severity::Info => p.secondary,
            }
        };
        let glyph_el = {
            let glyph = |s| {
                if matches!(s, Severity::Success) {
                    Glyph::Check
                } else {
                    Glyph::Alert
                }
            };
            let age = self.clock.time(0.32, w, cx);
            let a = if std::mem::discriminant(&self.severity)
                == std::mem::discriminant(&self.previous_tone)
            {
                1.
            } else {
                ease(age / 0.26)
            };
            let leave = 1. - ease(age / 0.16);
            let ic_sz = if self.compact_inline { 15. } else { 18. };
            div()
                .relative()
                .flex_none()
                .mt(px(if self.compact_inline { 1.5 } else { 0. }))
                .size(px(ic_sz))
                .child(transformed(
                    glyph(self.severity),
                    ic_sz,
                    color(self.severity).opacity(a),
                    0.,
                    0.6 + 0.4 * a,
                    (0., 0.),
                    1.,
                ))
                .when(a < 1. && leave > 0., |e| {
                    e.child(div().absolute().inset_0().child(transformed(
                        glyph(self.previous_tone),
                        ic_sz,
                        color(self.previous_tone).opacity(leave),
                        0.,
                        0.6 + 0.4 * leave,
                        (0., 0.),
                        1.,
                    )))
                })
        };
        if self.compact_inline {
            let mut card = row()
                .id("alert-alert")
                .role(Role::Alert)
                .w_full()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .px(px(13.))
                .py(px(10.))
                .rounded(px(10.))
                .bg(p.panel)
                .border_1()
                .border_color(p.line)
                .text_color(p.ink)
                .child(
                    row()
                        .flex_1()
                        .min_w_0()
                        .items_start()
                        .gap(px(9.))
                        .child(glyph_el)
                        .child(
                            column()
                                .flex_1()
                                .min_w_0()
                                .gap(px(2.))
                                .child(
                                    self.title
                                        .render(13., w, cx)
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::MEDIUM),
                                )
                                .child(
                                    self.body
                                        .render(12., w, cx)
                                        .text_size(px(12.))
                                        .line_height(px(17.))
                                        .text_color(p.secondary),
                                ),
                        ),
                );
            if let Some(a) = self.action.clone() {
                card = card.child(
                    command("alert-action", a, cx, |_, _, cx| cx.emit(AlertAction))
                        .h(px(28.))
                        .px(px(10.))
                        .text_size(px(12.))
                        .flex_none(),
                );
            }
            return column().w_full().max_w_full().child(card);
        }
        column()
            .max_w_full()
            .gap_3()
            .items_start()
            .child(
                frame(520., p)
                    .id("alert-alert")
                    .role(Role::Alert)
                    .flex_row()
                    .items_start()
                    .gap_3()
                    .child(glyph_el)
                    .child(
                        column()
                            .gap_1()
                            .child(
                                self.title
                                    .render(15., w, cx)
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(
                                self.body
                                    .render(12., w, cx)
                                    .text_size(px(12.))
                                    .text_color(p.secondary),
                            ),
                    ),
            )
            .when_some(self.action.clone(), |el, a| {
                el.child(command("alert-action", a, cx, |_, _, cx| {
                    cx.emit(AlertAction)
                }))
            })
    }
}
