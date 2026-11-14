//! Direct-child subagent catalog and the root Session's shared Team task board.
//!
//! Both come from durable Session events. Live clients use the equivalent Cordis
//! projections; this module keeps parsing and read-only replay host-testable.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// The official confirmation follows a used model, not merely a nonempty transcript.
pub fn disable_needs_confirmation(team: &TeamState) -> Option<bool> {
    team.has_used_model.map(|used| team.enabled && used)
}

/// How one child was created (`SubagentDescriptor.mode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubagentMode {
    /// Can receive follow-up messages.
    Continuable,
    /// One-shot run; its transcript is read-only forever.
    OneShot,
    /// Ran through an external provider without a local child session.
    External,
}

impl SubagentMode {
    /// Official `mode.*` label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Continuable => "可继续",
            Self::OneShot => "一次性",
            Self::External => "外部任务",
        }
    }

    fn parse(value: Option<&str>) -> Self {
        match value {
            Some("continuable") => Self::Continuable,
            Some("external") => Self::External,
            _ => Self::OneShot,
        }
    }
}

/// One durable catalog row: a direct child discovered by this session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubagentRow {
    pub child_id: String,
    pub label: String,
    pub mode: SubagentMode,
    pub created_at_ms: Option<u64>,
}

impl SubagentRow {
    /// Read one `subagent/catalog` payload.
    pub fn from_event_data(data: &Value) -> Option<Self> {
        let child_id = data["childId"].as_str().filter(|id| !id.is_empty())?;
        let label = data["label"]
            .as_str()
            .filter(|label| !label.trim().is_empty())
            .unwrap_or("子智能体");
        Some(Self {
            child_id: child_id.to_owned(),
            label: label.to_owned(),
            mode: SubagentMode::parse(data["mode"].as_str()),
            created_at_ms: data["childCreatedAt"].as_u64(),
        })
    }

    /// Whether this child has a locally readable transcript.
    pub fn has_local_session(&self) -> bool {
        self.mode != SubagentMode::External
    }
}

/// Insert or replace one catalog row, keeping discovery order.
///
/// The catalog is append-only per child, but reopening a session can merge an
/// older page with newer rows; the last observation of an identity wins.
pub fn upsert_subagent(rows: &mut Vec<SubagentRow>, row: SubagentRow) {
    if let Some(existing) = rows.iter_mut().find(|item| item.child_id == row.child_id) {
        *existing = row;
    } else {
        rows.push(row);
    }
}

/// Live activity label for one catalog row.
pub fn activity_label(row: &SubagentRow, running: bool) -> &'static str {
    if !row.has_local_session() {
        "外部任务"
    } else if running {
        "正在运行"
    } else {
        "已完成"
    }
}

/// One shared task in the Host's `agentTeam` projection.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TeamTask {
    pub id: String,
    pub subject: String,
    #[serde(default)]
    pub description: String,
    pub status: String,
    #[serde(default)]
    pub ready: bool,
    pub owner_id: Option<String>,
    pub owner_name: Option<String>,
}

impl TeamTask {
    /// Match the official board's owner and blocked-status presentation.
    pub fn owner(&self) -> &str {
        self.owner_name
            .as_deref()
            .or(self.owner_id.as_deref())
            .unwrap_or("未分配")
    }

    pub fn status_label(&self) -> &'static str {
        match self.status.as_str() {
            "in_progress" => "进行中",
            "completed" => "已完成",
            _ => "待处理",
        }
    }
}

/// Saved root-session Team selection and its read-only task board.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TeamState {
    pub enabled: bool,
    #[serde(default)]
    pub tasks: Vec<TeamTask>,
    pub failure: Option<String>,
    /// A completed model request makes disabling a confirmation, as upstream.
    #[serde(default)]
    pub has_used_model: Option<bool>,
    #[serde(default, skip_serializing)]
    pub expanded_task: Option<String>,
}

/// Fold the Team events visible in an offline history window.
pub struct OfflineTeam {
    pub enabled: bool,
    enabled_explicit: bool,
    tasks: Vec<Value>,
    failure: Option<String>,
    has_used_model: bool,
}

