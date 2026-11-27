use chrono::{DateTime, Local};
use gpui::{prelude::*, *};
use gpui_kit::component::tooltip::Tooltip;

use crate::ui_primitives::{FS_META, LH_META, StudioPalette, tnum_features};

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
