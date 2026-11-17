//! GPUI question composer and the Host's foreground and continued requests.

use super::{dock_controls::dock_action_button, *};
use crate::questions::{Clock, Question, RecordedAnswer};
use gpui::svg;
use std::{collections::HashSet, time::Duration};

#[wasm_bindgen(raw_module = "../trajectory-json.js")]
extern "C" {
    #[wasm_bindgen(js_name = clearQuestionDraft)]
    fn clear_draft(session: &str, request: &str);

    #[wasm_bindgen(js_name = questionSubmitting)]
    fn submitting(session: &str, request: &str) -> bool;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Foreground,
    Continuing,
    Continued,
    Review,
}

pub(super) struct PendingQuestions {
    id: String,
    pub(super) call_id: Option<String>,
    questions: Vec<Question>,
    review: Option<Vec<RecordedAnswer>>,
    focused: bool,
    wait_state: &'static str,
    pub(super) hidden: bool,
    stage: Stage,
    clock: Option<Clock>,
}

impl PendingQuestions {
    fn new(id: &str, update: &Value, stage: Stage) -> Option<Self> {
        let questions: Vec<Question> = serde_json::from_value(update["questions"].clone()).ok()?;
        if questions.is_empty() {
            return None;
        }
        Some(Self {
            id: id.to_owned(),
            call_id: update["callId"].as_str().map(str::to_owned),
            questions,
            review: None,
            focused: false,
            wait_state: "counting",
            hidden: stage == Stage::Continued,
            stage,
            clock: update["remainingMs"]
                .as_f64()
                .map(|remaining| Clock::Counting {
                    deadline: bridge::monotonic_ms() + remaining.max(0.),
                }),
        })
    }

    pub(super) fn is_pending(&self) -> bool {
        self.stage != Stage::Review
    }
}

impl Conversation {
    pub(super) fn visible_question(&self) -> Option<&PendingQuestions> {
        self.questions.iter().rev().find(|panel| !panel.hidden)
    }
}

impl DshApp {
    fn question_mut(&mut self, session: &str, id: &str) -> Option<&mut PendingQuestions> {
        self.conversation_mut(session)?
            .questions
            .iter_mut()
            .find(|panel| panel.id == id)
    }

    pub(super) fn handle_questions(
        &mut self,
        session: &str,
        update: &Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let was_showing = self.active_session.as_deref() == Some(session)
            && self.conversation.visible_question().is_some();
        match update["phase"].as_str() {
            Some("open") => {
                let Some(id) = update["requestId"].as_str() else {
                    return;
                };
                if self.question_mut(session, id).is_none()
                    && let Some(panel) = PendingQuestions::new(id, update, Stage::Foreground)
                    && let Some(conversation) = self.conversation_mut(session)
                {
                    let timed = panel.clock.is_some();
                    conversation.questions.push(panel);
                    if timed {
                        self.tick_question(session.to_owned(), id.to_owned(), w, cx);
                    }
                }
            }
            Some("closed") => {
                let Some(id) = update["requestId"].as_str() else {
                    return;
                };
                if let Some(panel) = self.question_mut(session, id)
                    && update["willContinue"] == true
                {
                    // Projection and close can arrive in either order.
                    if panel.stage == Stage::Foreground {
                        panel.stage = Stage::Continuing;
                    }
                    panel.clock = None;
                } else if let Some(conversation) = self.conversation_mut(session) {
                    conversation.questions.retain(|panel| panel.id != id);
                    clear_draft(session, id);
                }
            }
            Some("continued") => {
                let Some(rows) = update["rows"].as_array() else {
                    return;
                };
                let Some(conversation) = self.conversation_mut(session) else {
                    return;
                };
                conversation.settled_questions = question_records(&update["settled"]);
                conversation.queued_question_replies = question_records(&update["queued"]);
                conversation.questions.retain(|panel| {
                    let keep = panel.stage != Stage::Continued
                        || rows.iter().any(|row| row["callId"] == panel.id);
                    if !keep {
                        clear_draft(session, &panel.id);
                    }
                    keep
                });
                for row in rows {
                    let Some(id) = row["callId"].as_str() else {
                        continue;
                    };
                    if let Some(panel) = conversation
                        .questions
                        .iter_mut()
                        .find(|panel| panel.id == id)
                    {
                        if panel.stage != Stage::Continued {
                            panel.stage = Stage::Continued;
                            panel.clock = None;
                        }
                    } else if let Some(panel) = PendingQuestions::new(id, row, Stage::Continued) {
                        conversation.questions.push(panel);
                    }
                }
            }
            _ => return,
        }
        if self.active_session.as_deref() == Some(session)
            && self.page == WorkspacePage::Conversation
        {
            if was_showing {
                self.focus_after_questions(session, w, cx);
            }
            self.maybe_follow_tail();
        }
        cx.notify();
    }