impl OfflineTeam {
    pub fn state(&self) -> TeamState {
        TeamState {
            enabled: self.enabled,
            tasks: {
                self.tasks
                    .iter()
                    .filter(|task| task["status"] != "deleted")
                    .map(|task| {
                        let ready = task["blockedBy"].as_array().is_none_or(|blockers| {
                            blockers.iter().all(|id| {
                                self.tasks
                                    .iter()
                                    .find(|other| other["id"] == *id)
                                    .is_some_and(|other| other["status"] == "completed")
                            })
                        });
                        TeamTask {
                            id: task["id"].as_str().unwrap_or_default().to_owned(),
                            subject: task["subject"].as_str().unwrap_or_default().to_owned(),
                            description: task["description"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                            status: task["status"].as_str().unwrap_or_default().to_owned(),
                            ready,
                            owner_id: task["ownerId"].as_str().map(str::to_owned),
                            owner_name: task["ownerName"].as_str().map(str::to_owned),
                        }
                    })
                    .collect()
            },
            failure: self.failure.clone(),
            has_used_model: Some(self.has_used_model),
            expanded_task: None,
        }
    }

    fn observe(&mut self, root_id: &str, event: &Value) {
        let data = &event["data"];
        match event["type"].as_str() {
            Some("team/mode") => match data.as_object() {
                Some(data) if data.len() == 1 => match data["enabled"].as_bool() {
                    Some(enabled) => {
                        self.enabled = enabled;
                        self.enabled_explicit = true;
                    }
                    None => self.fail("persisted Agent Teams team/mode payload is invalid"),
                },
                _ => self.fail("persisted Agent Teams team/mode payload is invalid"),
            },
            Some("team/task") if self.failure.is_none() => {
                if !data
                    .get("teamId")
                    .and_then(Value::as_str)
                    .is_some_and(|team| !team.is_empty())
                {
                    self.fail("persisted Agent Teams team/task payload is invalid");
                    return;
                }
                if data["teamId"].as_str() != Some(root_id) {
                    return;
                }
                if data.get("version").and_then(Value::as_u64) != Some(2) {
                    self.fail("unsupported Agent Teams event version");
                    return;
                }
                if data.as_object().is_none_or(|data| data.len() != 3) {
                    self.fail("persisted Agent Teams team/task payload is invalid");
                    return;
                }
                let Some(task) = data.get("task").filter(|task| {
                    data["version"] == 2
                        && data["teamId"].as_str() == Some(root_id)
                        && valid_task(task)
                }) else {
                    if data["teamId"].as_str() == Some(root_id) {
                        self.fail("persisted Agent Teams team/task payload is invalid");
                    }
                    return;
                };
                let previous = self.tasks.iter().position(|item| item["id"] == task["id"]);
                let revision = task["revision"].as_u64().unwrap_or(0);
                let valid_revision = previous.map_or(revision == 1, |index| {
                    self.tasks[index]["revision"]
                        .as_u64()
                        .is_some_and(|current| revision == current + 1)
                });
                if !valid_revision {
                    self.fail("team task revision is not contiguous");
                    return;
                }
                match previous {
                    Some(index) => self.tasks[index] = task.clone(),
                    None => self.tasks.push(task.clone()),
                }
                if let Err(error) = validate_task_graph(&self.tasks) {
                    self.fail(&error);
                    return;
                }
                if !self.enabled_explicit {
                    self.enabled = true;
                }
            }
            Some("request/header") if data.pointer("/header/config/provider").is_some() => {
                self.has_used_model = true;
            }
            _ => {}
        }
    }

    fn fail(&mut self, message: &str) {
        if self.failure.is_none() {
            self.failure = Some(message.to_owned());
        }
    }
}

/// Project Team mode, tasks, and request usage from an offline root Session log.
pub fn offline_team(root_id: &str, events: &[Value]) -> Option<TeamState> {
    let mut team = OfflineTeam {
        enabled: false,
        enabled_explicit: false,
        tasks: Vec::new(),
        failure: None,
        has_used_model: false,
    };
    let mut available = false;
    for event in events {
        if event["type"] == "session/end-seed" && event["data"]["inherited"] == true {
            team = OfflineTeam {
                enabled: false,
                enabled_explicit: false,
                tasks: Vec::new(),
                failure: None,
                has_used_model: false,
            };
            available = false;
            continue;
        }
        available |= matches!(event["type"].as_str(), Some("team/mode" | "team/task"));
        team.observe(root_id, event);
    }
    available.then(|| team.state())
}

fn valid_task(task: &Value) -> bool {
    task["id"].as_str().is_some_and(|id| !id.is_empty())
        && task["revision"]
            .as_u64()
            .is_some_and(|revision| revision > 0)
        && task["subject"].is_string()
        && task["description"].is_string()
        && matches!(
            task["status"].as_str(),
            Some("pending" | "in_progress" | "completed" | "deleted")
        )
        && task["blockedBy"].as_array().is_some_and(|items| {
            items
                .iter()
                .all(|id| id.as_str().is_some_and(|id| !id.is_empty()))
        })
        && task["writeScopes"]
            .as_array()
            .is_some_and(|scopes| scopes.iter().all(|scope| scope.is_string()))
        && task["ownerId"].as_str().is_none_or(|id| !id.is_empty())
        && task["ownerName"]
            .as_str()
            .is_none_or(|name| !name.is_empty())
        && task.as_object().is_some_and(|task| {
            task.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "id" | "revision"
                        | "subject"
                        | "description"
                        | "status"
                        | "ownerId"
                        | "ownerName"
                        | "blockedBy"
                        | "writeScopes"
                )
            })
        })
        && task["ownerName"]
            .as_str()
            .is_none_or(|_| task["ownerId"].is_string())
}

