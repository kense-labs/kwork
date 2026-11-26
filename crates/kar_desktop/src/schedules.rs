//! Native Scheduled Tasks & Office Automation engine (`schedules.json`).
//!
//! Borrowed from ToWork's `scheduled-tasks` + `SessionAssociation` contract and adapted in
//! pure Rust for Kar Desktop: revision-fenced schedule definitions, idempotent dispatch receipts
//! (uncertain outcomes are never blindly replayed on restart), and optional Office template binding.

use std::{
    fs::{self, File, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<PathBuf>,
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

/// An OS-held lease distinguishes an active run in another Kar window from an interrupted one.
pub(crate) struct ScheduleRunLease {
    _file: File,
    path: PathBuf,
}
impl ScheduleRunLease {
    fn path(dir: &Path, id: Uuid) -> PathBuf {
        dir.join(format!(".schedule-run-{id}.lock"))
    }
    pub(crate) fn acquire(dir: &Path, id: Uuid) -> Result<Self> {
        let path = Self::path(dir, id);
        let file = File::create_new(&path)?;
        file.lock()?;
        Ok(Self { _file: file, path })
    }
    fn active(dir: &Path, id: Uuid) -> Result<bool> {
        let path = Self::path(dir, id);
        let file = match OpenOptions::new().read(true).write(true).open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.into()),
        };
        match file.try_lock() {
            Ok(()) => {
                let _ = fs::remove_file(path);
                Ok(false)
            }
            Err(std::fs::TryLockError::WouldBlock) => Ok(true),
            Err(std::fs::TryLockError::Error(e)) => Err(e.into()),
        }
    }
}
impl Drop for ScheduleRunLease {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl ScheduleRegistry {
    pub fn load_or_seed(store_dir: &Path, _default_runtime: &str, now_ms: u64) -> Self {
        Self::read(store_dir, now_ms).unwrap_or_default()
    }

