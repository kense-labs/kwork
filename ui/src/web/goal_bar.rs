//! The composer's GoalBar: current goal phase, objective, and its verbs.
//!
//! Official `ui-goal` docks a 36px strip above the input: a goal glyph, the
//! phase label, the ellipsized objective, and pause/resume/edit/clear. The
//! durable phase comes from the `goal` projection and activation is
//! process-local, so both arrive in one live view the Host re-reads after every
//! mutation. Creation still belongs to `/goal`; the strip only manages the
//! current goal.

use super::*;

pub(super) struct GoalEdit {
    pub(super) editor: gpui::Entity<Editor>,
    _subscription: Subscription,
}

impl DshApp {
    /// Read the current goal when a session opens.
    pub(super) fn request_goal_state(&mut self, session: &str) {
        if !self.conversation.acp_open {
            return;
        }
        invoke_with_reply("goal_state", "goal_state", json!({"sessionId": session}));
    }

    /// An initial read cannot replace a fresher live notification.
    pub(super) fn finish_goal_state(
        &mut self,
        owner: Option<&str>,
        result: Result<Value, String>,
        cx: &mut Context<Self>,
    ) {
        if let (Some(session), Ok(reply)) = (owner, result)
            && self.apply_goal_snapshot(session, &reply)
        {
            cx.notify();
        }
    }

    /// Apply a live `gpui_goal_changed` notification.
    pub(super) fn handle_goal_changed(
        &mut self,
        session: &str,
        update: &Value,
        cx: &mut Context<Self>,
    ) {
        if !self.apply_goal_snapshot(session, update) {
            return;
        }
        cx.notify();
        let selected = self.active_session.as_deref() == Some(session);
        // A new goal revision may have opened an autonomous round that already
        // failed before this client saw it (the ACP event can be emitted before
        // the conversation is attached). Re-read the durable history once while
        // the session is idle; the replayed `turn/end` failure keeps its row and
        // the strip's `goal_error` explains the failed round.
        if !selected {
            return;
        }
        let conversation = &mut self.conversation;
        let Some(goal) = conversation
            .goal
            .as_ref()
            .filter(|goal| goal.phase == crate::goal::GoalPhase::Active)
        else {
            return;
        };
        let revision = Some((goal.id.clone(), goal.revision));
        if conversation.goal_synced_revision == revision {
            return;
        }
        conversation.goal_synced_revision = revision;
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

    fn apply_goal_snapshot(&mut self, session: &str, snapshot: &Value) -> bool {
        let Some(conversation) = self.conversation_mut(session) else {
            return false;
        };
        let previous = conversation.goal.as_ref().map(|goal| goal.id.clone());
        if !crate::goal::apply_goal_snapshot(
            &mut conversation.goal,
            &mut conversation.goal_sequence,
            snapshot,
        ) {
            return false;
        }
        if previous.as_ref() != conversation.goal.as_ref().map(|goal| &goal.id) {
            conversation.goal_edit = None;
            conversation.goal_error = None;
        }
        true
    }

    /// Run one mutation with the exact identity and revision shown in the strip.
    pub(super) fn run_goal_action(
        &mut self,
        action: &'static str,
        objective: Option<String>,
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
                    "goalId": goal.id,
                    "revision": goal.revision,
                    "objective": objective,
                }),
            )
            .await;
            let _ = this.update_in(cx, |app, w, cx| {
                let selected = app.active_session.as_deref() == Some(&session);
                let Some(conversation) = app.conversation_mut(&session) else {
                    return;
                };
                conversation.goal_busy = false;
                match result {
                    Ok(reply) => {
                        if let Some(objective) = objective.as_deref()
                            && conversation.goal_edit.as_ref().is_some_and(|edit| {
                                edit.editor.read(cx).value().trim() == objective
                            })
                        {
                            let edit = conversation.goal_edit.take();
                            if selected
                                && edit.is_some_and(|edit| edit.editor.read(cx).focus.is_focused(w))
                            {
                                app.focus_composer(w, cx);
                            }
                        }
                        app.apply_goal_snapshot(&session, &reply);
                    }
                    Err(error) => {
                        if conversation
                            .goal
                            .as_ref()
                            .is_some_and(|current| current.id == goal.id)
                        {
                            conversation.goal_error = Some(error);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn edit_goal_objective(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.conversation.goal_busy || self.conversation.goal_edit.is_some() {
            return;
        }
        let Some(goal) = &self.conversation.goal else {
            return;
        };
        let input = editor("目标内容".into(), InputMode::InlineMulti, w, cx);
        input.update(cx, |editor, _| editor.font_size = 13.);
        replace(&input, goal.objective.clone(), w, cx);
        let subscription = cx.subscribe_in(&input, w, |app, sender, event, w, cx| {
            if !app
                .conversation
                .goal_edit
                .as_ref()
                .is_some_and(|edit| edit.editor == *sender)
            {
                return;
            }
            match event {
                EditEvent::Submitted => app.save_goal_objective(w, cx),
                EditEvent::Escape => app.cancel_goal_edit(w, cx),
                EditEvent::Changed => cx.notify(),
                _ => {}
            }
        });
        self.conversation.goal_error = None;
        self.conversation.goal_edit = Some(GoalEdit {
            editor: input.clone(),
            _subscription: subscription,
        });
        self.composer_menu = None;
        input.read(cx).focus.clone().focus(w, cx);
        cx.notify();
    }

    pub(super) fn save_goal_objective(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.conversation.goal_edit else {
            return;
        };
        let objective = edit.editor.read(cx).value().trim().to_owned();
        if !objective.is_empty() {
            self.run_goal_action("edit", Some(objective), w, cx);
        }
    }

    pub(super) fn cancel_goal_edit(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if !self.conversation.goal_busy && self.conversation.goal_edit.take().is_some() {
            self.conversation.goal_error = None;
            self.focus_composer(w, cx);
            cx.notify();
        }
    }
}

pub(super) fn goal_editor_height(value: &str, width: f32, w: &Window, cx: &App) -> f32 {
    let run = document_fonts::interface_style(w, cx).to_run(value.len());
    let rows = w
        .text_system()
        .shape_text(
            value.to_owned().into(),
            px(13.),
            &[run],
            Some(px(width.max(1.))),
            None,
        )
        .map_or(1, |lines| {
            lines
                .iter()
                .map(|line| line.wrap_boundaries().len() + 1)
                .sum()
        });
    (rows as f32 * 20. + 7.).clamp(28., 127.)
}
