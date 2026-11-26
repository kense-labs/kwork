use super::{ChatSession, GoalSetOptions, GoalSnapshot, KarBridgeEvent, WorkerState};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoalAction {
    Inspect,
    Set(String),
    Pause,
    Resume,
    Complete,
    Clear,
}

impl GoalAction {
    pub(crate) fn available(&self, status: &str) -> bool {
        match self {
            Self::Pause => matches!(
                status,
                "active" | "blocked" | "usageLimited" | "budgetLimited"
            ),
            Self::Resume => matches!(
                status,
                "paused" | "blocked" | "usageLimited" | "budgetLimited"
            ),
            Self::Complete => status != "complete",
            _ => true,
        }
    }
}

async fn execute(
    session: &ChatSession,
    action: &GoalAction,
) -> Result<Option<GoalSnapshot>, String> {
    let current = session.read_goal().await.map_err(|e| e.to_string())?;
    if matches!(action, GoalAction::Inspect) {
        return Ok(current);
    }
    let (objective, status) = match action {
        GoalAction::Set(objective) => {
            if current.as_ref().is_some_and(|g| g.objective == *objective) {
                return Ok(current);
            }
            let new_goal = current.as_ref().is_none_or(|g| g.status == "complete");
            (objective.clone(), new_goal.then(|| "active".to_string()))
        }
        _ => {
            let goal = current.ok_or_else(|| "当前会话没有目标".to_owned())?;
            if !action.available(&goal.status) {
                return Err(format!("目标状态已变更（{}），请重新查看目标", goal.status));
            }
            let status = match action {
                GoalAction::Pause => "paused",
                GoalAction::Resume => "active",
                GoalAction::Complete => "complete",
                GoalAction::Clear => {
                    session.clear_goal().await.map_err(|e| e.to_string())?;
                    if session
                        .read_goal()
                        .await
                        .map_err(|e| e.to_string())?
                        .is_some()
                    {
                        return Err("智能体尚未确认清除目标，请重新查看目标".into());
                    }
                    return Ok(None);
                }
                _ => unreachable!(),
            };
            (goal.objective, Some(status.to_owned()))
        }
    };
    // Omit a status on edits so that a paused goal stays paused; omit budget
    // unless the user actually supplies one.
    session
        .set_goal(GoalSetOptions {
            objective,
            status,
            token_budget: None,
        })
        .await
        .map(Some)
        .map_err(|e| e.to_string())
}

impl WorkerState {
    pub(super) fn apply_goal(&self, session: Arc<ChatSession>, action: GoalAction) {
        let chat_id = session.info().id;
        let events = self.evt_tx.clone();
        tokio::spawn(async move {
            let result = execute(&session, &action).await;
            let _ = events
                .send(KarBridgeEvent::GoalResult {
                    chat_id,
                    action,
                    result,
                })
                .await;
        });
    }
}
