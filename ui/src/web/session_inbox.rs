//! Pending Inbox rows: the messages waiting in the Host for the running or the
//! next turn.
//!
//! Official `QueueDock` keeps one row per durable Inbox item and lets the user
//! edit, remove, or steer it. The rows are Host-owned here too: the client only
//! mirrors what `_dsh/gpui/session/inbox-state` reports and drops a row when the
//! Host admits it, so a queued message survives closing the app.

use super::queue_attachments::QueueAttachment;
use super::*;
use crate::submit::SubmitMode;
use serde::Serialize;

#[wasm_bindgen(module = "/queue-accessibility.js")]
extern "C" {
    fn update_inbox(state: JsValue);
    pub(super) fn listen_inbox(callback: &js_sys::Function);
}

#[derive(PartialEq, Eq, Serialize)]
pub(super) struct AccessibleInbox {
    session: String,
    expanded: bool,
    locked: bool,
    busy: bool,
    running: bool,
    rows: Vec<AccessibleInboxRow>,
    pending: Option<AccessibleInboxRow>,
    editor: Option<AccessibleInboxEditor>,
    error: Option<String>,
}

#[derive(PartialEq, Eq, Serialize)]
struct AccessibleInboxRow {
    id: String,
    display: String,
    editable: bool,
    attachments: Vec<String>,
}

#[derive(PartialEq, Eq, Serialize)]
struct AccessibleInboxEditor {
    id: String,
    draft: String,
}

impl DshApp {
    pub(super) fn can_steer_inbox(&self, empty: bool) -> bool {
        empty
            && self.connected()
            && self.conversation.acp_open
            && self.conversation.running
            && self.composer_menu != Some(ComposerMenu::Commands)
            && self.conversation.inbox_submission.is_none()
            && self.conversation.inbox_action.is_none()
            && self
                .conversation
                .queued_prompts
                .iter()
                .any(|row| row.mode == SubmitMode::Queue)
    }

