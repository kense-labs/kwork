//! Workflow run presentation shared by replayed and live transcript rows.

use super::workspace::toggle_entry;
use super::*;
use crate::workflow_runs::{Member, Run, status_label};

impl DshApp {
    pub(super) fn render_workflow_run(
        &self,
        entry: &crate::state::ChatEntry,
        expanded: bool,
        active_session: Option<&str>,
        subagents: &[crate::subagents::SubagentRow],
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let p = dsh(cx);
        let Ok(run) = serde_json::from_str::<Run>(&entry.text) else {
            return div().child(super::workspace::render_unknown_json(
                "无法读取工作流事件",
                entry,
                cx,
            ));
        };
        let status = crate::workflow_runs::status(&run);
        let abnormal = matches!(status, "failed" | "error" | "cancelled" | "interrupted");
        let status_color = if matches!(status, "cancelled" | "interrupted") {
            p.state_warn_primary
        } else if abnormal {
            p.state_error_primary
        } else if status == "running" {
            p.state_business_primary
        } else {
            p.success
        };
        let key = entry.key.clone();
        let mut card = column()
            .w_full()
            .rounded(px(10.))
            .border(px(0.5))
            .border_color(p.border_l1)
            .bg(p.stage)
            .overflow_hidden()
            .child(
                row()
                    .id(gpui::ElementId::Name(
                        format!("workflow-{}", entry.key).into(),
                    ))
                    .role(gpui::Role::Button)
                    .aria_label(format!("工作流：{}", run.name))
                    .w_full()
                    .min_h(px(42.))
                    .px(px(12.))
                    .gap(px(8.))
                    .items_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(p.hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        toggle_entry(&mut this.conversation.expanded_entries, &key);
                        cx.notify();
                    }))
                    .child(icon(
                        if expanded { Glyph::Down } else { Glyph::Arrow },
                        12.,
                        p.faint,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(run.name.clone()),
                    )
                    .child(note(format!("{} 个成员", run.members.len()), cx))
                    .child(div().size(px(6.)).rounded_full().bg(status_color))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(status_color)
                            .child(status_label(status)),
                    ),
            );
        if expanded {
            let mut phases = column().w_full().px(px(12.)).pb(px(10.)).gap(px(8.));
            if run.members.is_empty() {
                phases = phases.child(note("没有启动成员", cx));
            }
            let mut grouped: Vec<(Option<String>, Vec<&Member>)> = Vec::new();
            for member in &run.members {
                if let Some((_, members)) =
                    grouped.iter_mut().find(|(phase, _)| *phase == member.phase)
                {
                    members.push(member);
                } else {
                    grouped.push((member.phase.clone(), vec![member]));
                }
            }
            for (index, (phase, members)) in grouped.into_iter().enumerate() {
                let phase_key = format!("{}-phase-{index}", entry.key);
                let phase_open =
                    status == "running" || self.conversation.expanded_entries.contains(&phase_key);
                let toggle_key = phase_key.clone();
                let phase_name = match phase.as_deref() {
                    None => "未分阶段".to_owned(),
                    Some("") => "空阶段名".to_owned(),
                    Some(name) => name.to_owned(),
                };
                let mut section = column()
                    .w_full()
                    .rounded(px(8.))
                    .bg(p.bg_module_platform)
                    .child(
                        row()
                            .id(gpui::ElementId::Name(phase_key.clone().into()))
                            .role(gpui::Role::Button)
                            .w_full()
                            .min_h(px(32.))
                            .px(px(10.))
                            .gap(px(6.))
                            .items_center()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                toggle_entry(&mut this.conversation.expanded_entries, &toggle_key);
                                cx.notify();
                            }))
                            .child(icon(
                                if phase_open {
                                    Glyph::Down
                                } else {
                                    Glyph::Arrow
                                },
                                10.,
                                p.faint,
                            ))
                            .child(div().flex_1().child(phase_name))
                            .child(note(format!("{} 个成员", members.len()), cx)),
                    );
                if phase_open {
                    for member in members {
                        let status = crate::workflow_runs::member_status(&run, member);
                        let child = member.child_id.clone();
                        let child_title = member.label.clone();
                        let known = active_session.is_some()
                            && status == "running"
                            && self.running_subagents.contains_key(&child)
                            && subagents.iter().any(|session| {
                                session.child_id == child && session.has_local_session()
                            });
                        let mut row =
                            row()
                                .id(gpui::ElementId::Name(
                                    format!("workflow-member-{}", member.seq).into(),
                                ))
                                .w_full()
                                .min_h(px(32.))
                                .px(px(10.))
                                .gap(px(8.))
                                .items_center()
                                .child(div().size(px(5.)).rounded_full().bg(
                                    if status == "completed" {
                                        p.success
                                    } else {
                                        status_color
                                    },
                                ))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(12.))
                                        .child(if member.label.is_empty() {
                                            "空成员名".to_owned()
                                        } else {
                                            member.label.clone()
                                        }),
                                )
                                .child(note(status_label(status), cx));
                        if known {
                            row = row
                                .role(gpui::Role::Button)
                                .aria_label(format!("打开 {}", child_title))
                                .cursor_pointer()
                                .hover(|style| style.bg(p.hover))
                                .on_click(cx.listener(move |this, _, w, cx| {
                                    this.open_subagent_child(&child, &child_title, w, cx);
                                }));
                        }
                        section = section.child(row);
                    }
                }
                phases = phases.child(section);
            }
            card = card.child(phases);
        }
        card
    }
}
