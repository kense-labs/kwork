//! The one window: architectural shell geometry (`AppShell` inset sheets), application state,
//! KUI Command Palette (`⌘K`), and everything a person can do in Kar Desktop.
//!
//! Rendering lives in the `ui_*` modules. This file owns the state those modules draw and the
//! actions they trigger, and talks to Kar strictly through [`KarBridgeHandle`].

mod aster_plan;
mod commands;
mod chat_goal;
mod hardware;
pub(crate) use hardware::speech_suffix;
mod slash_actions;
mod planning;
mod resize;
mod read_receipts;
mod provider_sign_in;
use resize::{Divider, PaneDrag, Span};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{prelude::*, *};
use kar::{
    ApprovalMode,
    chat::{Item, TurnStatus},
};
use kui::{
    ThemeMode,
    components::{
        branch_graph::BranchGraph,
        code_block::CodeBlock,
        code_suggestion::PatchReview,
        confirm_morph::{ConfirmMorph, ConfirmRequest, Intent as ConfirmIntent},
        copy_button::CopyButton,
        dialog::Dialog,
        file_dropzone::FileDropzone,
        glass_segmented::{GlassSegmentChanged, GlassSegmentOption, GlassSegmented},
        hold_to_confirm::HoldToConfirm,
        input::Input,
        json_viewer::JsonViewer,
        log_stream::LogStream,
        presence_stack::{PresencePerson, PresenceSize, PresenceStack, PresenceStatus},
        pro_chrome::{pro_presence, pro_view_transition_fill},
        tree_view::{TreeActivated, TreeNode, TreeView},
    },
    primitives::{
        disclosure::SurfaceEvent,
        editor::{EditEvent, Editor, EditorShortcutEvent, InputMode},
        family::Slide,
        pro_icons::{ProIcon, pro_icon},
    },
};
use uuid::Uuid;

use crate::{
    brief::{BriefBlock, BriefInventory, KarBrief, build_showcase_kui_brief},
    client_store::{ClientReducer, TurnCapture},
    coedit::{
        CoEditService, DemoCoEditEngine, DemoEditorMutation, FenceMutationPayload, LiveTargetState,
        RevisionFenceLedger, ScenarioKey, SubmissionOutcome, UserResolutionChoice,
    },
    kar_bridge::{ChatCreateFailure, KarBridgeEvent, KarBridgeHandle, KarCommand, LocalChatDestination, ProjectedChatState},
    office_host::{OfficeEditorView, OfficeTab, clean_doc_para_text, materialize_coedit_demo_files},
    remote_link::{PeerRole, QueueCopyError, RemoteAction, RemoteExtendedSnapshot, RemoteHostTelemetry, now_epoch_ms},
    schedules::{ScheduleCadence, ScheduleRegistry, ScheduleRunOutcome, ScheduledTask, workflow},
    studio_hub::{
        DiagLevel, DiagLogRing, DiffHunkData, GitWorkspaceSnapshot, GlobalSearchHit, InventoryTab,
        ProjectRegistry, TerminalRunRecord, UsageAnalyticsSnapshot, WorkspaceFilePreview,
        WorkspaceInventorySnapshot, WorkspaceTreeItem,
    },
    studio_window_options,
    types::{
        ChatAccessMode, ChatListEntry, FeedConnectionState, OfficeKind, PendingPromptAttempt,
        ProviderAvailability, ProviderStatusInfo, SelectionLocator,
        SelectionTarget, runtime_display_name, strip_selection_envelope,
    },
    ui_brief::BriefWidgetCache,
    ui_motion::{DismissFn, MenuPlacement, ToastState, icon_button_sm, menu_popup, render_toast},
    ui_primitives::{
        CTRL_SM_H, TITLEBAR_EDGE_INSET, macos_traffic_pad, titlebar_center_y, DOC_MIN_W, FS_META, FS_READ, FS_TAG, FS_UI, ICON_SM, LH_META, LH_UI,
        R_CTRL, R_MODAL, SHEET_GUTTER, SIDEBAR_W, StudioPalette, TERMINAL_MIN_H,
        UiLocale, clean_display_doc_name, render_kbd, ui_font,
    },
    ui_settings::SettingsTab,
};

const PALETTE_KEY_CONTEXT: &str = "KarCommandPalette";
actions!(kar_palette, [NextPaletteOption, PreviousPaletteOption]);

pub(crate) fn init_palette_keys(cx: &mut App) {
    let context = Some(PALETTE_KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("tab", NextPaletteOption, context),
        KeyBinding::new("down", NextPaletteOption, context),
        KeyBinding::new("shift-tab", PreviousPaletteOption, context),
        KeyBinding::new("up", PreviousPaletteOption, context),
    ]);
}

/// Share of the work area the conversation keeps when a document sits beside it.
const SPLIT_DEFAULT: f32 = 0.43;
/// The conversation's share when the document is given most of the room.
const SPLIT_WIDE: f32 = 0.34;
const SPLIT_MIN: f32 = 0.28;
const SPLIT_MAX: f32 = 0.68;
/// Narrowest the conversation sheet is allowed to get when comfortable room exists.
const CHAT_MIN_W: f32 = 360.0;
/// Viewport width below which entering Split automatically collapses the navigation rail.
const AUTO_COLLAPSE_RAIL_WIDTH: f32 = 1040.0;
const COMPACT_WORKSPACE_WIDTH: f32 = 700.;
/// Seconds for the rail and the document pane to settle after opening or closing.
const SHELL_SETTLE: f32 = 0.28;
/// A press this recent on a menu's trigger closed the menu; it must not reopen it.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceLayoutMode {
    /// The conversation alone.
    ChatFocus,
    /// The conversation with a document beside it.
    Split,
    /// The document given most of the room, the conversation alongside.
    CanvasWide,
}

/// Active surface in the Unified Right-Pane Workspace Dock.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RightPaneSurface {
    #[default]
    Office,
    GitWorkbench,
    Files,
    Terminal,
    CodeFile(PathBuf),
}

/// Active sub-view inside the Right-Dock `变更` surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GitSubMode {
    #[default]
    Changes,
    Graph,
    PullRequests,
    Issues,
}

/// Primary stage surface inside the floating sheet (`Conversation` | `TasksInbox` | `OfficeGallery`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MainStageView {
    #[default]
    Conversation,
    TasksInbox,
    OfficeGallery,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkspaceLocation {
    Draft(PathBuf),
    Thread(Uuid),
    Live(String),
    Inbox,
    Documents,
    Demo,
}

// The command worker publishes NavigationStarted just before applying this request.
// Mode changes queued by earlier clicks must not replace the current draft.
struct PendingNavigation {
    request: Uuid,
    destination: LocalChatDestination,
    started: bool,
}

struct PendingChatCreation {
    request: Uuid,
    goal_draft: Option<(PathBuf, String)>,
}

/// Scope filter for the KUI `AgentInboxBoard` + `AgentTasks` stage screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InboxScopeFilter {
    #[default]
    All,
    Waiting,
    Running,
    Done,
}

/// The menus of the window. At most one is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    ComposerAdd,
    ComposerSkills,
    ComposerApproval,
    ComposerEffort,
    ComposerWorkspaceMode,
    SettingsEffort,
    ComposerAgent,
    ComposerProject,
    ContextUsage,
    GitBranch,
    DetailsBranch,
    ComposerBranch,
    GitMore,
    SidebarFilter,
    SidebarMore,
    ThreadActions(Uuid),
    DetailsOpenIn,
    DetailsScripts,
    ChatMore,
    DocAdd,
    DocMore,
}

impl Menu {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::ComposerAdd => "menu-composer-add",
            Self::ComposerSkills => "menu-composer-skills",
            Self::ComposerEffort => "menu-composer-effort",
            Self::ComposerApproval => "menu-composer-approval",
            Self::ComposerWorkspaceMode => "menu-composer-workspace-mode",
            Self::SettingsEffort => "menu-settings-effort",
            Self::ComposerAgent => "menu-composer-agent",
            Self::ContextUsage => "menu-context-usage",
            Self::ComposerProject => "menu-composer-project",
            Self::GitBranch => "menu-git-branch",
            Self::DetailsBranch => "menu-details-branch",
            Self::ComposerBranch => "menu-composer-branch",
            Self::GitMore => "menu-git-more",
            Self::SidebarFilter => "menu-sidebar-filter",
            Self::SidebarMore => "menu-sidebar-more",
            Self::ThreadActions(_) => "menu-sidebar-thread-actions",
            Self::DetailsOpenIn => "menu-details-open-in",
            Self::DetailsScripts => "menu-details-scripts",
            Self::ChatMore => "menu-chat-more",
            Self::DocAdd => "menu-doc-add",
            Self::DocMore => "menu-doc-more",
        }
    }
}

fn attachment_tray_labels(locale: UiLocale) -> (&'static str, &'static str) {
    (
        locale.pick("打开文档或添加附件", "Open documents or attach files"),
        locale.pick(
            "Office 文档将在画布中打开。其他文件添加到当前草稿。",
            "Office documents open in the canvas. Other files attach to this draft.",
        ),
    )
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct AgentChoice {
    runtime: String,
    model: Option<String>,
    effort: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum DraftLocation {
    Project(PathBuf),
    Chat(Uuid),
    Remote(String, Option<Uuid>),
    Demo,
}

impl DraftLocation {
    fn for_mode(mode: &ChatAccessMode, project: &Path) -> Self {
        match mode {
            ChatAccessMode::NoneSelected => Self::Project(project.into()),
            ChatAccessMode::DemoShowcase => Self::Demo,
            ChatAccessMode::RemoteController { url, chat_id, .. }
            | ChatAccessMode::HttpReadOnly { url, chat_id } => Self::Remote(url.clone(), *chat_id),
            ChatAccessMode::WriteController { chat_id }
            | ChatAccessMode::LocalReadOnlySubscriber { chat_id, .. }
            | ChatAccessMode::ArchivedReadOnly { chat_id, .. } => Self::Chat(*chat_id),
        }
    }
}

struct ComposerDrafts {
    location: DraftLocation,
    saved: BTreeMap<DraftLocation, QueuedPrompt>,
}

impl QueuedPrompt {
    fn prepend(&mut self, recovered: QueuedPrompt) {
        let mut text = match (&self.target, &recovered.target) {
            (Some(current), Some(prior)) if current != prior => prior.compose_prompt(&recovered.text),
            _ => recovered.text,
        };
        if !text.is_empty() && !self.text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&self.text);
        self.text = text;
        if self.target.is_none() {
            self.target = recovered.target;
        }
        for path in recovered.attachments {
            if !self.attachments.contains(&path) {
                self.attachments.push(path);
            }
        }
    }
}

impl ComposerDrafts {
    fn new(project: PathBuf) -> Self {
        Self { location: DraftLocation::Project(project), saved: BTreeMap::new() }
    }

    fn switch(&mut self, next: DraftLocation, take_current: impl FnOnce() -> QueuedPrompt) -> Option<QueuedPrompt> {
        if self.location == next { return None; }
        let current = take_current();
        let previous = std::mem::replace(&mut self.location, next);
        if !current.text.is_empty() || current.target.is_some() || !current.attachments.is_empty() {
            self.saved.insert(previous, current);
        }
        Some(self.saved.remove(&self.location).unwrap_or_default())
    }
}

/// A message typed on the start screen, held until the chat it belongs to exists.
#[derive(Clone, Debug, Default)]
pub struct QueuedPrompt {
    pub text: String,
    pub target: Option<SelectionTarget>,
    pub attachments: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeTone {
    Info,
    Warning,
    Error,
}

/// What a notice is about. A newer fact about the same thing replaces the notice; facts
/// about other things leave it alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeTopic {
    /// The agent chosen for new chats cannot run.
    Agent,
    /// The live connection to a chat.
    Connection,
    /// Something that just went wrong.
    Event,
}

/// Where the button on a notice leads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeAction {
    None,
    /// Open the agents sheet, where what is missing can be put right.
    ShowAgents,
    ChooseProject,
}

/// Something the person should know and may be able to act on, shown above the conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub topic: NoticeTopic,
    pub tone: NoticeTone,
    pub title: String,
    pub detail: String,
    pub action: NoticeAction,
}

#[derive(Clone, Debug)]
pub struct WorkbenchConfig {
    pub chat_dir: PathBuf,
    pub workspace_dir: PathBuf,
    pub docs_dir: PathBuf,
    pub open_files: Vec<PathBuf>,
    pub subscribe_http: Option<String>,
    pub start_demo_chat: bool,
    pub start_brief_demo: bool,
    pub start_coedit_demo: bool,
    pub initial_coedit_scenario: Option<ScenarioKey>,
    pub dark_mode: bool,
    pub light_mode: bool,
    /// Reopen the most recent chat on launch instead of the blank start screen.
    pub open_latest_chat: bool,
    /// Resume the last selected local chat during an ordinary launch.
    pub resume_last_chat: bool,
    pub open_remote_sheet: bool,
    pub open_schedules_sheet: bool,
    pub open_context_popover: bool,
    pub open_git_tab: bool,
    pub open_skills_sheet: bool,
    pub open_usage_sheet: bool,
    pub open_handoff_sheet: bool,
    pub open_side_question: bool,
    pub open_logs_sheet: bool,
    pub sidebar_repo_files: bool,
    pub open_terminal_tab: bool,
    pub open_bottom_terminal: bool,
    pub hide_details_card: bool,
    pub lang_en: bool,
    pub lang_zh: bool,
    pub initial_terminal_cmd: Option<String>,
    pub open_git_graph: bool,
    pub open_tasks_inbox: bool,
    pub open_office_gallery: bool,
}

impl Default for WorkbenchConfig {
    fn default() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        // Finder launches an app bundle with "/" as its working directory.
        let bundled = cfg!(target_os = "macos")
            && std::env::current_exe().is_ok_and(|exe| exe.ends_with("Contents/MacOS/Kwrok"));
        let bundle_data = bundled
            .then(std::env::home_dir)
            .flatten()
            .map(|home| home.join("Library/Application Support/Kwrok"));
        let state_dir = bundle_data.as_ref().unwrap_or(&cwd);
        let chat_dir = std::env::var_os("KAR_CHAT_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join(".kar-desktop-chats"));
        Self {
            chat_dir,
            workspace_dir: if cwd == std::path::Path::new("/") && bundle_data.is_some() {
                state_dir.join("Workspace")
            } else {
                cwd.clone()
            },
            docs_dir: state_dir.join(".kar-desktop-docs"),
            open_files: Vec::new(),
            subscribe_http: None,
            start_demo_chat: false,
            start_brief_demo: false,
            start_coedit_demo: false,
            initial_coedit_scenario: None,
            dark_mode: false,
            light_mode: false,
            open_latest_chat: false,
            resume_last_chat: true,
            open_remote_sheet: false,
            open_schedules_sheet: false,
            open_context_popover: false,
            open_git_tab: false,
            open_skills_sheet: false,
            open_usage_sheet: false,
            open_handoff_sheet: false,
            open_side_question: false,
            open_logs_sheet: false,
            sidebar_repo_files: false,
            open_terminal_tab: false,
            open_bottom_terminal: false,
            hide_details_card: false,
            lang_en: false,
            lang_zh: false,
            initial_terminal_cmd: None,
            open_git_graph: false,
            open_tasks_inbox: false,
            open_office_gallery: false,
        }
    }
}

pub struct KarWorkbench {
    pub config: WorkbenchConfig,
    pub bridge: KarBridgeHandle,
    pub theme_mode: ThemeMode,
    follow_system_appearance: bool,
    pub ui_locale: UiLocale,
    pub focus_handle: FocusHandle,

    // ── Shell geometry & T3 Code Panel States ───────────────────────────────
    previous_window_width: f32,
    restore_sidebar_after_wide: bool,
    restore_sidebar_after_resize: bool,
    drawer_return_focus: Option<(MainStageView, FocusHandle)>,
    pub sidebar_open: bool,
    navigation: Vec<WorkspaceLocation>,
    navigation_index: usize,
    pub sidebar_filter: String,
    pub sidebar_project_filter: Option<PathBuf>,
    pub settled_chats: BTreeSet<Uuid>,
    pub(crate) hosted_writer: Option<Uuid>,
    pub(crate) sidebar_metadata: BTreeMap<Uuid, crate::kar_bridge::SidebarMetadata>,
    pub settled_shelf_open: bool,
    pub thread_details_open: bool,
    pub bottom_terminal_open: bool,
    pub bottom_terminal_h: f32,
    pub(crate) bottom_terminal_anim: Slide,
    pub(crate) dragging_terminal: bool,
    pub(crate) terminal_drag_anchor: Option<(f32, f32)>,
    pub expanded_turn_changes: BTreeMap<Uuid, bool>,
    pub layout_mode: WorkspaceLayoutMode,
    /// The conversation's share of the work area in `Split`, as last set by the person.
    pub split_ratio: f32,
    pane_drag: Option<PaneDrag>,
    pub(crate) sidebar_focus: FocusHandle,
    sidebar_divider_focus: FocusHandle,
    doc_divider_focus: FocusHandle,
    /// Rail width in px, sprung so collapsing and expanding glide.
    pub(crate) sidebar_anim: Slide,
    sidebar_width: f32,
    /// Conversation share of an open split, including animated divider adjustments.
    pub(crate) split_anim: Slide,
    /// Fraction of the open document pane currently revealed.
    dock_reveal: Slide,
    /// How much of the rail is showing this frame, 0 to 1.
    pub(crate) sidebar_reveal: f32,

    // ── Overlays ────────────────────────────────────────────────────────────
    pub open_menu: Option<Menu>,
    /// The menu a press outside just closed, and when.
    pub command_palette_open: bool,
    palette_return_focus: Option<FocusHandle>,
    pub(crate) cmd_query_editor: Entity<Editor>,
    pub(crate) cmd_query: String,
    pub(crate) cmd_selected_ix: usize,
    cmd_scroll: ScrollHandle,
    reveal_cmd_selection: bool,
    pub provider_sheet_open: bool,
    pub agents_show_uninstalled: Option<bool>,
    pub(crate) toast: Option<ToastState>,
    pub(crate) notice: Option<Notice>,
    pub(crate) notice_open: bool,

    // ── KUI components that keep their own state ────────────────────────────
    pub(crate) sidebar_search: Entity<Editor>,
    pub(crate) inbox_search: Entity<Editor>,
    pub(crate) documents_search: Entity<Editor>,
    pub(crate) coedit_segmented: Entity<GlassSegmented>,
    pub(crate) coedit_dismiss_morph: Entity<ConfirmMorph>,
    pub(crate) composer_editor: Entity<Editor>,
    pub(crate) question_draft: Option<(
        kar::Cursor,
        Entity<crate::ui_chat::questions::QuestionDraft>,
    )>,
    pub(crate) http_dialog: Entity<Dialog>,
    pub(crate) http_url_input: Entity<Input>,
    pub(crate) goal_dialog: Entity<Dialog>,
    goal_dialog_owner: Option<DraftLocation>,
    goal_pending: BTreeSet<Uuid>,
    goal_form_baseline: String,
    pub(crate) goal_input: Entity<Input>,
    goal_budget_input: Entity<Input>,
    pub(crate) rename_dialog: Entity<Dialog>,
    pub(crate) rename_input: Entity<Input>,
    pub(crate) branch_dialog: Entity<Dialog>,
    pub(crate) branch_input: Entity<Input>,
    pub(crate) branch_workspace: Option<PathBuf>,
    pub(crate) rename_target_chat: Option<Uuid>,
    pub(crate) quick_edit_dialog: Entity<Dialog>,
    pub(crate) quick_edit_primary: Entity<Input>,
    pub(crate) quick_edit_secondary: Entity<Input>,
    pub(crate) coedit_manual_editor: Entity<Editor>,

    // ── Agents and chats ────────────────────────────────────────────────────
    /// Agent that new chats start with. An open chat keeps the agent it was created with.
    pub selected_runtime: String,
    /// Specific model id chosen for new chats (`None` uses the agent's default model).
    pub selected_model: Option<String>,
    /// The person chose `selected_runtime`; do not swap it for another ready agent.

    /// True while `kar::orientation::orient` is probing CLI agents in the background.
    pub(crate) providers_probing: bool,
    pub(crate) provider_accounts_checked: bool,
    /// What new chats do when the agent needs permission.
    pub approval_mode: ApprovalMode,
    /// Reasoning effort requested for new chats when the agent supports it.
    pub reasoning_effort: Option<String>,
    pub providers: Vec<ProviderStatusInfo>,
    pub registry_catalog: Vec<crate::registry::CatalogEntry>,
    pub registry_installed: Vec<crate::registry::Installed>,
    pub chat_list: Vec<ChatListEntry>,
    read_receipts: read_receipts::ThreadReadReceipts,
    opened_unread_reply: Option<(Uuid, u64)>,
    pub(crate) pending_delete_chat: Option<(Uuid, Instant)>,
    pending_navigation: Option<PendingNavigation>,
    creating_chat: Option<PendingChatCreation>,
    pub access_mode: ChatAccessMode,
    pub connection_state: FeedConnectionState,
    pub projected: ProjectedChatState,
    session_commands: Option<(Uuid, Vec<crate::studio_hub::SlashCommandItem>)>,
    pub turn_targets: BTreeMap<Uuid, SelectionTarget>,
    pub prompt_attempts: BTreeMap<Uuid, PendingPromptAttempt>,
    pub active_http_server_url: Option<String>,
    /// Typed on the start screen and waiting for its chat to be created.
    drafts: ComposerDrafts,
    pub(crate) queued_prompt: Option<QueuedPrompt>,
    slash: commands::SlashPicker,
    planning: planning::Planning,
    aster_planning: aster_plan::AsterPlanning,
    /// Local files/images queued as attachments on the next prompt.
    pub(crate) pending_attachments: Vec<PathBuf>,

    // ── The conversation on screen ──────────────────────────────────────────
    pub(crate) copy_buttons: BTreeMap<Uuid, Entity<CopyButton>>,
    /// Folded parts of the conversation the reader opened or closed by hand, by turn and
    /// item index.
    pub(crate) part_open: BTreeMap<(Uuid, usize), bool>,
    /// Loaded git checkpoint diffs keyed by `(chat_id, turn_index)`.
    pub(crate) turn_review: Option<crate::turn_review::FileReview>,
    /// The one tool call whose input and output are showing.
    pub(crate) tool_detail: Option<(Uuid, usize)>,
    pub chat_scroll: ScrollHandle,
    pub(crate) chat_follow_paused: bool,
    pub(crate) turn_navigation: crate::ui_chat::turn_navigation::TurnNavigation,

    // ── Documents ───────────────────────────────────────────────────────────
    pub office_tabs: Vec<OfficeTab>,
    pub active_office_tab: Option<usize>,
    next_tab_id: u64,
    /// The document selection the next message is about.
    pub pinned_target: Option<SelectionTarget>,

    // ── Co-editing sample & live Revision Fence ledger ──────────────────────
    pub coedit_engine: Box<dyn CoEditService>,
    pub coedit_panel_open: bool,
    pub active_coedit_scenario: ScenarioKey,
    pub coedit_demo_dir: Option<PathBuf>,
    pub coedit_feedback: Option<String>,
    pub fence_ledger: RevisionFenceLedger,

    // ── ToWork-inspired Client Store, Interactive Briefs, Remote Link & Schedules ─
    pub client_reducer: ClientReducer,
    pub active_briefs: Vec<KarBrief>,
    pub(crate) brief_widgets: BriefWidgetCache,
    pub remote_sheet_open: bool,
    pub(crate) remote_tab_connect: bool,
    pub remote_telemetry: RemoteHostTelemetry,
    pub(crate) remote_mode_segmented: Entity<GlassSegmented>,
    pub(crate) remote_connect_url_input: Entity<Input>,
    pub(crate) kremote_invite_input: Entity<Input>,
    pub(crate) remote_connect_code_input: Entity<Input>,
    pub(crate) remote_device_name_input: Entity<Input>,
    pub(crate) remote_requested_role: PeerRole,
    pub(crate) remote_pair_pending: bool,
    pub(crate) remote_code_copied_at: Option<(String, bool, Instant)>,
    pub schedules_sheet_open: bool,
    pub(crate) schedule_creator_open: bool,
    pub schedule_registry: ScheduleRegistry,
    schedule_refresh_ms: u64,
    pub(crate) schedule_workspace: Option<PathBuf>,
    pub(crate) schedule_loaded: bool,
    pub(crate) schedule_error: Option<String>,
    pub(crate) editing_schedule_revision: Option<u64>,
    pub(crate) schedule_pending_runs: BTreeMap<Uuid, (Uuid, bool)>,
    pub(crate) schedule_save_pending: bool,
    pub(crate) schedule_title_input: Entity<Input>,
    pub(crate) schedule_prompt_input: Entity<Input>,
    pub(crate) schedule_draft_cadence: ScheduleCadence,
    pub(crate) schedule_bind_active_doc: bool,
    pub(crate) expanded_schedule_history: BTreeSet<String>,

    // ── Unified Settings, Projects, Worktrees, Right Dock & Terminal ────────
    overlay_return_focus: Option<FocusHandle>,
    pub settings_open: bool,
    pub active_settings_tab: SettingsTab,
    pub project_registry: ProjectRegistry,
    pub new_chat_use_worktree: bool,
    pub right_surface: RightPaneSurface,
    pub git_tab_open: bool,
    pub(crate) files_tab_open: bool,
    pub(crate) terminal_tab_open: bool,
    pub(crate) committing_workspaces: BTreeSet<PathBuf>,
    pub git_sub_mode: GitSubMode,
    pub(crate) git_staged_limit: usize,
    pub(crate) git_unstaged_limit: usize,
    pub expanded_git_diff_file: Option<(PathBuf, bool)>,
    pub inline_git_diffs: BTreeMap<(PathBuf, bool), Vec<DiffHunkData>>,
    panel_workspace: PathBuf,
    git_drafts: BTreeMap<PathBuf, String>,
    pub(crate) generating_commits: BTreeMap<PathBuf, String>,
    pub git_snapshot: GitWorkspaceSnapshot,
    pub(crate) git_branch_graph: Option<Entity<BranchGraph>>,
    pub(crate) git_commit_input: Entity<Input>,
    pub open_code_tabs: BTreeMap<PathBuf, WorkspaceFilePreview>,
    pub code_tab_order: Vec<PathBuf>,
    pub(crate) code_block_cache: BTreeMap<String, Entity<CodeBlock>>,
    pub(crate) patch_review_cache: RefCell<BTreeMap<String, Entity<PatchReview>>>,
    pub(crate) tool_json_viewers: RefCell<BTreeMap<String, Entity<JsonViewer>>>,
    pub workspace_tree: Vec<WorkspaceTreeItem>,
    pub(crate) workspace_tree_view: Option<Entity<TreeView>>,
    pub tree_show_hidden: bool,
    pub terminal_runs: Vec<TerminalRunRecord>,
    pub(crate) terminal_cmd_input: Entity<Input>,
    pub skills_sheet_open: bool,
    pub workspace_inventory: WorkspaceInventorySnapshot,
    pub active_inventory_tab: InventoryTab,
    pub(crate) skills_search_editor: Entity<Editor>,
    pub skills_filter_query: String,
    pub expanded_skill_name: Option<String>,
    pub expanded_tool_schema: Option<String>,
    pub usage_sheet_open: bool,
    pub usage_analytics: UsageAnalyticsSnapshot,
    pub handoff_sheet_open: bool,
    pub handoff_receipt: Option<(Uuid, Box<kar::chat::Handoff>)>,
    pub handoff_target_runtime: String,
    pub handoff_budget_kb: usize,
    pub(crate) handoff_note_input: Entity<Input>,
    pub side_question_open: bool,
    pub(crate) side_question_input: Entity<Input>,
    pub(crate) side_question_request: Option<Uuid>,
    pub side_question_pending: bool,
    pub side_question_last_reply: Option<(String, String)>,
    pub(crate) side_question_capability: Option<(Uuid,String)>,
    pub logs_sheet_open: bool,
    pub diag_logs: DiagLogRing,
    pub(crate) log_stream_revision: Option<u64>,
    pub(crate) log_stream_widget: Option<Entity<LogStream>>,
    pub global_search_hits: Vec<GlobalSearchHit>,
    pub start_dropzone_open: bool,
    pub(crate) file_dropzone: Entity<FileDropzone>,
    pub(crate) presence_stack: Option<Entity<PresenceStack>>,
    pub(crate) rollback_hold_buttons: RefCell<BTreeMap<(Uuid, Uuid, bool), Entity<HoldToConfirm>>>,
    /// Deferred reversible chat deletion `(chat_id, chat_title, started_at)`.
    pub(crate) deferred_delete_chat: Option<(Uuid, String, Instant)>,
    /// Live model-suggested follow-up prompt from `kar::PromptSuggestions`.
    pub live_prompt_suggestion: Option<kar::PromptSuggestion>,
    /// Primary stage surface (`Conversation` | `TasksInbox` | `OfficeGallery`).
    pub main_stage_view: MainStageView,
    pub inbox_scope: InboxScopeFilter,
    pub usage_by_project: bool,
    pub usage_period_days: u32,
    pub selected_skill_inspector: Option<String>,

    // ── Complete Preferences, PTY, Voice, Forge, Devices & Search ───────────
    pub prefs: crate::prefs::DesktopPreferences,
    pub active_pty: Option<std::sync::Arc<crate::pty_terminal::InteractivePtySession>>,
    pub pty_focus: FocusHandle,
    pub detected_dev_ports: Vec<u16>,
    pub active_voice_rec: Option<crate::hardware::Recording>,
    voice_capture_id: Option<Uuid>,
    voice_drafts: BTreeMap<Uuid, hardware::DictationDraft>,
    hardware_owners: BTreeMap<Uuid,DraftLocation>,
    migrating_voice_keys: BTreeMap<Uuid, String>,
    held_voice_key: Option<String>,
    pub(crate) microphone_name: Option<String>,
    pub voice_env: crate::hardware::VoiceState,
    pub installed_editors: Vec<crate::forge_devices_voice::InstalledEditor>,
    pub forge_prs: Vec<crate::forge_devices_voice::ForgePullRequest>,
    pub forge_issues: Vec<crate::forge_devices_voice::ForgeIssue>,
    pub forge_error: Option<String>,
    pub forge_create_pr_open: bool,
    pub(crate) forge_workspace: Option<PathBuf>,
    pub(crate) forge_pending: Option<Uuid>,
    forge_last_operation: Option<(String,Uuid)>,
    pub(crate) forge_draft_epoch: usize,
    pub devices: crate::hardware::DeviceState,
    pub external_transcripts: Vec<crate::forge_devices_voice::ExternalTranscriptCandidate>,
    pub storage_breakdown: Option<crate::forge_devices_voice::StorageUsageSummary>,
    pub managed_worktrees: Vec<crate::forge_devices_voice::ManagedWorktreeItem>,
    pub desktop_update_report: Option<crate::forge_devices_voice::DesktopAppUpdateReport>,
    pub agent_update_statuses: BTreeMap<String, (String, bool)>,
    pub(crate) provider_sign_in: provider_sign_in::ProviderSignIn,
    pub editing_schedule_id: Option<String>,
    pub chat_search_open: bool,
    pub chat_search_query: String,
    pub chat_search_match_ix: usize,
    pub quit_pending_since: Option<Instant>,
    pub pending_confirm_action: Option<(String, Instant)>,
    pub(crate) caffeinate_guard: crate::forge_devices_voice::CaffeinateGuard,
}

