//! KUI `agent_usage.rs` Application Screen:
//! `28px` Hero Cumulative Spend Card, Smooth GPUI `canvas` Trend Curve, 4-Cell KPI Footer,
//! and `"消耗去向拆解"` (`Models` vs `Projects`) Breakdown with `pro_pill_indicator` & `pro_spring_bar`.

use gpui::{prelude::*, *};
use kui::components::pro_chrome::{
    pro_mark_selected, pro_pill_indicator, pro_selection_sensor, pro_spring_bar,
};

use crate::{
    studio_hub::{RuntimeTokenRow, UsageAnalyticsSnapshot},
    types::{ChatAccessMode, format_tokens_compact},
    ui_motion::Tactile,
    ui_primitives::{
        FS_META, FS_TAG, FS_UI, R_CARD, R_CTRL, R_PILL, StudioPalette,
        render_provider_color_tile, render_tray_folder_artwork, tnum_features,
    },
    workbench::KarWorkbench,
};

fn catmull_rom_smooth(pts: &[(f32, f32)], steps_per_seg: usize) -> Vec<(f32, f32)> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let mut out = Vec::with_capacity(pts.len() * steps_per_seg + 1);
    for i in 0..pts.len() - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = pts[(i + 2).min(pts.len() - 1)];
        for step in 0..steps_per_seg {
            let t = step as f32 / steps_per_seg as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            let x = 0.5
                * ((2.0 * p1.0)
                    + (-p0.0 + p2.0) * t
                    + (2.0 * p0.0 - 5.0 * p1.0 + 4.0 * p2.0 - p3.0) * t2
                    + (-p0.0 + 3.0 * p1.0 - 3.0 * p2.0 + p3.0) * t3);
            let y = 0.5
                * ((2.0 * p1.1)
                    + (-p0.1 + p2.1) * t
                    + (2.0 * p0.1 - 5.0 * p1.1 + 4.0 * p2.1 - p3.1) * t2
                    + (-p0.1 + 3.0 * p1.1 - 3.0 * p2.1 + p3.1) * t3);
            out.push((x, y.clamp(0.0, 1.0)));
        }
    }
    if let Some(&last) = pts.last() {
        out.push((last.0, last.1.clamp(0.0, 1.0)));
    }
    out
}

fn demo_usage_snapshot() -> UsageAnalyticsSnapshot {
    UsageAnalyticsSnapshot {
        has_usage: true,
        total_chats: 14,
        total_turns: 62,
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
                cached_tokens: 120_000,
                reasoning_tokens: 8_000,
                quota_summary: None,
            },
        ],
        daily_activity: vec![
            ("D-13".into(), 2.),
            ("D-12".into(), 4.),
            ("D-11".into(), 3.),
            ("D-10".into(), 6.),
            ("D-9".into(), 5.),
            ("D-8".into(), 8.),
            ("D-7".into(), 4.),
            ("D-6".into(), 7.),
            ("D-5".into(), 9.),
            ("D-4".into(), 6.),
            ("D-3".into(), 11.),
            ("D-2".into(), 8.),
            ("D-1".into(), 12.),
            ("Today".into(), 10.),
        ],
    }
}

