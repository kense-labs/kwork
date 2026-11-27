//! Kar's rewind boundary is inclusive: it retains the selected completed turn.
use super::*;

#[derive(Debug, Clone, Copy)]
pub enum UndoTarget {
    KeepThrough(Uuid),
    ArchiveAndRestart,
}

impl UndoTarget {
    /// Return the actual retained boundary and the number of turns leaving the active conversation.
    pub(crate) fn before(turns: &[ChatTurn], index: usize) -> (Self, usize) {
        match turns[..index]
            .iter()
            .rposition(|t| t.status == TurnStatus::Completed)
        {
            Some(i) => (Self::KeepThrough(turns[i].id), turns.len() - i - 1),
            None => (Self::ArchiveAndRestart, turns.len()),
        }
    }
}

pub(crate) fn input_outstanding(input: &QueuedInput) -> bool {
    matches!(input.status, InputStatus::Pending | InputStatus::Sending)
        || (input.status == InputStatus::Uncertain && !input.uncertainty_acknowledged)
}

#[derive(Debug, Clone)]
pub enum UndoFailure {
    NotWriter,
    Changed,
    StoreUnavailable,
    Backend(String),
}

impl From<kar::Error> for UndoFailure {
    fn from(error: kar::Error) -> Self {
        Self::Backend(error.to_string())
    }
}

pub(super) fn validate(
    state: &WorkerState,
    chat: Uuid,
    expected_head: Uuid,
) -> Result<ChatStore, UndoFailure> {
    let writer = state
        .hosted_chat
        .as_ref()
        .map(|host| host.chat)
        .or_else(|| {
            state
                .active_controller
                .as_ref()
                .map(|session| session.info().id)
        });
    if state.selected_chat_id != Some(chat) || writer != Some(chat) {
        return Err(UndoFailure::NotWriter);
    }
    let store = state.store.clone().ok_or(UndoFailure::StoreUnavailable)?;
    let history = store.history(chat)?;
    let idle_head = history
        .turns
        .last()
        .is_some_and(|t| t.id == expected_head && t.status != TurnStatus::Running);
    if !idle_head
        || !history.pending_requests.is_empty()
        || store.queued_inputs(chat)?.iter().any(input_outstanding)
    {
        return Err(UndoFailure::Changed);
    }
    Ok(store)
}

#[derive(Debug, Clone)]
pub enum UndoOutcome {
    Archived,
    Rewound(usize),
}

pub(super) async fn apply(
    state: &mut WorkerState,
    store: ChatStore,
    chat: Uuid,
    target: UndoTarget,
    expected_head: Uuid,
) -> Result<UndoOutcome> {
    let keep_through = match target {
        UndoTarget::ArchiveAndRestart => {
            state.stop_feed();
            state.close_controller(false).await;
            sidebar::change(state, chat, None, Some(SidebarAction::Archive(true))).await?;
            return Ok(UndoOutcome::Archived);
        }
        UndoTarget::KeepThrough(turn) => turn,
    };
    let request = kar::chat::RewindRequest {
        id: Uuid::new_v4(),
        turn: keep_through,
        expected_head: Some(expected_head),
        policy: kar::chat::ForkPolicy::PreferNative,
        restore_files: false,
    };
    let receipt = if let Some(host) = state.hosted_chat.as_ref().filter(|host| host.chat == chat) {
        serde_json::from_value(host.post("rewinds", serde_json::json!(request)).await?)?
    } else {
        state.stop_feed();
        state.close_controller(false).await;
        store.rewind(chat, request).await?
    };
    Ok(UndoOutcome::Rewound(receipt.removed_turns.len()))
}

#[cfg(test)]
mod tests {
    use super::{TurnStatus, UndoTarget, Uuid, build_demo_seed_turns};

    #[test]
    fn undo_removes_the_selected_turn_and_reports_the_full_suffix() {
        let mut first = build_demo_seed_turns().remove(0);
        first.status = TurnStatus::Completed;
        let mut failed = first.clone();
        failed.id = Uuid::new_v4();
        failed.status = TurnStatus::Failed;
        let mut last = first.clone();
        last.id = Uuid::new_v4();
        let turns = [first, failed, last];
        assert!(matches!(
            UndoTarget::before(&turns, 0),
            (UndoTarget::ArchiveAndRestart, 3)
        ));
        for index in [1, 2] {
            assert!(
                matches!(UndoTarget::before(&turns, index), (UndoTarget::KeepThrough(id), 2) if id == turns[0].id)
            );
        }
        let turns = [turns[0].clone(), turns[2].clone()];
        assert!(
            matches!(UndoTarget::before(&turns, 1), (UndoTarget::KeepThrough(id), 1) if id == turns[0].id)
        );
    }
}