    /// Rehydrate the pending questions after a cold resume.
    ///
    /// `question_state` returns the bridge's `rows` projection directly, without
    /// the `settled` / `queued` snapshots carried by the `continued` event. Keep
    /// those snapshots intact and only reconcile the live panels here.
    pub(super) fn apply_question_state(&mut self, session: &str, rows: &[Value]) {
        let ids: HashSet<&str> = rows
            .iter()
            .filter_map(|row| row["callId"].as_str())
            .collect();
        if let Some(conversation) = self.conversation_mut(session) {
            conversation.questions.retain(|panel| {
                let keep = panel.stage != Stage::Continued || ids.contains(panel.id.as_str());
                if !keep {
                    clear_draft(session, &panel.id);
                }
                keep
            });
        }
        for row in rows {
            let Some(id) = row["callId"].as_str() else {
                continue;
            };
            if self.question_mut(session, id).is_some() {
                continue;
            }
            let Some(panel) = PendingQuestions::new(id, row, Stage::Continued) else {
                continue;
            };
            if let Some(conversation) = self.conversation_mut(session) {
                conversation.questions.push(panel);
            }
        }
    }

    fn tick_question(
        &mut self,
        session: String,
        id: String,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(w, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let keep_going = this.update_in(cx, |app, w, cx| {
                    let visible = app.active_session.as_deref() == Some(session.as_str())
                        && app.page == WorkspacePage::Conversation
                        && !app.show_settings
                        && app.conversation.approvals.is_empty()
                        && app.conversation.plan_reviews.is_empty()
                        && app
                            .conversation
                            .visible_question()
                            .is_some_and(|panel| panel.id == id);
                    let Some(panel) = app.question_mut(&session, &id) else {
                        return false;
                    };
                    let Some(clock) = panel.clock else {
                        return false;
                    };
                    if clock == Clock::Held {
                        return false;
                    }
                    let now = bridge::monotonic_ms();
                    let holding = visible && w.is_window_active() && panel.focused;
                    let clock = clock.step(now, holding);
                    panel.clock = Some(clock);
                    if !submitting(&session, &id) && clock.remaining(now) == Some(0.) {
                        panel.clock = None;
                        app.timeout_question(&session, &id, w, cx);
                    }
                    if visible {
                        cx.notify();
                    }
                    true
                });
                if !matches!(keep_going, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn timeout_question(
        &mut self,
        session: &str,
        id: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let args = json!({"sessionId":session, "requestId":id, "decision":"timeout"});
        let (session, id) = (session.to_owned(), id.to_owned());
        cx.spawn_in(w, async move |this, cx| {
            if let Err(error) = bridge::invoke("respond_question", args).await {
                let _ = this.update_in(cx, |app, _, cx| {
                    if let Some(panel) = app.question_mut(&session, &id)
                        && panel.stage == Stage::Foreground
                    {
                        panel.wait_state = "waiting";
                        app.push_toast(error, cx);
                    }
                });
            }
        })
        .detach();
    }

    fn focus_after_questions(&mut self, session: &str, w: &mut Window, cx: &mut Context<Self>) {
        if self.active_session.as_deref() == Some(session)
            && self.page == WorkspacePage::Conversation
            && !self.show_settings
            && self.conversation.approvals.is_empty()
            && self.conversation.plan_reviews.is_empty()
            && self.conversation.visible_question().is_none()
            && self.focused_right_pane(w, cx).is_none()
        {
            self.focus_composer(w, cx);
        }
    }

    pub(super) fn question_composer_action(
        &mut self,
        value: &Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.active_session.clone() else {
            return;
        };
        let Some(panel) = self
            .conversation
            .questions
            .iter_mut()
            .rev()
            .find(|panel| !panel.hidden)
        else {
            return;
        };
        if value["requestId"].as_str() != Some(&panel.id) {
            return;
        }
        match value["action"].as_str() {
            Some("question-focus") => panel.focused = value["focused"] == true,
            Some("question-hold") if panel.clock.is_some() => {
                panel.wait_state = if value["editing"] == true {
                    "editing"
                } else {
                    "waiting"
                };
                panel.clock = Some(Clock::Held);
            }
            Some("question-hide") if panel.call_id.is_some() => {
                panel.focused = false;
                panel.hidden = true;
                if value["settled"] == true {
                    let id = panel.id.clone();
                    self.conversation.questions.retain(|panel| panel.id != id);
                    clear_draft(&session, &id);
                }
                self.focus_after_questions(&session, w, cx);
            }
            _ => return,
        }
        cx.notify();
    }

    pub(super) fn reopen_question(&mut self, call_id: &str, cx: &mut Context<Self>) {
        let panels = &mut self.conversation.questions;
        if let Some(index) = panels
            .iter()
            .position(|panel| panel.is_pending() && panel.call_id.as_deref() == Some(call_id))
        {
            let mut panel = panels.remove(index);
            panel.hidden = false;
            panels.push(panel);
            cx.notify();
        }
    }

    fn review_question(&mut self, call_id: &str, cx: &mut Context<Self>) {
        let Some(answers) = self.conversation.settled_questions.get(call_id) else {
            return;
        };
        let Some(arguments) = self
            .conversation
            .entries
            .iter()
            .find(|entry| {
                entry.role == EntryRole::Tool && entry.external_id.as_deref() == Some(call_id)
            })
            .and_then(|entry| serde_json::from_str::<Value>(&entry.input).ok())
        else {
            return;
        };
        let id = format!("review-{call_id}");
        let mut update = json!({"callId":call_id});
        update["questions"] = arguments["questions"].clone();
        let Some(mut panel) = PendingQuestions::new(&id, &update, Stage::Review) else {
            return;
        };
        let Some(paired) = pair_recorded_answers(&panel.questions, answers) else {
            return;
        };
        panel.review = Some(paired.into_iter().cloned().collect());
        let panels = &mut self.conversation.questions;
        panels.retain(|panel| panel.id != id);
        panels.push(panel);
        cx.notify();
    }
}

fn question_records(rows: &Value) -> HashMap<String, Vec<RecordedAnswer>> {
    rows.as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let id = row["callId"].as_str()?.to_owned();
            Some((id, serde_json::from_value(row["answers"].clone()).ok()?))
        })
        .collect()
}

fn pair_recorded_answers<'a>(
    questions: &[Question],
    answers: &'a [RecordedAnswer],
) -> Option<Vec<&'a RecordedAnswer>> {
    if questions.is_empty() || questions.len() != answers.len() {
        return None;
    }
    let mut by_id: HashMap<_, _> = answers
        .iter()
        .map(|answer| (answer.id.as_str(), answer))
        .collect();
    questions
        .iter()
        .map(|question| by_id.remove(question.id.as_str()))
        .collect()
}

