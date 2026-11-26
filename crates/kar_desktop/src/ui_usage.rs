//! Recorded token totals and turn activity by date, agent, or project.

mod plot;

use std::collections::BTreeMap;

use crate::ui_motion::selection::{Group, Item, SelectionStrip};
use chrono::{Duration, NaiveDate};
use gpui::{prelude::*, *};
use kui::components::pro_chrome::pro_spring_bar;

use crate::{
    studio_hub::{RuntimeTokenRow, UsageAnalyticsSnapshot},
    types::{ChatAccessMode, format_tokens_compact},
    ui_primitives::{
        FS_METRIC, FS_META, FS_TAG, FS_UI, LH_METRIC, LH_META, R_PILL, StudioPalette, UiLocale, card_box,
        render_provider_color_tile, render_tray_folder_artwork, settings_page_shell, tnum_features,
    },
    workbench::KarWorkbench,
};

fn demo_usage_snapshot() -> UsageAnalyticsSnapshot {
    let today = chrono::Local::now().date_naive();
    let turns_by_day: BTreeMap<_, _> = [2, 4, 1, 6, 3, 5, 2, 7, 4, 3, 8, 1, 9, 7]
        .into_iter()
        .enumerate()
        .map(|(i, n)| (today - Duration::days(13 - i as i64), n))
        .collect();
    UsageAnalyticsSnapshot {
        has_usage: true,
        total_chats: 14,
        total_turns: turns_by_day.values().sum(),
        total_input_tokens: 1_842_000,
        total_output_tokens: 268_400,
        has_cache_read: true,
        total_cached_tokens: 1_410_000,
        has_cache_write: true,
        total_cache_write_tokens: 192_000,
        has_reasoning: true,
        total_reasoning_tokens: 94_200,
        cache_hit_ratio: 76.5,
        by_runtime: vec![
            RuntimeTokenRow {
                runtime_id: "codex".into(),
                display_name: "Codex".into(),
                chat_count: 7,
                turn_count: 34,
                input_tokens: 1_060_000,
                output_tokens: 148_000,
                has_usage: true,
                has_cache_read: true,
                cached_tokens: 860_000,
                reasoning_tokens: 62_000,
                quota_summary: Some("Plus · 充足".into()),
            },
            RuntimeTokenRow {
                runtime_id: "claude".into(),
                display_name: "Claude Code".into(),
                chat_count: 5,
                turn_count: 20,
                input_tokens: 582_000,
                output_tokens: 89_400,
                has_usage: true,
                has_cache_read: true,
                cached_tokens: 430_000,
                reasoning_tokens: 24_200,
                quota_summary: Some("Max 5x".into()),
            },
            RuntimeTokenRow {
                runtime_id: "gemini".into(),
                display_name: "Gemini CLI".into(),
                chat_count: 2,
                turn_count: 8,
                input_tokens: 200_000,
                output_tokens: 31_000,
                has_usage: true,
                has_cache_read: true,
                cached_tokens: 120_000,
                reasoning_tokens: 8_000,
                quota_summary: None,
            },
        ],
        turns_by_day,
        ..Default::default()
    }
}

