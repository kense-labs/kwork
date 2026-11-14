//! Durable and live workflow events projected into one run row.

use crate::state::{ChatEntry, EntryRole};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A child Agent's final status; absence means the workflow is still running.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct Member {
    pub(crate) seq: u64,
    pub(crate) label: String,
    pub(crate) child_id: String,
    pub(crate) phase: Option<String>,
    pub(crate) outcome: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Run {
    pub(crate) name: String,
    pub(crate) members: Vec<Member>,
    pub(crate) stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) interrupted: bool,
}

fn key(run_id: &Value) -> String {
    format!(
        "workflow-run-{}",
        run_id
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| run_id.to_string())
    )
}

/// Fold one event without relying on screen order. A late run-start attaches
/// to an existing run rather than creating a duplicate transcript row.
pub(crate) fn interrupt(entries: &mut [ChatEntry]) {
    for entry in entries
        .iter_mut()
        .filter(|entry| entry.role == EntryRole::WorkflowRun)
    {
        if let Ok(mut run) = serde_json::from_str::<Run>(&entry.text) {
            run.interrupted = true;
            entry.text = serde_json::to_string(&run).unwrap_or_else(|_| "{}".to_owned());
        }
    }
}

pub(crate) fn status(run: &Run) -> &str {
    if run.stop_reason.is_none() && run.interrupted {
        "interrupted"
    } else {
        run.stop_reason.as_deref().unwrap_or("running")
    }
}

pub(crate) fn member_status<'a>(run: &'a Run, member: &'a Member) -> &'a str {
    member.outcome.as_deref().unwrap_or_else(|| status(run))
}

pub(crate) fn apply(entries: &mut Vec<ChatEntry>, event: &Value, at_ms: Option<f64>) {
    let Some(kind) = event.get("type").and_then(Value::as_str) else {
        return;
    };
    let data = event.get("data").cloned().unwrap_or(Value::Null);
    let run_id = data.get("runId").cloned().unwrap_or(Value::Null);
    if run_id.is_null() {
        return;
    }
    let key = key(&run_id);
    let index = entries.iter().position(|entry| entry.key == key);
    let mut run = index
        .and_then(|index| serde_json::from_str::<Run>(&entries[index].text).ok())
        .unwrap_or_default();
    match kind {
        "tool-workflow/run-start" => {
            run.name = data
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
        }
        "tool-workflow/agent-start" => {
            let Some(seq) = data.get("seq").and_then(Value::as_u64) else {
                return;
            };
            if run.members.iter().any(|member| member.seq == seq) {
                return;
            }
            let Some(child_id) = data.get("childId").and_then(Value::as_str) else {
                return;
            };
            run.members.push(Member {
                seq,
                label: data
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                child_id: child_id.to_owned(),
                phase: data.get("phase").and_then(Value::as_str).map(str::to_owned),
                outcome: None,
            });
        }
        "tool-workflow/agent-end" => {
            let Some(seq) = data.get("seq").and_then(Value::as_u64) else {
                return;
            };
            let Some(member) = run.members.iter_mut().find(|member| member.seq == seq) else {
                return;
            };
            member.outcome = data
                .get("outcome")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
        "tool-workflow/run-end" => {
            run.stop_reason = data
                .get("stopReason")
                .and_then(Value::as_str)
                .map(str::to_owned);
            run.interrupted = false;
        }
        _ => return,
    }
    let text = serde_json::to_string(&run).unwrap_or_else(|_| "{}".to_owned());
    if let Some(index) = index {
        entries[index].text = text;
    } else {
        entries.push(ChatEntry {
            key,
            external_id: None,
            role: EntryRole::WorkflowRun,
            title: "workflow-run".to_owned(),
            text,
            input: String::new(),
            status: String::new(),
            tool: None,
            presentation: None,
            at_ms,
        });
    }
}

pub(crate) fn status_label(status: &str) -> &'static str {
    match status {
        "completed" => "已完成",
        "cancelled" => "已取消",
        "failed" | "error" => "失败",
        "interrupted" => "已中断",
        _ => "运行中",
    }
}

/// A fork/interrupt boundary is the only ordinary turn closure that proves an
/// unfinished run stopped; completed turns may have background runs still live.
pub(crate) fn interrupted(status: &str) -> bool {
    matches!(status, "interrupted" | "forked")
}

#[cfg(test)]
mod tests {
    use super::{Run, apply, interrupt, interrupted, member_status, status};
    use crate::state::EntryRole;
    use serde_json::json;

    #[test]
    fn phases_and_members_share_one_updating_run() {
        let mut entries = Vec::new();
        for event in [
            json!({"type":"tool-workflow/run-start","data":{"runId":7,"name":"Release"}}),
            json!({"type":"tool-workflow/agent-start","data":{"runId":7,"seq":1,"label":"API","childId":"child-a","phase":"build"}}),
            json!({"type":"tool-workflow/agent-start","data":{"runId":7,"seq":2,"label":"","childId":"child-b","phase":""}}),
            json!({"type":"tool-workflow/agent-end","data":{"runId":7,"seq":1,"outcome":"completed"}}),
            json!({"type":"tool-workflow/agent-end","data":{"runId":7,"seq":2,"outcome":"failed"}}),
            json!({"type":"tool-workflow/run-end","data":{"runId":7,"stopReason":"error"}}),
        ] {
            apply(&mut entries, &event, None);
        }
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].role, EntryRole::WorkflowRun);
        let run: Run = serde_json::from_str(&entries[0].text).unwrap();
        assert_eq!(run.name, "Release");
        assert_eq!(run.stop_reason.as_deref(), Some("error"));
        assert_eq!(run.members[0].child_id, "child-a");
        assert_eq!(run.members[1].phase.as_deref(), Some(""));
        assert_eq!(run.members[1].outcome.as_deref(), Some("failed"));
        assert!(!interrupted("completed"));
        assert!(interrupted("forked"));
        interrupt(&mut entries);
        let mut run: Run = serde_json::from_str(&entries[0].text).unwrap();
        assert_eq!(
            status(&run),
            "error",
            "a final run outcome wins over a later fork"
        );
        run.interrupted = false;
        run.stop_reason = None;
        assert_eq!(status(&run), "running");
        assert_eq!(member_status(&run, &run.members[0]), "completed");
    }
}
