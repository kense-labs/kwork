//! Local scheduled tasks, custom cron/weekday/interval/webhook builder, edit mode & 24h rhythm chart.

use crate::ui_motion::selection::{Group, SelectionStrip};
use chrono::{TimeZone, Timelike};
use gpui::{prelude::*, *};
use kui::{
    components::input::Input,
    primitives::pro_icons::{ProIcon, pro_icon},
};
use uuid::Uuid;

use crate::{
    remote_link::now_epoch_ms,
    schedules::{ScheduleCadence, ScheduleRunOutcome, ScheduleToneChoice, ScheduledTask},
    types::OfficeKind,
    ui_motion::{
        Tactile, icon_button_sm, primary_button, soft_button, soft_button_sm, text_button_sm,
    },
    ui_primitives::{
        FS_META, FS_TAG, FS_UI, ICON_XS, LH_META, R_PILL, R_SM, StudioPalette, card_box,
        clean_display_doc_name, mono_font, render_office_chip, render_provider_color_tile,
        settings_page_shell, sub_card_box, tnum_features,
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.ensure_schedule_workspace() { self.refresh_schedules(crate::remote_link::now_epoch_ms(),false,cx); }
        let creator_el = self
            .schedule_creator_open
            .then(|| self.render_schedule_creator(s, window, cx).into_any_element());
        let tasks = self.schedule_registry.tasks.clone();
        let rhythm_card = self.render_24h_rhythm_card(&tasks, s, cx);

        let mut list = div().flex().flex_col().gap(px(10.));
        for warning in &self.schedule_registry.migration_warnings {
            list = list.child(div().text_size(px(FS_META)).text_color(s.muted).child(warning.clone()));
        }
        if let Some(error)=&self.schedule_error {
            list=list.child(div().text_size(px(FS_META)).text_color(s.muted).child(error.clone()));
        }
        if tasks.is_empty() && !self.schedule_loaded && self.schedule_error.is_none() {
            list=list.child(div().text_size(px(FS_META)).text_color(s.muted).child(self.ui_locale.pick("正在读取工作区任务…","Loading workspace schedules…")));
        } else if tasks.is_empty() && self.schedule_error.is_none() {
            list = list.child(
                card_box(s)
                    .w_full()
                    .p(px(18.))
                    .text_size(px(FS_UI))
                    .text_color(s.muted)
                    .child(self.ui_locale.pick(
                        "还没有定时任务。点击右上角「新建定时任务」添加。",
                        "Use New task to schedule your first automated run.",
                    )),
            );
        } else {
            for task in &tasks {
                list = list.child(self.render_schedule_card(task, s, cx));
            }
        }

        settings_page_shell()
            .when_some(creator_el, |d, form| d.child(form))
            .child(rhythm_card)
            .child(list)
            .into_any_element()
    }

    fn render_24h_rhythm_card(
        &self,
        tasks: &[ScheduledTask],
        s: &StudioPalette,
        cx: &App,
    ) -> Div {
        let local_now = chrono::Local::now();
        let now_frac =
            ((local_now.hour() * 60 + local_now.minute()) as f32 / 1440.0).clamp(0., 1.);
        let now_str = local_now.format("%H:%M").to_string();
        let enabled_cnt = tasks
            .iter()
            .filter(|task| task.enabled && task.next_due_ms.is_some())
            .count();
        let mut stems = Vec::new();
        for task in tasks.iter().filter(|task| task.enabled) {
            let Some(next) = task
                .next_due_ms
                .and_then(|due| i64::try_from(due).ok())
                .and_then(|ms| chrono::Local.timestamp_millis_opt(ms).single())
            else {
                continue;
            };
            if next.date_naive() != local_now.date_naive() {
                continue;
            }
            let mut minutes = next.hour() * 60 + next.minute();
            loop {
                stems.push((minutes as f32 / 1440., resolve_tone(task.tone, s)));
                let ScheduleCadence::EveryMinutes(step) = task.cadence else {
                    break;
                };
                let Some(following) = minutes.checked_add(step.max(1)) else {
                    break;
                };
                if following >= 1440 {
                    break;
                }
                minutes = following;
            }
        }
        let line_soft = s.line_soft;
        let line_base = s.line;
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
            .text_size(px(FS_TAG))
            .text_color(s.faint);
        for lbl in ["00:00", "06:00", "12:00", "18:00", "24:00"] {
            ticks_row = ticks_row.child(div().child(lbl));
        }

        card_box(s)
            .w_full()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
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
                                    .child(self.ui_locale.pick("今日 24 小时触发排期", "24h Schedule Timeline")),
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
                                    .child(format!(
                                        "{enabled_cnt} {}",
                                        self.ui_locale.pick("项已启用", "enabled")
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .px(px(8.))
                            .py(px(2.))
                            .rounded(px(R_PILL))
                            .bg(s.pro.green_soft)
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .font_family(mono_font(cx))
                            .font_features(tnum_features())
                            .text_size(px(FS_META))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.pro.green_ink)
                            .child(div().size(px(5.5)).rounded_full().bg(s.pro.green))
                            .child(now_str),
                    ),
            )
            .child(plot_canvas)
            .child(ticks_row)
    }

    fn render_schedule_creator(
        &self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let bound_doc = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .map(|t| (t.kind, clean_display_doc_name(&t.doc_name(cx))));

        let kind_tag: u8 = match &self.schedule_draft_cadence {
            ScheduleCadence::DailyNineAm => 0,
            ScheduleCadence::CustomTime { .. } => 1,
            ScheduleCadence::EveryMinutes(_) => 2,
            ScheduleCadence::WeeklyMonday => 3,
            ScheduleCadence::Webhook { .. } => 4,
            ScheduleCadence::ManualOnly | ScheduleCadence::OnceAtMs(_) => 5,
        };

        let mode_strip = SelectionStrip::options(
            "schedule-cadence-kind",
            loc.pick("触发周期模式", "Trigger mode"),
            Group::Choices,
            [
                (0u8, loc.pick("每日 09:00", "Daily 09:00")),
                (1u8, loc.pick("自定义时刻/星期", "Custom time & days")),
                (2u8, loc.pick("按分钟间隔", "Interval")),
                (3u8, loc.pick("每周一", "Mondays")),
                (4u8, "Webhook"),
                (5u8, loc.pick("仅手动", "Manual")),
            ],
            kind_tag,
            s,
            cx,
            |wb, tag, _, cx| {
                wb.schedule_draft_cadence = match tag {
                    0 => ScheduleCadence::DailyNineAm,
                    1 => ScheduleCadence::CustomTime {
                        hour: 9,
                        minute: 30,
                        weekdays_mask: 0b001_1111,
                    },
                    2 => ScheduleCadence::EveryMinutes(30),
                    3 => ScheduleCadence::WeeklyMonday,
                    4 => ScheduleCadence::Webhook {
                        secret: Uuid::new_v4().simple().to_string()[..12].to_owned(),
                    },
                    _ => ScheduleCadence::ManualOnly,
                };
                cx.notify();
            },
        );

        let hhmm_inp = window.use_keyed_state("sched-custom-hhmm", cx, |win, cx| {
            Input::new("sched-hhmm", loc.pick("触发时间 (HH:MM)", "Time (HH:MM)"), win, cx)
                .with_value("09:30", win, cx)
        });
        let mins_inp = window.use_keyed_state("sched-custom-mins", cx, |win, cx| {
            Input::new("sched-mins", loc.pick("间隔分钟数", "Every N minutes"), win, cx)
                .with_value("30", win, cx)
        });

        let mut extra_cadence: Option<AnyElement> = None;
        match &self.schedule_draft_cadence {
            ScheduleCadence::CustomTime {
                hour,
                minute,
                weekdays_mask,
            } => {
                let h_cur = *hour;
                let m_cur = *minute;
                let mask_cur = if *weekdays_mask == 0 { 0x7F } else { *weekdays_mask };
                let hhmm_apply = hhmm_inp.clone();
                let mut days_row = div().flex().items_center().flex_wrap().gap(px(6.));
                for (bit, name) in ["周一", "周二", "周三", "周四", "周五", "周六", "周日"]
                    .into_iter()
                    .enumerate()
                {
                    let on = (mask_cur & (1 << bit)) != 0;
                    days_row = days_row.child(
                        Tactile::soft(("sched-wday", bit), s)
                            .h(px(28.))
                            .px(px(9.))
                            .radius(R_SM)
                            .lit(on)
                            .outlined(if on { s.ink } else { s.line_soft }, s.line_strong)
                            .text_size(px(FS_META))
                            .child(name)
                            .on_press(cx, move |wb, _, cx| {
                                let next_mask = mask_cur ^ (1 << bit);
                                wb.schedule_draft_cadence = ScheduleCadence::CustomTime {
                                    hour: h_cur,
                                    minute: m_cur,
                                    weekdays_mask: if next_mask == 0 { 1 << bit } else { next_mask },
                                };
                                cx.notify();
                            }),
                    );
                }
                extra_cadence = Some(
                    sub_card_box(s)
                        .p(px(12.))
                        .flex()
                        .items_end()
                        .justify_between()
                        .flex_wrap()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .items_end()
                                .gap(px(8.))
                                .child(div().w(px(140.)).child(hhmm_inp))
                                .child(
                                    soft_button_sm("sched-apply-hhmm", loc.pick("应用时刻", "Set time"), s)
                                        .on_press(cx, move |wb, _, cx| {
                                            let raw = hhmm_apply.read(cx).value(cx).trim().to_owned();
                                            let mut sp = raw.split(':');
                                            let nh = sp.next().and_then(|x| x.parse::<u8>().ok()).unwrap_or(9).min(23);
                                            let nm = sp.next().and_then(|x| x.parse::<u8>().ok()).unwrap_or(0).min(59);
                                            wb.schedule_draft_cadence = ScheduleCadence::CustomTime {
                                                hour: nh,
                                                minute: nm,
                                                weekdays_mask: mask_cur,
                                            };
                                            cx.notify();
                                        }),
                                ),
                        )
                        .child(days_row)
                        .into_any_element(),
                );
            }
            ScheduleCadence::EveryMinutes(cur_m) => {
                let mins_apply = mins_inp.clone();
                let cm = *cur_m;
                extra_cadence = Some(
                    sub_card_box(s)
                        .p(px(12.))
                        .flex()
                        .items_end()
                        .gap(px(8.))
                        .child(div().w(px(160.)).child(mins_inp))
                        .child(
                            soft_button_sm("sched-apply-mins", format!("当前：每 {cm} 分钟（点击更新）"), s)
                                .on_press(cx, move |wb, _, cx| {
                                    let val = mins_apply
                                        .read(cx)
                                        .value(cx)
                                        .trim()
                                        .parse::<u32>()
                                        .unwrap_or(30)
                                        .clamp(1, 1440);
                                    wb.schedule_draft_cadence = ScheduleCadence::EveryMinutes(val);
                                    cx.notify();
                                }),
                        )
                        .into_any_element(),
                );
            }
            ScheduleCadence::Webhook { secret } => {
                let sec_copy = secret.clone();
                extra_cadence = Some(
                    sub_card_box(s)
                        .px(px(12.))
                        .py(px(8.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .font_family(mono_font(cx))
                                .text_size(px(FS_META))
                                .text_color(s.ink)
                                .child(if secret.is_empty() {loc.pick("沿用已保存的签名密钥", "Use the saved signing secret")} else {loc.pick("保存前可复制此签名密钥", "Copy this signing secret before saving")}),
                        )
                        .child(
                            soft_button_sm("copy-wh-secret", loc.pick("复制密钥", "Copy secret"), s)
                                .disabled(secret.is_empty())
                                .on_press(cx, move |wb, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(sec_copy.clone()));
                                    wb.notify_toast("已复制 Webhook 密钥", cx);
                                }),
                        )
                        .into_any_element(),
                );
            }
            _ => {}
        }

        let is_editing = self.editing_schedule_id.is_some();

        card_box(s)
            .w_full()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .text_size(px(FS_UI))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(s.ink)
                    .child(if is_editing {
                        loc.pick("编辑定时任务", "Edit scheduled task")
                    } else {
                        loc.pick("新建自动化定时任务", "New scheduled task")
                    }),
            )
            .child(self.schedule_title_input.clone())
            .child(self.schedule_prompt_input.clone())
            .child(mode_strip)
            .when_some(extra_cadence, |d, el| d.child(el))
            .child(
                div()
                    .w_full()
                    .pt(px(2.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(
                        soft_button(
                            "sched-bind-active-doc",
                            match (&bound_doc, self.schedule_bind_active_doc) {
                                (Some((_, name)), true) => format!("已绑定模板：{name}"),
                                (Some((_, name)), false) => format!("绑定当前打开文档：{name}"),
                                (None, _) => loc
                                    .pick("无打开的办公文档模板", "No open document template")
                                    .into(),
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
                        primary_button(
                            "sched-create-save",
                            if is_editing {
                                loc.pick("保存修改", "Save changes")
                            } else {
                                loc.pick("保存定时任务", "Save task")
                            },
                            s,
                        )
                        .disabled(self.schedule_save_pending)
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
        let task_for_edit = task.clone();
        let enabled = task.enabled;
        let tone_color = resolve_tone(task.tone, s);
        let history_open = self.expanded_schedule_history.contains(&tid);

        let associated_chats = self
            .client_reducer
            .associations
            .list_associated_chats("schedules", "task", Some(&tid));

        let mut card = card_box(s)
            .w_full()
            .p(px(14.))
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
                            .h(px(28.))
                            .px(px(8.))
                            .radius(R_SM)
                            .gap(px(6.))
                            .flex_none()
                            .lit(enabled)
                            .tip(if enabled {
                                self.ui_locale.pick("暂停自动运行", "Pause schedule")
                            } else {
                                self.ui_locale.pick("启用自动运行", "Enable schedule")
                            })
                            .child(div().size(px(6.)).rounded_full().bg(if enabled {
                                tone_color
                            } else {
                                s.faint
                            }))
                            .child(if enabled {
                                self.ui_locale.pick("已启用", "Enabled")
                            } else {
                                self.ui_locale.pick("已暂停", "Paused")
                            })
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
                    .gap(px(6.))
                    .child(
                        soft_button_sm(
                            SharedString::from(format!("sched-edit-{tid}")),
                            self.ui_locale.pick("编辑", "Edit"),
                            s,
                        )
                        .disabled(task.engine_read_only)
                        .on_press(cx, move |this, window, cx| {
                            this.editing_schedule_id = Some(task_for_edit.id.clone());
                            this.editing_schedule_revision = Some(task_for_edit.revision);
                            this.schedule_bind_active_doc = false;
                            this.schedule_creator_open = true;
                            this.schedule_draft_cadence = task_for_edit.cadence.clone();
                            this.schedule_title_input.update(cx, |i, cx| {
                                i.set_value(task_for_edit.title.clone(), window, cx)
                            });
                            this.schedule_prompt_input.update(cx, |i, cx| {
                                i.set_value(task_for_edit.prompt.clone(), window, cx)
                            });
                            cx.notify();
                        }),
                    )
                    .child(
                        soft_button_sm(
                            SharedString::from(format!("sched-run-now-{tid}")),
                            self.ui_locale.pick("立即运行", "Run now"),
                            s,
                        )
                        .disabled(
                            tid_run.parse().ok().and_then(|id|self.schedule_pending_runs.get(&id)).is_some_and(|(_,pending)|*pending) || matches!(task.cadence, ScheduleCadence::Webhook { .. }) || task.history
                                .iter()
                                .any(|r| r.outcome == ScheduleRunOutcome::Admitted),
                        )
                        .child(pro_icon(ProIcon::Bolt, ICON_XS, s.ink))
                        .on_press(cx, move |this, window, cx| {
                            this.trigger_schedule_now(&tid_run, window, cx);
                        }),
                    )
                    .child(
                        icon_button_sm(
                            SharedString::from(format!("sched-del-{tid}")),
                            ProIcon::Trash,
                            self.ui_locale.pick(
                                "移除此定时任务（保留已生成的关联会话）",
                                "Remove schedule and keep its threads",
                            ),
                            s,
                        )
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
                                .rounded(px(R_SM))
                                .bg(s.tray)
                                .text_size(px(FS_TAG))
                                .text_color(s.text)
                                .child(render_office_chip(kind, s))
                                .child(clean_display_doc_name(name)),
                        )
                    }),
            )
            .child(
                text_button_sm(
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
        if matches!(task.cadence, ScheduleCadence::Webhook {..})
            && let Some(base) = &self.schedule_registry.webhook_base {
            let url=format!("{base}/{}", task.id);
            card=card.child(soft_button_sm(SharedString::from(format!("schedule-hook-{tid}")),
                self.ui_locale.pick("复制本机 Webhook 地址","Copy local webhook URL"),s)
                .on_press(cx,move |this,_,cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
                    this.notify_toast("已复制此工作台的签名 Webhook 地址",cx);
                }));
        }
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
                        .child(self.ui_locale.pick(
                            "暂无执行记录。点击「立即运行」将自动创建关联对话并投递任务。",
                            "No runs yet. Run now creates a separate thread.",
                        )),
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
                        sub_card_box(s)
                            .w_full()
                            .px(px(8.))
                            .py(px(5.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .flex_wrap()
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
                                            .child(self.ui_locale.pick(
                                                rec.outcome.label(),
                                                match rec.outcome {
                                                    ScheduleRunOutcome::Admitted => "Running",
                                                    ScheduleRunOutcome::Completed => "Completed",
                                                    ScheduleRunOutcome::Failed => "Failed",
                                                    ScheduleRunOutcome::OutcomeUnknown => {
                                                        "Outcome unknown"
                                                    }
                                                },
                                            )),
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
                                    text_button_sm(
                                        SharedString::from(format!("sched-jump-{tid}-{r_ix}")),
                                        self.ui_locale.pick("打开对话", "Open thread"),
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
