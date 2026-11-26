//! Native Scheduled Tasks & Office Automation engine (`schedules.json`).
//!
//! Borrowed from ToWork's `scheduled-tasks` + `SessionAssociation` contract and adapted in
//! pure Rust for Kar Desktop: revision-fenced schedule definitions, idempotent dispatch receipts
//! (uncertain outcomes are never blindly replayed on restart), and optional Office template binding.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use chrono::{Datelike, Local, TimeZone, Weekday};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::client_store::SessionAssociation;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleCadence {
    EveryMinutes(u32),
    #[default]
    DailyNineAm,
    WeeklyMonday,
    ManualOnly,
}

impl ScheduleCadence {
    pub const PRESETS: [Self; 4] = [
        Self::DailyNineAm,
        Self::EveryMinutes(60),
        Self::WeeklyMonday,
        Self::ManualOnly,
    ];

    pub fn label(self) -> String {
        match self {
            Self::EveryMinutes(m) => format!("每 {m} 分钟"),
            Self::DailyNineAm => "每日 09:00".into(),
            Self::WeeklyMonday => "每周一 09:00".into(),
            Self::ManualOnly => "仅手动触发".into(),
        }
    }

    fn next_due_after(self, now_ms: u64) -> Option<u64> {
        match self {
            Self::ManualOnly => None,
            Self::EveryMinutes(_) => now_ms.checked_add(self.interval_ms()?),
            Self::DailyNineAm | Self::WeeklyMonday => {
                let now = Local
                    .timestamp_millis_opt(now_ms.try_into().ok()?)
                    .single()?;
                let today = now.date_naive();
                // Include the following week when today's Monday occurrence has passed.
                (0..=14).find_map(|offset| {
                    let day = today.checked_add_days(chrono::Days::new(offset))?;
                    if self == Self::WeeklyMonday && day.weekday() != Weekday::Mon {
                        return None;
                    }
                    let due = Local
                        .from_local_datetime(&day.and_hms_opt(9, 0, 0)?)
                        .earliest()?;
                    (due > now).then(|| due.timestamp_millis() as u64)
                })
            }
        }
    }