impl KarWorkbench {
    pub fn new(mut config: WorkbenchConfig, window: &mut Window, cx: &mut Context<Self>) -> Self {
        config.resume_last_chat &= config.subscribe_http.is_none()
            && !config.start_demo_chat && !config.start_brief_demo && !config.start_coedit_demo
            && !config.open_tasks_inbox && !config.open_office_gallery
            && config.open_files.is_empty() && config.initial_terminal_cmd.is_none();

        let loaded_prefs = crate::prefs::DesktopPreferences::load(&config.chat_dir);
        let override_mode = match (config.dark_mode, config.light_mode) {
            (true, _) => Some(ThemeMode::Dark),
            (_, true) => Some(ThemeMode::Light),
            _ => match crate::client_store::desktop_preference(&config.chat_dir, "appearance")
                .as_deref()
            {
                Some("dark") => Some(ThemeMode::Dark),
                Some("light") => Some(ThemeMode::Light),
                _ => None,
            },
        };
        let theme_mode =
            override_mode.unwrap_or_else(|| crate::system_theme(cx.window_appearance()));
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() && this.held_voice_key.take().is_some() && this.active_voice_rec.is_some() {
                this.toggle_voice_dictation(window,cx);
            }
            cx.notify();
        }).detach();
        cx.observe_window_appearance(window, |this, window, cx| {
            if this.follow_system_appearance {
                this.theme_mode = crate::system_theme(window.appearance());
                crate::apply_theme_and_typography(this.theme_mode, &this.prefs, cx);
                cx.notify();
            }
        })
        .detach();
        crate::apply_theme_and_typography(theme_mode, &loaded_prefs, cx);

        let bridge =
            KarBridgeHandle::spawn(config.chat_dir.clone(), config.workspace_dir.clone());

        let initial_layout = if !config.open_files.is_empty()
            || config.start_coedit_demo
            || config.open_git_tab
            || config.sidebar_repo_files
            || config.open_terminal_tab
        {
            WorkspaceLayoutMode::Split
        } else {
            WorkspaceLayoutMode::ChatFocus
        };
        let initial_width = f32::from(window.viewport_size().width);
        let (sidebar_width, split_ratio) = resize::initial_sizes(&config);
        let default_sidebar_open = initial_width >= COMPACT_WORKSPACE_WIDTH
            && (initial_layout == WorkspaceLayoutMode::ChatFocus
                || initial_width >= AUTO_COLLAPSE_RAIL_WIDTH);
        let initial_sidebar_open = loaded_prefs.sidebar_open.unwrap_or(default_sidebar_open);
        let initial_scenario = config
            .initial_coedit_scenario
            .unwrap_or(ScenarioKey::WordSameParagraph);

        let ui_locale = if config.lang_en
            || (!config.lang_zh
                && crate::client_store::desktop_preference(&config.chat_dir, "language").as_deref()
                    == Some("en"))
        {
            UiLocale::En
        } else {
            UiLocale::Zh
        };

        crate::apply_ui_language(ui_locale, cx);
        let sidebar_search = cx.new(|cx| {
            let mut ed = Editor::new(
                ui_locale.pick("搜索对话", "Search threads"),
                InputMode::Search,
                window,
                cx,
            );
            ed.set_placeholder(ui_locale.pick("搜索", "Search"));
            ed.font_size = FS_UI;
            ed
        });
        cx.subscribe_in(
            &sidebar_search,
            window,
            |this, editor, event: &EditEvent, window, cx| match event {
                EditEvent::Changed => {
                    this.sidebar_filter = editor.read(cx).value().to_owned();
                    cx.notify();
                }
                EditEvent::Escape if editor.read(cx).value().is_empty() => {
                    this.focus_handle.focus(window, cx);
                }
                EditEvent::Escape => {
                    editor.update(cx, |ed, cx| ed.set_value("", window, cx));
                }
                _ => {}
            },
        )
        .detach();

        let mut collection_search = |label: &'static str| {
            let editor = cx.new(|cx| {
                let mut ed = Editor::new(label, InputMode::Search, window, cx);
                ed.set_placeholder(label);
                ed.font_size = FS_UI;
                ed
            });
            cx.subscribe(&editor, |_, _, event: &EditEvent, cx| {
                if matches!(event, EditEvent::Changed) {
                    cx.notify();
                }
            })
            .detach();
            editor
        };
        let inbox_search = collection_search(ui_locale.pick("搜索收件箱", "Search inbox"));
        let documents_search = collection_search(ui_locale.pick("搜索文档", "Search documents"));
        let cmd_query_editor = cx.new(|cx| {
            let mut ed = Editor::new("命令面板搜索", InputMode::Search, window, cx);
            ed.set_placeholder(ui_locale.pick("搜索命令、文件和对话…", "Search commands, files, and threads…"));
            ed.font_size = FS_UI;
            ed
        });
        cx.subscribe_in(
            &cmd_query_editor,
            window,
            |this, ed, ev: &EditEvent, window, cx| match ev {
                EditEvent::Changed => {
                    this.cmd_query = ed.read(cx).value().to_owned();
                    this.cmd_selected_ix = 0;
                    this.cmd_scroll.set_offset(point(px(0.), px(0.)));
                    this.global_search_hits.clear();
                    let q = this.cmd_query.trim().to_owned();
                    if q.chars().count() >= 2 {
                        this.bridge.send(KarCommand::GlobalSearch {
                            workspace: this.effective_workspace_dir(),
                            query: q,
                        });
                    }
                    cx.notify();
                }
                EditEvent::Submitted => {
                    this.confirm_selected_palette_item(window, cx);
                }
                EditEvent::Escape => {
                    this.close_command_palette(window, cx);
                }
                _ => {}
            },
        )
        .detach();

        let coedit_segmented = cx.new(|cx| {
            GlassSegmented::new(
                "示例场景",
                vec![
                    GlassSegmentOption::new(ScenarioKey::WordSameParagraph.as_str(), "Word"),
                    GlassSegmentOption::new(ScenarioKey::ExcelSameCellConflict.as_str(), "Excel"),
                    GlassSegmentOption::new(ScenarioKey::PptMoveAndText.as_str(), "PPT"),
                ],
                cx,
            )
            .with_compact(true)
            .with_width(180.)
            .with_value(initial_scenario.as_str())
        });
        cx.subscribe_in(
            &coedit_segmented,
            window,
            |this, _, ev: &GlassSegmentChanged, window, cx| {
                let sc = scenario_for_key(ev.0.as_ref());
                this.switch_coedit_scenario(sc, window, cx);
            },
        )
        .detach();

        let coedit_dismiss_morph = cx.new(|cx| {
            ConfirmMorph::new("coedit-dismiss-morph", "放弃这次修改", "确定放弃？", window, cx)
                .with_compact(true)
                .with_labels("放弃", "已放弃")
        });
        cx.subscribe_in(
            &coedit_dismiss_morph,
            window,
            |this, morph, ev: &ConfirmRequest, window, cx| {
                let ticket = ev.ticket;
                morph.update(cx, |m, cx| m.complete(ticket, true, cx));
                if ev.intent == ConfirmIntent::Delete {
                    let sc = this.active_coedit_scenario;
                    if let Some(op_id) = this
                        .coedit_engine
                        .proposals()
                        .iter()
                        .find(|p| p.scenario == sc)
                        .map(|p| p.operation_id)
                    {
                        this.submit_coedit_choice(
                            op_id,
                            UserResolutionChoice::DismissDeletedTarget,
                            window,
                            cx,
                        );
                    }
                }
            },
        )
        .detach();

        let composer_editor = cx.new(|cx| {
            let mut ed = Editor::new("消息", InputMode::InlineMulti, window, cx);
            ed.set_placeholder(ui_locale.pick(
                PLACEHOLDER_NEW_CHAT_ZH,
                PLACEHOLDER_NEW_CHAT_EN,
            ));
            ed.font_size = FS_READ;
            ed.set_command_submit(true);
            ed
        });
        cx.subscribe_in(
            &composer_editor,
            window,
            |this, _, ev: &EditEvent, window, cx| match ev {
                EditEvent::Changed => {
                    this.slash.edited();
                    if this.composer_editor.read(cx).value() == "/"
                        && this.workspace_inventory.commands_for(
                            &this.effective_workspace_dir(), this.active_or_selected_runtime_id(),
                        ).is_none()
                    {
                        this.bridge.send(KarCommand::ProbeWorkspaceInventory {
                            workspace: this.effective_workspace_dir(),
                            runtime_id: this.active_or_selected_runtime_id().to_owned(),
                        });
                    }
                    cx.notify();
                }
                EditEvent::Blurred => { this.slash.close(); cx.notify(); }
                EditEvent::Focused => cx.notify(),
                EditEvent::Submitted => this.submit_composer_prompt(window, cx),
                EditEvent::Escape => this.close_menu(cx),
                _ => {}
            },
        )
        .detach();
        cx.subscribe_in(
            &composer_editor,
            window,
            |this, _, _: &EditorShortcutEvent, window, cx| {
                this.submit_composer_prompt(window, cx);
            },
        )
        .detach();

        let coedit_manual_editor = cx.new(|cx| {
            let mut ed = Editor::new("合并后的值", InputMode::Single, window, cx);
            ed.set_placeholder("合并后的值");
            ed.font_size = FS_UI;
            ed.set_value("142000", window, cx);
            ed
        });
        cx.subscribe_in(
            &coedit_manual_editor,
            window,
            |this, _, ev: &EditEvent, window, cx| {
                if *ev == EditEvent::Submitted {
                    let sc = this.active_coedit_scenario;
                    if let Some(prop) = this
                        .coedit_engine
                        .proposals()
                        .iter()
                        .find(|p| p.scenario == sc)
                    {
                        let op_id = prop.operation_id;
                        let val = this.coedit_manual_editor.read(cx).value().trim().to_owned();
                        this.submit_coedit_choice(
                            op_id,
                            UserResolutionChoice::ManualMerge { merged_value: val },
                            window,
                            cx,
                        );
                    }
                }
            },
        )
        .detach();

        let http_url_input = cx.new(|cx| {
            let initial = config
                .subscribe_http
                .clone()
                .unwrap_or_else(|| "http://127.0.0.1:18090".into());
            Input::new("kar-http-url-input", "链接", window, cx)
                .with_value(initial, window, cx)
                .with_placeholder("http://127.0.0.1:18090", cx)
        });
        let url_field_for_dialog = http_url_input.clone();
        let http_dialog = cx.new(|cx| {
            Dialog::new("kar-http-dialog", "打开共享链接", "打开共享链接", cx)
                .without_trigger()
                .with_description("粘贴别人共享的对话链接。你会实时看到对话内容，但不能发送消息。")
                .with_body(move |_, _| div().w_full().py_1().child(url_field_for_dialog.clone()))
                .with_cancel("取消")
                .with_confirm("打开")
        });
        cx.subscribe_in(
            &http_dialog,
            window,
            |this, _, ev: &SurfaceEvent, _, cx| {
                if *ev == SurfaceEvent::Confirmed {
                    let url = this.http_url_input.read(cx).value(cx).trim().to_owned();
                    if !url.is_empty() {
                        this.bridge.send(KarCommand::ConnectHttpFeed { url });
                    }
                }
            },
        )
        .detach();

        let goal_input = cx.new(|cx| {
            Input::new("kar-goal-input", ui_locale.pick("对话目标", "Chat goal"), window, cx)
                .with_placeholder(ui_locale.pick("输入可验证的目标", "Enter a verifiable objective"), cx)
        });
        let goal_budget_input = cx.new(|cx| {
            Input::new("kar-goal-budget", ui_locale.pick("Token 预算", "Token budget"), window, cx)
                .with_placeholder(ui_locale.pick("可选，输入 token 数", "Optional token limit"), cx)
        });
        let goal_field_for_dialog = goal_input.clone();
        let goal_owner = cx.weak_entity();
        let goal_dialog = cx.new(|cx| {
            Dialog::new("kar-goal-dialog", ui_locale.pick("对话目标", "Chat goal"), ui_locale.pick("对话目标", "Chat goal"), cx)
                .without_trigger()
                .with_body(move |_, cx| Self::goal_dialog_body(&goal_owner, goal_field_for_dialog.clone(), cx))
                .with_cancel(ui_locale.pick("取消", "Cancel"))
                .with_confirm(ui_locale.pick("保存目标", "Save goal"))
        });
        cx.subscribe_in(
            &goal_dialog,
            window,
            |this, _, ev: &SurfaceEvent, win, cx| {
                if *ev == SurfaceEvent::Confirmed {
                    this.confirm_chat_goal(win, cx);
                }
            },
        )
        .detach();

        let rename_input = cx.new(|cx| {
            Input::new("kar-rename-chat-input", "对话标题", window, cx)
                .with_placeholder("输入新的对话标题", cx)
        });
        let rename_field_for_dialog = rename_input.clone();
        let rename_dialog = cx.new(|cx| {
            Dialog::new("kar-rename-chat-dialog", "重命名对话", "重命名对话", cx)
                .without_trigger()
                .with_description("标题保存在本机对话记录中，侧边栏和顶部标题栏同步更新。")
                .with_body(move |_, _| div().w_full().py_1().child(rename_field_for_dialog.clone()))
                .with_cancel("取消")
                .with_confirm("保存")
        });
        cx.subscribe_in(
            &rename_dialog,
            window,
            |this, _, ev: &SurfaceEvent, _, cx| {
                if *ev == SurfaceEvent::Confirmed
                    && let Some(chat_id) = this.rename_target_chat.take()
                {
                    let title = this.rename_input.read(cx).value(cx).trim().to_owned();
                    if !title.is_empty() {
                        this.bridge.send(KarCommand::RenameChat { chat_id, title });
                    }
                }
            },
        )
        .detach();

        for (field, dialog) in [(&rename_input, rename_dialog.clone()), (&goal_input, goal_dialog.clone()), (&goal_budget_input, goal_dialog.clone())] {
            let editor = field.read(cx).editor();
            cx.subscribe_in(&editor, window, move |this, _, event: &EditEvent, window, cx| {
                if !dialog.read(cx).is_open() { return; }
                match event {
                    EditEvent::Submitted => dialog.update(cx, |d, cx| {
                        d.set_open(false, window, cx);
                        cx.emit(SurfaceEvent::Confirmed);
                    }),
                    EditEvent::Escape => {
                        dialog.update(cx, |d, cx| d.set_open(false, window, cx));
                        this.composer_editor.update(cx, |editor, cx| editor.focus(window, cx));
                    }
                    _ => {}
                }
            }).detach();
        }

        let branch_input = cx.new(|cx| {
            Input::new("branch-name", "分支名称", window, cx)
                .with_placeholder("例如 feat/search", cx)
        });
        let branch_field = branch_input.clone();
        let branch_dialog = cx.new(|cx| {
            Dialog::new("kar-create-branch", "新建分支", "新建分支", cx)
                .without_trigger()
                .with_body(move |_, _| div().w_full().child(branch_field.clone()))
                .with_cancel("取消")
                .with_confirm("创建并切换")
        });
        cx.subscribe_in(&branch_dialog, window, |this, _, ev: &SurfaceEvent, window, cx| {
            if *ev == SurfaceEvent::Confirmed && let Some(ws) = this.branch_workspace.clone() {
                let name = this.branch_input.read(cx).value(cx).trim().to_owned();
                if name.is_empty() {
                    this.branch_input.update(cx, |field, cx| field.set_error("请输入分支名称", cx));
                    this.branch_dialog.update(cx, |dialog, cx| dialog.set_open(true, window, cx));
                } else {
                    this.bridge.send(KarCommand::GitSwitchBranch { workspace: ws, branch: name, create: true });
                    this.branch_workspace = None;
                }
            }
        }).detach();

        let quick_edit_primary = cx.new(|cx| {
            Input::new("kar-quick-edit-primary", "新内容", window, cx)
                .with_placeholder("文字、数值，或 =SUM(…) 这样的公式", cx)
        });
        let quick_edit_secondary = cx.new(|cx| {
            Input::new("kar-quick-edit-secondary", "位置", window, cx)
                .with_placeholder("段落序号、单元格（如 B4）或坐标（如 5.2,3.8）", cx)
        });
        let qe1 = quick_edit_primary.clone();
        let qe2 = quick_edit_secondary.clone();
        let quick_edit_dialog = cx.new(|cx| {
            Dialog::new("kar-quick-edit-dialog", "按位置修改", "按位置修改", cx)
                .without_trigger()
                .with_description(
                    "直接指定要改的地方：Word 填段落序号，Excel 填单元格，PPT 填对象的 x,y 坐标（厘米）。",
                )
                .with_body(move |_, _| {
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .py_1()
                        .child(qe1.clone())
                        .child(qe2.clone())
                })
                .with_cancel("取消")
                .with_confirm("修改")
        });
        cx.subscribe_in(
            &quick_edit_dialog,
            window,
            |this, _, ev: &SurfaceEvent, window, cx| {
                if *ev == SurfaceEvent::Confirmed {
                    this.apply_quick_edit_from_dialog(window, cx);
                }
            },
        )
        .detach();

        let kremote_invite_input = Self::kremote_invite_field(ui_locale, window, cx);
        let remote_mode_segmented = Self::remote_mode_selector(ui_locale, false, cx);
        let [
            remote_connect_url_input,
            remote_connect_code_input,
            remote_device_name_input,
        ] = Self::remote_fields(
            ui_locale,
            [
                config.subscribe_http.clone().unwrap_or_default(),
                String::new(),
                ui_locale.pick("本机", "This computer").to_owned(),
            ],
            window,
            cx,
        );

        let schedule_title_input = cx.new(|cx| {
            Input::new(
                "kar-sched-title",
                ui_locale.pick("任务标题", "Task name"),
                window,
                cx,
            )
            .with_placeholder(
                ui_locale.pick(
                    "例如：每日早九点核对预算表并生成 KUI 简报",
                    "Name this task",
                ),
                cx,
            )
        });
        let schedule_prompt_input = cx.new(|cx| {
            Input::new(
                "kar-sched-prompt",
                ui_locale.pick("智能体执行指令", "Instructions"),
                window,
                cx,
            )
            .with_placeholder(
                ui_locale.pick(
                    "描述定时触发时智能体要检查、汇总或生成的具体内容…",
                    "What should the agent do?",
                ),
                cx,
            )
        });

        let git_commit_input = cx.new(|cx| {
            Input::new("kar-git-commit-input", "提交说明", window, cx)
                .with_compact_bar(32.)
                .with_placeholder("描述本次更改…", cx)
        });

        cx.subscribe(&git_commit_input.read(cx).editor(), |_, _, ev: &EditEvent, cx| {
            if *ev == EditEvent::Changed { cx.notify(); }
        }).detach();

        let skills_search_editor = cx.new(|cx| {
            let mut ed = Editor::new("检索技能与工具", InputMode::Search, window, cx);
            ed.set_placeholder(ui_locale.pick("筛选技能、MCP、指令…", "Search skills and tools…"));
            ed.font_size = FS_META;
            ed
        });
        cx.subscribe(&skills_search_editor, |this, ed, ev: &EditEvent, cx| {
            if matches!(ev, EditEvent::Changed | EditEvent::Escape) {
                this.skills_filter_query = ed.read(cx).value().to_owned();
                cx.notify();
            }
        })
        .detach();

        let handoff_note_input = cx.new(|cx| {
            Input::new(
                "kar-handoff-note-input",
                ui_locale.pick("交办说明", "Handoff note (optional)"),
                window,
                cx,
            )
            .with_placeholder(
                ui_locale.pick(
                    "可选：告诉新智能体接手后优先做什么…",
                    "What should the next agent do first?",
                ),
                cx,
            )
        });

        let side_question_input = cx.new(|cx| {
            Input::new(
                "kar-side-question-input",
                ui_locale.pick("旁路问题", "Your question"),
                window,
                cx,
            )
            .with_placeholder(
                ui_locale.pick(
                    "例如：刚才我们约定的预算字段名是什么？",
                    "Ask about this conversation…",
                ),
                cx,
            )
        });

        let (label, description) = attachment_tray_labels(ui_locale);
        let file_dropzone = cx.new(|cx| {
            FileDropzone::new(label, 32 * 1024 * 1024, 8, cx).with_description(description)
        });
        let mut dropped_paths = BTreeMap::new();
        cx.subscribe_in(
            &file_dropzone,
            window,
            move |this, dz, ev: &kui::components::file_dropzone::FileEvent, window, cx| {
                if let kui::components::file_dropzone::FileEvent::Removed(ticket) = ev
                    && let Some(path) = dropped_paths.remove(ticket)
                {
                    this.pending_attachments
                        .retain(|attachment| attachment != &path);
                    for draft in this.drafts.saved.values_mut() {
                        draft.attachments.retain(|attachment| attachment != &path);
                    }
                    cx.notify();
                    return;
                }
                if let kui::components::file_dropzone::FileEvent::Upload { ticket, file } = ev {
                    let ticket = *ticket;
                    let safe_name = Path::new(&file.name)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "attachment.bin".into());
                    let out_dir = this
                        .config
                        .docs_dir
                        .join("dropped")
                        .join(Uuid::new_v4().to_string());
                    let dest = out_dir.join(&safe_name);
                    if std::fs::create_dir_all(&out_dir)
                        .and_then(|_| std::fs::write(&dest, file.bytes.as_ref()))
                        .is_ok()
                    {
                        dz.update(cx, |d, cx| {
                            d.report(
                                ticket,
                                kui::components::file_dropzone::UploadState::Uploaded,
                                cx,
                            );
                        });
                        if OfficeKind::for_path(&dest).is_some() {
                            this.open_office_file_tab(dest, false, None, window, cx);
                        } else if !this.pending_attachments.contains(&dest) {
                            dropped_paths.insert(ticket, dest.clone());
                            this.pending_attachments.push(dest);
                            this.notify_toast(format!("已添加附件「{safe_name}」"), cx);
                        }
                    } else {
                        dz.update(cx, |d, cx| {
                            d.report(
                                ticket,
                                kui::components::file_dropzone::UploadState::Failed(
                                    "写入临时文件失败".into(),
                                ),
                                cx,
                            );
                        });
                    }
                    cx.notify();
                }
            },
        )
        .detach();

        let terminal_cmd_input = cx.new(|cx| {
            Input::new("kar-terminal-cmd-input", "终端命令", window, cx)
                .with_compact_bar(30.)
                .with_placeholder("", cx)
        });

        let terminal_editor = terminal_cmd_input.read(cx).editor();
        terminal_editor.update(cx, |editor, _| editor.inherit_font_size = true);
        cx.subscribe_in(
            &terminal_editor,
            window,
            |this, _, event: &EditEvent, window, cx| match event {
                EditEvent::Submitted => this.submit_terminal_input(window, cx),
                EditEvent::Changed => cx.notify(),
                _ => {}
            },
        )
        .detach();

        let agent_choice =
            crate::client_store::desktop_preference(&config.chat_dir, "agent-choice")
                .and_then(|s| serde_json::from_str::<AgentChoice>(&s).ok())
                .filter(|choice| !choice.runtime.trim().is_empty());
        let approval_mode =
            crate::client_store::desktop_preference(&config.chat_dir, "new-chat-approvals")
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(ApprovalMode::Host);
        let new_chat_use_worktree =
            crate::client_store::desktop_preference(&config.chat_dir, "new-chat-worktree")
                .as_deref()
                == Some("true");
        let read_receipts = read_receipts::ThreadReadReceipts::load(&config.chat_dir, now_epoch_ms());
        let client_reducer = ClientReducer::new(&config.chat_dir);
        let fence_ledger = RevisionFenceLedger::load(&config.docs_dir);
        let schedule_registry = ScheduleRegistry::default();
        let project_registry =
            ProjectRegistry::load_or_seed(&config.chat_dir, &config.workspace_dir);
        let initial_briefs = if config.start_brief_demo {
            vec![build_showcase_kui_brief(None)]
        } else {
            Vec::new()
        };
        let initial_right_surface = if config.open_terminal_tab {
            RightPaneSurface::Terminal
        } else if config.sidebar_repo_files {
            RightPaneSurface::Files
        } else if config.open_git_tab {
            RightPaneSurface::GitWorkbench
        } else {
            RightPaneSurface::Office
        };
        let (initial_settings_open, initial_settings_tab) = if config.open_skills_sheet {
            (true, SettingsTab::SkillsMcp)
        } else if config.open_usage_sheet {
            (true, SettingsTab::Usage)
        } else if config.open_schedules_sheet {
            (true, SettingsTab::Schedules)
        } else if config.open_remote_sheet {
            (true, SettingsTab::Remote)
        } else if config.open_logs_sheet {
            (true, SettingsTab::Logs)
        } else {
            (false, SettingsTab::General)
        };
        let initial_git_sub_mode = if config.open_git_graph {
            GitSubMode::Graph
        } else {
            GitSubMode::Changes
        };

        let mut navigation = vec![WorkspaceLocation::Draft(config.workspace_dir.clone())];
        if config.open_tasks_inbox {
            navigation.push(WorkspaceLocation::Inbox);
        } else if config.open_office_gallery {
            navigation.push(WorkspaceLocation::Documents);
        }
        let mut this = Self {
            config: config.clone(),
            bridge: bridge.clone(),
            theme_mode,
            follow_system_appearance: override_mode.is_none(),
            ui_locale,
            focus_handle: cx.focus_handle(),
            previous_window_width: f32::from(window.viewport_size().width),
            restore_sidebar_after_wide: false,
            restore_sidebar_after_resize: false,
            drawer_return_focus: None,
            sidebar_open: initial_sidebar_open,
            navigation_index: navigation.len() - 1,
            navigation,
            sidebar_filter: String::new(),
            sidebar_project_filter: loaded_prefs.active_project_filter.clone(),
            settled_chats: loaded_prefs.settled_chats.clone(),
            sidebar_metadata: BTreeMap::new(),
            hosted_writer: None,
            settled_shelf_open: false,
            thread_details_open: !config.hide_details_card && if initial_width < COMPACT_WORKSPACE_WIDTH {
                loaded_prefs.compact_thread_details_open
            } else {
                loaded_prefs.thread_details_open
            },
            bottom_terminal_open: config.open_bottom_terminal || loaded_prefs.bottom_terminal_open,
            bottom_terminal_h: loaded_prefs.bottom_terminal_h.clamp(TERMINAL_MIN_H, 680.),
            bottom_terminal_anim: Slide::new(
                if config.open_bottom_terminal || loaded_prefs.bottom_terminal_open {
                    loaded_prefs.bottom_terminal_h.clamp(TERMINAL_MIN_H, 680.)
                } else {
                    0.
                },
            ),
            dragging_terminal: false,
            terminal_drag_anchor: None,
            expanded_turn_changes: BTreeMap::new(),
            layout_mode: initial_layout,
            split_ratio,
            pane_drag: None,
            sidebar_focus: cx.focus_handle(),
            sidebar_divider_focus: cx.focus_handle().tab_stop(true),
            doc_divider_focus: cx.focus_handle().tab_stop(true),
            sidebar_anim: Slide::new(if initial_sidebar_open { sidebar_width } else { 0. }),
            sidebar_width,
            split_anim: Slide::new(split_ratio),
            dock_reveal: Slide::new(if initial_layout == WorkspaceLayoutMode::ChatFocus { 0. } else { 1. }),
            sidebar_reveal: if initial_sidebar_open { 1.0 } else { 0.0 },
            open_menu: config.open_context_popover.then_some(Menu::ContextUsage),
            palette_return_focus: None,
            command_palette_open: false,
            cmd_query_editor,
            cmd_query: String::new(),
            cmd_selected_ix: 0,
            cmd_scroll: ScrollHandle::new(),
            reveal_cmd_selection: false,
            provider_sheet_open: false,
            agents_show_uninstalled: None,
            toast: None,
            notice: None,
            notice_open: false,
            sidebar_search,
            inbox_search,
            documents_search,
            coedit_segmented,
            coedit_dismiss_morph,
            composer_editor,
            question_draft: None,
            http_dialog,
            http_url_input,
            goal_dialog,
            goal_dialog_owner: None,
            goal_pending: BTreeSet::new(),
            goal_form_baseline: String::new(),
            goal_input,
            goal_budget_input,
            rename_dialog,
            rename_input,
            branch_input,
            branch_dialog,
            branch_workspace: None,
            rename_target_chat: None,
            quick_edit_dialog,
            quick_edit_primary,
            quick_edit_secondary,
            coedit_manual_editor,
            selected_runtime: agent_choice
                .as_ref()
                .map_or_else(|| crate::aster::ID.into(), |c| c.runtime.clone()),
            selected_model: agent_choice.as_ref().and_then(|c| c.model.clone()),
            providers_probing: true,
            provider_accounts_checked: false,
            approval_mode,
            reasoning_effort: agent_choice.and_then(|c| c.effort),
            providers: Vec::new(),
            registry_catalog: vec![], registry_installed: vec![],
            chat_list: Vec::new(),
            read_receipts,
            opened_unread_reply: None,
            pending_delete_chat: None,
            pending_navigation: None,
            creating_chat: None,
            access_mode: ChatAccessMode::NoneSelected,
            connection_state: FeedConnectionState::Idle,
            projected: ProjectedChatState::default(),
            session_commands: None,
            turn_targets: BTreeMap::new(),
            prompt_attempts: BTreeMap::new(),
            active_http_server_url: None,
            drafts: ComposerDrafts::new(config.workspace_dir.clone()),
            queued_prompt: None,
            slash: commands::SlashPicker::default(),
            planning: planning::Planning::default(),
            aster_planning: Default::default(),
            pending_attachments: Vec::new(),
            copy_buttons: BTreeMap::new(),
            part_open: BTreeMap::new(),
            turn_review: None,
            tool_detail: None,
            chat_scroll: ScrollHandle::new(),
            chat_follow_paused: false,
            turn_navigation: Default::default(),
            office_tabs: Vec::new(),
            active_office_tab: None,
            next_tab_id: 1,
            pinned_target: None,
            coedit_engine: Box::new(DemoCoEditEngine::new()),
            coedit_panel_open: false,
            active_coedit_scenario: initial_scenario,
            coedit_demo_dir: None,
            coedit_feedback: None,
            fence_ledger,
            client_reducer,
            active_briefs: initial_briefs,
            brief_widgets: BriefWidgetCache::default(),
            remote_sheet_open: config.open_remote_sheet,
            remote_tab_connect: false,
            kremote_invite_input,
            remote_telemetry: RemoteHostTelemetry::default(),
            remote_mode_segmented,
            remote_connect_url_input,
            remote_connect_code_input,
            remote_device_name_input,
            remote_requested_role: PeerRole::Controller,
            remote_pair_pending: false,
            remote_code_copied_at: None,
            schedules_sheet_open: config.open_schedules_sheet,
            schedule_creator_open: false,
            schedule_registry,
            schedule_refresh_ms: 0,
            schedule_workspace: None,
            schedule_loaded: false,
            schedule_error: None,
            editing_schedule_revision: None,
            schedule_pending_runs: BTreeMap::new(),
            schedule_save_pending: false,
            schedule_title_input,
            schedule_prompt_input,
            schedule_draft_cadence: ScheduleCadence::DailyNineAm,
            schedule_bind_active_doc: false,
            expanded_schedule_history: BTreeSet::new(),
            overlay_return_focus: None,
            settings_open: initial_settings_open,
            active_settings_tab: initial_settings_tab,
            project_registry,
            new_chat_use_worktree,
            right_surface: initial_right_surface,
            git_tab_open: config.open_git_tab,
            files_tab_open: config.sidebar_repo_files,
            terminal_tab_open: config.open_terminal_tab,
            committing_workspaces: BTreeSet::new(),
            git_sub_mode: initial_git_sub_mode,
            git_staged_limit: 32,
            git_unstaged_limit: 32,
            expanded_git_diff_file: None,
            inline_git_diffs: BTreeMap::new(),
            panel_workspace: config.workspace_dir.clone(),
            git_drafts: BTreeMap::new(),
            generating_commits: BTreeMap::new(),
            git_snapshot: if config.start_demo_chat || config.start_brief_demo {
                crate::studio_hub::demo_git_snapshot()
            } else {
                GitWorkspaceSnapshot::default()
            },
            git_branch_graph: None,
            git_commit_input,
            open_code_tabs: BTreeMap::new(),
            code_tab_order: Vec::new(),
            code_block_cache: BTreeMap::new(),
            patch_review_cache: RefCell::default(),
            tool_json_viewers: RefCell::default(),
            workspace_tree: Vec::new(),
            workspace_tree_view: None,
            tree_show_hidden: loaded_prefs.tree_show_hidden,
            terminal_runs: Vec::new(),
            terminal_cmd_input,
            skills_sheet_open: config.open_skills_sheet,
            workspace_inventory: crate::studio_hub::seed_local_workspace_inventory(
                &config.workspace_dir,
                "codex",
            ),
            active_inventory_tab: InventoryTab::All,
            skills_search_editor,
            skills_filter_query: String::new(),
            expanded_skill_name: None,
            expanded_tool_schema: None,
            usage_sheet_open: config.open_usage_sheet,
            usage_analytics: UsageAnalyticsSnapshot::default(),
            handoff_sheet_open: config.open_handoff_sheet,
            handoff_receipt: None,
            handoff_target_runtime: "claude".into(),
            handoff_budget_kb: 64,
            handoff_note_input,
            side_question_open: false,
            side_question_input,
            side_question_pending: false,
            side_question_last_reply: None,
            side_question_capability: None,
            side_question_request: None,
            logs_sheet_open: config.open_logs_sheet,
            diag_logs: DiagLogRing::default(),
            log_stream_widget: None,
            log_stream_revision: None,
            global_search_hits: Vec::new(),
            start_dropzone_open: false,
            file_dropzone,
            presence_stack: None,
            rollback_hold_buttons: RefCell::default(),
            deferred_delete_chat: None,
            live_prompt_suggestion: None,
            main_stage_view: if config.open_tasks_inbox {
                MainStageView::TasksInbox
            } else if config.open_office_gallery {
                MainStageView::OfficeGallery
            } else {
                MainStageView::Conversation
            },
            inbox_scope: InboxScopeFilter::All,
            usage_by_project: false,
            usage_period_days: 28,
            selected_skill_inspector: None,
            prefs: loaded_prefs,
            active_pty: None,
            pty_focus: cx.focus_handle(),
            detected_dev_ports: Vec::new(),
            active_voice_rec: None,
            voice_capture_id: None,
            voice_drafts: BTreeMap::new(),
            hardware_owners: Default::default(),
            migrating_voice_keys: Default::default(),
            held_voice_key: None,
            microphone_name: None,
            voice_env: Default::default(),
            installed_editors: Vec::new(),
            forge_prs: Vec::new(),
            forge_issues: Vec::new(),
            forge_error: None,
            forge_create_pr_open: false,
            forge_workspace:None, forge_pending:None, forge_last_operation:None, forge_draft_epoch:0,
            devices: Default::default(),
            external_transcripts: Vec::new(),
            storage_breakdown: None,
            managed_worktrees: Vec::new(),
            desktop_update_report: None,
            agent_update_statuses: BTreeMap::new(),
            provider_sign_in: Default::default(),
            editing_schedule_id: None,
            chat_search_open: false,
            chat_search_query: String::new(),
            chat_search_match_ix: 0,
            quit_pending_since: None,
            pending_confirm_action: None,
            caffeinate_guard: crate::forge_devices_voice::CaffeinateGuard::new(),
        };

        if config.open_office_gallery {
            this.bridge.send(KarCommand::RefreshWorkspaceTree {
                workspace: config.workspace_dir.clone(),
                show_hidden: this.tree_show_hidden,
            });
        }

        if initial_settings_open {
            this.select_settings_tab(initial_settings_tab, cx);
        }

        if !config.open_latest_chat && let Some(command) = this.config.initial_terminal_cmd.take() {
            this.run_terminal_command_str(command, window, cx);
        }

        for path in &config.open_files {
            this.open_office_file_tab(path.clone(), false, None, window, cx);
        }

        if let Some(url) = &config.subscribe_http {
            this.bridge
                .send(KarCommand::ConnectHttpFeed { url: url.clone() });
        } else if config.start_demo_chat {
            this.request_navigation(LocalChatDestination::Demo, cx);
        } else if config.start_brief_demo {
            this.access_mode = ChatAccessMode::DemoShowcase;
            this.projected.is_demo = true;
            this.projected.can_steer = true;
            this.projected.context_usage = Some(kar::ContextUsage {
                tokens: Some(37_900),
                context_window: Some(200_000),
                percent: Some(18.95),
            });
            this.sync_client_projection(&this.projected.clone());
        }

        if config.open_remote_sheet && (config.start_demo_chat || config.start_brief_demo) {
            this.bridge.send(KarCommand::StartHttpServer {
                chat_id: Uuid::nil(),
                listen_addr: "127.0.0.1:0".into(),
            });
        }

        if config.start_coedit_demo {
            this.enter_coedit_demo_workshop(window, cx);
            if let Some(sc) = config.initial_coedit_scenario {
                this.switch_coedit_scenario(sc, window, cx);
            }
        }

        // Background ticker (checks due `ScheduledTask`s and deferred chat deletions off the hot path)
        cx.spawn_in(window, async move |weak, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(1200)).await;
                let keep_going = weak.update_in(cx, |this, _, cx| {
                    this.poll_due_schedules(cx);
                    this.poll_deferred_delete_chat(cx);
                });
                if keep_going.is_err() {
                    break;
                }
            }
        })
        .detach();

        let quit_tx = bridge.tx.clone();
        cx.on_app_quit(move |_, _| {
            let quit_tx = quit_tx.clone();
            let _ = quit_tx.try_send(KarCommand::Shutdown);
            async move { quit_tx.closed().await; }
        }).detach();
        cx.on_release(|this, _| this.bridge.send(KarCommand::Shutdown)).detach();

        let rx = bridge.rx;
        cx.spawn_in(window, async move |weak, cx| {
            while let Ok(ev) = rx.recv().await {
                let ok = weak.update_in(cx, |this, window, cx| {
                    this.on_bridge_event(ev, window, cx);
                });
                if ok.is_err() {
                    break;
                }
            }
        })
        .detach();

        this.composer_editor
            .update(cx, |ed, cx| ed.focus(window, cx));
        this
    }

    // ── Shell ───────────────────────────────────────────────────────────────

    fn split_target(&self, mode: WorkspaceLayoutMode) -> f32 {
        match mode {
            WorkspaceLayoutMode::ChatFocus => 1.0,
            WorkspaceLayoutMode::Split => self.split_ratio.clamp(SPLIT_MIN, SPLIT_MAX),
            WorkspaceLayoutMode::CanvasWide => SPLIT_WIDE,
        }
    }

    pub fn set_layout_mode(&mut self, mode: WorkspaceLayoutMode, cx: &mut Context<Self>) {
        if self.layout_mode != mode {
            let narrow = self.previous_window_width < AUTO_COLLAPSE_RAIL_WIDTH;
            if mode == WorkspaceLayoutMode::CanvasWide && self.sidebar_open {
                self.set_sidebar_open(false, cx);
                self.restore_sidebar_after_wide = true;
            } else if self.layout_mode == WorkspaceLayoutMode::CanvasWide
                && std::mem::take(&mut self.restore_sidebar_after_wide)
                && (mode == WorkspaceLayoutMode::ChatFocus || !narrow)
            {
                self.set_sidebar_open(true, cx);
            } else if mode == WorkspaceLayoutMode::Split && narrow {
                self.set_sidebar_open(false, cx);
            }
            if mode != WorkspaceLayoutMode::ChatFocus {
                let target = self.split_target(mode);
                if self.dock_reveal.value(cx) <= 0.001 {
                    self.split_anim.reset(target);
                } else {
                    self.split_anim.duration(target, SHELL_SETTLE, cx);
                }
            }
            self.layout_mode = mode;
            let visible = mode != WorkspaceLayoutMode::ChatFocus;
            self.dock_reveal.duration(f32::from(visible), SHELL_SETTLE, cx);
            cx.notify();
        }
    }

    fn ensure_split_visible(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
            if self.sidebar_open
                && f32::from(window.viewport_size().width) < AUTO_COLLAPSE_RAIL_WIDTH
            {
                self.set_sidebar_open(false, cx);
            }
            self.set_layout_mode(WorkspaceLayoutMode::Split, cx);
        }
    }

    /// Show or hide the document pane beside the conversation.
    pub fn toggle_document_pane(&mut self, cx: &mut Context<Self>) {
        let next = if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
            WorkspaceLayoutMode::Split
        } else {
            WorkspaceLayoutMode::ChatFocus
        };
        self.set_layout_mode(next, cx);
    }

    pub fn toggle_thread_details(&mut self, cx: &mut Context<Self>) {
        self.thread_details_open = !self.thread_details_open;
        if self.previous_window_width < COMPACT_WORKSPACE_WIDTH {
            self.prefs.compact_thread_details_open = self.thread_details_open;
        } else {
            self.prefs.thread_details_open = self.thread_details_open;
        }
        if !self.thread_details_open {
            self.dismiss_thread_details_menu();
        }
        self.save_prefs();
        cx.notify();
    }

    fn dismiss_thread_details_menu(&mut self) {
        if matches!(self.open_menu, Some(Menu::DetailsBranch | Menu::DetailsOpenIn | Menu::DetailsScripts)) {
            self.open_menu = None;
        }
    }

    pub fn set_bottom_terminal_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if open && self.right_surface == RightPaneSurface::Terminal && self.layout_mode != WorkspaceLayoutMode::ChatFocus {
            self.set_layout_mode(WorkspaceLayoutMode::ChatFocus, cx);
        }
        if self.bottom_terminal_open != open {
            self.bottom_terminal_open = open;
            self.prefs.bottom_terminal_open = open;
            self.save_prefs();
            self.bottom_terminal_anim.duration(
                if open { self.bottom_terminal_h } else { 0. },
                0.22,
                cx,
            );
            cx.notify();
        }
    }

    pub fn toggle_bottom_terminal(&mut self, cx: &mut Context<Self>) {
        self.set_bottom_terminal_open(!self.bottom_terminal_open, cx);
    }

    pub fn toggle_settle_chat(&mut self, chat_id: Uuid, cx: &mut Context<Self>) {
        let settled = !self.settled_chats.contains(&chat_id);
        self.bridge.send(KarCommand::UpdateSidebar { chat_id, action: crate::kar_bridge::SidebarAction::Settle(settled) });
        cx.notify();
    }

    pub fn composer_placeholder_for_mode(&self, mode: &ChatAccessMode) -> &'static str {
        let loc = self.ui_locale;
        match mode {
            ChatAccessMode::NoneSelected => loc.pick(PLACEHOLDER_NEW_CHAT_ZH, PLACEHOLDER_NEW_CHAT_EN),
            ChatAccessMode::WriteController { .. } => loc.pick("继续追问…","Ask a follow-up…"),
            ChatAccessMode::RemoteController { role, .. } => {
                if role.can_mutate() {
                    loc.pick(PLACEHOLDER_REMOTE_CTRL, "Ask remote agent anything…")
                } else {
                    loc.pick(PLACEHOLDER_SHARED, "Read-only live shared thread")
                }
            }
            ChatAccessMode::DemoShowcase => {
                loc.pick(PLACEHOLDER_NEW_CHAT_ZH, PLACEHOLDER_NEW_CHAT_EN)
            }
            ChatAccessMode::LocalReadOnlySubscriber { .. } => {
                loc.pick(PLACEHOLDER_ELSEWHERE, "Active in another window (live read-only)")
            }
            ChatAccessMode::ArchivedReadOnly { .. } => {
                loc.pick(PLACEHOLDER_ARCHIVED, "Archived read-only thread")
            }
            ChatAccessMode::HttpReadOnly { .. } => {
                loc.pick(PLACEHOLDER_SHARED, "Read-only live shared thread")
            }
        }
    }

    pub fn save_prefs(&self) {
        self.prefs.save(&self.config.chat_dir);
    }

    pub(crate) fn forge_operation(&mut self,key:&str) -> Uuid {
        let id=self.forge_last_operation.as_ref().filter(|(k,_)|k==key).map(|(_,id)|*id).unwrap_or_else(Uuid::new_v4);
        self.forge_last_operation=Some((key.to_owned(),id));
        self.forge_pending=Some(id);
        id
    }

    pub fn confirm_or_run(
        &mut self,
        key: impl Into<String>,
        required: bool,
        hint: impl Into<String>,
        cx: &mut Context<Self>,
        action: impl FnOnce(&mut Self, &mut Context<Self>),
    ) {
        let key = key.into();
        if !required {
            self.pending_confirm_action = None;
            action(self, cx);
            return;
        }
        if let Some((prev_key, at)) = &self.pending_confirm_action
            && prev_key == &key
            && at.elapsed().as_secs_f32() < 5.0
        {
            self.pending_confirm_action = None;
            action(self, cx);
        } else {
            self.pending_confirm_action = Some((key, Instant::now()));
            self.notify_toast(hint.into(), cx);
        }
    }

    pub fn open_workspace_in_preferred_editor(
        &mut self,
        target: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.bridge.send(KarCommand::OpenEditor {
            workspace: self.effective_workspace_dir(), target,
            preferred: self.prefs.preferred_external_editor.clone(),
        });
        cx.notify();
    }

    pub(crate) fn follows_system_appearance(&self) -> bool {
        self.follow_system_appearance
    }

    pub(crate) fn set_follow_system_appearance(&mut self, cx: &mut Context<Self>) {
        self.follow_system_appearance = true;
        self.theme_mode = crate::system_theme(cx.window_appearance());
        let _ = crate::client_store::write_desktop_preference(
            &self.config.chat_dir,
            "appearance",
            "system",
        );
        crate::apply_theme_and_typography(self.theme_mode, &self.prefs, cx);
        cx.notify();
    }

    pub(crate) fn select_appearance(&mut self, mode: ThemeMode, cx: &mut Context<Self>) {
        self.follow_system_appearance = false;
        self.theme_mode = mode;
        let _ = crate::client_store::write_desktop_preference(
            &self.config.chat_dir,
            "appearance",
            if mode == ThemeMode::Dark {
                "dark"
            } else {
                "light"
            },
        );
        crate::apply_theme_and_typography(mode, &self.prefs, cx);
        cx.notify();
    }

    pub fn toggle_ui_locale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ui_locale = self.ui_locale.toggle();
        crate::apply_ui_language(self.ui_locale, cx);
        let _ = crate::client_store::write_desktop_preference(
            &self.config.chat_dir,
            "language",
            self.ui_locale.pick("zh", "en"),
        );
        let loc = self.ui_locale;
        let (label, description) = attachment_tray_labels(loc);
        self.file_dropzone
            .update(cx, |tray, cx| tray.set_labels(label, description, cx));
        for (input, label, hint) in [
            (
                &self.schedule_title_input,
                loc.pick("任务标题", "Task name"),
                loc.pick(
                    "例如：每日早九点核对预算表并生成 KUI 简报",
                    "Name this task",
                ),
            ),
            (
                &self.schedule_prompt_input,
                loc.pick("智能体执行指令", "Instructions"),
                loc.pick(
                    "描述定时触发时智能体要检查、汇总或生成的具体内容…",
                    "What should the agent do?",
                ),
            ),
            (
                &self.side_question_input,
                loc.pick("旁路问题", "Your question"),
                loc.pick(
                    "例如：刚才我们约定的预算字段名是什么？",
                    "Ask about this conversation…",
                ),
            ),
            (
                &self.handoff_note_input,
                loc.pick("交办说明", "Handoff note (optional)"),
                loc.pick(
                    "可选：告诉新智能体接手后优先做什么…",
                    "What should the next agent do first?",
                ),
            ),
        ] {
            input.update(cx, |field, cx| field.set_label_and_hint(label, hint, cx));
        }
        self.log_stream_revision = None;
        self.log_stream_widget = None;
        if self.settings_open && self.active_settings_tab == SettingsTab::Usage {
            self.bridge.send(KarCommand::RefreshUsageAnalytics { providers: self.providers.clone() });
        }
        let search_ph = self.ui_locale.pick("搜索", "Search");
        self.sidebar_search.update(cx, |ed, cx| {
            ed.set_label(self.ui_locale.pick("搜索对话", "Search threads"));
            ed.set_placeholder(search_ph);
            cx.notify();
        });
        for (editor, placeholder) in [
            (
                &self.skills_search_editor,
                self.ui_locale.pick("筛选技能、MCP、指令…", "Search skills and tools…"),
            ),
            (
                &self.cmd_query_editor,
                self.ui_locale.pick(
                    "搜索命令、文件和对话…",
                    "Search commands, files, and threads…",
                ),
            ),
            (
                &self.inbox_search,
                self.ui_locale.pick("搜索收件箱", "Search inbox"),
            ),
            (
                &self.documents_search,
                self.ui_locale.pick("搜索文档", "Search documents"),
            ),
        ] {
            editor.update(cx, |ed, cx| {
                ed.set_placeholder(placeholder);
                cx.notify();
            });
        }
        let cmp_ph = self.composer_placeholder_for_mode(&self.access_mode);
        self.composer_editor.update(cx, |ed, cx| {
            ed.set_placeholder(cmp_ph);
            cx.notify();
        });
        let invite = self.kremote_invite_input.read(cx).value(cx).to_string();
        self.kremote_invite_input = Self::kremote_invite_field(self.ui_locale, window, cx);
        self.kremote_invite_input.update(cx, |input, cx| input.set_value(invite, window, cx));
        self.remote_mode_segmented =
            Self::remote_mode_selector(self.ui_locale, self.remote_tab_connect, cx);
        let values = [
            &self.remote_connect_url_input,
            &self.remote_connect_code_input,
            &self.remote_device_name_input,
        ]
        .map(|input| input.read(cx).value(cx).to_string());
        [
            self.remote_connect_url_input,
            self.remote_connect_code_input,
            self.remote_device_name_input,
        ] = Self::remote_fields(self.ui_locale, values, window, cx);
        cx.notify();
    }

    /// Give the document most of the room, or put the divider back where it was.
    pub fn toggle_wide_document(&mut self, cx: &mut Context<Self>) {
        let next = if self.layout_mode == WorkspaceLayoutMode::CanvasWide {
            WorkspaceLayoutMode::Split
        } else {
            WorkspaceLayoutMode::CanvasWide
        };
        self.set_layout_mode(next, cx);
    }

    pub(crate) fn collection_search_field(
        &self,
        editor: Entity<Editor>,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let target = editor.clone();
        div()
            .id(("collection-search", editor.entity_id()))
            .flex_1()
            .min_w(px(140.))
            .h(px(34.))
            .px(px(10.))
            .gap(px(8.))
            .border_1()
            .border_color(s.line)
            .rounded(px(8.))
            .flex()
            .items_center()
            .child(pro_icon(ProIcon::Search, 15., s.muted))
            .child(div().flex_1().min_w_0().h(px(21.)).child(editor))
            .on_click(
                cx.listener(move |_, _, win, cx| target.update(cx, |ed, cx| ed.focus(win, cx))),
            )
    }

    fn dismiss_compact_sidebar(&mut self, cx: &mut Context<Self>) {
        if self.previous_window_width < COMPACT_WORKSPACE_WIDTH && self.sidebar_open {
            self.set_sidebar_open(false, cx);
        }
    }

    pub fn set_sidebar_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.sidebar_open != open {
            if self.previous_window_width >= COMPACT_WORKSPACE_WIDTH {
                self.restore_sidebar_after_resize = false;
            }
            self.restore_sidebar_after_wide = false;
            self.sidebar_open = open;
            self.sidebar_anim.duration(
                if open { self.sidebar_width } else { 0. },
                SHELL_SETTLE,
                cx,
            );
            cx.notify();
        }
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        let next = !self.sidebar_open;
        self.prefs.sidebar_open = Some(next);
        self.save_prefs();
        self.set_sidebar_open(next, cx);
    }

    // ── Menus, dialogs, toasts, notices ─────────────────────────────────────

    /// Open a menu, or close it if it is the one already open.
    pub fn toggle_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        self.open_menu = (self.open_menu != Some(menu)).then_some(menu);
        cx.notify();
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.open_menu.take().is_some() {
            cx.notify();
        }
    }

    /// A press landed outside `menu` while it was open.
    pub(crate) fn dismiss_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        if self.open_menu == Some(menu) {
            self.open_menu = None;
            cx.notify();
        }
    }

    pub(crate) fn render_menu(
        &self,
        menu: Menu,
        placement: MenuPlacement,
        dimensions: impl Into<crate::ui_motion::MenuSize>,
        rows: Vec<AnyElement>,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.open_menu == Some(menu);
        let weak = cx.weak_entity();
        let on_dismiss: DismissFn = Rc::new(move |_, cx| {
            if let Some(entity) = weak.upgrade() {
                entity.update(cx, |this, cx| this.dismiss_menu(menu, cx));
            }
        });
        menu_popup(menu.id(), open, placement, dimensions, rows, on_dismiss, s)
    }

    fn any_dialog_open(&self, cx: &App) -> bool {
        self.http_dialog.read(cx).is_open()
            || self.goal_dialog.read(cx).is_open()
            || self.quick_edit_dialog.read(cx).is_open()
            || self.rename_dialog.read(cx).is_open()
            || self.branch_dialog.read(cx).is_open()
    }

    pub(crate) fn open_rename_chat_dialog(
        &mut self,
        chat_id: Uuid,
        current_title: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_menu = None;
        self.command_palette_open = false;
        self.provider_sheet_open = false;
        self.rename_target_chat = Some(chat_id);
        self.rename_input
            .update(cx, |inp, cx| inp.set_value(current_title.to_owned(), window, cx));
        self.rename_dialog
            .update(cx, |d, cx| d.set_open(true, window, cx));
        self.rename_input
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        cx.notify();
    }

    /// Briefly confirm something that worked.
    pub fn notify_toast(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.toast = Some(ToastState::new(msg, false));
        cx.notify();
    }

    /// Briefly say that something did not work.
    pub fn notify_warning(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.toast = Some(ToastState::new(msg, true));
        cx.notify();
    }

    pub fn show_toast(
        &mut self,
        msg: impl Into<SharedString>,
        warning: bool,
        cx: &mut Context<Self>,
    ) {
        self.toast = Some(ToastState::new(msg, warning));
        cx.notify();
    }

    pub(crate) fn show_notice(&mut self, notice: Notice, cx: &mut Context<Self>) {
        self.notice = Some(notice);
        self.notice_open = true;
        cx.notify();
    }

    /// Hide the notice if it is about `topic`. The words stay until it has folded away.
    pub(crate) fn clear_notice(&mut self, topic: NoticeTopic, cx: &mut Context<Self>) {
        if self.notice_open && self.notice.as_ref().is_some_and(|n| n.topic == topic) {
            self.notice_open = false;
            cx.notify();
        }
    }

    pub(crate) fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        if self.notice_open {
            self.notice_open = false;
            cx.notify();
        }
    }

    pub fn toggle_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_palette_open {
            self.close_command_palette(window, cx);
        } else {
            let origin = if self.settings_open || self.handoff_sheet_open || self.side_question_open
            {
                self.overlay_return_focus.clone()
            } else {
                window.focused(cx)
            };
            self.close_all_overlays(cx);
            self.palette_return_focus = origin;
            self.command_palette_open = true;
            self.cmd_query.clear();
            self.global_search_hits.clear();
            self.cmd_selected_ix = 0;
            self.cmd_scroll.set_offset(point(px(0.), px(0.)));
            self.cmd_query_editor.update(cx, |ed, cx| {
                ed.set_value("", window, cx);
                ed.focus(window, cx);
            });
            cx.notify();
        }
    }

    fn close_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_palette_open {
            self.command_palette_open = false;
            self.palette_return_focus
                .take()
                .as_ref()
                .unwrap_or(&self.focus_handle)
                .focus(window, cx);
            cx.notify();
        }
    }

    fn accept_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.command_palette_open = false;
        self.palette_return_focus = None;
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    pub fn toggle_provider_sheet(&mut self, cx: &mut Context<Self>) {
        self.toggle_settings(SettingsTab::Agents, cx);
    }

    fn filtered_palette_items(&self) -> Vec<&'static PaletteItem> {
        let q = self.cmd_query.trim().to_lowercase();
        PALETTE_ITEMS
            .iter()
            .filter(|item|item.id!="side_question" || self.supports_side_question())
            .filter(|item| {
                item.label.to_lowercase().contains(&q)
                    || item.label_en.to_lowercase().contains(&q)
                    || item.section.to_lowercase().contains(&q)
                    || item.keywords.to_lowercase().contains(&q)
            })
            .collect()
    }

    fn can_prompt_from_palette(&self) -> bool {
        self.access_mode.is_writable() || matches!(self.access_mode, ChatAccessMode::NoneSelected)
    }

    fn confirm_selected_palette_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.filtered_palette_items();
        let trimmed = self.cmd_query.trim().to_owned();
        let searching = !trimmed.is_empty();
        let has_ask = searching && self.can_prompt_from_palette();
        let shown_hits: Vec<GlobalSearchHit> = if searching {
            self.global_search_hits.iter().take(12).cloned().collect()
        } else {
            Vec::new()
        };
        let total = items.len() + shown_hits.len() + usize::from(has_ask);
        if total == 0 {
            self.close_command_palette(window, cx);
            return;
        }
        let ix = self.cmd_selected_ix.min(total - 1);
        self.accept_command_palette(window, cx);
        if ix < items.len() {
            let cmd_id = items[ix].id;
            self.execute_command_id(cmd_id, window, cx);
        } else if ix < items.len() + shown_hits.len() {
            let hit = &shown_hits[ix - items.len()];
            match hit {
                GlobalSearchHit::FilePath { rel_path }
                | GlobalSearchHit::FileContent { rel_path, .. } => {
                    self.open_workspace_any_file(rel_path.clone(), window, cx);
                }
                GlobalSearchHit::ChatMessage { chat_id, .. } => {
                    self.open_chat(*chat_id, window, cx);
                }
            }
        } else if has_ask {
            self.fill_starter_prompt(&trimmed, window, cx);
            self.submit_composer_prompt(window, cx);
        }
        cx.notify();
    }

    fn execute_command_id(&mut self, cmd_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match cmd_id {
            "new_chat" => self.start_new_chat(window, cx),
            "tasks_inbox" => self.open_tasks_inbox_stage(cx),
            "office_gallery" => self.open_office_gallery_stage(cx),
            "bind_selection" => self.bind_active_selection_to_chat(window, cx),
            "pick_workspace" => self.pick_workspace(window, cx),
            "pick_attachment" => self.pick_attachments(window, cx),
            "open_file" => self.pick_documents(window, cx),
            "save_doc" => self.save_active_document(window, cx),
            "save_as_doc" => self.prompt_save_as_active_document(window, cx),
            "new_docx" => self.open_blank_office_tab(OfficeKind::Word, window, cx),
            "new_xlsx" => self.open_blank_office_tab(OfficeKind::Excel, window, cx),
            "new_pptx" => self.open_blank_office_tab(OfficeKind::Ppt, window, cx),
            "toggle_docs" => self.toggle_document_pane(cx),
            "toggle_wide" => self.toggle_wide_document(cx),
            "toggle_sidebar" => self.toggle_sidebar(cx),
            "toggle_theme" => {
                self.select_appearance(self.theme_mode.toggle(), cx);
            }
            "open_settings" => self.open_settings(SettingsTab::General, cx),
            "provider_health" => self.open_settings(SettingsTab::Agents, cx),
            "remote_pair" => self.open_settings(SettingsTab::Remote, cx),
            "schedules" => self.open_settings(SettingsTab::Schedules, cx),
            "git_workbench" => self.open_git_workbench_tab(cx),
            "files_dock" => self.open_files_dock_tab(cx),
            "terminal_dock" => self.open_terminal_dock_tab(cx),
            "skills_mcp" => self.open_settings(SettingsTab::SkillsMcp, cx),
            "usage_analytics" => self.open_settings(SettingsTab::Usage, cx),
            "handoff_chat" => self.toggle_handoff_sheet(cx),
            "side_question" => self.toggle_side_question_sheet(cx),
            "logs_stream" => self.open_settings(SettingsTab::Logs, cx),
            "insert_brief" => self.insert_showcase_brief(cx),
            "context_inspector" => {
                self.toggle_menu(Menu::ContextUsage, cx);
            }
            "start_http" => self.start_sharing(cx),
            "http_sub" => self.open_shared_link_dialog(window, cx),
            "second_win" => self.open_subscriber_second_window(cx),
            "open_docx" => { self.open_working_sample("sample_minutes.docx", window, cx); }
            "open_xlsx" => { self.open_working_sample("sample_formulas.xlsx", window, cx); }
            "open_pptx" => { self.open_working_sample("sample_deck.pptx", window, cx); }
            "demo_chat" => self.open_sample_chat(window, cx),
            "coedit_word" => {
                self.enter_coedit_demo_workshop(window, cx);
                self.switch_coedit_scenario(ScenarioKey::WordSameParagraph, window, cx);
            }
            "coedit_excel" => {
                self.enter_coedit_demo_workshop(window, cx);
                self.switch_coedit_scenario(ScenarioKey::ExcelSameCellConflict, window, cx);
            }
            "coedit_ppt" => {
                self.enter_coedit_demo_workshop(window, cx);
                self.switch_coedit_scenario(ScenarioKey::PptMoveAndText, window, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    // ── What Kar reports ────────────────────────────────────────────────────

    fn on_bridge_event(&mut self, ev: KarBridgeEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            KarBridgeEvent::StorageInspected(report) => self.storage_breakdown = Some(report),
            KarBridgeEvent::EditorsLoaded(editors) => self.installed_editors = editors,
            KarBridgeEvent::RegistryCatalog(rows) => self.registry_catalog = rows,
            KarBridgeEvent::RegistryInstalled(rows) => {
                crate::registry::merge(&mut self.providers, &rows);
                self.registry_installed = rows;
            }
            KarBridgeEvent::LegacyAcpRegistered(id) => {
                self.prefs.custom_acp_agents.retain(|agent| agent.id != id);
                self.save_prefs();
            }
            KarBridgeEvent::ForgeCredentialStored(index) => {
                let old = match index { 0 => &mut self.prefs.github_token, 1 => &mut self.prefs.gitlab_token, _ => &mut self.prefs.linear_token };
                if !old.is_empty() { old.clear(); self.save_prefs(); }
            }
            KarBridgeEvent::StoreReady { .. } => {
                self.bridge.send(KarCommand::Registry(crate::registry::Action::List));
                for agent in self.prefs.custom_acp_agents.iter().filter(|agent| agent.enabled) {
                    self.bridge.send(KarCommand::Registry(crate::registry::Action::Register(agent.clone())));
                }

                let legacy = [self.prefs.github_token.clone(), self.prefs.gitlab_token.clone(), self.prefs.linear_token.clone()];
                if legacy.iter().any(|s| !s.is_empty()) {
                    self.bridge.send(KarCommand::StoreForgeCredentials(crate::kar_bridge::ForgeCredentials(legacy)));
                }
            }
            KarBridgeEvent::ProvidersProbing(probing) => {
                self.providers_probing = probing;
            }
            KarBridgeEvent::ProvidersUpdated { providers: mut list, accounts_checked } => {
                self.provider_accounts_checked = accounts_checked;
                if accounts_checked {
                    for provider in &list {
                        if matches!(provider.auth, crate::types::ProviderAuthState::LoggedIn { .. }) {
                            self.provider_sign_in.signed_in(&provider.id);
                        }
                    }
                }
                crate::registry::merge(&mut list, &self.registry_installed);
                if crate::aster::is_aster(&self.selected_runtime)
                    && let Some(aster) = list.iter().find(|p| crate::aster::is_aster(&p.id))
                {
                    self.selected_runtime = aster.id.clone();
                }
                if let Some(sel_model) = &self.selected_model
                    && let Some(agent) = list.iter().find(|p| p.id == self.selected_runtime)
                    && !agent.models.is_empty()
                    && !agent.models.iter().any(|m| &m.id == sel_model)
                    && !self.prefs.has_custom_model(&agent.id, sel_model)
                {
                    self.selected_model = None;
                }
                self.providers = list;
                self.refresh_agent_notice(cx);
            }
            KarBridgeEvent::SidebarMetadata { dir, rows } => {
                if self.config.chat_dir != dir { return; }
                for (id, meta) in &rows {
                    if meta.pinned { self.project_registry.pinned_chats.insert(*id); } else { self.project_registry.pinned_chats.remove(id); }
                    if meta.settled || meta.archived { self.settled_chats.insert(*id); } else { self.settled_chats.remove(id); }
                }
                self.sidebar_metadata = rows;
                for entry in &mut self.chat_list {
                    if let Some(title) = self.sidebar_metadata.get(&entry.info.id).and_then(|m| m.title.as_deref()) {
                        entry.title = title.to_owned();
                    }
                }
            }
            KarBridgeEvent::ReadReceiptsChanged { dir, bytes } => {
                if self.config.chat_dir == dir { self.read_receipts.refresh(&bytes); }
            }
            KarBridgeEvent::ChatsPatched { dir, changed, removed } => {
                if self.config.chat_dir != dir { return; }
                self.chat_list.retain(|entry| !removed.contains(&entry.info.id));
                for mut entry in changed {
                    if let Some(title) = self.sidebar_metadata.get(&entry.info.id).and_then(|m| m.title.as_ref()) {
                        entry.title.clone_from(title);
                    }
                    if let Some(existing) = self.chat_list.iter_mut().find(|old| old.info.id == entry.info.id) {
                        *existing = entry;
                    } else {
                        self.chat_list.push(entry);
                    }
                }
                self.chat_list.sort_by_key(|entry| std::cmp::Reverse(entry.last_activity_ms));
            }
            KarBridgeEvent::ChatsListed(entries) => {
                let latest = std::mem::take(&mut self.config.open_latest_chat);
                let resume = std::mem::take(&mut self.config.resume_last_chat);
                self.chat_list = entries;
                for entry in &mut self.chat_list {
                    if let Some(title) = self.sidebar_metadata.get(&entry.info.id).and_then(|m| m.title.as_deref()) {
                        entry.title = title.to_owned();
                    }
                }
                if (latest || resume) && matches!(self.access_mode, ChatAccessMode::NoneSelected)
                    && self.queued_prompt.is_none() && self.composer_editor.read(cx).value().is_empty()
                    && self.config.subscribe_http.is_none() && !self.config.start_demo_chat {
                    let chat = if latest {
                        self.chat_list.first().map(|entry| entry.info.id)
                    } else {
                        self.resume_chat_or_workspace_plan(cx)
                    };
                    if let Some(chat_id) = chat {
                        self.request_navigation(LocalChatDestination::Thread(chat_id), cx);
                    } else if self.config.initial_terminal_cmd.take().is_some() {
                        self.notify_warning("没有可重新打开的对话，启动命令已取消", cx);
                    }
                }
            }
            KarBridgeEvent::TurnTargetsLoaded(map) => {
                self.turn_targets = map;
                self.turn_navigation.invalidate();
            }
            KarBridgeEvent::TurnDiffLoaded { key, result } => {
                if let Some(review) = &mut self.turn_review && review.key == key {
                    review.patch = Some(result.map(std::sync::Arc::new));
                }
            }
            KarBridgeEvent::WorkspaceFilesRestored { files } => {
                let restored: Vec<PathBuf> = files
                    .into_iter()
                    .map(|p| p.canonicalize().unwrap_or(p))
                    .collect();
                let active_idx = self.active_office_tab;
                let mut reloaded_any = false;
                for idx in 0..self.office_tabs.len() {
                    let should_reload = {
                        let tab = &self.office_tabs[idx];
                        !tab.is_demo
                            && !tab.is_dirty(cx)
                            && tab
                                .path(cx)
                                .map(|p| restored.contains(&p.canonicalize().unwrap_or(p)))
                                .unwrap_or(false)
                    };
                    if should_reload && let Some(path) = self.office_tabs[idx].path(cx) {
                        let id = self.office_tabs[idx].id;
                        if let Ok(fresh) = OfficeTab::open_file(id, path, false, None, window, cx) {
                            self.office_tabs[idx] = fresh;
                            reloaded_any = true;
                        }
                    }
                }
                self.active_office_tab = active_idx;
                if reloaded_any {
                    self.notify_toast("已同步恢复右侧打开的文档", cx);
                }
            }
            KarBridgeEvent::SessionCommandsUpdated { chat_id, commands } => {
                if matches!(self.access_mode, ChatAccessMode::WriteController { chat_id: active } if active == chat_id) {
                    self.session_commands = Some((chat_id, commands));
                }
            }
            KarBridgeEvent::AsterModeReady { chat_id, result } => self.aster_mode_result(chat_id, result, window, cx),
            KarBridgeEvent::PlanChatCreated { chat_id, request_id, topic } => {
                self.plan_chat_created(chat_id, request_id, topic, window, cx);
            }
            KarBridgeEvent::PlanDelta {chat_id, request_id, item, delta} => self.plan_delta(chat_id, request_id, item, &delta, cx),
            KarBridgeEvent::PlanProgress { chat_id, request_id, label } => self.plan_progress(chat_id,request_id,label,cx),
            KarBridgeEvent::PlanGenerated { chat_id, request_id, result } => {
                self.finish_planning(chat_id, request_id, result, window, cx);
            }
            KarBridgeEvent::PlanQuestions {chat_id, request_id, questions} => {
                self.receive_plan_questions(chat_id, request_id, questions, window, cx);
            }
            KarBridgeEvent::HandoffReceipt { chat_id, latest } => {
                if self.current_chat_id() == Some(chat_id) {
                    self.handoff_receipt = latest.map(|row| (chat_id,row));
                }
            }
            KarBridgeEvent::HostedSession(chat) => self.hosted_writer = Some(chat),
            KarBridgeEvent::NavigationStarted(request) => {
                if let Some(pending) = &mut self.pending_navigation {
                    pending.started = pending.request == request;
                }
            }
            KarBridgeEvent::NavigationUnavailable(request) => {
                if self.pending_navigation.as_ref().is_some_and(|p| p.request == request) {
                    self.pending_navigation = None;
                    let writable = self.access_mode.is_writable()
                        || matches!(self.access_mode, ChatAccessMode::NoneSelected);
                    self.composer_editor.update(cx, |editor, cx| {
                        editor.disabled = !writable;
                        cx.notify();
                    });
                    self.notify_warning(
                        self.ui_locale.pick("暂时无法打开对话存储", "Chat storage is unavailable"),
                        cx,
                    );
                }
            }
            KarBridgeEvent::ChatCreated { request, chat_id } => {
                if !self.owns_chat_creation(request) {
                    return;
                }
                self.creating_chat = None;
                let mode = ChatAccessMode::WriteController { chat_id };
                self.on_bridge_event(KarBridgeEvent::AccessModeChanged(mode), window, cx);
                if let Some(queued) = self.queued_prompt.take() {
                    self.dispatch_prompt(queued.text, queued.target, queued.attachments, cx);
                }
            }
            KarBridgeEvent::AccessModeChanged(mode) => {
                if self.pending_navigation.as_ref().is_some_and(|navigation| {
                    !navigation.started || !navigation.destination.matches(&mode)
                }) {
                    return;
                }
                self.pending_navigation = None;
                self.hosted_writer = None;
                if let Some(id) = mode.selected_chat_id() { self.load_saved_plan(id, cx); }
                self.session_commands = None;
                let writable =
                    mode.is_writable() || matches!(mode, ChatAccessMode::NoneSelected);
                let placeholder = self.composer_placeholder_for_mode(&mode);
                self.composer_editor.update(cx, |ed, cx| {
                    ed.disabled = !writable;
                    ed.set_placeholder(placeholder);
                    cx.notify();
                });
                let chat_changed = std::mem::discriminant(&self.access_mode)
                    != std::mem::discriminant(&mode)
                    || self.current_chat_id() != chat_id_of(&mode);
                if chat_changed && self.config.subscribe_http.is_none()
                    && let ChatAccessMode::WriteController { chat_id } | ChatAccessMode::LocalReadOnlySubscriber { chat_id, .. }
                        | ChatAccessMode::ArchivedReadOnly { chat_id, .. } = &mode
                    && let Err(err) = crate::client_store::remember_chat(&self.config.chat_dir, *chat_id) {
                    self.notify_warning(format!("无法保存上次打开的对话：{err}"), cx);
                }
                if chat_changed {
                    self.side_question_open=false;
                    self.side_question_capability=None;
                    self.side_question_request = None;
                    self.side_question_pending = false;
                    self.side_question_last_reply = None;
                    self.side_question_input
                        .update(cx, |input, cx| input.set_value("", window, cx));
                }
                if self.settings_open
                    && self.active_settings_tab == SettingsTab::Remote
                    && self.access_mode.is_remote_paired() != mode.is_remote_paired()
                {
                    self.focus_handle.focus(window, cx);
                }
                if self.remote_pair_pending
                    && matches!(mode, ChatAccessMode::RemoteController { .. })
                {
                    self.remote_pair_pending = false;
                    self.remote_connect_code_input
                        .update(cx, |input, cx| input.set_value("", window, cx));
                }
                if self.access_mode != mode
                    && (self.access_mode.is_remote_paired() || mode.is_remote_paired())
                {
                    self.projected.queued_inputs.clear();
                }
                self.access_mode = mode;
                self.sync_plan_placeholder(cx);
                let live = matches!(self.access_mode, ChatAccessMode::HttpReadOnly { .. } | ChatAccessMode::RemoteController { .. });
                if (chat_changed || live) && self.main_stage_view == MainStageView::Conversation {
                    match self.access_mode {
                        ChatAccessMode::WriteController { chat_id }
                        | ChatAccessMode::LocalReadOnlySubscriber { chat_id, .. }
                        | ChatAccessMode::ArchivedReadOnly { chat_id, .. } => {
                            self.record_location(WorkspaceLocation::Thread(chat_id));
                        }
                        ChatAccessMode::DemoShowcase => self.record_location(WorkspaceLocation::Demo),
                        ChatAccessMode::HttpReadOnly { ref url, .. }
                        | ChatAccessMode::RemoteController { ref url, .. } => {
                            self.record_location(WorkspaceLocation::Live(url.clone()));
                        }
                        ChatAccessMode::NoneSelected => {}
                    }
                }
                self.switch_composer_draft(window, cx);
                if chat_changed {
                    self.clear_notice(NoticeTopic::Event, cx);
                    self.clear_notice(NoticeTopic::Connection, cx);
                    self.tool_detail = None;
                    self.live_prompt_suggestion = None;
                    self.turn_review = None;
                    self.question_draft = None;
                    self.patch_review_cache.get_mut().retain(|key, _| !key.starts_with("turn-patch-"));
                    self.sync_workspace_panels(window, cx);
                    self.client_reducer.detail_cache.release_all_unlocked();
                    if let Some(cid) = self.current_chat_id() {
                        let inv = BriefInventory::load(&self.config.chat_dir, &cid.to_string());
                        self.active_briefs = inv.briefs;
                    } else if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
                        if !self.config.start_brief_demo {
                            self.active_briefs.clear();
                        }
                    } else {
                        self.active_briefs.clear();
                    }
                }
                self.refresh_agent_notice(cx);
            }
            KarBridgeEvent::ConnectionStateChanged(conn) => {
                if self.pending_navigation.is_some() {
                    return;
                }
                match &conn {
                    FeedConnectionState::Disconnected { reason, .. } => {
                        let detail = format!("{reason} 已收到的内容都还在。");
                        self.show_notice(
                            Notice {
                                topic: NoticeTopic::Connection,
                                tone: NoticeTone::Error,
                                title: "连接已断开".into(),
                                detail,
                                action: NoticeAction::None,
                            },
                            cx,
                        );
                    }
                    FeedConnectionState::Connected { .. } | FeedConnectionState::Idle => {
                        self.clear_notice(NoticeTopic::Connection, cx);
                    }
                    _ => {}
                }
                self.connection_state = conn;
            }
            KarBridgeEvent::ChatTurnActivity { chat_id, status, activity_ms } => {
                if let Some(entry) = self.chat_list.iter_mut().find(|entry| entry.info.id == chat_id)
                    && entry.last_activity_ms.is_none_or(|last| activity_ms >= last)
                {
                    entry.last_status = Some(status);
                    entry.last_activity_ms = Some(activity_ms);
                }
            }
            KarBridgeEvent::ChatStateUpdated(mut proj) => {
                if self.pending_navigation.is_some() {
                    return;
                }
                // Ignore stale projections arriving from a previously deselected chat.
                if let Some(proj_cid) = proj.chat_id
                    && !proj.is_demo
                {
                    if matches!(self.access_mode, ChatAccessMode::NoneSelected) {
                        return;
                    }
                    if let Some(cur_cid) = self.current_chat_id()
                        && cur_cid != proj_cid
                    {
                        return;
                    }
                }

                if proj.chat_id == self.projected.chat_id && proj.goal.is_none() {
                    proj.goal = self.projected.goal.clone();
                }
                if self.question_draft.as_ref().is_some_and(|(cursor, _)| {
                    !proj
                        .pending_requests
                        .iter()
                        .any(|request| &request.cursor == cursor)
                }) {
                    self.question_draft = None;
                }
                let same_chat =
                    proj.chat_id == self.projected.chat_id && proj.is_demo == self.projected.is_demo;
                let max_y = f32::from(self.chat_scroll.max_offset().y);
                let cur_y = f32::from(self.chat_scroll.offset().y);
                let at_end = max_y + cur_y <= 1.;
                let new_turn_appended = same_chat && proj.turns.len() > self.projected.turns.len();
                let switched_chat = !same_chat;
                if switched_chat {
                    self.chat_follow_paused = false;
                }

                let was_running = same_chat && self.turn_in_progress();
                let had_pending = same_chat && !self.projected.pending_requests.is_empty();
                self.turn_navigation.update_turns(&self.projected, &proj);
                self.sync_timeline_entities(&proj, cx);
                self.sync_client_projection(&proj);
                let workspace_changed = self.projected.info.as_ref().map(|i| &i.workspace)
                    != proj.info.as_ref().map(|i| &i.workspace);
                if same_chat && self.access_mode.is_remote_paired() {
                    // Kar's public event feed projects turns; the paired state endpoint owns its queue.
                    proj.queued_inputs = std::mem::take(&mut self.projected.queued_inputs);
                }
                if self.projected.goal != proj.goal && let Some(id) = proj.chat_id {
                    self.sync_goal_form(id, proj.goal.as_ref().and_then(Option::as_ref), window, cx);
                }
                if let (Some(id), Some(turn)) = (proj.chat_id, proj.turns.last())
                    && let Some(entry) = self.chat_list.iter_mut().find(|entry| entry.info.id == id)
                {
                    if entry.turn_count == 0 && entry.title == "新对话" {
                        entry.title = ChatListEntry::from_info_and_turns(entry.info.clone(), &proj.turns).title;
                    }
                    entry.turn_count = proj.turns.len();
                    entry.last_status = Some(turn.status);
                    entry.last_activity_ms = Some(turn.finished_ms.unwrap_or(turn.started_ms));
                }
                self.projected = proj;
                self.reconcile_plan_submission(cx);
                if !had_pending
                    && self.access_mode.is_writable()
                    && let Some(info) = &self.projected.info
                {
                    let is_allow = crate::prefs::is_approval_allow(info.approvals);
                    let is_deny = info.approvals == ApprovalMode::Deny;
                    if is_allow || is_deny {
                        for req in self.projected.pending_requests.clone() {
                            if req.kind == kar::RequestKind::Permission {
                                let decision = if is_allow {
                                    let controls = req.controls();
                                    if controls
                                        .decisions
                                        .contains(&kar::PermissionDecision::AllowForSession)
                                    {
                                        kar::PermissionDecision::AllowForSession
                                    } else {
                                        kar::PermissionDecision::AllowOnce
                                    }
                                } else {
                                    kar::PermissionDecision::Deny
                                };
                                self.bridge.send(KarCommand::DecidePermission {
                                    request: req,
                                    decision,
                                });
                            }
                        }
                    }
                }
                let now_running = self.turn_in_progress();
                let now_pending = !self.projected.pending_requests.is_empty();
                self.caffeinate_guard
                    .sync(self.prefs.keep_awake_while_working && now_running);
                if (!had_pending && now_pending)
                    && self.prefs.system_notifications != crate::prefs::NotificationLevel::Off
                {
                    crate::forge_devices_voice::send_macos_notification(
                        "Kwrok 需要你的确认",
                        "智能体正在等待权限审批或问题回复",
                        self.prefs.notification_sound,
                    );
                } else if was_running && !now_running {
                    let failed = self
                        .projected
                        .turns
                        .last()
                        .is_some_and(|t| t.status == TurnStatus::Failed);
                    if failed
                        && self.prefs.system_notifications != crate::prefs::NotificationLevel::Off
                    {
                        crate::forge_devices_voice::send_macos_notification(
                            "Kwrok 对话执行受阻",
                            "最新一轮智能体任务未能完成，请查看详情",
                            self.prefs.notification_sound,
                        );
                    } else if !failed
                        && self.prefs.system_notifications
                            == crate::prefs::NotificationLevel::All
                    {
                        crate::forge_devices_voice::send_macos_notification(
                            "Kwrok 任务已完成",
                            "智能体已完成本轮回复与改动",
                            self.prefs.notification_sound,
                        );
                    }
                    if !now_pending
                        && self.creating_chat.is_none()
                        && self.access_mode.is_writable()
                        && let Some(queued) = self.queued_prompt.take()
                    {
                        self.dispatch_prompt(queued.text, queued.target, queued.attachments, cx);
                    }
                }
                if switched_chat || workspace_changed {
                    self.sync_workspace_panels(window, cx);
                }
                if self.chat_is_open() && self.projected.info.is_some()
                    && let Some(command) = self.config.initial_terminal_cmd.take()
                {
                    self.run_terminal_command_str(command, window, cx);
                }
                // A newly displayed decision reveals its first field. Subsequent feed
                // updates must preserve the reader's position while they fill it out.
                if self.pending_decisions().next().is_none() {
                    if switched_chat && self.projected.turns.len() <= 1 && !self.turn_in_progress() {
                        self.chat_scroll.set_offset(point(px(0.), px(0.)));
                    } else if switched_chat || (!self.chat_follow_paused
                        && (new_turn_appended || (at_end && !was_running && !now_running))) {
                        self.chat_scroll.scroll_to_bottom();
                    }
                }
            }
            KarBridgeEvent::PromptLifecycle { attempt } => {
                let failed = attempt.last_error.is_some();
                self.planning.observe_attempt(&attempt);
                self.prompt_attempts.insert(attempt.request_id, attempt);
                if failed { self.reconcile_plan_submission(cx); }
            }
            KarBridgeEvent::Hardware {workspace,request_id,result} => self.on_hardware(workspace,request_id,result,window,cx),
            KarBridgeEvent::GoalResult { chat_id, action, result } =>
                self.on_goal_result(chat_id, action, result, window, cx),
            KarBridgeEvent::GoalChatCreated(chat_id) => {
                self.goal_pending.insert(chat_id);
            }
            KarBridgeEvent::ChatCreateFailed {
                request,
                plan_request_id,
                cause,
                mut detail,
            } => {
                if let Some(id) = plan_request_id {
                    self.plan_create_failed(id, window, cx);
                }
                if !self.owns_chat_creation(request) {
                    return;
                }
                self.restore_queued_prompt(window, cx);
                self.creating_chat = None;
                self.config.initial_terminal_cmd = None;
                let (title, action) = match cause {
                    ChatCreateFailure::Store => ("无法保存新对话".into(), NoticeAction::None),
                    ChatCreateFailure::Runtime(id) => (
                        format!("{} 没能启动", runtime_display_name(&id)), NoticeAction::ShowAgents,
                    ),
                    ChatCreateFailure::Worktree => {
                        if !self.git_snapshot.is_repo {
                            detail = "请选择包含 Git 仓库的项目，或在输入框下方切回本地目录。".into();
                        }
                        ("无法创建隔离工作树".into(), NoticeAction::ChooseProject)
                    }
                };
                self.show_notice(
                    Notice {
                        topic: NoticeTopic::Event,
                        tone: NoticeTone::Error,
                        title,
                        detail,
                        action,
                    },
                    cx,
                );
            }
            KarBridgeEvent::HttpServerState {
                listening_url,
                message,
                ..
            } => {
                let was_sharing = self.active_http_server_url.is_some();
                match (&listening_url, was_sharing) {
                    (Some(_), _) => self.notify_toast("已开启双向节点与 6 位短码服务", cx),
                    (None, true) => self.notify_toast("已停止节点共享", cx),
                    (None, false) => {}
                }
                self.active_http_server_url = listening_url;
                if let Some(msg) = message {
                    self.notify_warning(msg, cx);
                }
                self.sync_host_extended_snapshot();
            }
            KarBridgeEvent::BannerNotice {
                title,
                detail,
                is_error,
            } => {
                self.show_notice(
                    Notice {
                        topic: NoticeTopic::Event,
                        tone: if is_error {
                            NoticeTone::Error
                        } else {
                            NoticeTone::Info
                        },
                        title,
                        detail,
                        action: NoticeAction::None,
                    },
                    cx,
                );
            }
            KarBridgeEvent::QueuedInputRetrieved {
                chat_id,
                input_id,
                text,
                attachments,
            } => {
                let location = DraftLocation::Chat(chat_id);
                let target = self.turn_targets.get(&input_id).cloned();
                let text = if target.is_some() {
                    strip_selection_envelope(&text).to_owned()
                } else {
                    text
                };
                self.prepend_composer_draft(
                    location.clone(),
                    QueuedPrompt {
                        text,
                        target,
                        attachments,
                    },
                    window,
                    cx,
                );
                if self.drafts.location == location {
                    self.composer_editor
                        .update(cx, |ed, cx| ed.focus(window, cx));
                }
                self.notify_toast(
                    self.ui_locale
                        .pick("已取回至对话草稿", "Returned to conversation draft"),
                    cx,
                );
            }
            KarBridgeEvent::ToastMessage { text, warning } => {
                if warning {
                    self.notify_warning(text, cx);
                } else {
                    self.notify_toast(text, cx);
                }
            }
            KarBridgeEvent::OfficeMcpCall(call) => {
                self.handle_office_mcp_call(call, window, cx);
            }
            KarBridgeEvent::RemotePairFailed { code_rejected } => {
                self.remote_pair_pending = false;
                let (field, message) = if code_rejected {
                    (
                        &self.remote_connect_code_input,
                        self.ui_locale.pick(
                            "配对码未被接受，请核对主机当前显示的配对码。",
                            "Code not accepted. Check the code on the host.",
                        ),
                    )
                } else {
                    (
                        &self.remote_connect_url_input,
                        self.ui_locale.pick(
                            "无法配对。请检查地址及主机的共享状态。",
                            "Could not pair. Check the address and confirm the host is sharing.",
                        ),
                    )
                };
                field.update(cx, |input, cx| input.set_error(message, cx));
                if !self.settings_open {
                    self.notify_warning(message, cx);
                }
            }
            KarBridgeEvent::RemoteTelemetryUpdated(telem) => {
                if self.settings_open
                    && self.active_settings_tab == SettingsTab::Remote
                    && !self.remote_tab_connect
                    && (self.remote_telemetry.listen_url.is_some() != telem.listen_url.is_some()
                        || self.remote_telemetry.active_ticket.is_some()
                            != telem.active_ticket.is_some())
                {
                    self.focus_handle.focus(window, cx);
                }
                let people: Vec<PresencePerson> = telem
                    .peers
                    .iter()
                    .filter(|p| !p.revoked)
                    .map(|p| {
                        PresencePerson::new(p.peer_id.to_string(), p.device_name.clone())
                            .with_status(PresenceStatus::Active)
                            .with_detail(p.role.label())
                    })
                    .collect();
                if people.is_empty() {
                    self.presence_stack = None;
                } else {
                    self.presence_stack = Some(cx.new(|cx| {
                        PresenceStack::new("kar-remote-presence", people, cx)
                            .with_size(PresenceSize::Small)
                    }));
                }
                self.remote_telemetry = telem;
            }
            KarBridgeEvent::RemoteQueueCopied {
                url,
                peer_id,
                chat_id,
                result,
            } => {
                if !matches!(&self.access_mode, ChatAccessMode::RemoteController { url: current, peer_id: peer, chat_id: Some(chat), role, .. }
                    if current == &url && *peer == peer_id && *chat == chat_id && role.can_mutate())
                {
                    return;
                }
                let loc = self.ui_locale;
                match result {
                    Ok(text) => {
                        let location = DraftLocation::Remote(url, Some(chat_id));
                        self.prepend_composer_draft(location.clone(), QueuedPrompt { text, ..Default::default() }, window, cx);
                        if self.drafts.location == location {
                            self.composer_editor.update(cx, |ed, cx| ed.focus(window, cx));
                        }
                        self.notify_toast(loc.pick("已复制到草稿，原消息仍保留在主机队列中", "Copied to draft. The original stays queued on the host."), cx);
                    }
                    Err(reason) => self.notify_warning(match reason {
                        QueueCopyError::HostAttachments => loc.pick("这条消息含主机附件，请在主机上编辑。原消息仍在队列中。", "Edit this message on the host to keep its attachments. It remains queued."),
                        QueueCopyError::Changed => loc.pick("该消息已离开此对话的待发队列，草稿已保留", "This message is no longer pending in this thread. Your draft is unchanged."),
                        QueueCopyError::Unavailable => loc.pick("无法确认主机队列，草稿已保留", "Could not verify the host queue. Your draft is unchanged."),
                    }, cx),
                }
            }
            KarBridgeEvent::RemoteExtendedStateUpdated {
                url,
                peer_id,
                snapshot: ext,
            } => {
                if matches!(&self.access_mode, ChatAccessMode::RemoteController { url: current, chat_id, peer_id: peer, .. } if current == &url && *peer == peer_id && *chat_id == ext.chat_id)
                {
                    self.projected.queued_inputs = ext.queued_inputs.unwrap_or_default();
                    self.active_briefs = ext.briefs;
                    for (tid, cap) in ext.turn_captures {
                        self.client_reducer.record_turn_capture(cap);
                        let _ = tid;
                    }
                    if let (Some(cid), Some(assoc)) = (ext.chat_id, ext.association) {
                        self.client_reducer.bind_association(cid, assoc);
                    }
                }
            }
            KarBridgeEvent::WorkflowsLoaded { workspace, result, saved, run_task, report_errors } => {
                if workspace != self.effective_workspace_dir() { return; }
                self.ensure_schedule_workspace();
                if saved { self.schedule_save_pending = false; }
                if let Some(task)=run_task {
                    if result.is_ok() { self.schedule_pending_runs.remove(&task); }
                    else if let Some((_,in_flight))=self.schedule_pending_runs.get_mut(&task) { *in_flight=false; }
                }
                self.schedule_error=result.as_ref().err().cloned();
                match result {
                    Ok(registry) => {
                        for task in &registry.tasks {
                            for receipt in &task.history {
                                if let Some(chat) = receipt.chat_id && self.client_reducer.associations.get(&chat).is_none() {
                                    self.client_reducer.bind_association(chat, task.association());
                                }
                            }
                        }
                        self.schedule_loaded = true;
                        self.schedule_registry = registry;
                        if saved {
                            self.schedule_title_input.update(cx, |input,cx| input.set_value("", window, cx));
                            self.schedule_prompt_input.update(cx, |input,cx| input.set_value("", window, cx));
                            self.editing_schedule_id = None;
                            self.schedule_creator_open = false;
                            self.notify_toast("任务已保存到工作台", cx);
                        }
                    }
                    Err(error) => {
                        if report_errors { self.notify_warning(error, cx); }
                        else { log::warn!("Cannot refresh Kar workflows: {error}"); }
                    }
                }
                cx.notify();
            }
            KarBridgeEvent::AutoBackgroundedCommand(bg) => {
                let summary = bg.command_summary.clone();
                self.client_reducer.register_background_command(bg);
                self.notify_toast(
                    format!("命令 `{summary}` 已超 30s，已自动转入后台跟踪，新补充立即送达"),
                    cx,
                );
            }
            KarBridgeEvent::GitCommitFinished { workspace, message, success } => {
                self.committing_workspaces.remove(&workspace);
                if success && self.git_drafts.get(&workspace).is_some_and(|draft| draft.trim() == message) {
                    self.git_drafts.remove(&workspace);
                }
                if success && workspace == self.effective_workspace_dir()
                    && self.git_commit_input.read(cx).value(cx).trim() == message {
                    self.git_commit_input.update(cx, |input, cx| input.set_value("", window, cx));
                }
            }
            KarBridgeEvent::GitWorkspaceUpdated { workspace, snapshot } => {
                if workspace != self.effective_workspace_dir() { return; }
                let refresh_tree = self.workspace_tree_view.is_none() || self.git_snapshot != snapshot;
                self.git_snapshot = snapshot;
                self.inline_git_diffs.clear();
                self.patch_review_cache.get_mut()
                    .retain(|k, _| !k.starts_with("inline-diff-"));
                if let Some((ref exp_rel, staged)) = self.expanded_git_diff_file {
                    if self.git_snapshot.files.iter().any(|f| &f.path == exp_rel && if staged { f.is_staged() } else { f.has_unstaged() }) {
                        self.bridge.send(KarCommand::LoadGitFileDiff {
                            staged,
                            workspace: self.effective_workspace_dir(),
                            rel_path: exp_rel.clone(),
                            is_demo: self.git_snapshot.is_demo,
                        });
                    } else {
                        self.expanded_git_diff_file = None;
                    }
                }
                self.git_branch_graph = None;
                if refresh_tree {
                    self.bridge.send(KarCommand::RefreshWorkspaceTree { workspace, show_hidden: self.tree_show_hidden });
                }
            }
            KarBridgeEvent::GitFileDiffLoaded { workspace, rel_path, staged, hunks } => {
                if workspace != self.panel_workspace { return; }
                let cache_key = format!("inline-diff-{staged}-{}", rel_path.display());
                self.patch_review_cache.get_mut().remove(&cache_key);
                self.inline_git_diffs.insert((rel_path, staged), hunks);
            }
            KarBridgeEvent::WorkspaceTreeUpdated { workspace, show_hidden, tree } => {
                if workspace != self.effective_workspace_dir() || show_hidden != self.tree_show_hidden { return; }
                if self.workspace_tree_view.is_none() || self.workspace_tree != tree {
                    self.workspace_tree = tree;
                    self.rebuild_workspace_tree_view(window, cx);
                }
            }
            KarBridgeEvent::WorktreeCreated { chat_id, meta } => {
                let branch = meta.branch.clone();
                self.project_registry.ensure_project(&meta.repo_root);
                self.project_registry.chat_worktrees.insert(chat_id, meta);
                let _ = self.project_registry.save(&self.config.chat_dir);
                self.sync_workspace_panels(window, cx);
                self.notify_toast(format!("已创建隔离 Worktree `{branch}`"), cx);
            }
            KarBridgeEvent::TerminalOutputChunk { id, chunk } => {
                if let Some(run) = self.terminal_runs.iter_mut().find(|r| r.id == id) {
                    const MAX_TERM_CHARS: usize = 48_000;
                    run.output.push_str(&chunk);
                    if run.output.len() > MAX_TERM_CHARS {
                        let drop_to = run.output.len() - 36_000;
                        let boundary = run
                            .output
                            .char_indices()
                            .find(|(idx, _)| *idx >= drop_to)
                            .map(|(idx, _)| idx)
                            .unwrap_or(0);
                        run.output = format!("…(前序输出已截断)\n{}", &run.output[boundary..]);
                    }
                }
            }
            KarBridgeEvent::TerminalCommandFinished { id, exit_code } => {
                if let Some(run) = self.terminal_runs.iter_mut().find(|r| r.id == id) {
                    run.running = false;
                    run.exit_code = Some(exit_code);
                }
                self.bridge.send(KarCommand::RefreshGitWorkspace {
                    workspace: self.effective_workspace_dir(),
                });
            }
            KarBridgeEvent::WorkspaceFileLoaded { workspace, preview } => {
                if workspace != self.panel_workspace { return; }
                let rel = preview.rel_path.clone();
                let key = rel.to_string_lossy().into_owned();
                self.code_block_cache.remove(&key);
                self.patch_review_cache.get_mut().remove(&format!("tab-patch-{key}"));
                if !self.code_tab_order.contains(&rel) {
                    self.code_tab_order.push(rel.clone());
                }
                self.open_code_tabs.insert(rel.clone(), preview);
                self.right_surface = RightPaneSurface::CodeFile(rel);
                if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
                    self.set_layout_mode(WorkspaceLayoutMode::Split, cx);
                }
            }
            KarBridgeEvent::WorkspaceInventoryUpdated(inventory) => {
                if inventory.belongs_to(&self.effective_workspace_dir(), self.active_or_selected_runtime_id()) {
                    self.code_block_cache.retain(|key, _| !key.starts_with("skill-md-"));
                    self.tool_json_viewers.get_mut().retain(|key, _| !key.starts_with("tool-schema-"));
                    self.workspace_inventory = inventory;
                }
            }
            KarBridgeEvent::GlobalSearchResults { workspace, query, hits } => {
                if workspace == self.panel_workspace && self.cmd_query.trim() == query.trim() {
                    self.global_search_hits = hits;
                }
            }
            KarBridgeEvent::UsageAnalyticsUpdated(analytics) => {
                self.usage_analytics = analytics;
            }
            KarBridgeEvent::SideQuestionCapability{chat,native,supported}=>{
                if self.current_chat_id() == Some(chat) {
                    self.side_question_capability=supported.then_some((chat,native));
                    if supported && self.config.open_side_question {self.config.open_side_question=false; self.side_question_open=true;}
                }
            }
            KarBridgeEvent::SideQuestionAnswered{ request_id,question,result }=>{
                if self.side_question_request!=Some(request_id){return;}
                self.side_question_request=None;
                self.side_question_pending=false;
                match result {
                    Ok(answer)=>self.side_question_last_reply=Some((question,answer)),
                    Err(message)=>self.notify_warning(format!("旁路提问未完成：{message}"),cx),
                }
            }
            KarBridgeEvent::HandoffCompleted {
                target_runtime,
                summary_message,
                chat_id,
            } => {
                if chat_id.is_some() && chat_id != self.current_chat_id() { return; }
                if chat_id.is_none() { self.selected_runtime = target_runtime.clone(); }
                self.handoff_note_input
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.diag_logs.push(
                    DiagLevel::Info,
                    "handoff",
                    Some("DONE"),
                    format!(
                        "会话已平滑接力至 {}（携带摘要 {} 字节）",
                        runtime_display_name(&target_runtime),
                        summary_message.len()
                    ),
                    Vec::new(),
                );
                self.notify_toast(
                    format!("已切换并接力至 {}", runtime_display_name(&target_runtime)),
                    cx,
                );
            }
            KarBridgeEvent::DiagLogAppended(rec) => {
                self.diag_logs.push(
                    rec.level,
                    rec.source,
                    rec.tag.as_deref(),
                    rec.message,
                    rec.fields,
                );
            }
            KarBridgeEvent::PromptSuggestionUpdated {
                chat_id,
                suggestion,
            } => {
                if self.current_chat_id() == Some(chat_id) {
                    self.live_prompt_suggestion =
                        self.prefs.prompt_suggestions_enabled.then_some(suggestion).flatten();
                }
            }
            KarBridgeEvent::LoginStarted { attempt, runtime_id } => {
                self.provider_sign_in.start(attempt, runtime_id);
            }
            KarBridgeEvent::LoginInteractionPrompt { attempt, auth_url, device_code, message } => {
                self.provider_sign_in.prompt(attempt, auth_url, device_code, message);
            }
            KarBridgeEvent::LoginFinished { attempt } => self.provider_sign_in.finish(attempt),
            KarBridgeEvent::AgentUpdateStatus {
                runtime_id,
                summary,
                update_available,
            } => {
                self.agent_update_statuses
                    .insert(runtime_id, (summary, update_available));
            }
            KarBridgeEvent::ForgeMutationFinished{workspace,chat,operation,created,result} => {
                if self.forge_pending==Some(operation){self.forge_pending=None;}
                match result {
                    Ok(message) => {
                        self.forge_last_operation=None;
                        if self.effective_workspace_dir()==workspace {
                            if created && self.current_chat_id()==Some(chat) {self.forge_create_pr_open=false; self.forge_draft_epoch=self.forge_draft_epoch.wrapping_add(1);}
                            self.bridge.send(KarCommand::RefreshForgeData{workspace:workspace.clone()});
                        }
                        if self.current_chat_id()==Some(chat) && self.effective_workspace_dir()==workspace { self.notify_toast(message,cx); }
                    }
                    Err(error)=>if self.current_chat_id()==Some(chat) && self.effective_workspace_dir()==workspace {self.notify_warning(error,cx)},
                }
            }
            KarBridgeEvent::ForgeDataUpdated {
                workspace,
                prs,
                issues,
                error,
            } => {
                if self.effective_workspace_dir()!=workspace{return;}
                self.forge_workspace=Some(workspace);
                self.forge_prs = prs;
                self.forge_issues = issues;
                self.forge_error = error;
            }
            KarBridgeEvent::CommitMessageGenerated { workspace, result } => {
                let Some(original) = self.generating_commits.remove(&workspace) else { return; };
                match result {
                    Ok(message) if workspace == self.panel_workspace => {
                        if self.git_commit_input.read(cx).value(cx) == original {
                            self.git_commit_input.update(cx, |inp, cx| inp.set_value(message, window, cx));
                        }
                    }
                    Ok(message) => {
                        if self.git_drafts.get(&workspace).map(String::as_str).unwrap_or_default() == original {
                            self.git_drafts.insert(workspace, message);
                        }
                    }
                    Err(error) => self.notify_warning(format!("生成提交说明失败：{error}"), cx),
                }
            }
            KarBridgeEvent::GitRepoCloned { repo_path } => {
                let _ = self.project_registry.ensure_project(&repo_path);
                let _ = self.project_registry.save(&self.config.chat_dir);
                self.switch_workspace_dir(repo_path, window, cx);
            }
            KarBridgeEvent::DesktopUpdateChecked(report) => {
                self.notify_toast(report.status_message.clone(), cx);
                self.desktop_update_report = Some(report);
            }
        }
        cx.notify();
    }

    fn sync_timeline_entities(&mut self, projected: &ProjectedChatState, cx: &mut Context<Self>) {
        let turns = &projected.turns;
        let root = projected.info.as_ref().map(|info| info.native_id.as_str());
        for turn in turns {
            if turn.status == TurnStatus::Running {
                self.copy_buttons.remove(&turn.id);
                continue;
            }
            let steps = crate::ui_chat::turn_steps::TurnSteps::new(turn, root);
            let reply = reply_markdown(turn.items.iter().enumerate().filter_map(|(index, item)| match &item.item {
                Item::Message { text, .. } if steps.is_answer(index) => Some(text.as_str()),
                _ => None,
            }));

            if !reply.is_empty() {
                if let Some(button) = self.copy_buttons.get(&turn.id) {
                    button.update(cx, |b, _| b.set_text(reply));
                } else {
                    let id = format!("copy-{}", turn.id);
                    let button = cx.new(|_| {
                        CopyButton::new(id, reply, self.ui_locale.pick("复制本轮回复", "Copy entire response")).toolbar()
                    });
                    self.copy_buttons.insert(turn.id, button);
                }
            } else {
                self.copy_buttons.remove(&turn.id);
            }
        }

        let on_screen: BTreeSet<Uuid> = turns.iter().map(|t| t.id).collect();
        self.copy_buttons.retain(|id, _| on_screen.contains(id));
        let chat = projected.chat_id;
        self.rollback_hold_buttons.get_mut().retain(|(owner, turn, _), _| Some(*owner) == chat && on_screen.contains(turn));
        self.part_open.retain(|(id, _), _| on_screen.contains(id));
        if self.tool_detail.is_some_and(|(id, _)| !on_screen.contains(&id)) {
            self.tool_detail = None;
        }
    }

    pub(crate) fn refresh_agent_notice(&mut self, cx: &mut Context<Self>) {
        if matches!(self.access_mode, ChatAccessMode::NoneSelected)
            && let Some(agent) = self.providers.iter().find(|p| p.id == self.selected_runtime)
            && let Some(reason) = agent.diagnostic_reason()
        {
            let notice = Notice {
                topic: NoticeTopic::Agent,
                tone: NoticeTone::Warning,
                title: format!("{} {}", agent.display_name(), agent.status_label()),
                detail: reason,
                action: NoticeAction::ShowAgents,
            };
            let busy_with_error = self.notice_open
                && self
                    .notice
                    .as_ref()
                    .is_some_and(|n| n.topic != NoticeTopic::Agent);
            if !busy_with_error && (self.notice.as_ref() != Some(&notice) || !self.notice_open) {
                self.show_notice(notice, cx);
            }
            return;
        }
        self.clear_notice(NoticeTopic::Agent, cx);
    }

    // ── Chats ───────────────────────────────────────────────────────────────

    fn write_chat_preference<T: serde::Serialize>(
        &mut self,
        key: &str,
        value: T,
        cx: &mut Context<Self>,
    ) {
        let result = serde_json::to_string(&value)
            .map_err(std::io::Error::other)
            .and_then(|value| {
                crate::client_store::write_desktop_preference(&self.config.chat_dir, key, &value)
            });
        if let Err(error) = result {
            self.notify_warning(error.to_string(), cx);
        }
    }

    pub(crate) fn save_agent_choice(&mut self, cx: &mut Context<Self>) {
        self.write_chat_preference(
            "agent-choice",
            AgentChoice {
                runtime: self.selected_runtime.clone(),
                model: self.selected_model.clone(),
                effort: self.selected_effort().map(str::to_owned),
            },
            cx,
        );
    }

    pub(crate) fn effective_chat_approvals(&self) -> ApprovalMode {
        self.projected
            .info
            .as_ref()
            .filter(|_| self.chat_is_open())
            .map(|i| i.approvals)
            .unwrap_or(self.approval_mode)
    }

    pub(crate) fn set_new_chat_approvals(&mut self, mode: ApprovalMode, cx: &mut Context<Self>) {
        self.approval_mode = mode;
        self.write_chat_preference("new-chat-approvals", mode, cx);
        cx.notify();
    }

    pub(crate) fn set_active_or_new_chat_approvals(
        &mut self,
        mode: ApprovalMode,
        cx: &mut Context<Self>,
    ) {
        if self.chat_is_open()
            && self.access_mode.is_writable()
            && let Some(chat_id) = self.current_chat_id()
        {
            if let Some(ref mut info) = self.projected.info {
                info.approvals = mode;
            }
            self.bridge
                .send(KarCommand::SetChatApprovalMode { chat_id, mode });
            let label = self.ui_locale.approval_label(mode);
            self.notify_toast(
                format!(
                    "{}「{label}」",
                    self.ui_locale
                        .pick("当前对话权限已切换为", "Thread permission set to ")
                ),
                cx,
            );
            cx.notify();
        } else {
            self.set_new_chat_approvals(mode, cx);
        }
    }

    pub(crate) fn set_new_chat_worktree(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.new_chat_use_worktree = enabled;
        self.write_chat_preference("new-chat-worktree", enabled, cx);
        cx.notify();
    }

    /// Choose the agent new chats start with.
    pub fn select_runtime_by_id(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.selected_runtime != id {
            self.aster_planning.draft = false;
            self.selected_runtime = id.to_owned();
            if matches!(self.access_mode, ChatAccessMode::NoneSelected) {
                self.sync_workspace_inventory(&self.effective_workspace_dir());
            }
            if let Some(sel_model) = &self.selected_model {
                let valid = self
                    .providers
                    .iter()
                    .find(|p| p.id == id)
                    .is_some_and(|p| p.models.iter().any(|m| &m.id == sel_model));
                if !valid && !self.prefs.has_custom_model(id, sel_model) {
                    self.selected_model = None;
                }
            }
        }
        self.open_menu = None;
        self.refresh_agent_notice(cx);
        cx.notify();
    }

    pub(crate) fn offered_efforts(&self) -> &[String] {
        self.providers
            .iter()
            .find(|p| p.id == self.selected_runtime)
            .map_or(&[], |p| p.effort_levels(self.selected_model.as_deref()))
    }

    pub(crate) fn selected_effort(&self) -> Option<&str> {
        self.reasoning_effort
            .as_deref()
            .filter(|level| self.offered_efforts().iter().any(|item| item == level))
    }

    pub(crate) fn effective_runtime(&self) -> String {
        match (&self.access_mode, self.projected.info.as_ref()) {
            (ChatAccessMode::NoneSelected | ChatAccessMode::DemoShowcase, _) | (_, None) => {
                self.selected_runtime.clone()
            }
            (_, Some(info)) => info.runtime.clone(),
        }
    }

    pub(crate) fn chat_is_open(&self) -> bool {
        !matches!(
            self.access_mode,
            ChatAccessMode::NoneSelected | ChatAccessMode::DemoShowcase
        )
    }

    /// Work in this window can outlive the selected conversation or the visible panel.
    pub(crate) fn running_thread_ids(&self) -> BTreeSet<Uuid> {
        let mut running: BTreeSet<_> = self.chat_list.iter()
            .filter(|entry| entry.last_status == Some(TurnStatus::Running))
            .map(|entry| entry.info.id).collect();
        running.extend(self.prompt_attempts.values()
            .filter(|attempt| attempt.in_flight).filter_map(|attempt| attempt.chat_id));
        running
    }

    pub(crate) fn unread_completion(&self, entry: &ChatListEntry) -> Option<u64> {
        let at = entry.last_activity_ms?;
        let visited = self.sidebar_metadata.get(&entry.info.id).and_then(|meta| meta.visited_ms);
        (entry.last_status == Some(TurnStatus::Completed)
            && self.read_receipts.unread(entry.info.id, at, visited)).then_some(at)
    }

    fn mark_visible_reply_read(&mut self, window_active: bool) {
        let Some(id) = self.current_chat_id() else { return };
        if self.projected.chat_id != Some(id) {
            return;
        }
        let Some(turn) = self.projected.turns.last().filter(|turn| turn.status == TurnStatus::Completed)
        else { return };
        let at = turn.finished_ms.unwrap_or(turn.started_ms);
        // An explicit open acknowledges only the reply the reader chose. A later background
        // completion still needs attention, even if this chat is selected in an inactive window.
        let opened = self.opened_unread_reply == Some((id, at));
        if !window_active && !opened { return; }
        if opened { self.opened_unread_reply = None; }
        let visited = self.sidebar_metadata.get(&id).and_then(|meta| meta.visited_ms);
        if !self.read_receipts.unread(id, at, visited) { return; }
        self.read_receipts.acknowledge(id, at);
        if self.sidebar_metadata.contains_key(&id) {
            self.bridge.send(KarCommand::UpdateSidebar {
                chat_id: id, action: crate::kar_bridge::SidebarAction::Visit(at),
            });
        }
    }

    pub(crate) fn has_running_task(&self) -> bool {
        self.turn_in_progress()
            || self.prompt_attempts.values().any(|attempt| attempt.in_flight)
            || self.schedule_registry.tasks.iter().any(|t| t.history.iter().any(|r| r.outcome == ScheduleRunOutcome::Admitted))
            || self.terminal_runs.iter().any(|run| run.running)
    }

    /// Scoped strictly to the currently open chat so switching chats or pressing `⌘N`
    /// is never blocked by an in-flight turn on another chat.
    pub(crate) fn turn_in_progress(&self) -> bool {
        if matches!(self.access_mode, ChatAccessMode::NoneSelected) {
            return false;
        }
        let cur_cid = self.current_chat_id();
        self.projected
            .turns
            .last()
            .is_some_and(|t| t.status == TurnStatus::Running)
            || self.prompt_attempts.values().any(|a| {
                a.in_flight
                    && match (cur_cid, a.chat_id) {
                        (Some(c), Some(ac)) => c == ac,
                        (None, None) => matches!(self.access_mode, ChatAccessMode::DemoShowcase),
                        _ => false,
                    }
            })
    }

    pub(crate) fn current_chat_id(&self) -> Option<Uuid> {
        chat_id_of(&self.access_mode).or_else(|| match self.access_mode {
            ChatAccessMode::NoneSelected | ChatAccessMode::DemoShowcase => None,
            _ => self.projected.info.as_ref().map(|i| i.id),
        })
    }

    fn current_chat_info(&self) -> Option<&kar::chat::ChatInfo> {
        let id = self.current_chat_id()?;
        self.projected.info.as_ref().filter(|info| info.id == id).or_else(|| {
            self.chat_list.iter().find(|entry| entry.info.id == id).map(|entry| &entry.info)
        })
    }

    /// Active workspace directory: returns the chat's isolated Git Worktree (or chat workspace)
    /// when a local chat is open, otherwise `self.config.workspace_dir`.
    pub fn effective_workspace_dir(&self) -> PathBuf {
        if let Some(cid) = self.current_chat_id()
            && let Some(wt) = self.project_registry.worktree_for(cid)
            && wt.worktree_path.is_dir()
        {
            return wt.worktree_path.clone();
        }
        if let Some(info) = self.current_chat_info()
            && info.workspace.is_dir()
        {
            return info.workspace.clone();
        }
        self.config.workspace_dir.clone()
    }

    fn sync_workspace_inventory(&mut self, workspace: &Path) {
        let runtime = self.active_or_selected_runtime_id().to_owned();
        if !self.workspace_inventory.belongs_to(workspace, &runtime) {
            self.workspace_inventory = crate::studio_hub::seed_local_workspace_inventory(workspace, &runtime);
            self.bridge.send(KarCommand::ProbeWorkspaceInventory {
                workspace: workspace.to_owned(),
                runtime_id: runtime,
            });
        }
    }

    fn sync_workspace_panels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.git_staged_limit = 32;
        self.git_unstaged_limit = 32;
        let workspace = self.effective_workspace_dir();
        self.sync_workspace_inventory(&workspace);
        if workspace == self.panel_workspace {
            return;
        }

        let previous = std::mem::replace(&mut self.panel_workspace, workspace.clone());
        let draft = self.git_commit_input.read(cx).value(cx).to_string();
        if !draft.is_empty() {
            self.git_drafts.insert(previous, draft);
        }
        let next_draft = self.git_drafts.remove(&workspace).unwrap_or_default();
        self.git_commit_input
            .update(cx, |input, cx| input.set_value(next_draft, window, cx));

        self.git_snapshot = GitWorkspaceSnapshot::default();
        self.git_branch_graph = None;
        self.expanded_git_diff_file = None;
        self.inline_git_diffs.clear();
        self.workspace_tree.clear();
        self.workspace_tree_view = None;
        self.open_code_tabs.clear();
        self.code_tab_order.clear();
        self.code_block_cache.clear();
        self.patch_review_cache.get_mut()
            .retain(|key, _| key.starts_with("turn-patch-"));
        if matches!(self.right_surface, RightPaneSurface::CodeFile(_)) {
            self.files_tab_open = true;
        self.right_surface = RightPaneSurface::Files;
        }
        self.global_search_hits.clear();
        self.bridge.send(KarCommand::RefreshGitWorkspace {
            workspace: workspace.clone(),
        });
        if self.tree_show_hidden {
            self.bridge.send(KarCommand::RefreshWorkspaceTree {
                workspace: workspace.clone(),
                show_hidden: true,
            });
        }
    }

    fn record_location(&mut self, location: WorkspaceLocation) {
        if self.navigation.get(self.navigation_index) == Some(&location) {
            return;
        }
        self.navigation.truncate(self.navigation_index + 1);
        if self.navigation.len() >= 128 {
            self.navigation.remove(0);
        }
        self.navigation.push(location);
        self.navigation_index = self.navigation.len() - 1;
    }

    fn next_history_index(&self, forward: bool) -> Option<usize> {
        let reachable = |index: &usize| match &self.navigation[*index] {
            WorkspaceLocation::Live(recorded) => matches!(&self.access_mode,
                ChatAccessMode::HttpReadOnly { url, .. } | ChatAccessMode::RemoteController { url, .. } if recorded == url),
            _ => true,
        };
        if forward {
            (self.navigation_index + 1..self.navigation.len()).find(reachable)
        } else {
            (0..self.navigation_index).rev().find(reachable)
        }
    }

    pub(crate) fn can_navigate(&self, forward: bool) -> bool {
        self.next_history_index(forward).is_some()
    }

    pub(crate) fn navigate_history(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.next_history_index(forward) else {
            return;
        };
        self.navigation_index = index;
        match self.navigation[self.navigation_index].clone() {
            WorkspaceLocation::Draft(workspace) => {
                self.select_project_and_new_chat(workspace, window, cx)
            }
            WorkspaceLocation::Thread(id) => self.open_chat(id, window, cx),
            WorkspaceLocation::Live(_) => {
                self.dismiss_compact_sidebar(cx);
                self.close_all_overlays(cx);
                self.main_stage_view = MainStageView::Conversation;
                cx.notify();
            }
            WorkspaceLocation::Inbox => self.open_tasks_inbox_stage(cx),
            WorkspaceLocation::Documents => self.open_office_gallery_stage(cx),
            WorkspaceLocation::Demo => self.open_sample_chat(window, cx),
        }
    }

    pub fn open_tasks_inbox_stage(&mut self, cx: &mut Context<Self>) {
        self.dismiss_compact_sidebar(cx);
        self.record_location(WorkspaceLocation::Inbox);
        self.close_all_overlays(cx);
        self.main_stage_view = MainStageView::TasksInbox;
        cx.notify();
    }

    pub fn open_office_gallery_stage(&mut self, cx: &mut Context<Self>) {
        self.dismiss_compact_sidebar(cx);
        self.record_location(WorkspaceLocation::Documents);
        self.close_all_overlays(cx);
        self.main_stage_view = MainStageView::OfficeGallery;
        if self.workspace_tree.is_empty() {
            self.bridge.send(KarCommand::RefreshWorkspaceTree {
                workspace: self.effective_workspace_dir(),
                show_hidden: self.tree_show_hidden,
            });
        }
        cx.notify();
    }

    pub fn start_new_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.aster_planning = Default::default();
        self.dismiss_compact_sidebar(cx);
        self.record_location(WorkspaceLocation::Draft(self.config.workspace_dir.clone()));
        self.config.resume_last_chat = false;
        self.config.open_latest_chat = false;
        self.close_all_overlays(cx);
        self.main_stage_view = MainStageView::Conversation;
        self.coedit_panel_open = false;
        self.restore_queued_prompt(window, cx);
        self.active_briefs.clear();
        if self.chat_selection_pending() || !matches!(self.access_mode, ChatAccessMode::NoneSelected) {
            self.request_navigation(LocalChatDestination::Draft, cx);
        }
        self.access_mode = ChatAccessMode::NoneSelected;
        self.projected = ProjectedChatState::default();
        self.switch_composer_draft(window, cx);
        self.sync_workspace_panels(window, cx);
        self.composer_editor
            .update(cx, |ed, cx| ed.focus(window, cx));
        cx.notify();
    }

    fn owns_chat_creation(&self, request: Uuid) -> bool {
        self.creating_chat.as_ref().is_some_and(|creation| creation.request == request)
    }

    fn creating_chat_goal(&self) -> bool {
        self.creating_chat.as_ref().is_some_and(|creation| creation.goal_draft.is_some())
    }

    fn chat_selection_pending(&self) -> bool {
        self.creating_chat.is_some() || self.pending_navigation.is_some()
    }

    fn request_navigation(&mut self, destination: LocalChatDestination, cx: &mut Context<Self>) {
        let request = Uuid::new_v4();
        self.creating_chat = None;
        self.pending_navigation = Some(PendingNavigation {
            request,
            destination,
            started: false,
        });
        self.composer_editor.update(cx, |editor, cx| {
            editor.disabled = destination != LocalChatDestination::Draft;
            cx.notify();
        });
        self.bridge.send(KarCommand::Navigate { request, destination });
    }

    pub fn open_chat(&mut self, chat_id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        self.opened_unread_reply = self.chat_list.iter().find(|entry| entry.info.id == chat_id)
            .and_then(|entry| self.unread_completion(entry)).map(|at| (chat_id, at));
        self.dismiss_compact_sidebar(cx);
        self.config.resume_last_chat = false;
        self.config.open_latest_chat = false;
        self.main_stage_view = MainStageView::Conversation;
        self.coedit_panel_open = false;
        // A failed resume leaves the archive selected. Selecting it again must retry
        // the connection, even though its chat ID is already on screen.
        let already_open = matches!(
            &self.access_mode,
            ChatAccessMode::WriteController { chat_id: active }
            | ChatAccessMode::LocalReadOnlySubscriber { chat_id: active, .. } if *active == chat_id
        );
        if already_open && !self.chat_selection_pending() {
            self.record_location(WorkspaceLocation::Thread(chat_id));
            self.close_all_overlays(cx);
            cx.notify();
            return;
        }
        self.restore_queued_prompt(window, cx);
        self.request_navigation(LocalChatDestination::Thread(chat_id), cx);
        cx.notify();
    }

    pub fn toggle_pin_chat(&mut self, chat_id: Uuid, cx: &mut Context<Self>) {
        let pinned = !self.project_registry.is_pinned(chat_id);
        self.bridge.send(KarCommand::UpdateSidebar { chat_id, action: crate::kar_bridge::SidebarAction::Pin(pinned) });
        cx.notify();
    }

    pub fn toggle_project_folder_collapsed(&mut self, proj_id: &str, cx: &mut Context<Self>) {
        self.project_registry.toggle_collapsed(proj_id);
        let _ = self.project_registry.save(&self.config.chat_dir);
        cx.notify();
    }

    pub fn switch_workspace_dir(
        &mut self,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let proj = self.project_registry.ensure_project(&dir);
        let _ = self.project_registry.save(&self.config.chat_dir);
        self.config.workspace_dir = proj.root_path.clone();
        self.switch_composer_draft(window, cx);
        self.sync_workspace_panels(window, cx);
        cx.notify();
    }

    pub fn select_project_and_new_chat(
        &mut self,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.restore_queued_prompt(window, cx);
        self.switch_workspace_dir(dir, window, cx);
        self.start_new_chat(window, cx);
    }

    pub fn add_project_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pick_workspace(window, cx);
    }

    pub fn open_sample_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.restore_queued_prompt(window, cx);
        self.dismiss_compact_sidebar(cx);
        self.record_location(WorkspaceLocation::Demo);
        self.main_stage_view = MainStageView::Conversation;
        self.coedit_panel_open = false;
        if self.chat_selection_pending() || !matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            self.request_navigation(LocalChatDestination::Demo, cx);
        }
        cx.notify();
    }

    fn switch_composer_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = DraftLocation::for_mode(&self.access_mode, &self.config.workspace_dir);
        if next != self.drafts.location && self.active_voice_rec.is_some() {
            self.finish_dictation(window, cx);
        }
        let Some(next) = self.drafts.switch(next, || QueuedPrompt {
            text: self.composer_editor.read(cx).value().to_owned(),
            target: self.pinned_target.take(),
            attachments: std::mem::take(&mut self.pending_attachments),
        }) else { return; };
        self.composer_editor.update(cx, |ed, cx| ed.set_value(next.text, window, cx));
        self.pinned_target = next.target;
        self.pending_attachments = next.attachments;
    }

    fn restore_queued_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((workspace, text)) = self.creating_chat.as_mut().and_then(|c| c.goal_draft.take()) {
            self.prepend_composer_draft(
                DraftLocation::Project(workspace),
                QueuedPrompt { text, ..Default::default() },
                window,
                cx,
            );
        }
        if let Some(queued) = self.queued_prompt.take() {
            self.prepend_composer_draft(self.drafts.location.clone(), queued, window, cx);
        }
    }

    fn prepend_composer_draft(
        &mut self,
        location: DraftLocation,
        queued: QueuedPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.drafts.location != location {
            self.drafts
                .saved
                .entry(location)
                .or_default()
                .prepend(queued);
            return;
        }
        let mut draft = QueuedPrompt {
            text: self.composer_editor.read(cx).value().to_owned(),
            target: self.pinned_target.take(),
            attachments: std::mem::take(&mut self.pending_attachments),
        };
        draft.prepend(queued);
        self.composer_editor
            .update(cx, |ed, cx| ed.set_value(draft.text, window, cx));
        self.pinned_target = draft.target;
        self.pending_attachments = draft.attachments;
    }

    pub fn create_new_real_chat(&mut self, cx: &mut Context<Self>) -> bool {
        self.create_real_chat_with_goal(None, cx)
    }

    fn create_real_chat_with_goal(&mut self, initial_goal: Option<crate::kar_bridge::GoalEdit>, cx: &mut Context<Self>) -> bool {
        self.create_chat_for_composer(initial_goal, None, cx)
    }

    fn create_chat_for_composer(&mut self, initial_goal: Option<crate::kar_bridge::GoalEdit>, initial_plan: Option<(Uuid, String)>, cx: &mut Context<Self>) -> bool {
        if self.creating_chat.is_some() || self.opening_existing_conversation() {
            return false;
        }
        self.open_menu = None;
        let agent = self
            .providers
            .iter()
            .find(|p| p.id == self.selected_runtime);
        if agent.is_some_and(|a| a.diagnostic_reason().is_some()) {
            self.notice_open = false;
            self.refresh_agent_notice(cx);
            return false;
        }
        let program = agent.and_then(|p| match &p.availability {
            ProviderAvailability::Available { command, .. } => command.clone(),
            _ => None,
        });
        self.project_registry
            .ensure_project(&self.config.workspace_dir);
        let _ = self.project_registry.save(&self.config.chat_dir);
        let request = Uuid::new_v4();
        self.creating_chat = Some(PendingChatCreation {
            request,
            goal_draft: initial_goal.as_ref().map(|goal| {
                (self.config.workspace_dir.clone(), goal.command())
            }),
        });
        self.bridge.send(KarCommand::CreateChat {
            request,
            aster_plan: crate::aster::is_aster(&self.selected_runtime) && self.aster_planning.draft,
            initial_goal,
            initial_plan,
            runtime_id: self.selected_runtime.clone(),
            model: self.selected_model.clone(),
            program,
            workspace: self.config.workspace_dir.clone(),
            approval_mode: self.approval_mode,
            effort: self.selected_effort().map(str::to_owned),
            use_worktree: self.new_chat_use_worktree,
        });
        true
    }

    pub(crate) fn reveal_chat_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.main_stage_view != MainStageView::Conversation {
            let location = match &self.access_mode {
                ChatAccessMode::NoneSelected => {
                    WorkspaceLocation::Draft(self.effective_workspace_dir())
                }
                ChatAccessMode::DemoShowcase => WorkspaceLocation::Demo,
                ChatAccessMode::HttpReadOnly { url, .. }
                | ChatAccessMode::RemoteController { url, .. } => {
                    WorkspaceLocation::Live(url.clone())
                }
                ChatAccessMode::WriteController { chat_id }
                | ChatAccessMode::LocalReadOnlySubscriber { chat_id, .. }
                | ChatAccessMode::ArchivedReadOnly { chat_id, .. } => {
                    WorkspaceLocation::Thread(*chat_id)
                }
            };
            self.record_location(location);
            self.main_stage_view = MainStageView::Conversation;
        }
        self.close_all_overlays(cx);
        if f32::from(window.viewport_size().width) < COMPACT_WORKSPACE_WIDTH {
            self.dismiss_compact_sidebar(cx);
            self.set_layout_mode(WorkspaceLayoutMode::ChatFocus, cx);
            self.set_bottom_terminal_open(false, cx);
        }
        self.composer_editor
            .update(cx, |editor, cx| editor.focus(window, cx));
        cx.notify();
    }

    pub fn fill_starter_prompt(
        &mut self,
        prompt: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composer_editor.update(cx, |ed, cx| {
            let value = if ed.value().is_empty() {
                prompt.to_owned()
            } else {
                format!("{}\n\n{prompt}", ed.value())
            };
            ed.set_value(value, window, cx);
        });
        self.reveal_chat_composer(window, cx);
    }

    fn opening_existing_conversation(&self) -> bool {
        self.pending_navigation
            .as_ref()
            .is_some_and(|navigation| navigation.destination != LocalChatDestination::Draft)
    }

    pub fn submit_composer_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.opening_existing_conversation()
            || self.aster_switching()
            || self.composer_dictation().is_some()
        {
            return;
        }
        self.open_menu = None;
        let typed = self.composer_editor.read(cx).value().trim().to_owned();
        if (typed.is_empty() && self.pending_attachments.is_empty())
            || self.queued_prompt.is_some()
        {
            return;
        }
        if self.submit_slash_command(&typed, window, cx) || self.submit_planning_input(&typed, window, cx) {
            return;
        }
        let raw = if typed.is_empty() {
            "请结合附件内容处理。".to_owned()
        } else {
            typed
        };

        if self.turn_in_progress() {
            if matches!(
                self.access_mode,
                ChatAccessMode::WriteController { .. }
                    | ChatAccessMode::RemoteController {
                        role: PeerRole::Controller,
                        ..
                    }
            ) {
                let target = self.pinned_target.take();
                let attachments = std::mem::take(&mut self.pending_attachments);
                self.composer_editor
                    .update(cx, |ed, cx| ed.set_value("", window, cx));
                self.chat_follow_paused = false;
                self.chat_scroll.scroll_to_bottom();
                if self.prefs.follow_up_mode == crate::prefs::FollowUpMode::Queue {
                    self.queued_prompt = Some(QueuedPrompt {
                        text: raw,
                        target,
                        attachments,
                    });
                } else {
                    let request_id = Uuid::new_v4();
                    let payload = match &target {
                        Some(tgt) => tgt.compose_prompt(&raw),
                        None => raw.clone(),
                    };
                    if let Some(ref tgt) = target {
                        self.turn_targets.insert(request_id, tgt.clone());
                    }
                    self.record_immutable_turn_capture(request_id, target.as_ref(), &attachments);
                    self.bridge.send(KarCommand::SteerOrQueuePrompt {
                        request_id,
                        raw_user_text: raw,
                        payload,
                        target,
                        attachments,
                    });
                }
                cx.notify();
            }
            return;
        }

        if matches!(self.access_mode, ChatAccessMode::NoneSelected) {
            if self.create_new_real_chat(cx) {
                self.queued_prompt = Some(QueuedPrompt {
                    text: raw,
                    target: self.pinned_target.take(),
                    attachments: std::mem::take(&mut self.pending_attachments),
                });
                self.composer_editor
                    .update(cx, |ed, cx| ed.set_value("", window, cx));
                self.chat_scroll.scroll_to_bottom();
                cx.notify();
            }
            return;
        }

        if !self.access_mode.is_writable() {
            return;
        }
        let target = self.pinned_target.take();
        let attachments = std::mem::take(&mut self.pending_attachments);
        self.composer_editor
            .update(cx, |ed, cx| ed.set_value("", window, cx));
        self.dispatch_prompt(raw, target, attachments, cx);
    }

    fn dispatch_prompt(
        &mut self,
        raw: String,
        target: Option<SelectionTarget>,
        attachments: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.dispatch_prompt_id(Uuid::new_v4(), raw, target, attachments, cx);
    }

    fn dispatch_prompt_id(&mut self, request_id: Uuid, raw: String, target: Option<SelectionTarget>, attachments: Vec<PathBuf>, cx: &mut Context<Self>) {
        let payload = match &target {
            Some(tgt) => tgt.compose_prompt(&raw),
            None => raw.clone(),
        };
        if let Some(ref tgt) = target {
            self.turn_targets.insert(request_id, tgt.clone());
        }
        self.record_immutable_turn_capture(request_id, target.as_ref(), &attachments);
        self.chat_follow_paused = false;
        self.chat_scroll.scroll_to_bottom();
        if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            self.bridge.send(KarCommand::SendDemoPrompt {
                request_id,
                raw_user_text: raw,
                target,
            });
        } else {
            self.bridge.send(KarCommand::SendPrompt {
                request_id,
                raw_user_text: raw,
                payload,
                target,
                attachments,
            });
        }
        cx.notify();
    }

    pub fn retry_turn_with_same_uuid(
        &mut self,
        turn_id: Uuid,
        fallback_user_text: String,
        cx: &mut Context<Self>,
    ) {
        let (raw_user_text, payload, target, attachments) =
            if let Some(prev) = self.prompt_attempts.get(&turn_id) {
                (
                    prev.raw_user_text.clone(),
                    prev.sent_payload.clone(),
                    prev.target.clone(),
                    prev.attachments.clone(),
                )
            } else {
                let target = self.turn_targets.get(&turn_id).cloned();
                let payload = match &target {
                    Some(tgt) => tgt.compose_prompt(&fallback_user_text),
                    None => fallback_user_text.clone(),
                };
                (fallback_user_text, payload, target, Vec::new())
            };
        self.bridge.send(KarCommand::SendPrompt {
            request_id: turn_id,
            raw_user_text,
            payload,
            target,
            attachments,
        });
        cx.notify();
    }

    pub(crate) fn refresh_agents(&mut self, cx: &mut Context<Self>) {
        self.bridge.send(KarCommand::ProbeProvidersOnly);
        cx.notify();
    }

    // ── Sharing ─────────────────────────────────────────────────────────────

    pub(crate) fn start_sharing(&mut self, cx: &mut Context<Self>) {
        self.open_menu = None;
        let bind_addr = if self.prefs.lan_listen_enabled {
            "0.0.0.0:0"
        } else {
            "127.0.0.1:0"
        };
        if let ChatAccessMode::WriteController { chat_id }
        | ChatAccessMode::LocalReadOnlySubscriber { chat_id, .. }
        | ChatAccessMode::ArchivedReadOnly { chat_id, .. } = self.access_mode
        {
            self.bridge.send(KarCommand::StartHttpServer {
                chat_id,
                listen_addr: bind_addr.into(),
            });
        } else if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            self.bridge.send(KarCommand::StartHttpServer {
                chat_id: Uuid::nil(),
                listen_addr: bind_addr.into(),
            });
        } else {
            self.notify_warning("先打开一个本地对话，再开启节点共享", cx);
        }
    }

    pub(crate) fn stop_sharing(&mut self, cx: &mut Context<Self>) {
        self.open_menu = None;
        self.bridge.send(KarCommand::StopHttpServer);
        cx.notify();
    }

    pub(crate) fn open_shared_link_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        self.command_palette_open = false;
        self.provider_sheet_open = false;
        self.http_dialog
            .update(cx, |d, cx| d.set_open(true, window, cx));
        cx.notify();
    }

    pub(crate) fn open_subscriber_second_window(&mut self, cx: &mut Context<Self>) {
        self.open_menu = None;
        let mut sub_cfg = self.config.clone();
        sub_cfg.open_files.clear();
        sub_cfg.initial_terminal_cmd = None;
        sub_cfg.start_coedit_demo = false;
        sub_cfg.start_demo_chat = false;
        sub_cfg.dark_mode = !self.follow_system_appearance && self.theme_mode == ThemeMode::Dark;
        sub_cfg.light_mode = !self.follow_system_appearance && self.theme_mode == ThemeMode::Light;
        sub_cfg.lang_en = self.ui_locale == UiLocale::En;
        sub_cfg.lang_zh = self.ui_locale == UiLocale::Zh;
        sub_cfg.open_latest_chat = true;
        sub_cfg.subscribe_http = self.active_http_server_url.clone();
        let options = studio_window_options(
            crate::APP_NAME,
            size(px(1260.), px(800.)),
            size(px(860.), px(540.)),
            cx,
        );
        let _ = gpui_kit::open_window(options, cx, move |w, cx| {
            cx.new(|cx| KarWorkbench::new(sub_cfg, w, cx))
        });
    }

    // ── Attachments & Documents ─────────────────────────────────────────────

    pub fn pick_attachments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("添加附件".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                for path in paths {
                    if path.is_file() && !this.pending_attachments.contains(&path) {
                        this.pending_attachments.push(path);
                    }
                }
                this.composer_editor
                    .update(cx, |ed, cx| ed.focus(window, cx));
                cx.notify();
            });
        })
        .detach();
    }

    pub fn pick_documents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("打开".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                for path in paths {
                    if OfficeKind::for_path(&path).is_some() {
                        this.open_office_file_tab(path, false, None, window, cx);
                    } else {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        this.notify_warning(format!("{name} 不是 Word、Excel 或 PPT 文件"), cx);
                    }
                }
            });
        })
        .detach();
    }

    pub fn pick_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.select_project_and_new_chat(dir, window, cx);
            });
        })
        .detach();
    }

    pub(crate) fn open_working_sample(
        &mut self,
        filename: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.open_menu = None;
        let saved_variant = filename.replace('.', ".saved.");
        let candidate = if self.config.docs_dir.join(filename).is_file() {
            self.config.docs_dir.join(filename)
        } else if self.config.docs_dir.join(&saved_variant).is_file() {
            self.config.docs_dir.join(&saved_variant)
        } else {
            let corpus = |rel: &str| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|root| root.join("tests/corpus").join(rel))
            };
            let source = match filename {
                "sample_minutes.docx" => corpus("docx/minutes.docx"),
                "sample_formulas.xlsx" => corpus("xlsx/01_formulas.xlsx"),
                "sample_deck.pptx" => corpus("pptx/deck.pptx"),
                _ => None,
            };
            let dst = self.config.docs_dir.join(filename);
            if let Some(src) = source.filter(|p| p.is_file()) {
                let _ = std::fs::create_dir_all(&self.config.docs_dir);
                let _ = std::fs::copy(&src, &dst);
            }
            dst
        };

        if candidate.is_file() {
            let path = candidate.canonicalize().unwrap_or(candidate);
            self.open_office_file_tab(path.clone(), false, None, window, cx);
            self.active_office_tab
                .and_then(|index| self.office_tabs.get(index))
                .and_then(|tab| tab.path(cx))
                .as_ref()
                == Some(&path)
        } else {
            self.notify_warning("这份示例文档不在当前安装中", cx);
            false
        }
    }

    pub fn open_office_file_tab(
        &mut self,
        path: PathBuf,
        is_demo: bool,
        demo_key: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.main_stage_view = MainStageView::Conversation;
        self.ensure_split_visible(window, cx);
        // Bug #1 Fix: always switch the Right Dock surface back to `Office` when opening a document!
        self.right_surface = RightPaneSurface::Office;
        let canonical = path.canonicalize().unwrap_or(path);
        if let Some(idx) = self.office_tabs.iter().position(|t| {
            t.is_demo == is_demo
                && t.path(cx)
                    .map(|p| p.canonicalize().unwrap_or(p) == canonical)
                    .unwrap_or(false)
        }) {
            self.office_tabs[idx].focus(window, cx);
            self.active_office_tab = Some(idx);
            cx.notify();
            return;
        }
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        match OfficeTab::open_file(id, canonical, is_demo, demo_key, window, cx) {
            Ok(tab) => {
                if !is_demo {
                    self.coedit_panel_open = false;
                }
                tab.focus(window, cx);
                self.office_tabs.push(tab);
                self.active_office_tab = Some(self.office_tabs.len() - 1);
            }
            Err(err) => self.notify_warning(format!("打不开这个文件：{err:#}"), cx),
        }
        cx.notify();
    }

    pub(crate) fn open_blank_office_tab(
        &mut self,
        kind: OfficeKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_menu = None;
        self.main_stage_view = MainStageView::Conversation;
        self.ensure_split_visible(window, cx);
        self.right_surface = RightPaneSurface::Office;
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = OfficeTab::new_blank(id, kind, window, cx);
        tab.focus(window, cx);
        self.coedit_panel_open = false;
        self.office_tabs.push(tab);
        self.active_office_tab = Some(self.office_tabs.len() - 1);
        cx.notify();
    }

    pub fn activate_office_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx >= self.office_tabs.len() {
            return;
        }
        self.main_stage_view = MainStageView::Conversation;
        if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
            self.set_layout_mode(WorkspaceLayoutMode::Split, cx);
        }
        self.right_surface = RightPaneSurface::Office;
        self.active_office_tab = Some(idx);
        if let Some(key) = self.office_tabs[idx].demo_scenario_key {
            let scenario = scenario_for_key(key);
            self.coedit_panel_open = true;
            self.active_coedit_scenario = scenario;
            self.coedit_segmented
                .update(cx, |seg, cx| seg.sync_value(scenario.as_str(), cx));
        }
        cx.notify();
    }

    pub fn close_office_tab(&mut self, tab_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.office_tabs.iter().find(|t| t.id == tab_id).cloned() else {
            return;
        };
        if tab.is_demo || !tab.is_dirty(cx) {
            self.remove_office_tab(tab_id, cx);
            return;
        }
        let name = clean_display_doc_name(&tab.doc_name(cx));
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("要保存对“{name}”的修改吗？"),
            Some("不保存的话，这些修改会丢失。"),
            &["保存", "不保存", "取消"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(choice) = answer.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| match choice {
                0 => {
                    if tab.needs_save_as(cx) {
                        this.save_as_and_then_close(tab, tab_id, window, cx);
                    } else {
                        match tab.save_in_place(window, cx) {
                            Ok(_) => this.remove_office_tab(tab_id, cx),
                            Err(err) => this.notify_warning(format!("没有保存成功：{err:#}"), cx),
                        }
                    }
                }
                1 => this.remove_office_tab(tab_id, cx),
                _ => {}
            });
        })
        .detach();
    }

    fn save_as_and_then_close(
        &mut self,
        tab: OfficeTab,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dir = tab
            .path(cx)
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| self.config.workspace_dir.clone());
        let suggested = tab.default_save_filename(cx);
        let receiver = cx.prompt_for_new_path(&dir, Some(&suggested));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(dest))) = receiver.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                match tab.save_copy_to(&dest, window, cx) {
                    Ok(_) => {
                        this.remove_office_tab(tab_id, cx);
                        this.notify_toast("已保存并关闭", cx);
                    }
                    Err(err) => this.notify_warning(format!("没有保存成功：{err:#}"), cx),
                }
            });
        })
        .detach();
    }

    fn remove_office_tab(&mut self, tab_id: u64, cx: &mut Context<Self>) {
        let Some(idx) = self.office_tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        self.office_tabs.remove(idx);
        self.active_office_tab = match self.active_office_tab {
            _ if self.office_tabs.is_empty() => None,
            Some(active) if active > idx => Some(active - 1),
            Some(active) if active == idx => Some(idx.min(self.office_tabs.len() - 1)),
            other => other,
        };
        if self.right_surface == RightPaneSurface::Office && self.office_tabs.is_empty() {
            self.right_surface = self
                .code_tab_order
                .first()
                .cloned()
                .map(RightPaneSurface::CodeFile)
                .unwrap_or_else(|| self.remaining_dock_surface());
        }
        if !self.office_tabs.iter().any(|t| t.is_demo) {
            self.coedit_panel_open = false;
        }
        cx.notify();
    }

    pub fn save_active_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        let Some(tab) = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .cloned()
        else {
            return;
        };
        if tab.is_demo {
            self.notify_warning("示例文档仅供演练，不会保存", cx);
            return;
        }
        if tab.needs_save_as(cx) {
            self.prompt_save_as_active_document(window, cx);
            return;
        }
        match tab.save_in_place(window, cx) {
            Ok(_) => {
                let doc_key = clean_display_doc_name(&tab.doc_name(cx));
                let _ = self
                    .fence_ledger
                    .bump_human_edit(&doc_key, &format!("saved@{}", now_epoch_ms()));
                self.fence_ledger.save(&self.config.docs_dir);
                self.notify_toast("已保存（Revision Fence 已递进）", cx);
            }
            Err(err) => self.notify_warning(format!("没有保存成功：{err:#}"), cx),
        }
    }

    /// Native system Save-As dialog (`cx.prompt_for_new_path`) for untitled or existing documents.
    pub(crate) fn prompt_save_as_active_document(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_menu = None;
        let Some(idx) = self.active_office_tab else {
            return;
        };
        let Some(tab) = self.office_tabs.get(idx).cloned() else {
            return;
        };
        if tab.is_demo {
            self.notify_warning("示例文档仅供演练，不会保存", cx);
            return;
        }
        let tab_id = tab.id;
        let dir = tab
            .path(cx)
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| self.config.workspace_dir.clone());
        let suggested = tab.default_save_filename(cx);
        let receiver = cx.prompt_for_new_path(&dir, Some(&suggested));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(dest))) = receiver.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                match tab.save_copy_to(&dest, window, cx) {
                    Ok(_) => {
                        let name = dest
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        if let Some(pos) = this.office_tabs.iter().position(|t| t.id == tab_id)
                            && let Ok(fresh) =
                                OfficeTab::open_file(tab_id, dest, false, None, window, cx)
                        {
                            this.office_tabs[pos] = fresh;
                            this.active_office_tab = Some(pos);
                        }
                        this.notify_toast(format!("已另存为 {name}"), cx);
                    }
                    Err(err) => this.notify_warning(format!("没有保存成功：{err:#}"), cx),
                }
            });
        })
        .detach();
    }

    /// Save an automatic `.saved.<ext>` sibling copy alongside the original document.
    pub(crate) fn save_copy_of_active_document(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_menu = None;
        let Some(idx) = self.active_office_tab else {
            return;
        };
        let Some(tab) = self.office_tabs.get(idx).cloned() else {
            return;
        };
        if tab.is_demo {
            self.notify_warning("示例文档仅供演练，不会保存", cx);
            return;
        }
        let target_path = tab.path(cx).unwrap_or_else(|| {
            let ext = match tab.kind {
                OfficeKind::Word => "docx",
                OfficeKind::Excel => "xlsx",
                OfficeKind::Ppt => "pptx",
                OfficeKind::Pdf => "pdf",
            };
            self.config
                .docs_dir
                .join(format!("untitled_{}.{ext}", tab.id))
        });
        let copy_dest = build_saved_copy_path(&target_path, &self.config.docs_dir);
        match tab.save_copy_to(&copy_dest, window, cx) {
            Ok(_) => {
                let name = copy_dest
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.office_tabs.remove(idx);
                self.active_office_tab = None;
                self.open_office_file_tab(copy_dest, false, None, window, cx);
                self.notify_toast(format!("已保存副本 {name}"), cx);
            }
            Err(err) => self.notify_warning(format!("没有保存成功：{err:#}"), cx),
        }
    }

    pub fn bind_active_selection_to_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        let Some(tab) = self
            .active_office_tab
            .and_then(|idx| self.office_tabs.get(idx))
        else {
            return;
        };
        self.pinned_target = Some(tab.capture_selection(cx));
        self.reveal_chat_composer(window, cx);
    }

    pub fn jump_to_selection_target(
        &mut self,
        target: &SelectionTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_split_visible(window, cx);
        if let SelectionLocator::CodeLines { rel_path, .. } = &target.locator {
            self.open_workspace_code_tab(rel_path.clone(), cx);
            return;
        }
        let found_idx = self
            .office_tabs
            .iter()
            .position(|t| t.id == target.tab_id)
            .or_else(|| {
                let target_path = target.file_path.as_ref()?;
                let canon = target_path
                    .canonicalize()
                    .unwrap_or_else(|_| target_path.clone());
                self.office_tabs.iter().position(|t| {
                    t.path(cx)
                        .map(|p| p.canonicalize().unwrap_or(p) == canon)
                        .unwrap_or(false)
                })
            });

        let tab_idx = match found_idx {
            Some(i) => Some(i),
            None => match &target.file_path {
                Some(path) if path.is_file() => {
                    self.open_office_file_tab(path.clone(), false, None, window, cx);
                    self.active_office_tab
                }
                _ => None,
            },
        };

        match tab_idx.and_then(|i| self.office_tabs.get(i).cloned().map(|tab| (i, tab))) {
            Some((i, tab)) => {
                self.right_surface = RightPaneSurface::Office;
                self.active_office_tab = Some(i);
                tab.reveal_locator(&target.locator, window, cx);
                cx.notify();
            }
            None => self.notify_warning("这份文档已经不在原来的位置了", cx),
        }
    }

    pub(crate) fn open_quick_edit_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_all_overlays(cx);
        self.populate_quick_edit_dialog(window, cx);
        self.quick_edit_dialog
            .update(cx, |d, cx| d.set_open(true, window, cx));
    }

    pub(crate) fn populate_quick_edit_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .cloned()
        else {
            return;
        };
        let sel = tab.capture_selection(cx);
        let (content, place) = match &sel.locator {
            SelectionLocator::WordParagraph { start_para, .. } => {
                (sel.excerpt.clone(), format!("{}", start_para + 1))
            }
            SelectionLocator::ExcelRange {
                active_cell,
                raw_input,
                computed_value,
                ..
            } => {
                let initial = if !raw_input.is_empty() {
                    raw_input.clone()
                } else {
                    computed_value.clone()
                };
                (initial, active_cell.clone())
            }
            SelectionLocator::PptObject { bounds_cm, .. } => (
                sel.excerpt.clone(),
                bounds_cm
                    .map(|(x, y, _, _)| format!("{x:.2},{y:.2}"))
                    .unwrap_or_else(|| "2.00,3.00".into()),
            ),
            SelectionLocator::PdfPage { page_index, .. } => {
                (sel.excerpt.clone(), format!("{}", page_index + 1))
            }
            SelectionLocator::CodeLines { start_line, .. } => {
                (sel.excerpt.clone(), format!("{start_line}"))
            }
        };
        self.quick_edit_primary
            .update(cx, |inp, cx| inp.set_value(content, window, cx));
        self.quick_edit_secondary
            .update(cx, |inp, cx| inp.set_value(place, window, cx));
    }

    fn apply_quick_edit_from_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .cloned()
        else {
            return;
        };
        let primary = self.quick_edit_primary.read(cx).value(cx).to_owned();
        let secondary = self.quick_edit_secondary.read(cx).value(cx).trim().to_owned();
        let sel = tab.capture_selection(cx);

        let outcome = match &sel.locator {
            SelectionLocator::WordParagraph { start_para, .. } => {
                let idx = secondary
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| n.checked_sub(1))
                    .unwrap_or(*start_para);
                tab.replace_word_paragraph(idx, &primary, window, cx)
                    .map(|()| format!("已修改第 {} 段", idx + 1))
            }
            SelectionLocator::ExcelRange {
                sheet_index,
                active_cell,
                ..
            } => {
                let cell = if secondary.is_empty() {
                    active_cell.as_str()
                } else {
                    secondary.as_str()
                };
                tab.write_excel_cell(*sheet_index, cell, &primary, window, cx)
                    .map(|shown| format!("{cell} 现在是 {shown}"))
            }
            SelectionLocator::PptObject {
                slide_index,
                shape_id,
                ..
            } => {
                let Some(sid) = *shape_id else {
                    self.notify_warning("这一页没有可以修改的对象", cx);
                    return;
                };
                let parsed_xy = secondary.split_once(',').and_then(|(xs, ys)| {
                    Some((xs.trim().parse::<f64>().ok()?, ys.trim().parse::<f64>().ok()?))
                });
                let new_txt = (!primary.is_empty()).then_some(primary.as_str());
                tab.update_ppt_shape(*slide_index, sid, new_txt, parsed_xy, window, cx)
                    .map(|()| format!("已修改第 {} 页的对象", slide_index + 1))
            }
            SelectionLocator::PdfPage { .. } => {
                let page = secondary
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| n.checked_sub(1))
                    .unwrap_or(0);
                tab.reveal_locator(
                    &SelectionLocator::PdfPage {
                        page_index: page,
                        total_pages: 1,
                    },
                    window,
                    cx,
                );
                Ok(format!("已跳转至 PDF 第 {} 页", page + 1))
            }
            SelectionLocator::CodeLines { .. } => Ok("代码文件请在聊天或终端中修改".into()),
        };

        match outcome {
            Ok(msg) => self.notify_toast(msg, cx),
            Err(err) => self.notify_warning(format!("没有改成：{err:#}"), cx),
        }
    }

    pub fn select_excel_cell_on_active_tab(
        &mut self,
        cell_a1: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .cloned()
        {
            tab.reveal_locator(
                &SelectionLocator::ExcelRange {
                    sheet_index: 0,
                    sheet_name: String::new(),
                    range: cell_a1.to_owned(),
                    active_cell: cell_a1.to_owned(),
                    raw_input: String::new(),
                    computed_value: String::new(),
                },
                window,
                cx,
            );
            cx.notify();
        }
    }

    // ── Co-editing sample (temporary sample documents only) ─────────────────

    pub fn enter_coedit_demo_workshop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        self.main_stage_view = MainStageView::Conversation;
        self.ensure_split_visible(window, cx);
        let demo_dir = self.coedit_demo_dir.clone().unwrap_or_else(|| {
            std::env::temp_dir().join(format!("kar_desktop_coedit_demo_{}", std::process::id()))
        });
        match materialize_coedit_demo_files(&demo_dir) {
            Ok((word_p, excel_p, ppt_p)) => {
                self.coedit_demo_dir = Some(demo_dir);
                for (path, scenario) in [
                    (word_p, ScenarioKey::WordSameParagraph),
                    (excel_p, ScenarioKey::ExcelSameCellConflict),
                    (ppt_p, ScenarioKey::PptMoveAndText),
                ] {
                    self.open_office_file_tab(path, true, Some(scenario.as_str()), window, cx);
                }
                for sc in [
                    ScenarioKey::WordSameParagraph,
                    ScenarioKey::ExcelSameCellConflict,
                    ScenarioKey::PptMoveAndText,
                ] {
                    if let Some(tab) = self
                        .office_tabs
                        .iter()
                        .find(|t| t.demo_scenario_key == Some(sc.as_str()))
                    {
                        let base_rev = tab.revision(cx).unwrap_or(1);
                        let shape_id = match (&tab.view, sc) {
                            (OfficeEditorView::Ppt(e), ScenarioKey::PptMoveAndText) => e
                                .read(cx)
                                .agent_read(|p, _| p.slide(0).shapes.first().map(|s| s.id)),
                            _ => None,
                        };
                        self.coedit_engine.reset_scenario(sc, base_rev, shape_id);
                    }
                }
                self.coedit_dismiss_morph
                    .update(cx, |m, cx| m.restore(cx));
                self.coedit_panel_open = true;
                self.switch_coedit_scenario(self.active_coedit_scenario, window, cx);
                self.coedit_feedback = None;
            }
            Err(err) => self.notify_warning(format!("示例文档没有准备好：{err:#}"), cx),
        }
        cx.notify();
    }

    pub fn switch_coedit_scenario(
        &mut self,
        scenario: ScenarioKey,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_coedit_scenario = scenario;
        self.coedit_segmented
            .update(cx, |seg, cx| seg.sync_value(scenario.as_str(), cx));
        if let Some(idx) = self
            .office_tabs
            .iter()
            .position(|t| t.demo_scenario_key == Some(scenario.as_str()))
        {
            self.active_office_tab = Some(idx);
        }
        self.coedit_panel_open = true;
        self.coedit_feedback = None;
        cx.notify();
    }

    pub(crate) fn read_live_coedit_state(
        &self,
        scenario: ScenarioKey,
        cx: &App,
    ) -> Option<LiveTargetState> {
        let tab = self
            .office_tabs
            .iter()
            .find(|t| t.demo_scenario_key == Some(scenario.as_str()))?;
        let revision = tab.revision(cx).unwrap_or(1);
        match (&tab.view, scenario) {
            (OfficeEditorView::Word(e), ScenarioKey::WordSameParagraph) => {
                let text = e.read(cx).agent_read(|doc, _| {
                    if doc.len() > 1 {
                        clean_doc_para_text(doc, 1)
                    } else {
                        String::new()
                    }
                });
                Some(LiveTargetState {
                    scenario,
                    revision,
                    current_value: text,
                    current_aux: String::new(),
                    target_exists: true,
                    ppt_xy_cm: None,
                })
            }
            (OfficeEditorView::Excel(e), ScenarioKey::ExcelSameCellConflict) => {
                if e.read(cx).is_busy() {
                    return None;
                }
                let (b3_val, b4_total) = e.read(cx).agent_read(cx, |book, _| {
                    (book.display_value(0, 2, 1), book.display_value(0, 3, 1))
                });
                Some(LiveTargetState {
                    scenario,
                    revision,
                    current_value: b3_val,
                    current_aux: format!("合计 B4 = {b4_total}"),
                    target_exists: true,
                    ppt_xy_cm: None,
                })
            }
            (OfficeEditorView::Ppt(e), ScenarioKey::PptMoveAndText) => {
                let target_id = self.coedit_ppt_shape_id();
                let (exists, txt, xy) = e.read(cx).agent_read(|pres, _| {
                    if let Some(ix) = pres.shape_index(0, target_id) {
                        let s = &pres.slide(0).shapes[ix];
                        let t = s
                            .text
                            .as_ref()
                            .map(|t| t.plain_text())
                            .unwrap_or_default();
                        let xfrm = s.xfrm.or_else(|| pres.placed((0, ix)).map(|p| p.xfrm));
                        let coords = xfrm.map(|x| (x.x as f64 / 360_000.0, x.y as f64 / 360_000.0));
                        (true, t, coords)
                    } else {
                        (false, String::new(), None)
                    }
                });
                Some(LiveTargetState {
                    scenario,
                    revision,
                    current_value: txt,
                    current_aux: String::new(),
                    target_exists: exists,
                    ppt_xy_cm: xy,
                })
            }
            _ => None,
        }
    }

    fn coedit_ppt_shape_id(&self) -> u32 {
        self.coedit_engine
            .proposals()
            .iter()
            .find(|p| p.scenario == ScenarioKey::PptMoveAndText)
            .and_then(|p| p.target_shape_id)
            .unwrap_or(2)
    }

    pub fn trigger_coedit_human_step(
        &mut self,
        step: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sc = self.active_coedit_scenario;
        let Some(tab) = self
            .office_tabs
            .iter()
            .find(|t| t.demo_scenario_key == Some(sc.as_str()))
            .cloned()
        else {
            return;
        };

        if let Some(live_before) = self.read_live_coedit_state(sc, cx) {
            let _ = self.coedit_engine.sync_live_state(&live_before);
        }

        let res = match step {
            "word_human_date" => tab.replace_word_paragraph(
                1,
                "发布排期：日期 2026-12-01，负责人：待定，状态：评审中",
                window,
                cx,
            ),
            "word_mid_conflict_edit" => tab.replace_word_paragraph(
                1,
                "发布排期：日期 2026-12-08，负责人：待定，状态：加急评审",
                window,
                cx,
            ),
            "excel_human_b3" => tab
                .write_excel_cell(0, "B3", "135000", window, cx)
                .map(|_| ()),
            "excel_mid_conflict_edit" => tab
                .write_excel_cell(0, "B3", "139000", window, cx)
                .map(|_| ()),
            "ppt_human_move" => {
                let sid = self.coedit_ppt_shape_id();
                tab.update_ppt_shape(0, sid, None, Some((6.50, 4.80)), window, cx)
            }
            "ppt_mid_conflict_move" => {
                let sid = self.coedit_ppt_shape_id();
                tab.update_ppt_shape(0, sid, None, Some((8.20, 5.40)), window, cx)
            }
            "ppt_human_delete" => {
                self.coedit_dismiss_morph
                    .update(cx, |m, cx| m.restore(cx));
                let sid = self.coedit_ppt_shape_id();
                tab.delete_ppt_shape(0, sid, window, cx)
            }
            "reset_all" => {
                if let Some(dir) = self.coedit_demo_dir.take() {
                    let _ = std::fs::remove_dir_all(&dir);
                }
                self.office_tabs.retain(|t| !t.is_demo);
                self.active_office_tab = self.office_tabs.len().checked_sub(1);
                self.enter_coedit_demo_workshop(window, cx);
                return;
            }
            _ => Ok(()),
        };

        if let Err(err) = res {
            self.coedit_feedback = Some(format!("这一步没有执行：{err:#}"));
        } else if let Some(live_after) = self.read_live_coedit_state(sc, cx) {
            let _ = self.coedit_engine.sync_live_state(&live_after);
        }
        cx.notify();
    }

    pub fn submit_coedit_choice(
        &mut self,
        op_id: Uuid,
        choice: UserResolutionChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sc = self.active_coedit_scenario;
        let Some(live) = self.read_live_coedit_state(sc, cx) else {
            return;
        };
        let Some(tab) = self
            .office_tabs
            .iter()
            .find(|t| t.demo_scenario_key == Some(sc.as_str()))
            .cloned()
        else {
            return;
        };

        match self.coedit_engine.submit_decision(op_id, choice, &live) {
            Ok(SubmissionOutcome::NeedsReconfirm { latest_value, .. }) => {
                self.coedit_feedback = Some(format!(
                    "你处理的时候文档又变了，现在是“{latest_value}”。请先确认最新内容。"
                ));
            }
            Ok(SubmissionOutcome::Accepted { proposal, mutation }) => {
                let applied = match mutation {
                    DemoEditorMutation::None => Ok(()),
                    DemoEditorMutation::ReplaceWordParagraph { para_idx, text } => {
                        tab.replace_word_paragraph(para_idx, &text, window, cx)
                    }
                    DemoEditorMutation::WriteExcelCell {
                        sheet,
                        cell_a1,
                        value,
                    } => tab
                        .write_excel_cell(sheet, &cell_a1, &value, window, cx)
                        .map(|_| ()),
                    DemoEditorMutation::UpdatePptShape {
                        slide,
                        shape_id,
                        text,
                        xy_cm,
                    } => tab.update_ppt_shape(slide, shape_id, text.as_deref(), xy_cm, window, cx),
                };
                self.coedit_feedback = match applied {
                    Ok(()) => proposal.resolution_result,
                    Err(err) => Some(format!("没有应用成功：{err:#}")),
                };
            }
            Err(err) => {
                self.coedit_feedback = Some(format!("没有提交成功：{err:#}"));
            }
        }
        cx.notify();
    }

    // ── Keyboard ────────────────────────────────────────────────────────────

    fn handle_global_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.any_dialog_open(cx) {
            return;
        }
        let cmd = ev.keystroke.modifiers.platform || ev.keystroke.modifiers.control;
        let shift = ev.keystroke.modifiers.shift;
        let key = ev.keystroke.key.as_str();
        if self.settings_open || self.handoff_sheet_open || self.side_question_open {
            if key == "escape" && self.open_menu.take().is_some() {
                cx.notify();
                cx.stop_propagation();
                return;
            }
            if key == "escape" || (cmd && key == ",") {
                self.close_all_overlays(cx);
                cx.stop_propagation();
            } else if cmd && key == "k" {
                self.toggle_command_palette(window, cx);
                cx.stop_propagation();
            }
            return;
        }

        if self.command_palette_open && !cmd && key == "escape" {
            self.close_command_palette(window, cx);
            cx.stop_propagation();
            return;
        }

        // Custom & built-in dynamic keybindings (`prefs.matches_command`) + `⌘Q` quit safety
        let ks = &ev.keystroke;
        if cmd && key.eq_ignore_ascii_case("q") && !shift && !ks.modifiers.alt {
            match self.prefs.quit_shortcut_mode {
                crate::prefs::QuitShortcutMode::Immediate => {
                    cx.quit();
                    return;
                }
                crate::prefs::QuitShortcutMode::PressTwice => {
                    if self
                        .quit_pending_since
                        .is_some_and(|t| t.elapsed().as_secs_f32() < 2.2)
                    {
                        cx.quit();
                    } else {
                        self.quit_pending_since = Some(Instant::now());
                        self.notify_toast(
                            self.ui_locale
                                .pick("再按一次 ⌘Q 退出 Kwrok", "Press ⌘Q again to quit"),
                            cx,
                        );
                    }
                    cx.stop_propagation();
                    return;
                }
                crate::prefs::QuitShortcutMode::Hold => {
                    if self
                        .quit_pending_since
                        .is_some_and(|t| t.elapsed().as_millis() >= 650 && t.elapsed().as_secs_f32() < 3.5)
                    {
                        cx.quit();
                    } else if self.quit_pending_since.is_none_or(|t| t.elapsed().as_secs_f32() >= 3.5) {
                        self.quit_pending_since = Some(Instant::now());
                        self.notify_toast(
                            self.ui_locale
                                .pick("继续按住 ⌘Q 退出应用…", "Keep holding ⌘Q to quit…"),
                            cx,
                        );
                    }
                    cx.stop_propagation();
                    return;
                }
            }
        }

        let matched_cmd = crate::prefs::KEYBINDING_COMMANDS
            .iter()
            .find(|def| self.prefs.matches_command(def.id, ks))
            .map(|def| def.id);
        if let Some(cid) = matched_cmd {
            let mut dispatched = true;
            match cid {
                "app.command_palette" => self.toggle_command_palette(window, cx),
                "app.settings" => self.toggle_settings(self.active_settings_tab, cx),
                "theme.toggle" => {
                    let next = if self.theme_mode == ThemeMode::Dark {
                        ThemeMode::Light
                    } else {
                        ThemeMode::Dark
                    };
                    self.select_appearance(next, cx);
                }
                "locale.toggle" => self.toggle_ui_locale(window, cx),
                "chat.new" => self.start_new_chat(window, cx),
                "chat.find" if self.main_stage_view == MainStageView::Conversation
                    && (self.layout_mode == WorkspaceLayoutMode::ChatFocus
                        || self.focus_handle.contains_focused(window, cx)) =>
                {
                    self.chat_search_open = !self.chat_search_open;
                    cx.notify();
                }
                "chat.handoff" => {
                    self.handoff_sheet_open = !self.handoff_sheet_open;
                    cx.notify();
                }
                "chat.side_question" => self.toggle_side_question_sheet(cx),
                "chat.voice_dictate" => {
                    if !ev.is_held {
                        if self.prefs.voice_dictation_mode == crate::prefs::DictationTriggerMode::Hold {
                            if self.active_voice_rec.is_none() {
                                self.held_voice_key = Some(key.to_owned());
                                self.toggle_voice_dictation(window, cx);
                                if self.active_voice_rec.is_none() {self.held_voice_key=None;}
                            }
                        } else {
                            self.toggle_voice_dictation(window, cx);
                        }
                    }
                },
                "chat.compact" => self.bridge.send(KarCommand::CompactChat),
                "view.toggle_sidebar" => self.toggle_sidebar(cx),
                "view.toggle_terminal" => self.toggle_bottom_terminal(cx),
                "view.toggle_right_dock" => self.toggle_document_pane(cx),
                "view.open_inbox" => {
                    self.main_stage_view = MainStageView::TasksInbox;
                    cx.notify();
                }
                "view.open_gallery" => {
                    self.main_stage_view = MainStageView::OfficeGallery;
                    cx.notify();
                }
                "view.open_git" => self.open_git_workbench_tab(cx),
                "view.open_prs" => {
                    self.git_sub_mode = GitSubMode::PullRequests;
                    self.open_git_workbench_tab(cx);
                }
                "view.open_issues" => {
                    self.git_sub_mode = GitSubMode::Issues;
                    self.open_git_workbench_tab(cx);
                }
                "view.open_files" => self.open_files_dock_tab(cx),
                "office.save"
                    if self.active_office_tab.is_some()
                        && self.right_surface == RightPaneSurface::Office
                        && self.layout_mode != WorkspaceLayoutMode::ChatFocus =>
                {
                    self.save_active_document(window, cx);
                }
                "editor.open_external" => self.open_workspace_in_preferred_editor(None, cx),
                "settings.agents" => self.toggle_settings(SettingsTab::Agents, cx),
                "settings.skills_mcp" => self.toggle_settings(SettingsTab::SkillsMcp, cx),
                "settings.usage" => self.toggle_settings(SettingsTab::Usage, cx),
                "settings.schedules" => self.toggle_settings(SettingsTab::Schedules, cx),
                "settings.keybindings" => self.toggle_settings(SettingsTab::Keybindings, cx),
                "settings.devices" => self.toggle_settings(SettingsTab::Devices, cx),
                "settings.remote" => self.toggle_settings(SettingsTab::Remote, cx),
                "settings.logs" => self.toggle_settings(SettingsTab::Logs, cx),
                _ => dispatched = false,
            }
            if dispatched {
                cx.stop_propagation();
                return;
            }
        }

        if cmd {
            // Note: `⌘B` is intentionally NOT hijacked here so Word/Excel/PPT retain native Bold!
            let handled = match key.to_ascii_lowercase().as_str() {
                "k" => {
                    self.toggle_command_palette(window, cx);
                    true
                }
                "," => {
                    self.toggle_settings(self.active_settings_tab, cx);
                    true
                }
                "n" => {
                    self.start_new_chat(window, cx);
                    true
                }
                "o" => {
                    self.pick_documents(window, cx);
                    true
                }
                "s" if shift
                    && self.active_office_tab.is_some()
                    && self.right_surface == RightPaneSurface::Office
                    && self.layout_mode != WorkspaceLayoutMode::ChatFocus =>
                {
                    self.prompt_save_as_active_document(window, cx);
                    true
                }
                "s" if self.active_office_tab.is_some()
                    && self.right_surface == RightPaneSurface::Office
                    && self.layout_mode != WorkspaceLayoutMode::ChatFocus =>
                {
                    self.save_active_document(window, cx);
                    true
                }
                "." | "\\" => {
                    if self.sidebar_open && self.sidebar_focus.contains_focused(window, cx) {
                        self.focus_handle.focus(window, cx);
                    }
                    self.toggle_sidebar(cx);
                    true
                }
                "j" => {
                    self.toggle_document_pane(cx);
                    if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
                        self.composer_editor
                            .update(cx, |ed, cx| ed.focus(window, cx));
                    }
                    true
                }
                "g" if shift => {
                    self.open_git_workbench_tab(cx);
                    true
                }
                "e" if shift => {
                    self.open_files_dock_tab(cx);
                    true
                }
                "t" if shift => {
                    self.open_terminal_dock_tab(cx);
                    true
                }
                "m" if shift => {
                    self.toggle_settings(SettingsTab::SkillsMcp, cx);
                    true
                }
                "u" if shift => {
                    self.toggle_settings(SettingsTab::Usage, cx);
                    true
                }
                "l" if shift => {
                    self.toggle_settings(SettingsTab::Logs, cx);
                    true
                }
                _ => false,
            };
            if handled {
                cx.stop_propagation();
                return;
            }
        }

        if key == "escape"
            && (self.command_palette_open
                || self.settings_open
                || self.provider_sheet_open
                || self.remote_sheet_open
                || self.schedules_sheet_open
                || self.skills_sheet_open
                || self.usage_sheet_open
                || self.handoff_sheet_open
                || self.side_question_open
                || self.logs_sheet_open
                || self.open_menu.is_some())
        {
            self.close_all_overlays(cx);
            cx.stop_propagation();
        } else if key == "escape" && self.turn_review.is_some() {
            self.close_file_review(window, cx);
            cx.stop_propagation();
        }
    }

    // ── Modals (`deferred` priority 4 above toolbars & popovers) ────────────

    pub(crate) fn render_modal(
        &self,
        id: &'static str,
        open: bool,
        top: f32,
        card: AnyElement,
        on_close: fn(&mut Self, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.weak_entity();
        pro_presence(id, open, 0.17, 0.12, move |amount| {
            div()
                .absolute()
                .inset_0()
                .child(
                    deferred(
                        div()
                            .id(id)
                            .size_full()
                            .bg(hsla(0., 0., 0., 0.45 * amount))
                            .flex()
                            .flex_col()
                            .items_center()
                            .pt(px(top + 8. * (1. - amount)))
                            .px(px(20.))
                            .when(open, |d| {
                                let weak = weak.clone();
                                d.occlude().on_mouse_down(
                                    MouseButton::Left,
                                    move |_, window, cx| {
                                        if let Some(entity) = weak.upgrade() {
                                            entity.update(cx, |this, cx| {
                                                on_close(this, window, cx);
                                                cx.notify();
                                            });
                                        }
                                    },
                                )
                            })
                            .child(
                                div()
                                    .max_w_full()
                                    .opacity(amount)
                                    .when(open, |d| {
                                        d.on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                    })
                                    .child(card),
                            ),
                    )
                    .with_priority(4),
                )
                .into_any_element()
        })
    }

    fn move_palette_selection(&mut self, forward: bool, count: usize, cx: &mut Context<Self>) {
        if count > 0 {
            self.cmd_selected_ix = if forward {
                (self.cmd_selected_ix + 1) % count
            } else {
                self.cmd_selected_ix.checked_sub(1).unwrap_or(count - 1).min(count - 1)
            };
            self.reveal_cmd_selection = true;
            cx.notify();
        }
        cx.stop_propagation();
    }

    fn palette_option(
        id: impl Into<ElementId>,
        index: usize,
        active: bool,
        s: &StudioPalette,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        // Arrow keys operate from the search field. Pointer movement changes the same
        // active option, so Enter always opens the row that is visibly highlighted.
        div()
            .id(id)
            .role(Role::ListBoxOption)
            .aria_selected(active)
            .when(active, |row| row.aria_active_descendant().bg(s.active))
            .w_full()
            .h(px(32.))
            .flex_none()
            .px(px(10.))
            .rounded(px(R_CTRL))
            .flex()
            .items_center()
            .gap(px(10.))
            .cursor_pointer()
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.cmd_selected_ix != index {
                    this.cmd_selected_ix = index;
                    cx.notify();
                }
            }))
    }

    fn render_command_palette_modal(
        &mut self,
        s: &StudioPalette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let viewport = window.viewport_size();
        let items = self.filtered_palette_items();
        let trimmed = self.cmd_query.trim().to_owned();
        let searching = !trimmed.is_empty();
        let has_ask = searching && self.can_prompt_from_palette();
        let hit_count = if searching {
            self.global_search_hits.len().min(12)
        } else {
            0
        };
        let total = items.len() + hit_count + usize::from(has_ask);
        let selected = if total > 0 {
            self.cmd_selected_ix.min(total - 1)
        } else {
            0
        };

        let mut list = div()
            .id("kar-cmd-palette-scroll")
            .role(Role::ListBox)
            .aria_label(loc.pick("命令结果", "Command results"))
            .track_scroll(&self.cmd_scroll)
            .min_h_0()
            .flex_1()
            .overflow_y_scroll()
            .px(px(8.))
            .py(px(6.))
            .flex()
            .flex_col()
            .gap(px(1.));
        let mut selected_child = selected;
        let mut current_section = "";
        for (ix, item) in items.iter().enumerate() {
            if item.section != current_section {
                current_section = item.section;
                if ix <= selected {
                    selected_child += 1;
                }
                list = list.child(
                    div()
                        .px(px(10.))
                        .flex_none()
                        .pt(px(if ix == 0 { 2. } else { 10. }))
                        .pb(px(4.))
                        .text_size(s.font_size(FS_META))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.muted)
                        .child(match item.section {
                            "对话" => loc.pick("对话", "Threads"),
                            "工作区侧栏" => loc.pick("工作区", "Workspace"),
                            "视图" => loc.pick("视图", "View"),
                            "系统与设置" => loc.pick("设置", "Settings"),
                            "文档" => loc.pick("文档", "Documents"),
                            "智能体与共享" => loc.pick("共享", "Sharing"),
                            _ => loc.pick("示例", "Examples"),
                        }),
                );
            }
            let cmd_id = item.id;
            let active = ix == selected;
            list = list.child(
                Self::palette_option(("cmd-palette-item", ix), ix, active, s, cx)
                    .child(pro_icon(
                        item.icon,
                        ICON_SM,
                        if active { s.ink } else { s.muted },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(s.font_size(FS_UI))
                            .line_height(s.line_height(LH_UI))
                            .text_color(if active { s.ink } else { s.text })
                            .child(loc.pick(item.label, item.label_en)),
                    )
                    .when(!item.shortcut.is_empty(), |row| {
                        row.child(render_kbd(item.shortcut, s))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.accept_command_palette(window, cx);
                        this.execute_command_id(cmd_id, window, cx);
                    })),
            );
        }

        if searching && !self.global_search_hits.is_empty() {
            if selected >= items.len() {
                selected_child += 1;
            }
            list = list.child(
                div()
                    .px(px(10.))
                    .pt(px(if items.is_empty() { 2. } else { 10. }))
                    .pb(px(4.))
                    .text_size(s.font_size(FS_META))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.muted)
                    .child(loc.pick("工作区文件与历史对话命中", "Files and threads")),
            );
            for (h_ix, hit) in self.global_search_hits.iter().take(12).enumerate() {
                let hit_active = selected == items.len() + h_ix;
                match hit {
                    GlobalSearchHit::FilePath { rel_path } => {
                        let rel = rel_path.clone();
                        let label = rel.display().to_string();
                        list = list.child(
                            Self::palette_option(("cmd-hit-path", h_ix), items.len() + h_ix, hit_active, s, cx)
                                .child(pro_icon(ProIcon::PageDoc, ICON_SM, s.pro.accent_info))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(s.font_size(FS_META))
                                        .text_color(s.ink)
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .text_size(s.font_size(FS_TAG))
                                        .text_color(s.muted)
                                        .child(loc.pick("打开文件", "Open file")),
                                )
                                .on_click(cx.listener(move |this, _, win, cx| {
                                    this.accept_command_palette(win, cx);
                                    this.open_workspace_any_file(rel.clone(), win, cx);
                                })),
                        );
                    }
                    GlobalSearchHit::FileContent {
                        rel_path,
                        line,
                        snippet,
                    } => {
                        let rel = rel_path.clone();
                        let title = format!("{}:{line}", rel.display());
                        let sub = snippet.clone();
                        list = list.child(
                            Self::palette_option(("cmd-hit-line", h_ix), items.len() + h_ix, hit_active, s, cx)
                                .child(pro_icon(ProIcon::Search, ICON_SM, s.muted))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(s.font_size(FS_META))
                                        .text_color(s.ink)
                                        .child(format!("{title}  —  {sub}")),
                                )
                                .on_click(cx.listener(move |this, _, win, cx| {
                                    this.accept_command_palette(win, cx);
                                    this.open_workspace_any_file(rel.clone(), win, cx);
                                })),
                        );
                    }
                    GlobalSearchHit::ChatMessage {
                        chat_id,
                        chat_title,
                        role_label,
                        snippet,
                    } => {
                        let cid = *chat_id;
                        let desc = format!("[{role_label}] {chat_title}: {snippet}");
                        list = list.child(
                            Self::palette_option(("cmd-hit-msg", h_ix), items.len() + h_ix, hit_active, s, cx)
                                .child(pro_icon(ProIcon::Sparkles, ICON_SM, s.pro.purple))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(s.font_size(FS_META))
                                        .text_color(s.ink)
                                        .child(desc),
                                )
                                .on_click(cx.listener(move |this, _, win, cx| {
                                    this.accept_command_palette(win, cx);
                                    this.open_chat(cid, win, cx);
                                })),
                        );
                    }
                }
            }
        }

        if has_ask {
            let ask_ix = items.len() + hit_count;
            let active = selected == ask_ix;
            if active {
                selected_child += 1;
            }
            let q_for_send = trimmed.clone();
            list = list
                .child(
                    div()
                        .px(px(10.))
                        .pt(px(if items.is_empty() { 2. } else { 10. }))
                        .pb(px(4.))
                        .text_size(s.font_size(FS_META))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.muted)
                        .child(loc.pick("立即提问", "Ask in thread")),
                )
                .child(
                    Self::palette_option("cmd-palette-ask-agent", ask_ix, active, s, cx)
                        .child(pro_icon(
                            ProIcon::Sparkles,
                            ICON_SM,
                            if active { s.ink } else { s.muted },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(s.font_size(FS_UI))
                                .text_color(s.ink)
                                .child(format!(
                                    "{} “{trimmed}”",
                                    loc.pick("发送给智能体", "Ask the agent")
                                )),
                        )
                        .child(render_kbd("↵", s))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.accept_command_palette(window, cx);
                            this.fill_starter_prompt(&q_for_send, window, cx);
                            this.submit_composer_prompt(window, cx);
                        })),
                );
        }

        if total == 0 {
            list = list.child(
                div()
                    .p(px(12.))
                    .text_size(s.font_size(FS_UI))
                    .text_color(s.muted)
                    .child(loc.pick(
                        "没有匹配的命令、文件或对话",
                        "No matching commands, files, or threads",
                    )),
            );
        }
        if std::mem::take(&mut self.reveal_cmd_selection) {
            self.cmd_scroll.scroll_to_item(selected_child);
        }

        let card = crate::ui_primitives::frosted_panel(&s)
            .key_context(PALETTE_KEY_CONTEXT)
            .on_action(cx.listener(move |this, _: &NextPaletteOption, _, cx| {
                this.move_palette_selection(true, total, cx);
            }))
            .on_action(cx.listener(move |this, _: &PreviousPaletteOption, _, cx| {
                this.move_palette_selection(false, total, cx);
            }))
            .w(px((f32::from(viewport.width) - 24.).min(576.)))
            .h(px((f32::from(viewport.height) * 0.85).min(420.)))
            .max_w_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(52.))
                    .flex_none()
                    .px(px(14.))
                    .border_b_1()
                    .border_color(s.line_soft)
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(pro_icon(ProIcon::Search, ICON_SM, s.muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(28.))
                            .child(self.cmd_query_editor.clone()),
                    ),
            )
            .child(list)
            .child(
                div()
                    .h(px(38.))
                    .flex_none()
                    .rounded_b(px(R_MODAL))
                    .px(px(14.))
                    .bg(s.stage)
                    .border_t_1()
                    .border_color(s.line_soft)
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(s.font_size(FS_META))
                    .line_height(s.line_height(LH_META))
                    .text_color(s.muted)
                    .child(loc.pick(
                        "↑↓  选择      ↵  执行      Esc  关闭",
                        "↑↓  Navigate      ↵  Select      Esc  Close",
                    )),
            )
            .into_any_element();

        self.render_modal(
            "kar-command-palette",
            self.command_palette_open,
            f32::from(viewport.height) * 0.1,
            card,
            |this, window, cx| this.close_command_palette(window, cx),
            cx,
        )
    }

    // ── ToWork-inspired Client Store, Interactive Briefs, Remote Link & Schedules ─

    fn sync_client_projection(
        &mut self,
        proj: &ProjectedChatState,
    ) {
        self.client_reducer.sync_background_commands(&proj.turns);

        let previous = if self.projected.chat_id == proj.chat_id {
            self.projected.turns.as_slice()
        } else {
            &[]
        };
        self.client_reducer.stream_budget.sync_tool_snapshots(
            proj.chat_id.unwrap_or_default(), previous, &proj.turns,
        );

        // Keep pending-approval tool details protected in the 64 MiB LRU cache
        let has_pending = !proj.pending_requests.is_empty();
        if !has_pending && self.tool_detail.is_none() {
            self.client_reducer.detail_cache.release_all_unlocked();
        }

    }

    fn record_immutable_turn_capture(
        &mut self,
        turn_id: Uuid,
        target: Option<&SelectionTarget>,
        _attachments: &[PathBuf],
    ) {
        let mut office_revs = BTreeMap::new();
        for (doc_name, rev) in &self.fence_ledger.revisions {
            office_revs.insert(doc_name.clone(), *rev);
        }
        let chat_id = self.current_chat_id();
        let association =
            chat_id.and_then(|cid| self.client_reducer.association_for(&cid).cloned());
        let info = self.projected.info.as_ref();
        let cap = TurnCapture {
            turn_id,
            chat_id,
            runtime_id: self.effective_runtime(),
            model: info.map_or_else(|| self.selected_model.clone(), |i| i.model.clone()),
            effort: info.map_or_else(
                || self.selected_effort().map(str::to_owned),
                |i| i.settings.effort.clone(),
            ),
            approval_pretty: TurnCapture::approval_label(
                info.map_or(self.approval_mode, |i| i.approvals),
            )
            .to_owned(),
            workspace: self.effective_workspace_dir(),
            target_selection: target.cloned(),
            approved_plan: self.current_plan_was_approved(chat_id, turn_id),
            document_revisions: office_revs,
            association,
            captured_at_ms: now_epoch_ms(),
        };
        self.client_reducer.record_turn_capture(cap);
        self.sync_host_extended_snapshot();
    }

    pub(crate) fn sync_host_extended_snapshot(&self) {
        if self.active_http_server_url.is_none() && self.remote_telemetry.listen_url.is_none() {
            return;
        }
        let chat_id = self.current_chat_id();
        let association = chat_id.and_then(|cid| self.client_reducer.association_for(&cid).cloned());
        let snap = RemoteExtendedSnapshot {
            host_name: String::new(),
            chat_id,
            association,
            briefs: self.active_briefs.iter().filter(|brief|brief.chat_id==chat_id).cloned().collect(),
            queued_inputs: None,
            turn_captures: self.client_reducer.turn_captures.iter().filter(|(_,capture)|capture.chat_id==chat_id)
                .map(|(id,capture)|(*id,capture.clone())).collect(),
            revision: 0,
        };
        self.bridge
            .send(KarCommand::SyncHostExtendedSnapshot { snapshot: snap });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_brief_for_active_chat(
        &mut self,
        id: Option<String>,
        expected_revision: Option<u64>,
        title: String,
        summary: String,
        badge: Option<String>,
        blocks: Vec<BriefBlock>,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<KarBrief> {
        let now_ms = now_epoch_ms();
        let chat_id = self.current_chat_id();
        let turn_id = self.projected.turns.last().map(|t| t.id);
        let mut inv = BriefInventory {
            briefs: self.active_briefs.clone(),
        };
        let brief = inv
            .write_brief(
                id,
                expected_revision,
                chat_id,
                turn_id,
                title,
                summary,
                badge,
                blocks,
                now_ms,
            )?
            .clone();
        if let Some(cid) = chat_id {
            let _ = inv.save(&self.config.chat_dir, &cid.to_string());
        }
        self.active_briefs = inv.briefs;
        self.brief_widgets
            .viewed_revision
            .insert(brief.id.clone(), brief.latest_revision);
        self.sync_host_extended_snapshot();
        self.chat_scroll.scroll_to_bottom();
        cx.notify();
        Ok(brief)
    }

    pub fn insert_showcase_brief(&mut self, cx: &mut Context<Self>) {
        self.open_menu = None;
        let sample = build_showcase_kui_brief(None);
        let snap = sample.current().clone();
        let id = format!("brief-{}", &Uuid::new_v4().to_string()[..6]);
        if let Ok(b) = self.upsert_brief_for_active_chat(
            Some(id),
            None,
            snap.title,
            snap.summary,
            snap.badge,
            snap.blocks,
            cx,
        ) {
            self.notify_toast(format!("已插入示例简报「{}」", b.current().title), cx);
        }
    }

    pub fn select_brief_revision(&mut self, rev: u64, cx: &mut Context<Self>) {
        if let Some(first) = self.active_briefs.first() {
            self.brief_widgets
                .viewed_revision
                .insert(first.id.clone(), rev);
            cx.notify();
        }
    }

    pub fn submit_brief_review(
        &mut self,
        brief_id: &str,
        rev_num: u64,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(brief) = self.active_briefs.iter().find(|b| b.id == brief_id).cloned() else {
            return;
        };

        let Some(revision) = brief.at_revision(Some(rev_num)) else {
            return;
        };
        if brief.submitted_revisions.contains(&rev_num) || !self.access_mode.is_writable() {
            return;
        }
        let formatted = {
            let draft = self
                .brief_widgets
                .drafts
                .entry(brief_id.to_owned())
                .or_default();
            if draft.pinned_revision.is_some_and(|r| r != rev_num)
                || draft.missing_required(revision).is_some()
            {
                return;
            }
            draft.touch_revision(rev_num);
            draft.format_reply_message(&brief)
        };
        if let Some(b_mut) = self.active_briefs.iter_mut().find(|b| b.id == brief_id) {
            b_mut.submitted_revisions.insert(rev_num);
        }
        if let Some(cid) = self.current_chat_id() {
            let inv = BriefInventory {
                briefs: self.active_briefs.clone(),
            };
            let _ = inv.save(&self.config.chat_dir, &cid.to_string());
        }

        if matches!(self.access_mode, ChatAccessMode::RemoteController { .. }) {
            let req_id = Uuid::new_v4();
            self.bridge.send(KarCommand::SendPrompt {
                request_id: req_id,
                raw_user_text: formatted.clone(),
                payload: formatted,
                target: None,
                attachments: Vec::new(),
            });
            let _ = RemoteAction::SubmitBriefFeedback {
                brief_id: brief.id,
                revision: rev_num,
                formatted_reply: String::new(),
            };
        } else if self.turn_in_progress() {
            let req_id = Uuid::new_v4();
            self.bridge.send(KarCommand::SteerOrQueuePrompt {
                request_id: req_id,
                raw_user_text: formatted.clone(),
                payload: formatted,
                target: None,
                attachments: Vec::new(),
            });
        } else if matches!(self.access_mode, ChatAccessMode::NoneSelected) {
            if self.create_new_real_chat(cx) {
                self.queued_prompt = Some(QueuedPrompt {
                    text: formatted,
                    target: None,
                    attachments: Vec::new(),
                });
            }
        } else {
            self.dispatch_prompt(formatted, None, Vec::new(), cx);
        }
        self.notify_toast(format!("已提交简报 v{rev_num} 评审结论"), cx);
        cx.notify();
    }

    pub fn start_sharing_and_issue_code(&mut self, cx: &mut Context<Self>) {
        if self.active_http_server_url.is_some() {
            self.bridge.send(KarCommand::IssuePairingCode);
        } else {
            self.start_sharing(cx);
        }
    }

    pub fn toggle_remote_sheet(&mut self, cx: &mut Context<Self>) {
        self.toggle_settings(SettingsTab::Remote, cx);
    }


    pub fn seed_guidance_demo(&mut self, cx: &mut Context<Self>) {
        let now = crate::remote_link::now_epoch_ms();
        self.projected.can_steer = true;
        self.projected.queued_inputs = vec![
            kar::chat::QueuedInput {
                needs_acknowledgement: false,
                uncertainty_acknowledged: false,
                attachments: Vec::new(),
                id: Uuid::new_v4(),
                text: "同时把 B4 单元格的年度增幅公式同步写入周会纪要第 3 段".into(),
                status: kar::chat::InputStatus::Pending,
                refusal: None,
                promotion: None,
            },
            kar::chat::QueuedInput {
                needs_acknowledgement: false,
                uncertainty_acknowledged: false,
                attachments: Vec::new(),
                id: Uuid::new_v4(),
                text: "完成后调用 kar_write_brief 输出 v3 汇总简报卡片".into(),
                status: kar::chat::InputStatus::Pending,
                refusal: None,
                promotion: None,
            },
        ];
        self.client_reducer.background_commands.insert(
            "cmd-cargo-bench-42".into(),
            crate::client_store::BackgroundedCommand {
                tool_id: "cmd-cargo-bench-42".into(),
                turn_id: Uuid::new_v4(),
                command_summary: "cargo test --workspace --test acceptance (≥30s 自动转入后台)".into(),
                started_ms: now.saturating_sub(38_000),
                promoted_ms: now.saturating_sub(8_000),
                auto_promoted_by_guidance: true,
                follow_up_requested: true,
                completed: false,
                exit_code: None,
                output_tail: "running 6 acceptance tests...".into(),
                follow_up_delivered: false,
            },
        );
        cx.notify();
    }

    pub fn submit_remote_pair_form(&mut self, cx: &mut Context<Self>) {
        if self.remote_pair_pending { return; }
        self.remote_connect_code_input
            .update(cx, |field, cx| field.set_error("", cx));
        self.remote_connect_url_input
            .update(cx, |field, cx| field.set_error("", cx));
        let url = self
            .remote_connect_url_input
            .read(cx)
            .value(cx)
            .trim()
            .to_owned();
        let code = self
            .remote_connect_code_input
            .read(cx)
            .value(cx)
            .trim()
            .to_owned();
        let dev = self
            .remote_device_name_input
            .read(cx)
            .value(cx)
            .trim()
            .to_owned();
        if url.is_empty() {
            self.notify_warning(
                self.ui_locale.pick(
                    "请填写主机上显示的地址。",
                    "Enter the address shown on the host.",
                ),
                cx,
            );
            return;
        }
        let digits: String = code
            .chars()
            .filter(|ch| !ch.is_whitespace() && !matches!(ch, '-' | '·'))
            .collect();
        if digits.len() != 6 || !digits.bytes().all(|c| c.is_ascii_digit()) {
            self.notify_warning(
                self.ui_locale.pick(
                    "请输入主机显示的六位数字。",
                    "Enter the six digits shown on the host.",
                ),
                cx,
            );
            return;
        }
        self.remote_pair_pending = true;
        self.bridge.send(KarCommand::PairWithRemoteHost {
            url,
            code: digits,
            device_name: if dev.is_empty() {
                self.ui_locale.pick("本机", "This computer").into()
            } else {
                dev
            },
            role: self.remote_requested_role,
        });
        cx.notify();
    }

    pub fn toggle_schedules_sheet(&mut self, cx: &mut Context<Self>) {
        self.toggle_settings(SettingsTab::Schedules, cx);
    }

    pub(crate) fn ensure_schedule_workspace(&mut self) -> bool {
        let workspace = self.effective_workspace_dir();
        if self.schedule_workspace.as_ref() == Some(&workspace) {
            return false;
        }
        self.schedule_workspace = Some(workspace);
        self.schedule_registry = ScheduleRegistry::default();
        self.schedule_loaded = false;
        self.schedule_error = None;
        self.schedule_creator_open = false;
        self.schedule_save_pending = false;
        self.editing_schedule_id = None;
        self.editing_schedule_revision = None;
        self.expanded_schedule_history.clear();
        self.schedule_pending_runs.clear();
        true
    }

    pub(crate) fn toggle_schedule_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_schedule_workspace();
        self.schedule_creator_open = !self.schedule_creator_open;
        self.editing_schedule_id = None;
        self.editing_schedule_revision = None;
        self.schedule_bind_active_doc = false;
        self.schedule_draft_cadence = ScheduleCadence::DailyNineAm;
        self.schedule_title_input.update(cx, |i,cx| i.set_value("",window,cx));
        self.schedule_prompt_input.update(cx, |i,cx| i.set_value("",window,cx));
        cx.notify();
    }

    pub(crate) fn refresh_schedules(
        &mut self,
        now_ms: u64,
        show_error: bool,
        _cx: &mut Context<Self>,
    ) {
        self.ensure_schedule_workspace();
        self.schedule_refresh_ms = now_ms;
        self.bridge.send(KarCommand::Workflows {workspace:self.effective_workspace_dir(),action:workflow::Action::Refresh,report_errors:show_error});
    }

    fn poll_due_schedules(&mut self, cx: &mut Context<Self>) {
        let now_ms = now_epoch_ms();
        // Wake up any snoozed threads whose reminder time has arrived
        if !self.prefs.snoozed_chats.is_empty() {
            let before = self.prefs.snoozed_chats.len();
            self.prefs.snoozed_chats.retain(|_, wake_ms| *wake_ms > now_ms);
            if self.prefs.snoozed_chats.len() < before {
                self.save_prefs();
                self.notify_toast("已唤醒到期的「稍后提醒」对话", cx);
            }
        }
        // Refresh PTY live output & sniff localhost dev ports when PTY is active
        if let Some(ref pty) = self.active_pty {
            let snap = pty.snapshot(self.theme_mode == ThemeMode::Dark);
            for port in snap.detected_ports {
                if !self.detected_dev_ports.contains(&port) {
                    self.detected_dev_ports.push(port);
                }
            }
        }
        let viewing = self.settings_open && self.active_settings_tab == SettingsTab::Schedules;
        let interval = if viewing { 2_000 } else { 30_000 };
        if self.schedule_workspace.as_ref()!=Some(&self.effective_workspace_dir()) || now_ms.saturating_sub(self.schedule_refresh_ms) >= interval {
            self.refresh_schedules(now_ms, false, cx);
        }
    }

    pub fn save_new_schedule_from_form(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let title = self.schedule_title_input.read(cx).value(cx).trim().to_owned();
        let prompt = self.schedule_prompt_input.read(cx).value(cx).trim().to_owned();
        if title.is_empty() || prompt.is_empty() {
            self.notify_warning("请填写任务标题与执行指令", cx);
            return;
        }
        let template_path = if self.schedule_bind_active_doc {
            self.active_office_tab
                .and_then(|i| self.office_tabs.get(i))
                .and_then(|t| t.path(cx))
        } else {
            None
        };
        if self.ensure_schedule_workspace() || !self.schedule_creator_open || self.schedule_save_pending {return;}
        let existing = self.editing_schedule_id.as_ref()
            .and_then(|id|self.schedule_registry.tasks.iter().find(|t| &t.id==id));
        if self.editing_schedule_id.is_some() && existing.is_none() {
            self.notify_warning("此任务已被移除，请关闭编辑器。",cx);
            return;
        }
        let expected=self.editing_schedule_revision;
        let draft = ScheduledTask {
            id: self.editing_schedule_id.clone().unwrap_or_else(||Uuid::new_v4().to_string()),
            revision: expected.unwrap_or_default(), title, prompt,
            workspace: Some(self.effective_workspace_dir()), cadence:self.schedule_draft_cadence.clone(),
            enabled:existing.is_none_or(|t|t.enabled),
            runtime_id:existing.map_or_else(||self.selected_runtime.clone(),|t|t.runtime_id.clone()),
            model:existing.map_or_else(||self.selected_model.clone(),|t|t.model.clone()),
            effort:existing.map_or_else(||self.selected_effort().map(str::to_owned),|t|t.effort.clone()),
            office_template: template_path.or_else(||existing.and_then(|t|t.office_template.clone())),
            pre_script: existing.and_then(|t|t.pre_script.clone()),
            tone:existing.map_or(crate::schedules::ScheduleToneChoice::Blue,|t|t.tone),
            next_due_ms:None, history:Vec::new(), engine_read_only:false,
        };
        self.schedule_save_pending = true;
        self.bridge.send(KarCommand::Workflows {workspace:self.effective_workspace_dir(), action:workflow::Action::Save{task:draft,expected},report_errors:true});
        cx.notify();
    }

    pub fn toggle_schedule_enabled(&mut self, task_id: &str, _cx: &mut Context<Self>) {
        if self.ensure_schedule_workspace() {return;}
        if let Ok(id)=task_id.parse()
            && let Some(task)=self.schedule_registry.tasks.iter().find(|t|t.id==task_id) {
            self.bridge.send(KarCommand::Workflows {workspace:self.effective_workspace_dir(),action:workflow::Action::Enable{id,enabled:!task.enabled},report_errors:true});
        }
    }

    pub fn trigger_schedule_now(&mut self, task_id:&str, _window:&mut Window, cx:&mut Context<Self>) {
        if self.ensure_schedule_workspace() { return; }
        if let Ok(id)=task_id.parse() {
            let entry = self.schedule_pending_runs.entry(id).or_insert_with(||(Uuid::new_v4(), false));
            if entry.1 { return; }
            entry.1=true;
            let operation=entry.0;
            self.expanded_schedule_history.insert(task_id.to_owned());
            self.bridge.send(KarCommand::Workflows {workspace:self.effective_workspace_dir(), action:workflow::Action::Run{id,operation},report_errors:true});
            cx.notify();
        }
    }

    pub fn remove_schedule_task(&mut self, task_id:&str, _cx:&mut Context<Self>) {
        if self.ensure_schedule_workspace() {return;}
        if let Ok(id)=task_id.parse() {
            self.bridge.send(KarCommand::Workflows {workspace:self.effective_workspace_dir(), action:workflow::Action::Remove{id},report_errors:true});
        }
    }

    pub fn withdraw_or_edit_queued_input(
        &mut self,
        input_id: Uuid,
        edit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.access_mode.is_writable() {
            return;
        }
        if let ChatAccessMode::RemoteController {
            url,
            peer_id,
            chat_id,
            ..
        } = &self.access_mode
        {
            if let Some(chat_id) = *chat_id {
                self.bridge.send(KarCommand::RemoteQueuedAction {
                    url: url.clone(),
                    peer_id: *peer_id,
                    chat_id,
                    input_id,
                    copy_to_draft: edit,
                });
            }
            return;
        }
        if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            let Some(index) = self
                .projected
                .queued_inputs
                .iter()
                .position(|q| q.id == input_id && q.status == kar::chat::InputStatus::Pending)
            else {
                return;
            };
            let queued = self.projected.queued_inputs.remove(index);
            if edit {
                self.prepend_composer_draft(
                    DraftLocation::Demo,
                    QueuedPrompt {
                        text: queued.text,
                        ..Default::default()
                    },
                    window,
                    cx,
                );
                self.composer_editor
                    .update(cx, |ed, cx| ed.focus(window, cx));
            }
            cx.notify();
        } else if let Some(chat_id) = self.current_chat_id() {
            self.bridge.send(KarCommand::WithdrawQueued {
                chat_id,
                input_id,
                edit,
            });
        }
    }

    /// Resolve a live Revision-Fenced Office conflict (`FenceLedger`).
    pub fn resolve_fenced_conflict(
        &mut self,
        op_id: Uuid,
        apply_agent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(conflict) = self.fence_ledger.resolve_fenced_conflict(op_id) else {
            return;
        };
        if apply_agent {
            if let Some(tab) = self
                .office_tabs
                .iter()
                .find(|t| clean_display_doc_name(&t.doc_name(cx)) == conflict.doc_key)
                .cloned()
                && let Some(ref payload) = conflict.pending_mutation
            {
                let res = match payload {
                    FenceMutationPayload::WordParagraph {
                        paragraph_1based,
                        text,
                    } => tab.replace_word_paragraph(
                        paragraph_1based.saturating_sub(1),
                        text,
                        window,
                        cx,
                    ),
                    FenceMutationPayload::ExcelCell {
                        sheet_idx,
                        cell_a1,
                        value,
                    } => tab
                        .write_excel_cell(*sheet_idx, cell_a1, value, window, cx)
                        .map(|_| ()),
                    FenceMutationPayload::PptShapeText {
                        slide_1based,
                        shape_id,
                        text,
                    } => tab.update_ppt_shape(
                        slide_1based.saturating_sub(1),
                        *shape_id,
                        Some(text.as_str()),
                        None,
                        window,
                        cx,
                    ),
                };
                if let Err(err) = res {
                    self.notify_warning(format!("应用修改失败：{err:#}"), cx);
                    return;
                }
            }
            let _ = self.fence_ledger.commit_applied(
                &conflict.doc_key,
                conflict.kind,
                conflict.operation_id,
                conflict.live_revision,
                &conflict.proposed_summary,
                now_epoch_ms(),
            );
            self.fence_ledger.save(&self.config.docs_dir);
            self.notify_toast("已确认并应用智能体提案，Revision Fence 已前推", cx);
        } else {
            self.fence_ledger.save(&self.config.docs_dir);
            self.notify_toast("已保留你的人工版本，忽略过期的智能体提案", cx);
        }
        cx.notify();
    }

    // ── Studio Hub Actions (Git, Code Tabs, Skills/MCP, Usage, Handoff, Logs) ─

    pub fn close_all_overlays(&mut self, cx: &mut Context<Self>) {
        self.command_palette_open = false;
        self.settings_open = false;
        self.provider_sheet_open = false;
        self.remote_sheet_open = false;
        self.schedules_sheet_open = false;
        self.skills_sheet_open = false;
        self.usage_sheet_open = false;
        self.handoff_sheet_open = false;
        self.side_question_open = false;
        self.logs_sheet_open = false;
        self.open_menu = None;
        cx.notify();
    }

    pub fn active_or_selected_runtime_id(&self) -> &str {
        self.current_chat_info()
            .map_or(&self.selected_runtime, |info| info.runtime.as_str())
    }

    pub(crate) fn open_surface(&mut self, surface: &RightPaneSurface, cx: &mut Context<Self>) {
        match surface {
            RightPaneSurface::GitWorkbench => self.open_git_workbench_tab(cx),
            RightPaneSurface::Files => self.open_files_dock_tab(cx),
            RightPaneSurface::Terminal => self.open_terminal_dock_tab(cx),
            RightPaneSurface::Office | RightPaneSurface::CodeFile(_) => {}
        }
    }

    pub(crate) fn close_surface(&mut self, surface: &RightPaneSurface, cx: &mut Context<Self>) {
        match surface {
            RightPaneSurface::GitWorkbench => self.git_tab_open = false,
            RightPaneSurface::Files => self.files_tab_open = false,
            RightPaneSurface::Terminal => self.terminal_tab_open = false,
            _ => return,
        }
        if self.right_surface == *surface {
            self.right_surface = self.remaining_dock_surface();
        }
        cx.notify();
    }

    fn remaining_dock_surface(&self) -> RightPaneSurface {
        if self.git_tab_open {
            RightPaneSurface::GitWorkbench
        } else if self.files_tab_open {
            RightPaneSurface::Files
        } else if self.terminal_tab_open {
            RightPaneSurface::Terminal
        } else if let Some(path) = self.code_tab_order.last() {
            RightPaneSurface::CodeFile(path.clone())
        } else {
            RightPaneSurface::Office
        }
    }

    pub fn open_git_workbench_tab(&mut self, cx: &mut Context<Self>) {
        self.dismiss_compact_sidebar(cx);
        self.close_all_overlays(cx);
        self.main_stage_view = MainStageView::Conversation;
        self.git_tab_open = true;
        self.right_surface = RightPaneSurface::GitWorkbench;
        if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
            self.set_layout_mode(WorkspaceLayoutMode::Split, cx);
        }
        self.bridge.send(KarCommand::RefreshGitWorkspace {
            workspace: self.effective_workspace_dir(),
        });
        cx.notify();
    }

    pub fn open_files_dock_tab(&mut self, cx: &mut Context<Self>) {
        self.dismiss_compact_sidebar(cx);
        self.close_all_overlays(cx);
        self.main_stage_view = MainStageView::Conversation;
        self.files_tab_open = true;
        self.right_surface = RightPaneSurface::Files;
        if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
            self.set_layout_mode(WorkspaceLayoutMode::Split, cx);
        }
        if self.workspace_tree.is_empty() {
            self.bridge.send(KarCommand::RefreshWorkspaceTree {
                workspace: self.effective_workspace_dir(),
                show_hidden: self.tree_show_hidden,
            });
        }
        cx.notify();
    }

    pub fn open_terminal_dock_tab(&mut self, cx: &mut Context<Self>) {
        self.dismiss_compact_sidebar(cx);
        self.set_bottom_terminal_open(false, cx);
        self.close_all_overlays(cx);
        self.main_stage_view = MainStageView::Conversation;
        self.terminal_tab_open = true;
        self.right_surface = RightPaneSurface::Terminal;
        if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
            self.set_layout_mode(WorkspaceLayoutMode::Split, cx);
        }
        cx.notify();
    }

    pub fn close_git_workbench_tab(&mut self, cx: &mut Context<Self>) {
        self.close_surface(&RightPaneSurface::GitWorkbench, cx);
    }

    pub fn open_workspace_code_tab(&mut self, rel_path: PathBuf, cx: &mut Context<Self>) {
        if self.open_code_tabs.contains_key(&rel_path) {
            self.right_surface = RightPaneSurface::CodeFile(rel_path);
            if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
                self.set_layout_mode(WorkspaceLayoutMode::Split, cx);
            }
            cx.notify();
            return;
        }
        self.bridge.send(KarCommand::ReadWorkspaceFile {
            workspace: self.effective_workspace_dir(),
            rel_path,
        });
    }

    pub fn open_workspace_any_file(
        &mut self,
        rel_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ws = self.effective_workspace_dir();
        let abs = if rel_path.is_absolute() {
            rel_path.clone()
        } else {
            ws.join(&rel_path)
        };
        if OfficeKind::for_path(&abs).is_some() {
            self.open_office_file_tab(abs, false, None, window, cx);
        } else {
            self.open_workspace_code_tab(rel_path, cx);
        }
    }

    pub fn close_workspace_code_tab(&mut self, rel_path: &Path, cx: &mut Context<Self>) {
        self.open_code_tabs.remove(rel_path);
        self.code_tab_order.retain(|p| p != rel_path);
        if self.right_surface == RightPaneSurface::CodeFile(rel_path.to_path_buf()) {
            if let Some(next_code) = self.code_tab_order.last().cloned() {
                self.right_surface = RightPaneSurface::CodeFile(next_code);
            } else if !self.office_tabs.is_empty() {
                self.right_surface = RightPaneSurface::Office;
            } else {
                self.right_surface = self.remaining_dock_surface();
            }
        }
        cx.notify();
    }

    pub fn toggle_skills_sheet(&mut self, cx: &mut Context<Self>) {
        self.toggle_settings(SettingsTab::SkillsMcp, cx);
    }

    pub fn toggle_usage_sheet(&mut self, cx: &mut Context<Self>) {
        self.toggle_settings(SettingsTab::Usage, cx);
    }

    pub fn toggle_handoff_sheet(&mut self, cx: &mut Context<Self>) {
        let next = !self.handoff_sheet_open;
        self.close_all_overlays(cx);
        self.handoff_sheet_open = next;
        if next {
            let cur = self.active_or_selected_runtime_id();
            if self.handoff_target_runtime == cur {
                self.handoff_target_runtime = if cur == "codex" {
                    "claude".into()
                } else {
                    "codex".into()
                };
            }
        }
        cx.notify();
    }

    pub fn toggle_side_question_sheet(&mut self, cx: &mut Context<Self>) {
        if !self.supports_side_question() {return;}
        let next = !self.side_question_open;
        self.close_all_overlays(cx);
        self.side_question_open = next;
        cx.notify();
    }

    pub fn toggle_logs_sheet(&mut self, cx: &mut Context<Self>) {
        self.toggle_settings(SettingsTab::Logs, cx);
    }

    pub fn stage_deferred_chat_delete(
        &mut self,
        chat_id: Uuid,
        title: String,
        cx: &mut Context<Self>,
    ) {
        // Commit any prior pending deletion immediately so at most one is staged
        if let Some((prev_id, _, _)) = self.deferred_delete_chat.take() {
            self.bridge.send(KarCommand::DeleteChat { chat_id: prev_id });
        }
        self.pending_delete_chat = None;
        self.deferred_delete_chat = Some((chat_id, title, Instant::now()));
        cx.notify();
    }

    pub fn undo_deferred_chat_delete(&mut self, cx: &mut Context<Self>) {
        if let Some((_, title, _)) = self.deferred_delete_chat.take() {
            self.notify_toast(format!("已撤销删除「{title}」"), cx);
            cx.notify();
        }
    }

    fn poll_deferred_delete_chat(&mut self, cx: &mut Context<Self>) {
        if let Some((cid, _, started)) = self.deferred_delete_chat.as_ref()
            && started.elapsed() >= Duration::from_secs(5)
        {
            let cid = *cid;
            self.deferred_delete_chat = None;
            self.bridge.send(KarCommand::DeleteChat { chat_id: cid });
            cx.notify();
        }
    }

    pub(crate) fn rebuild_workspace_tree_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        fn to_kui_node(item: &WorkspaceTreeItem) -> TreeNode {
            TreeNode::new(
                item.id.clone(),
                item.name.clone(),
                item.children.iter().map(to_kui_node).collect(),
            )
            .with_icon(if item.is_dir { ProIcon::Folder } else { ProIcon::PageDoc })
        }
        let nodes: Vec<TreeNode> = self.workspace_tree.iter().map(to_kui_node).collect();
        let first_expanded: Vec<SharedString> = self
            .workspace_tree
            .iter()
            .filter(|i| i.is_dir)
            .take(2)
            .map(|i| SharedString::from(i.id.clone()))
            .collect();
        let tv = cx.new(|cx| TreeView::new(nodes, cx).expanded(first_expanded).embedded());
        cx.subscribe_in(
            &tv,
            window,
            |this, _, ev: &TreeActivated, window, cx| {
                let rel = PathBuf::from(ev.0.as_ref());
                let workspace = this.effective_workspace_dir();
                match workspace.join(&rel).metadata() {
                    Ok(entry) if entry.is_file() => this.open_workspace_any_file(rel, window, cx),
                    Ok(_) => {}
                    Err(err) => {
                        let message = this.ui_locale.pick("无法打开", "Cannot open");
                        this.notify_warning(format!("{message} {}: {err}", rel.display()), cx);
                        this.bridge.send(KarCommand::RefreshWorkspaceTree {
                            workspace,
                            show_hidden: this.tree_show_hidden,
                        });
                    }
                }
            },
        )
        .detach();
        self.workspace_tree_view = Some(tv);
    }
}

