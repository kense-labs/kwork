//! Installation-local first-run state for the desktop ACP client.

#[path = "../../shared/desktop_onboarding.rs"]
pub(crate) mod protocol;

use crate::preferences::{edit_preferences, preferences_path, read_preferences};
use protocol::{Action, Completion, Process, Progress, Purpose, Reply, Snapshot, Step, Usage};
use std::path::Path;
use tauri::{Manager, Webview};

#[tauri::command]
pub(crate) async fn desktop_onboarding(
    webview: Webview,
    action: Action,
    revision: Option<u64>,
) -> Result<Reply, String> {
    if webview.label() != "main" {
        return Err("只有主界面可以更新首次引导。".into());
    }
    let path = preferences_path(webview.app_handle())?;
    tokio::task::spawn_blocking(move || apply_action(&path, action, revision))
        .await
        .map_err(|_| "未能读取或保存首次引导。")?
}

fn apply_action(path: &Path, action: Action, revision: Option<u64>) -> Result<Reply, String> {
    if action == Action::Read {
        return Ok(Reply {
            snapshot: read_preferences(path)?.onboarding.unwrap_or_default(),
            conflict: false,
        });
    }
    edit_preferences(path, |preferences| {
        let current = preferences.onboarding.unwrap_or_default();
        if Some(current.revision) != revision {
            return Ok(Reply {
                snapshot: current,
                conflict: true,
            });
        }
        let progress = transition(current.progress, action)?;
        if progress.step == Step::Done {
            preferences.work_details = Some(
                match progress.process {
                    Some(Process::Compact) => "compact",
                    Some(Process::Detailed) => "detailed",
                    _ => "standard",
                }
                .into(),
            );
            preferences.performance_usage = Some(
                if progress.usage == Usage::Compact {
                    "compact"
                } else {
                    "detailed"
                }
                .into(),
            );
            preferences.developer_tools = Some(progress.developer_tools);
        }
        let next = Snapshot {
            revision: current
                .revision
                .checked_add(1)
                .ok_or("首次引导进度版本已达上限。")?,
            progress,
        };
        preferences.onboarding = Some(next);
        Ok(Reply {
            snapshot: next,
            conflict: false,
        })
    })
}

fn transition(mut current: Progress, action: Action) -> Result<Progress, String> {
    if current.version != 1 || current.step == Step::Done {
        return Err("当前首次引导进度无法修改。".into());
    }
    match (current.step, action) {
        (Step::Welcome, Action::Next) | (Step::Purpose, Action::Back) => {
            current.step = Step::Credit
        }
        (Step::Credit, Action::Back) => current.step = Step::Welcome,
        (Step::Credit, Action::Next) | (Step::Process, Action::Back) => {
            current.step = Step::Purpose
        }
        (Step::Purpose, Action::SelectPurpose { purpose }) => current.purpose = purpose,
        (Step::Purpose, Action::Next)
            if matches!(current.purpose, Some(Purpose::Development | Purpose::Both)) =>
        {
            current.step = Step::Process
        }
        (Step::Process, Action::SelectProcess { process }) => current.process = Some(process),
        (
            Step::Purpose,
            Action::Finish {
                completion: Completion::Completed,
            },
        ) if current.purpose == Some(Purpose::Office) => {
            finish(&mut current, Completion::Completed, Process::Compact, false);
        }
        (
            Step::Process,
            Action::Finish {
                completion: Completion::Completed,
            },
        ) if matches!(current.purpose, Some(Purpose::Development | Purpose::Both))
            && current.process.is_some() =>
        {
            let process = current.process.expect("process selected");
            finish(&mut current, Completion::Completed, process, true);
        }
        (
            Step::Credit | Step::Purpose | Step::Process,
            Action::Finish {
                completion: Completion::Skipped,
            },
        ) => {
            finish(&mut current, Completion::Skipped, Process::Standard, false);
        }
        (
            _,
            Action::Finish {
                completion: Completion::ApiKey,
            },
        ) => {
            finish(&mut current, Completion::ApiKey, Process::Standard, true);
        }
        _ => return Err("这一步无法执行所选操作。".into()),
    }
    Ok(current)
}

fn finish(
    progress: &mut Progress,
    completion: Completion,
    process: Process,
    developer_tools: bool,
) {
    progress.step = Step::Done;
    progress.completion = Some(completion);
    progress.process = Some(process);
    progress.usage = if developer_tools {
        Usage::Detailed
    } else {
        Usage::Compact
    };
    progress.developer_tools = developer_tools;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resumes_choices_and_commits_preferences_in_the_completed_document() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("workspaces.json");
        edit_preferences(&path, |preferences| {
            preferences.sidebar_width = Some(320.);
            Ok(())
        })
        .unwrap();
        let mut revision = 0;
        for action in [
            Action::Next,
            Action::Next,
            Action::SelectPurpose {
                purpose: Some(Purpose::Both),
            },
            Action::Next,
            Action::SelectProcess {
                process: Process::Detailed,
            },
        ] {
            let reply = apply_action(&path, action, Some(revision)).unwrap();
            assert!(!reply.conflict);
            revision = reply.snapshot.revision;
        }
        let restored = apply_action(&path, Action::Read, None).unwrap().snapshot;
        assert_eq!(restored.progress.step, Step::Process);
        assert_eq!(restored.progress.process, Some(Process::Detailed));
        let stale = apply_action(&path, Action::Back, Some(revision - 1)).unwrap();
        assert!(stale.conflict);
        assert_eq!(stale.snapshot, restored);
        let finish = Action::Finish {
            completion: Completion::Completed,
        };
        let completed = apply_action(&path, finish, Some(revision)).unwrap();
        let disk = read_preferences(&path).unwrap();
        assert_eq!(disk.onboarding, Some(completed.snapshot));
        assert_eq!(disk.sidebar_width, Some(320.));
        assert_eq!(disk.work_details.as_deref(), Some("detailed"));
        assert_eq!(disk.performance_usage.as_deref(), Some("detailed"));
        assert_eq!(disk.developer_tools, Some(true));
        assert!(apply_action(&path, Action::Back, Some(completed.snapshot.revision)).is_err());
    }

    #[test]
    fn completion_applies_the_office_skip_and_api_key_defaults() {
        for (current, completion, process, usage, tools) in [
            (
                Progress {
                    step: Step::Purpose,
                    purpose: Some(Purpose::Office),
                    ..Progress::default()
                },
                Completion::Completed,
                Process::Compact,
                Usage::Compact,
                false,
            ),
            (
                Progress {
                    step: Step::Credit,
                    ..Progress::default()
                },
                Completion::Skipped,
                Process::Standard,
                Usage::Compact,
                false,
            ),
            (
                Progress::default(),
                Completion::ApiKey,
                Process::Standard,
                Usage::Detailed,
                true,
            ),
        ] {
            let result = transition(current, Action::Finish { completion }).unwrap();
            assert_eq!(result.step, Step::Done);
            assert_eq!(result.process, Some(process));
            assert_eq!(result.usage, usage);
            assert_eq!(result.developer_tools, tools);
        }
        let unfinished = Progress {
            step: Step::Process,
            purpose: Some(Purpose::Development),
            ..Progress::default()
        };
        assert!(
            transition(
                unfinished,
                Action::Finish {
                    completion: Completion::Completed
                }
            )
            .is_err()
        );
    }
}
