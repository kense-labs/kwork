use kar::{
    RequestKind,
    chat::{ChatTurn, Item, Role, TurnStatus},
};

use crate::{
    kar_bridge::ProjectedChatState,
    types::{ChatAccessMode, FeedConnectionState, PendingPromptAttempt},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Activity {
    Starting,
    Thinking,
    Working,
    Replying,
}

pub(super) fn current<'a>(
    access: &ChatAccessMode,
    projected: &ProjectedChatState,
    starting_draft: bool,
    attempts: impl IntoIterator<Item = &'a PendingPromptAttempt>,
    connection: &FeedConnectionState,
) -> Option<Activity> {
    match access {
        ChatAccessMode::NoneSelected => return starting_draft.then_some(Activity::Starting),
        ChatAccessMode::ArchivedReadOnly { .. } => return None,
        ChatAccessMode::RemoteController { .. } | ChatAccessMode::HttpReadOnly { .. }
            if !matches!(connection, FeedConnectionState::Connected { .. }) =>
        {
            return None;
        }
        _ => {}
    }

    let selected = access.selected_chat_id();
    let same_chat = if matches!(access, ChatAccessMode::DemoShowcase) {
        projected.is_demo
    } else {
        !projected.is_demo && selected.is_some() && selected == projected.chat_id
    };
    if same_chat {
        if projected.pending_requests.iter().any(|request| {
            matches!(
                request.kind,
                RequestKind::Permission | RequestKind::Question
            )
        }) {
            return None;
        }
        if let Some(turn) = projected
            .turns
            .last()
            .filter(|t| t.status == TurnStatus::Running)
        {
            let root = projected.info.as_ref().map(|info| info.native_id.as_str());
            return Some(turn_activity(turn, root));
        }
    }

    // The local send RPC may settle after Kar has published the terminal turn.
    // Only a request absent from its own projection can bridge the pre-turn gap.
    let pending_local_send = matches!(access, ChatAccessMode::WriteController { .. })
        && attempts.into_iter().any(|attempt| {
            attempt.in_flight
                && attempt.chat_id == selected
                && !(same_chat
                    && projected
                        .turns
                        .iter()
                        .rev()
                        .any(|turn| turn.id == attempt.request_id))
        });
    pending_local_send.then_some(Activity::Working)
}

