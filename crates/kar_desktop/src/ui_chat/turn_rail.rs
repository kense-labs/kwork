//! Scroll-linked conversation outline in the transcript's left margin.
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{prelude::*, *};
use kui::primitives::{family::Slide, pro_icons::pro_svg};

use crate::{
    ui_primitives::{DIVIDER_HIT_WIDTH, FOLDED_GRIP_HIT_WIDTH, FS_CONTROL, StudioPalette, UiLocale},
    workbench::KarWorkbench,
};

const PITCH: f32 = 10.;
const HIT_WIDTH: f32 = 32.;
pub(super) const CONTENT_CLEARANCE: f32 = HIT_WIDTH + 6.;

pub(super) fn leading_inset(sidebar_open: bool) -> f32 {
    let grip = if sidebar_open { (DIVIDER_HIT_WIDTH / 2.).ceil() } else { FOLDED_GRIP_HIT_WIDTH };
    grip + 12.
}

#[derive(Clone)]
pub(crate) struct TurnRailEntry {
    pub(crate) title: SharedString,
    pub(crate) reply: SharedString,
    pub(crate) bookmarked: bool,
}

struct Inspection {
    first: usize,
    pointer: Option<f32>,
    previous: usize,
    over_track: bool,
    over_card: bool,
    close_at: Option<Instant>,
    wheel: f32,
    focus: FocusHandle,
    bookmark_focus: FocusHandle,
    top: Rc<Cell<Pixels>>,
    swell: Slide,
    center: Slide,
}

impl Inspection {
    fn inspect(&mut self, pos: f32, cx: &App) {
        if self.pointer.is_none() {
            self.center.reset(pos);
        }
        self.center.duration(pos, 0.09, cx);
        self.pointer = Some(pos);
        self.previous = pos.round() as usize;
        self.close_at = None;
    }

    fn hover(&mut self, inside: bool, card: bool) {
        if card {
            self.over_card = inside;
        } else {
            self.over_track = inside;
        }
        self.close_at = (!self.over_card && !self.over_track)
            .then(|| Instant::now() + Duration::from_millis(100));
    }

    fn under_mouse(&self, y: Pixels, count: usize) -> f32 {
        self.first as f32
            + ((f32::from(y - self.top.get()) / PITCH) - 0.5).clamp(0., (count - 1) as f32)
    }

    fn reveal(&mut self, index: usize, count: usize) {
        self.first = self.first.clamp((index + 1).saturating_sub(count), index);
    }
}