/// The official projection freezes its board at the first invalid durable record.
fn validate_task_graph(tasks: &[Value]) -> Result<(), String> {
    let ids: HashMap<&str, &Value> = tasks
        .iter()
        .filter_map(|task| Some((task["id"].as_str()?, task)))
        .collect();
    for task in tasks.iter().filter(|task| task["status"] != "deleted") {
        let mut blockers = HashSet::new();
        for id in task["blockedBy"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !blockers.insert(id) {
                return Err(format!(
                    "team task \"{}\" repeats blocker \"{id}\"",
                    task["id"]
                ));
            }
            if id == task["id"] {
                return Err(format!("team task \"{id}\" cannot block itself"));
            }
            if ids.get(id).is_none_or(|task| task["status"] == "deleted") {
                return Err(format!(
                    "blocker task \"{id}\" for \"{}\" is missing or deleted",
                    task["id"]
                ));
            }
        }
    }
    fn visit<'a>(
        id: &'a str,
        tasks: &HashMap<&str, &'a Value>,
        path: &mut Vec<&'a str>,
    ) -> Result<(), String> {
        if path.contains(&id) {
            return Err(format!("task dependency cycle includes \"{id}\""));
        }
        let Some(task) = tasks.get(id).filter(|task| task["status"] != "deleted") else {
            return Ok(());
        };
        path.push(id);
        for blocker in task["blockedBy"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            visit(blocker, tasks, path)?;
        }
        path.pop();
        Ok(())
    }
    for id in ids.keys() {
        visit(id, &ids, &mut Vec::new())?;
    }
    Ok(())
}