    pub(crate) fn read(dir: &Path, now_ms: u64) -> Result<Self> {
        let raw = match fs::read(dir.join("schedules.json")) {
            Ok(raw) => raw,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let mut reg: Self = serde_json::from_slice(&raw).context("Cannot read schedules.json")?;
        for task in &mut reg.tasks {
            if task.enabled
                && matches!(
                    task.cadence,
                    ScheduleCadence::DailyNineAm | ScheduleCadence::WeeklyMonday
                )
                && task.next_due_ms.is_none_or(|due| {
                    task.cadence.next_due_after(due.saturating_sub(1)) != Some(due)
                })
            {
                task.next_due_ms = task.cadence.next_due_after(now_ms);
            }
            for rec in &mut task.history {
                if rec.outcome == ScheduleRunOutcome::Admitted
                    && !ScheduleRunLease::active(dir, rec.request_id)?
                {
                    rec.outcome = ScheduleRunOutcome::OutcomeUnknown;
                }
            }
        }
        Ok(reg)
    }

    /// Apply the operation to the latest durable registry under an inter-process lock.
    /// Return values that trigger side effects are released only after the receipt is saved.
    pub fn transact<T>(
        &mut self,
        dir: &Path,
        now_ms: u64,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        fs::create_dir_all(dir)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("schedules.lock"))?;
        lock.lock()?;
        let mut latest = Self::read(dir, now_ms)?;
        let revision = latest.revision;
        let value = match f(&mut latest) {
            Ok(v) => v,
            Err(e) => {
                *self = Self::read(dir, now_ms)?;
                return Err(e);
            }
        };
        if latest.revision != revision {
            latest.save(dir)?;
        }
        *self = latest;
        Ok(value)
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(".schedules-{}.tmp", Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let file = File::create_new(&tmp)?;
            serde_json::to_writer_pretty(&file, self)?;
            file.sync_all()?;
            fs::rename(&tmp, dir.join("schedules.json"))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
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
            if draft.enabled
                && (draft.next_due_ms.is_none() || draft.cadence != self.tasks[pos].cadence)
            {
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
            if !task.enabled
                || task
                    .history
                    .iter()
                    .any(|r| r.outcome == ScheduleRunOutcome::Admitted)
            {
                continue;
            }
            let Some(due_ms) = task.next_due_ms else {
                continue;
            };
            if now_ms >= due_ms {
                let req_id = Uuid::new_v4();
                let next = match task.cadence {
                    ScheduleCadence::EveryMinutes(_) => task
                        .cadence
                        .next_due_after(due_ms)
                        .filter(|due| *due > now_ms)
                        .or_else(|| task.cadence.next_due_after(now_ms)),
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
        if task
            .history
            .iter()
            .any(|r| r.outcome == ScheduleRunOutcome::Admitted)
        {
            bail!("任务正在运行");
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

    pub fn update_run(
        &mut self,
        request_id: Uuid,
        chat_id: Option<Uuid>,
        outcome: ScheduleRunOutcome,
    ) -> bool {
        for task in &mut self.tasks {
            if let Some(rec) = task.history.iter_mut().find(|r| r.request_id == request_id) {
                rec.chat_id = chat_id.or(rec.chat_id);
                // An admission event can arrive after the completion was already recorded.
                if outcome != ScheduleRunOutcome::Admitted
                    || !matches!(
                        rec.outcome,
                        ScheduleRunOutcome::Completed | ScheduleRunOutcome::Failed
                    )
                {
                    rec.outcome = outcome;
                }
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
        reg.save_task(
            ScheduledTask {
                id: String::new(),
                revision: 0,
                title: "Review".into(),
                prompt: "Summarize".into(),
                workspace: None,
                cadence: ScheduleCadence::EveryMinutes(15),
                enabled: false,
                runtime_id: "codex".into(),
                model: None,
                effort: None,
                office_template: None,
                tone: ScheduleToneChoice::default(),
                next_due_ms: None,
                history: vec![],
            },
            None,
            10_000,
        )
        .unwrap();
        reg
    }

    #[test]
    fn first_launch_has_no_schedules_and_does_not_write_examples() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(
            ScheduleRegistry::load_or_seed(tmp.path(), "codex", 10_000)
                .tasks
                .is_empty()
        );
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
    #[test]
    fn separate_windows_claim_once_and_keep_other_windows_changes() {
        use std::sync::{Arc, Barrier};
        let store = tempfile::tempdir().unwrap();
        let mut initial = task_registry();
        let id = initial.tasks[0].id.clone();
        initial.set_enabled(&id, true, 0).unwrap();
        initial.save(store.path()).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let results = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..2)
                .map(|_| {
                    let mut reg = initial.clone();
                    let gate = Arc::clone(&barrier);
                    let path = store.path();
                    scope.spawn(move || {
                        gate.wait();
                        reg.transact(path, 900_001, |latest| {
                            let due = latest.claim_due_runs(900_001);
                            let mut leases = Vec::new();
                            for (_, id) in due {
                                leases.push(ScheduleRunLease::acquire(path, id)?);
                            }
                            Ok(leases)
                        })
                        .unwrap()
                    })
                })
                .collect();
            workers
                .into_iter()
                .flat_map(|t| t.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(results.len(), 1);
        let observed = ScheduleRegistry::read(store.path(), 900_002).unwrap();
        assert_eq!(
            observed.tasks[0].history[0].outcome,
            ScheduleRunOutcome::Admitted
        );
        let mut stale = initial;
        let second = Uuid::new_v4();
        assert!(
            stale
                .transact(store.path(), 900_002, |r| r
                    .record_manual_run(&id, second, 900_002))
                .is_err()
        );
        drop(results);
        assert_eq!(
            ScheduleRegistry::read(store.path(), 900_003).unwrap().tasks[0].history[0].outcome,
            ScheduleRunOutcome::OutcomeUnknown
        );
    }

    #[test]
    fn broken_schedule_file_preserves_the_form_and_previous_bytes() {
        let store = tempfile::tempdir().unwrap();
        let path = store.path().join("schedules.json");
        fs::write(&path, b"{unfinished").unwrap();
        let mut reg = task_registry();
        let id = reg.tasks[0].id.clone();
        assert!(
            reg.transact(store.path(), 500, |r| r.set_enabled(&id, true, 500))
                .is_err()
        );
        assert!(!reg.tasks[0].enabled);
        assert_eq!(fs::read(path).unwrap(), b"{unfinished");
    }
}
