//! Persistent sidebar-only workspace titles, expansion and visibility.

use crate::desktop_onboarding::protocol::Snapshot as OnboardingSnapshot;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WorkspacePreferences {
    pub(crate) onboarding: Option<OnboardingSnapshot>,
    pub(crate) developer_tools: Option<bool>,
    pub aliases: HashMap<String, String>,
    pub hidden: Vec<String>,
    pub added_workspaces: Vec<String>,
    /// Browser-local Workspace display order, including hidden groups.
    pub workspace_order: Vec<String>,
    pub collapsed: Vec<String>,
    pub sidebar_collapsed: Option<bool>,
    pub sidebar_width: Option<f32>,
    pub group_by: Option<String>,
    pub order_by: Option<String>,
    /// Browser-local Session order for each Workspace and the flat-list account.
    pub session_order_by_account: HashMap<String, Vec<String>>,
    pub archived_filter: Option<String>,
    /// 置顶的会话 ID（排序时置顶组内最前）。
    pub pinned: Vec<String>,
    /// Legacy session aliases from builds that saved renames in sidebar preferences.
    pub session_titles: HashMap<String, String>,
    /// 已归档的会话 ID。
    pub archived: Vec<String>,
    /// 外观：dark / light。
    pub theme_mode: Option<String>,
    /// 正文字号步进（官方 --dsh-content-font-delta，约 -2..=4）。
    pub content_font_delta: Option<i32>,
    /// 工作详情折叠：compact / standard / detailed / verbose。
    pub work_details: Option<String>,
    pub collapse_timing: Option<String>,
    pub folded_replies: Option<String>,
    pub workspace_open_with: Option<String>,
    /// Id of the blank session the client opened at startup, so the next launch
    /// reuses it instead of leaving one unused session on disk per launch.
    pub scratch_session: Option<String>,
    /// Most recently selected session, including nonempty conversations.
    pub selected_session: Option<String>,
    /// Chat HTTP(S) link destination: `sidebar` (built-in browser) or `new-tab`.
    pub link_open: Option<String>,
    /// Composer statistics detail level: `compact` or `detailed`.
    pub performance_usage: Option<String>,
    /// Files-search preferences by workspace root (official per-workspace
    /// sidebar-files search storage).
    pub files_search: HashMap<String, FilesSearchPreference>,
}

/// One Files-search exclusion rule as persisted.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FilesSearchRulePreference {
    pub path: String,
    pub enabled: bool,
}

/// One workspace root's persisted Files-search preferences.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FilesSearchPreference {
    /// Exclusion rules, newest first.
    pub rules: Vec<FilesSearchRulePreference>,
    /// Whether hidden entries participate; defaults to hidden.
    pub show_hidden: Option<bool>,
    /// Whether `.gitignore` rules apply; defaults to on.
    pub respect_gitignore: Option<bool>,
}

#[tauri::command]
pub async fn load_workspace_preferences(app: AppHandle) -> Result<WorkspacePreferences, String> {
    let path = preferences_path(&app)?;
    tokio::task::spawn_blocking(move || read_preferences(&path))
        .await
        .map_err(|error| format!("读取工作区显示设置失败：{error}"))?
}

#[tauri::command]
pub async fn save_workspace_preferences(
    app: AppHandle,
    preferences: WorkspacePreferences,
    onboarding_revision: Option<u64>,
) -> Result<(), String> {
    let path = preferences_path(&app)?;
    tokio::task::spawn_blocking(move || {
        edit_preferences(&path, |stored| {
            merge_workspace_save(stored, preferences, onboarding_revision);
            Ok(())
        })
    })
    .await
    .map_err(|error| format!("保存工作区显示设置失败：{error}"))?
}

