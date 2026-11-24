//! A labelled trigger for a short list of related commands.
use crate::{
    foundation::Theme,
    primitives::{
        family::{Paint, PhaseClock, Slide, Tone, face, lettering},
        glyph::{Glyph, icon, transformed},
        menu::{MenuAlign, MenuControl, MenuCore, MenuItem, MenuSelection, render_menu},
    },
};
use gpui::{prelude::*, *};
pub struct DropdownMenu {
    core: MenuCore,
    label: SharedString,
    icon: Option<Glyph>,
    icon_only: bool,
    compact: bool,
    ghost: bool,
    width: Option<Slide>,
    previous: SharedString,
    changed: PhaseClock,
    turn: Slide,
}
impl EventEmitter<MenuSelection> for DropdownMenu {}
impl DropdownMenu {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        items: Vec<MenuItem>,
        cx: &App,
    ) -> Self {
        let label = label.into();
        Self {
            core: MenuCore::new(id.into(), label.clone(), items, cx),
            previous: label.clone(),
            changed: Default::default(),
            label,
            icon: None,
            icon_only: false,
            compact: false,
            ghost: false,
            width: None,
            turn: Slide::snappy(0.),
        }
    }
    pub fn with_align(mut self, align: MenuAlign) -> Self {
        self.core.align = align;
        self
    }
    pub fn with_icon(mut self, g: Glyph) -> Self {
        self.icon = Some(g);
        self
    }
    pub fn icon_only(mut self) -> Self {
        self.icon_only = true;
        self
    }
    pub fn with_compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }
    pub fn with_ghost(mut self, ghost: bool) -> Self {
        self.ghost = ghost;
        self
    }
    pub fn set_label(&mut self, label: impl Into<SharedString>, cx: &mut Context<Self>) {
        let next = label.into();
        if self.label == next {
            return;
        }
        self.previous = std::mem::replace(&mut self.label, next);
        self.changed.restart(cx);
        self.core.name = self.label.clone();
        cx.notify()
    }
    pub fn is_open(&self) -> bool {
        self.core.open
    }
    pub fn focus(&self, w: &mut Window, cx: &mut App) {
        self.core.trigger.focus(w, cx)
    }
}
impl MenuControl for DropdownMenu {
    fn core(&mut self) -> &mut MenuCore {
        &mut self.core
    }
    fn selected(&mut self, i: usize, w: &mut Window, cx: &mut Context<Self>) {
        let id = self.core.items[i].id.clone();
        self.core.close(w, cx);
        cx.emit(MenuSelection(id));
        cx.notify()
    }
}
impl Render for DropdownMenu {
    fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Paint::get(cx);
        let r = if self.compact {
            px(7.)
        } else {
            Theme::global(cx).tokens.radius.md
        };
        let fs = if self.compact { 12.5 } else { 14. };
        let btn_h = if self.compact {
            28.
        } else if self.icon_only {
            32.
        } else {
            36.
        };
        let extra_pad = if self.compact { 36. } else { 52. };
        let icon_extra = if self.icon.is_some() {
            if self.compact { 18. } else { 22. }
        } else {
            0.
        };
        let target = if self.icon_only {
            btn_h
        } else {
            let mut style = w.text_style();
            style.font_weight = FontWeight::MEDIUM;
            let advance: f32 = w
                .text_system()
                .shape_line(
                    self.label.clone(),
                    px(fs),
                    &[style.to_run(self.label.len())],
                    None,
                )
                .width
                .into();
            advance.ceil() + extra_pad + icon_extra
        }
        .min(f32::from(w.viewport_size().width) - 16.);
        let sw = self.width.get_or_insert_with(|| Slide::new(target));
        sw.set(target, cx);
        let width = sw.get(w, cx);
        self.turn.set(if self.core.open { 1. } else { 0. }, cx);
        let use_ghost = self.ghost || (self.compact && self.icon_only);
        let btn = face(
            "trigger",
            self.label.clone(),
            btn_h,
            r.into(),
            if use_ghost { Tone::Clear } else { Tone::Surface },
            cx,
        )
        .w(px(width))
        .gap(px(if self.compact { 4.5 } else { 6. }))
        .text_size(px(fs))
        .when(self.icon_only, |e| e.px_0())
        .when(!self.icon_only && self.compact, |e| e.px(px(8.)))
        .when(!self.icon_only && !use_ghost, |e| e.border_1().border_color(p.line))
        .when(use_ghost && self.core.open, |e| e.bg(p.hover))
        .hover(move |s| s.bg(p.hover))
        .when_some(self.icon, |e, g| {
            if g == Glyph::More && self.compact {
                let dot_c = p.ink.opacity(0.68);
                e.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(2.5))
                        .child(div().size(px(2.6)).rounded_full().bg(dot_c))
                        .child(div().size(px(2.6)).rounded_full().bg(dot_c))
                        .child(div().size(px(2.6)).rounded_full().bg(dot_c)),
                )
            } else {
                e.child(icon(
                    g,
                    if self.compact { 14.5 } else { 15. },
                    p.secondary,
                ))
            }
        })
        .when(!self.icon_only, |e| {
            let text_width = (width - (extra_pad - 1.) - icon_extra).max(1.);
            let age = self.changed.time(0.48, w, cx);
            e.child(
                div()
                    .w(px(text_width))
                    .overflow_hidden()
                    .child(if age < 0.48 {
                        lettering(
                            self.previous.clone(),
                            self.label.clone(),
                            age,
                            false,
                            fs,
                            p.ink,
                            text_width,
                        )
                        .into_any_element()
                    } else {
                        div()
                            .truncate()
                            .child(self.label.clone())
                            .into_any_element()
                    }),
            )
            .child(transformed(
                Glyph::Down,
                if self.compact { 11.5 } else { 13. },
                p.secondary,
                self.turn.get(w, cx) * std::f32::consts::PI,
                1.,
                (0., 0.),
                1.,
            ))
        });
        render_menu(self, btn, false, 192., w, cx)
    }
}
