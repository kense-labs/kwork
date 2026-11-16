//! The composer's GoalBar: current goal phase, objective, and its verbs.
//!
//! Official `ui-goal` docks a 36px strip above the input: a goal glyph, the
//! phase label, the ellipsized objective, and pause/resume/edit/clear. The
//! durable phase comes from the `goal` projection and activation is
//! process-local, so both arrive in one live view the Host re-reads after every
//! mutation. Creation still belongs to `/goal`; the strip only manages the
//! current goal.

use super::*;

impl DshApp {
    /// Read the current goal when a session opens.
    pub(super) fn request_goal_state(&mut self, session: &str) {
        if !self.conversation.acp_open {
            return;
        }
        invoke_with_reply("goal_state", "goal_state", json!({"sessionId": session}));
    }

    /// Apply a `goal_state` / `goal_action` reply.
    pub(super) fn finish_goal_state(
        &mut self,
        owner: Option<&str>,
        result: Result<Value, String>,
        _w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = owner else {
            return;
        };
        let parsed = match result {
            Ok(reply) => Ok(parse_goal_view(&reply["goal"])),
            Err(error) => Err(error),
        };
        self.apply_goal_reply(session, parsed);
        cx.notify();
    }

    /// Apply a live `gpui_goal_changed` notification.
    pub(super) fn handle_goal_changed(
        &mut self,
        session: &str,
        update: &Value,
        cx: &mut Context<Self>,
    ) {
        let goal = parse_goal_view(&update["goal"]);
        let selected = self.active_session.as_deref() == Some(session);
        let revision = goal.as_ref().map(|goal| goal.revision);
        let active = goal
            .as_ref()
            .is_some_and(|goal| goal.phase == crate::goal::GoalPhase::Active);
        self.apply_goal_reply(session, Ok(goal));
        cx.notify();
        // A new goal revision may have opened an autonomous round that already
        // failed before this client saw it (the ACP event can be emitted before
        // the conversation is attached). Re-read the durable history once while
        // the session is idle; the replayed `turn/end` failure keeps its row and
        // the strip's `goal_error` explains the failed round.
        if !selected || !active {
            return;
        }
        let first_sync = self
            .session_conversation(session)
            .is_some_and(|conversation| conversation.goal_synced_revision != revision);
        if !first_sync {
            return;
        }
        if let Some(conversation) = self.conversation_mut(session) {
            conversation.goal_synced_revision = revision;
        }
        let session = session.to_owned();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(2))
                .await;
            this.update(cx, |app, cx| {
                if app.active_session.as_deref() != Some(session.as_str())
                    || app
                        .session_conversation(&session)
                        .is_none_or(|conversation| {
                            conversation.running || conversation.history_loading
                        })
                {
                    return;
                }
                app.request_session_history(cx);
            })
            .ok();
        })
        .detach();
    }

    fn apply_goal_reply(
        &mut self,
        session: &str,
        result: Result<Option<crate::goal::GoalView>, String>,
    ) {
        let Some(conversation) = self.conversation_mut(session) else {
            return;
        };
        conversation.goal_busy = false;
        match result {
            Ok(goal) => {
                conversation.goal_error = None;
                crate::goal::apply_goal_view(&mut conversation.goal, goal);
            }
            Err(error) => conversation.goal_error = Some(error),
        }
    }

    /// Run one goal verb through the Host goal service (compare-and-set on the
    /// revision the strip is showing).
    pub(super) fn run_goal_action(
        &mut self,
        action: &'static str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.active_session.clone() else {
            return;
        };
        let Some(goal) = self.conversation.goal.clone() else {
            return;
        };
        if self.conversation.goal_busy {
            return;
        }
        self.conversation.goal_busy = true;
        self.conversation.goal_error = None;
        cx.notify();
        cx.spawn_in(w, async move |this, cx| {
            let result = bridge::invoke(
                "goal_action",
                json!({
                    "sessionId": session,
                    "action": action,
                    "revision": goal.revision,
                }),
            )
            .await;
            let _ = this.update_in(cx, |this, w, cx| {
                this.finish_goal_state(Some(&session), result, w, cx);
            });
        })
        .detach();
    }

    /// Load the objective back into the composer as `/goal edit …`: the Host
    /// command owns the rewrite, so the strip needs no inline editor.
    pub(super) fn edit_goal_objective(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(goal) = self.conversation.goal.clone() else {
            return;
        };
        let line = format!("/goal edit {}", goal.objective.trim());
        replace(&self.composer, &line, w, cx);
        self.composer_menu = None;
        self.focus_composer(w, cx);
        cx.notify();
    }
}

/// One Host goal view, tolerating `null` for "no goal".
fn parse_goal_view(value: &Value) -> Option<crate::goal::GoalView> {
    if value.is_null() {
        return None;
    }
    crate::goal::GoalView::from_json(value)
}