impl KarWorkbench {
    pub(crate) fn render_usage_panel(
        &self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let is_demo_fallback = self.usage_analytics.total_chats == 0
            && !self.usage_analytics.has_usage
            && matches!(self.access_mode, ChatAccessMode::DemoShowcase);
        let fallback_snap;
        let snap = if is_demo_fallback {
            fallback_snap = demo_usage_snapshot();
            &fallback_snap
        } else {
            &self.usage_analytics
        };

        let tokens = |count: u64| format_tokens_compact(count.min(i64::MAX as u64) as i64);
        let total_combined = snap
            .total_input_tokens
            .saturating_add(snap.total_output_tokens);
        let hero_card = card_box(s)
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div().p(px(16.)).flex().flex_col().gap(px(10.)).child(
                    div()
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .text_size(px(FS_META))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(s.muted)
                                                .child(loc.pick(
                                                    "累计词元",
                                                    "All-time tokens (input + output)",
                                                )),
                                        )
                                        .when(is_demo_fallback, |r| {
                                            r.child(
                                                div()
                                                    .px(px(6.))
                                                    .py(px(1.))
                                                    .rounded(px(R_PILL))
                                                    .bg(s.pro.amber_soft)
                                                    .text_size(px(FS_TAG))
                                                    .text_color(s.pro.amber_ink)
                                                    .child(loc.pick("演示数据", "Demo data")),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_baseline()
                                        .gap(px(10.))
                                        .child(
                                            div()
                                                .font_features(tnum_features())
                                                .text_size(px(FS_METRIC))
                                                .line_height(px(LH_METRIC))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(s.ink)
                                                .child(if snap.has_usage {
                                                    tokens(total_combined)
                                                } else {
                                                    "—".into()
                                                }),
                                        )
                                        .when(
                                            snap.has_cache_read && snap.total_input_tokens > 0,
                                            |r| {
                                                r.child(
                                                    div()
                                                        .px(px(7.))
                                                        .py(px(2.))
                                                        .rounded(px(R_PILL))
                                                        .bg(s.pro.green_soft)
                                                        .font_features(tnum_features())
                                                        .text_size(px(FS_TAG))
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .text_color(s.pro.green_ink)
                                                        .child(format!(
                                                            "{} {:.1}%",
                                                            loc.pick("缓存命中", "Cache hit"),
                                                            snap.cache_hit_ratio
                                                        )),
                                                )
                                            },
                                        ),
                                ),
                        ),
                ),
            )
            .child(
                div()
                    .px(px(16.))
                    .py(px(11.))
                    .bg(s.tray.opacity(if s.dark { 0.45 } else { 0.55 }))
                    .border_t_1()
                    .border_color(s.line_soft)
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .flex_wrap()
                    .child(usage_footer_cell(
                        loc.pick("输入词元", "Input tokens"),
                        if snap.has_usage {
                            tokens(snap.total_input_tokens)
                        } else {
                            "—".into()
                        },
                        s.pro.blue,
                        s,
                    ))
                    .child(usage_footer_cell(
                        loc.pick("输出词元", "Output tokens"),
                        if snap.has_usage {
                            tokens(snap.total_output_tokens)
                        } else {
                            "—".into()
                        },
                        s.pro.teal,
                        s,
                    ))
                    .child(usage_footer_cell(
                        loc.pick("缓存读取", "Cached input"),
                        if snap.has_cache_read {
                            tokens(snap.total_cached_tokens)
                        } else {
                            "—".into()
                        },
                        s.pro.green,
                        s,
                    ))
                    .child(usage_footer_cell(
                        loc.pick("对话记录", "Conversations"),
                        format!(
                            "{} {} · {} {}",
                            snap.total_chats,
                            loc.pick("会话", "chats"),
                            snap.total_turns,
                            loc.pick("轮", "turns")
                        ),
                        s.pro.purple,
                        s,
                    )),
            );

        let split_toggle = SelectionStrip::options(
            "usage-breakdown",
            loc.pick("消耗去向", "Usage breakdown"),
            Group::Tabs,
            [
                (false, loc.pick("按智能体", "By agent")),
                (true, loc.pick("按项目", "By project")),
            ],
            self.usage_by_project,
            s,
            cx,
            |wb, value, _, cx| {
                wb.usage_by_project = value;
                cx.notify();
            },
        );