fn bookmark(saved: bool, color: Hsla) -> AnyElement {
    const OUTLINE: &str = r#"<path d="M5 6a3 3 0 0 1 3-3h8a3 3 0 0 1 3 3v13a1.4 1.4 0 0 1-2.1 1.2L12 17.5l-4.9 2.7A1.4 1.4 0 0 1 5 19Z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>"#;
    const FILLED: &str = r#"<path d="M5 6a3 3 0 0 1 3-3h8a3 3 0 0 1 3 3v13a1.4 1.4 0 0 1-2.1 1.2L12 17.5l-4.9 2.7A1.4 1.4 0 0 1 5 19Z" fill="currentColor"/>"#;
    pro_svg(
        "0 0 24 24",
        if saved { FILLED } else { OUTLINE },
        16.,
        color,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    scope: SharedString,
    entries: Rc<[TurnRailEntry]>,
    current: usize,
    inset: f32,
    width: f32,
    height: f32,
    loc: UiLocale,
    s: &StudioPalette,
    window: &mut Window,
    cx: &mut Context<KarWorkbench>,
) -> AnyElement {
    let total = entries.len();
    if total == 0 || height < 28. || width - inset < 80. {
        return div().into_any_element();
    }
    let current = current.min(total - 1);
    let count = total.min(((height - 16.) / PITCH).floor().max(1.) as usize);
    let track_height = PITCH * count as f32;
    let track_top = ((height - track_height) * 0.5).floor();
    let state = window.use_keyed_state(
        SharedString::from(format!("turn-rail-{scope}")),
        cx,
        |_, cx| Inspection {
            first: current.saturating_sub(count / 2).min(total - count),
            pointer: None,
            previous: current,
            over_track: false,
            over_card: false,
            close_at: None,
            wheel: 0.,
            focus: cx.focus_handle().tab_stop(true),
            bookmark_focus: cx.focus_handle().tab_stop(true),
            top: Rc::new(Cell::new(px(0.))),
            swell: Slide::new(0.),
            center: Slide::new(current as f32),
        },
    );
    let (first, selected, previous, swell, center) = state.update(cx, |st, cx| {
        let keyboard = (st.focus.is_focused(window) || st.bookmark_focus.is_focused(window))
            && window.last_input_was_keyboard();
        if let Some(close_at) = st.close_at {
            if Instant::now() >= close_at {
                st.close_at = None;
                if !keyboard {
                    st.pointer = None;
                }
            } else {
                window.request_animation_frame();
            }
        }
        if !keyboard && !st.over_card && !st.over_track && st.close_at.is_none() {
            st.pointer = None;
        }
        st.first = st.first.min(total - count);
        if st.pointer.is_none() {
            st.reveal(current, count);
            if keyboard {
                st.inspect(current as f32, cx);
            }
        }
        let selected = st
            .pointer
            .map(|pos| (pos.round() as usize).clamp(st.first, st.first + count - 1));
        st.swell
            .duration(if selected.is_some() { 1. } else { 0. }, 0.14, cx);
        (
            st.first,
            selected,
            st.previous.min(total - 1),
            st.swell.get(window, cx),
            st.center.get(window, cx),
        )
    });

    let inspection = state.read(cx);
    let top = inspection.top.clone();
    let focus = inspection.focus.clone();
    let bookmark_focus = inspection.bookmark_focus.clone();
    let marks = entries.clone();
    let rest_ink = s.ink.opacity(if s.dark { 0.27 } else { 0.24 });
    let active_ink = s.ink.opacity(0.78);
    let saved_ink = s.ink.opacity(0.54);
    let hot_ink = s.ink;
    let lines = canvas(
        move |bounds, _, _| top.set(bounds.top()),
        move |bounds, _, win, _| {
            for i in first..first + count {
                let proximity = (1. - (i as f32 - center).abs() / 3.45).clamp(0., 1.);
                let len = 6. + 20. * proximity * swell;
                let color = if selected == Some(i) {
                    hot_ink
                } else if i == current {
                    active_ink
                } else if marks[i].bookmarked {
                    saved_ink
                } else {
                    rest_ink
                };
                let origin = bounds.origin + point(px(4.), px(PITCH * (i - first) as f32 + 4.));
                win.paint_quad(fill(Bounds::new(origin, size(px(len), px(2.))), color));
            }
        },
    )
    .size_full();

    let hover = state.clone();
    let moved = state.clone();
    let clicked = state.clone();
    let keys = state.clone();
    let scrolled = state.clone();
    let index = selected.unwrap_or(current);
    let mut track = div()
        .id("turn-rail-track")
        .absolute()
        .top(px(track_top))
        .left(px(inset))
        .w(px(HIT_WIDTH))
        .h(px(track_height))
        .track_focus(&focus)
        .tab_stop(true)
        .role(Role::Slider)
        .aria_orientation(Orientation::Vertical)
        .aria_label(loc.pick("会话轮次", "Conversation turns"))
        .aria_min_numeric_value(1.)
        .aria_max_numeric_value(total as f64)
        .aria_numeric_value((index + 1) as f64)
        .aria_numeric_value_step(1.)
        .aria_value(entries[index].title.clone())
        .aria_description(loc.pick(
            "上下键预览，回车跳转，B 键切换书签",
            "Use arrow keys to preview, Enter to jump, B to toggle a bookmark",
        ))
        .cursor_pointer()
        .on_hover(move |inside, win, cx| {
            hover.update(cx, |st, _| st.hover(*inside, false));
            win.refresh();
        })
        .on_mouse_move(move |ev, win, cx| {
            moved.update(cx, |st, cx| {
                let pos = st.under_mouse(ev.position.y, count);
                st.inspect(pos, cx);
            });
            win.refresh();
        })
        .on_click(cx.listener(move |wb, ev: &ClickEvent, win, cx| {
            let i = match ev {
                ClickEvent::Keyboard(_) => index,
                _ => clicked.read(cx).under_mouse(ev.position().y, count).round() as usize,
            };
            wb.jump_to_rail_turn(i, win, cx);
            cx.stop_propagation();
        }))
        .on_key_down(cx.listener(move |wb, ev: &KeyDownEvent, win, cx| {
            let next = match ev.keystroke.key.as_str() {
                "up" => Some(index.saturating_sub(1)),
                "down" => Some((index + 1).min(total - 1)),
                "home" => Some(0),
                "end" => Some(total - 1),
                "enter" => {
                    wb.jump_to_rail_turn(index, win, cx);
                    None
                }
                "b" if !ev.keystroke.modifiers.modified() => {
                    wb.toggle_rail_bookmark(index, cx);
                    None
                }
                _ => return,
            };
            if let Some(i) = next {
                keys.update(cx, |st, cx| {
                    st.reveal(i, count);
                    st.inspect(i as f32, cx);
                });
            }
            win.prevent_default();
            cx.stop_propagation();
            win.refresh();
        }))
        .when(count < total, |track| {
            track.on_scroll(move |ev, win, cx| {
                scrolled.update(cx, |st, cx| {
                    let local = st.under_mouse(ev.position.y, count) - st.first as f32;
                    st.wheel -= f32::from(ev.delta.pixel_delta(px(PITCH)).y) / PITCH;
                    let step = st.wheel.trunc() as isize;
                    st.wheel -= step as f32;
                    st.first = st.first.saturating_add_signed(step).min(total - count);
                    st.inspect(st.first as f32 + local, cx);
                });
                win.prevent_default();
                cx.stop_propagation();
                win.refresh();
            })
        })
        .child(lines);

    for (action, forward) in [
        (AccessibleAction::Increment, true),
        (AccessibleAction::Decrement, false),
    ] {
        let reader = state.clone();
        track = track.on_a11y_action(action, move |_, win, cx| {
            let i = if forward {
                (index + 1).min(total - 1)
            } else {
                index.saturating_sub(1)
            };
            reader.update(cx, |st, cx| {
                st.focus.focus(win, cx);
                st.reveal(i, count);
                st.inspect(i as f32, cx);
            });
            win.refresh();
        });
    }
    let mut overlay = div().absolute().top_0().left_0().size(px(0.)).child(track);
    if selected.is_some() || swell > 0.02 {
        let i = selected.unwrap_or(previous);
        let entry = &entries[i];
        let card_x = inset + HIT_WIDTH + if width < 400. { 6. } else { 14. };
        let card_width = (width - card_x - 8.).min(320.);
        let lines = (((height - 46.) / 20.).floor().max(0.) as usize).min(3);
        let with_reply = lines > 0 && !entry.reply.is_empty();
        let max_height = if with_reply {
            44. + lines as f32 * 20.
        } else {
            40.
        };
        let middle = track_top + (i.saturating_sub(first) as f32 + 0.5) * PITCH;
        let card_top = (middle - max_height * 0.5).clamp(0., (height - max_height).max(0.));
        let bookmark_label = if entry.bookmarked {
            loc.pick("移除这轮的书签", "Remove turn bookmark")
        } else {
            loc.pick("为这轮添加书签", "Bookmark this turn")
        };
        let bookmark_action = div()
            .id("turn-rail-bookmark")
            .size(px(24.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .role(Role::Button)
            .track_focus(&bookmark_focus)
            .tab_stop(true)
            .aria_label(bookmark_label)
            .aria_toggled(entry.bookmarked.into())
            .focus_visible(|d| d.border_1().border_color(s.ink))
            .rounded(px(crate::ui_primitives::R_SM))
            .hover(|d| d.bg(s.hover))
            .active(|d| d.bg(s.active))
            .cursor_pointer()
            .on_click(cx.listener(move |wb, _, _, cx| {
                wb.toggle_rail_bookmark(i, cx);
                cx.stop_propagation();
            }))
            .child(bookmark(
                entry.bookmarked,
                if entry.bookmarked { s.ink } else { s.muted },
            ));
        let card = crate::ui_primitives::frosted_panel(s)
            .id("turn-rail-preview")
            .w(px(card_width))
            .min_w_0()
            .p(px(8.))
            .flex()
            .flex_col()
            .gap(px(3.))
            .hover(|d| d.border_color(s.line_strong))
            .active(|d| d.border_color(s.muted))
            .cursor_pointer()
            .on_click(cx.listener(move |wb, _, win, cx| {
                wb.jump_to_rail_turn(i, win, cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(s.font_size(FS_CONTROL))
                            .line_height(s.line_height(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(s.ink)
                            .child(entry.title.clone()),
                    )
                    .child(bookmark_action),
            )
            .when(with_reply, |card| {
                card.child(
                    div()
                        .line_clamp(lines)
                        .overflow_hidden()
                        .text_size(s.font_size(FS_CONTROL))
                        .line_height(s.line_height(20.))
                        .text_color(s.muted)
                        .child(entry.reply.clone()),
                )
            });
        let hover = state.clone();
        let bridge = div()
            .id("turn-rail-card-bridge")
            .occlude()
            .pl(px(card_x - inset - HIT_WIDTH))
            .on_hover(move |inside, win, cx| {
                hover.update(cx, |st, cx| {
                    if *inside && st.pointer.is_none() {
                        st.inspect(i as f32, cx);
                    }
                    st.hover(*inside, true);
                });
                win.refresh();
            })
            .child(card);
        overlay = overlay.child(
            div()
                .absolute()
                .left(px(inset + HIT_WIDTH))
                .top(px(card_top))
                .h(px(max_height))
                .flex()
                .items_center()
                .opacity(swell)
                .child(bridge),
        );
    }
    overlay.into_any_element()
}
