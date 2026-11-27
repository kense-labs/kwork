use chrono::{DateTime, Local};
use gpui::{prelude::*, *};
use gpui_kit::component::tooltip::Tooltip;

use crate::ui_primitives::{FS_META, LH_META, StudioPalette, tnum_features};
use std::time::Duration;
use uuid::Uuid;

struct TurnClock {
    started_ms: u64,
    _tick: Task<()>,
}

impl Render for TurnClock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let secs = crate::remote_link::now_epoch_ms().saturating_sub(self.started_ms) / 1000;
        let palette = StudioPalette::get(cx);
        div()
            .flex_none()
            .font_features(tnum_features())
            .text_size(palette.font_size(FS_META))
            .text_color(palette.muted)
            .child(format!("{}:{:02}", secs / 60, secs % 60))
    }
}

pub(super) fn live_clock(
    turn: Uuid,
    started_ms: u64,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let clock = window.use_keyed_state(
        SharedString::from(format!("live-clock-{turn}-{started_ms}")),
        cx,
        |_, cx| {
            cx.new(|cx: &mut Context<TurnClock>| {
                let tick = cx.spawn(async move |clock, cx| {
                    loop {
                        let elapsed = crate::remote_link::now_epoch_ms().saturating_sub(started_ms);
                        cx.background_executor()
                            .timer(Duration::from_millis(1000 - elapsed % 1000))
                            .await;
                        if clock.update(cx, |_, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                });
                TurnClock {
                    started_ms,
                    _tick: tick,
                }
            })
        },
    );
    clock.read(cx).clone().into_any_element()
}

pub(super) struct MessageTime {
    pub(super) short: String,
    pub(super) full: String,
}

pub(super) fn local(ms: u64) -> Option<MessageTime> {
    if ms == 0 {
        return None;
    }
    let date = DateTime::from_timestamp_millis(i64::try_from(ms).ok()?)?.with_timezone(&Local);
    Some(MessageTime {
        short: date.format("%H:%M").to_string(),
        full: date.format("%Y-%m-%d %H:%M:%S %:z").to_string(),
    })
}

pub(super) fn label(
    id: impl Into<ElementId>,
    time: MessageTime,
    s: &StudioPalette,
    detail: Option<String>,
) -> Stateful<Div> {
    let hint: SharedString = match detail {
        Some(detail) => format!("{}\n{detail}", time.full).into(),
        None => time.full.into(),
    };
    div()
        .id(id)
        .flex_none()
        .text_size(s.font_size(FS_META))
        .line_height(s.line_height(LH_META))
        .font_features(tnum_features())
        .text_color(s.muted)
        .aria_description(hint.clone())
        .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
        .child(time.short)
}