fn turn_activity(turn: &ChatTurn, root: Option<&str>) -> Activity {
    // A provider can leave a reasoning span or a child's reply open after it
    // moves on. Only the latest root segment can advertise thinking or writing.
    let mut fallback = Activity::Thinking;
    for entry in turn.items.iter().rev() {
        let session = entry.source.session_id.as_deref();
        if !entry.source.path.is_empty()
            || session.is_some_and(|id| turn.child_sessions.contains_key(id))
            || session.zip(root).is_some_and(|(id, root)| id != root)
        {
            fallback = Activity::Working;
            continue;
        }
        return match &entry.item {
            Item::Message {
                role: Role::User, ..
            } => continue,
            Item::Message {
                role: Role::Reasoning,
                complete: false,
                ..
            } => fallback,
            Item::Message {
                role: Role::Assistant,
                text,
                complete: false,
                ..
            } if !text.trim().is_empty() => Activity::Replying,
            _ => Activity::Working,
        };
    }
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;
    use kar::{
        chat::TimelineItem,
        observe::{Agent, Tool},
    };
    use serde_json::json;
    use uuid::Uuid;

    fn turn() -> ChatTurn {
        serde_json::from_value(json!({
            "id": Uuid::new_v4(), "started_ms": 0, "status": "running", "items": []
        }))
        .unwrap()
    }

    fn message(session: &str, role: Role, complete: bool) -> TimelineItem {
        TimelineItem {
            source: Agent {
                session_id: Some(session.into()),
                path: vec![],
            },
            item: Item::Message {
                role,
                text: "text".into(),
                provider_id: None,
                complete,
            },
        }
    }

    #[test]
    fn tool_boundaries_and_final_text_do_not_end_a_live_turn() {
        let mut turn = turn();
        let phase = |t: &ChatTurn| turn_activity(t, Some("main"));
        turn.items.push(message("main", Role::Reasoning, false));
        assert_eq!(phase(&turn), Activity::Thinking);
        for id in ["read", "write"] {
            turn.items.push(TimelineItem {
                source: turn.items[0].source.clone(),
                item: Item::Tool {
                    id: id.into(),
                    activity: Tool::default(),
                },
            });
            assert_eq!(phase(&turn), Activity::Working);
            if let Item::Tool { activity, .. } = &mut turn.items.last_mut().unwrap().item {
                activity.finished = true;
            }
            assert_eq!(phase(&turn), Activity::Working);
        }
        turn.items.push(message("main", Role::Assistant, false));
        assert_eq!(phase(&turn), Activity::Replying);
        turn.items.push(message("main", Role::Assistant, true));
        assert_eq!(phase(&turn), Activity::Working);
    }

    #[test]
    fn a_lingering_child_does_not_mask_the_root_reply_updated_in_place() {
        let mut turn = turn();
        turn.items.push(message("main", Role::Reasoning, false));
        turn.items.push(message("worker", Role::Assistant, false));
        assert_eq!(turn_activity(&turn, Some("main")), Activity::Working);
        turn.child_sessions.insert("worker".into(), "main".into());
        assert_eq!(turn_activity(&turn, None), Activity::Working);
        turn.items[0] = message("main", Role::Assistant, false);
        assert_eq!(turn_activity(&turn, Some("main")), Activity::Replying);
        turn.items[0].source.session_id = None;
        assert_eq!(turn_activity(&turn, Some("main")), Activity::Replying);
    }

    #[test]
    fn only_the_selected_unprojected_send_bridges_startup() {
        let chat = Uuid::new_v4();
        let mode = ChatAccessMode::WriteController { chat_id: chat };
        let mut state = ProjectedChatState {
            chat_id: Some(chat),
            ..Default::default()
        };
        let mut turn = turn();
        let mut send = PendingPromptAttempt {
            chat_id: Some(chat),
            request_id: turn.id,
            raw_user_text: String::new(),
            sent_payload: String::new(),
            target: None,
            attachments: vec![],
            attempts: 1,
            in_flight: true,
            last_error: None,
        };
        let conn = &FeedConnectionState::Idle;
        assert_eq!(
            current(&ChatAccessMode::NoneSelected, &state, true, [&send], conn),
            Some(Activity::Starting)
        );
        assert_eq!(
            current(&mode, &state, false, [&send], conn),
            Some(Activity::Working)
        );
        turn.items.push(message("child", Role::Assistant, false));
        state.turns.push(turn);
        for terminal in [
            TurnStatus::Completed,
            TurnStatus::Failed,
            TurnStatus::Aborted,
            TurnStatus::Interrupted,
        ] {
            state.turns[0].status = terminal;
            assert_eq!(current(&mode, &state, false, [&send], conn), None);
        }
        state.turns[0].status = TurnStatus::Running;
        let other = ChatAccessMode::WriteController {
            chat_id: Uuid::new_v4(),
        };
        assert_eq!(current(&other, &state, true, [&send], conn), None);
        assert_eq!(
            current(&ChatAccessMode::NoneSelected, &state, false, [&send], conn),
            None
        );
        state.turns.clear();
        state.queued_inputs.push(
            serde_json::from_value(json!({
                "id": Uuid::new_v4(), "status": "pending", "text": "next turn"
            }))
            .unwrap(),
        );
        send.in_flight = false;
        assert_eq!(current(&mode, &state, false, [&send], conn), None);
    }

    #[test]
    fn decisions_and_remote_connection_gate_live_activity() {
        let chat = Uuid::new_v4();
        let mode = ChatAccessMode::HttpReadOnly {
            url: "test".into(),
            chat_id: Some(chat),
        };
        let mut state = ProjectedChatState {
            chat_id: Some(chat),
            turns: vec![turn()],
            ..Default::default()
        };
        let connected = FeedConnectionState::Connected {
            cursor: None,
            duplicates_ignored: 0,
            gaps_recovered: 0,
        };
        let remote_activity = |mode: &ChatAccessMode, state: &ProjectedChatState| {
            current(mode, state, false, [], &connected)
        };
        assert_eq!(remote_activity(&mode, &state), Some(Activity::Thinking));
        let unbound = ChatAccessMode::HttpReadOnly {
            url: "new".into(),
            chat_id: None,
        };
        assert_eq!(remote_activity(&unbound, &state), None);
        let reconnecting = FeedConnectionState::Reconnecting {
            last_cursor: None,
            reason: String::new(),
            attempt: 1,
        };
        assert_eq!(current(&mode, &state, false, [], &reconnecting), None);
        state.pending_requests.push(
            serde_json::from_value(json!({
                "cursor": {"stream_id":chat,"after_seq":1}, "id":1, "method":"request",
                "kind":"service", "session_id":null, "agent_path":[], "body":{}
            }))
            .unwrap(),
        );
        assert_eq!(remote_activity(&mode, &state), Some(Activity::Thinking));
        for kind in [RequestKind::Permission, RequestKind::Question] {
            state.pending_requests[0].kind = kind;
            assert_eq!(remote_activity(&mode, &state), None);
        }
    }
}