fn merge_workspace_save(
    stored: &mut WorkspacePreferences,
    mut incoming: WorkspacePreferences,
    onboarding_revision: Option<u64>,
) {
    incoming.onboarding = stored.onboarding;
    incoming.developer_tools = stored.developer_tools;
    let stale = stored.onboarding.is_some_and(|record| {
        record.progress.step == crate::desktop_onboarding::protocol::Step::Done
            && Some(record.revision) != onboarding_revision
    });
    if stale {
        incoming.work_details = stored.work_details.take();
        incoming.performance_usage = stored.performance_usage.take();
    }
    *stored = incoming;
}

/// The stock ACP profile has no Web conversation form. Keep the desktop
/// preference in its own file so sidebar saves cannot overwrite it.
pub(crate) async fn desktop_composer_preference(
    app: &AppHandle,
    mode: Option<String>,
) -> Result<serde_json::Value, String> {
    let path = preferences_path(app)?.with_file_name("composer.json");
    let mode =
        tokio::task::spawn_blocking(move || read_or_save_composer_mode(&path, mode.as_deref()))
            .await
            .map_err(|error| format!("无法访问发送设置：{error}"))??;
    Ok(serde_json::json!({"mode": mode}))
}

fn read_or_save_composer_mode(path: &Path, requested: Option<&str>) -> Result<String, String> {
    let mode = if let Some(mode) = requested {
        mode.to_owned()
    } else {
        match std::fs::read(path) {
            Ok(bytes) => {
                serde_json::from_slice::<String>(&bytes).map_err(|error| error.to_string())?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => "queue".to_owned(),
            Err(error) => return Err(error.to_string()),
        }
    };
    if !matches!(mode.as_str(), "queue" | "steer") {
        return Err("发送行为无效。".to_owned());
    }
    if requested.is_some() {
        let directory = path.parent().ok_or("设置目录无效")?;
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let mut file =
            tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
        serde_json::to_writer(&mut file, &mode).map_err(|error| error.to_string())?;
        file.flush().map_err(|error| error.to_string())?;
        file.persist(path).map_err(|error| error.to_string())?;
    }
    Ok(mode)
}

#[tauri::command]
pub async fn load_developer_tools(webview: tauri::Webview) -> Result<bool, String> {
    let path = developer_tools_path(&webview)?;
    tokio::task::spawn_blocking(move || {
        if let Some(enabled) = read_preferences(&path)?.developer_tools {
            return Ok(enabled);
        }
        match std::fs::read(path.with_file_name("developer-tools.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.to_string()),
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn save_developer_tools(webview: tauri::Webview, enabled: bool) -> Result<bool, String> {
    let path = developer_tools_path(&webview)?;
    tokio::task::spawn_blocking(move || {
        edit_preferences(&path, |preferences| {
            preferences.developer_tools = Some(enabled);
            Ok(enabled)
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

fn developer_tools_path(webview: &tauri::Webview) -> Result<PathBuf, String> {
    if webview.label() != "main" {
        return Err("只有主界面可以访问代码工作视图设置。".into());
    }
    preferences_path(webview.app_handle())
}

pub(crate) fn preferences_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join("workspaces.json"))
        .map_err(|error| format!("无法确定工作区显示设置目录：{error}"))
}

pub(crate) fn read_preferences(path: &Path) -> Result<WorkspacePreferences, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("工作区显示设置格式无效：{error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(WorkspacePreferences::default())
        }
        Err(error) => Err(format!("无法读取工作区显示设置：{error}")),
    }
}

pub(crate) fn edit_preferences<T>(
    path: &Path,
    edit: impl FnOnce(&mut WorkspacePreferences) -> Result<T, String>,
) -> Result<T, String> {
    let directory = path.parent().ok_or("工作区显示设置路径无效")?;
    std::fs::create_dir_all(directory).map_err(|error| format!("无法创建设置目录：{error}"))?;
    let lock = std::fs::File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("lock"))
        .map_err(|error| format!("无法打开设置锁：{error}"))?;
    lock.lock()
        .map_err(|error| format!("无法锁定设置：{error}"))?;
    let mut preferences = read_preferences(path)?;
    let result = edit(&mut preferences)?;
    write_preferences(path, &preferences)?;
    Ok(result)
}

fn write_preferences(path: &Path, preferences: &impl Serialize) -> Result<(), String> {
    let directory = path.parent().ok_or("工作区显示设置路径无效")?;
    std::fs::create_dir_all(directory).map_err(|error| format!("无法创建设置目录：{error}"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)
        .map_err(|error| format!("无法创建临时设置文件：{error}"))?;
    let bytes = serde_json::to_vec_pretty(preferences)
        .map_err(|error| format!("无法编码工作区显示设置：{error}"))?;
    temporary
        .write_all(&bytes)
        .map_err(|error| format!("无法保存显示设置：{error}"))?;
    temporary
        .persist(path)
        .map(|_| ())
        .map_err(|error| format!("无法更新显示设置：{error}"))
}

#[cfg(test)]
mod tests {
    use super::{FilesSearchPreference, FilesSearchRulePreference, WorkspacePreferences};

    #[test]
    fn sidebar_save_keeps_newer_onboarding_choices_and_shared_developer_setting() {
        use crate::desktop_onboarding::protocol::{Progress, Snapshot, Step};
        let onboarding = Snapshot {
            revision: 6,
            progress: Progress {
                step: Step::Done,
                ..Progress::default()
            },
        };
        let mut stored = WorkspacePreferences {
            onboarding: Some(onboarding),
            developer_tools: Some(true),
            work_details: Some("detailed".into()),
            performance_usage: Some("detailed".into()),
            ..WorkspacePreferences::default()
        };
        super::merge_workspace_save(
            &mut stored,
            WorkspacePreferences {
                sidebar_width: Some(331.),
                work_details: Some("standard".into()),
                performance_usage: Some("compact".into()),
                ..WorkspacePreferences::default()
            },
            Some(5),
        );
        assert_eq!(stored.onboarding, Some(onboarding));
        assert_eq!(stored.developer_tools, Some(true));
        assert_eq!(stored.sidebar_width, Some(331.));
        assert_eq!(stored.work_details.as_deref(), Some("detailed"));
        assert_eq!(stored.performance_usage.as_deref(), Some("detailed"));
        super::merge_workspace_save(
            &mut stored,
            WorkspacePreferences {
                work_details: Some("compact".into()),
                performance_usage: Some("compact".into()),
                ..WorkspacePreferences::default()
            },
            Some(6),
        );
        assert_eq!(stored.work_details.as_deref(), Some("compact"));
        assert_eq!(stored.performance_usage.as_deref(), Some("compact"));
        assert_eq!(stored.developer_tools, Some(true));
    }

    #[test]
    fn desktop_composer_mode_defaults_and_persists_without_sidebar_writes() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("composer.json");
        assert_eq!(
            super::read_or_save_composer_mode(&config, None).unwrap(),
            "queue"
        );
        assert!(!config.exists());
        super::read_or_save_composer_mode(&config, Some("steer")).unwrap();
        assert!(super::read_or_save_composer_mode(&config, Some("invalid")).is_err());
        super::write_preferences(
            &directory.path().join("workspace-preferences.json"),
            &WorkspacePreferences::default(),
        )
        .unwrap();
        assert_eq!(
            super::read_or_save_composer_mode(&config, None).unwrap(),
            "steer"
        );
        super::read_or_save_composer_mode(&config, Some("queue")).unwrap();
        assert_eq!(
            super::read_or_save_composer_mode(&config, None).unwrap(),
            "queue"
        );
    }

    #[test]
    fn link_open_preference_round_trips_as_camel_case() {
        let preferences = WorkspacePreferences {
            link_open: Some("new-tab".to_string()),
            performance_usage: Some("compact".to_string()),
            ..WorkspacePreferences::default()
        };
        let encoded = serde_json::to_value(&preferences).expect("serialize preferences");
        assert_eq!(encoded["linkOpen"], "new-tab");
        assert_eq!(encoded["performanceUsage"], "compact");
        let decoded: WorkspacePreferences =
            serde_json::from_value(encoded).expect("deserialize preferences");
        assert_eq!(decoded.link_open.as_deref(), Some("new-tab"));
        assert_eq!(decoded.performance_usage.as_deref(), Some("compact"));
    }

    #[test]
    fn files_search_preferences_round_trip_as_camel_case() {
        let mut preferences = WorkspacePreferences::default();
        preferences.files_search.insert(
            "/Users/leon/Documents/KWork".to_string(),
            FilesSearchPreference {
                rules: vec![FilesSearchRulePreference {
                    path: "node_modules".to_string(),
                    enabled: false,
                }],
                show_hidden: Some(true),
                respect_gitignore: Some(false),
            },
        );
        preferences
            .files_search
            .insert("/tmp/other".to_string(), FilesSearchPreference::default());
        let encoded = serde_json::to_value(&preferences).expect("encode");
        let entry = &encoded["filesSearch"]["/Users/leon/Documents/KWork"];
        assert_eq!(entry["rules"][0]["path"], "node_modules");
        assert_eq!(entry["rules"][0]["enabled"], false);
        assert_eq!(entry["showHidden"], true);
        assert_eq!(entry["respectGitignore"], false);
        let decoded: WorkspacePreferences = serde_json::from_value(encoded).expect("decode");
        assert_eq!(
            decoded.files_search.get("/Users/leon/Documents/KWork"),
            preferences.files_search.get("/Users/leon/Documents/KWork")
        );
        // A file written before the search preferences existed still loads.
        let older: WorkspacePreferences =
            serde_json::from_str(r#"{"aliases":{}}"#).expect("older file");
        assert!(older.files_search.is_empty());
    }

    #[test]
    fn session_order_round_trips_and_old_files_default_to_empty() {
        let mut preferences = WorkspacePreferences::default();
        preferences.session_order_by_account.insert(
            "__flat_session_order__".to_string(),
            vec!["session-b".to_string(), "session-a".to_string()],
        );
        let encoded = serde_json::to_value(&preferences).expect("serialize session order");
        assert_eq!(
            encoded["sessionOrderByAccount"]["__flat_session_order__"],
            serde_json::json!(["session-b", "session-a"])
        );
        let decoded: WorkspacePreferences =
            serde_json::from_value(encoded).expect("deserialize session order");
        assert_eq!(
            decoded.session_order_by_account,
            preferences.session_order_by_account
        );
        let older: WorkspacePreferences =
            serde_json::from_str(r#"{"orderBy":"updated"}"#).expect("older file");
        assert!(older.session_order_by_account.is_empty());
    }

    #[test]
    fn workspace_order_round_trips_and_old_files_default_to_empty() {
        let preferences = WorkspacePreferences {
            workspace_order: vec!["/work/b".to_string(), "/work/a".to_string()],
            ..WorkspacePreferences::default()
        };
        let encoded = serde_json::to_value(&preferences).expect("serialize workspace order");
        assert_eq!(
            encoded["workspaceOrder"],
            serde_json::json!(["/work/b", "/work/a"])
        );
        let decoded: WorkspacePreferences =
            serde_json::from_value(encoded).expect("deserialize workspace order");
        assert_eq!(decoded.workspace_order, preferences.workspace_order);
        let older: WorkspacePreferences =
            serde_json::from_str(r#"{"addedWorkspaces":[]}"#).expect("older file");
        assert!(older.workspace_order.is_empty());
    }

    #[test]
    fn older_files_without_the_link_preference_still_load() {
        let decoded: WorkspacePreferences =
            serde_json::from_str(r#"{"workspaceOpenWith":"cursor"}"#)
                .expect("legacy preferences load");
        assert_eq!(decoded.link_open, None);
        assert_eq!(decoded.workspace_open_with.as_deref(), Some("cursor"));
    }
}