impl KarWorkbench {
    pub(crate) fn render_usage_panel(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let is_demo_fallback = !self.usage_analytics.has_usage
            && matches!(self.access_mode, ChatAccessMode::DemoShowcase);
        let fallback_snap;
        let snap = if is_demo_fallback {
            fallback_snap = demo_usage_snapshot();
            &fallback_snap
        } else {
            &self.usage_analytics
        };

        // ── 1. Period Selector Capsules (`近 7 天 / 近 30 天 / 全部`) ─────────
        let period_sensor = pro_selection_sensor();
        let period_key = SharedString::from(format!("usage-p-{}", self.usage_period_days));
        let mut period_bar = div()
            .relative()
            .p(px(2.5))
            .rounded(px(R_CTRL + 2.))
            .bg(s.tray)
            .border_1()
            .border_color(s.line_soft)
            .flex()
            .items_center()
            .gap(px(2.))
            .child(pro_pill_indicator(
                "kar-usage-period-pill",
                period_key,
                period_sensor.clone(),
                R_CTRL,
                s.card,
                Some(s.line),
                false,
            ));

        for (ix, (days, label)) in [(7u32, "近 7 天"), (30, "近 30 天"), (365, "全部记录")]
            .into_iter()
            .enumerate()
        {
            let active = self.usage_period_days == days;
            period_bar = period_bar.child(
                div()
                    .relative()
                    .rounded(px(R_CTRL))
                    .when(active, |d| d.child(pro_mark_selected(&period_sensor)))
                    .child(
                        Tactile::ghost(("usage-period-opt", ix), s)
                            .fills(transparent_black(), s.hover)
                            .h(px(25.))
                            .px(px(10.))
                            .text_size(px(FS_META))
                            .text_color(if active { s.ink } else { s.muted })
                            .child(label)
                            .on_press(cx, move |this, _, cx| {
                                this.usage_period_days = days;
                                cx.notify();
                            }),
                    ),
            );
        }

        // ── 2. KUI `agent_usage.rs` Hero Cumulative Card + Smooth Canvas Curve ─
        let total_combined = (snap.total_input_tokens + snap.total_output_tokens) as i64;
        let window_len = match self.usage_period_days {
            7 => 7usize,
            30 => 14,
            _ => snap.daily_activity.len().max(10),
        };
        let slice_start = snap.daily_activity.len().saturating_sub(window_len);
        let raw_series: Vec<f32> = if snap.daily_activity.is_empty() {
            vec![0.; 10]
        } else {
            snap.daily_activity[slice_start..]
                .iter()
                .map(|(_, v)| *v as f32)
                .collect()
        };
        let max_y = raw_series
            .iter()
            .copied()
            .fold(0.0_f32, f32::max)
            .max(1.0);
        let norm_pts: Vec<(f32, f32)> = if raw_series.len() <= 1 {
            vec![(0.0, 0.15), (1.0, 0.15)]
        } else {
            let denom = (raw_series.len() - 1) as f32;
            raw_series
                .iter()
                .enumerate()
                .map(|(i, v)| (i as f32 / denom, (*v / max_y).clamp(0.04, 0.92)))
                .collect()
        };
        let smooth_pts = catmull_rom_smooth(&norm_pts, 6);
        let stroke_col = s.pro.accent_info;
        let fill_col = s.pro.accent_info.opacity(if s.dark { 0.14 } else { 0.10 });
        let grid_col = s.line_soft;

        let trend_canvas = div().w_full().h(px(92.)).py(px(4.)).child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let bw = f32::from(bounds.size.width);
                    let bh = f32::from(bounds.size.height);
                    if bw <= 12. || bh <= 12. || smooth_pts.len() < 2 {
                        return;
                    }
                    let pad_top = 8.0_f32;
                    let pad_bot = 6.0_f32;
                    let usable_h = (bh - pad_top - pad_bot).max(4.);
                    let px_pt = |(nx, ny): (f32, f32)| {
                        point(
                            bounds.left() + px(nx * bw),
                            bounds.top() + px(pad_top + (1.0 - ny) * usable_h),
                        )
                    };
                    let base_y = bounds.bottom() - px(pad_bot);

                    for frac in [0.35_f32, 0.75] {
                        let gy = bounds.top() + px(pad_top + frac * usable_h);
                        let mut gl = PathBuilder::stroke(px(1.));
                        gl.move_to(point(bounds.left(), gy));
                        gl.line_to(point(bounds.right(), gy));
                        if let Ok(p) = gl.build() {
                            window.paint_path(p, grid_col);
                        }
                    }

                    let first = px_pt(smooth_pts[0]);
                    let last = px_pt(*smooth_pts.last().unwrap_or(&smooth_pts[0]));

                    let mut area = PathBuilder::fill();
                    area.move_to(point(first.x, base_y));
                    area.line_to(first);
                    for &pt in smooth_pts.iter().skip(1) {
                        area.line_to(px_pt(pt));
                    }
                    area.line_to(point(last.x, base_y));
                    area.close();
                    if let Ok(path) = area.build() {
                        window.paint_path(path, fill_col);
                    }

                    let mut line = PathBuilder::stroke(px(2.));
                    line.move_to(first);
                    for &pt in smooth_pts.iter().skip(1) {
                        line.line_to(px_pt(pt));
                    }
                    if let Ok(path) = line.build() {
                        window.paint_path(path, stroke_col);
                    }
                },
            )
            .size_full(),
        );

        let hero_card = div()
            .w_full()
            .rounded(px(R_CARD))
            .bg(s.surface)
            .border_1()
            .border_color(s.line_soft)
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
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
                                                    .child("累计词元总吞吐 (Input + Output)"),
                                            )
                                            .when(is_demo_fallback, |r| {
                                                r.child(
                                                    div()
                                                        .px(px(6.))
                                                        .py(px(1.))
                                                        .rounded(px(R_PILL))
                                                        .bg(s.pro.amber_soft)
                                                        .text_size(px(10.))
                                                        .text_color(s.pro.amber_ink)
                                                        .child("演示数据"),
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
                                                    .text_size(px(28.))
                                                    .line_height(px(32.))
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(s.ink)
                                                    .child(format_tokens_compact(total_combined)),
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
                                                                "缓存命中 {:.1}%",
                                                                snap.cache_hit_ratio
                                                            )),
                                                    )
                                                },
                                            ),
                                    ),
                            )
                            .child(period_bar),
                    )
                    .child(trend_canvas),
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
                    .child(usage_footer_cell(
                        "输入词元",
                        format_tokens_compact(snap.total_input_tokens as i64),
                        s.pro.blue,
                        s,
                    ))
                    .child(usage_footer_cell(
                        "输出与推理",
                        format_tokens_compact(snap.total_output_tokens as i64),
                        s.pro.teal,
                        s,
                    ))
                    .child(usage_footer_cell(
                        "Prompt 缓存读取",
                        if snap.has_cache_read {
                            format_tokens_compact(snap.total_cached_tokens as i64)
                        } else {
                            "—".into()
                        },
                        s.pro.green,
                        s,
                    ))
                    .child(usage_footer_cell(
                        "覆盖会话与轮次",
                        format!("{} 会话 · {} 轮", snap.total_chats, snap.total_turns),
                        s.pro.purple,
                        s,
                    )),
            );

        // ── 3. KUI `"消耗去向拆解"` (`Where it went`: Models vs Projects) ────
        let split_sensor = pro_selection_sensor();
        let split_key: SharedString = if self.usage_by_project {
            "ub-proj".into()
        } else {
            "ub-model".into()
        };
        let split_toggle = div()
            .relative()
            .p(px(2.5))
            .rounded(px(R_CTRL + 2.))
            .bg(s.tray)
            .border_1()
            .border_color(s.line_soft)
            .flex()
            .items_center()
            .gap(px(2.))
            .child(pro_pill_indicator(
                "kar-usage-where-pill",
                split_key,
                split_sensor.clone(),
                R_CTRL,
                s.card,
                Some(s.line),
                false,
            ))
            .child(
                div()
                    .relative()
                    .rounded(px(R_CTRL))
                    .when(!self.usage_by_project, |d| {
                        d.child(pro_mark_selected(&split_sensor))
                    })
                    .child(
                        Tactile::ghost("usage-tab-models", s)
                            .fills(transparent_black(), s.hover)
                            .h(px(24.))
                            .px(px(9.))
                            .text_size(px(FS_META))
                            .text_color(if !self.usage_by_project {
                                s.ink
                            } else {
                                s.muted
                            })
                            .child("按智能体模型")
                            .on_press(cx, |this, _, cx| {
                                this.usage_by_project = false;
                                cx.notify();
                            }),
                    ),
            )
            .child(
                div()
                    .relative()
                    .rounded(px(R_CTRL))
                    .when(self.usage_by_project, |d| {
                        d.child(pro_mark_selected(&split_sensor))
                    })
                    .child(
                        Tactile::ghost("usage-tab-projects", s)
                            .fills(transparent_black(), s.hover)
                            .h(px(24.))
                            .px(px(9.))
                            .text_size(px(FS_META))
                            .text_color(if self.usage_by_project {
                                s.ink
                            } else {
                                s.muted
                            })
                            .child("按工作区项目")
                            .on_press(cx, |this, _, cx| {
                                this.usage_by_project = true;
                                cx.notify();
                            }),
                    ),
            );

        let mut breakdown_card = div()
            .w_full()
            .p(px(15.))
            .rounded(px(R_CARD))
            .bg(s.surface)
            .border_1()
            .border_color(s.line_soft)
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(FS_UI))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(s.ink)
                            .child("消耗去向拆解"),
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
                        .child("发起对话后，各 CLI 智能体的词元吞吐与缓存占比将自动汇总于此。"),
                );
            } else {
                let max_tok = snap
                    .by_runtime
                    .iter()
                    .map(|r| r.input_tokens + r.output_tokens)
                    .max()
                    .unwrap_or(1)
                    .max(1);
                for (ix, row) in snap.by_runtime.iter().enumerate() {
                    let sum = row.input_tokens + row.output_tokens;
                    let ratio = (sum as f32 / max_tok as f32).clamp(0.04, 1.);
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
                                                        "{} 会话 · {} 轮",
                                                        row.chat_count, row.turn_count
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
                                            .text_color(s.ink)
                                            .child(format!(
                                                "{}（缓存 {}）",
                                                format_tokens_compact(sum as i64),
                                                format_tokens_compact(row.cached_tokens as i64)
                                            )),
                                    ),
                            )
                            .child(pro_spring_bar(
                                SharedString::from(format!("usage-rt-bar-{}", row.runtime_id)),
                                ratio,
                                6.,
                                99.,
                                s.tray,
                                bar_color,
                            )),
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
                            turns += c.turn_count.max(1);
                        }
                    }
                    (p.name.clone(), chats, turns)
                })
                .collect();

            if is_demo_fallback && proj_rows.iter().all(|(_, c, _)| *c == 0) {
                proj_rows = vec![
                    ("KWrok".into(), 8, 38),
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
                    0.06
                } else {
                    (turns as f32 / max_turns as f32).clamp(0.08, 1.0)
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
                                        .child(format!("{chats} 会话 · {turns} 轮交互")),
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

        let mut body = div()
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(hero_card)
            .child(breakdown_card);

        if !is_demo_fallback
            && snap.daily_activity.iter().any(|(_, n)| *n > 0.)
            && let Some(hm) = &self.usage_heatmap
        {
            body = body.child(hm.clone());
        }

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
}

fn usage_footer_cell(
    label: &'static str,
    value: String,
    dot_col: Hsla,
    s: &StudioPalette,
) -> Div {
    div()
        .flex_1()
        .min_w_0()
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
                        .text_size(px(10.5))
                        .text_color(s.muted)
                        .truncate()
                        .child(label),
                ),
        )
        .child(
            div()
                .font_features(tnum_features())
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(s.ink)
                .truncate()
                .child(value),
        )
}
