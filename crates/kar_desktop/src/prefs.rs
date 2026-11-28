//! Durable client preferences, keybinding registry, and compatibility helpers for Kwrok.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use gpui::Keystroke;
use kar::ApprovalMode;
use kui::components::shortcut_recorder::ShortcutPlatform;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{types::ProviderModelOption, ui_primitives::UiLocale};

// ── Preference Enums ────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationLevel {
    Off,
    #[default]
    NeedsInput,
    All,
}

impl NotificationLevel {
    pub const ALL: [Self; 3] = [Self::Off, Self::NeedsInput, Self::All];

    pub fn label(self, loc: UiLocale) -> &'static str {
        match self {
            Self::Off => loc.pick("关闭提醒", "Off"),
            Self::NeedsInput => loc.pick("需我确认或出错时", "Input & errors"),
            Self::All => loc.pick("任务完成也提醒", "All events"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSortOrder {
    #[default]
    Recent,
    Alphabetical,
    Manual,
}

impl ProjectSortOrder {
    pub const ALL: [Self; 3] = [Self::Recent, Self::Alphabetical, Self::Manual];

    pub fn label(self, loc: UiLocale) -> &'static str {
        match self {
            Self::Recent => loc.pick("最近活跃", "Last activity"),
            Self::Alphabetical => loc.pick("按名称", "Alphabetical"),
            Self::Manual => loc.pick("添加顺序", "Added order"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuitShortcutMode {
    #[default]
    Immediate,
    PressTwice,
    Hold,
}

impl QuitShortcutMode {
    pub const ALL: [Self; 3] = [Self::Immediate, Self::PressTwice, Self::Hold];

    pub fn label(self, loc: UiLocale) -> &'static str {
        match self {
            Self::Immediate => loc.pick("直接退出", "Immediate"),
            Self::PressTwice => loc.pick("连按两次 ⌘Q", "Press twice"),
            Self::Hold => loc.pick("长按退出", "Hold"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontScale {
    Compact,
    #[default]
    Default,
    Comfortable,
    Large,
}

impl FontScale {
    pub const ALL: [Self; 4] = [Self::Compact, Self::Default, Self::Comfortable, Self::Large];

    pub fn delta_px(self) -> f32 {
        match self {
            Self::Compact => -1.0,
            Self::Default => 0.0,
            Self::Comfortable => 1.0,
            Self::Large => 2.0,
        }
    }

    pub fn label(self, loc: UiLocale) -> &'static str {
        match self {
            Self::Compact => loc.pick("紧凑", "Compact"),
            Self::Default => loc.pick("标准", "Default"),
            Self::Comfortable => loc.pick("舒适偏大", "Comfortable"),
            Self::Large => loc.pick("大字号", "Large"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatWidthMode {
    Compact,
    #[default]
    Standard,
    Wide,
    Full,
}

impl ChatWidthMode {
    pub const ALL: [Self; 4] = [Self::Compact, Self::Standard, Self::Wide, Self::Full];

    pub fn max_width_px(self) -> f32 {
        match self {
            Self::Compact => 640.0,
            Self::Standard => 736.0,
            Self::Wide => 940.0,
            Self::Full => f32::MAX,
        }
    }

    pub fn width_for_pane(self, pane_width: f32) -> f32 {
        pane_width.max(1.0).min(self.max_width_px())
    }

    pub fn label(self, loc: UiLocale) -> &'static str {
        match self {
            Self::Compact => loc.pick("紧凑阅读", "Compact"),
            Self::Standard => loc.pick("适中专注", "Standard"),
            Self::Wide => loc.pick("宽屏舒展", "Wide"),
            Self::Full => loc.pick("铺满窗口", "Full"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendShortcutMode {
    #[default]
    Enter,
    ModEnter,
}

impl SendShortcutMode {
    pub const ALL: [Self; 2] = [Self::Enter, Self::ModEnter];

    pub fn label(self, loc: UiLocale) -> &'static str {
        match self {
            Self::Enter => loc.pick("回车直接发送", "Enter"),
            Self::ModEnter => loc.pick("⌘+回车发送", "⌘Enter"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowUpMode {
    #[default]
    Queue,
    Steer,
}

impl FollowUpMode {
    pub const ALL: [Self; 2] = [Self::Queue, Self::Steer];

    pub fn label(self, loc: UiLocale) -> &'static str {
        match self {
            Self::Queue => loc.pick("等本轮结束再发", "Queue"),
            Self::Steer => loc.pick("立即插话引导", "Steer"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLayoutMode {
    #[default]
    Unified,
    Split,
}

impl DiffLayoutMode {
    pub const ALL: [Self; 2] = [Self::Unified, Self::Split];

    pub fn label(self, loc: UiLocale) -> &'static str {
        match self {
            Self::Unified => loc.pick("上下合并", "Unified"),
            Self::Split => loc.pick("左右并排", "Split"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceProviderKind {
    #[default]
    Local,
    OpenAi,
    Grok,
    OpenRouter,
}

impl VoiceProviderKind {
    pub const ALL: [Self; 4] = [Self::Local, Self::OpenAi, Self::Grok, Self::OpenRouter];

    pub fn key(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::OpenAi => "openai",
            Self::Grok => "grok",
            Self::OpenRouter => "openrouter",
        }
    }

    pub fn label(self, loc: UiLocale) -> &'static str {
        match self {
            Self::Local => loc.pick("本地离线", "Local STT"),
            Self::OpenAi => "OpenAI Whisper",
            Self::Grok => "xAI Grok",
            Self::OpenRouter => "OpenRouter",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationTriggerMode {
    #[default]
    Toggle,
    Hold,
}

impl DictationTriggerMode {
    pub const ALL: [Self; 2] = [Self::Toggle, Self::Hold];

    pub fn label(self, loc: UiLocale) -> &'static str {
        match self {
            Self::Toggle => loc.pick("点击开始/结束", "Click toggle"),
            Self::Hold => loc.pick("按住说话", "Hold to talk"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitStyleMode {
    #[default]
    RepoConventions,
    ConventionalCommits,
    Custom,
}

impl CommitStyleMode {
    pub fn label(self, loc: UiLocale) -> &'static str {
        match self {
            Self::RepoConventions => loc.pick("跟随仓库惯例", "Repo conventions"),
            Self::ConventionalCommits => "Conventional Commits",
            Self::Custom => loc.pick("自定义提示词", "Custom"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrMergeMethod {
    #[default]
    Squash,
    Merge,
    Rebase,
}

impl PrMergeMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Squash => "Squash & Merge",
            Self::Merge => "Merge Commit",
            Self::Rebase => "Rebase & Merge",
        }
    }

    pub fn gh_flag(self) -> &'static str {
        match self {
            Self::Squash => "--squash",
            Self::Merge => "--merge",
            Self::Rebase => "--rebase",
        }
    }
}

// ── Structured Preference Records ───────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ModelKey {
    pub runtime: String,
    pub model: String,
}

impl ModelKey {
    pub fn new(runtime: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            runtime: runtime.into(),
            model: model.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomAcpAgent {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectScript {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub run_on_worktree_create: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedRemoteEnv {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub token: Option<String>,
    pub peer_id: Option<Uuid>,
    pub read_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshSimulatorHost {
    pub id: String,
    pub label: String,
    pub destination: String,
    pub port: u16,
}

impl SshSimulatorHost {
    pub(crate) fn backend_host(&self)->serde_json::Value {
        serde_json::json!({"id":self.id,"label":self.label,"target":self.destination,"port":self.port})
    }
}

fn default_true() -> bool {
    true
}
fn default_terminal_h() -> f32 {
    280.0
}
fn default_code_font_size() -> f32 {
    13.0
}
fn default_voice_model() -> String {
    "whisper-small".into()
}
fn default_voice_lang() -> String {
    "auto".into()
}
fn default_editor() -> String {
    "auto".into()
}
fn default_wt_prefix() -> String {
    "kar/wt-".into()
}
fn default_channel() -> String {
    "stable".into()
}

/// Forward-compatible `ApprovalMode` helpers so `kar_desktop` supports `"allow"` (Full Auto)
/// smoothly whether `kar::ApprovalMode::Allow` is compiled in or deserialized.
pub fn approval_mode_allow() -> Option<ApprovalMode> {
    serde_json::from_str::<ApprovalMode>("\"allow\"").ok()
}

pub fn is_approval_allow(mode: ApprovalMode) -> bool {
    #[allow(unreachable_patterns)]
    match mode {
        ApprovalMode::Host | ApprovalMode::Deny => false,
        _ => true,
    }
}

pub fn all_approval_modes() -> Vec<ApprovalMode> {
    let mut list = vec![ApprovalMode::Host, ApprovalMode::Deny];
    if let Some(allow) = approval_mode_allow() {
        list.insert(1, allow);
    }
    list
}

// ── Master `DesktopPreferences` ─────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopPreferences {
    // 1. Persisted workspace shell state
    pub sidebar_open: Option<bool>,
    pub thread_details_open: bool,
    pub tree_show_hidden: bool,
    pub bottom_terminal_open: bool,
    #[serde(default = "default_terminal_h")]
    pub bottom_terminal_h: f32,
    pub active_project_filter: Option<PathBuf>,
    pub settled_chats: BTreeSet<Uuid>,
    pub snoozed_chats: BTreeMap<Uuid, u64>,
    #[serde(default = "default_true")]
    pub time_format_24h: bool,

    // 2. Notifications
    pub system_notifications: NotificationLevel,
    #[serde(default = "default_true")]
    pub in_app_notifications: bool,
    pub notification_sound: bool,

    // 3. Sidebar grouping & order
    pub sidebar_project_order: ProjectSortOrder,
    /// Prioritize working chats within the current list or project.
    #[serde(default = "default_true")]
    pub sidebar_working_section: bool,

    // 4. Confirmations & Quit safety
    pub confirm_unpin: bool,
    pub confirm_archive: bool,
    #[serde(default = "default_true")]
    pub confirm_delete: bool,
    #[serde(default = "default_true")]
    pub confirm_terminal_close: bool,
    pub quit_shortcut_mode: QuitShortcutMode,

    // 5. Appearance, Fonts & Themes
    pub named_theme_id: Option<String>,
    pub ui_font_scale: FontScale,
    #[serde(default = "default_code_font_size")]
    pub code_font_size: f32,
    pub code_font_family: Option<String>,
    pub chat_width_mode: ChatWidthMode,
    #[serde(default = "default_true")]
    pub word_wrap_code: bool,

    // 6. Chat & Diff UX
    pub send_shortcut: SendShortcutMode,
    pub follow_up_mode: FollowUpMode,
    pub default_collapse_tools: bool,
    pub diff_layout: DiffLayoutMode,
    pub diff_ignore_whitespace: bool,
    pub proactive_diff_panel: bool,
    #[serde(default = "default_true")]
    pub rollback_default_files: bool,
    #[serde(default = "default_true")]
    pub prompt_suggestions_enabled: bool,

    // 7. Models, Fallback & Quota Recovery
    pub favorite_models: Vec<ModelKey>,
    pub hidden_models: BTreeSet<ModelKey>,
    pub custom_models: BTreeMap<String, Vec<String>>,
    pub custom_acp_agents: Vec<CustomAcpAgent>,
    pub fallback_runtime: Option<String>,
    pub fallback_model: Option<String>,
    #[serde(default = "default_true")]
    pub auto_pause_on_quota_limit: bool,
    pub auto_resume_after_quota_reset: bool,

    // 8. Custom Keybindings
    pub custom_keybindings: BTreeMap<String, String>,

    // 9. Voice Dictation
    pub voice_provider: VoiceProviderKind,
    #[serde(default = "default_voice_model")]
    pub voice_local_model: String,
    pub voice_openrouter_model: Option<String>,
    #[serde(default = "default_voice_lang")]
    pub voice_language: String,
    pub voice_dictation_mode: DictationTriggerMode,
    #[serde(default = "default_true")]
    pub voice_remove_fillers: bool,
    pub voice_custom_words: Vec<String>,
    pub voice_api_keys: BTreeMap<String, String>,

    // 10. Projects, Source Control, Forge & Worktrees
    pub github_token: String,
    pub gitlab_token: String,
    pub linear_token: String,
    pub commit_writing_style: CommitStyleMode,
    pub commit_custom_instructions: String,
    pub pr_default_merge_method: PrMergeMethod,
    pub auto_pull_on_switch: bool,
    #[serde(default = "default_editor")]
    pub preferred_external_editor: String,
    #[serde(default = "default_true")]
    pub new_worktrees_from_origin: bool,
    #[serde(default = "default_wt_prefix")]
    pub worktree_branch_prefix: String,
    pub project_scripts: BTreeMap<String, Vec<ProjectScript>>,

    // 11. Remote & Sharing
    pub lan_listen_enabled: bool,
    pub saved_environments: Vec<SavedRemoteEnv>,

    // 12. Devices, Maintenance & Updates
    #[serde(default = "default_true")]
    pub devices_enabled: bool,
    #[serde(default = "default_true")]
    pub devices_agent_access: bool,
    pub device_ssh_hosts: Vec<SshSimulatorHost>,
    #[serde(default = "default_true")]
    pub auto_show_dev_preview_pill: bool,
    pub keep_awake_while_working: bool,
    #[serde(default = "default_true")]
    pub auto_check_provider_updates: bool,
    #[serde(default = "default_channel")]
    pub update_channel: String,
}

impl Default for DesktopPreferences {
    fn default() -> Self {
        Self {
            sidebar_open: None,
            thread_details_open: true,
            tree_show_hidden: false,
            bottom_terminal_open: false,
            bottom_terminal_h: default_terminal_h(),
            active_project_filter: None,
            settled_chats: BTreeSet::new(),
            snoozed_chats: BTreeMap::new(),
            time_format_24h: true,
            system_notifications: NotificationLevel::NeedsInput,
            in_app_notifications: true,
            notification_sound: false,
            sidebar_project_order: ProjectSortOrder::Recent,
            sidebar_working_section: true,
            confirm_unpin: false,
            confirm_archive: false,
            confirm_delete: true,
            confirm_terminal_close: true,
            quit_shortcut_mode: QuitShortcutMode::Immediate,
            named_theme_id: None,
            ui_font_scale: FontScale::Default,
            code_font_size: default_code_font_size(),
            code_font_family: None,
            chat_width_mode: ChatWidthMode::Standard,
            word_wrap_code: true,
            send_shortcut: SendShortcutMode::Enter,
            follow_up_mode: FollowUpMode::Queue,
            default_collapse_tools: false,
            diff_layout: DiffLayoutMode::Unified,
            diff_ignore_whitespace: false,
            proactive_diff_panel: false,
            rollback_default_files: true,
            prompt_suggestions_enabled: true,
            favorite_models: Vec::new(),
            hidden_models: BTreeSet::new(),
            custom_models: BTreeMap::new(),
            custom_acp_agents: Vec::new(),
            fallback_runtime: None,
            fallback_model: None,
            auto_pause_on_quota_limit: true,
            auto_resume_after_quota_reset: false,
            custom_keybindings: BTreeMap::new(),
            voice_provider: VoiceProviderKind::Local,
            voice_local_model: default_voice_model(),
            voice_openrouter_model: None,
            voice_language: default_voice_lang(),
            voice_dictation_mode: DictationTriggerMode::Toggle,
            voice_remove_fillers: true,
            voice_custom_words: Vec::new(),
            voice_api_keys: BTreeMap::new(),
            github_token: String::new(),
            gitlab_token: String::new(),
            linear_token: String::new(),
            commit_writing_style: CommitStyleMode::RepoConventions,
            commit_custom_instructions: String::new(),
            pr_default_merge_method: PrMergeMethod::Squash,
            auto_pull_on_switch: false,
            preferred_external_editor: default_editor(),
            new_worktrees_from_origin: true,
            worktree_branch_prefix: default_wt_prefix(),
            project_scripts: BTreeMap::new(),
            lan_listen_enabled: false,
            saved_environments: Vec::new(),
            devices_enabled: true,
            devices_agent_access: true,
            device_ssh_hosts: Vec::new(),
            auto_show_dev_preview_pill: true,
            keep_awake_while_working: false,
            auto_check_provider_updates: true,
            update_channel: default_channel(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct KeybindingFileEntry {
    pub key: String,
    pub command: String,
}

impl DesktopPreferences {
    pub fn file_path(store_dir: &Path) -> PathBuf {
        store_dir.join("desktop_preferences.json")
    }

    pub fn keybindings_json_path(store_dir: &Path) -> PathBuf {
        store_dir.join("keybindings.json")
    }

    pub fn load(store_dir: &Path) -> Self {
        let mut prefs: Self = std::fs::read_to_string(Self::file_path(store_dir))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        prefs.active_project_filter = prefs.active_project_filter.as_deref()
            .map(crate::studio_hub::normalize_project_path);
        prefs.code_font_size = prefs.code_font_size.clamp(11.0, 18.0);
        prefs.bottom_terminal_h = prefs.bottom_terminal_h.clamp(176.0, 640.0);

        // Merge any external edits made directly to `keybindings.json`.
        if let Ok(raw) = std::fs::read_to_string(Self::keybindings_json_path(store_dir))
            && let Ok(entries) = serde_json::from_str::<Vec<KeybindingFileEntry>>(&raw)
        {
            for entry in entries {
                if !entry.command.trim().is_empty() && !entry.key.trim().is_empty() {
                    prefs
                        .custom_keybindings
                        .insert(entry.command.trim().to_owned(), entry.key.trim().to_owned());
                }
            }
        }
        prefs
    }

    pub fn save(&self, store_dir: &Path) {
        let _ = std::fs::create_dir_all(store_dir);
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let tmp = store_dir.join(format!(".desktop_preferences-{}.tmp", Uuid::new_v4()));
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, Self::file_path(store_dir));
            } else {
                let _ = std::fs::remove_file(tmp);
            }
        }
        self.save_keybindings_json(store_dir);
    }

    pub fn save_keybindings_json(&self, store_dir: &Path) {
        let _ = std::fs::create_dir_all(store_dir);
        let entries: Vec<KeybindingFileEntry> = KEYBINDING_COMMANDS
            .iter()
            .map(|def| KeybindingFileEntry {
                key: self.effective_chord(def.id).to_owned(),
                command: def.id.to_owned(),
            })
            .collect();
        if let Ok(json) = serde_json::to_vec_pretty(&entries) {
            let tmp = store_dir.join(format!(".keybindings-{}.tmp", Uuid::new_v4()));
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, Self::keybindings_json_path(store_dir));
            } else {
                let _ = std::fs::remove_file(tmp);
            }
        }
    }

    pub fn is_model_favorite(&self, runtime: &str, model: &str) -> bool {
        self.favorite_models
            .iter()
            .any(|k| k.runtime == runtime && k.model == model)
    }

    pub fn toggle_model_favorite(&mut self, runtime: &str, model: &str) {
        if let Some(pos) = self
            .favorite_models
            .iter()
            .position(|k| k.runtime == runtime && k.model == model)
        {
            self.favorite_models.remove(pos);
        } else {
            self.favorite_models.push(ModelKey::new(runtime, model));
        }
    }

    pub fn is_model_hidden(&self, runtime: &str, model: &str) -> bool {
        self.hidden_models
            .iter()
            .any(|k| k.runtime == runtime && k.model == model)
    }

    pub(crate) fn has_custom_model(&self, runtime: &str, model: &str) -> bool {
        self.custom_models
            .get(runtime)
            .is_some_and(|models| models.iter().any(|id| id == model))
    }

    /// The composer and agent settings should offer the same models for a provider.
    pub(crate) fn model_choices(
        &self,
        runtime: &str,
        reported: &[ProviderModelOption],
        selected: Option<&str>,
        query: &str,
        show_hidden: bool,
        loc: UiLocale,
    ) -> Vec<(String, String)> {
        let mut choices: Vec<_> = reported
            .iter()
            .map(|model| {
                let label = if model.label.trim().is_empty() {
                    &model.id
                } else {
                    &model.label
                };
                (model.id.clone(), label.clone())
            })
            .collect();
        if let Some(custom) = self.custom_models.get(runtime) {
            for id in custom {
                if !choices.iter().any(|(known, _)| known == id) {
                    choices.push((
                        id.clone(),
                        format!("{id} · {}", loc.pick("自定义", "Custom")),
                    ));
                }
            }
        }
        choices.retain(|(id, label)| {
            (show_hidden || selected == Some(id.as_str()) || !self.is_model_hidden(runtime, id))
                && (query.is_empty()
                    || id.to_lowercase().contains(query)
                    || label.to_lowercase().contains(query))
        });
        choices.sort_by_key(|(id, _)| !self.is_model_favorite(runtime, id));
        choices
    }

    pub fn toggle_model_hidden(&mut self, runtime: &str, model: &str) {
        let key = ModelKey::new(runtime, model);
        if !self.hidden_models.remove(&key) {
            self.hidden_models.insert(key);
        }
    }

    pub fn effective_chord<'a>(&'a self, cmd_id: &str) -> &'a str {
        if let Some(custom) = self.custom_keybindings.get(cmd_id) {
            return custom.as_str();
        }
        KEYBINDING_COMMANDS
            .iter()
            .find(|d| d.id == cmd_id)
            .map_or("", |d| d.default_chord)
    }

    pub fn effective_badge(&self, cmd_id: &str) -> String {
        format_chord_badge(self.effective_chord(cmd_id))
    }

    pub fn matches_command(&self, cmd_id: &str, ks: &Keystroke) -> bool {
        let chord = self.effective_chord(cmd_id);
        !chord.is_empty() && ShortcutPlatform::detected().matches_keystroke(chord, ks)
    }

    pub fn find_conflict(&self, editing_cmd_id: &str, candidate_chord: &str) -> Option<&'static KeybindingDef> {
        let platform = ShortcutPlatform::detected();
        KEYBINDING_COMMANDS.iter().find(|def| {
            def.id != editing_cmd_id && platform.matches(self.effective_chord(def.id), candidate_chord)
        })
    }
}

// ── Built-in Keybinding Command Table ───────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct KeybindingDef {
    pub id: &'static str,
    pub category_zh: &'static str,
    pub category_en: &'static str,
    pub label_zh: &'static str,
    pub label_en: &'static str,
    pub default_chord: &'static str,
}

impl KeybindingDef {
    pub fn label(&self, loc: UiLocale) -> &'static str {
        loc.pick(self.label_zh, self.label_en)
    }

    pub fn category(&self, loc: UiLocale) -> &'static str {
        loc.pick(self.category_zh, self.category_en)
    }
}

pub const KEYBINDING_COMMANDS: &[KeybindingDef] = &[
    // 通用与窗口 (Application)
    KeybindingDef {
        id: "app.command_palette",
        category_zh: "通用导航",
        category_en: "General",
        label_zh: "打开命令面板与全局搜索",
        label_en: "Open command palette & search",
        default_chord: "cmd+k",
    },
    KeybindingDef {
        id: "app.settings",
        category_zh: "通用导航",
        category_en: "General",
        label_zh: "打开工作台设置",
        label_en: "Open settings",
        default_chord: "cmd+,",
    },
    KeybindingDef {
        id: "theme.toggle",
        category_zh: "通用导航",
        category_en: "General",
        label_zh: "切换浅色 / 深色外观",
        label_en: "Toggle light / dark theme",
        default_chord: "cmd+alt+t",
    },
    KeybindingDef {
        id: "locale.toggle",
        category_zh: "通用导航",
        category_en: "General",
        label_zh: "切换中英文界面语言",
        label_en: "Switch interface language",
        default_chord: "cmd+alt+l",
    },
    // 对话与智能体 (Chat & Agents)
    KeybindingDef {
        id: "chat.new",
        category_zh: "对话与智能体",
        category_en: "Chat & Agents",
        label_zh: "新建对话草稿",
        label_en: "New thread draft",
        default_chord: "cmd+n",
    },
    KeybindingDef {
        id: "chat.find",
        category_zh: "对话与智能体",
        category_en: "Chat & Agents",
        label_zh: "在当前对话内查找",
        label_en: "Find in active thread",
        default_chord: "cmd+f",
    },
    KeybindingDef {
        id: "chat.handoff",
        category_zh: "对话与智能体",
        category_en: "Chat & Agents",
        label_zh: "跨智能体便携交接对话",
        label_en: "Handoff thread to another agent",
        default_chord: "cmd+shift+h",
    },
    KeybindingDef {
        id: "chat.side_question",
        category_zh: "对话与智能体",
        category_en: "Chat & Agents",
        label_zh: "发起零污染旁路提问",
        label_en: "Ask ephemeral side question",
        default_chord: "cmd+shift+/",
    },
    KeybindingDef {
        id: "chat.voice_dictate",
        category_zh: "对话与智能体",
        category_en: "Chat & Agents",
        label_zh: "启动 / 停止语音听写",
        label_en: "Start / stop voice dictation",
        default_chord: "cmd+shift+v",
    },
    KeybindingDef {
        id: "chat.compact",
        category_zh: "对话与智能体",
        category_en: "Chat & Agents",
        label_zh: "立即压缩当前上下文",
        label_en: "Compact conversation context",
        default_chord: "cmd+shift+c",
    },
    // 面板与布局 (Panels & Layout)
    KeybindingDef {
        id: "view.toggle_sidebar",
        category_zh: "面板与布局",
        category_en: "Panels & Layout",
        label_zh: "显示 / 隐藏左侧会话栏",
        label_en: "Toggle left sidebar",
        default_chord: "cmd+.",
    },
    KeybindingDef {
        id: "view.toggle_terminal",
        category_zh: "面板与布局",
        category_en: "Panels & Layout",
        label_zh: "显示 / 隐藏底部交互终端",
        label_en: "Toggle bottom PTY terminal",
        default_chord: "ctrl+`",
    },
    KeybindingDef {
        id: "view.toggle_right_dock",
        category_zh: "面板与布局",
        category_en: "Panels & Layout",
        label_zh: "展开 / 收起右侧工作区面板",
        label_en: "Toggle right workspace dock",
        default_chord: "cmd+j",
    },
    KeybindingDef {
        id: "view.open_inbox",
        category_zh: "面板与布局",
        category_en: "Panels & Layout",
        label_zh: "打开待办与后台任务收件箱",
        label_en: "Open tasks & approvals inbox",
        default_chord: "cmd+shift+t",
    },
    KeybindingDef {
        id: "view.open_gallery",
        category_zh: "面板与布局",
        category_en: "Panels & Layout",
        label_zh: "打开办公与 PDF 文档画廊",
        label_en: "Open Office & PDF gallery",
        default_chord: "cmd+shift+o",
    },
    KeybindingDef {
        id: "view.open_browser",
        category_zh: "面板与布局",
        category_en: "Panels & Layout",
        label_zh: "打开内置浏览器与实时预览",
        label_en: "Open built-in browser & preview",
        default_chord: "cmd+shift+b",
    },
    // 代码、Git 与办公文档 (Code, Git & Office)
    KeybindingDef {
        id: "view.open_git",
        category_zh: "代码与文档",
        category_en: "Code & Office",
        label_zh: "打开 Git 变更与分支面板",
        label_en: "Open Git changes & graph",
        default_chord: "cmd+shift+g",
    },
    KeybindingDef {
        id: "view.open_prs",
        category_zh: "代码与文档",
        category_en: "Code & Office",
        label_zh: "打开 Pull Requests 代码审查面板",
        label_en: "Open Pull Requests panel",
        default_chord: "cmd+shift+p",
    },
    KeybindingDef {
        id: "view.open_issues",
        category_zh: "代码与文档",
        category_en: "Code & Office",
        label_zh: "打开 Issues 议题追踪面板",
        label_en: "Open Issues panel",
        default_chord: "cmd+shift+i",
    },
    KeybindingDef {
        id: "view.open_files",
        category_zh: "代码与文档",
        category_en: "Code & Office",
        label_zh: "打开工作区文件树",
        label_en: "Open workspace file tree",
        default_chord: "cmd+shift+e",
    },
    KeybindingDef {
        id: "office.save",
        category_zh: "代码与文档",
        category_en: "Code & Office",
        label_zh: "保存当前打开的办公文档",
        label_en: "Save active Office / PDF document",
        default_chord: "cmd+s",
    },
    KeybindingDef {
        id: "editor.open_external",
        category_zh: "代码与文档",
        category_en: "Code & Office",
        label_zh: "在外部编辑器中打开工作区",
        label_en: "Open workspace in external editor",
        default_chord: "cmd+alt+e",
    },
    // 设置快捷直达 (Settings Pages)
    KeybindingDef {
        id: "settings.agents",
        category_zh: "设置直达",
        category_en: "Settings Pages",
        label_zh: "打开设置 · 智能体与账号",
        label_en: "Settings · Providers & accounts",
        default_chord: "cmd+shift+a",
    },
    KeybindingDef {
        id: "settings.skills_mcp",
        category_zh: "设置直达",
        category_en: "Settings Pages",
        label_zh: "打开设置 · 技能与 MCP",
        label_en: "Settings · Skills & MCP",
        default_chord: "cmd+shift+m",
    },
    KeybindingDef {
        id: "settings.usage",
        category_zh: "设置直达",
        category_en: "Settings Pages",
        label_zh: "打开设置 · 用量与缓存",
        label_en: "Settings · Usage & cache",
        default_chord: "cmd+shift+u",
    },
    KeybindingDef {
        id: "settings.schedules",
        category_zh: "设置直达",
        category_en: "Settings Pages",
        label_zh: "打开设置 · 定时自动化",
        label_en: "Settings · Scheduled tasks",
        default_chord: "cmd+alt+s",
    },
    KeybindingDef {
        id: "settings.keybindings",
        category_zh: "设置直达",
        category_en: "Settings Pages",
        label_zh: "打开设置 · 快捷键",
        label_en: "Settings · Keybindings",
        default_chord: "cmd+shift+k",
    },
    KeybindingDef {
        id: "settings.devices",
        category_zh: "设置直达",
        category_en: "Settings Pages",
        label_zh: "打开设置 · 移动设备与预览",
        label_en: "Settings · Devices & preview",
        default_chord: "cmd+shift+d",
    },
    KeybindingDef {
        id: "settings.remote",
        category_zh: "设置直达",
        category_en: "Settings Pages",
        label_zh: "打开设置 · 远端配对与分享",
        label_en: "Settings · Sharing & environments",
        default_chord: "cmd+shift+r",
    },
    KeybindingDef {
        id: "settings.logs",
        category_zh: "设置直达",
        category_en: "Settings Pages",
        label_zh: "打开设置 · 维护、日志与关于",
        label_en: "Settings · Maintenance & logs",
        default_chord: "cmd+shift+l",
    },
];

/// Convert a normalized chord string (e.g. `"cmd+shift+g"`) into compact macOS glyph notation (`"⌘⇧G"`).
pub fn format_chord_badge(chord: &str) -> String {
    if chord.trim().is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for tok in chord.split('+').map(|t| t.trim().to_ascii_lowercase()) {
        match tok.as_str() {
            "cmd" | "meta" | "mod" | "super" => out.push('⌘'),
            "ctrl" | "control" => out.push('⌃'),
            "alt" | "opt" | "option" => out.push('⌥'),
            "shift" => out.push('⇧'),
            "enter" | "return" => out.push('↵'),
            "escape" | "esc" => out.push_str("Esc"),
            "space" => out.push_str("Space"),
            "backspace" => out.push('⌫'),
            "tab" => out.push('⇥'),
            "up" | "arrowup" => out.push('↑'),
            "down" | "arrowdown" => out.push('↓'),
            "left" | "arrowleft" => out.push('←'),
            "right" | "arrowright" => out.push('→'),
            other => out.push_str(&other.to_ascii_uppercase()),
        }
    }
    out
}

/// Record a user's `Keystroke` into a normalized chord string (`"cmd+shift+k"`), ignoring bare modifier keys.
pub fn chord_from_keystroke(ks: &Keystroke) -> Option<String> {
    let key = ks.key.trim().to_ascii_lowercase();
    if key.is_empty()
        || matches!(
            key.as_str(),
            "shift" | "control" | "alt" | "platform" | "meta" | "super" | "capslock" | "fn"
        )
    {
        return None;
    }
    let m = ks.modifiers;
    if !m.platform && !m.control && !m.alt && !key.starts_with('f') {
        return None;
    }
    let mut parts = Vec::new();
    if m.platform {
        parts.push("cmd".to_owned());
    }
    if m.control {
        parts.push("ctrl".to_owned());
    }
    if m.alt {
        parts.push("alt".to_owned());
    }
    if m.shift {
        parts.push("shift".to_owned());
    }
    let norm_key = match key.as_str() {
        "?" => {
            if !m.shift {
                parts.push("shift".to_owned());
            }
            "/".to_owned()
        }
        other => other.to_owned(),
    };
    parts.push(norm_key);
    Some(parts.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_choices_share_custom_favorites_and_retain_the_hidden_selection() {
        let mut prefs = DesktopPreferences::default();
        let runtime = "codex";
        prefs
            .custom_models
            .insert(runtime.into(), vec!["custom".into(), "live".into()]);
        prefs
            .custom_models
            .insert("other".into(), vec!["unrelated".into()]);
        prefs.toggle_model_favorite(runtime, "custom");
        prefs.toggle_model_hidden(runtime, "hidden");
        let model = |id: &str, label: &str| ProviderModelOption {
            id: id.into(),
            label: label.into(),
            is_default: false,
            effort_levels: vec![],
        };
        let models = vec![model("live", "Published"), model("hidden", "")];
        let choices = |selected, query, hidden| {
            prefs.model_choices(runtime, &models, selected, query, hidden, UiLocale::En)
        };
        assert_eq!(
            choices(None, "", false),
            [
                ("custom".into(), "custom · Custom".into()),
                ("live".into(), "Published".into())
            ]
        );
        assert_eq!(
            choices(None, "published", false),
            [("live".into(), "Published".into())]
        );
        assert!(choices(None, "hidden", false).is_empty());
        assert_eq!(
            choices(Some("hidden"), "hidden", false),
            [("hidden".into(), "hidden".into())]
        );
        assert_eq!(
            choices(None, "hidden", true),
            choices(Some("hidden"), "hidden", false)
        );
    }

    #[test]
    fn reading_width_fits_narrow_panes_and_can_fill_an_ultrawide_pane() {
        assert_eq!(
            ChatWidthMode::ALL.map(|mode| mode.width_for_pane(480.)),
            [480.; 4]
        );
        assert_eq!(
            ChatWidthMode::ALL.map(|mode| mode.width_for_pane(2048.)),
            [640., 736., 940., 2048.]
        );
    }

    #[test]
    fn preferences_roundtrip_and_sync_keybindings_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            DesktopPreferences::file_path(dir.path()),
            r#"{"sidebar_open":false}"#,
        )
        .expect("write preferences from an earlier version");
        let mut prefs = DesktopPreferences::load(dir.path());
        assert!(prefs.thread_details_open);
        assert_eq!(prefs.sidebar_open, Some(false));
        assert_eq!(prefs.active_project_filter, None);
        prefs.active_project_filter = Some(dir.path().join("."));
        let cid = Uuid::new_v4();
        prefs.settled_chats.insert(cid);
        prefs.tree_show_hidden = true;
        prefs.thread_details_open = false;
        prefs.ui_font_scale = FontScale::Large;
        prefs.chat_width_mode = ChatWidthMode::Full;
        prefs.code_font_family = Some("Monaco".into());
        prefs.code_font_size = 16.0;
        prefs
            .custom_keybindings
            .insert("app.settings".into(), "cmd+shift+,".into());
        prefs.toggle_model_favorite("claude", "claude-opus-4-6");
        prefs.save(dir.path());

        let loaded = DesktopPreferences::load(dir.path());
        assert!(loaded.settled_chats.contains(&cid));
        assert_eq!(loaded.active_project_filter, Some(dir.path().canonicalize().unwrap()));
        assert!(loaded.tree_show_hidden);
        assert_eq!(loaded.sidebar_open, Some(false));
        assert!(!loaded.thread_details_open);
        assert_eq!(loaded.ui_font_scale, FontScale::Large);
        assert_eq!(loaded.chat_width_mode, ChatWidthMode::Full);
        assert_eq!(loaded.code_font_family.as_deref(), Some("Monaco"));
        assert_eq!(loaded.code_font_size, 16.0);
        assert_eq!(loaded.effective_chord("app.settings"), "cmd+shift+,");
        assert!(loaded.is_model_favorite("claude", "claude-opus-4-6"));
        assert!(DesktopPreferences::keybindings_json_path(dir.path()).exists());
    }

    #[test]
    fn conflict_detection_catches_duplicate_chords() {
        let prefs = DesktopPreferences::default();
        let conflict = prefs.find_conflict("chat.new", "cmd+k");
        assert_eq!(conflict.map(|c| c.id), Some("app.command_palette"));
        assert!(prefs.find_conflict("app.command_palette", "cmd+k").is_none());
    }

    #[test]
    fn chord_badge_formats_macos_modifiers_cleanly() {
        assert_eq!(format_chord_badge("cmd+shift+g"), "⌘⇧G");
        assert_eq!(format_chord_badge("cmd+,"), "⌘,");
    }
}