        let mut breakdown_card = card_box(s)
            .p(px(15.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(FS_UI))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(s.ink)
                            .child(loc.pick("用量分布", "Usage breakdown")),
                    )
                    .child(split_toggle),
            );

        let tones = [
            s.pro.blue,
            s.pro.purple,
            s.pro.teal,
            s.pro.orange,
            s.pro.green,
        ];

        if !self.usage_by_project {
            if snap.by_runtime.is_empty() {
                breakdown_card = breakdown_card.child(
                    div()
                        .py(px(14.))
                        .text_size(px(FS_META))
                        .text_color(s.muted)
                        .child(loc.pick(
                            "智能体上报词元后，可在这里查看用量。",
                            "Usage appears here when an agent reports it.",
                        )),
                );
            } else {
                let max_tok = snap
                    .by_runtime
                    .iter()
                    .map(|r| r.input_tokens.saturating_add(r.output_tokens))
                    .max()
                    .unwrap_or(1)
                    .max(1);
                for (ix, row) in snap.by_runtime.iter().enumerate() {
                    let sum = row.input_tokens.saturating_add(row.output_tokens);
                    let ratio = sum as f32 / max_tok as f32;
                    let token_summary = if !row.has_usage {
                        loc.pick("词元未上报", "Tokens not reported").to_string()
                    } else if row.has_cache_read {
                        format!(
                            "{} · {} {}",
                            tokens(sum),
                            loc.pick("缓存", "cached"),
                            tokens(row.cached_tokens)
                        )
                    } else {
                        tokens(sum)
                    };
                    let bar_color = tones[ix % tones.len()];
                    breakdown_card = breakdown_card.child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .flex_wrap()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex()
                                            .flex_wrap()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(render_provider_color_tile(
                                                &row.runtime_id,
                                                22.,
                                                s,
                                                cx,
                                            ))
                                            .child(
                                                div()
                                                    .text_size(px(FS_META))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(s.ink)
                                                    .child(row.display_name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(FS_TAG))
                                                    .text_color(s.muted)
                                                    .child(format!(
                                                        "{} {} · {} {}",
                                                        row.chat_count,
                                                        loc.pick("会话", "chats"),
                                                        row.turn_count,
                                                        loc.pick("轮", "turns")
                                                    )),
                                            )
                                            .when_some(row.quota_summary.clone(), |r, q| {
                                                r.child(
                                                    div()
                                                        .px(px(6.))
                                                        .py(px(1.))
                                                        .rounded(px(R_PILL))
                                                        .bg(s.pro.accent_soft)
                                                        .text_size(px(FS_TAG))
                                                        .text_color(s.pro.accent_info)
                                                        .child(q),
                                                )
                                            }),
                                    )
                                    .child(
                                        div()
                                            .font_features(tnum_features())
                                            .text_size(px(FS_META))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(if row.has_usage { s.ink } else { s.muted })
                                            .child(token_summary),
                                    ),
                            )
                            .when(row.has_usage, |col| {
                                col.child(pro_spring_bar(
                                    SharedString::from(format!("usage-rt-bar-{}", row.runtime_id)),
                                    ratio,
                                    6.,
                                    99.,
                                    s.tray,
                                    bar_color,
                                ))
                            }),
                    );
                }
            }
        } else {
            let mut proj_rows: Vec<(String, usize, usize)> = self
                .project_registry
                .projects
                .iter()
                .map(|p| {
                    let mut chats = 0usize;
                    let mut turns = 0usize;
                    for c in &self.chat_list {
                        if c.info.workspace == p.root_path {
                            chats += 1;
                            turns += snap.turns_by_chat.get(&c.info.id).copied().unwrap_or(0);
                        }
                    }
                    (
                        crate::studio_hub::project_display_name(&p.name).to_owned(),
                        chats,
                        turns,
                    )
                })
                .collect();

            if is_demo_fallback && proj_rows.iter().all(|(_, c, _)| *c == 0) {
                proj_rows = vec![
                    ("Kwork".into(), 8, 38),
                    ("kar-frontend".into(), 4, 17),
                    ("kantor-docs".into(), 2, 7),
                ];
            }
            let max_turns = proj_rows
                .iter()
                .map(|(_, _, t)| *t)
                .max()
                .unwrap_or(1)
                .max(1);

            for (ix, (pname, chats, turns)) in proj_rows.into_iter().enumerate() {
                let ratio = if turns == 0 {
                    0.0
                } else {
                    turns as f32 / max_turns as f32
                };
                let bar_color = tones[ix % tones.len()];
                breakdown_card = breakdown_card.child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(render_tray_folder_artwork(18.))
                                        .child(
                                            div()
                                                .text_size(px(FS_META))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(s.ink)
                                                .child(pname.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .font_features(tnum_features())
                                        .text_size(px(FS_META))
                                        .text_color(s.muted)
                                        .child(format!(
                                            "{chats} {} · {turns} {}",
                                            loc.pick("会话", "chats"),
                                            loc.pick("轮", "turns")
                                        )),
                                ),
                        )
                        .child(pro_spring_bar(
                            SharedString::from(format!("usage-proj-bar-{ix}")),
                            ratio,
                            6.,
                            99.,
                            s.tray,
                            bar_color,
                        )),
                );
            }
        }

        let activity = self.conversation_activity(snap, is_demo_fallback, s, window, cx);
        let body = settings_page_shell()
            .child(activity)
            .child(hero_card)
            .child(breakdown_card);

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("kar-usage-analytics-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
            .into_any_element()
    }

    fn conversation_activity(
        &self,
        snap: &UsageAnalyticsSnapshot,
        demo: bool,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let days = self.usage_period_days;
        let today = chrono::Local::now().date_naive();
        let compact_labels = loc == UiLocale::En && window.viewport_size().width < px(640.);
        let options = [
            (7, loc.pick("7 天", "7 days"), "7d"),
            (28, loc.pick("28 天", "28 days"), "28d"),
            (30, loc.pick("30 天", "30 days"), "30d"),
            (0, loc.pick("全部", "All time"), "All time"),
        ]
        .into_iter()
        .map(|(period, label, short)| {
            let item = Item::new(
                period.to_string(),
                label,
                period == days,
                cx,
                move |wb, _, cx| {
                    wb.usage_period_days = period;
                    cx.notify();
                },
            );
            if compact_labels {
                item.content(short)
            } else {
                item
            }
        })
        .collect();
        let periods = SelectionStrip::new(
            "usage-period",
            loc.pick("活动周期", "Activity period"),
            Group::Choices,
            options,
            s,
        );
        let card = card_box(s)
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(16.))
            .p(px(16.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(FS_UI))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(s.ink)
                            .child(loc.pick("对话活动", "Conversation activity"))
                            .when(demo, |row| {
                                row.child(
                                    div()
                                        .text_size(px(FS_META))
                                        .font_weight(FontWeight::NORMAL)
                                        .text_color(s.muted)
                                        .child(loc.pick("演示", "Demo")),
                                )
                            }),
                    )
                    .child(periods),
            );
        if !demo && snap.daily_activity.is_empty() && snap.turns_by_day.is_empty() {
            return card.child(activity_notice(
                loc.pick("正在读取对话记录…", "Reading conversations…"),
                s,
            ));
        }
        let series = ActivitySeries::new(&snap.turns_by_day, today, days);
        if series.active_days == 0 {
            return card.child(activity_notice(
                loc.pick("这段时间没有对话记录", "No conversations in this period"),
                s,
            ));
        }
        let active_days = match (loc, series.active_days) {
            (UiLocale::Zh, n) => format!("活跃 {n} 天"),
            (UiLocale::En, 1) => "1 active day".to_string(),
            (UiLocale::En, n) => format!("{n} active days"),
        };
        let metric =
            div()
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(5.))
                        .child(
                            div()
                                .text_size(px(FS_METRIC))
                                .line_height(px(LH_METRIC))
                                .font_weight(FontWeight::SEMIBOLD)
                                .font_features(tnum_features())
                                .text_color(s.ink)
                                .child(series.total.to_string()),
                        )
                        .child(div().text_size(px(FS_META)).text_color(s.muted).child(
                            loc.pick("轮", if series.total == 1 { "turn" } else { "turns" }),
                        )),
                )
                .child(
                    div()
                        .text_size(px(FS_META))
                        .line_height(px(LH_META))
                        .text_color(s.muted)
                        .child(active_days),
                );
        let chart = plot::activity_plot(&series.buckets, today, days, loc, s, window, cx);
        card.child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_x(px(28.))
                .gap_y(px(8.))
                .child(metric)
                .child(div().flex_1().flex_basis(px(210.)).min_w_0().child(chart)),
        )
    }
}