const DOWN: &[u8] = include_bytes!("../../assets/question-chevron-down.svg");
const QUESTION: &[u8] = include_bytes!("../../assets/question-question.svg");

fn question_icon(bytes: &'static [u8], size: f32, color: gpui::Hsla) -> gpui::Svg {
    svg()
        .data(bytes)
        .size(px(size))
        .flex_none()
        .text_color(color)
}

impl DshApp {
    pub(super) fn render_questions(&self, width: f32, cx: &Context<Self>) -> Option<gpui::Div> {
        let panel = self.conversation.visible_question()?;
        let session = self.active_session.as_deref()?;
        let channel = match panel.stage {
            Stage::Foreground => "waterfall",
            Stage::Continued => "rpc",
            Stage::Continuing | Stage::Review => "none",
        };
        let wait_state = if panel.stage == Stage::Continued {
            "continued"
        } else {
            panel.wait_state
        };
        let countdown = panel.clock.map(|clock| {
            json!({
                "remainingMs": clock.remaining(bridge::monotonic_ms()).unwrap_or(0.),
                "running": matches!(clock, Clock::Counting { .. }),
            })
        });
        let live: Vec<_> = self
            .conversation
            .questions
            .iter()
            .map(|panel| panel.id.as_str())
            .collect();
        let mut interaction = json!({
            "kind":"question", "requestId":panel.id, "questions":panel.questions,
            "canHide":panel.call_id.is_some(), "liveKeys":live,
            "snapshot":{
                "channel":channel, "closed":false, "waitState":wait_state,
                "state": if panel.stage == Stage::Continued {"continued"} else {"open"},
            }
        });
        if let Some(countdown) = countdown {
            interaction["snapshot"]["countdown"] = countdown;
        }
        if let Some(review) = &panel.review {
            interaction["review"] = json!(review);
        }
        Some(super::chat_markdown::interaction(
            session,
            &panel.id,
            width,
            &interaction,
            self.trajectory_controls_hidden(),
            cx,
        ))
    }