    pub(super) fn steer_inbox(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.active_session.clone() else {
            return;
        };
        let items: Vec<_> = self
            .conversation
            .queued_prompts
            .iter()
            .filter(|row| row.mode == SubmitMode::Queue)
            .map(|row| row.item_id.clone())
            .collect();
        self.conversation.inbox_action = items.first().cloned();
        self.conversation.inbox_error = None;
        cx.spawn_in(w, async move |app, cx| {
            let mut moved = Vec::new();
            let mut error = None;
            for item in items {
                let result = bridge::invoke(
                    "inbox_action",
                    json!({
                        "sessionId": session, "itemId": item, "action": "steer",
                    }),
                )
                .await;
                match result {
                    Ok(_) => moved.push(item),
                    Err(failure) => {
                        if !failure.contains("session/steer-unavailable")
                            && !failure.contains("session/queue-item")
                        {
                            error = Some(failure);
                        }
                        break;
                    }
                }
            }
            let _ = app.update_in(cx, |app, _, cx| {
                let Some(conversation) = app.conversation_mut(&session) else {
                    return;
                };
                conversation.inbox_action = None;
                conversation.inbox_error = error;
                conversation.inbox_seq += 1;
                for row in &mut conversation.queued_prompts {
                    if moved.contains(&row.item_id) {
                        row.mode = SubmitMode::Steer;
                    }
                }
                app.request_inbox_state(&session);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Ask the Host for the authoritative pending list of one session.
    ///
    /// The locally allocated sequence travels with the request and comes back
    /// inside the reply, so a slow snapshot cannot delete a row that a newer
    /// submission added meanwhile.
    pub(super) fn request_inbox_state(&mut self, session: &str) {
        let Some(conversation) = self.conversation_mut(session) else {
            return;
        };
        let issued = conversation.inbox_snapshot_request();
        invoke_with_reply(
            "inbox_state",
            "inbox_state",
            json!({"sessionId": session, "requestSeq": issued}),
        );
    }

    /// Apply one `inbox_state` reply.
    pub(super) fn finish_inbox_state(
        &mut self,
        owner: Option<&str>,
        result: Result<Value, String>,
        _w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(reply) = result else {
            // A failed snapshot keeps the mirrored rows: they are still owned by
            // the Host, and the next mutation refreshes them anyway.
            return;
        };
        let issued = reply["requestSeq"].as_u64().unwrap_or(0);
        let items = reply["items"].as_array().cloned().unwrap_or_default();
        let Some(conversation) = owner.and_then(|session| self.conversation_mut(session)) else {
            return;
        };
        conversation.apply_inbox_snapshot(&items, issued);
        if self.active_session.as_deref() == owner {
            self.request_inbox_images();
        }
        cx.notify();
    }

    /// Render one user message the Host logged.
    ///
    /// A message the client submitted already has its optimistic transcript row;
    /// the durable identity only has to be adopted. A message admitted from the
    /// Inbox (steering, or a queue row that just became a turn) has no local row
    /// yet and must be appended at its real position in the log.
    pub(super) fn apply_logged_user_message(
        &mut self,
        session: &str,
        update: &Value,
        cx: &mut Context<Self>,
    ) {
        let Some(message_id) = update["messageId"].as_str() else {
            return;
        };
        let text = update["content"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_owned();
        let has_image = update["hasImage"].as_bool().unwrap_or(false);
        let at = update["time"].as_f64().unwrap_or_else(bridge::now_ms);
        let selected = self.active_session.as_deref() == Some(session);
        let Some(conversation) = self.conversation_mut(session) else {
            return;
        };
        let local_submission_admitted = conversation
            .inbox_submission
            .as_mut()
            .filter(|submission| update["requestId"].as_str() == Some(&submission.id));
        if let Some(submission) = local_submission_admitted {
            submission.admitted = true;
        }
        let was_pending = conversation.admit_inbox_row(message_id);
        let files: Vec<crate::state::UserFile> = update["files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|file| serde_json::from_value(file.clone()).ok())
            .collect();
        let display = if files.is_empty() {
            pending_inbox_display(&text, has_image)
        } else {
            text
        };

        let appended = crate::state::adopt_logged_user_message(
            &mut conversation.entries,
            message_id,
            display,
            Some(at),
        );
        // Attached images travel as attachment references; the row keeps them so
        // the bubble can show thumbnails once the host reads the objects.
        let images: Vec<crate::state::UserImage> = update["images"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|image| {
                Some(crate::state::UserImage {
                    attachment_id: image.get("attachmentId")?.as_str()?.to_owned(),
                    media_type: image
                        .get("mediaType")
                        .and_then(Value::as_str)
                        .unwrap_or("image/png")
                        .to_owned(),
                    name: image
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    width: image.get("width").and_then(Value::as_u64),
                    height: image.get("height").and_then(Value::as_u64),
                    data_url: None,
                })
            })
            .collect();
        if (!images.is_empty() || !files.is_empty())
            && let Some(entry) = conversation.entries.iter_mut().find(|entry| {
                entry.role == crate::state::EntryRole::User
                    && entry.external_id.as_deref() == Some(message_id)
            })
        {
            crate::state::set_user_attachments(entry, &images, &files);
        }
        if !images.is_empty() {
            self.request_user_images();
        }
        if was_pending {
            self.request_inbox_state(session);
        }
        if (appended || was_pending) && selected {
            self.maybe_follow_tail();
            cx.notify();
        }
    }

    fn edit_inbox_row(
        &mut self,
        session: &str,
        item: &str,
        focus_canvas: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_session.as_deref() != Some(session)
            || self.conversation.inbox_action.is_some()
        {
            return;
        }
        let Some(text) = self
            .conversation
            .queued_prompts
            .iter()
            .find(|row| row.item_id == item && row.mode == SubmitMode::Queue)
            .and_then(|row| row.edit_text.as_ref())
        else {
            return;
        };
        let input = editor("编辑排队消息".into(), InputMode::InlineMulti, w, cx);
        input.update(cx, |editor, _| editor.font_size = 13.);
        replace(&input, text.clone(), w, cx);
        let subscription = cx.subscribe_in(&input, w, |app, sender, event, w, cx| {
            if !app
                .conversation
                .inbox_edit
                .as_ref()
                .is_some_and(|edit| edit.editor == *sender)
            {
                return;
            }
            match event {
                EditEvent::Submitted => app.save_inbox_edit(w, cx),
                EditEvent::Escape => app.cancel_inbox_edit(w, cx),
                EditEvent::Changed => cx.notify(),
                _ => {}
            }
        });
        self.conversation.inbox_edit = Some(InboxEdit {
            item_id: item.to_owned(),
            editor: input.clone(),
            _subscription: subscription,
        });
        self.conversation.inbox_error = None;
        self.composer_menu = None;
        if focus_canvas {
            input.read(cx).focus.clone().focus(w, cx);
        }
        cx.notify();
    }

    fn save_inbox_edit(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let (Some(session), Some(edit)) =
            (self.active_session.clone(), &self.conversation.inbox_edit)
        else {
            return;
        };
        let text = edit.editor.read(cx).value().to_owned();
        if !text.trim().is_empty() {
            let item = edit.item_id.clone();
            self.mutate_inbox_row(&session, &item, InboxAction::Edit(text), w, cx);
        }
    }

    fn cancel_inbox_edit(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.conversation.inbox_action.is_none() && self.conversation.inbox_edit.take().is_some()
        {
            self.focus_composer(w, cx);
            cx.notify();
        }
    }

    fn mutate_inbox_row(
        &mut self,
        session: &str,
        item: &str,
        action: InboxAction,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_session.as_deref() != Some(session)
            || self.conversation.inbox_action.is_some()
            || !self
                .conversation
                .queued_prompts
                .iter()
                .any(|row| row.item_id == item && row.mode == SubmitMode::Queue)
            || (matches!(action, InboxAction::Steer) && !self.conversation.running)
        {
            return;
        }
        self.conversation.inbox_action = Some(item.to_owned());
        self.conversation.inbox_error = None;
        let session = session.to_owned();
        let item = item.to_owned();
        let (verb, text) = match &action {
            InboxAction::Edit(text) => ("edit", Some(text)),
            InboxAction::Remove => ("remove", None),
            InboxAction::Steer => ("steer", None),
        };
        let payload = json!({
            "sessionId": session, "itemId": item, "action": verb, "text": text,
        });
        cx.spawn_in(w, async move |app, cx| {
            let result = bridge::invoke("inbox_action", payload).await;
            let _ = app.update_in(cx, |app, w, cx| {
                let selected = app.active_session.as_deref() == Some(session.as_str());
                let Some(conversation) = app.conversation_mut(&session) else {
                    return;
                };
                conversation.inbox_action = None;
                let mut restore_focus = false;
                match result {
                    Ok(reply) => {
                        conversation.inbox_seq += 1;
                        match action {
                            InboxAction::Remove => {
                                conversation.admit_inbox_row(&item);
                            }
                            InboxAction::Steer => {
                                if let Some(row) = conversation
                                    .queued_prompts
                                    .iter_mut()
                                    .find(|r| r.item_id == item)
                                {
                                    row.mode = SubmitMode::Steer;
                                }
                            }
                            InboxAction::Edit(_) => {
                                if let Some(row) = conversation
                                    .queued_prompts
                                    .iter_mut()
                                    .find(|r| r.item_id == item)
                                    && let Some(edited) = QueuedPrompt::from_inbox(&reply["item"])
                                {
                                    *row = edited;
                                }
                                if conversation
                                    .inbox_edit
                                    .as_ref()
                                    .is_some_and(|edit| edit.item_id == item)
                                    && let Some(edit) = conversation.inbox_edit.take()
                                {
                                    restore_focus =
                                        selected && edit.editor.read(cx).focus.is_focused(w);
                                }
                            }
                        }
                    }
                    Err(error) => conversation.inbox_error = Some(error),
                }
                if restore_focus {
                    app.focus_composer(w, cx);
                }
                app.request_inbox_state(&session);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn toggle_inbox(&mut self, cx: &mut Context<Self>) {
        if self.conversation.inbox_action.is_none() && self.conversation.inbox_edit.is_none() {
            self.conversation.inbox_expanded = !self.conversation.inbox_expanded;
            self.request_inbox_images();
            cx.notify();
        }
    }

    pub(super) fn publish_accessible_inbox(&mut self, cx: &App) {
        let state = self.accessible_inbox(cx);
        if state != self.inbox_accessibility
            && let Ok(value) = serde_wasm_bindgen::to_value(&state)
        {
            self.inbox_accessibility = state;
            update_inbox(value);
        }
    }

    fn accessible_inbox(&self, cx: &App) -> Option<AccessibleInbox> {
        if !self.composer_tools_available() {
            return None;
        }
        let conversation = &self.conversation;
        let rows: Vec<_> = conversation
            .queued_prompts
            .iter()
            .filter(|row| row.mode == SubmitMode::Queue)
            .map(|row| AccessibleInboxRow {
                id: row.item_id.clone(),
                display: row.display.clone(),
                editable: row.edit_text.is_some(),
                attachments: row.attachments.iter().map(QueueAttachment::label).collect(),
            })
            .collect();
        let pending = conversation
            .pending_inbox_submission(SubmitMode::Queue)
            .map(|send| AccessibleInboxRow {
                id: send.id.clone(),
                display: send.display.clone(),
                editable: false,
                attachments: send
                    .attachments
                    .iter()
                    .map(QueueAttachment::label)
                    .collect(),
            });
        if rows.is_empty() && pending.is_none() {
            return None;
        }
        let editor = conversation
            .inbox_edit
            .as_ref()
            .map(|edit| AccessibleInboxEditor {
                id: edit.item_id.clone(),
                draft: edit.editor.read(cx).value().to_owned(),
            });
        let busy = conversation.inbox_action.is_some();
        let locked = busy || editor.is_some();
        Some(AccessibleInbox {
            session: self.active_session.clone()?,
            rows,
            pending,
            editor,
            busy,
            locked,
            expanded: conversation.inbox_expanded || locked,
            running: conversation.running,
            error: conversation.inbox_error.clone(),
        })
    }

    pub(super) fn handle_accessible_inbox_action(
        &mut self,
        action: &str,
        payload: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(payload) = serde_json::from_str::<Value>(payload) else {
            return;
        };
        let Some(session) = payload["session"].as_str() else {
            return;
        };
        if !self.composer_tools_available() || self.active_session.as_deref() != Some(session) {
            return;
        }
        if action == "toggle" {
            self.toggle_inbox(cx);
            return;
        }
        let Some(item) = payload["item"].as_str() else {
            return;
        };
        match action {
            "edit" => self.edit_inbox_row(session, item, false, w, cx),
            "remove" => self.mutate_inbox_row(session, item, InboxAction::Remove, w, cx),
            "steer" => self.mutate_inbox_row(session, item, InboxAction::Steer, w, cx),
            "draft" | "save" | "cancel" => {
                if self.conversation.inbox_action.is_some() {
                    return;
                }
                let Some(edit) = self
                    .conversation
                    .inbox_edit
                    .as_ref()
                    .filter(|edit| edit.item_id == item)
                else {
                    return;
                };
                if action == "cancel" {
                    self.conversation.inbox_edit = None;
                    cx.notify();
                } else if let Some(text) = payload["text"].as_str() {
                    let input = edit.editor.clone();
                    if input.read(cx).value() != text {
                        replace(&input, text, w, cx);
                        cx.notify();
                    }
                    if action == "save" {
                        self.save_inbox_edit(w, cx);
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn render_queue_cards(
        &self,
        width: f32,
        w: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let session = self.active_session.as_ref()?;
        let pending = self
            .conversation
            .pending_inbox_submission(SubmitMode::Queue);
        let rows: Vec<_> = self
            .conversation
            .queued_prompts
            .iter()
            .filter(|row| row.mode == SubmitMode::Queue)
            .collect();
        let count = rows.len() + usize::from(pending.is_some());
        if count == 0 {
            return None;
        }
        let p = dsh(cx);
        let busy = self.conversation.inbox_action.is_some();
        let interaction = busy || self.conversation.inbox_edit.is_some();
        let expanded = self.conversation.inbox_expanded || interaction;
        let mut panel = column()
            .w(px((width - 24.).max(0.)))
            .max_w_full()
            .mx_auto()
            .mb(px(-9.))
            .py(px(2.))
            .rounded_t(px(crate::tokens::Tokens::RADIUS_LG))
            .border(px(0.5))
            .border_color(p.line)
            .bg(p.stage);
        if count > 1 {
            let label = format!("{count} 条排队消息");
            let header = row()
                .id("queue-header")
                .role(gpui::Role::Button)
                .aria_label(label.clone())
                .aria_expanded(expanded)
                .w_full()
                .h(px(36.))
                .px(px(12.))
                .gap(px(8.))
                .items_center()
                .child(icon(Glyph::Menu, 14., p.label_caption))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(13.))
                        .text_color(p.secondary)
                        .child(label),
                )
                .when(!expanded && pending.is_some(), |header| {
                    header.child(queue_status(cx))
                })
                .child(icon(
                    if expanded { Glyph::Down } else { Glyph::Up },
                    12.,
                    p.label_caption,
                ));
            panel = panel.child(if interaction {
                header
            } else {
                dock_controls::dock_action_button(
                    header
                        .tab_index(0)
                        .cursor_pointer()
                        .hover(|style| style.bg(p.hover)),
                    cx.listener(|app, _, _, cx| app.toggle_inbox(cx)),
                )
            });
        }
        let error = self.conversation.inbox_error.as_ref().map(|error| {
            div()
                .px(px(12.))
                .py(px(4.))
                .text_size(px(12.))
                .text_color(p.state_error_primary)
                .child(error.clone())
        });
        if count > 1 && !expanded {
            return Some(panel.children(error));
        }
        let mut list = column()
            .id("queue-list")
            .w_full()
            .max_h(px(180.))
            .overflow_y_scroll();
        for queued in rows {
            let mut line = queue_row(count == 1, cx);
            let edit = self
                .conversation
                .inbox_edit
                .as_ref()
                .filter(|edit| edit.item_id == queued.item_id);
            if let Some(edit) = edit {
                let draft = edit.editor.read(cx).value();
                let height = goal_bar::goal_editor_height(draft, width - 140., w, cx);
                line = line
                    .child(
                        div()
                            .id("queue-inline-editor")
                            .flex_1()
                            .min_w_0()
                            .h(px(height))
                            .px(px(8.))
                            .py(px(3.))
                            .rounded(px(8.))
                            .border(px(0.5))
                            .border_color(p.border_l4)
                            .bg(p.bg_base)
                            .child(edit.editor.clone()),
                    )
                    .child(queue_button(
                        "queue-save",
                        "保存排队消息",
                        Glyph::Check,
                        !busy && !draft.trim().is_empty(),
                        cx,
                        cx.listener(|app, _, w, cx| app.save_inbox_edit(w, cx)),
                    ))
                    .child(queue_button(
                        "queue-cancel",
                        "取消编辑",
                        Glyph::Close,
                        !busy,
                        cx,
                        cx.listener(|app, _, w, cx| app.cancel_inbox_edit(w, cx)),
                    ));
            } else {
                line = line
                    .children(self.render_queue_attachments(&queued.attachments, cx))
                    .child(queue_preview(&queued.display, cx));
                for (key, name, glyph, enabled) in [
                    (
                        "edit",
                        "编辑排队消息",
                        Glyph::Edit,
                        !busy && queued.edit_text.is_some(),
                    ),
                    ("remove", "删除排队消息", Glyph::Trash, !busy),
                    (
                        "steer",
                        "插话发送",
                        Glyph::Send,
                        !busy && self.conversation.running,
                    ),
                ] {
                    let session = session.clone();
                    let item = queued.item_id.clone();
                    line = line.child(queue_button(
                        format!("queue-{key}-{item}"),
                        name,
                        glyph,
                        enabled,
                        cx,
                        cx.listener(move |app, _, w, cx| match key {
                            "edit" => app.edit_inbox_row(&session, &item, true, w, cx),
                            "remove" => {
                                app.mutate_inbox_row(&session, &item, InboxAction::Remove, w, cx)
                            }
                            _ => app.mutate_inbox_row(&session, &item, InboxAction::Steer, w, cx),
                        }),
                    ));
                }
            }
            list = list.child(line);
        }
        if let Some(send) = pending {
            list = list.child(
                queue_row(count == 1, cx)
                    .children(self.render_queue_attachments(&send.attachments, cx))
                    .child(queue_preview(&send.display, cx))
                    .child(queue_status(cx)),
            );
        }
        Some(panel.child(list).children(error))
    }
}

pub(super) struct InboxEdit {
    pub(super) item_id: String,
    editor: gpui::Entity<Editor>,
    _subscription: Subscription,
}

pub(super) struct InboxSubmission {
    pub(super) id: String,
    pub(super) admitted: bool,
    pub(super) mode: SubmitMode,
    pub(super) display: String,
    pub(super) attachments: Vec<QueueAttachment>,
}

enum InboxAction {
    Edit(String),
    Remove,
    Steer,
}

fn queue_row(single: bool, cx: &App) -> gpui::Div {
    row()
        .w_full()
        .min_h(px(36.))
        .px(px(12.))
        .py(px(4.))
        .gap(px(6.))
        .items_center()
        .when(single, |row| {
            row.child(icon(Glyph::Menu, 14., dsh(cx).label_caption))
        })
}

fn queue_preview(display: &str, cx: &App) -> gpui::Div {
    let text = display.split_whitespace().collect::<Vec<_>>().join(" ");
    div()
        .flex_1()
        .min_w_0()
        .truncate()
        .text_size(px(13.))
        .text_color(dsh(cx).secondary)
        .child(text)
}

fn queue_status(cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .text_size(px(12.))
        .text_color(dsh(cx).faint)
        .child("发送中…")
}

fn queue_button(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    glyph: Glyph,
    enabled: bool,
    cx: &App,
    action: impl Fn(&(), &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let p = dsh(cx);
    let button = row()
        .id(id)
        .role(gpui::Role::Button)
        .aria_label(label)
        .size(px(28.))
        .flex_none()
        .rounded(px(8.))
        .items_center()
        .justify_center()
        .child(icon(
            glyph,
            14.,
            if enabled {
                p.secondary
            } else {
                p.faint.opacity(0.45)
            },
        ));
    if enabled {
        dock_controls::dock_action_button(
            button
                .tab_index(0)
                .cursor_pointer()
                .hover(|style| style.bg(p.hover)),
            action,
        )
    } else {
        button
    }
}
