//! Tool approvals replace the composer until the outstanding ACP request is resolved.

use super::*;
use gpui::{FocusHandle, Role};
use serde::Serialize;

#[wasm_bindgen(module = "/approval-accessibility.js")]
extern "C" {
    fn update_approval(state: JsValue);
    pub(super) fn listen_approval(callback: &js_sys::Function);
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Decision {
    Reject,
    RejectAlways,
    Allow,
    Other,
    AllowAlways,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
struct Choice {
    id: String,
    label: String,
    #[serde(skip)]
    decision: Decision,
}

impl Choice {
    fn from_event(option: &Value) -> Option<Self> {
        let id = option["optionId"].as_str().filter(|id| !id.is_empty())?;
        let name = option["name"].as_str().unwrap_or(id);
        let (decision, label) = match option["kind"].as_str() {
            Some("reject_once") => (Decision::Reject, "拒绝".to_owned()),
            Some("reject_always") => (Decision::RejectAlways, "始终拒绝".to_owned()),
            Some("allow_once") => (Decision::Allow, "允许一次".to_owned()),
            Some("allow_always") => (Decision::AllowAlways, "始终允许".to_owned()),
            None => {
                let decision = match id {
                    "reject-once" | "reject" | "deny" => Decision::Reject,
                    "reject-always" | "always-reject" => Decision::RejectAlways,
                    "allow-once" => Decision::Allow,
                    "allow-always" | "always-allow" => Decision::AllowAlways,
                    _ => Decision::Other,
                };
                let label = if decision == Decision::RejectAlways {
                    "始终拒绝".to_owned()
                } else {
                    crate::approval_command::approval_option_label(id, name)
                };
                (decision, label)
            }
            _ => (Decision::Other, name.to_owned()),
        };
        Some(Self {
            id: id.to_owned(),
            label,
            decision,
        })
    }
}

pub(super) struct ApprovalRequest {
    pub(super) id: String,
    pub(super) tool_call: Value,
    choices: Vec<Choice>,
    answering: bool,
    error: Option<String>,
}

impl ApprovalRequest {
    pub(super) fn from_event(event: &Value) -> Option<Self> {
        let id = event["requestId"].as_str()?.to_owned();
        let mut choices = Vec::new();
        for choice in event["options"]
            .as_array()?
            .iter()
            .filter_map(Choice::from_event)
        {
            if !choices.iter().any(|old: &Choice| old.id == choice.id) {
                choices.push(choice);
            }
        }
        if choices.is_empty() {
            return None;
        }
        choices.sort_by_key(|option| option.decision);
        Some(Self {
            id,
            tool_call: event["toolCall"].clone(),
            choices,
            answering: false,
            error: None,
        })
    }

    fn option(&self, decision: Decision) -> Option<String> {
        self.choices
            .iter()
            .find(|choice| choice.decision == decision)
            .map(|choice| choice.id.clone())
    }
}

#[derive(PartialEq, Eq, Serialize)]
struct Card {
    id: String,
    subject: String,
    command: Option<String>,
    choices: Vec<Choice>,
    allow: Option<String>,
    reject: Option<String>,
    answering: bool,
    error: Option<String>,
}

pub(super) struct ApprovalSurface {
    scope: FocusHandle,
    details: FocusHandle,
    rendered: Option<Card>,
    visible: Option<Card>,
}

impl ApprovalSurface {
    pub(super) fn new(cx: &App) -> Self {
        Self {
            scope: cx.focus_handle(),
            details: cx.focus_handle().tab_stop(true),
            rendered: None,
            visible: None,
        }
    }

    pub(super) fn begin_frame(&mut self) {
        self.rendered = None;
    }
}

impl DshApp {
    pub(super) fn publish_accessible_approval(&mut self) {
        let card = self
            .approvals
            .rendered
            .take()
            .filter(|_| self.workspace_tree_available());
        if card != self.approvals.visible
            && let Ok(state) = serde_wasm_bindgen::to_value(&card)
        {
            self.approvals.visible = card;
            update_approval(state);
        }
    }

    pub(super) fn answer_accessible_approval(
        &mut self,
        request_id: &str,
        option_id: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .approvals
            .visible
            .as_ref()
            .is_some_and(|card| card.id == request_id)
        {
            self.answer_approval(request_id, option_id, w, cx);
        }
    }

    fn answer_approval(&mut self, id: &str, option: &str, w: &mut Window, cx: &mut Context<Self>) {
        if self.page != WorkspacePage::Conversation || !self.workspace_tree_available() {
            return;
        }
        let (Some(session), Some(request)) = (
            self.active_session.clone(),
            self.conversation.approvals.front_mut(),
        ) else {
            return;
        };
        if request.id != id
            || request.answering
            || !request.choices.iter().any(|choice| choice.id == option)
        {
            return;
        }
        request.answering = true;
        request.error = None;
        if self.approvals.scope.contains_focused(w, cx) {
            self.approvals.details.focus(w, cx);
        }
        let id = id.to_owned();
        let payload = json!({"sessionId": session, "requestId": id, "optionId": option});
        cx.spawn_in(w, async move |app, cx| {
            let reply = bridge::invoke("respond_permission", payload).await;
            let _ = app.update_in(cx, |app, w, cx| {
                app.finish_approval(&session, &id, reply, w, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_approval(
        &mut self,
        session: &str,
        id: &str,
        reply: Result<Value, String>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match reply {
            Err(error) if error != "permission/request-closed" => {
                if let Some(request) = self.conversation_mut(session).and_then(|conversation| {
                    conversation
                        .approvals
                        .iter_mut()
                        .find(|request| request.id == id)
                }) {
                    request.answering = false;
                    request.error = Some(format!("提交审批失败，请重试。{error}"));
                }
            }
            _ => self.close_approval(session, id, w, cx),
        }
        cx.notify();
    }

    pub(super) fn close_approval(
        &mut self,
        session: &str,
        id: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = self.active_session.as_deref() == Some(session);
        let Some(conversation) = self.conversation_mut(session) else {
            return;
        };
        let was_first = conversation
            .approvals
            .front()
            .is_some_and(|request| request.id == id);
        conversation.approvals.retain(|request| request.id != id);
        if selected && was_first && self.approvals.scope.contains_focused(w, cx) {
            if self.conversation.approvals.is_empty() {
                self.focus_composer(w, cx);
            } else {
                self.approvals.details.focus(w, cx);
            }
        }
    }

    fn approval_shortcut_focused(&self, w: &Window, cx: &App) -> bool {
        self.page == WorkspacePage::Conversation
            && self.workspace_tree_available()
            && !self.conversation.approvals.is_empty()
            && self.approvals.scope.contains_focused(w, cx)
    }

    pub(super) fn handle_approval_shortcut(
        &mut self,
        input: &shortcut_settings::ShortcutInput,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<bool> {
        let combo = input.combo.as_str();
        if !matches!(
            input.code.as_str(),
            "Enter" | "NumpadEnter" | "Escape" | "Space"
        ) && !matches!(combo, "Enter" | "Escape" | " ")
        {
            return None;
        }
        if !self.approval_shortcut_focused(w, cx) {
            return None;
        }
        self.last_escape = None;
        if input.modifiers != 0 || input.repeat || input.composing {
            return Some(true);
        }
        let decision = match combo {
            "Escape" => Decision::Reject,
            "Enter" if self.approvals.details.is_focused(w) => Decision::Allow,
            _ => return Some(false), // The focused button handles its own activation.
        };
        if let Some(request) = self.conversation.approvals.front()
            && let Some(option) = request.option(decision)
        {
            let id = request.id.clone();
            self.answer_approval(&id, &option, w, cx);
        }
        Some(true)
    }

    pub(super) fn render_permission(
        &mut self,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Stateful<gpui::Div>> {
        let request = self.conversation.approvals.front()?;
        let entries = &self.conversation.entries;
        let card = Card {
            id: request.id.clone(),
            subject: match crate::approval_command::approval_tool_label(&request.tool_call, entries)
            {
                Some(tool) => format!("工具 {tool} 请求越权执行"),
                None => "工具请求越权执行".to_owned(),
            },
            command: crate::approval_command::approval_command(&request.tool_call, entries),
            choices: request.choices.clone(),
            allow: request.option(Decision::Allow),
            reject: request.option(Decision::Reject),
            answering: request.answering,
            error: request.error.clone(),
        };
        let p = dsh(cx);
        let mut actions = row().w_full().flex_wrap().justify_end().gap(px(8.));
        let primary = if card
            .choices
            .iter()
            .any(|choice| choice.decision == Decision::AllowAlways)
        {
            Decision::AllowAlways
        } else {
            Decision::Allow
        };
        for choice in &card.choices {
            let request_id = card.id.clone();
            let option_id = choice.id.clone();
            let button = Button::new(gpui::ElementId::Name(
                format!("{}-{}", request_id, option_id).into(),
            ));
            let button = if choice.decision == primary {
                button.primary()
            } else {
                button.secondary()
            };
            actions = actions.child(
                button
                    .label(choice.label.clone())
                    .disabled(card.answering)
                    .on_click(cx.listener(move |app, _, w, cx| {
                        app.answer_approval(&request_id, &option_id, w, cx);
                    }))
                    .into_themed(cx),
            );
        }
        let details = column()
            .id("approval-details")
            .track_focus(&self.approvals.details)
            .role(Role::Group)
            .aria_label("审批详情")
            .focus_visible(|style| style.bg(p.hover))
            .w_full()
            .max_h(px(336.).min(w.viewport_size().height * 0.48))
            .overflow_y_scroll()
            .gap(px(6.))
            .px(px(16.))
            .pt(px(12.))
            .child(
                div()
                    .text_size(px(15.))
                    .line_height(px(24.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(card.subject.clone()),
            )
            .when_some(card.command.clone(), |details, command| {
                details.child(
                    div()
                        .font_family("monospace")
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .whitespace_normal()
                        .text_color(p.label_tertiary)
                        .child(command),
                )
            });
        let dot_color = if card.answering {
            p.state_business_primary
        } else {
            p.state_warn_primary
        };
        let warning = card.error.clone();
        self.approvals.rendered = Some(card);
        Some(
            column()
                .id("approval-card")
                .track_focus(&self.approvals.scope)
                .role(Role::Group)
                .aria_label("工具审批")
                .w(px(self.composer_width(w)))
                .max_w_full()
                .mx_auto()
                .mt(px(8.))
                .mb(px(12.))
                .flex_none()
                .overflow_hidden()
                .rounded(px(20.))
                .border_1()
                .border_color(p.state_warn_primary.opacity(0.6))
                .bg(workspace::composer_bg(cx))
                .shadow(elevation_soft())
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    row()
                        .w_full()
                        .h(px(38.))
                        .px(px(16.))
                        .gap(px(8.))
                        .items_center()
                        .rounded_t(px(19.))
                        .bg(p.state_warn_tertiary)
                        .text_color(p.state_warn_primary)
                        .text_size(px(13.))
                        .child(crate::anim::pending_dot(
                            "approval-dot",
                            div().size(px(8.)).rounded_full().bg(dot_color),
                        ))
                        .child("等待审批"),
                )
                .child(details)
                .when_some(warning, |card, error| {
                    card.child(
                        div()
                            .px(px(16.))
                            .pt(px(4.))
                            .text_size(px(13.))
                            .text_color(p.state_warn_label)
                            .child(error),
                    )
                })
                .child(actions.px(px(16.)).py(px(14.))),
        )
    }
}