/// Official TeamBoard progress order: completed, active, then pending.
pub fn progress_label(team: &TeamState) -> String {
    let completed = team
        .tasks
        .iter()
        .filter(|task| task.status == "completed")
        .count();
    let active = team
        .tasks
        .iter()
        .filter(|task| task.status == "in_progress")
        .count();
    let pending = team.tasks.len() - completed - active;
    [
        (completed > 0).then(|| format!("{completed} 已完成")),
        (active > 0).then(|| format!("{active} 进行中")),
        (pending > 0).then(|| format!("{pending} 待处理")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_catalog_fact_keeps_identity_label_and_mode() {
        let row = SubagentRow::from_event_data(&json!({
            "version": 0,
            "childId": "3ee01e83-40f7-4abf-b1e4-03c4bc0d85ef",
            "childCreatedAt": 1_794_454_553_990u64,
            "mode": "continuable",
            "label": "研究 goal 官方 UI 呈现",
        }))
        .expect("catalog row");
        assert_eq!(row.child_id, "3ee01e83-40f7-4abf-b1e4-03c4bc0d85ef");
        assert_eq!(row.label, "研究 goal 官方 UI 呈现");
        assert_eq!(row.mode, SubagentMode::Continuable);
        assert_eq!(row.created_at_ms, Some(1_794_454_553_990));
        assert_eq!(row.mode.label(), "可继续");
    }

    #[test]
    fn missing_labels_and_unknown_modes_fall_back() {
        let row = SubagentRow::from_event_data(&json!({
            "childId": "child-1", "mode": "mystery", "label": "   ",
        }))
        .expect("catalog row");
        assert_eq!(row.label, "子智能体");
        assert_eq!(row.mode, SubagentMode::OneShot);
        assert!(SubagentRow::from_event_data(&json!({"label": "x"})).is_none());
        assert!(SubagentRow::from_event_data(&json!(null)).is_none());
    }

    #[test]
    fn the_same_child_is_never_listed_twice() {
        let mut rows = Vec::new();
        upsert_subagent(
            &mut rows,
            SubagentRow::from_event_data(&json!({
                "childId": "a", "label": "第一版", "mode": "continuable",
            }))
            .unwrap(),
        );
        upsert_subagent(
            &mut rows,
            SubagentRow::from_event_data(&json!({
                "childId": "b", "label": "另一个", "mode": "one-shot",
            }))
            .unwrap(),
        );
        upsert_subagent(
            &mut rows,
            SubagentRow::from_event_data(&json!({
                "childId": "a", "label": "改名", "mode": "continuable",
            }))
            .unwrap(),
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "改名");
        assert_eq!(rows[1].child_id, "b");
    }

    #[test]
    fn activity_distinguishes_running_finished_and_external() {
        let local = SubagentRow::from_event_data(&json!({
            "childId": "a", "label": "本地", "mode": "continuable",
        }))
        .unwrap();
        assert_eq!(activity_label(&local, true), "正在运行");
        assert_eq!(activity_label(&local, false), "已完成");
        let external = SubagentRow::from_event_data(&json!({
            "childId": "b", "label": "外部", "mode": "external",
        }))
        .unwrap();
        assert_eq!(activity_label(&external, true), "外部任务");
        assert!(!external.has_local_session());
    }

    fn task(id: &str, revision: u64, status: &str, blocked_by: &[&str]) -> Value {
        json!({"version": 2, "teamId": "root", "task": {
            "id": id, "revision": revision, "subject": format!("Subject {id}"),
            "description": "Details", "status": status, "blockedBy": blocked_by,
            "writeScopes": [], "ownerId": "child", "ownerName": "Worker",
        }})
    }

    #[test]
    fn offline_team_replays_saved_mode_and_task_readiness() {
        let state = offline_team("root", &[
            json!({"type": "team/mode", "data": {"enabled": true}}),
            json!({"type": "team/task", "data": task("task-1", 1, "completed", &[])}),
            json!({"type": "team/task", "data": task("task-2", 1, "pending", &["task-1"])}),
            json!({"type": "request/header", "data": {"header": {"config": {"provider": "p"}}}}),
        ])
        .unwrap();
        assert!(state.enabled);
        assert_eq!(state.has_used_model, Some(true));
        assert_eq!(state.tasks.len(), 2);
        assert!(state.tasks[1].ready);
        assert_eq!(state.tasks[1].owner(), "Worker");
        assert_eq!(offline_team("root", &[]), None);
    }

    #[test]
    fn deleted_tasks_stay_hidden_and_revision_gaps_report_a_failure() {
        let state = offline_team(
            "root",
            &[
                json!({"type": "team/task", "data": task("task-1", 1, "pending", &[])}),
                json!({"type": "team/task", "data": task("task-1", 2, "deleted", &[])}),
            ],
        )
        .unwrap();
        assert!(state.tasks.is_empty());
        let state = offline_team(
            "root",
            &[
                json!({"type": "team/task", "data": task("task-1", 2, "pending", &[])}),
                json!({"type": "team/mode", "data": {"enabled": false}}),
            ],
        )
        .unwrap();
        assert!(state.failure.is_some());
        assert!(!state.enabled);
    }

    #[test]
    fn another_team_and_malformed_modes_do_not_change_the_board() {
        let state = offline_team(
            "root",
            &[
                json!({"type": "team/task", "data": task("task-1", 1, "pending", &[])}),
                json!({"type": "team/task", "data": {"version": 2, "teamId": "other", "task": {}}}),
                json!({"type": "team/mode", "data": {"enabled": "yes"}}),
            ],
        )
        .unwrap();
        assert!(state.failure.is_some());
        assert!(state.enabled);
    }

    #[test]
    fn invalid_tasks_keep_the_last_valid_board_and_inherited_events_do_not_leak() {
        let mut invalid = task("task-2", 1, "pending", &[]);
        invalid["task"]["unexpected"] = json!(true);
        let state = offline_team(
            "root",
            &[
                json!({"type": "team/mode", "data": {"enabled": true}}),
                json!({"type": "team/task", "data": task("task-1", 1, "pending", &[])}),
                json!({"type": "team/task", "data": invalid}),
            ],
        )
        .unwrap();
        assert!(state.failure.is_some());
        assert_eq!(state.tasks.len(), 1);
        assert_eq!(state.tasks[0].id, "task-1");

        let state = offline_team(
            "root",
            &[
                json!({"type": "team/mode", "data": {"enabled": true}}),
                json!({"type": "session/end-seed", "data": {"inherited": true}}),
                json!({"type": "team/mode", "data": {"enabled": false}}),
            ],
        )
        .unwrap();
        assert!(!state.enabled);
    }

    #[test]
    fn disabling_a_used_team_requires_confirmation() {
        let team = TeamState {
            enabled: true,
            has_used_model: Some(true),
            ..TeamState::default()
        };
        assert_eq!(disable_needs_confirmation(&team), Some(true));
        assert_eq!(
            disable_needs_confirmation(&TeamState {
                has_used_model: Some(false),
                ..team
            }),
            Some(false)
        );
        assert_eq!(disable_needs_confirmation(&TeamState::default()), None);
    }

    #[test]
    fn task_progress_uses_visible_non_deleted_tasks() {
        let state = TeamState {
            tasks: vec![
                serde_json::from_value(task("a", 1, "completed", &[])["task"].clone()).unwrap(),
                serde_json::from_value(task("b", 1, "in_progress", &[])["task"].clone()).unwrap(),
                serde_json::from_value(task("c", 1, "pending", &[])["task"].clone()).unwrap(),
            ],
            ..TeamState::default()
        };
        assert_eq!(progress_label(&state), "1 已完成 · 1 进行中 · 1 待处理");
    }
}