impl Render for KarWorkbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let overlay_open = self.settings_open || self.handoff_sheet_open || self.side_question_open;
        if overlay_open && self.overlay_return_focus.is_none() {
            self.overlay_return_focus = Some(
                window
                    .focused(cx)
                    .unwrap_or_else(|| self.focus_handle.clone()),
            );
            self.focus_handle.focus(window, cx);
        } else if !overlay_open && let Some(origin) = self.overlay_return_focus.take()
            && !self.command_palette_open && !self.any_dialog_open(cx) {
            origin.focus(window, cx);
        }
        let s = StudioPalette::get(cx);
        if self.toast.as_ref().is_some_and(ToastState::expired) {
            self.toast = None;
        }
        // Bug #12 Fix: poll deferred chat deletion at root level so it completes even if sidebar is collapsed (`⌘.`)
        if self.deferred_delete_chat.is_some() {
            window.request_animation_frame();
            self.poll_deferred_delete_chat(cx);
        }

        // Keep both working panes usable when an already-split window is resized.
        let window_w = f32::from(window.viewport_size().width);
        if self.pane_drag.is_some() && (self.previous_window_width - window_w).abs() > 0.1 {
            self.finish_divider(true, cx);
        }
        let compact = window_w < COMPACT_WORKSPACE_WIDTH;
        if compact != (self.previous_window_width < COMPACT_WORKSPACE_WIDTH) {
            self.thread_details_open = if compact {
                self.prefs.compact_thread_details_open
            } else {
                self.prefs.thread_details_open
            };
            if !self.thread_details_open {
                self.dismiss_thread_details_menu();
            }
        }
        let crossed_compact = compact && self.previous_window_width >= COMPACT_WORKSPACE_WIDTH;
        let crossed_split = self.layout_mode != WorkspaceLayoutMode::ChatFocus
            && window_w < AUTO_COLLAPSE_RAIL_WIDTH
            && self.previous_window_width >= AUTO_COLLAPSE_RAIL_WIDTH;
        if self.sidebar_open && (crossed_compact || crossed_split) {
            self.set_sidebar_open(false, cx);
            self.restore_sidebar_after_resize = true;
        } else if self.restore_sidebar_after_resize
            && !compact
            && (self.layout_mode == WorkspaceLayoutMode::ChatFocus
                || window_w >= AUTO_COLLAPSE_RAIL_WIDTH)
        {
            self.restore_sidebar_after_resize = false;
            self.set_sidebar_open(true, cx);
        }
        self.previous_window_width = window_w;
        let drawer_active = compact && self.sidebar_open;
        if drawer_active
            && self.drawer_return_focus.is_none()
            && !overlay_open
            && !self.command_palette_open
        {
            let origin = window
                .focused(cx)
                .unwrap_or_else(|| self.focus_handle.clone());
            self.drawer_return_focus = Some((self.main_stage_view, origin));
            self.sidebar_search
                .update(cx, |ed, cx| ed.focus(window, cx));
        } else if !drawer_active
            && let Some((stage, origin)) = self.drawer_return_focus.take()
            && !overlay_open
            && !self.command_palette_open
            && !self.any_dialog_open(cx)
            && !self.composer_editor.read(cx).focus.is_focused(window)
        {
            if stage == self.main_stage_view {
                origin
            } else {
                self.focus_handle.clone()
            }
            .focus(window, cx);
        }
        let dock_closed = self.layout_mode == WorkspaceLayoutMode::ChatFocus;
        let dock_reveal = self.dock_reveal.get(window, cx).clamp(0., 1.);
        let sidebar_limit = if compact {
            SIDEBAR_W
        } else {
            let wide = resize::sidebar_max(window_w, false);
            wide + (resize::sidebar_max(window_w, true) - wide) * dock_reveal
        };
        let rail_width = if compact {
            SIDEBAR_W
        } else {
            self.sidebar_width.min(sidebar_limit)
        };
        self.sidebar_reveal =
            (self.sidebar_anim.get(window, cx) / self.sidebar_width).clamp(0., 1.);
        let sidebar_w = self.sidebar_reveal * rail_width;
        let in_conversation_stage = self.main_stage_view == MainStageView::Conversation;
        let show_doc = in_conversation_stage
            && (self.layout_mode != WorkspaceLayoutMode::ChatFocus
                || (!compact && dock_reveal > 0.001));
        let single_dock = compact && show_doc;
        if in_conversation_stage && !single_dock && !drawer_active && !overlay_open
            && !self.command_palette_open && !self.any_dialog_open(cx)
        {
            self.mark_visible_reply_read(window.is_window_active());
        }
        if ((single_dock || !in_conversation_stage)
            && self.composer_editor.read(cx).focus.is_focused(window))
            || (!self.sidebar_open && self.sidebar_search.read(cx).focus.is_focused(window))
        {
            self.focus_handle.focus(window, cx);
        }

        let max_term_h = (f32::from(window.viewport_size().height) - 180.).max(TERMINAL_MIN_H);
        let term_drawer_h = if in_conversation_stage {
            self.bottom_terminal_anim
                .get(window, cx)
                .clamp(0., max_term_h)
        } else {
            0.
        };

        let rail_space = if compact { 0. } else { sidebar_w };
        let usable = (window_w - rail_space).max(1.);
        let split_span = resize::chat_span((usable - SHEET_GUTTER).max(1.), self.split_anim.get(window, cx));
        // Constrain the open layout once, then reveal it from zero. Applying the document
        // minimum to the animated width would jump the divider by up to DOC_MIN_W at click.
        let dock_w = if single_dock { usable } else { split_span.extent - split_span.value };
        let revealed_width = if show_doc && !single_dock { (dock_w + SHEET_GUTTER) * dock_reveal } else { 0. };
        let chat_w = usable - revealed_width;
        let (stage_key, inner_stage, review_rail_open) = match self.main_stage_view {
            MainStageView::Conversation if single_dock => ("stage-conversation", div().into_any_element(), false),
            MainStageView::Conversation => {
                let (chat, rail) = self.render_chat_pane(chat_w, revealed_width, window, cx);
                ("stage-conversation", chat, rail)
            }
            MainStageView::TasksInbox => {
                ("stage-tasks-inbox", self.render_tasks_inbox_screen(usable, window, cx), false)
            }
            MainStageView::OfficeGallery => {
                ("stage-office-gallery", self.render_office_gallery_screen(usable, window, cx), false)
            }
        };
        let chat_view = pro_view_transition_fill("kar-main-stage-vtx", stage_key, inner_stage);
        let term_drawer =
            (term_drawer_h > 0.5).then(|| self.render_bottom_terminal_drawer(&s, window, cx));
        let doc_view = show_doc.then(|| self.render_office_pane(dock_w, single_dock, window, cx));
        let sidebar_view = (sidebar_w > 0.5).then(|| self.render_sidebar(rail_width, window, cx));

        // An in-flight split is narrower than its draggable minimum. Until the visible
        // boundary reaches that minimum, a drag would snap to a different width.
        let dividers_ready = compact || !in_conversation_stage ||
            (dock_reveal - f32::from(!dock_closed)).abs() * (dock_w + SHEET_GUTTER) <= 1.;
        let left_handle = (dividers_ready && (!compact || !self.sidebar_open)).then(|| {
            let span = Span {
                value: rail_width,
                min: resize::SIDEBAR_MIN,
                max: sidebar_limit,
                extent: window_w,
            };
            let document_width = (show_doc && self.layout_mode != WorkspaceLayoutMode::ChatFocus)
                .then_some(dock_w);
            let x = if self.sidebar_open { sidebar_w } else { 0. };
            self.workspace_divider(
                Divider::Sidebar, x, span, !self.sidebar_open, document_width, &s, window, cx,
            )
        });
        let show_right_handle = dividers_ready && in_conversation_stage && !review_rail_open && !drawer_active
            && (!compact || dock_closed);
        let right_handle = show_right_handle.then(|| {
            let x = if show_doc { rail_space + chat_w + SHEET_GUTTER * 0.5 * dock_reveal } else { window_w };
            self.workspace_divider(
                Divider::Document, x, split_span, dock_closed, None, &s, window, cx,
            )
        });

        let chat_column = div()
            .h_full()
            .bg(crate::ui_primitives::material::reading_surface(&s))
            .flex()
            .flex_col()
            .overflow_hidden()
            .map(|d| {
                if show_doc {
                    d.flex_none().w(px(chat_w))
                } else {
                    d.flex_1().min_w_0()
                }
            })
            .child(div().flex_1().min_h_0().overflow_hidden().child(chat_view))
            .when_some(term_drawer, |d, drawer| {
                d.child(
                    div()
                        .h(px(term_drawer_h))
                        .w_full()
                        .flex_none()
                        .overflow_hidden()
                        .child(drawer),
                )
            });

        // Anchor the controls to the window so resizing and moving panes cannot move the targets.
        let toggle_top = titlebar_center_y(window) - CTRL_SM_H / 2.;
        let toggle_slot = || div()
            .absolute()
            .top(px(toggle_top))
            .size(px(CTRL_SM_H))
            .occlude();
        let sidebar_label = if self.sidebar_open {
            self.ui_locale.pick("收起侧边栏  ⌘.", "Hide sidebar  ⌘.")
        } else {
            self.ui_locale.pick("展开侧边栏  ⌘.", "Show sidebar  ⌘.")
        };
        let sidebar_toggle = toggle_slot().left(px(macos_traffic_pad(window))).child(
            icon_button_sm("window-toggle-sidebar", ProIcon::SidebarLeft, sidebar_label, &s)
                .expanded(self.sidebar_open)
                .on_press(cx, |this, window, cx| {
                    this.focus_handle.focus(window, cx);
                    this.set_sidebar_open(!this.sidebar_open, cx);
                }),
        );
        let right_toggle = in_conversation_stage.then(|| {
            let review = review_rail_open;
            let open = !dock_closed || review;
            let label = if single_dock {
                self.ui_locale.pick("收起右侧面板，返回对话  ⌘J", "Hide right panel and return to thread  ⌘J")
            } else if open {
                self.ui_locale.pick("收起右侧面板  ⌘J", "Hide right panel  ⌘J")
            } else {
                self.ui_locale.pick("展开右侧面板  ⌘J", "Show right panel  ⌘J")
            };
            toggle_slot().right(px(TITLEBAR_EDGE_INSET)).child(
                icon_button_sm("window-toggle-dock", ProIcon::SidebarRight, label, &s)
                    .expanded(open)
                    .on_press(cx, move |this, window, cx| {
                        this.focus_handle.focus(window, cx);
                        this.dismiss_compact_sidebar(cx);
                        if review {
                            this.close_file_review(window, cx);
                        } else {
                            this.toggle_document_pane(cx);
                        }
                    }),
            )
        });

        let stage_inner = div()
            .id("kar-main")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_row()
            .when(!single_dock, |d| d.child(chat_column))
            .when(show_doc && !single_dock, |d| d.child(div().flex_none().h_full().w(px(SHEET_GUTTER * dock_reveal)).bg(s.line)))
            .when_some(doc_view, |d, doc| {
                d.child(
                    div()
                        .h_full()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .child(div().w(px(dock_w)).h_full().flex_none().overflow_hidden().child(doc)),
                )
            });

        let main = div()
            .flex_1()
            .min_w_0()
            .h_full()

            .flex()
            .child(stage_inner);

        let rail = sidebar_view.map(|sidebar| {
            div()
                .relative()
                .w(px(sidebar_w))
                .h_full()
                .flex_none()
                .when(compact, |d| d.bg(s.stage))
                .overflow_hidden()
                .child(
                    div()
                        .absolute()
                        .right_0()
                        .top_0()
                        .w(px(rail_width))
                        .h_full()
                        .child(sidebar),
                )
        });
        let (inline_rail, drawer_rail) = if compact { (None, rail) } else { (rail, None) };

        let command_palette = self.render_command_palette_modal(&s, window, cx);
        let settings_sheet = self.render_settings_modal(&s, window, cx);
        let handoff_sheet = self.render_handoff_modal(&s, window, cx);
        let side_question_sheet = self.render_side_question_modal(&s, window, cx);
        let toast_layer = render_toast(self.toast.as_ref(), &s, window, cx);
        let delete_undo = if self.sidebar_open { None } else { self.render_delete_undo(&s, cx) };

        div()
            .id("kar-workbench-root")
            .font_family(ui_font(cx))
            .text_color(s.ink)
            .track_focus(&self.focus_handle)
            .capture_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "escape" && !this.any_dialog_open(cx) {
                    if this.composer_dictation().is_some() && !this.composer_editor.read(cx).is_composing() {
                        this.discard_voice_dictation(cx);
                        window.prevent_default();
                        cx.stop_propagation();
                        return;
                    }
                    if this.open_menu.is_some() {
                        this.close_menu(cx);
                        cx.stop_propagation();
                        return;
                    }
                    if this.previous_window_width < COMPACT_WORKSPACE_WIDTH
                        && this.sidebar_open
                        && !this.command_palette_open
                        && !this.settings_open
                        && !this.handoff_sheet_open
                        && !this.side_question_open
                    {
                        this.set_sidebar_open(false, cx);
                        cx.stop_propagation();
                        return;
                    }
                }
                if (ev.keystroke.modifiers.platform || ev.keystroke.modifiers.control)
                    && matches!(ev.keystroke.key.as_str(), "n" | "j")
                {
                    this.handle_global_key(ev, window, cx);
                }
            }))
            .capture_key_up(cx.listener(|this, ev: &gpui::KeyUpEvent, window, cx| {
                if this.held_voice_key.as_deref() == Some(ev.keystroke.key.as_str()) {
                    this.held_voice_key = None;
                    if this.active_voice_rec.is_some() {this.toggle_voice_dictation(window, cx);}
                    cx.stop_propagation();
                }
            }))
            .on_key_down(cx.listener(Self::handle_global_key))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, window, cx| {
                if this.dragging_terminal
                    && let Some((start_y, start_h)) = this.terminal_drag_anchor
                {
                    let dy = start_y - f32::from(ev.position.y);
                    let max_h = (f32::from(window.viewport_size().height) - 180.)
                        .max(TERMINAL_MIN_H);
                    let next_h = (start_h + dy).clamp(TERMINAL_MIN_H, max_h);
                    this.bottom_terminal_h = next_h;
                    this.bottom_terminal_anim.reset(next_h);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.dragging_terminal {
                        this.dragging_terminal = false;
                        this.terminal_drag_anchor = None;
                        this.prefs.bottom_terminal_h = this.bottom_terminal_h;
                        this.save_prefs();
                        cx.notify();
                    }
                }),
            )
            .size_full()
            .relative()
            .bg(crate::ui_primitives::material::window_tint(&s))
            .flex()
            .flex_row()
            .overflow_hidden()
            .when_some(inline_rail, |d, rail| d.child(rail))
            .child(main)
            .children(left_handle)
            .children(right_handle)
            .when(self.pane_drag.is_some(), |d| d.child(self.capture_divider_drag(cx)))
            .when_some(drawer_rail, |d, rail| {
                d.child(
                    div().absolute().inset_0().occlude().flex().child(rail).child(
                        div()
                            .id("close-compact-sidebar")
                            .flex_1()
                            .h_full()
                            .bg(black().opacity(0.25))
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_sidebar_open(false, cx)),
                            ),
                    ),
                )
            })
            .child(sidebar_toggle)
            .children(right_toggle)
            .when_some(kantor_ui::overlay::menu_shield(window), |d, shield| {
                d.child(shield)
            })
            .child(command_palette)
            .child(settings_sheet)
            .child(handoff_sheet)
            .child(side_question_sheet)
            .child(self.http_dialog.clone())
            .child(self.goal_dialog.clone())
            .child(self.rename_dialog.clone())
            .child(self.branch_dialog.clone())
            .child(self.quick_edit_dialog.clone())
            .child(toast_layer)
            .when_some(delete_undo, |root, banner| root.child(
                div().absolute().top(px(52.)).left(px(2.)).w_full().max_w(px(380.)).child(banner),
            ))
    }
}