fn usage_footer_cell(label: &'static str, value: String, dot_col: Hsla, s: &StudioPalette) -> Div {
    div()
        .flex_1()
        .min_w(px(110.))
        .flex()
        .flex_col()
        .gap(px(3.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .child(div().size(px(6.)).rounded_full().bg(dot_col))
                .child(
                    div()
                        .text_size(px(FS_META))
                        .text_color(s.muted)
                        .truncate()
                        .child(label),
                ),
        )
        .child(
            div()
                .font_features(tnum_features())
                .text_size(px(FS_UI))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(s.ink)
                .truncate()
                .child(value),
        )
}

fn activity_notice(label: &'static str, s: &StudioPalette) -> Div {
    div()
        .py(px(12.))
        .text_size(px(FS_UI))
        .text_color(s.muted)
        .child(label)
}

struct ActivityBucket {
    first: NaiveDate,
    last: NaiveDate,
    turns: usize,
}

struct ActivitySeries {
    buckets: Vec<ActivityBucket>,
    active_days: usize,
    total: usize,
}

impl ActivitySeries {
    fn new(counts: &BTreeMap<NaiveDate, usize>, today: NaiveDate, days: u32) -> Self {
        let first = if days == 0 {
            counts.keys().next().copied().unwrap_or(today).min(today)
        } else {
            today - Duration::days(i64::from(days - 1))
        };
        let span = (today - first).num_days() + 1;
        let n = span.min(60);
        let mut total: usize = 0;
        let mut active_days = 0;
        let buckets = (0..n)
            .map(|i| {
                let start = first + Duration::days(i * span / n);
                let end = first + Duration::days((i + 1) * span / n - 1);
                let mut turns: usize = 0;
                for (_, &count) in counts.range(start..=end) {
                    turns = turns.saturating_add(count);
                    active_days += usize::from(count > 0);
                }
                total = total.saturating_add(turns);
                ActivityBucket {
                    first: start,
                    last: end,
                    turns,
                }
            })
            .collect();
        Self {
            buckets,
            active_days,
            total,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ActivitySeries, BTreeMap, Duration, NaiveDate};

    #[test]
    fn activity_ranges_cover_each_calendar_day_once() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
        let counts = [
            (0, 1),
            (6, 4),
            (7, 3),
            (27, 2),
            (29, 7),
            (30, 11),
            (500, 9),
            (-1, 99),
        ]
        .map(|(d, c)| (today - Duration::days(d), c))
        .into_iter()
        .collect();
        for (range, total, active_days, len) in [
            (7, 5, 2, 7),
            (28, 10, 4, 28),
            (30, 17, 5, 30),
            (0, 37, 7, 60),
        ] {
            let series = ActivitySeries::new(&counts, today, range);
            assert_eq!(
                (series.total, series.active_days, series.buckets.len()),
                (total, active_days, len)
            );
            assert_eq!(series.buckets.last().unwrap().last, today);
            for pair in series.buckets.windows(2) {
                assert_eq!(pair[0].last.succ_opt(), Some(pair[1].first));
            }
            for bucket in series.buckets {
                assert_eq!(
                    bucket.turns,
                    counts
                        .range(bucket.first..=bucket.last)
                        .map(|(_, n)| *n)
                        .sum::<usize>()
                );
            }
        }
        let empty = ActivitySeries::new(&BTreeMap::new(), today, 0);
        assert_eq!(
            (empty.total, empty.active_days, empty.buckets.len()),
            (0, 0, 1)
        );
    }
}
