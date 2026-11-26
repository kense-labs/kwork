use super::{ChatSession, GoalSetOptions, GoalSnapshot, KarBridgeEvent, WorkerState};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalEdit {
    pub objective: String,
    pub token_budget: Option<u64>,
}

impl GoalEdit {
    pub(crate) fn command(&self) -> String {
        if let Some(tokens) = self.token_budget {
            format!("/goal --budget {tokens} {}", self.objective)
        } else {
            format!("/goal set {}", self.objective)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoalAction {
    Inspect,
    Set(GoalEdit),
    Budget(u64),
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
            Self::Complete | Self::Budget(_) => status != "complete",
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
    let (objective, status, token_budget) = match action {
        GoalAction::Set(edit) => {
            if current.as_ref().is_some_and(|g| {
                g.objective == edit.objective
                    && edit
                        .token_budget
                        .is_none_or(|budget| g.token_budget.flatten() == Some(budget))
            }) {
                return Ok(current);
            }
            let new_goal = current.as_ref().is_none_or(|g| g.status == "complete");
            (
                edit.objective.clone(),
                new_goal.then(|| "active".to_string()),
                edit.token_budget,
            )
        }
        _ => {
            let goal = current.ok_or_else(|| "当前会话没有目标".to_owned())?;
            if !action.available(&goal.status) {
                return Err(format!("目标状态已变更（{}），请重新查看目标", goal.status));
            }
            let (status, budget) = match action {
                GoalAction::Pause => (Some("paused"), None),
                GoalAction::Resume => (Some("active"), None),
                GoalAction::Complete => (Some("complete"), None),
                GoalAction::Budget(tokens) => (None, Some(*tokens)),
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
            (goal.objective, status.map(str::to_owned), budget)
        }
    };
    // Omit a status on edits so that a paused goal stays paused; omit budget
    // unless the user actually supplies one.
    session
        .set_goal(GoalSetOptions {
            objective,
            status,
            token_budget,
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