const PLACEHOLDER_NEW_CHAT_ZH: &str = "输入消息，或按 / 查看指令…";
const PLACEHOLDER_NEW_CHAT_EN: &str = "Write a message, or type / for commands…";
const PLACEHOLDER_REMOTE_CTRL: &str = "向远端智能体发送消息…";
const PLACEHOLDER_ELSEWHERE: &str = "这个对话正在另一个窗口里进行，当前窗口实时同步查看";
const PLACEHOLDER_ARCHIVED: &str = "原工作文件夹已不在本机，当前以归档只读查看";
const PLACEHOLDER_SHARED: &str = "这是别人共享的实时对话，只能查看";

fn chat_id_of(mode: &ChatAccessMode) -> Option<Uuid> {
    match mode {
        ChatAccessMode::WriteController { chat_id }
        | ChatAccessMode::LocalReadOnlySubscriber { chat_id, .. }
        | ChatAccessMode::ArchivedReadOnly { chat_id, .. } => Some(*chat_id),
        ChatAccessMode::HttpReadOnly { chat_id, .. }
        | ChatAccessMode::RemoteController { chat_id, .. } => *chat_id,
        ChatAccessMode::NoneSelected | ChatAccessMode::DemoShowcase => None,
    }
}

pub(crate) fn scenario_for_key(key: &str) -> ScenarioKey {
    match key {
        "excel_same_cell" => ScenarioKey::ExcelSameCellConflict,
        "ppt_move_text" => ScenarioKey::PptMoveAndText,
        _ => ScenarioKey::WordSameParagraph,
    }
}