    pub(super) fn render_question_tool(
        &self,
        call_id: &str,
        entry: Option<&ChatEntry>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let p = dsh(cx);
        let pending = self
            .conversation
            .questions
            .iter()
            .find(|panel| panel.is_pending() && panel.call_id.as_deref() == Some(call_id));
        let settled = self.conversation.settled_questions.get(call_id);
        let queued = self.conversation.queued_question_replies.get(call_id);
        let questions: Vec<Question> = entry
            .and_then(|entry| serde_json::from_str::<Value>(&entry.input).ok())
            .and_then(|arguments| serde_json::from_value(arguments["questions"].clone()).ok())
            .unwrap_or_default();
        let output = entry
            .and_then(|entry| serde_json::from_str::<Value>(&entry.text).ok())
            .unwrap_or_default();
        let result_answers: Option<Vec<RecordedAnswer>> =
            serde_json::from_value(output["answers"].clone()).ok();
        let answers = queued.or(settled).or(result_answers.as_ref());
        let paired = answers.and_then(|answers| pair_recorded_answers(&questions, answers));
        let continued = pending
            .is_some_and(|panel| matches!(panel.stage, Stage::Continuing | Stage::Continued));
        let running =
            entry.is_some_and(|entry| matches!(entry.status.as_str(), "in_progress" | "pending"));
        let status = entry.map(|entry| entry.status.as_str()).unwrap_or_default();
        let failed = status == "failed";
        let mut verdict = None;
        let summary = if let Some(answers) = answers.filter(|answers| !answers.is_empty()) {
            let answered = answers
                .iter()
                .filter(|answer| {
                    !answer.selected.is_empty()
                        || answer.custom.as_ref().is_some_and(|text| !text.is_empty())
                })
                .count();
            format!("{answered}/{} 已回答", answers.len())
        } else if continued {
            verdict = Some("等待中的问题仍可在输入框中回答");
            "已继续工作，仍可回答".into()
        } else if pending.is_some() || running {
            "等待回答".into()
        } else if output["pending"] == true || settled.is_some() {
            verdict = Some("该问题已结束，结果见下方对话");
            "已结束".into()
        } else if status == "ASK_CANCELLED" {
            verdict = Some("本轮已取消，未提交回答");
            "已取消".into()
        } else if status == "ASK_ABORTED" {
            verdict = Some("本轮已中断，未提交回答");
            "已中断".into()
        } else if failed {
            "失败".into()
        } else {
            String::new()
        };
        let key = format!("question-tool-{call_id}");
        let expanded = self.conversation.expanded_entries.contains(&key);
        let for_toggle = key.clone();
        let mut title = row()
            .flex_1()
            .min_w_0()
            .gap(px(6.))
            .h(px(24.))
            .child(question_icon(
                if expanded { DOWN } else { QUESTION },
                16.,
                p.label_tertiary,
            ))
            .child(
                div()
                    .flex_none()
                    .text_size(px(13.))
                    .text_color(p.label_tertiary)
                    .child("提问"),
            )
            .when(!summary.is_empty(), |row| {
                row.child(div().text_color(p.label_dimmed).child("·"))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .text_color(if failed {
                                p.state_error_primary
                            } else {
                                p.label_tertiary
                            })
                            .child(summary),
                    )
            });
        if entry.is_some() {
            title = title.child(div().flex_1());
        }
        let mut header = row().w_full().min_h(px(24.)).gap(px(8.));
        if entry.is_some() {
            header = header.child(dock_action_button(
                title
                    .id(gpui::ElementId::Name(key.clone().into()))
                    .role(gpui::Role::Button)
                    .aria_label("提问")
                    .aria_expanded(expanded)
                    .tab_index(0)
                    .cursor_pointer(),
                cx.listener(move |app, _, _, cx| toggle_question_disclosure(app, &for_toggle, cx)),
            ));
        } else {
            header = header.child(title);
        }
        if pending.is_some() || ((settled.is_some() || queued.is_some()) && paired.is_some()) {
            let id = call_id.to_owned();
            let answerable = pending.is_some();
            let read_panel = settled.is_some() && queued.is_none();
            let label = if answerable { "回答" } else { "查看回答" };
            header = header.child(dock_action_button(
                row()
                    .id(gpui::ElementId::Name(
                        format!("answer-question-{id}").into(),
                    ))
                    .role(gpui::Role::Button)
                    .aria_label(label)
                    .tab_index(0)
                    .h(px(22.))
                    .px(px(10.))
                    .gap(px(4.))
                    .rounded_full()
                    .flex_none()
                    .text_size(px(12.))
                    .text_color(p.label_primary)
                    .bg(p.button_floating)
                    .cursor_pointer()
                    .hover(|style| style.bg(p.button_floating_hover))
                    .child(question_icon(QUESTION, 13., p.label_primary))
                    .child(label),
                cx.listener(move |app, _, _, cx| {
                    if answerable {
                        app.reopen_question(&id, cx);
                    } else if read_panel {
                        app.review_question(&id, cx);
                    } else {
                        toggle_question_disclosure(app, &key, cx);
                    }
                }),
            ));
        }
        let mut row = column().w_full().mt(px(4.)).gap_0().child(header);
        if expanded && let Some(entry) = entry {
            let mut detail = column()
                .id(gpui::ElementId::Name(
                    format!("question-details-{call_id}").into(),
                ))
                .w_full()
                .ml(px(4.))
                .my(px(4.))
                .px(px(20.))
                .py(px(16.))
                .gap(px(16.))
                .max_h(px(360.))
                .overflow_y_scroll()
                .rounded(px(16.))
                .border(px(0.5))
                .border_color(p.border_l1)
                .bg(p.bg_base)
                .text_size(px(14.))
                .line_height(px(24.));
            if let Some(paired) = paired {
                for (question, answer) in questions.iter().zip(paired) {
                    let answers: Vec<_> = answer
                        .selected
                        .iter()
                        .cloned()
                        .chain(answer.custom.clone().filter(|value| !value.is_empty()))
                        .collect();
                    let item = column().gap(px(2.)).child(
                        div()
                            .text_color(p.label_tertiary)
                            .child(question.question.clone()),
                    );
                    detail = detail.child(if answers.is_empty() {
                        item.child(div().text_color(p.label_tertiary).child("未回答"))
                    } else {
                        item.children(answers.into_iter().map(|value| div().child(value)))
                    });
                }
            } else if let Some(verdict) = verdict.filter(|_| !questions.is_empty()) {
                detail = detail.child(verdict).child(
                    column()
                        .pl(px(20.))
                        .gap(px(8.))
                        .text_color(p.label_tertiary)
                        .children(
                            questions
                                .into_iter()
                                .map(|q| div().child(format!("• {}", q.question))),
                        ),
                );
            } else {
                detail = detail
                    .when(!entry.input.is_empty(), |d| d.child(entry.input.clone()))
                    .when(!entry.text.is_empty(), |d| d.child(entry.text.clone()));
            }
            row = row.child(detail);
        }
        row
    }
}

fn toggle_question_disclosure(app: &mut DshApp, key: &str, cx: &mut Context<DshApp>) {
    super::workspace::toggle_entry(&mut app.conversation.expanded_entries, key);
    cx.notify();
}
