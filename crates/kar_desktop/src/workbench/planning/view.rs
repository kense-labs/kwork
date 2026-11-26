use super::*;
use crate::{
    ui_motion::{Tactile, icon_button},
    ui_primitives::{FS_CONTROL, R_CTRL},
};

impl KarWorkbench {
    pub(crate) fn render_approved_plan_turn(
        &self,
        id: Uuid,
        proposal: &str,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let expanded = self.planning.expanded_approvals.contains(&id);
        let title = proposal
            .lines()
            .find(|line| !line.trim().is_empty())
            .map(|line| line.trim().trim_start_matches('#').trim())
            .unwrap_or("");
        let status = loc.pick("已确认计划", "Approved plan");
        let mut header = div()
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(pro_icon(ProIcon::Tasks, 15., s.muted))
            .child(
                div()
                    .flex_none()
                    .text_size(px(FS_META))
                    .text_color(s.muted)
                    .child(status),
            );
        if !title.is_empty() {
            header = header.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(FS_CONTROL))
                    .font_weight(FontWeight::MEDIUM)
                    .child(title.to_owned()),
            );
        }
        header = header.child(pro_icon(
            if expanded {
                ProIcon::ChevronDown
            } else {
                ProIcon::ChevronRight
            },
            12.,
            s.muted,
        ));
        let bar = Tactile::ghost(format!("approved-plan-{id}"), s)
            .w_full()
            .h(px(32.))
            .px(px(8.))
            .aria(format!("{status} {title}"))
            .child(header)
            .on_press(cx, move |wb, _, cx| {
                if !wb.planning.expanded_approvals.remove(&id) {
                    wb.planning.expanded_approvals.insert(id);
                }
                cx.notify();
            });
        let mut card = div()
            .min_w_0()
            .w_full()
            .rounded(px(R_CTRL))
            .bg(s.soft)
            .child(bar);
        if expanded {
            let plan_body = Uuid::new_v5(&id, b"approved-plan");
            let copy = proposal.to_owned();
            let label = loc.pick("复制计划", "Copy plan");
            card = card.child(
                div()
                    .px(px(10.))
                    .pb(px(7.))
                    .child(
                        self.render_assistant_markdown_blocks(plan_body, 0, proposal, false, s, cx),
                    )
                    .child(
                        icon_button(format!("copy-approved-plan-{id}"), ProIcon::Copy, label, s)
                            .on_press(cx, move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                            }),
                    ),
            );
        }
        card.into_any_element()
    }

    pub(crate) fn render_planning_panel(
        &self,
        pane_w: f32,
        window: &Window,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.planning_here() {
            return None;
        }
        let loc = self.ui_locale;
        let chat = self.plan_owner();
        let record = self.current_plan();
        let phase = record.map_or(Phase::Draft, |p| p.phase);
        let question = record
            .and_then(|p| self.planning.questions.get(&p.id))
            .and_then(Clarification::next);
        let preview = record.and_then(|p| self.planning.previews.get(&p.id));
        let title = match phase {
            _ if question.is_some() => loc.pick("需要你确认", "A question for you"),
            Phase::Draft => loc.pick("计划模式", "Plan mode"),
            Phase::Working if preview.is_some() => loc.pick("正在起草计划", "Drafting plan"),
            Phase::Working => loc.pick("正在制定计划", "Planning"),
            Phase::Ready => loc.pick("待确认的计划", "Proposed plan"),
            Phase::Interrupted => loc.pick("规划已中断", "Planning interrupted"),
            _ => return None,
        };
        let expanded = chat.is_some_and(|id| self.planning.expanded.contains(&id));
        let mut header = div().w_full().flex().items_center().gap(px(6.));
        if phase == Phase::Ready
            && let Some(cid) = chat
        {
            header = header.child(
                Tactile::ghost("plan-disclosure", s)
                    .min_w_0()
                    .h(px(28.))
                    .px(px(5.))
                    .gap(px(6.))
                    .aria(title)
                    .child(pro_icon(ProIcon::Tasks, 15., s.ink))
                    .child(
                        div()
                            .text_size(px(FS_CONTROL))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(pro_icon(
                        if expanded {
                            ProIcon::ChevronDown
                        } else {
                            ProIcon::ChevronRight
                        },
                        12.,
                        s.muted,
                    ))
                    .on_press(cx, move |wb, _, cx| {
                        if !wb.planning.expanded.remove(&cid) {
                            wb.planning.expanded.insert(cid);
                        }
                        cx.notify();
                    }),
            );
        } else {
            header = header.child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .child(pro_icon(ProIcon::Tasks, 15., s.ink))
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(FS_CONTROL))
                            .child(title),
                    ),
            );
        }
        if let Some(questions) = record.and_then(|p| self.planning.questions.get(&p.id))
            && questions.questions.len() > 1
        {
            header = header.child(
                div()
                    .text_size(px(FS_META))
                    .text_color(s.muted)
                    .child(format!(
                        "{} / {}",
                        questions.answers.len() + 1,
                        questions.questions.len()
                    )),
            );
        }
        header = header.child(div().flex_1());
        if phase == Phase::Ready
            && let (Some(cid), Some(p)) = (chat, record)
        {
            let id = p.id;
            let label = loc.pick("实施计划", "Implement plan");
            header = header.child(
                Tactile::primary("implement-reviewed-plan", s)
                    .h(px(27.))
                    .px(px(9.))
                    .radius(R_CTRL)
                    .text_size(px(FS_META))
                    .aria(label)
                    .child(label)
                    .disabled(
                        self.turn_in_progress()
                            || self.planning.creating.is_some()
                            || !self.composer_editor.read(cx).value().trim().is_empty(),
                    )
                    .on_press(cx, move |wb, _, cx| wb.implement_plan(cid, id, cx)),
            );
        }
        header = header.child(
            icon_button(
                "leave-plan-mode",
                ProIcon::Close,
                if phase == Phase::Working {
                    loc.pick("取消规划", "Cancel planning")
                } else {
                    loc.pick("退出计划模式", "Leave plan mode")
                },
                s,
            )
            .size(px(26.))
            .disabled(self.planning.creating.is_some())
            .on_press(cx, |wb, window, cx| wb.exit_plan(window, cx)),
        );
        let mut panel = div()
            .id("composer-planning")
            .w_full()
            .min_w_0()
            .mb(px(5.))
            .px(px(10.))
            .py(px(6.))
            .rounded(px(12.))
            .border_1()
            .border_color(s.line_soft)
            .bg(s.card)
            .text_color(s.ink)
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(header);
        let height = (f32::from(window.viewport_size().height) * 0.24).clamp(52., 215.);
        if let (Some(q), Some(chat), Some(p)) = (question, chat, record) {
            let id = p.id;
            let mut body = div()
                .id("plan-questions")
                .max_h(px(height))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .items_start()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(FS_UI))
                        .text_color(s.text)
                        .child(q.prompt.clone()),
                );
            for (index, option) in q.options.iter().enumerate() {
                let question_id = q.id.clone();
                let answer = option.label.clone();
                body = body.child(
                    Tactile::ghost(("plan-answer", index), s)
                        .max_w_full()
                        .h_auto()
                        .px(px(7.))
                        .py(px(4.))
                        .text_size(px(FS_CONTROL))
                        .ink(s.text, s.ink)
                        .aria(answer.clone())
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(div().font_weight(FontWeight::MEDIUM).child(answer.clone()))
                                .when_some(option.description.clone(), |row, description| {
                                    row.child(
                                        div()
                                            .text_size(px(FS_META))
                                            .text_color(s.muted)
                                            .child(description),
                                    )
                                }),
                        )
                        .on_press(cx, move |wb, w, cx| {
                            wb.answer_plan_question(chat, id, &question_id, &answer, w, cx)
                        }),
                );
            }
            panel = panel.child(body);
        } else if let Some((id, text)) = match (phase, record, preview) {
            (Phase::Ready, Some(p), _) if expanded => Some((p.id, p.proposal.as_str())),
            (Phase::Working, Some(p), Some(draft)) => Some((p.id, draft.text.as_str())),
            _ => None,
        } {
            panel = panel.child(
                div()
                    .id("proposal-scroll")
                    .w_full()
                    .min_w_0()
                    .max_h(px(height))
                    .overflow_y_scroll()
                    .px(px(5.))
                    .child(self.render_assistant_markdown_blocks(id, 0, text, phase == Phase::Working, s, cx)),
            );
        } else if phase == Phase::Interrupted
            && let (Some(p), Some(chat)) = (record, chat)
        {
            let text = p.error.clone().unwrap_or_else(|| {
                loc.pick(
                    "上次规划尚未完成。",
                    "The previous planning session ended before the plan was ready.",
                )
                .into()
            });
            let topic = p.topic.clone();
            panel = panel
                .child(
                    div()
                        .id("planning-error")
                        .max_h(px(height))
                        .overflow_y_scroll()
                        .text_size(px(FS_META))
                        .text_color(s.muted)
                        .child(text),
                )
                .child(
                    Tactile::ghost("retry-plan", s)
                        .h(px(26.))
                        .text_size(px(FS_META))
                        .aria(loc.pick("重新规划", "Retry planning"))
                        .child(loc.pick("重新规划", "Retry planning"))
                        .on_press(cx, move |wb, _, cx| {
                            wb.generate_plan(chat, &topic, cx);
                        }),
                );
        } else if phase == Phase::Working
            && let Some(p) = record
        {
            panel = panel.child(
                div()
                    .truncate()
                    .text_size(px(FS_META))
                    .text_color(s.muted)
                    .child(
                        self.planning
                            .progress
                            .get(&p.id)
                            .unwrap_or(&p.topic)
                            .clone(),
                    ),
            );
        } else if phase == Phase::Draft {
            panel = panel.child(div().text_size(px(FS_META)).text_color(s.muted).child(
                if pane_w < 400. {
                    loc.pick("只读规划，确认后实施。", "Read-only until you approve.")
                } else {
                    loc.pick(
                        "Codex 只读查看工作区，等你确认计划后再实施。",
                        "Codex explores the workspace read-only. You decide when to implement.",
                    )
                },
            ));
        }
        Some(panel.into_any_element())
    }
}