struct PaletteItem {
    id: &'static str,
    label: &'static str,
    label_en: &'static str,
    section: &'static str,
    shortcut: &'static str,
    icon: ProIcon,
    keywords: &'static str,
}

const PALETTE_ITEMS: &[PaletteItem] = &[
    PaletteItem {
        id: "new_chat",
        label: "新建对话",
        label_en: "New thread",
        section: "对话",
        shortcut: "⌘N",
        icon: ProIcon::NewChat,
        keywords: "new chat 新建对话",
    },
    PaletteItem {
        id: "tasks_inbox",
        label: "打开收件箱",
        label_en: "Open inbox",
        section: "对话",
        shortcut: "",
        icon: ProIcon::Thread,
        keywords: "tasks inbox board waterfall approval 任务 待办 看板 审批 瀑布流",
    },
    PaletteItem {
        id: "bind_selection",
        label: "引用文档选区",
        label_en: "Attach document selection",
        section: "对话",
        shortcut: "",
        icon: ProIcon::Code,
        keywords: "bind selection 引用选区 定位",
    },
    PaletteItem {
        id: "pick_attachment",
        label: "添加附件…",
        label_en: "Attach a file…",
        section: "对话",
        shortcut: "",
        icon: ProIcon::Upload,
        keywords: "attach image file 附件 图片",
    },
    PaletteItem {
        id: "pick_workspace",
        label: "选择工作区…",
        label_en: "Choose workspace…",
        section: "对话",
        shortcut: "",
        icon: ProIcon::Folder,
        keywords: "workspace folder 文件夹 目录",
    },
    PaletteItem {
        id: "handoff_chat",
        label: "交接当前对话",
        label_en: "Hand off thread",
        section: "对话",
        shortcut: "",
        icon: ProIcon::ExternalArrow,
        keywords: "handoff switch relay codex claude cursor 接力 切换 移交",
    },
    PaletteItem {
        id: "side_question",
        label: "旁路提问",
        label_en: "Ask a side question",
        section: "对话",
        shortcut: "",
        icon: ProIcon::Sparkles,
        keywords: "side question ephemeral fork 旁路 快问",
    },
    PaletteItem {
        id: "insert_brief",
        label: "插入示例简报",
        label_en: "Insert sample brief",
        section: "对话",
        shortcut: "",
        icon: ProIcon::UsageChart,
        keywords: "brief kui chart sankey review 简报 图表 桑基图 评审",
    },
    PaletteItem {
        id: "context_inspector",
        label: "查看上下文用量",
        label_en: "View context usage",
        section: "对话",
        shortcut: "",
        icon: ProIcon::Memory,
        keywords: "context window capacity bands compact 上下文 词元 压缩",
    },
    PaletteItem {
        id: "git_workbench",
        label: "查看 Git 变更",
        label_en: "Open Git changes",
        section: "工作区侧栏",
        shortcut: "⇧⌘G",
        icon: ProIcon::GitBranch,
        keywords: "git diff changes branch commit stage unstage 变更 分支 提交 暂存",
    },
    PaletteItem {
        id: "files_dock",
        label: "浏览工作区文件",
        label_en: "Browse workspace files",
        section: "工作区侧栏",
        shortcut: "⇧⌘E",
        icon: ProIcon::Folder,
        keywords: "files tree explorer repo 文件树 浏览 代码仓",
    },
    PaletteItem {
        id: "terminal_dock",
        label: "打开终端",
        label_en: "Open terminal",
        section: "工作区侧栏",
        shortcut: "⇧⌘T",
        icon: ProIcon::Terminal,
        keywords: "terminal shell console cargo command 终端 命令行",
    },
    PaletteItem {
        id: "office_gallery",
        label: "打开文档库",
        label_en: "Open documents",
        section: "文档",
        shortcut: "",
        icon: ProIcon::PageDoc,
        keywords: "office gallery templates files word excel ppt 办公 画布 模板 画廊",
    },
    PaletteItem {
        id: "open_file",
        label: "打开本地文档…",
        label_en: "Open document…",
        section: "文档",
        shortcut: "⌘O",
        icon: ProIcon::PageDoc,
        keywords: "open docx xlsx pptx 打开文件",
    },
    PaletteItem {
        id: "save_doc",
        label: "保存文档",
        label_en: "Save document",
        section: "文档",
        shortcut: "⌘S",
        icon: ProIcon::Check,
        keywords: "save 保存",
    },
    PaletteItem {
        id: "save_as_doc",
        label: "另存为…",
        label_en: "Save as…",
        section: "文档",
        shortcut: "⇧⌘S",
        icon: ProIcon::Download,
        keywords: "save as 另存为",
    },
    PaletteItem {
        id: "new_docx",
        label: "新建文稿",
        label_en: "New document",
        section: "文档",
        shortcut: "",
        icon: ProIcon::PageDoc,
        keywords: "new word docx 新建文档",
    },
    PaletteItem {
        id: "new_xlsx",
        label: "新建电子表格",
        label_en: "New spreadsheet",
        section: "文档",
        shortcut: "",
        icon: ProIcon::Grid,
        keywords: "new excel xlsx 新建表格",
    },
    PaletteItem {
        id: "new_pptx",
        label: "新建演示文稿",
        label_en: "New presentation",
        section: "文档",
        shortcut: "",
        icon: ProIcon::Note,
        keywords: "new ppt pptx slides 新建演示",
    },
    PaletteItem {
        id: "toggle_docs",
        label: "切换右侧面板",
        label_en: "Toggle right panel",
        section: "视图",
        shortcut: "⌘J",
        icon: ProIcon::SidebarRight,
        keywords: "split docs toggle 分栏 文档区",
    },
    PaletteItem {
        id: "toggle_wide",
        label: "切换宽幅面板",
        label_en: "Toggle wide panel",
        section: "视图",
        shortcut: "",
        icon: ProIcon::Expand,
        keywords: "wide canvas 宽幅 展开文档",
    },
    PaletteItem {
        id: "toggle_sidebar",
        label: "切换侧栏",
        label_en: "Toggle sidebar",
        section: "视图",
        shortcut: "⌘.",
        icon: ProIcon::SidebarLeft,
        keywords: "sidebar rail 侧边栏 导航",
    },
    PaletteItem {
        id: "toggle_theme",
        label: "切换浅色或深色",
        label_en: "Toggle light or dark appearance",
        section: "视图",
        shortcut: "",
        icon: ProIcon::Eye,
        keywords: "theme dark light 深色 浅色 夜间",
    },
    PaletteItem {
        id: "open_settings",
        label: "打开设置",
        label_en: "Open settings",
        section: "系统与设置",
        shortcut: "⌘,",
        icon: ProIcon::Sliders,
        keywords: "settings preferences agents skills usage logs 设置 控制中心 偏好",
    },
    PaletteItem {
        id: "provider_health",
        label: "智能体与账号",
        label_en: "Providers and accounts",
        section: "系统与设置",
        shortcut: "",
        icon: ProIcon::Sliders,
        keywords: "agents models codex claude gemini 智能体 模型 账号 安装",
    },
    PaletteItem {
        id: "remote_pair",
        label: "连接远端设备",
        label_en: "Connect a remote device",
        section: "系统与设置",
        shortcut: "",
        icon: ProIcon::Link,
        keywords: "remote pair hmac code 远端 配对 短码 双向控制",
    },
    PaletteItem {
        id: "schedules",
        label: "查看定时任务",
        label_en: "Open scheduled tasks",
        section: "系统与设置",
        shortcut: "",
        icon: ProIcon::Clock,
        keywords: "schedule cron automation 定时 排程 自动化 关联会话",
    },
    PaletteItem {
        id: "skills_mcp",
        label: "技能和 MCP",
        label_en: "Skills and MCP",
        section: "系统与设置",
        shortcut: "⇧⌘M",
        icon: ProIcon::Sliders,
        keywords: "skills mcp commands tools schema 技能 指令 工具",
    },
    PaletteItem {
        id: "usage_analytics",
        label: "查看用量",
        label_en: "View usage",
        section: "系统与设置",
        shortcut: "⇧⌘U",
        icon: ProIcon::UsageChart,
        keywords: "usage tokens cache quota heatmap rings 用量 缓存 额度 热力图",
    },
    PaletteItem {
        id: "logs_stream",
        label: "查看诊断日志",
        label_en: "View diagnostics",
        section: "系统与设置",
        shortcut: "⇧⌘L",
        icon: ProIcon::Memory,
        keywords: "logs diagnostics stream trace 日志 诊断 事件",
    },
    PaletteItem {
        id: "start_http",
        label: "共享当前对话",
        label_en: "Share current thread",
        section: "智能体与共享",
        shortcut: "",
        icon: ProIcon::ExternalArrow,
        keywords: "share http feed 共享 链接",
    },
    PaletteItem {
        id: "http_sub",
        label: "查看共享对话…",
        label_en: "Open shared thread…",
        section: "智能体与共享",
        shortcut: "",
        icon: ProIcon::Globe,
        keywords: "subscribe http 打开共享 订阅",
    },
    PaletteItem {
        id: "second_win",
        label: "在新窗口查看对话",
        label_en: "View thread in a new window",
        section: "智能体与共享",
        shortcut: "",
        icon: ProIcon::Copy,
        keywords: "second window 新窗口 双窗口",
    },
    PaletteItem {
        id: "open_docx",
        label: "打开会议纪要示例",
        label_en: "Open meeting notes sample",
        section: "示例演练",
        shortcut: "",
        icon: ProIcon::PageDoc,
        keywords: "sample word minutes 示例 纪要",
    },
    PaletteItem {
        id: "open_xlsx",
        label: "打开预算表示例",
        label_en: "Open budget sample",
        section: "示例演练",
        shortcut: "",
        icon: ProIcon::Grid,
        keywords: "sample excel formulas 示例 预算",
    },
    PaletteItem {
        id: "open_pptx",
        label: "打开演示文稿示例",
        label_en: "Open slide deck sample",
        section: "示例演练",
        shortcut: "",
        icon: ProIcon::Note,
        keywords: "sample ppt deck 示例 演示",
    },
    PaletteItem {
        id: "demo_chat",
        label: "打开演示对话",
        label_en: "Open demo thread",
        section: "示例演练",
        shortcut: "",
        icon: ProIcon::Sparkles,
        keywords: "demo chat 示例对话",
    },
    PaletteItem {
        id: "coedit_word",
        label: "演练协同编辑文稿",
        label_en: "Try document co-editing",
        section: "示例演练",
        shortcut: "",
        icon: ProIcon::GitBranch,
        keywords: "coedit word 协同 双写 段落",
    },
    PaletteItem {
        id: "coedit_excel",
        label: "演练协同编辑表格",
        label_en: "Try spreadsheet co-editing",
        section: "示例演练",
        shortcut: "",
        icon: ProIcon::GitBranch,
        keywords: "coedit excel 协同 双写 冲突 单元格",
    },
    PaletteItem {
        id: "coedit_ppt",
        label: "演练协同编辑演示文稿",
        label_en: "Try presentation co-editing",
        section: "示例演练",
        shortcut: "",
        icon: ProIcon::GitBranch,
        keywords: "coedit ppt 协同 双写 幻灯片",
    },
];

