//! Write text to the clipboard and acknowledge only successful writes.
use crate::foundation::Theme;
use crate::primitives::{
    clipboard,
    control::{UiLanguage, activate},
    family::{
        Paint, PhaseClock, Slide, Tone, face, lettering, measure, press, standard_ease as ease,
    },
    glyph::{Glyph, transformed},
};
use gpui::{prelude::*, *};
use std::time::Duration;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyResult(pub bool);
pub struct CopyButton {
    id: ElementId,
    text: SharedString,
    label: SharedString,
    icon_only: bool,
    compact: bool,
    result: Option<bool>,
    previous: Option<bool>,
    clock: PhaseClock,
    width: Option<Slide>,
    pressed: Slide,
    busy: bool,
    epoch: u64,
}
impl EventEmitter<CopyResult> for CopyButton {}
impl CopyButton {
    pub fn new(
        id: impl Into<ElementId>,
        text: impl Into<SharedString>,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            label: label.into(),
            icon_only: false,
            compact: false,
            result: None,
            previous: None,
            clock: Default::default(),
            width: None,
            pressed: Slide::snappy(0.),
            busy: false,
            epoch: 0,
        }
    }
    pub fn icon_only(mut self) -> Self {
        self.icon_only = true;
        self
    }
    pub fn with_compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }
    pub fn set_text(&mut self, text: impl Into<SharedString>) {
        self.text = text.into();
    }
    fn copy(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.epoch += 1;
        let epoch = self.epoch;
        let write = clipboard::write(&self.text, cx);
        cx.spawn(async move |v, cx| {
            let ok = write.await;
            let _ = v.update(cx, |v, cx| {
                v.busy = false;
                if v.result != Some(ok) {
                    v.previous = v.result;
                    v.clock.restart(cx);
                    v.result = Some(ok);
                }
                cx.emit(CopyResult(ok));
                cx.notify()
            });
            cx.background_executor()
                .timer(Duration::from_millis(1900))
                .await;
            let _ = v.update(cx, |v, cx| {
                if v.epoch == epoch {
                    v.previous = v.result;
                    v.clock.restart(cx);
                    v.result = None;
                    cx.notify()
                }
            });
        })
        .detach();
    }
}
impl Render for CopyButton {
    fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Paint::get(cx);
        let l = UiLanguage::current(cx);
        let phase = |s| -> (Glyph, SharedString, Hsla) {
            match s {
                None => (Glyph::Copy, self.label.clone(), p.ink),
                Some(true) => (Glyph::Check, l.text("Copied", "已复制").into(), p.success),
                Some(false) => (Glyph::Alert, l.text("Failed", "失败").into(), p.danger),
            }
        };
        let (g, label, ink) = phase(self.result);
        let (old, prev, old_ink) = phase(self.previous);
        let age = self.clock.time(0.26, w, cx);
        let fs = if self.compact { 12. } else { 14. };
        let btn_h = if self.compact { 26. } else { 36. };
        let natural = measure(&label, fs, w);
        let width = self.width.get_or_insert_with(|| Slide::new(natural));
        width.duration(natural, 0.22, cx);
        let visible = width.get(w, cx);
        let scale = 1. - self.pressed.get(w, cx) * if self.icon_only { 0.04 } else { 0.03 };
        let side = (if self.compact { 13.5 } else { 16. }) * scale;
        let idle_ink = if self.compact && self.result.is_none() {
            p.secondary
        } else {
            ink
        };
        let mut glyph = div().relative().size(px(side));
        if age < 0.10 {
            glyph = glyph.child(div().absolute().inset_0().child(transformed(
                old,
                side,
                old_ink.opacity(1. - ease(age / 0.10)),
                0.,
                1.,
                (0., 0.),
                1.,
            )))
        }
        glyph = glyph.child(div().absolute().inset_0().child(transformed(
            g,
            side,
            idle_ink.opacity(ease(age / 0.16)),
            0.,
            0.85 + 0.15 * ease(age / 0.16),
            (0., 0.),
            if g == Glyph::Check {
                ease((age - 0.04) / 0.22)
            } else {
                1.
            },
        )));
        let compact = self.compact;
        let icon_only = self.icon_only;
        let pad_base = if compact { 36. } else { 50. };
        let f = face(
            self.id.clone(),
            self.label.clone(),
            btn_h,
            if compact {
                6.5
            } else {
                Theme::global(cx).tokens.radius.md.into()
            },
            if icon_only {
                Tone::Clear
            } else {
                Tone::Surface
            },
            cx,
        )
        .border_1()
        .border_color(if icon_only {
            transparent_black()
        } else {
            p.line
        })
        .h(px(btn_h * scale))
        .w(px(
            (if icon_only { btn_h } else { pad_base + visible }) * scale
        ))
        .when(compact && icon_only, |e| e.px_0())
        .hover(move |s| {
            if compact && icon_only {
                s.bg(p.hover)
            } else {
                s.border_color(p.secondary.opacity(0.48))
            }
        })
        .child(glyph)
        .when(!icon_only, |e| {
            e.child(lettering(
                prev,
                label.clone(),
                age,
                false,
                fs * scale,
                p.ink,
                visible * scale,
            ))
        })
        .debug_selector({
            let value = label.clone();
            move || value.to_string()
        })
        .aria_description(label);
        div()
            .flex()
            .items_center()
            .justify_center()
            .h(px(btn_h))
            .w(px(if self.icon_only { btn_h } else { pad_base + visible }))
            .child(activate(
                press(f, cx, |v| &mut v.pressed),
                cx,
                |v, _, cx| v.copy(cx),
            ))
    }
}