    pub fn interval_ms(self) -> Option<u64> {
        match self {
            Self::EveryMinutes(m) => Some(u64::from(m.max(1)) * 60_000),
            Self::DailyNineAm => Some(24 * 3600 * 1000),
            Self::WeeklyMonday => Some(7 * 24 * 3600 * 1000),
            Self::ManualOnly => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleToneChoice {
    #[default]
    Blue,
    Teal,
    Purple,
    Orange,
    Green,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleRunOutcome {
    #[default]
    Admitted,
    Completed,
    Failed,
    OutcomeUnknown,
}

impl ScheduleRunOutcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Admitted => "执行中",
            Self::Completed => "已完成",
            Self::Failed => "未通过",
            Self::OutcomeUnknown => "结果未确认",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleRunReceipt {
    pub request_id: Uuid,
    pub chat_id: Option<Uuid>,
    pub triggered_ms: u64,
    pub manual: bool,
    pub captured_runtime: String,
    #[serde(default)]
    pub captured_effort: Option<String>,
    pub outcome: ScheduleRunOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: String,
    pub revision: u64,
    pub title: String,
    pub prompt: String,
    pub cadence: ScheduleCadence,
    pub enabled: bool,
    pub runtime_id: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    /// Optional Office document template (`.docx` / `.xlsx` / `.pptx`) bound to this automation.
    #[serde(default)]
    pub office_template: Option<PathBuf>,
    #[serde(default)]
    pub tone: ScheduleToneChoice,
    #[serde(default)]
    pub next_due_ms: Option<u64>,
    #[serde(default)]
    pub history: Vec<ScheduleRunReceipt>,
}

impl ScheduledTask {
    pub fn association(&self) -> SessionAssociation {
        SessionAssociation::schedule(self.id.clone(), self.title.clone())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ScheduleRegistry {
    pub revision: u64,
    pub seeded_defaults: bool,
    pub tasks: Vec<ScheduledTask>,
}

impl ScheduleRegistry {
    pub fn load_or_seed(store_dir: &Path, _default_runtime: &str, now_ms: u64) -> Self {
        let path = store_dir.join("schedules.json");
        if let Ok(raw) = std::fs::read_to_string(&path)
            && let Ok(mut reg) = serde_json::from_str::<Self>(&raw)
        {
            // Honest restart recovery: any run left in `Admitted` across a process restart becomes
            // `OutcomeUnknown` rather than silently re-triggering side effects.
            for task in &mut reg.tasks {
                if task.enabled && matches!(task.cadence, ScheduleCadence::DailyNineAm | ScheduleCadence::WeeklyMonday)
                    && task.next_due_ms.is_none_or(|due| task.cadence.next_due_after(due.saturating_sub(1)) != Some(due)) {
                    task.next_due_ms = task.cadence.next_due_after(now_ms);
                }
                for rec in &mut task.history {
                    if rec.outcome == ScheduleRunOutcome::Admitted {
                        rec.outcome = ScheduleRunOutcome::OutcomeUnknown;
                    }
                }
            }
            return reg;
        }
        Self::default()
    }

    pub fn save(&self, store_dir: &Path) -> Result<()> {
        let _ = std::fs::create_dir_all(store_dir);
        let path = store_dir.join("schedules.json");
        let tmp = store_dir.join("schedules.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn save_task(
        &mut self,
        mut draft: ScheduledTask,
        expected_task_revision: Option<u64>,
        now_ms: u64,
    ) -> Result<&ScheduledTask> {
        if draft.title.trim().is_empty() {
            bail!("定时任务标题不能为空");
        }
        if draft.prompt.trim().is_empty() {
            bail!("定时任务指令不能为空");
        }
        if let Some(pos) = self.tasks.iter().position(|t| t.id == draft.id) {
            let cur_rev = self.tasks[pos].revision;
            if let Some(exp) = expected_task_revision
                && exp != cur_rev
            {
                bail!("任务已被更新（期望 v{exp}，当前 v{cur_rev}），请刷新后再保存");
            }
            draft.revision = cur_rev + 1;
            draft.history = self.tasks[pos].history.clone();
            if draft.enabled && (draft.next_due_ms.is_none() || draft.cadence != self.tasks[pos].cadence) {
                draft.next_due_ms = draft.cadence.next_due_after(now_ms);
            }
            self.tasks[pos] = draft;
            self.revision = self.revision.saturating_add(1);
            return Ok(&self.tasks[pos]);
        }

        let id = if draft.id.trim().is_empty() {
            format!("sched-{}", &Uuid::new_v4().to_string()[..8])
        } else {
            draft.id
        };
        draft.id = id;
        draft.revision = 1;
        if draft.enabled && draft.next_due_ms.is_none() {
            draft.next_due_ms = draft.cadence.next_due_after(now_ms);
        }
        self.tasks.push(draft);
        self.revision = self.revision.saturating_add(1);
        Ok(self.tasks.last().expect("pushed"))
    }

    pub fn set_enabled(&mut self, task_id: &str, enabled: bool, now_ms: u64) -> Result<bool> {
        let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) else {
            bail!("未找到定时任务 `{task_id}`");
        };
        task.enabled = enabled;
        task.revision = task.revision.saturating_add(1);
        task.next_due_ms = if enabled {
            task.cadence.next_due_after(now_ms)
        } else {
            None
        };
        self.revision = self.revision.saturating_add(1);
        Ok(enabled)
    }

    pub fn remove_task(&mut self, task_id: &str) -> bool {
        let before = self.tasks.len();
        self.tasks.retain(|t| t.id != task_id);
        let removed = self.tasks.len() < before;
        if removed {
            self.revision = self.revision.saturating_add(1);
        }
        removed
    }

    /// Claim due schedules (latest due occurrence recovery without duplicate flood).
    pub fn claim_due_runs(&mut self, now_ms: u64) -> Vec<(ScheduledTask, Uuid)> {
        let mut due = Vec::new();
        for task in &mut self.tasks {
            if !task.enabled {
                continue;
            }
            let Some(due_ms) = task.next_due_ms else {
                continue;
            };
            if now_ms >= due_ms {
                let req_id = Uuid::new_v4();
                let next = match task.cadence {
                    ScheduleCadence::EveryMinutes(_) => task.cadence.next_due_after(due_ms)
                        .filter(|due| *due > now_ms).or_else(|| task.cadence.next_due_after(now_ms)),
                    cadence => cadence.next_due_after(now_ms),
                };
                task.next_due_ms = next;
                if next.is_none() {
                    task.enabled = false;
                }
                task.history.push(ScheduleRunReceipt {
                    request_id: req_id,
                    chat_id: None,
                    triggered_ms: now_ms,
                    manual: false,
                    captured_runtime: task.runtime_id.clone(),
                    captured_effort: task.effort.clone(),
                    outcome: ScheduleRunOutcome::Admitted,
                });
                due.push((task.clone(), req_id));
            }
        }
        if !due.is_empty() {
            self.revision = self.revision.saturating_add(1);
        }
        due
    }

    /// Enqueue a manual run with idempotent `request_id` while preserving the existing schedule cursor.
    pub fn record_manual_run(
        &mut self,
        task_id: &str,
        request_id: Uuid,
        now_ms: u64,
    ) -> Result<ScheduledTask> {
        let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) else {
            bail!("未找到定时任务 `{task_id}`");
        };
        if task.history.iter().any(|r| r.request_id == request_id) {
            return Ok(task.clone());
        }
        task.history.push(ScheduleRunReceipt {
            request_id,
            chat_id: None,
            triggered_ms: now_ms,
            manual: true,
            captured_runtime: task.runtime_id.clone(),
            captured_effort: task.effort.clone(),
            outcome: ScheduleRunOutcome::Admitted,
        });
        self.revision = self.revision.saturating_add(1);
        Ok(task.clone())
    }

    pub fn attach_run_chat(
        &mut self,
        request_id: Uuid,
        chat_id: Uuid,
        outcome: ScheduleRunOutcome,
    ) -> bool {
        self.update_run(request_id, Some(chat_id), outcome)
    }

    pub fn update_run(&mut self, request_id: Uuid, chat_id: Option<Uuid>, outcome: ScheduleRunOutcome) -> bool {
        for task in &mut self.tasks {
            if let Some(rec) = task
                .history
                .iter_mut()
                .find(|r| r.request_id == request_id)
            {
                rec.chat_id = chat_id.or(rec.chat_id);
                rec.outcome = outcome;
                self.revision = self.revision.saturating_add(1);
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_schedules_use_the_next_local_nine_oclock() {
        let at = |day, hour| {
            Local
                .with_ymd_and_hms(2026, 11, day, hour, 0, 0)
                .single()
                .unwrap()
                .timestamp_millis() as u64
        };
        assert_eq!(
            ScheduleCadence::DailyNineAm.next_due_after(at(23, 8)),
            Some(at(23, 9))
        );
        assert_eq!(
            ScheduleCadence::DailyNineAm.next_due_after(at(23, 9)),
            Some(at(24, 9))
        );
        assert_eq!(
            ScheduleCadence::WeeklyMonday.next_due_after(at(23, 9)),
            Some(at(30, 9))
        );
    }

    fn task_registry() -> ScheduleRegistry {
        let mut reg = ScheduleRegistry::default();
        reg.save_task(ScheduledTask {
            id: String::new(), revision: 0, title: "Review".into(), prompt: "Summarize".into(),
            cadence: ScheduleCadence::EveryMinutes(15), enabled: false, runtime_id: "codex".into(),
            model: None, effort: None, office_template: None, tone: ScheduleToneChoice::default(),
            next_due_ms: None, history: vec![],
        }, None, 10_000).unwrap();
        reg
    }

    #[test]
    fn first_launch_has_no_schedules_and_does_not_write_examples() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(ScheduleRegistry::load_or_seed(tmp.path(), "codex", 10_000).tasks.is_empty());
        assert!(!tmp.path().join("schedules.json").exists());
    }

    #[test]
    fn manual_run_preserves_schedule_cursor_and_deduplicates_request_id() {
        let mut reg = task_registry();
        let id = reg.tasks[0].id.clone();
        reg.set_enabled(&id, true, 10_000).unwrap();
        let due_before = reg.tasks[0].next_due_ms;

        let req = Uuid::new_v4();
        reg.record_manual_run(&id, req, 12_000).unwrap();
        reg.record_manual_run(&id, req, 12_500).unwrap(); // idempotent retry
        assert_eq!(reg.tasks[0].history.len(), 1);
        assert_eq!(reg.tasks[0].next_due_ms, due_before);

        let cid = Uuid::new_v4();
        assert!(reg.attach_run_chat(req, cid, ScheduleRunOutcome::Completed));
        assert_eq!(reg.tasks[0].history[0].chat_id, Some(cid));
    }

    #[test]
    fn restart_marks_in_flight_runs_as_outcome_unknown() {
        let tmp = tempfile::tempdir().unwrap();
        let mut reg = task_registry();
        let id = reg.tasks[0].id.clone();
        let req = Uuid::new_v4();
        reg.record_manual_run(&id, req, 11_000).unwrap();
        reg.save(tmp.path()).unwrap();

        let reloaded = ScheduleRegistry::load_or_seed(tmp.path(), "codex", 20_000);
        assert_eq!(
            reloaded.tasks[0].history[0].outcome,
            ScheduleRunOutcome::OutcomeUnknown
        );
    }
}
