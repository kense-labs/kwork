//! A compact capacity inspector for the counts a Kar runtime actually reports.
use gpui::{prelude::*, *};
use kar::{CompactionStatus, ContextUsage, ManualCompactionReport};

use crate::ui_primitives::{FS_CONTROL, FS_META, LH_CONTROL, LH_META, StudioPalette, UiLocale, tnum_features};

pub(super) fn fraction(usage: &ContextUsage) -> Option<f32> {
    let percent = usage.percent.filter(|value| value.is_finite());
    let ratio = percent.map(|value| value / 100.).or_else(|| {
        let limit = usage.context_window.filter(|limit| *limit > 0)?;
        Some(usage.tokens? as f64 / limit as f64)
    })?;
    Some(ratio.clamp(0., 1.) as f32)
}

fn count_row(label: &'static str, value: u64, s: &StudioPalette) -> Div {
    let mut number = value.to_string();
    let mut group_end = number.len();
    while group_end > 3 {
        group_end -= 3;
        number.insert(group_end, ',');
    }
    detail_row(label, number, s)
}

pub(super) fn detail_row(label: &'static str, value: String, s: &StudioPalette) -> Div {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .text_size(s.font_size(FS_CONTROL))
        .line_height(s.line_height(LH_CONTROL))
        .child(div().flex_none().text_color(s.muted).child(label))
        .child(div().min_w_0().truncate().font_features(tnum_features()).text_color(s.ink).child(value))
}

pub(super) fn inspector(
    usage: Option<&ContextUsage>,
    compaction: Option<&ManualCompactionReport>,
    loc: UiLocale,
    s: &StudioPalette,
) -> Div {
    let ratio = usage.and_then(fraction);
    let used = usage.and_then(|u| u.tokens);
    let limit = usage.and_then(|u| u.context_window).filter(|n| *n > 0);
    let heading = loc.pick("上下文用量", "Context window");
    let mut panel = div()
        .min_w_0()
        .w_full()
        .px(px(8.))
        .py(px(8.))
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .text_size(s.font_size(FS_CONTROL))
                .line_height(s.line_height(LH_CONTROL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(s.text)
                .child(heading)
                .when_some(ratio, |row, r| {
                    row.child(
                        div()
                            .text_size(s.font_size(FS_CONTROL))
                            .line_height(s.line_height(LH_CONTROL))
                            .font_features(tnum_features())
                            .text_color(s.ink)
                            .child(format!("{:.0}%", r * 100.)),
                    )
                }),
        );
    if let Some(ratio) = ratio {
        let ink = if ratio >= 0.9 {
            s.pro.amber_ink
        } else {
            s.text
        };
        let meter = div()
            .id("context-capacity-meter")
            .role(Role::Meter)
            .aria_label(heading)
            .aria_min_numeric_value(0.)
            .aria_max_numeric_value(100.)
            .aria_numeric_value((ratio * 100.) as f64)
            .w_full()
            .h(px(4.))
            .mb(px(4.))
            .rounded_full()
            .overflow_hidden()
            .bg(s.line_soft)
            .child(div().h_full().w(relative(ratio)).rounded_full().bg(ink));
        panel = panel.child(meter);
    }
    panel
        .when_some(used, |p, n| {
            p.child(count_row(loc.pick("已用词元", "Tokens used"), n, s))
        })
        .when_some(limit, |p, n| {
            p.child(count_row(loc.pick("窗口容量", "Token capacity"), n, s))
        })
        .when(ratio.is_none() && used.is_none() && limit.is_none(), |p| {
            p.child(
                div()
                    .text_size(s.font_size(FS_META))
                    .line_height(s.line_height(LH_META))
                    .text_color(s.muted)
                    .child(loc.pick("智能体尚未上报用量", "The agent hasn’t reported usage yet")),
            )
        })
        .when_some(compaction, |p, report| {
            let status = match &report.status {
                CompactionStatus::Dispatched => loc.pick("已请求压缩", "Compaction requested"),
                CompactionStatus::Accepted => loc.pick("压缩中", "Compacting"),
                CompactionStatus::Completed { .. } => {
                    loc.pick("上次压缩已完成", "Last compaction completed")
                }
                CompactionStatus::Aborted { .. } => {
                    loc.pick("上次压缩已取消", "Last compaction canceled")
                }
                CompactionStatus::Rejected { .. } => {
                    loc.pick("上次压缩请求被拒绝", "Last compaction declined")
                }
                CompactionStatus::Failed { .. } => {
                    loc.pick("上次压缩失败", "Last compaction failed")
                }
                CompactionStatus::Unconfirmed { .. } => {
                    loc.pick("上次压缩结果未确认", "Last compaction unconfirmed")
                }
            };
            p.child(
                div()
                    .text_size(s.font_size(FS_META))
                    .text_color(s.muted)
                    .child(status),
            )
        })
}
