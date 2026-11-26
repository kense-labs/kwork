//! KUI `agent_schedules.rs` Application Screen:
//! 24-Hour Smooth Activity Curve, Live `NOW` Glowing Cursor, Trigger Stems & Session Run History.

use chrono::Timelike;
use gpui::{prelude::*, *};
use kui::primitives::pro_icons::{ProIcon, pro_icon};
use uuid::Uuid;

use crate::{
    remote_link::now_epoch_ms,
    schedules::{ScheduleCadence, ScheduleRunOutcome, ScheduleToneChoice, ScheduledTask},
    types::OfficeKind,
    ui_motion::{Tactile, primary_button, soft_button, text_button},
    ui_primitives::{
        FS_META, FS_TAG, FS_UI, ICON_XS, LH_META, R_CARD, R_CTRL, R_PILL, StudioPalette,
        clean_display_doc_name, mono_font, render_office_chip, render_provider_color_tile,
        tnum_features,
    },
    workbench::KarWorkbench,
};

fn resolve_tone(tone: ScheduleToneChoice, s: &StudioPalette) -> Hsla {
    match tone {
        ScheduleToneChoice::Blue => s.pro.blue,
        ScheduleToneChoice::Teal => s.pro.teal,
        ScheduleToneChoice::Purple => s.pro.purple,
        ScheduleToneChoice::Orange => s.pro.orange,
        ScheduleToneChoice::Green => s.pro.green,
    }
}

fn paint_disc_at(window: &mut Window, center: Point<Pixels>, r: f32, color: Hsla) {
    let steps = 16usize;
    let mut pb = PathBuilder::fill();
    for i in 0..steps {
        let ang = (i as f32 / steps as f32) * std::f32::consts::TAU;
        let pt = point(center.x + px(ang.cos() * r), center.y + px(ang.sin() * r));
        if i == 0 {
            pb.move_to(pt);
        } else {
            pb.line_to(pt);
        }
    }
    pb.close();
    if let Ok(path) = pb.build() {
        window.paint_path(path, color);
    }
}

impl KarWorkbench {
    pub(crate) fn render_schedules_panel(
        &mut self,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let creator_el = self
            .schedule_creator_open
            .then(|| self.render_schedule_creator(s, cx).into_any_element());
        let tasks = self.schedule_registry.tasks.clone();
        let rhythm_card = self.render_24h_rhythm_card(&tasks, s, cx);

        let mut list = div().flex().flex_col().gap(px(10.));
        if tasks.is_empty() {
            list = list.child(
                div()
                    .w_full()
                    .p(px(18.))
                    .rounded(px(R_CARD))
                    .bg(s.surface)
                    .border_1()
                    .border_color(s.line_soft)
                    .text_size(px(FS_UI))
                    .text_color(s.muted)
                    .child("还没有配置定时自动化任务，点击右上角「新建任务」即可添加。"),
            );
        } else {
            for task in &tasks {
                list = list.child(self.render_schedule_card(task, s, cx));
            }
        }

        div()
            .w_full()
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(rhythm_card)
            .when_some(creator_el, |d, form| d.child(form))
            .child(list)
            .into_any_element()
    }