fn build_saved_copy_path(original: &Path, fallback_dir: &Path) -> PathBuf {
    let stem = original
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("document");
    let clean_stem = stem.strip_suffix(".saved").unwrap_or(stem);
    let ext = original
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("docx");
    let dir = original.parent().unwrap_or(fallback_dir);
    dir.join(format!("{clean_stem}.saved.{ext}"))
}


fn reply_markdown<'a>(messages: impl Iterator<Item = &'a str>) -> String {
    messages
        .filter(|message| !message.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod reply_copy_tests {
    #[test]
    fn whole_reply_preserves_indentation_and_hard_breaks() {
        let parts = ["    val = 2\n", " \n", "End  \nnext\n"];
        assert_eq!(
            super::reply_markdown(parts.into_iter()),
            "    val = 2\n\n\nEnd  \nnext\n"
        );
    }
}

#[cfg(test)]
mod composer_draft_tests {
    use super::{ComposerDrafts, DraftLocation, QueuedPrompt};
    use std::path::PathBuf;
    use uuid::Uuid;

    #[test]
    fn switching_project_and_chat_restores_their_own_drafts_and_attachments() {
        let first = PathBuf::from("/test/first");
        let second = DraftLocation::Project("/test/second".into());
        let chat = DraftLocation::Chat(Uuid::new_v4());
        let mut drafts = ComposerDrafts::new(first.clone());
        let attachment = first.join("notes.md");
        assert!(drafts.switch(DraftLocation::Project(first.clone()), || panic!("same editor")).is_none());
        let first_draft = QueuedPrompt { text: "project notes".into(), attachments: vec![attachment.clone()], target: None };
        assert!(drafts.switch(chat.clone(), || first_draft).unwrap().text.is_empty());
        assert!(drafts.switch(second.clone(), || QueuedPrompt { text: "chat draft".into(), ..Default::default() }).unwrap().text.is_empty());
        let restored = drafts.switch(DraftLocation::Project(first), QueuedPrompt::default).unwrap();
        assert_eq!(restored.text, "project notes");
        assert_eq!(restored.attachments, vec![attachment]);
        assert_eq!(drafts.switch(chat, QueuedPrompt::default).unwrap().text, "chat draft");
        assert!(drafts.switch(second, QueuedPrompt::default).unwrap().attachments.is_empty());
    }
}
