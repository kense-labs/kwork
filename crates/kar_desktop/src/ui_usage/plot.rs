//! Native, inspectable bars for discrete conversation counts.
use chrono::{Datelike, NaiveDate};
use gpui::{prelude::*, *};

use super::ActivityBucket;
use crate::{
    ui_primitives::{FS_META, LH_META, StudioPalette, UiLocale, tnum_features},
    workbench::KarWorkbench,
};

struct Inspection {
    focus: FocusHandle,
    hovered: Option<usize>,
    cursor: usize,
}

fn date_label(date: NaiveDate, loc: UiLocale, year: bool) -> String {
    match (loc, year) {
        (UiLocale::Zh, true) => date.format("%Y年%-m月%-d日").to_string(),
        (UiLocale::Zh, false) => date.format("%-m月%-d日").to_string(),
        (UiLocale::En, true) => date.format("%b %-d, %Y").to_string(),
        (UiLocale::En, false) => date.format("%b %-d").to_string(),
    }
}

fn interval_label(bucket: &ActivityBucket, today: NaiveDate, loc: UiLocale) -> String {
    let show_year =
        bucket.first.year() != today.year() || bucket.first.year() != bucket.last.year();
    let first = date_label(bucket.first, loc, show_year);
    if bucket.first == bucket.last {
        first
    } else {
        format!("{first} – {}", date_label(bucket.last, loc, show_year))
    }
}

fn turn_label(turns: usize, loc: UiLocale) -> String {
    match loc {
        UiLocale::Zh => format!("{turns} 轮"),
        UiLocale::En if turns == 1 => "1 turn".into(),
        UiLocale::En => format!("{turns} turns"),
    }
}

pub(super) fn activity_plot(
    buckets: &[ActivityBucket],
    today: NaiveDate,
    days: u32,
    loc: UiLocale,
    s: &StudioPalette,
    window: &mut Window,
    cx: &mut Context<KarWorkbench>,
) -> AnyElement {
    let (Some(first), Some(last)) = (buckets.first(), buckets.last()) else {
        return div().into_any_element();
    };
    let end = buckets.len() - 1;
    let inspection =
        window.use_keyed_state(("usage-activity-inspection", days as usize), cx, |_, cx| {
            Inspection {
                focus: cx.focus_handle().tab_stop(true),
                hovered: None,
                cursor: end,
            }
        });
    let state = inspection.read(cx);
    let focus = state.focus.clone();
    let active = state
        .hovered
        .or_else(|| focus.is_focused(window).then_some(state.cursor))
        .map(|i| i.min(end));
    let read_index = active.unwrap_or(end);
    let active_bucket = &buckets[read_index];
    let date = interval_label(active_bucket, today, loc);
    let count = turn_label(active_bucket.turns, loc);
    let hot = s
        .pro
        .teal
        .blend(s.ink.opacity(if s.dark { 0.20 } else { 0.38 }));
    let ink = s
        .pro
        .teal
        .blend(s.ink.opacity(if s.dark { 0.0 } else { 0.16 }));
    let rest = s.ink.opacity(if s.dark { 0.26 } else { 0.19 });
    let scale = buckets.iter().map(|b| b.turns).max().unwrap_or(0).max(1) as f32;

    let keys = inspection.clone();
    let pointer = inspection.clone();
    let mut bars = div()
        .id("usage-activity-plot")
        .track_focus(&focus)
        .tab_stop(true)
        .role(Role::Slider)
        .aria_label(loc.pick("逐日查看对话轮数", "Explore conversation activity"))
        .aria_min_numeric_value(1.)
        .aria_max_numeric_value((end + 1) as f64)
        .aria_numeric_value((read_index + 1) as f64)
        .aria_numeric_value_step(1.)
        .aria_value(format!("{date} · {count}"))
        .w_full()
        .h(px(54.))
        .flex()
        .rounded(px(4.))
        .border_1()
        .border_color(transparent_black())
        .focus_visible(move |style| style.border_color(hot))
        .on_hover(move |inside, win, cx| {
            if !*inside && pointer.read(cx).hovered.is_some() {
                pointer.update(cx, |state, _| state.hovered = None);
                win.refresh();
            }
        })
        .on_key_down(move |ev, win, cx| {
            let state = keys.read(cx);
            let index = state.hovered.unwrap_or(state.cursor).min(end);
            let next = match ev.keystroke.key.as_str() {
                "right" | "up" => (index + 1).min(end),
                "left" | "down" => index.saturating_sub(1),
                "home" => 0,
                "end" => end,
                _ => return,
            };
            keys.update(cx, |state, _| {
                state.hovered = None;
                state.cursor = next;
            });
            win.prevent_default();
            cx.stop_propagation();
            win.refresh();
        });
    for (action, advance) in [
        (AccessibleAction::Increment, true),
        (AccessibleAction::Decrement, false),
    ] {
        let input = inspection.clone();
        bars = bars.on_a11y_action(action, move |_, win, cx| {
            input.update(cx, |state, cx| {
                let i = state.hovered.take().unwrap_or(state.cursor).min(end);
                state.cursor = if advance {
                    (i + 1).min(end)
                } else {
                    i.saturating_sub(1)
                };
                state.focus.focus(win, cx);
            });
            win.refresh();
        });
    }

    for (i, bucket) in buckets.iter().enumerate() {
        let selected = Some(i) == active;
        let hovered = inspection.clone();
        let pressed = inspection.clone();
        // The full column is the hit area. Only the data mark has a maximum width.
        let column = div()
            .id(("usage-activity-bin", i))
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .justify_center()
            .cursor_pointer()
            .on_mouse_move(move |_, win, cx| {
                if hovered.read(cx).hovered != Some(i) {
                    hovered.update(cx, |state, _| state.hovered = Some(i));
                    win.refresh();
                }
            })
            .on_mouse_down(MouseButton::Left, move |_, win, cx| {
                pressed.update(cx, |state, cx| {
                    state.cursor = i;
                    state.hovered = Some(i);
                    state.focus.focus(win, cx);
                });
                win.prevent_default();
                win.refresh();
            })
            .child(
                div()
                    .h_full()
                    .w_full()
                    .max_w(px(16.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_end()
                    .when(selected, |d| d.rounded(px(3.)).bg(ink.opacity(0.08)))
                    .child(if bucket.turns == 0 {
                        div()
                            .size(px(2.5))
                            .rounded_full()
                            .bg(if selected { hot } else { rest })
                    } else {
                        div()
                            .w(relative(0.64))
                            .max_w(px(10.))
                            .h(px((bucket.turns as f32 / scale * 46.).max(1.)))
                            .rounded_t(px(2.))
                            .bg(if selected { hot } else { ink })
                    }),
            );
        bars = bars.child(column);
    }

    let (left, right) = if active.is_some() {
        (date, count)
    } else {
        let first_day = date_label(first.first, loc, first.first.year() != today.year());
        let last_day = if last.last == today {
            loc.pick("今天", "Today").to_string()
        } else {
            date_label(last.last, loc, last.last.year() != today.year())
        };
        if first.first == last.last {
            (last_day, String::new())
        } else {
            (first_day, last_day)
        }
    };
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(5.))
        .child(bars)
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .when(right.is_empty(), |d| d.justify_center())
                .gap(px(6.))
                .text_size(px(FS_META))
                .line_height(px(LH_META))
                .font_features(tnum_features())
                .text_color(if active.is_some() { s.text } else { s.muted })
                .child(div().min_w_0().truncate().child(left))
                .when(!right.is_empty(), |d| {
                    d.child(div().flex_none().child(right))
                }),
        )
        .into_any_element()
}