    /// KUI `agent_schedules.rs` signature 24-Hour Smooth Activity Curve + Trigger Stems + Live `NOW` Cursor.
    fn render_24h_rhythm_card(
        &self,
        tasks: &[ScheduledTask],
        s: &StudioPalette,
        cx: &App,
    ) -> Div {
        let local_now = chrono::Local::now();
        let now_frac =
            ((local_now.hour() * 60 + local_now.minute()) as f32 / 1440.0).clamp(0.02, 0.98);
        let now_str = local_now.format("%H:%M").to_string();
        let enabled_cnt = tasks.iter().filter(|t| t.enabled).count();

        // Collect (x_fraction_0_to_1, color) stems for enabled tasks
        let mut stems: Vec<(f32, Hsla)> = Vec::new();
        for (idx, t) in tasks.iter().filter(|t| t.enabled).enumerate() {
            let col = resolve_tone(t.tone, s);
            match t.cadence {
                ScheduleCadence::DailyNineAm | ScheduleCadence::WeeklyMonday => {
                    let jitter = (idx as f32 * 0.028).min(0.12);
                    stems.push(((9.0 / 24.0) + jitter, col));
                }
                ScheduleCadence::EveryMinutes(m) => {
                    let step_h = (m.max(30) as f32 / 60.0).clamp(2.0, 6.0);
                    let mut h = step_h;
                    while h < 23.5 {
                        stems.push((h / 24.0, col));
                        h += step_h * 2.0;
                    }
                }
                ScheduleCadence::ManualOnly => {}
            }
        }

        // Sample 72 smooth points across 24h summing Gaussian pulses around each stem
        let mut curve_pts: Vec<(f32, f32)> = Vec::with_capacity(73);
        for i in 0..=72 {
            let x = i as f32 / 72.0;
            let mut y = 0.08 + 0.04 * (x * std::f32::consts::TAU * 2.0).sin().abs();
            for &(sx, _) in &stems {
                let d = (x - sx) / 0.055;
                y += 0.62 * (-0.5 * d * d).exp();
            }
            let d_now = (x - now_frac) / 0.045;
            y += 0.22 * (-0.5 * d_now * d_now).exp();
            curve_pts.push((x, y.clamp(0.06, 0.92)));
        }

        let line_soft = s.line_soft;
        let line_base = s.line;
        let curve_stroke = s.pro.accent_info;
        let curve_fill = s.pro.accent_info.opacity(if s.dark { 0.14 } else { 0.10 });
        let now_col = s.pro.green;

        let plot_canvas = div().w_full().h(px(96.)).child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let bw = f32::from(bounds.size.width);
                    let bh = f32::from(bounds.size.height);
                    if bw <= 12. || bh <= 12. {
                        return;
                    }
                    let pad_top = 10.0_f32;
                    let pad_bot = 6.0_f32;
                    let usable_h = (bh - pad_top - pad_bot).max(4.);
                    let base_y = bounds.bottom() - px(pad_bot);
                    let px_pt = |(nx, ny): (f32, f32)| {
                        point(
                            bounds.left() + px(nx * bw),
                            bounds.top() + px(pad_top + (1.0 - ny) * usable_h),
                        )
                    };

                    // Horizontal guide lines + baseline
                    for frac in [0.35_f32, 0.70] {
                        let gy = bounds.top() + px(pad_top + frac * usable_h);
                        let mut gl = PathBuilder::stroke(px(1.));
                        gl.move_to(point(bounds.left(), gy));
                        gl.line_to(point(bounds.right(), gy));
                        if let Ok(p) = gl.build() {
                            window.paint_path(p, line_soft);
                        }
                    }
                    let mut base = PathBuilder::stroke(px(1.));
                    base.move_to(point(bounds.left(), base_y));
                    base.line_to(point(bounds.right(), base_y));
                    if let Ok(p) = base.build() {
                        window.paint_path(p, line_base);
                    }

                    // Area fill + smooth stroke
                    if curve_pts.len() >= 2 {
                        let first = px_pt(curve_pts[0]);
                        let last = px_pt(*curve_pts.last().unwrap_or(&curve_pts[0]));
                        let mut area = PathBuilder::fill();
                        area.move_to(point(first.x, base_y));
                        area.line_to(first);
                        for &pt in curve_pts.iter().skip(1) {
                            area.line_to(px_pt(pt));
                        }
                        area.line_to(point(last.x, base_y));
                        area.close();
                        if let Ok(path) = area.build() {
                            window.paint_path(path, curve_fill);
                        }

                        let mut stroke = PathBuilder::stroke(px(1.8));
                        stroke.move_to(first);
                        for &pt in curve_pts.iter().skip(1) {
                            stroke.line_to(px_pt(pt));
                        }
                        if let Ok(path) = stroke.build() {
                            window.paint_path(path, curve_stroke);
                        }
                    }

                    // Scheduled task stems
                    let stem_top = bounds.top() + px(18.);
                    for &(sx, col) in &stems {
                        let mx = bounds.left() + px(sx * bw);
                        let mut stem = PathBuilder::stroke(px(1.3));
                        stem.move_to(point(mx, stem_top));
                        stem.line_to(point(mx, base_y));
                        if let Ok(path) = stem.build() {
                            window.paint_path(path, col.opacity(0.65));
                        }
                        paint_disc_at(window, point(mx, stem_top), 4.5, col.opacity(0.22));
                        paint_disc_at(window, point(mx, stem_top), 2.6, col);
                    }

                    // Live `NOW` cursor stem + glowing head
                    let now_x = bounds.left() + px(now_frac * bw);
                    let now_top = bounds.top() + px(10.);
                    let mut now_stem = PathBuilder::stroke(px(1.6));
                    now_stem.move_to(point(now_x, now_top));
                    now_stem.line_to(point(now_x, base_y));
                    if let Ok(path) = now_stem.build() {
                        window.paint_path(path, now_col.opacity(0.88));
                    }
                    paint_disc_at(window, point(now_x, now_top), 5.5, now_col.opacity(0.24));
                    paint_disc_at(window, point(now_x, now_top), 3.0, now_col);
                },
            )
            .size_full(),
        );

        let mut ticks_row = div()
            .w_full()
            .pt(px(2.))
            .flex()
            .items_center()
            .justify_between()
            .font_family(mono_font(cx))
            .font_features(tnum_features())
            .text_size(px(10.))
            .text_color(s.faint);
        for lbl in ["00:00", "06:00", "12:00", "18:00", "24:00"] {
            ticks_row = ticks_row.child(div().child(lbl));
        }

        div()
            .w_full()
            .p(px(15.))
            .rounded(px(R_CARD))
            .bg(s.surface)
            .border_1()
            .border_color(s.line_soft)
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(FS_UI))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(s.ink)
                                    .child("24 小时自动化排程节律"),
                            )
                            .child(
                                div()
                                    .px(px(6.))
                                    .py(px(1.))
                                    .rounded(px(R_PILL))
                                    .bg(s.tray)
                                    .font_features(tnum_features())
                                    .text_size(px(FS_TAG))
                                    .text_color(s.muted)
                                    .child(format!("{enabled_cnt} 项已启用")),
                            ),
                    )
                    .child(
                        div()
                            .px(px(7.))
                            .py(px(2.))
                            .rounded(px(R_PILL))
                            .bg(s.pro.green_soft)
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .font_family(mono_font(cx))
                            .font_features(tnum_features())
                            .text_size(px(10.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.pro.green_ink)
                            .child(div().size(px(5.5)).rounded_full().bg(s.pro.green))
                            .child(format!("NOW {now_str}")),
                    ),
            )
            .child(plot_canvas)
            .child(ticks_row)
    }

    fn render_schedule_creator(&self, s: &StudioPalette, cx: &mut Context<Self>) -> Div {
        let bound_doc = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .map(|t| (t.kind, clean_display_doc_name(&t.doc_name(cx))));

        let mut cadence_row = div().flex().flex_wrap().items_center().gap(px(6.));
        for (ix, preset) in ScheduleCadence::PRESETS.into_iter().enumerate() {
            let active = self.schedule_draft_cadence == preset;
            cadence_row = cadence_row.child(
                soft_button(("sched-cadence-opt", ix), preset.label(), s)
                    .lit(active)
                    .on_press(cx, move |this, _, cx| {
                        this.schedule_draft_cadence = preset;
                        cx.notify();
                    }),
            );
        }

        div()
            .w_full()
            .p(px(14.))
            .rounded(px(R_CARD))
            .bg(s.surface)
            .border_1()
            .border_color(s.line)
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(self.schedule_title_input.clone())
            .child(self.schedule_prompt_input.clone())
            .child(cadence_row)
            .child(
                div()
                    .w_full()
                    .pt(px(2.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        soft_button(
                            "sched-bind-active-doc",
                            match (&bound_doc, self.schedule_bind_active_doc) {
                                (Some((_, name)), true) => format!("已绑定模板：{name}"),
                                (Some((_, name)), false) => format!("绑定当前打开文档：{name}"),
                                (None, _) => "无打开的 Office 模板".into(),
                            },
                            s,
                        )
                        .disabled(bound_doc.is_none())
                        .lit(self.schedule_bind_active_doc && bound_doc.is_some())
                        .when_some(bound_doc.map(|(k, _)| k), |b, kind| {
                            b.child(render_office_chip(kind, s))
                        })
                        .on_press(cx, |this, _, cx| {
                            this.schedule_bind_active_doc = !this.schedule_bind_active_doc;
                            cx.notify();
                        }),
                    )
                    .child(
                        primary_button("sched-create-save", "保存定时任务", s)
                            .child(pro_icon(ProIcon::Check, ICON_XS, s.on_primary))
                            .on_press(cx, |this, window, cx| {
                                this.save_new_schedule_from_form(window, cx);
                            }),
                    ),
            )
    }

    fn render_schedule_card(
        &self,
        task: &ScheduledTask,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Div {
        let tid = task.id.clone();
        let tid_toggle = tid.clone();
        let tid_run = tid.clone();
        let tid_del = tid.clone();
        let tid_hist = tid.clone();
        let enabled = task.enabled;
        let tone_color = resolve_tone(task.tone, s);
        let history_open = self.expanded_schedule_history.contains(&tid);

        let associated_chats =
            self.client_reducer
                .associations
                .list_associated_chats("schedules", "task", Some(&tid));

        let mut card = div()
            .w_full()
            .p(px(14.))
            .rounded(px(R_CARD))
            .bg(s.surface)
            .border_1()
            .border_color(if enabled {
                tone_color.opacity(0.35)
            } else {
                s.line_soft
            })
            .flex()
            .flex_col()
            .gap(px(8.));

        let top = div()
            .w_full()
            .flex()
            .items_start()
            .justify_between()
            .gap(px(10.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .child(
                        Tactile::ghost(SharedString::from(format!("sched-enabled-{tid}")), s)
                            .h(px(crate::ui_primitives::CTRL_H))
                            .px(px(8.))
                            .gap(px(6.))
                            .flex_none()
                            .lit(enabled)
                            .tip(if enabled {
                                "暂停自动运行"
                            } else {
                                "启用自动运行"
                            })
                            .aria(if enabled {
                                "已启用，点击暂停自动运行"
                            } else {
                                "已暂停，点击启用自动运行"
                            })
                            .child(div().size(px(6.)).rounded_full().bg(if enabled {
                                tone_color
                            } else {
                                s.faint
                            }))
                            .child(if enabled { "已启用" } else { "已暂停" })
                            .on_press(cx, move |this, _, cx| {
                                this.toggle_schedule_enabled(&tid_toggle, cx);
                            }),
                    )
                    .child(
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
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(FS_UI))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(s.ink)
                                            .child(task.title.clone()),
                                    )
                                    .child(
                                        div()
                                            .px(px(6.))
                                            .py(px(1.))
                                            .rounded(px(R_PILL))
                                            .bg(s.tray)
                                            .text_size(px(FS_TAG))
                                            .text_color(s.muted)
                                            .child(task.cadence.label()),
                                    ),
                            )
                            .child(
                                div()
                                    .line_clamp(2)
                                    .text_size(px(FS_META))
                                    .line_height(px(LH_META))
                                    .text_color(s.muted)
                                    .child(task.prompt.clone()),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        soft_button(
                            SharedString::from(format!("sched-run-now-{tid}")),
                            "立即运行",
                            s,
                        )
                        .child(pro_icon(ProIcon::Bolt, ICON_XS, s.ink))
                        .on_press(cx, move |this, window, cx| {
                            this.trigger_schedule_now(&tid_run, window, cx);
                        }),
                    )
                    .child(
                        Tactile::ghost(SharedString::from(format!("sched-del-{tid}")), s)
                            .size(px(26.))
                            .justify_center()
                            .tip("移除此定时任务（保留已生成的关联会话）")
                            .child(pro_icon(ProIcon::Trash, ICON_XS, s.muted))
                            .on_press(cx, move |this, _, cx| {
                                this.remove_schedule_task(&tid_del, cx);
                            }),
                    ),
            );

        let meta_bar = div()
            .w_full()
            .pt(px(2.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(FS_META))
                            .text_color(s.text)
                            .child(render_provider_color_tile(&task.runtime_id, 18., s, cx))
                            .child(crate::types::runtime_display_name(&task.runtime_id)),
                    )
                    .when_some(task.office_template.as_ref(), |row, path| {
                        let name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("template");
                        let kind = OfficeKind::for_path(path).unwrap_or(OfficeKind::Excel);
                        row.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .px(px(6.))
                                .py(px(1.))
                                .rounded(px(5.))
                                .bg(s.tray)
                                .text_size(px(FS_TAG))
                                .text_color(s.text)
                                .child(render_office_chip(kind, s))
                                .child(clean_display_doc_name(name)),
                        )
                    }),
            )
            .child(
                text_button(
                    SharedString::from(format!("sched-hist-btn-{tid}")),
                    format!(
                        "关联会话 ({})",
                        task.history.len().max(associated_chats.len())
                    ),
                    s,
                )
                .child(pro_icon(
                    if history_open {
                        ProIcon::ChevronUp
                    } else {
                        ProIcon::ChevronDown
                    },
                    ICON_XS,
                    s.muted,
                ))
                .on_press(cx, move |this, _, cx| {
                    if !this.expanded_schedule_history.remove(&tid_hist) {
                        this.expanded_schedule_history.insert(tid_hist.clone());
                    }
                    cx.notify();
                }),
            );

        card = card.child(top).child(meta_bar);
        if enabled
            && let Some(due) = task
                .next_due_ms
                .and_then(|ms| i64::try_from(ms).ok())
                .and_then(chrono::DateTime::from_timestamp_millis)
        {
            card = card.child(
                div()
                    .font_features(tnum_features())
                    .text_size(px(FS_META))
                    .text_color(s.muted)
                    .child(format!(
                        "下次自动触发：{}",
                        due.with_timezone(&chrono::Local).format("%m-%d %H:%M")
                    )),
            );
        }

        if history_open {
            let mut hist_col = div()
                .w_full()
                .pt(px(6.))
                .border_t_1()
                .border_color(s.line_soft)
                .flex()
                .flex_col()
                .gap(px(4.));
            if task.history.is_empty() && associated_chats.is_empty() {
                hist_col = hist_col.child(
                    div()
                        .text_size(px(FS_META))
                        .text_color(s.muted)
                        .child("暂无执行记录。点击「立即运行」将自动创建关联对话并投递任务。"),
                );
            } else {
                for (r_ix, rec) in task.history.iter().rev().take(6).enumerate() {
                    let (badge_bg, badge_fg) = match rec.outcome {
                        ScheduleRunOutcome::Completed => (s.pro.green_soft, s.pro.green_ink),
                        ScheduleRunOutcome::Admitted => (s.pro.accent_soft, s.pro.accent_info),
                        ScheduleRunOutcome::Failed => (s.pro.red_soft, s.pro.red_ink),
                        ScheduleRunOutcome::OutcomeUnknown => (s.pro.amber_soft, s.pro.amber_ink),
                    };
                    let chat_opt: Option<Uuid> = rec.chat_id;
                    let age_mins = now_epoch_ms()
                        .saturating_sub(rec.triggered_ms)
                        .div_ceil(60_000);
                    hist_col = hist_col.child(
                        div()
                            .w_full()
                            .px(px(8.))
                            .py(px(5.))
                            .rounded(px(R_CTRL))
                            .bg(s.card)
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .px(px(6.))
                                            .py(px(1.))
                                            .rounded(px(R_PILL))
                                            .bg(badge_bg)
                                            .text_size(px(FS_TAG))
                                            .text_color(badge_fg)
                                            .child(rec.outcome.label()),
                                    )
                                    .child(div().text_size(px(FS_META)).text_color(s.text).child(
                                        if rec.manual {
                                            format!("手动触发 · {age_mins} 分钟前")
                                        } else {
                                            format!("自动触发 · {age_mins} 分钟前")
                                        },
                                    )),
                            )
                            .when_some(chat_opt, |row, cid| {
                                row.child(
                                    text_button(
                                        SharedString::from(format!("sched-jump-{tid}-{r_ix}")),
                                        "打开对话",
                                        s,
                                    )
                                    .child(pro_icon(ProIcon::ExternalArrow, 11., s.muted))
                                    .on_press(
                                        cx,
                                        move |this, window, cx| {
                                            this.close_all_overlays(cx);
                                            this.open_chat(cid, window, cx);
                                        },
                                    ),
                                )
                            }),
                    );
                }
            }
            card = card.child(hist_col);
        }

        card
    }
}
