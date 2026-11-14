//! Wire Host-owned reading state into menus and the visible conversation view.

use super::*;

impl DshApp {
    pub(super) fn apply_session_reading(&mut self, snapshot: &Value) {
        self.session_reading.replace(snapshot);
    }

    pub(super) fn accept_session_reading(&mut self, record: &Value) {
        self.session_reading.accept(record);
    }

    /// Explicit marks wait for the Host's broadcast; never change them optimistically.
    pub(super) fn toggle_session_read(&mut self, id: &str, cx: &mut Context<Self>) {
        let unread = self.session_reading.completion_unread(id, false);
        if let Some(args) = self.session_reading.mark(id, !unread) {
            invoke("session_read_state", args);
        } else {
            self.push_toast("未能确认阅读状态，请重试", cx);
        }
    }

    pub(super) fn toggle_active_session_read(
        &mut self,
        embedded: Option<(DockPaneId, RightPanelTab)>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if embedded.is_some()
            || self.focused_right_pane(w, cx).is_some()
            || self.page != WorkspacePage::Conversation
            || self.show_welcome
            || self.subagent_view.is_some()
        {
            return;
        }
        if let Some(id) = self.active_session.clone().filter(|id| {
            !self.archived_sessions.contains(id)
                && self
                    .sessions
                    .iter()
                    .any(|session| session.id == *id && !session.blank)
        }) {
            self.toggle_session_read(&id, cx);
        }
    }

    pub(super) fn schedule_reading_ack(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.reading_ack_scheduled {
            return;
        }
        self.reading_ack_scheduled = true;
        let owner = cx.weak_entity();
        w.defer(cx, move |w, cx| {
            if let Some(owner) = owner.upgrade() {
                owner.update(cx, |app, cx| {
                    app.reading_ack_scheduled = false;
                    app.acknowledge_visible_session(w, cx);
                });
            }
        });
    }

    fn acknowledge_visible_session(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if !w.is_window_active()
            || self.focused_right_pane(w, cx).is_some()
            || self.shortcuts.open
            || self.show_settings
            || self.show_welcome
            || self.page != WorkspacePage::Conversation
            || self.subagent_view.is_some()
            || self.confirm_permission.is_some()
            || self.workspace_rename.is_some()
            || self.workspace_delete.is_some()
            || self.session_rename.is_some()
            || self.image_viewer.is_some()
            || self.file_open_error.is_some()
            || self.stats_dialog.is_some()
            || self.turn_usage_dialog.is_some()
            || self.session_feedback.visible()
            || self.session_downloads.visible()
            || !self.conversation.acp_open
        {
            return;
        }
        let Some(id) = self.active_session.clone() else {
            return;
        };
        let received = self.conversation.turn_process.received_turn();
        if let Some(args) = self.session_reading.acknowledge(&id, true, true, received) {
            invoke_with_reply("session_read_state", "session_read_ack", args);
        }
    }

    pub(super) fn finish_session_read_ack(
        &mut self,
        _result: Result<Value, String>,
        _cx: &mut Context<Self>,
    ) {
        // Background acknowledgement is silent. A focus change or a later
        // completion permits another attempt without changing the visible state.
        self.session_reading.finish_acknowledge();
    }
}
