//! Off-render-thread Kar SDK runtime bridge (`ChatStore`, `ChatSession`, `HttpChatFeed`,
//! `ChatReplica`, `orientation::orient`, and `serve_mcp_tools` Office MCP host).
//!
//! Every disk, subprocess, and HTTP/SSE operation runs on a dedicated Tokio runtime thread,
//! communicating with GPUI strictly over `async_channel`.

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Result;
use kar::{
    ApprovalMode, Capabilities, CompactionStatus, ContextUsage, Error as KarError, GoalSetOptions,
    GoalSnapshot, InputImage, InstallationOptions, InstallationSnapshot, ManualCompactionReport,
    McpTransport, NativeTask, PendingRequest, PolicyChoice, SessionOptions, UsageSnapshot,
    account::{AccountUsageSnapshot, AuthStatus},
    chat::{
        AttachmentKind, ChangeKind, ChatCursor, ChatEvent, ChatHistory, ChatInfo, ChatReplica,
        ChatSession, ChatStore, ChatTurn, Checkpoint, CheckpointPoint,
        FileChange as CheckpointFileChange, HttpChatFeed, InputStatus, Item, QueuedInput, Role,
        TimelineItem, TurnStatus, event_router,
    },
    host_tools::ServedMcpTools,
    observe::{Agent, Tool},
    orientation::{Depth, OrientOptions, Probe, orient},
};
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::{
    client_store::{AUTO_BACKGROUND_THRESHOLD_MS, BackgroundedCommand},
    mcp_office::{OfficeMcpCall, start_office_mcp_server},
    remote_link::{
        PeerRole, RemoteAction, RemoteCommandEnvelope, RemoteExtendedSnapshot, RemoteHostTelemetry,
        RemoteLinkCore, RemoteOutcome, SharedRemoteLink, QueueCopyError, client_fetch_remote_state,
        client_redeem_pair_code, client_send_remote_command, now_epoch_ms,
        remote_link_router_with_updates,
    },
    schedules::{ScheduledTask, ScheduleRunOutcome},
    studio_hub::{
        ChatWorktreeMeta, DiagLevel, DiagLogRecord, DiffHunkData, GitWorkspaceSnapshot,
        GlobalSearchHit, UsageAnalyticsSnapshot, WorkspaceFilePreview, WorkspaceInventorySnapshot,
        WorkspaceTreeItem, build_portable_handoff_context, compute_usage_analytics,
        create_isolated_worktree_for_chat, discover_git_workspace, probe_inventory_with_office,
        GitDiffSide, read_diff_side, read_workspace_file_preview, run_global_search,
        scan_workspace_tree, scan_workspace_tree_with_options, switch_or_create_git_branch,
        unstage_git_paths,
    },
    types::{
        ChatAccessMode, ChatListEntry, FeedConnectionState, PendingPromptAttempt,
        ProviderAuthState, ProviderAvailability, ProviderModelOption, ProviderStatusInfo,
        SelectionTarget, runtime_display_name, runtime_supports_effort,
    },
};

#[derive(Debug)]
pub enum KarCommand {
    RefreshStoreAndProviders {
        store_dir: PathBuf,
        workspace: PathBuf,
    },
    ProbeProvidersOnly,
    ApplyProbedProviders {
        generation: u64,
        providers: Vec<ProviderStatusInfo>,
    },
    ListChats,
    CreateChat {
        runtime_id: String,
        model: Option<String>,
        program: Option<PathBuf>,
        workspace: PathBuf,
        approval_mode: ApprovalMode,
        effort: Option<String>,
        use_worktree: bool,
    },
    SelectLocalChat {
        chat_id: Uuid,
        prefer_read_only: bool,
    },
    /// Leave the open chat and return to the blank start state. Nothing is created or deleted.
    CloseChat,
    RenameChat {
        chat_id: Uuid,
        title: String,
    },
    SendPrompt {
        request_id: Uuid,
        raw_user_text: String,
        payload: String,
        target: Option<SelectionTarget>,
        attachments: Vec<PathBuf>,
    },
    /// Send a follow-up into a running turn (`steer_with_id` if supported, otherwise `enqueue`).
    SteerOrQueuePrompt {
        request_id: Uuid,
        raw_user_text: String,
        payload: String,
        target: Option<SelectionTarget>,
        attachments: Vec<PathBuf>,
    },
    RemoteQueuedAction {
        url: String,
        peer_id: Uuid,
        chat_id: Uuid,
        input_id: Uuid,
        copy_to_draft: bool,
    },
    WithdrawQueued {
        chat_id: Uuid,
        input_id: Uuid,
        edit: bool,
    },
    AbortTurn,
    DecidePermission {
        request: PendingRequest,
        decision: kar::PermissionDecision,
    },
    SelectPolicy {
        request: PendingRequest,
        choice: PolicyChoice,
    },
    /// Reply to a question the agent asked (a choice, a typed answer, or declining to answer).
    AnswerQuestion {
        request: PendingRequest,
        reply: kar::QuestionReply,
    },
    ForkChatAt {
        chat_id: Uuid,
        turn_id: Uuid,
        model_override: Option<String>,
    },
    RollbackToTurn {
        turn_id: Uuid,
        before: bool,
    },
    LoadTurnDiff(crate::turn_review::ReviewKey),
    CompactChat,
    SetGoal {
        objective: String,
    },
    ClearGoal,
    StopTask {
        task_id: String,
    },
    DeleteChat {
        chat_id: Uuid,
    },
    InstallAgent {
        runtime_id: String,
        line: Option<String>,
    },
    ConnectHttpFeed {
        url: String,
    },
    DisconnectFeed,
    StartHttpServer {
        chat_id: Uuid,
        listen_addr: String,
    },
    StopHttpServer,
    /// Issue a fresh 6-digit 60-second single-use pairing code on the local host link.
    IssuePairingCode,
    /// Immediately revoke an authorized remote device's HMAC session token.
    RevokePairedPeer {
        peer_id: Uuid,
    },
    /// Publish updated briefs, associations, and turn captures to paired remote controllers.
    SyncHostExtendedSnapshot {
        snapshot: RemoteExtendedSnapshot,
    },
    /// Redeem a 6-digit pairing code on a remote Kar host and enter bidirectional `RemoteController` mode.
    PairWithRemoteHost {
        url: String,
        code: String,
        device_name: String,
        role: PeerRole,
    },
    /// Forward an authenticated remote controller command into the local `WorkerState`.
    HandleRemoteCommand {
        peer_id: Uuid,
        env: RemoteCommandEnvelope,
    },
    /// Spawn an isolated session for a due/manual `ScheduledTask` and bind its `SessionAssociation`.
    CreateScheduledChat {
        request_id: Uuid,
        task: ScheduledTask,
        workspace: PathBuf,
    },
    EnterDemoShowcase,
    SendDemoPrompt {
        request_id: Uuid,
        raw_user_text: String,
        target: Option<SelectionTarget>,
    },
    FinishDemoPrompt {
        request_id: Uuid,
        raw_user_text: String,
        target: Option<SelectionTarget>,
        tool_call_id: String,
    },
    /// Discover workspace Git repository, branches, commit graph, and file tree.
    RefreshGitWorkspace {
        workspace: PathBuf,
    },
    /// Stage explicit paths via `kar::workbench_git::Repository::stage`.
    GitStagePaths {
        workspace: PathBuf,
        paths: Vec<PathBuf>,
    },
    /// Unstage explicit paths from index (`git reset HEAD -- <paths>`).
    GitUnstagePaths {
        workspace: PathBuf,
        paths: Vec<PathBuf>,
    },
    /// Stage all modified/untracked files in the workspace.
    GitStageAll {
        workspace: PathBuf,
    },
    /// Switch to an existing branch or create & switch to a new branch via `Repository`.
    GitSwitchBranch {
        workspace: PathBuf,
        branch: String,
        create: bool,
    },
    /// Load hunk-level diff for an inline expanded file row inside the Changes dock.
    LoadGitFileDiff {
        staged: bool,
        workspace: PathBuf,
        rel_path: PathBuf,
        is_demo: bool,
    },
    /// Rescan workspace file tree with explicit hidden-file toggle.
    RefreshWorkspaceTree {
        workspace: PathBuf,
        show_hidden: bool,
    },
    /// Run a live workspace shell command inside the integrated Right-Dock Terminal.
    RunWorkspaceCommand {
        id: Uuid,
        cwd: PathBuf,
        command: String,
    },
    /// Stop a running workspace shell command in the integrated Right-Dock Terminal.
    KillWorkspaceCommand {
        id: Uuid,
    },
    /// Commit staged changes via `kar::workbench_git::Repository::commit`.
    GitCommit {
        stage_paths: Vec<PathBuf>,
        workspace: PathBuf,
        message: String,
    },
    /// Fast-forward pull via `kar::workbench_git::Repository::pull`.
    GitPull {
        workspace: PathBuf,
    },
    /// Publish branch via `kar::workbench_git::Repository::push`.
    GitPush {
        workspace: PathBuf,
    },
    /// Read a workspace file & its Git diff hunks for Right-Pane preview.
    ReadWorkspaceFile {
        workspace: PathBuf,
        rel_path: PathBuf,
    },
    /// Probe workspace Skills, Slash Commands, MCP Servers & Tools via `kar::inventory`.
    ProbeWorkspaceInventory {
        workspace: PathBuf,
        runtime_id: String,
    },
    /// Run off-thread search across workspace file paths, file lines, and `ChatStore` messages.
    GlobalSearch {
        workspace: PathBuf,
        query: String,
    },
    /// Recompute token, prompt cache, and account quota analytics across `ChatStore`.
    RefreshUsageAnalytics {
        providers: Vec<ProviderStatusInfo>,
    },
    /// Hand off the current conversation to another runtime (`Codex` -> `Claude` / `Cursor` / `Antigravity` / ...).
    HandoffChat {
        source_chat: Option<Uuid>,
        target_runtime: String,
        target_model: Option<String>,
        user_note: String,
        budget_bytes: usize,
        workspace: PathBuf,
    },
    /// Ask an ephemeral zero-pollution side question (`Runtime::side_question` or context-read summary).
    AskSideQuestion {
        request_id: Uuid,
        runtime_id: String,
        native_session_id: Option<String>,
        workspace: PathBuf,
        question: String,
        context_excerpt: String,
    },
    Shutdown,
}

#[derive(Clone, Debug, Default)]
pub struct ProjectedChatState {
    pub chat_id: Option<Uuid>,
    pub info: Option<ChatInfo>,
    pub turns: Vec<ChatTurn>,
    pub pending_requests: Vec<PendingRequest>,
    pub queued_inputs: Vec<QueuedInput>,
    pub context_usage: Option<ContextUsage>,
    pub usage: Vec<UsageSnapshot>,
    pub goal: Option<Option<GoalSnapshot>>,
    pub tasks: Vec<NativeTask>,
    pub manual_compaction: Option<ManualCompactionReport>,
    pub workspace_position: Option<CheckpointPoint>,
    pub can_fork: bool,
    pub can_steer: bool,
    pub can_compact: bool,
    pub can_goals: bool,
    pub can_stop_task: bool,
    pub can_images: bool,
    pub cursor: Option<ChatCursor>,
    pub is_demo: bool,
}

#[derive(Clone, Debug)]
pub enum ChatCreateFailure {
    Store,
    Worktree,
    Runtime(String),
}

#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum KarBridgeEvent {
    StoreReady {
        store_dir: PathBuf,
        workspace: PathBuf,
    },
    ProvidersProbing(bool),
    ProvidersUpdated(Vec<ProviderStatusInfo>),
    ChatsListed(Vec<ChatListEntry>),
    TurnTargetsLoaded(BTreeMap<Uuid, SelectionTarget>),
    TurnDiffLoaded {
        key: crate::turn_review::ReviewKey,
        result: Result<Vec<String>, String>,
    },
    WorkspaceFilesRestored {
        files: Vec<PathBuf>,
    },
    AccessModeChanged(ChatAccessMode),
    ConnectionStateChanged(FeedConnectionState),
    ChatStateUpdated(ProjectedChatState),
    QueuedInputRetrieved {
        chat_id: Uuid,
        input_id: Uuid,
        text: String,
        attachments: Vec<PathBuf>,
    },
    PromptLifecycle {
        attempt: PendingPromptAttempt,
    },
    /// A new chat could not be started, so whatever was waiting to be sent in it never left.
    ChatCreateFailed {
        cause: ChatCreateFailure,
        detail: String,
    },
    HttpServerState {
        listening_url: Option<String>,
        chat_id: Option<Uuid>,
        message: Option<String>,
    },
    BannerNotice {
        title: String,
        detail: String,
        is_error: bool,
    },
    ToastMessage {
        text: String,
        warning: bool,
    },
    OfficeMcpCall(OfficeMcpCall),
    RemoteTelemetryUpdated(RemoteHostTelemetry),
    RemotePairFailed {
        code_rejected: bool,
    },
    RemoteExtendedStateUpdated {
        url: String,
        peer_id: Uuid,
        snapshot: RemoteExtendedSnapshot,
    },
    RemoteQueueCopied {
        url: String,
        peer_id: Uuid,
        chat_id: Uuid,
        result: Result<String, QueueCopyError>,
    },
    RemoteBriefFeedbackReceived {
        brief_id: String,
        revision: u64,
        summary_markdown: String,
    },
    ScheduleRunFinished { request_id: Uuid, outcome: ScheduleRunOutcome },
    ScheduledChatCreated {
        request_id: Uuid,
        task_id: String,
        chat_id: Uuid,
    },
    AutoBackgroundedCommand(BackgroundedCommand),
    GitCommitFinished { workspace: PathBuf, message: String, success: bool },
    GitWorkspaceUpdated {
        workspace: PathBuf,
        snapshot: GitWorkspaceSnapshot,
        tree: Vec<WorkspaceTreeItem>,
    },
    GitFileDiffLoaded {
        staged: bool,
        workspace: PathBuf,
        rel_path: PathBuf,
        hunks: Vec<DiffHunkData>,
    },
    WorkspaceTreeUpdated {
        workspace: PathBuf,
        show_hidden: bool,
        tree: Vec<WorkspaceTreeItem>,
    },
    WorktreeCreated {
        chat_id: Uuid,
        meta: ChatWorktreeMeta,
    },
    TerminalOutputChunk {
        id: Uuid,
        chunk: String,
    },
    TerminalCommandFinished {
        id: Uuid,
        exit_code: i32,
    },
    WorkspaceFileLoaded { workspace: PathBuf, preview: WorkspaceFilePreview },
    WorkspaceInventoryUpdated { workspace: PathBuf, inventory: WorkspaceInventorySnapshot },
    GlobalSearchResults {
        workspace: PathBuf,
        query: String,
        hits: Vec<GlobalSearchHit>,
    },
    UsageAnalyticsUpdated(UsageAnalyticsSnapshot),
    SideQuestionAnswered {
        request_id: Uuid,
        question: String,
        answer: String,
        via_native_fork: bool,
    },
    HandoffCompleted {
        chat_id: Option<Uuid>,
        target_runtime: String,
        summary_message: String,
    },
    DiagLogAppended(DiagLogRecord),
    /// Live model-suggested follow-up prompt from `kar::PromptSuggestions`.
    PromptSuggestionUpdated {
        chat_id: Uuid,
        suggestion: Option<kar::PromptSuggestion>,
    },
}

#[derive(Clone)]
pub struct KarBridgeHandle {
    pub tx: async_channel::Sender<KarCommand>,
    pub rx: async_channel::Receiver<KarBridgeEvent>,
}

impl KarBridgeHandle {
    pub fn spawn(initial_store_dir: PathBuf, initial_workspace: PathBuf) -> Self {
        let (cmd_tx, cmd_rx) = async_channel::unbounded::<KarCommand>();
        let (evt_tx, evt_rx) = async_channel::unbounded::<KarBridgeEvent>();
        let loop_cmd_tx = cmd_tx.clone();

        let _ = std::thread::Builder::new()
            .name("kar-desktop-tokio".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("build tokio runtime for Kar bridge");
                rt.block_on(worker_loop(
                    initial_store_dir,
                    initial_workspace,
                    loop_cmd_tx,
                    cmd_rx,
                    evt_tx,
                ));
            });

        Self {
            tx: cmd_tx,
            rx: evt_rx,
        }
    }

    pub fn send(&self, cmd: KarCommand) {
        let _ = self.tx.send_blocking(cmd);
    }
}

/// Build an augmented `PATH` so GUI-launched macOS bundles discover Homebrew, Cargo, and `~/.local/bin` CLIs
/// without needing `unsafe { std::env::set_var(...) }`.
fn augmented_path_os() -> OsString {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let mut extra = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/opt/homebrew/sbin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ];
    if let Some(home) = std::env::home_dir() {
        extra.insert(0, home.join(".local/bin"));
        extra.insert(1, home.join(".cargo/bin"));
        extra.insert(2, home.join(".bun/bin"));
    }
    for candidate in extra {
        if !dirs.iter().any(|existing| existing == &candidate) {
            dirs.push(candidate);
        }
    }
    std::env::join_paths(dirs).unwrap_or_else(|_| OsString::from("/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"))
}

fn configure_runtime_path(runtime_id: &str, opts: &mut SessionOptions) {
    let refuses_env = kar::runtime(runtime_id)
        .ok()
        .and_then(|rt| rt.protocol.child_environment_refusal())
        .is_some();
    if !refuses_env {
        opts.env
            .insert(OsString::from("PATH"), augmented_path_os());
    }
}

#[derive(Default, Serialize, Deserialize)]
struct PersistedTargets {
    by_turn: BTreeMap<Uuid, SelectionTarget>,
}

fn targets_file(store_dir: &Path) -> PathBuf {
    store_dir.join("kar_desktop_turn_targets.json")
}

fn load_persisted_targets(store_dir: &Path) -> BTreeMap<Uuid, SelectionTarget> {
    let path = targets_file(store_dir);
    std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<PersistedTargets>(&bytes).ok())
        .map(|p| p.by_turn)
        .unwrap_or_default()
}

fn save_persisted_target(store_dir: &Path, turn_id: Uuid, target: SelectionTarget) {
    let mut map = load_persisted_targets(store_dir);
    map.insert(turn_id, target);
    let path = targets_file(store_dir);
    if let Ok(bytes) = serde_json::to_vec_pretty(&PersistedTargets { by_turn: map }) {
        let _ = std::fs::write(path, bytes);
    }
}

#[derive(Default, Serialize, Deserialize)]
struct PersistedChatTitles {
    by_chat: BTreeMap<Uuid, String>,
}

fn titles_file(store_dir: &Path) -> PathBuf {
    store_dir.join("kar_desktop_chat_titles.json")
}

fn load_persisted_titles(store_dir: &Path) -> BTreeMap<Uuid, String> {
    let path = titles_file(store_dir);
    std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<PersistedChatTitles>(&bytes).ok())
        .map(|p| p.by_chat)
        .unwrap_or_default()
}

fn save_persisted_title(store_dir: &Path, chat_id: Uuid, title: Option<&str>) {
    let mut map = load_persisted_titles(store_dir);
    match title.map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => {
            map.insert(chat_id, t.to_owned());
        }
        None => {
            map.remove(&chat_id);
        }
    }
    let path = titles_file(store_dir);
    if let Ok(bytes) = serde_json::to_vec_pretty(&PersistedChatTitles { by_chat: map }) {
        let _ = std::fs::write(path, bytes);
    }
}

fn infer_attachment_mime_and_kind(path: &Path, bytes: &[u8]) -> (String, AttachmentKind) {
    if let Ok(img) = InputImage::from_bytes(bytes) {
        return (img.mime_type().to_owned(), AttachmentKind::Image);
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "pdf" => "application/pdf",
        "md" | "markdown" => "text/markdown",
        "json" => "application/json",
        "csv" => "text/csv",
        "txt" | "rs" | "py" | "ts" | "js" | "toml" | "yaml" | "yml" => "text/plain",
        _ => "application/octet-stream",
    };
    (mime.to_owned(), AttachmentKind::File)
}

fn persist_attachment_paths(
    store: &ChatStore,
    chat_id: Uuid,
    paths: &[PathBuf],
) -> kar::Result<Vec<Uuid>> {
    let mut ids = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = std::fs::read(path)?;
        let raw_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("attachment.bin");
        let safe_name: String = raw_name
            .chars()
            .map(|c| if c.is_control() || matches!(c, '/' | '\\') { '_' } else { c })
            .take(180)
            .collect();
        let (mime, kind) = infer_attachment_mime_and_kind(path, &bytes);
        let att_id = Uuid::new_v4();
        let saved = store.persist_attachment(chat_id, att_id, &safe_name, &mime, kind, &bytes)?;
        ids.push(saved.id);
    }
    Ok(ids)
}

struct ActiveHttpServer {
    chat_id: Uuid,
    stop_tx: oneshot::Sender<()>,
    join: JoinHandle<()>,
}

impl ActiveHttpServer {
    async fn stop(self) {
        let _ = self.stop_tx.send(());
        let _ = self.join.await;
    }
}

#[derive(Clone, Debug)]
struct ActiveRemoteSession {
    url: String,
    token: String,
    peer_id: Uuid,
    host_name: String,
    role: PeerRole,
    chat_id: Option<Uuid>,
}

struct WorkerState {
    store_dir: PathBuf,
    workspace: PathBuf,
    store: Option<ChatStore>,
    selected_chat_id: Option<Uuid>,
    active_controller: Option<Arc<ChatSession>>,
    feed_cancel: Option<watch::Sender<bool>>,
    http_server: Option<ActiveHttpServer>,
    remote_link: SharedRemoteLink,
    remote_session: Option<ActiveRemoteSession>,
    mcp_office: Option<ServedMcpTools>,
    mcp_supported_runtimes: HashMap<String, bool>,
    goals_supported_runtimes: HashMap<String, bool>,
    chat_list_cache: HashMap<Uuid, (Option<SystemTime>, Option<String>, ChatListEntry)>,
    probe_generation: u64,
    demo_turns: Vec<ChatTurn>,
    in_demo: bool,
    running_terminals: HashMap<Uuid, watch::Sender<bool>>,
    prompt_suggestions: kar::PromptSuggestions,
    suggestion_watcher: Option<tokio::task::JoinHandle<()>>,
    cmd_tx: async_channel::Sender<KarCommand>,
    evt_tx: async_channel::Sender<KarBridgeEvent>,
}

impl WorkerState {
    fn emit(&self, ev: KarBridgeEvent) {
        let _ = self.evt_tx.send_blocking(ev);
    }

    fn emit_remote_telemetry(&self) {
        let now = now_epoch_ms();
        let snap = {
            let guard = self.remote_link.lock().unwrap_or_else(|e| e.into_inner());
            guard.telemetry(now)
        };
        self.emit(KarBridgeEvent::RemoteTelemetryUpdated(snap));
    }

    fn dispatch_remote_envelope(&self, env: RemoteCommandEnvelope) -> bool {
        let Some(ref remote) = self.remote_session else {
            return false;
        };
        if !remote.role.can_mutate() {
            self.emit(KarBridgeEvent::ToastMessage {
                text: "当前配对角色为「只读观察」，无法发送控制指令".into(),
                warning: true,
            });
            return true;
        }
        let url = remote.url.clone();
        let token = remote.token.clone();
        let evt_tx = self.evt_tx.clone();
        tokio::spawn(async move {
            let res = tokio::task::spawn_blocking(move || {
                client_send_remote_command(&url, &token, &env)
            })
            .await;
            match res {
                Ok(Ok(receipt)) => match receipt.outcome {
                    RemoteOutcome::Admitted | RemoteOutcome::Pong { .. } => {
                        let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                            text: "指令已通过 HMAC 双向链路送达主机".into(),
                            warning: false,
                        });
                    }
                    RemoteOutcome::OutcomeUnknown { detail } => {
                        let _ = evt_tx.send_blocking(KarBridgeEvent::BannerNotice {
                            title: "远端指令结果待确认（OutcomeUnknown）".into(),
                            detail,
                            is_error: false,
                        });
                    }
                    RemoteOutcome::Rejected { reason } => {
                        let _ = evt_tx.send_blocking(KarBridgeEvent::BannerNotice {
                            title: "远端主机拒绝了该指令".into(),
                            detail: reason,
                            is_error: true,
                        });
                    }
                },
                Ok(Err(err)) => {
                    let _ = evt_tx.send_blocking(KarBridgeEvent::BannerNotice {
                        title: "远端控制连接异常".into(),
                        detail: err.to_string(),
                        is_error: true,
                    });
                }
                Err(err) => {
                    let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                        text: format!("远端任务调度失败：{err}"),
                        warning: true,
                    });
                }
            }
        });
        true
    }

    fn turn_appears_busy(&self) -> bool {
        self.active_controller
            .as_ref()
            .and_then(|s| s.history().ok())
            .and_then(|h| h.turns.last().map(|t| t.status == TurnStatus::Running))
            .unwrap_or(false)
    }

    fn stop_feed(&mut self) {
        self.remote_session = None;
        if let Some(cancel) = self.feed_cancel.take() {
            let _ = cancel.send(true);
        }
    }

    fn send_prompt(
        &mut self,
        request_id: Uuid,
        raw_user_text: String,
        payload: String,
        target: Option<SelectionTarget>,
        attachments: Vec<PathBuf>,
    ) {
        if let Some(ref tgt) = target {
            save_persisted_target(&self.store_dir, request_id, tgt.clone());
            self.emit(KarBridgeEvent::TurnTargetsLoaded(load_persisted_targets(
                &self.store_dir,
            )));
        }
        let Some(session) = self.active_controller.clone() else {
            if self.dispatch_remote_envelope(RemoteCommandEnvelope {
                request_id,
                expected_revision: None,
                action: RemoteAction::SendPrompt {
                    text: payload,
                    target,
                },
            }) {
                return;
            }
            self.emit(KarBridgeEvent::BannerNotice {
                title: "消息没有发出".into(),
                detail: "这个对话目前只能查看。新建一个对话或使用 6 位配对码连接主机后再发送。"
                    .into(),
                is_error: true,
            });
            return;
        };
        let chat_id = session.info().id;
        let mut attempt = PendingPromptAttempt {
            chat_id: Some(chat_id),
            request_id,
            raw_user_text,
            sent_payload: payload.clone(),
            target,
            attachments: attachments.clone(),
            attempts: 1,
            in_flight: true,
            last_error: None,
        };
        self.emit(KarBridgeEvent::PromptLifecycle {
            attempt: attempt.clone(),
        });

        let evt_tx = self.evt_tx.clone();
        let store_opt = self.store.clone();
        let store_dir = self.store_dir.clone();
        let caps = Some(session.capabilities());
        tokio::spawn(async move {
            let send_res = if attachments.is_empty() {
                session.send(request_id, payload).await
            } else if let Some(ref store) = store_opt {
                match persist_attachment_paths(store, chat_id, &attachments) {
                    Ok(att_ids) => session.send_attached(request_id, payload, att_ids).await,
                    Err(err) => Err(err),
                }
            } else {
                session.send(request_id, payload).await
            };

            attempt.in_flight = false;
            match send_res {
                Ok(_turn) => {
                    attempt.last_error = None;
                    let _ = evt_tx.send_blocking(KarBridgeEvent::PromptLifecycle { attempt });
                    if let Ok(h) = session.history() {
                        let queued = active_queued_inputs(store_opt.as_ref(), chat_id);
                        let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                            state_from_history(h, caps, queued),
                        ));
                    }
                }
                Err(err) => {
                    attempt.last_error = Some(err.to_string());
                    let _ = evt_tx.send_blocking(KarBridgeEvent::PromptLifecycle {
                        attempt: attempt.clone(),
                    });
                    let _ = evt_tx.send_blocking(KarBridgeEvent::BannerNotice {
                        title: "这条消息没有完成".into(),
                        detail: format!("{err}。可以在消息下方点「重试」。"),
                        is_error: true,
                    });
                    if let Ok(h) = session.history() {
                        let queued = active_queued_inputs(store_opt.as_ref(), chat_id);
                        let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                            state_from_history(h, caps, queued),
                        ));
                    }
                }
            }
            if let Some(store) = store_opt
                && let Ok(infos) = store.list()
            {
                let custom_titles = load_persisted_titles(&store_dir);
                let entries = infos
                    .into_iter()
                    .map(|info| {
                        let title_override = custom_titles.get(&info.id).map(String::as_str);
                        let turns = read_chat_history_resilient(&store, &store_dir, info.id)
                            .map(|h| h.turns)
                            .unwrap_or_default();
                        ChatListEntry::from_info_and_turns_with_title(info, &turns, title_override)
                    })
                    .collect();
                let _ = evt_tx.send_blocking(KarBridgeEvent::ChatsListed(entries));
            }
        });
    }

    async fn close_controller(&mut self, abort_if_busy: bool) {
        if let Some(task) = self.suggestion_watcher.take() { task.abort(); }
        let should_abort = abort_if_busy && self.turn_appears_busy();
        if let Some(arc_session) = self.active_controller.take() {
            if should_abort {
                let _ = arc_session.abort().await;
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
            if let Ok(owned) = Arc::try_unwrap(arc_session) {
                let _ = owned.close().await;
            }
        }
    }

    fn attach_office_mcp(&self, runtime_id: &str, opts: &mut SessionOptions) {
        let supported = self
            .mcp_supported_runtimes
            .get(runtime_id)
            .copied()
            .unwrap_or_else(|| default_runtime_mcp_support(runtime_id));
        if supported && let Some(ref mcp) = self.mcp_office {
            let (name, srv) = mcp.entry();
            opts.mcp_servers.insert(name, srv);
        }
    }

    fn spawn_provider_probe(&mut self, show_spinner: bool) {
        self.probe_generation = self.probe_generation.wrapping_add(1);
        let generation = self.probe_generation;
        let workspace = self.workspace.clone();
        let cmd_tx = self.cmd_tx.clone();
        if show_spinner {
            self.emit(KarBridgeEvent::ProvidersProbing(true));
        }
        tokio::spawn(async move {
            let providers = probe_all_providers(workspace).await;
            let _ = cmd_tx
                .send(KarCommand::ApplyProbedProviders {
                    generation,
                    providers,
                })
                .await;
        });
    }

    fn refresh_chat_list(&mut self) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let custom_titles = load_persisted_titles(&self.store_dir);
        match store.list() {
            Ok(infos) => {
                let mut next_cache = HashMap::with_capacity(infos.len());
                let mut entries = Vec::with_capacity(infos.len());
                for info in infos {
                    let karchat = self.store_dir.join(format!("{}.karchat", info.id));
                    let mtime = std::fs::metadata(&karchat)
                        .and_then(|m| m.modified())
                        .ok();
                    let custom_title = custom_titles.get(&info.id).cloned();
                    let entry = match self.chat_list_cache.get(&info.id) {
                        Some((cached_m, cached_title, cached_entry))
                            if mtime.is_some()
                                && *cached_m == mtime
                                && *cached_title == custom_title =>
                        {
                            let mut cloned = cached_entry.clone();
                            cloned.info = info;
                            cloned
                        }
                        _ => {
                            let turns = read_chat_history_resilient(&store, &self.store_dir, info.id)
                                .map(|h| h.turns)
                                .unwrap_or_default();
                            ChatListEntry::from_info_and_turns_with_title(
                                info,
                                &turns,
                                custom_title.as_deref(),
                            )
                        }
                    };
                    next_cache.insert(entry.info.id, (mtime, custom_title, entry.clone()));
                    entries.push(entry);
                }
                self.chat_list_cache = next_cache;
                self.emit(KarBridgeEvent::ChatsListed(entries));
            }
            Err(err) => {
                self.emit(KarBridgeEvent::BannerNotice {
                    title: "无法读取对话列表".into(),
                    detail: err.to_string(),
                    is_error: true,
                });
            }
        }
    }

    /// Drop the open chat (feed, controller, sample mode) and tell the UI nothing is selected.
    async fn leave_chat(&mut self) {
        self.in_demo = false;
        self.selected_chat_id = None;
        self.stop_feed();
        self.close_controller(false).await;
        self.emit(KarBridgeEvent::AccessModeChanged(
            ChatAccessMode::NoneSelected,
        ));
        self.emit(KarBridgeEvent::ConnectionStateChanged(
            FeedConnectionState::Idle,
        ));
        self.emit(KarBridgeEvent::ChatStateUpdated(ProjectedChatState::default()));
    }
}

/// Ensure `{chat_id}.lock` exists before reading history of an abandoned chat so `ChatStore::history`
/// never fails with `ENOENT` on imported or recovered `.karchat` journals.
fn read_chat_history_resilient(
    store: &ChatStore,
    store_dir: &Path,
    chat_id: Uuid,
) -> kar::Result<ChatHistory> {
    let lock_path = store_dir.join(format!("{chat_id}.lock"));
    if !lock_path.exists() {
        let _ = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path);
    }
    store.history(chat_id)
}

fn active_queued_inputs(store: Option<&ChatStore>, chat_id: Uuid) -> Vec<QueuedInput> {
    store
        .and_then(|s| s.queued_inputs(chat_id).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|q| matches!(q.status, InputStatus::Pending | InputStatus::Sending))
        .collect()
}

fn default_runtime_mcp_support(runtime_id: &str) -> bool {
    matches!(
        runtime_id,
        "codex" | "claude" | "pi" | "grok" | "kimi" | "qwen" | "qoder" | "factory" | "codebuddy"
    )
}

fn decl_supports_hosted_mcp(
    runtime_id: &str,
    decl: Option<&kar::SessionCapabilityDeclaration>,
) -> bool {
    if !default_runtime_mcp_support(runtime_id) {
        return false;
    }
    match decl {
        Some(decl) => {
            if decl
                .refused_session_options
                .contains_key("mcp_server_credentials")
            {
                return false;
            }
            runtime_id == "pi"
                || decl
                    .mcp_transports
                    .as_ref()
                    .is_some_and(|t| t.contains(&McpTransport::Http))
        }
        None => default_runtime_mcp_support(runtime_id),
    }
}

fn summarize_account_usage(probe: Option<&Probe<AccountUsageSnapshot>>) -> Option<String> {
    let Some(Probe::Ok {
        value: AccountUsageSnapshot::Available(quota),
        ..
    }) = probe
    else {
        return None;
    };
    if quota.rate_limited {
        return Some("额度受限".into());
    }
    quota.windows.first().map(|w| {
        let pct = (w.used_ratio.clamp(0.0, 1.0) * 100.0).round() as u32;
        format!("已用 {pct}%")
    })
}

fn candidate_cli_names(runtime_id: &str) -> &'static [&'static str] {
    // Matches `kar/src/runtime.rs::builtins()` 1-for-1 so `fast_seed_providers_from_path()`
    // never marks a GUI editor launcher (e.g. `qoder` or `cursor`) ready when the CLI adapter
    // (`qodercli` / `agent`) is missing.
    match runtime_id {
        "codex" => &["codex"],
        "claude" => &["claude"],
        "grok" => &["grok"],
        "opencode" => &["opencode"],
        "kimi" => &["kimi"],
        "pi" => &["pi"],
        "qwen" => &["qwen"],
        "cursor" => &["agent"],
        "antigravity" => &["agy"],
        "codebuddy" => &["codebuddy"],
        "factory" => &["droid"],
        "kiro" => &["kiro-cli"],
        "qoder" => &["qodercli"],
        "devin" => &["devin"],
        "dsh" => &["dsh"],
        "fx" => &["fx"],
        "muse" => &["muse"],
        _ => &[],
    }
}

fn runtime_fallback_paths(runtime_id: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let home = std::env::home_dir();
    match runtime_id {
        "codex" => {
            #[cfg(target_os = "macos")]
            for base in [
                Some(PathBuf::from("/Applications")),
                home.as_ref().map(|h| h.join("Applications")),
            ]
            .into_iter()
            .flatten()
            {
                for app in ["ChatGPT.app", "Codex.app"] {
                    let resources = base.join(app).join("Contents/Resources");
                    paths.push(resources.join("codex"));
                    paths.push(resources.join("codex-cli/CodexCLI.app/Contents/MacOS/codex"));
                }
            }
            if let Some(dir) = std::env::var_os("CODEX_INSTALL_DIR").filter(|v| !v.is_empty()) {
                paths.push(PathBuf::from(dir).join("codex"));
            }
            if let Some(ref h) = home {
                paths.push(h.join(".local/bin/codex"));
            }
        }
        "kimi" => {
            if let Some(ref h) = home {
                paths.push(h.join(".kimi-code/bin/kimi"));
            }
        }
        "grok" => {
            if let Some(ref h) = home {
                paths.push(h.join(".grok/bin/grok"));
            }
        }
        "opencode" => {
            if let Some(ref h) = home {
                paths.push(h.join(".opencode/bin/opencode"));
            }
        }
        "qoder" => {
            if let Some(ref h) = home {
                paths.push(h.join(".local/bin/qodercli"));
            }
        }
        _ => {}
    }
    paths
}

/// Phase-1 (<3ms, zero subprocesses) filesystem scan of `augmented_path_os()` + Kar's macOS/Unix
/// bundle fallbacks so the UI shows installed runtimes (`● Codex 就绪`) immediately on frame 1
/// while `orient(Depth::Models)` enriches versions, accounts, and model lists silently in the background.
pub fn fast_seed_providers_from_path() -> Vec<ProviderStatusInfo> {
    let prioritized = [
        "codex",
        "claude",
        "grok",
        "opencode",
        "kimi",
        "pi",
        "qwen",
        "cursor",
        "antigravity",
    ];
    let mut catalog = kar::runtimes();
    catalog.sort_by_key(|rt| {
        prioritized
            .iter()
            .position(|&p| p == rt.id)
            .unwrap_or(usize::MAX)
    });
    let path_dirs: Vec<PathBuf> = std::env::split_paths(&augmented_path_os()).collect();

    catalog
        .into_iter()
        .map(|rt| {
            let id = rt.id.clone();
            let found_cmd = candidate_cli_names(&id)
                .iter()
                .find_map(|bin| {
                    path_dirs
                        .iter()
                        .map(|dir| dir.join(bin))
                        .find(|candidate| candidate.is_file())
                })
                .or_else(|| {
                    runtime_fallback_paths(&id)
                        .into_iter()
                        .find(|candidate| candidate.is_file())
                });
            let can_install = if let Some(first) = rt.install_lines().first() {
                rt.install_plan_for(Some(first.line)).is_ok()
            } else {
                rt.install_plan().is_ok()
            };
            let install_lines = rt
                .install_lines()
                .iter()
                .map(|l| (l.line.to_owned(), l.description.to_owned()))
                .collect();
            let (availability, auth) = match found_cmd {
                Some(cmd) => (
                    ProviderAvailability::Available {
                        version: "installed".into(),
                        command: Some(cmd),
                    },
                    ProviderAuthState::Unknown {
                        note: "本地 CLI 已就绪".into(),
                    },
                ),
                None => (ProviderAvailability::Missing, ProviderAuthState::NotChecked),
            };
            ProviderStatusInfo {
                protocol: format!("{:?}", rt.protocol).to_ascii_lowercase(),
                availability,
                auth,
                mcp_office_supported: default_runtime_mcp_support(&id),
                steering_supported: matches!(id.as_str(), "codex" | "claude" | "pi"),
                goals_supported: matches!(id.as_str(), "codex" | "dsh"),
                can_install,
                install_lines,
                models: Vec::new(),
                quota_summary: None,
                id,
            }
        })
        .collect()
}

async fn probe_all_providers(workspace: PathBuf) -> Vec<ProviderStatusInfo> {
    let prioritized = [
        "codex",
        "claude",
        "grok",
        "opencode",
        "kimi",
        "pi",
        "qwen",
        "cursor",
        "antigravity",
    ];
    let mut catalog = kar::runtimes();
    catalog.sort_by_key(|rt| {
        prioritized
            .iter()
            .position(|&p| p == rt.id)
            .unwrap_or(usize::MAX)
    });
    let protocol_by_id: HashMap<String, String> = catalog
        .iter()
        .map(|rt| (rt.id.clone(), format!("{:?}", rt.protocol).to_ascii_lowercase()))
        .collect();
    let can_install_by_id: HashMap<String, bool> = catalog
        .iter()
        .map(|rt| {
            let ok = if let Some(first) = rt.install_lines().first() {
                rt.install_plan_for(Some(first.line)).is_ok()
            } else {
                rt.install_plan().is_ok()
            };
            (rt.id.clone(), ok)
        })
        .collect();
    let lines_by_id: HashMap<String, Vec<(String, String)>> = catalog
        .iter()
        .map(|rt| {
            let lines = rt
                .install_lines()
                .iter()
                .map(|l| (l.line.to_owned(), l.description.to_owned()))
                .collect();
            (rt.id.clone(), lines)
        })
        .collect();

    let mut install_opts = InstallationOptions {
        cwd: Some(workspace),
        ..Default::default()
    };
    install_opts
        .env
        .insert(OsString::from("PATH"), Some(augmented_path_os()));

    let orient_opts = OrientOptions {
        installation: install_opts,
        depth: Depth::Models,
        runtimes: None,
        customization: false,
        timeout: Some(Duration::from_secs(4)),
        cancellation: Default::default(),
    };

    let Ok(report) = orient(&catalog, &orient_opts).await else {
        return catalog
            .into_iter()
            .map(|rt| {
                let id = rt.id.clone();
                ProviderStatusInfo {
                    protocol: format!("{:?}", rt.protocol).to_ascii_lowercase(),
                    availability: ProviderAvailability::Missing,
                    auth: ProviderAuthState::NotChecked,
                    mcp_office_supported: default_runtime_mcp_support(&id),
                    steering_supported: false,
                    goals_supported: false,
                    can_install: if let Some(first) = rt.install_lines().first() {
                        rt.install_plan_for(Some(first.line)).is_ok()
                    } else {
                        rt.install_plan().is_ok()
                    },
                    install_lines: rt
                        .install_lines()
                        .iter()
                        .map(|l| (l.line.to_owned(), l.description.to_owned()))
                        .collect(),
                    models: Vec::new(),
                    quota_summary: None,
                    id,
                }
            })
            .collect();
    };

    report
        .runtimes
        .into_iter()
        .map(|row| {
            let id = row.id.clone();
            let protocol = protocol_by_id
                .get(&id)
                .cloned()
                .unwrap_or_else(|| "native".into());
            let can_install = can_install_by_id.get(&id).copied().unwrap_or(false);
            let install_lines = lines_by_id.get(&id).cloned().unwrap_or_default();
            let decl = row.session_capabilities.as_ref();
            let mcp_office_supported = decl_supports_hosted_mcp(&id, decl);
            let steering_supported = decl.and_then(|d| d.steering).unwrap_or(false);
            let goals_supported = matches!(id.as_str(), "codex" | "dsh")
                && decl.is_some_and(|d| !d.refused_session_options.contains_key("goals"));

            let availability = match row.installation {
                Probe::Ok {
                    value:
                        InstallationSnapshot::Available {
                            command, version, ..
                        },
                    ..
                } => ProviderAvailability::Available {
                    version: version.unwrap_or_else(|| "installed".into()),
                    command,
                },
                Probe::Ok {
                    value: InstallationSnapshot::Missing,
                    ..
                } => ProviderAvailability::Missing,
                Probe::Ok {
                    value: InstallationSnapshot::Failed { reason, .. },
                    ..
                } => ProviderAvailability::Failed { reason },
                Probe::Failed { reason, .. } => ProviderAvailability::Failed {
                    reason: reason.to_owned(),
                },
                Probe::TimedOut { .. } => ProviderAvailability::Failed {
                    reason: "检查安装状态超时".into(),
                },
                Probe::Skipped { .. } | Probe::Unsupported => ProviderAvailability::Missing,
            };

            let auth = match row.auth {
                Probe::Ok {
                    value: AuthStatus::LoggedIn { source, account },
                    ..
                } => {
                    let summary = account
                        .as_ref()
                        .and_then(|a| a.email.clone().or_else(|| a.method.clone()))
                        .unwrap_or(source);
                    let plan = account.as_ref().and_then(|a| a.plan.clone());
                    ProviderAuthState::LoggedIn { summary, plan }
                }
                Probe::Ok {
                    value: AuthStatus::LoggedOut { source, detail, .. },
                    ..
                } => {
                    let src = match detail {
                        Some(d) if !d.is_empty() => format!("{source} ({d})"),
                        _ => source,
                    };
                    ProviderAuthState::LoggedOut { source: src }
                }
                Probe::Ok {
                    value: AuthStatus::Unknown { detail, .. },
                    ..
                } => ProviderAuthState::Unknown { note: detail },
                Probe::Failed { reason, .. } => ProviderAuthState::Unknown {
                    note: reason.to_owned(),
                },
                Probe::TimedOut { .. } => ProviderAuthState::Unknown {
                    note: "登录状态检查超时".into(),
                },
                Probe::Skipped { .. } | Probe::Unsupported => {
                    if matches!(availability, ProviderAvailability::Available { .. }) {
                        ProviderAuthState::Unknown {
                            note: "由程序自行管理登录".into(),
                        }
                    } else {
                        ProviderAuthState::NotChecked
                    }
                }
            };

            let models = match row.models {
                Some(Probe::Ok { value, .. }) => value
                    .iter()
                    .filter_map(ProviderModelOption::from_kar_entry)
                    .collect(),
                _ => Vec::new(),
            };
            let quota_summary = summarize_account_usage(row.account_usage.as_ref());

            ProviderStatusInfo {
                id,
                protocol,
                availability,
                auth,
                mcp_office_supported,
                steering_supported,
                goals_supported,
                can_install,
                install_lines,
                models,
                quota_summary,
            }
        })
        .collect()
}

fn state_from_replica(
    info: Option<ChatInfo>,
    replica: &ChatReplica,
    caps: Option<Capabilities>,
    queued_inputs: Vec<QueuedInput>,
) -> ProjectedChatState {
    let chat_id = info.as_ref().map(|i| i.id);
    ProjectedChatState {
        chat_id,
        info,
        turns: replica.turns().to_vec(),
        pending_requests: replica.pending_requests().to_vec(),
        queued_inputs,
        context_usage: replica.context_usage().cloned(),
        usage: replica.usage().to_vec(),
        goal: replica.goal().map(|g| g.cloned()),
        tasks: replica.tasks().to_vec(),
        manual_compaction: replica.manual_compaction().cloned(),
        workspace_position: None,
        can_fork: caps.is_some_and(|c| c.fork),
        can_steer: caps.is_some_and(|c| c.steering),
        can_compact: caps.is_some_and(|c| c.manual_compaction),
        can_goals: caps.is_some_and(|c| c.goals),
        can_stop_task: caps.is_some_and(|c| c.stop_task),
        can_images: caps.is_some_and(|c| c.image_input),
        cursor: Some(replica.cursor()),
        is_demo: false,
    }
}

fn state_from_history(
    history: ChatHistory,
    caps: Option<Capabilities>,
    queued_inputs: Vec<QueuedInput>,
) -> ProjectedChatState {
    let chat_id = Some(history.chat.id);
    ProjectedChatState {
        chat_id,
        info: Some(history.chat),
        turns: history.turns,
        pending_requests: history.pending_requests,
        queued_inputs,
        context_usage: history.context_usage,
        usage: history.usage,
        goal: history.goal,
        tasks: history.tasks,
        manual_compaction: history.manual_compaction,
        workspace_position: history.workspace_position,
        can_fork: caps.is_some_and(|c| c.fork),
        can_steer: caps.is_some_and(|c| c.steering),
        can_compact: caps.is_some_and(|c| c.manual_compaction),
        can_goals: caps.is_some_and(|c| c.goals),
        can_stop_task: caps.is_some_and(|c| c.stop_task),
        can_images: caps.is_some_and(|c| c.image_input),
        cursor: None,
        is_demo: false,
    }
}

/// Apply one `ChatEvent` to `replica`.
/// - Duplicate events (`Ok(false)`) increment `duplicates_ignored` without duplicating turns/tools.
/// - Cursor gaps (`Err(KarError::InvalidCursor)`) re-fetch `/snapshot` and rebuild `replica`.
pub async fn apply_http_event_with_gap_recovery(
    feed: &HttpChatFeed,
    replica: &mut ChatReplica,
    info: &mut Option<ChatInfo>,
    event_result: kar::Result<ChatEvent>,
    duplicates_ignored: &mut u64,
    gaps_recovered: &mut u64,
) -> kar::Result<bool> {
    match event_result {
        Ok(event) => match replica.apply(event) {
            Ok(true) => Ok(true),
            Ok(false) => {
                *duplicates_ignored += 1;
                Ok(false)
            }
            Err(KarError::InvalidCursor) => {
                *gaps_recovered += 1;
                let fresh = feed.snapshot().await?;
                *info = Some(fresh.history.chat.clone());
                *replica = ChatReplica::from_snapshot(fresh);
                Ok(true)
            }
            Err(other) => Err(other),
        },
        Err(KarError::InvalidCursor) => {
            *gaps_recovered += 1;
            let fresh = feed.snapshot().await?;
            *info = Some(fresh.history.chat.clone());
            *replica = ChatReplica::from_snapshot(fresh);
            Ok(true)
        }
        Err(other) => Err(other),
    }
}

fn parse_http_host_port(base_url: &str) -> Result<String, String> {
    let trimmed = base_url.trim();
    let (rest, default_port) = if let Some(r) = trimmed.strip_prefix("http://") {
        (r, 80u16)
    } else if let Some(r) = trimmed.strip_prefix("https://") {
        (r, 443u16)
    } else {
        return Err(format!("需要 http:// 或 https:// 地址: {trimmed}"));
    };
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        return Err("缺少主机地址".into());
    }
    if authority.contains(':') {
        Ok(authority.to_owned())
    } else {
        Ok(format!("{authority}:{default_port}"))
    }
}

async fn check_http_reachable(base_url: &str) -> Result<(), String> {
    let addr = parse_http_host_port(base_url)?;
    match tokio::time::timeout(
        Duration::from_millis(250),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err(format!("连接 {addr} 超时")),
    }
}

fn spawn_local_follower(
    store: ChatStore,
    store_dir: PathBuf,
    chat_id: Uuid,
    caps: Option<Capabilities>,
    evt_tx: async_channel::Sender<KarBridgeEvent>,
) -> watch::Sender<bool> {
    let (cancel_tx, mut cancel_rx) = watch::channel(false);
    tokio::spawn(async move {
        let lock_path = store_dir.join(format!("{chat_id}.lock"));
        if !lock_path.exists() {
            let _ = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path);
        }
        let snap = match tokio::task::spawn_blocking({
            let store = store.clone();
            move || store.feed_snapshot(chat_id)
        })
        .await
        {
            Ok(Ok(s)) => s,
            Ok(Err(err)) => {
                let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                    FeedConnectionState::Disconnected {
                        last_cursor: None,
                        reason: err.to_string(),
                    },
                ));
                return;
            }
            Err(err) => {
                let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                    FeedConnectionState::Disconnected {
                        last_cursor: None,
                        reason: err.to_string(),
                    },
                ));
                return;
            }
        };

        let info = Some(snap.history.chat.clone());
        let mut replica = ChatReplica::from_snapshot(snap);
        let mut duplicates_ignored = 0u64;
        let mut gaps_recovered = 0u64;

        let queued = active_queued_inputs(Some(&store), chat_id);
        let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(state_from_replica(
            info.clone(),
            &replica,
            caps,
            queued,
        )));
        let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
            FeedConnectionState::Connected {
                cursor: Some(replica.cursor().to_string()),
                duplicates_ignored,
                gaps_recovered,
            },
        ));

        let mut feed = match tokio::task::spawn_blocking({
            let store = store.clone();
            let after = replica.cursor();
            move || store.follow(chat_id, Some(after))
        })
        .await
        {
            Ok(Ok(f)) => f,
            Ok(Err(err)) => {
                let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                    FeedConnectionState::Disconnected {
                        last_cursor: Some(replica.cursor().to_string()),
                        reason: err.to_string(),
                    },
                ));
                return;
            }
            Err(_) => return,
        };

        loop {
            tokio::select! {
                _ = cancel_rx.changed() => break,
                item = feed.recv() => {
                    match item {
                        Ok(ev) => {
                            match replica.apply(ev) {
                                Ok(true) => {
                                    let queued = active_queued_inputs(Some(&store), chat_id);
                                    let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                        state_from_replica(info.clone(), &replica, caps, queued),
                                    ));
                                    let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                                        FeedConnectionState::Connected {
                                            cursor: Some(replica.cursor().to_string()),
                                            duplicates_ignored,
                                            gaps_recovered,
                                        },
                                    ));
                                }
                                Ok(false) => {
                                    duplicates_ignored += 1;
                                }
                                Err(KarError::InvalidCursor) => {
                                    gaps_recovered += 1;
                                    if let Ok(Ok(fresh)) = tokio::task::spawn_blocking({
                                        let store = store.clone();
                                        move || store.feed_snapshot(chat_id)
                                    }).await {
                                        replica = ChatReplica::from_snapshot(fresh);
                                        let queued = active_queued_inputs(Some(&store), chat_id);
                                        let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                            state_from_replica(info.clone(), &replica, caps, queued),
                                        ));
                                    }
                                }
                                Err(err) => {
                                    let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                                        FeedConnectionState::Disconnected {
                                            last_cursor: Some(replica.cursor().to_string()),
                                            reason: err.to_string(),
                                        },
                                    ));
                                    break;
                                }
                            }
                        }
                        Err(err) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                                FeedConnectionState::Disconnected {
                                    last_cursor: Some(replica.cursor().to_string()),
                                    reason: err.to_string(),
                                },
                            ));
                            break;
                        }
                    }
                }
            }
        }
    });
    cancel_tx
}

/// Spawn an HTTP read-only subscriber backed by `HttpChatFeed` (`GET /snapshot` + `GET /events`)
/// and `ChatReplica`. Never falls back to demo data on disconnect.
pub fn spawn_http_subscriber(
    url: String,
    evt_tx: async_channel::Sender<KarBridgeEvent>,
) -> watch::Sender<bool> {
    spawn_http_subscriber_internal(url, None, evt_tx)
}

fn spawn_paired_http_subscriber(
    session: ActiveRemoteSession,
    evt_tx: async_channel::Sender<KarBridgeEvent>,
) -> watch::Sender<bool> {
    let url = session.url.clone();
    spawn_http_subscriber_internal(url, Some(session), evt_tx)
}

fn spawn_http_subscriber_internal(
    url: String,
    paired: Option<ActiveRemoteSession>,
    evt_tx: async_channel::Sender<KarBridgeEvent>,
) -> watch::Sender<bool> {
    let (cancel_tx, mut cancel_rx) = watch::channel(false);
    let _ = std::thread::Builder::new()
        .name("kar-http-feed-sub".into())
        .spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            let local = tokio::task::LocalSet::new();
            rt.block_on(local.run_until(async move {
                run_http_subscriber_loop(url, paired, evt_tx, &mut cancel_rx).await;
            }));
        });
    cancel_tx
}

async fn run_http_subscriber_loop(
    url: String,
    paired: Option<ActiveRemoteSession>,
    evt_tx: async_channel::Sender<KarBridgeEvent>,
    cancel_rx: &mut watch::Receiver<bool>,
) {
    let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
        FeedConnectionState::Connecting {
            endpoint: url.clone(),
        },
    ));

    if let Some(ref rem) = paired {
        let u = url.clone();
        let tok = rem.token.clone();
        if let Ok(Ok(ext)) =
            tokio::task::spawn_blocking(move || client_fetch_remote_state(&u, &tok)).await
        {
            let _ = evt_tx.send_blocking(KarBridgeEvent::RemoteExtendedStateUpdated {
                url: url.clone(),
                peer_id: rem.peer_id,
                snapshot: ext,
            });
        }
    }

    let mut feed = match HttpChatFeed::new(&url) {
        Ok(f) => f,
        Err(err) => {
            let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                FeedConnectionState::Disconnected {
                    last_cursor: None,
                    reason: format!("这个链接无法使用：{err}"),
                },
            ));
            return;
        }
    };

    // Phase 1: Acquire initial `GET /snapshot` with retry backoff while offline.
    // If paired against a DemoShowcase host (without a `.karchat` journal), still stay connected via `/v1/state`!
    let mut attempt = 0u32;
    let initial_snapshot = loop {
        if *cancel_rx.borrow() {
            return;
        }
        match feed.snapshot().await {
            Ok(snap) => break Some(snap),
            Err(err) => {
                if paired.is_some() && attempt >= 1 {
                    break None;
                }
                attempt += 1;
                let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                    FeedConnectionState::Reconnecting {
                        last_cursor: None,
                        reason: format!("暂时连不上：{err}"),
                        attempt,
                    },
                ));
                let delay = Duration::from_millis((250 * u64::from(attempt.min(10))).max(250));
                tokio::select! {
                    _ = cancel_rx.changed() => return,
                    _ = tokio::time::sleep(delay) => {}
                }
            }
        }
    };

    let Some(initial_snapshot) = initial_snapshot else {
        if let Some(rem) = paired {
            let _ = evt_tx.send_blocking(KarBridgeEvent::AccessModeChanged(
                ChatAccessMode::RemoteController {
                    url: url.clone(),
                    peer_id: rem.peer_id,
                    host_name: rem.host_name.clone(),
                    token: rem.token.clone(),
                    role: rem.role,
                    chat_id: rem.chat_id,
                },
            ));
            let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                FeedConnectionState::Connected {
                    cursor: Some("v1-state".into()),
                    duplicates_ignored: 0,
                    gaps_recovered: 0,
                },
            ));
            let mut sync_tick = tokio::time::interval(Duration::from_millis(1200));
            loop {
                tokio::select! {
                    _ = cancel_rx.changed() => break,
                    _ = sync_tick.tick() => {
                        let u = url.clone();
                        let tok = rem.token.clone();
                        match tokio::task::spawn_blocking(move || client_fetch_remote_state(&u, &tok)).await {
                            Ok(Ok(ext)) => {
                                let _ = evt_tx.send_blocking(KarBridgeEvent::RemoteExtendedStateUpdated { url: url.clone(), peer_id: rem.peer_id, snapshot: ext });
                            }
                            Ok(Err(err)) => {
                                let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                                    FeedConnectionState::Disconnected {
                                        last_cursor: None,
                                        reason: err.to_string(),
                                    },
                                ));
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
        }
        return;
    };

    let chat_id = initial_snapshot.history.chat.id;
    let mut info = Some(initial_snapshot.history.chat.clone());
    let mut replica = ChatReplica::from_snapshot(initial_snapshot);
    let mut duplicates_ignored = 0u64;
    let mut gaps_recovered = 0u64;
    let mut offline_attempt = 0u32;
    let mut is_currently_offline = false;

    let access_mode = match &paired {
        Some(rem) => ChatAccessMode::RemoteController {
            url: url.clone(),
            peer_id: rem.peer_id,
            host_name: rem.host_name.clone(),
            token: rem.token.clone(),
            role: rem.role,
            chat_id: Some(chat_id),
        },
        None => ChatAccessMode::HttpReadOnly {
            url: url.clone(),
            chat_id: Some(chat_id),
        },
    };
    let _ = evt_tx.send_blocking(KarBridgeEvent::AccessModeChanged(access_mode));
    let mut initial_proj = state_from_replica(info.clone(), &replica, None, Vec::new());
    if let Some(ref rem) = paired {
        initial_proj.can_steer = rem.role.can_mutate();
    }
    let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(initial_proj));
    let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
        FeedConnectionState::Connected {
            cursor: Some(replica.cursor().to_string()),
            duplicates_ignored,
            gaps_recovered,
        },
    ));

    // Phase 2: Stream `GET /events` via `HttpChatFeed::recv(committed_cursor)` while
    // monitoring TCP liveness + `/v1/state` sync when paired.
    let mut health_tick = tokio::time::interval(Duration::from_millis(450));
    health_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut state_sync_counter = 0u32;

    loop {
        let committed_cursor = replica.cursor();
        tokio::select! {
            _ = cancel_rx.changed() => break,
            _ = health_tick.tick() => {
                match check_http_reachable(&url).await {
                    Ok(()) => {
                        if is_currently_offline {
                            is_currently_offline = false;
                            offline_attempt = 0;
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                                FeedConnectionState::Connected {
                                    cursor: Some(replica.cursor().to_string()),
                                    duplicates_ignored,
                                    gaps_recovered,
                                },
                            ));
                        }
                        state_sync_counter = state_sync_counter.wrapping_add(1);
                        if let Some(ref rem) = paired && state_sync_counter.is_multiple_of(3) {
                            let u = url.clone();
                            let tok = rem.token.clone();
                            match tokio::task::spawn_blocking(move || {
                                let ping = RemoteCommandEnvelope {
                                    request_id: Uuid::new_v4(),
                                    expected_revision: None,
                                    action: RemoteAction::Ping { sent_ms: now_epoch_ms() },
                                };
                                let _ = client_send_remote_command(&u, &tok, &ping);
                                client_fetch_remote_state(&u, &tok)
                            }).await {
                                Ok(Ok(ext)) => {
                                    let _ = evt_tx.send_blocking(KarBridgeEvent::RemoteExtendedStateUpdated { url: url.clone(), peer_id: rem.peer_id, snapshot: ext });
                                }
                                Ok(Err(err)) if err.to_string().contains("403") => {
                                    let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                                        FeedConnectionState::Disconnected {
                                            last_cursor: Some(replica.cursor().to_string()),
                                            reason: "主机已撤销本设备的配对授权。".into(),
                                        },
                                    ));
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(reason) => {
                        is_currently_offline = true;
                        offline_attempt += 1;
                        let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                            FeedConnectionState::Reconnecting {
                                last_cursor: Some(replica.cursor().to_string()),
                                reason: format!("连接中断（{reason}），恢复后会从断开的位置继续。"),
                                attempt: offline_attempt,
                            },
                        ));
                    }
                }
            }
            ev_res = feed.recv(committed_cursor) => {
                match apply_http_event_with_gap_recovery(
                    &feed,
                    &mut replica,
                    &mut info,
                    ev_res,
                    &mut duplicates_ignored,
                    &mut gaps_recovered,
                )
                .await
                {
                    Ok(changed) => {
                        is_currently_offline = false;
                        offline_attempt = 0;
                        if changed {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                state_from_replica(info.clone(), &replica, None, Vec::new()),
                            ));
                        }
                        let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                            FeedConnectionState::Connected {
                                cursor: Some(replica.cursor().to_string()),
                                duplicates_ignored,
                                gaps_recovered,
                            },
                        ));
                    }
                    Err(err) => {
                        is_currently_offline = true;
                        offline_attempt += 1;
                        let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                            FeedConnectionState::Reconnecting {
                                last_cursor: Some(replica.cursor().to_string()),
                                reason: format!("连接异常（{err}），正在从断开的位置重试。"),
                                attempt: offline_attempt,
                            },
                        ));
                        tokio::time::sleep(Duration::from_millis(300)).await;
                    }
                }
            }
        }
    }
}

fn build_demo_seed_turns() -> Vec<ChatTurn> {
    let turn_id = Uuid::from_u128(0x019a_0000_0000_7000_8000_0000_0000_0001);
    let root_agent = Agent {
        session_id: Some("demo-session-01".into()),
        path: Vec::new(),
    };
    let read_tool = Tool {
        elided: false,
        name: Some("office_read_word".into()),
        native_type: Some("mcp".into()),
        server: Some("kantor".into()),
        tool_name: Some("office_read_word".into()),
        mcp_app_uri: None,
        finished: true,
        input: Some(serde_json::json!({"limit": 10, "queue": "src/sync/queue.ts"})),
        preview: String::new(),
        input_partial: false,
        result: Some(kar::ToolResult {
            status: Some(kar::ToolStatus::Succeeded),
            exit_code: Some(0),
            content: Some(vec![kar::OutputPart::Text {
                text: "{\"total_paragraphs\": 4, \"paragraphs\": [{\"paragraph\": 1, \"text\": \"Meeting minutes — offline queue & Kantor revision fence sync\"}]}".into(),
            }]),
            file_changes: vec![
                kar::FileChange {
                    path: "src/sync/queue.ts".into(),
                    kind: kar::ChangeKind::Modified,
                    move_to: None,
                },
                kar::FileChange {
                    path: "src/sync/reconcile.rs".into(),
                    kind: kar::ChangeKind::Added,
                    move_to: None,
                },
                kar::FileChange {
                    path: "docs/minutes_copy.docx".into(),
                    kind: kar::ChangeKind::Modified,
                    move_to: None,
                },
            ],
        }),
    };

    vec![ChatTurn {
        app_context: None,
        merged_context: None,
        requested_output_schema: None,
        background_note: None,
        id: turn_id,
        started_ms: 1_763_963_000_000,
        finished_ms: Some(1_763_963_134_000),
        status: TurnStatus::Completed,
        native_input_receipt: None,
        process_cleanups: Vec::new(),
        native_result_id: None,
        native_result_inputs: Vec::new(),
        failure: None,
        structured_output: None,
        native_continuation: false,
        steered_from: None,
        native_child_origin: None,
        child_sessions: BTreeMap::new(),
        attachments: Vec::new(),
        steered_attachments: BTreeMap::new(),
        pi_image_content_sha256: None,
        items: vec![
            TimelineItem {
                source: root_agent.clone(),
                item: Item::Message {
                    role: Role::User,
                    text: "Notes typed offline are lost on reload. Queue edits locally and sync them when we reconnect.".into(),
                    provider_id: None,
                    complete: true,
                },
            },
            TimelineItem {
                source: root_agent.clone(),
                item: Item::Message {
                    role: Role::Reasoning,
                    text: "Inspecting local IndexedDB queue persistence, Rust revision fence ledger, and Kantor Word document sync state…".into(),
                    provider_id: None,
                    complete: true,
                },
            },
            TimelineItem {
                source: root_agent.clone(),
                item: Item::Tool {
                    id: "call_demo_001".into(),
                    activity: read_tool,
                },
            },
            TimelineItem {
                source: root_agent,
                item: Item::Message {
                    role: Role::Assistant,
                    text: "Offline edits now survive a reload and sync cleanly when the connection returns.\n\n- **Queue first.** Every edit is written to local journal storage before it is sent, in `src/sync/queue.ts`.\n- **Replay in order.** On reconnect the queue drains oldest first in `src/sync/reconcile.rs` and stops at the first revision conflict.\n- **Office fence.** Synchronized Kantor notes in `docs/minutes_copy.docx` verify monotonic `expected_revision`.\n\n| Check | Result |\n| --- | --- |\n| Unit tests | 14 passed |\n| Clippy & typecheck | Clean (0 warnings) |\n| Bundle & snapshot overhead | +1.2 kB |\n\nI tested it end-to-end too: edited with the network off, reloaded, went back online, and the note saved cleanly. Want me to open a pull request?".into(),
                    provider_id: None,
                    complete: true,
                },
            },
        ],
        steered_inputs: BTreeMap::new(),
        fork_anchor: None,
        native_turn_id: Some("demo-turn-1".into()),
        pi_input_timestamp_ms: None,
        input_sent: true,
        checkpoint: Some(Checkpoint {
            parent: None,
            before_tree: "demo_tree_before".into(),
            after_tree: Some("demo_tree_after".into()),
            files: vec![
                CheckpointFileChange {
                    path: PathBuf::from("src/sync/queue.ts"),
                    kind: ChangeKind::Modified,
                },
                CheckpointFileChange {
                    path: PathBuf::from("src/sync/reconcile.rs"),
                    kind: ChangeKind::Added,
                },
                CheckpointFileChange {
                    path: PathBuf::from("docs/minutes_copy.docx"),
                    kind: ChangeKind::Modified,
                },
            ],
            repositories: Vec::new(),
        }),
    }]
}

async fn worker_loop(
    initial_store_dir: PathBuf,
    initial_workspace: PathBuf,
    cmd_tx: async_channel::Sender<KarCommand>,
    cmd_rx: async_channel::Receiver<KarCommand>,
    evt_tx: async_channel::Sender<KarBridgeEvent>,
) {
    let mcp_office = match start_office_mcp_server(evt_tx.clone()).await {
        Ok(server) => {
            emit_diag_log(
                &evt_tx,
                DiagLevel::Info,
                "kar_office",
                Some("MCP"),
                "Office MCP ready",
            );
            Some(server)
        }
        Err(err) => {
            emit_diag_log(
                &evt_tx,
                DiagLevel::Warn,
                "kar_office",
                Some("MCP"),
                format!("Office MCP unavailable: {err}"),
            );
            None
        }
    };
    let (remote_dispatch_tx, remote_dispatch_rx) =
        async_channel::bounded::<(Uuid, RemoteCommandEnvelope)>(128);
    let forward_cmd_tx = cmd_tx.clone();
    tokio::spawn(async move {
        while let Ok((peer_id, env)) = remote_dispatch_rx.recv().await {
            let _ = forward_cmd_tx
                .send(KarCommand::HandleRemoteCommand { peer_id, env })
                .await;
        }
    });
    let host_label = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .map(|u| format!("Kar ({u})"))
        .unwrap_or_else(|_| "Kar 桌面主机".into());
    let remote_link = Arc::new(std::sync::Mutex::new(RemoteLinkCore::new(
        host_label,
        Some(remote_dispatch_tx),
    )));

    let prompt_suggestions = kar::PromptSuggestions::new(true);
    let mut state = WorkerState {
        store_dir: initial_store_dir.clone(),
        workspace: initial_workspace.clone(),
        store: None,
        selected_chat_id: None,
        active_controller: None,
        feed_cancel: None,
        http_server: None,
        remote_link,
        remote_session: None,
        mcp_office,
        mcp_supported_runtimes: HashMap::new(),
        goals_supported_runtimes: HashMap::new(),
        chat_list_cache: HashMap::new(),
        probe_generation: 0,
        demo_turns: build_demo_seed_turns(),
        in_demo: false,
        running_terminals: HashMap::new(),
        prompt_suggestions,
        suggestion_watcher: None,
        cmd_tx,
        evt_tx,
    };

    init_store_and_providers(&mut state, initial_store_dir, initial_workspace);

    while let Ok(cmd) = cmd_rx.recv().await {
        match cmd {
            KarCommand::RefreshStoreAndProviders {
                store_dir,
                workspace,
            } => {
                init_store_and_providers(&mut state, store_dir, workspace);
            }
            KarCommand::ProbeProvidersOnly => {
                state.spawn_provider_probe(true);
            }
            KarCommand::ApplyProbedProviders {
                generation,
                providers,
            } => {
                if generation != state.probe_generation {
                    continue;
                }
                state.mcp_supported_runtimes = providers
                    .iter()
                    .map(|p| (p.id.clone(), p.mcp_office_supported))
                    .collect();
                state.goals_supported_runtimes = providers
                    .iter()
                    .map(|p| (p.id.clone(), p.goals_supported))
                    .collect();
                state.emit(KarBridgeEvent::ProvidersProbing(false));
                state.emit(KarBridgeEvent::ProvidersUpdated(providers));
            }
            KarCommand::ListChats => {
                state.refresh_chat_list();
            }
            KarCommand::RenameChat { chat_id, title } => {
                save_persisted_title(&state.store_dir, chat_id, Some(&title));
                state.chat_list_cache.remove(&chat_id);
                state.refresh_chat_list();
                state.emit(KarBridgeEvent::ToastMessage {
                    text: "对话标题已更新".into(),
                    warning: false,
                });
            }
            KarCommand::CreateChat {
                runtime_id,
                model,
                program,
                workspace,
                approval_mode,
                effort,
                use_worktree,
            } => {
                let Some(store) = state.store.clone() else {
                    state.emit(KarBridgeEvent::ChatCreateFailed {
                        cause: ChatCreateFailure::Store,
                        detail: format!(
                            "无法写入对话存储目录 {}，请检查它的权限。",
                            state.store_dir.display()
                        ),
                    });
                    continue;
                };
                state.in_demo = false;
                state.stop_feed();
                state.close_controller(false).await;

                // The runtime needs its suggestion source before the store assigns a chat ID.
                let source_id = Uuid::new_v4();
                let mut created_worktree: Option<ChatWorktreeMeta> = None;
                let effective_ws = if use_worktree {
                    let store_dir = state.store_dir.clone();
                    let ws_for_wt = workspace.clone();
                    let prepared = tokio::task::spawn_blocking(move || {
                        create_isolated_worktree_for_chat(&ws_for_wt, &store_dir, source_id)
                    })
                    .await.map_err(anyhow::Error::from).and_then(|result| result);
                    match prepared {
                        Ok(meta) => {
                            let path = meta.worktree_path.clone();
                            created_worktree = Some(meta);
                            path
                        }
                        Err(detail) => {
                            state.emit(KarBridgeEvent::ChatCreateFailed {
                                cause: ChatCreateFailure::Worktree,
                                detail: detail.to_string(),
                            });
                            continue;
                        }
                    }
                } else {
                    workspace.clone()
                };

                let _ = std::fs::create_dir_all(&effective_ws);
                let mut opts = SessionOptions::new(&effective_ws);
                opts.approvals = approval_mode;
                opts.model = model.filter(|m| !m.trim().is_empty());
                opts.prompt_suggestions = Some(state.prompt_suggestions.source(source_id));
                configure_runtime_path(&runtime_id, &mut opts);
                if runtime_supports_effort(&runtime_id) {
                    opts.settings.effort = effort;
                }
                if state
                    .goals_supported_runtimes
                    .get(&runtime_id)
                    .copied()
                    .unwrap_or(runtime_id == "codex")
                {
                    opts.settings.goals = true;
                }
                state.attach_office_mcp(&runtime_id, &mut opts);

                match store
                    .create(&runtime_id, program.as_deref(), opts)
                    .await
                {
                    Ok(session) => {
                        let chat_id = session.info().id;
                        state.suggestion_watcher = Some(spawn_prompt_suggestion_watcher(
                            state.prompt_suggestions.clone(),
                            source_id,
                            chat_id,
                            state.evt_tx.clone(),
                        ));
                        if let Some(meta) = created_worktree {
                            emit_diag_log(
                                &state.evt_tx,
                                DiagLevel::Info,
                                "git",
                                Some("WORKTREE"),
                                format!(
                                    "已为对话 {chat_id} 创建独立隔离分支 {} -> {}",
                                    meta.branch,
                                    meta.worktree_path.display()
                                ),
                            );
                            state.emit(KarBridgeEvent::WorktreeCreated { chat_id, meta });
                        }
                        state.selected_chat_id = Some(chat_id);
                        let caps = Some(session.capabilities());
                        let history = session.history().ok();
                        let session = Arc::new(session);
                        state.active_controller = Some(session);
                        state.emit(KarBridgeEvent::AccessModeChanged(
                            ChatAccessMode::WriteController { chat_id },
                        ));
                        if let Some(h) = history {
                            let queued = active_queued_inputs(Some(&store), chat_id);
                            state.emit(KarBridgeEvent::ChatStateUpdated(state_from_history(
                                h, caps, queued,
                            )));
                        }
                        state.feed_cancel = Some(spawn_local_follower(
                            store,
                            state.store_dir.clone(),
                            chat_id,
                            caps,
                            state.evt_tx.clone(),
                        ));
                        state.refresh_chat_list();
                    }
                    Err(err) => {
                        let mut detail = err.to_string();
                        if let Some(tree) = created_worktree
                            && let Err(cleanup) = kar::workbench_git::Repository::open(&tree.repo_root)
                                .and_then(|repo| repo.remove_worktree(tree.worktree_id))
                        {
                            detail.push_str(&format!(
                                "；工作树保留在 {}：{cleanup}",
                                tree.worktree_path.display()
                            ));
                        }
                        state.selected_chat_id = None;
                        state.emit(KarBridgeEvent::ChatCreateFailed {
                            cause: ChatCreateFailure::Runtime(runtime_id),
                            detail,
                        });
                    }
                }
            }
            KarCommand::CloseChat => {
                state.leave_chat().await;
            }
            KarCommand::SelectLocalChat {
                chat_id,
                prefer_read_only,
            } => {
                let Some(store) = state.store.clone() else {
                    continue;
                };
                state.in_demo = false;
                state.selected_chat_id = Some(chat_id);
                state.stop_feed();
                state.close_controller(false).await;

                if prefer_read_only {
                    state.emit(KarBridgeEvent::AccessModeChanged(
                        ChatAccessMode::LocalReadOnlySubscriber {
                            chat_id,
                            reason: "这个窗口只用来查看，内容会实时同步。".into(),
                        },
                    ));
                    state.feed_cancel = Some(spawn_local_follower(
                        store,
                        state.store_dir.clone(),
                        chat_id,
                        None,
                        state.evt_tx.clone(),
                    ));
                    continue;
                }

                let initial_history = read_chat_history_resilient(&store, &state.store_dir, chat_id);
                let rt_id = initial_history
                    .as_ref()
                    .ok()
                    .map(|h| h.chat.runtime.clone())
                    .unwrap_or_default();
                let workspace_exists = initial_history
                    .as_ref()
                    .ok()
                    .is_none_or(|h| h.chat.workspace.is_dir());

                // Fast path: if the recorded workspace directory was removed (e.g. a temp dir from an
                // imported or finished test session), do NOT launch a live follower or surface red I/O errors!
                if !workspace_exists
                    && let Ok(h) = initial_history
                {
                    let reason = format!(
                        "原工作区目录（{}）已不存在，当前作为历史归档查阅。",
                        h.chat.workspace.display()
                    );
                    state.emit(KarBridgeEvent::AccessModeChanged(
                        ChatAccessMode::ArchivedReadOnly { chat_id, reason },
                    ));
                    state.emit(KarBridgeEvent::ConnectionStateChanged(
                        FeedConnectionState::Idle,
                    ));
                    state.emit(KarBridgeEvent::ChatStateUpdated(state_from_history(
                        h,
                        None,
                        Vec::new(),
                    )));
                    continue;
                }

                let mcp_entry = if state
                    .mcp_supported_runtimes
                    .get(&rt_id)
                    .copied()
                    .unwrap_or_else(|| default_runtime_mcp_support(&rt_id))
                {
                    state.mcp_office.as_ref().map(ServedMcpTools::entry)
                } else {
                    None
                };
                let rt_for_path = rt_id.clone();
                let sugg_source = state.prompt_suggestions.source(chat_id);

                let configure = |opts: &mut SessionOptions| {
                    configure_runtime_path(&rt_for_path, opts);
                    opts.prompt_suggestions = Some(sugg_source.clone());
                    if let Some((name, srv)) = &mcp_entry {
                        opts.mcp_servers.insert(name.clone(), srv.clone());
                    }
                };
                let mut resumed = store.resume_with(chat_id, &configure).await;
                // Codex can still be releasing its native thread after the desktop
                // has closed. Retry the same thread, without sending a turn.
                for delay in [200, 500, 1000, 2000, 3500] {
                    let Err(err) = &resumed else { break };
                    let message = err.to_string().to_ascii_lowercase();
                    if rt_id != "codex" || matches!(err, KarError::Busy)
                        || message.contains("another controller")
                        || !(message.contains("thread") && message.contains("already")) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    resumed = store.resume_with(chat_id, &configure).await;
                }
                match resumed {
                    Ok(session) => {
                        state.suggestion_watcher = Some(spawn_prompt_suggestion_watcher(
                            state.prompt_suggestions.clone(),
                            chat_id,
                            chat_id,
                            state.evt_tx.clone(),
                        ));
                        let caps = Some(session.capabilities());
                        let session = Arc::new(session);
                        state.active_controller = Some(session);
                        state.emit(KarBridgeEvent::AccessModeChanged(
                            ChatAccessMode::WriteController { chat_id },
                        ));
                        state.feed_cancel = Some(spawn_local_follower(
                            store,
                            state.store_dir.clone(),
                            chat_id,
                            caps,
                            state.evt_tx.clone(),
                        ));
                        state.refresh_chat_list();
                    }
                    Err(err) => {
                        let err_text = err.to_string();
                        if err_text.contains("another controller") || matches!(err, KarError::Busy)
                        {
                            let reason =
                                "这个对话正在另一个窗口里进行，此处实时同步最新进展。".to_owned();
                            state.emit(KarBridgeEvent::AccessModeChanged(
                                ChatAccessMode::LocalReadOnlySubscriber { chat_id, reason },
                            ));
                            state.feed_cancel = Some(spawn_local_follower(
                                store,
                                state.store_dir.clone(),
                                chat_id,
                                None,
                                state.evt_tx.clone(),
                            ));
                        } else {
                            let friendly = if err_text.contains("No such file or directory") {
                                "原对话关联的目录或程序已变动，当前以归档模式查看。".to_owned()
                            } else {
                                format!("暂无法恢复会话连接（{err_text}），当前以归档模式查看。")
                            };
                            state.emit(KarBridgeEvent::AccessModeChanged(
                                ChatAccessMode::ArchivedReadOnly {
                                    chat_id,
                                    reason: friendly,
                                },
                            ));
                            state.emit(KarBridgeEvent::ConnectionStateChanged(
                                FeedConnectionState::Idle,
                            ));
                            if let Ok(h) =
                                read_chat_history_resilient(&store, &state.store_dir, chat_id)
                            {
                                state.emit(KarBridgeEvent::ChatStateUpdated(state_from_history(
                                    h,
                                    None,
                                    Vec::new(),
                                )));
                            }
                        }
                    }
                }
            }
            KarCommand::SendPrompt {
                request_id,
                raw_user_text,
                payload,
                target,
                attachments,
            } => {
                state.send_prompt(request_id, raw_user_text, payload, target, attachments);
            }
            KarCommand::SteerOrQueuePrompt {
                request_id,
                raw_user_text: _,
                payload,
                target,
                attachments,
            } => {
                if let Some(ref tgt) = target {
                    save_persisted_target(&state.store_dir, request_id, tgt.clone());
                    state.emit(KarBridgeEvent::TurnTargetsLoaded(load_persisted_targets(
                        &state.store_dir,
                    )));
                }
                let Some(session) = state.active_controller.clone() else {
                    let _ = state.dispatch_remote_envelope(RemoteCommandEnvelope {
                        request_id,
                        expected_revision: None,
                        action: RemoteAction::SteerOrQueue {
                            text: payload,
                            target,
                        },
                    });
                    continue;
                };
                let chat_id = session.info().id;
                let evt_tx = state.evt_tx.clone();
                let store_opt = state.store.clone();
                let caps = session.capabilities();
                // Auto-background foreground commands that have already exceeded 30s when new guidance arrives!
                if let Ok(h) = session.history()
                    && let Some(running_turn) = h.turns.last().filter(|t| t.status == TurnStatus::Running)
                {
                    let now_ms = now_epoch_ms();
                    if now_ms.saturating_sub(running_turn.started_ms) >= AUTO_BACKGROUND_THRESHOLD_MS {
                        for ti in &running_turn.items {
                            if let Item::Tool { id, activity } = &ti.item
                                && !activity.finished
                            {
                                let cmd_summary = activity
                                    .tool_name
                                    .clone()
                                    .or_else(|| activity.name.clone())
                                    .unwrap_or_else(|| "foreground_command".into());
                                state.emit(KarBridgeEvent::AutoBackgroundedCommand(
                                    BackgroundedCommand {
                                        tool_id: id.clone(),
                                        turn_id: running_turn.id,
                                        command_summary: cmd_summary,
                                        started_ms: running_turn.started_ms,
                                        promoted_ms: now_ms,
                                        auto_promoted_by_guidance: true,
                                        follow_up_requested: true,
                                        completed: false,
                                        exit_code: None,
                                        output_tail: String::new(),
                                        follow_up_delivered: false,
                                    },
                                ));
                            }
                        }
                    }
                }
                tokio::spawn(async move {
                    if attachments.is_empty() && caps.steering {
                        match session.steer_with_id(request_id, payload.clone()).await {
                            Ok(()) => {
                                let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                    text: "已即时补充给正在执行的智能体".into(),
                                    warning: false,
                                });
                                return;
                            }
                            Err(
                                KarError::Unsupported(_) | KarError::Refused(_) | KarError::Busy,
                            ) => {
                                // Safe to fall back to queue: the provider explicitly did not accept steering.
                            }
                            Err(err) => {
                                let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                    text: format!("补充消息发送异常：{err}"),
                                    warning: true,
                                });
                                return;
                            }
                        }
                    }

                    let enqueue_res = if attachments.is_empty() {
                        session.enqueue(request_id, payload)
                    } else if let Some(ref store) = store_opt {
                        match persist_attachment_paths(store, chat_id, &attachments) {
                            Ok(att_ids) => {
                                store.enqueue_attached(chat_id, request_id, payload, &att_ids)
                            }
                            Err(err) => Err(err),
                        }
                    } else {
                        session.enqueue(request_id, payload)
                    };

                    match enqueue_res {
                        Ok(_) => {
                            if let Ok(h) = session.history() {
                                let queued = active_queued_inputs(store_opt.as_ref(), chat_id);
                                let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                    state_from_history(h, Some(caps), queued),
                                ));
                            }
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: "已加入待发队列，当前回合完成后自动发送".into(),
                                warning: false,
                            });
                        }
                        Err(err) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("无法加入队列：{err}"),
                                warning: true,
                            });
                        }
                    }
                });
            }
            KarCommand::RemoteQueuedAction {
                url,
                peer_id,
                chat_id,
                input_id,
                copy_to_draft,
            } => {
                let Some(remote) = state.remote_session.as_ref().filter(|r| {
                    r.url == url
                        && r.peer_id == peer_id
                        && r.chat_id == Some(chat_id)
                        && r.role.can_mutate()
                }) else {
                    state.emit(KarBridgeEvent::RemoteQueueCopied {
                        url,
                        peer_id,
                        chat_id,
                        result: Err(QueueCopyError::Changed),
                    });
                    continue;
                };
                if !copy_to_draft {
                    state.dispatch_remote_envelope(RemoteCommandEnvelope {
                        request_id: Uuid::new_v4(),
                        expected_revision: None,
                        action: RemoteAction::WithdrawQueued {
                            chat_id: Some(chat_id),
                            input_id,
                        },
                    });
                    continue;
                }
                let token = remote.token.clone();
                let endpoint = url.clone();
                let events = state.evt_tx.clone();
                tokio::spawn(async move {
                    let result = tokio::task::spawn_blocking(move || {
                        client_fetch_remote_state(&endpoint, &token)
                            .map_err(|_| QueueCopyError::Unavailable)?
                            .pending_text(chat_id, input_id)
                    })
                    .await
                    .unwrap_or(Err(QueueCopyError::Unavailable));
                    let _ = events
                        .send(KarBridgeEvent::RemoteQueueCopied {
                            url,
                            peer_id,
                            chat_id,
                            result,
                        })
                        .await;
                });
            }
            KarCommand::WithdrawQueued {
                chat_id,
                input_id,
                edit,
            } => {
                let Some(store) = state.store.clone() else {
                    continue;
                };
                let attachments = if edit {
                    let queued = store.queued_inputs(chat_id).ok().and_then(|entries| {
                        entries
                            .into_iter()
                            .find(|q| q.id == input_id && q.status == InputStatus::Pending)
                    });
                    let Some(queued) = queued else {
                        state.emit(KarBridgeEvent::ToastMessage {
                            text: "该消息已离开待发队列，草稿已保留".into(),
                            warning: true,
                        });
                        continue;
                    };
                    let paths = queued
                        .attachments
                        .iter()
                        .map(|a| {
                            state
                                .store_dir
                                .join("attachments")
                                .join(a.id.to_string())
                                .join("content")
                                .join(&a.name)
                        })
                        .collect::<Vec<_>>();
                    if queued.attachments.iter().any(|a| Path::new(&a.name).file_name().and_then(|n| n.to_str()) != Some(a.name.as_str()))
                        || paths.iter().any(|path| !path.is_file()) {
                        state.emit(KarBridgeEvent::ToastMessage {
                            text: "无法读取排队消息的附件，消息与草稿已保留".into(),
                            warning: true,
                        });
                        continue;
                    }
                    paths
                } else {
                    Vec::new()
                };
                match store.withdraw(chat_id, input_id) {
                    Ok(receipt) => {
                        if let Some(ref session) = state.active_controller
                            && session.info().id == chat_id
                            && let Ok(h) = session.history()
                        {
                            let caps = Some(session.capabilities());
                            let queued = active_queued_inputs(Some(&store), chat_id);
                            state.emit(KarBridgeEvent::ChatStateUpdated(state_from_history(
                                h, caps, queued,
                            )));
                        }
                        if edit {
                            state.emit(KarBridgeEvent::QueuedInputRetrieved {
                                chat_id,
                                input_id,
                                text: receipt.text,
                                attachments,
                            });
                        } else {
                            state.emit(KarBridgeEvent::ToastMessage {
                                text: "已撤回排队消息".into(),
                                warning: false,
                            });
                        }
                    }
                    Err(err) => {
                        state.emit(KarBridgeEvent::ToastMessage {
                            text: format!("无法撤回排队消息：{err}"),
                            warning: true,
                        });
                    }
                }
            }
            KarCommand::AbortTurn => {
                if let Some(session) = state.active_controller.clone() {
                    let evt_tx = state.evt_tx.clone();
                    tokio::spawn(async move {
                        if let Err(err) = session.abort().await {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::BannerNotice {
                                title: "没能停下来".into(),
                                detail: err.to_string(),
                                is_error: true,
                            });
                        }
                    });
                } else {
                    let _ = state.dispatch_remote_envelope(RemoteCommandEnvelope {
                        request_id: Uuid::new_v4(),
                        expected_revision: None,
                        action: RemoteAction::AbortTurn,
                    });
                }
            }
            KarCommand::DecidePermission { request, decision } => {
                if let Some(session) = state.active_controller.clone() {
                    let chat_id = session.info().id;
                    let store_opt = state.store.clone();
                    let evt_tx = state.evt_tx.clone();
                    let caps = Some(session.capabilities());
                    tokio::spawn(async move {
                        if let Err(err) = session.decide(&request, decision).await {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::BannerNotice {
                                title: "你的选择没有送达".into(),
                                detail: err.to_string(),
                                is_error: true,
                            });
                        } else if let Ok(h) = session.history() {
                            let queued = active_queued_inputs(store_opt.as_ref(), chat_id);
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                state_from_history(h, caps, queued),
                            ));
                        }
                    });
                } else {
                    let allow = matches!(
                        decision,
                        kar::PermissionDecision::AllowOnce
                            | kar::PermissionDecision::AllowForTurn
                            | kar::PermissionDecision::AllowForSession
                            | kar::PermissionDecision::AllowAlways
                    );
                    let _ = state.dispatch_remote_envelope(RemoteCommandEnvelope {
                        request_id: Uuid::new_v4(),
                        expected_revision: None,
                        action: RemoteAction::DecidePermission {
                            pending_id: request.id.to_string(),
                            allow,
                        },
                    });
                }
            }
            KarCommand::SelectPolicy { request, choice } => {
                if let Some(session) = state.active_controller.clone() {
                    let chat_id = session.info().id;
                    let store_opt = state.store.clone();
                    let evt_tx = state.evt_tx.clone();
                    let caps = Some(session.capabilities());
                    tokio::spawn(async move {
                        if let Err(err) = session.select_policy(&request, choice).await {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::BannerNotice {
                                title: "权限规则没有生效".into(),
                                detail: err.to_string(),
                                is_error: true,
                            });
                        } else if let Ok(h) = session.history() {
                            let queued = active_queued_inputs(store_opt.as_ref(), chat_id);
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                state_from_history(h, caps, queued),
                            ));
                        }
                    });
                }
            }
            KarCommand::AnswerQuestion { request, reply } => {
                if let Some(session) = state.active_controller.clone() {
                    let chat_id = session.info().id;
                    let store_opt = state.store.clone();
                    let evt_tx = state.evt_tx.clone();
                    let caps = Some(session.capabilities());
                    tokio::spawn(async move {
                        if let Err(err) = session.answer_question(&request, reply).await {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::BannerNotice {
                                title: "你的回答没有送达".into(),
                                detail: err.to_string(),
                                is_error: true,
                            });
                        } else if let Ok(h) = session.history() {
                            let queued = active_queued_inputs(store_opt.as_ref(), chat_id);
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                state_from_history(h, caps, queued),
                            ));
                        }
                    });
                }
            }
            KarCommand::ForkChatAt {
                chat_id,
                turn_id,
                model_override,
            } => {
                let Some(store) = state.store.clone() else {
                    continue;
                };
                if let Ok(h) = read_chat_history_resilient(&store, &state.store_dir, chat_id) {
                    let forkable = h
                        .turns
                        .iter()
                        .find(|t| t.id == turn_id)
                        .and_then(|t| t.checkpoint.as_ref())
                        .and_then(|cp| cp.after_tree.as_ref())
                        .is_some();
                    if !forkable {
                        state.emit(KarBridgeEvent::ToastMessage {
                            text: "该回合尚未生成完整检查点，完成后才可分叉".into(),
                            warning: true,
                        });
                        continue;
                    }
                }

                state.in_demo = false;
                state.stop_feed();
                state.close_controller(false).await;

                let rt_id = read_chat_history_resilient(&store, &state.store_dir, chat_id)
                    .ok()
                    .map(|h| h.chat.runtime)
                    .unwrap_or_default();
                let mcp_entry = if state
                    .mcp_supported_runtimes
                    .get(&rt_id)
                    .copied()
                    .unwrap_or_else(|| default_runtime_mcp_support(&rt_id))
                {
                    state.mcp_office.as_ref().map(ServedMcpTools::entry)
                } else {
                    None
                };
                let mcp_fallback = mcp_entry.clone();
                let rt_for_fork = rt_id.clone();
                let rt_for_fallback = rt_id.clone();
                let next_model = model_override.filter(|m| !m.trim().is_empty());

                match store
                    .fork_at_with(chat_id, turn_id, move |opts| {
                        configure_runtime_path(&rt_for_fork, opts);
                        if let Some(m) = next_model {
                            opts.model = Some(m);
                        }
                        if let Some((name, srv)) = mcp_entry {
                            opts.mcp_servers.insert(name, srv);
                        }
                    })
                    .await
                {
                    Ok(forked) => {
                        let new_id = forked.info().id;
                        state.selected_chat_id = Some(new_id);
                        let caps = Some(forked.capabilities());
                        let history = forked.history().ok();
                        state.active_controller = Some(Arc::new(forked));
                        state.emit(KarBridgeEvent::AccessModeChanged(
                            ChatAccessMode::WriteController { chat_id: new_id },
                        ));
                        if let Some(h) = history {
                            let queued = active_queued_inputs(Some(&store), new_id);
                            state.emit(KarBridgeEvent::ChatStateUpdated(state_from_history(
                                h, caps, queued,
                            )));
                        }
                        state.feed_cancel = Some(spawn_local_follower(
                            store,
                            state.store_dir.clone(),
                            new_id,
                            caps,
                            state.evt_tx.clone(),
                        ));
                        state.refresh_chat_list();
                        state.emit(KarBridgeEvent::ToastMessage {
                            text: "已从此步分叉出新对话".into(),
                            warning: false,
                        });
                    }
                    Err(err) => {
                        state.emit(KarBridgeEvent::ToastMessage {
                            text: format!("无法分叉：{err}"),
                            warning: true,
                        });
                        // Re-open original chat preserving `kantor` MCP and PATH.
                        if let Ok(resumed) = store
                            .resume_with(chat_id, move |opts| {
                                configure_runtime_path(&rt_for_fallback, opts);
                                if let Some((name, srv)) = mcp_fallback {
                                    opts.mcp_servers.insert(name, srv);
                                }
                            })
                            .await
                        {
                            state.selected_chat_id = Some(chat_id);
                            let caps = Some(resumed.capabilities());
                            state.active_controller = Some(Arc::new(resumed));
                            state.emit(KarBridgeEvent::AccessModeChanged(
                                ChatAccessMode::WriteController { chat_id },
                            ));
                            state.feed_cancel = Some(spawn_local_follower(
                                store,
                                state.store_dir.clone(),
                                chat_id,
                                caps,
                                state.evt_tx.clone(),
                            ));
                        }
                    }
                }
            }
            KarCommand::RollbackToTurn { turn_id, before } => {
                let Some(session) = state.active_controller.clone() else {
                    continue;
                };
                let chat_id = session.info().id;
                let workspace = session.info().workspace.clone();
                let store_opt = state.store.clone();
                let evt_tx = state.evt_tx.clone();
                let caps = Some(session.capabilities());
                tokio::spawn(async move {
                    let pt = CheckpointPoint {
                        turn: turn_id,
                        before,
                    };
                    match session.rollback(pt, false).await {
                        Ok(report) => {
                            if let Ok(h) = session.history() {
                                let queued = active_queued_inputs(store_opt.as_ref(), chat_id);
                                let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                    state_from_history(h, caps, queued),
                                ));
                            }
                            if report.conflicts.is_empty() && !report.files.is_empty() {
                                let restored: Vec<PathBuf> = report
                                    .files
                                    .iter()
                                    .map(|fc| workspace.join(&fc.path))
                                    .collect();
                                let _ = evt_tx.send_blocking(
                                    KarBridgeEvent::WorkspaceFilesRestored { files: restored },
                                );
                            }
                            let msg = if report.conflicts.is_empty() {
                                if before {
                                    format!("已撤销此步及之后的 {} 个文件改动", report.files.len())
                                } else {
                                    format!("已回退 {} 个工作区文件到此步状态", report.files.len())
                                }
                            } else {
                                format!(
                                    "存在 {} 处未保存冲突，为保护本地修改未覆盖这些文件",
                                    report.conflicts.len()
                                )
                            };
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: msg,
                                warning: !report.conflicts.is_empty(),
                            });
                        }
                        Err(err) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("回退文件失败：{err}"),
                                warning: true,
                            });
                        }
                    }
                });
            }
            KarCommand::LoadTurnDiff(key) => {
                let store = state.store.clone();
                let evt_tx = state.evt_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let result = store.ok_or_else(|| "本地对话存储不可用".to_owned())
                        .and_then(|store| crate::turn_review::file_patch(&store, &key));
                    let _ = evt_tx.send_blocking(KarBridgeEvent::TurnDiffLoaded { key, result });
                });
            }
            KarCommand::CompactChat => {
                let Some(session) = state.active_controller.clone() else {
                    continue;
                };
                let chat_id = session.info().id;
                let store_opt = state.store.clone();
                let evt_tx = state.evt_tx.clone();
                let caps = Some(session.capabilities());
                tokio::spawn(async move {
                    match session.start_compaction().await {
                        Ok(handle) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: "正在整理对话上下文…".into(),
                                warning: false,
                            });
                            let report = handle.wait(Some(Duration::from_secs(45))).await;
                            if let Ok(h) = session.history() {
                                let queued = active_queued_inputs(store_opt.as_ref(), chat_id);
                                let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                    state_from_history(h, caps, queued),
                                ));
                            }
                            match report.status {
                                CompactionStatus::Completed { .. } => {
                                    let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                        text: "对话上下文已整理完毕".into(),
                                        warning: false,
                                    });
                                }
                                CompactionStatus::Rejected { reason, .. }
                                | CompactionStatus::Failed { reason, .. } => {
                                    let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                        text: format!("上下文整理未完成：{reason}"),
                                        warning: true,
                                    });
                                }
                                _ => {
                                    let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                        text: "上下文整理状态待确认".into(),
                                        warning: true,
                                    });
                                }
                            }
                        }
                        Err(err) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("无法整理上下文：{err}"),
                                warning: true,
                            });
                        }
                    }
                });
            }
            KarCommand::SetGoal { objective } => {
                let Some(session) = state.active_controller.clone() else {
                    continue;
                };
                let chat_id = session.info().id;
                let store_opt = state.store.clone();
                let evt_tx = state.evt_tx.clone();
                let caps = Some(session.capabilities());
                tokio::spawn(async move {
                    let opts = GoalSetOptions {
                        objective,
                        status: Some("active".into()),
                        token_budget: None,
                    };
                    match session.set_goal(opts).await {
                        Ok(_) => {
                            if let Ok(h) = session.history() {
                                let queued = active_queued_inputs(store_opt.as_ref(), chat_id);
                                let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                    state_from_history(h, caps, queued),
                                ));
                            }
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: "已更新对话目标".into(),
                                warning: false,
                            });
                        }
                        Err(err) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("设置目标失败：{err}"),
                                warning: true,
                            });
                        }
                    }
                });
            }
            KarCommand::ClearGoal => {
                let Some(session) = state.active_controller.clone() else {
                    continue;
                };
                let chat_id = session.info().id;
                let store_opt = state.store.clone();
                let evt_tx = state.evt_tx.clone();
                let caps = Some(session.capabilities());
                tokio::spawn(async move {
                    match session.clear_goal().await {
                        Ok(()) => {
                            if let Ok(h) = session.history() {
                                let queued = active_queued_inputs(store_opt.as_ref(), chat_id);
                                let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                    state_from_history(h, caps, queued),
                                ));
                            }
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: "已清除对话目标".into(),
                                warning: false,
                            });
                        }
                        Err(err) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("清除目标失败：{err}"),
                                warning: true,
                            });
                        }
                    }
                });
            }
            KarCommand::StopTask { task_id } => {
                let Some(session) = state.active_controller.clone() else {
                    continue;
                };
                let chat_id = session.info().id;
                let store_opt = state.store.clone();
                let evt_tx = state.evt_tx.clone();
                let caps = Some(session.capabilities());
                tokio::spawn(async move {
                    match session.stop_task(&task_id).await {
                        Ok(()) => {
                            if let Ok(h) = session.history() {
                                let queued = active_queued_inputs(store_opt.as_ref(), chat_id);
                                let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                    state_from_history(h, caps, queued),
                                ));
                            }
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: "已停止后台任务".into(),
                                warning: false,
                            });
                        }
                        Err(err) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("无法停止任务：{err}"),
                                warning: true,
                            });
                        }
                    }
                });
            }
            KarCommand::DeleteChat { chat_id } => {
                let is_selected = state.selected_chat_id == Some(chat_id)
                    || state
                        .active_controller
                        .as_ref()
                        .is_some_and(|s| s.info().id == chat_id);
                if is_selected {
                    state.leave_chat().await;
                }
                if state
                    .http_server
                    .as_ref()
                    .is_some_and(|srv| srv.chat_id == chat_id)
                    && let Some(srv) = state.http_server.take()
                {
                    srv.stop().await;
                    state.emit(KarBridgeEvent::HttpServerState {
                        listening_url: None,
                        chat_id: Some(chat_id),
                        message: None,
                    });
                }

                let lock_file = state.store_dir.join(format!("{chat_id}.lock"));
                if let Ok(f) = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&lock_file)
                    && matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock))
                {
                    state.emit(KarBridgeEvent::ToastMessage {
                        text: "该对话正在另一个窗口运行中，请先结束后再删除".into(),
                        warning: true,
                    });
                    continue;
                }

                // Determine checkpoint store lineage before removing `.karchat` so we never delete
                // `{root_id}.git` while another fork still references it.
                let checkpoint_root = state
                    .store
                    .as_ref()
                    .and_then(|s| s.list().ok())
                    .and_then(|list| {
                        let target = list.iter().find(|i| i.id == chat_id)?;
                        let root = target
                            .forked_from
                            .as_ref()
                            .map_or(target.id, |f| f.checkpoint_store);
                        let shared = list.iter().any(|other| {
                            other.id != chat_id
                                && other
                                    .forked_from
                                    .as_ref()
                                    .map_or(other.id, |f| f.checkpoint_store)
                                    == root
                        });
                        (!shared).then_some(root)
                    });

                let karchat = state.store_dir.join(format!("{chat_id}.karchat"));
                let stop_lock = state.store_dir.join(format!("{chat_id}.stop.lock"));
                let inputs_file = state.store_dir.join(format!("{chat_id}.inputs"));
                let inputs_lock = state.store_dir.join(format!("{chat_id}.inputs.lock"));
                if let Err(err) = std::fs::remove_file(&karchat) {
                    state.emit(KarBridgeEvent::ToastMessage {
                        text: format!("无法删除对话：{err}"),
                        warning: true,
                    });
                } else {
                    let _ = std::fs::remove_file(&lock_file);
                    let _ = std::fs::remove_file(&stop_lock);
                    let _ = std::fs::remove_file(&inputs_file);
                    let _ = std::fs::remove_dir_all(&inputs_file);
                    let _ = std::fs::remove_file(&inputs_lock);
                    if let Some(root_id) = checkpoint_root {
                        let _ =
                            std::fs::remove_dir_all(state.store_dir.join(format!("{root_id}.git")));
                    }
                    save_persisted_title(&state.store_dir, chat_id, None);
                    state.chat_list_cache.remove(&chat_id);
                    state.refresh_chat_list();
                    state.emit(KarBridgeEvent::ToastMessage {
                        text: "对话已删除".into(),
                        warning: false,
                    });
                }
            }
            KarCommand::InstallAgent { runtime_id, line } => {
                let Ok(rt) = kar::runtime(&runtime_id) else {
                    state.emit(KarBridgeEvent::ToastMessage {
                        text: format!("未知智能体：{runtime_id}"),
                        warning: true,
                    });
                    continue;
                };
                let name = runtime_display_name(&runtime_id);
                state.emit(KarBridgeEvent::ToastMessage {
                    text: match &line {
                        Some(l) => format!("正在后台安装 {name} ({l})，请稍候…"),
                        None => format!("正在后台安装 {name}，请稍候…"),
                    },
                    warning: false,
                });
                let evt_tx = state.evt_tx.clone();
                let cmd_tx = state.cmd_tx.clone();
                tokio::spawn(async move {
                    match rt
                        .install_with(line.as_deref(), Duration::from_secs(180))
                        .await
                    {
                        Ok(_) => {
                            kar::clear_installation_cache();
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("{name} 安装完成，正在刷新状态…"),
                                warning: false,
                            });
                            let _ = cmd_tx.send(KarCommand::ProbeProvidersOnly).await;
                        }
                        Err(err) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("{name} 安装失败：{err}"),
                                warning: true,
                            });
                        }
                    }
                });
            }
            KarCommand::ConnectHttpFeed { url } => {
                state.in_demo = false;
                state.selected_chat_id = None;
                state.stop_feed();
                state.close_controller(false).await;
                state.emit(KarBridgeEvent::AccessModeChanged(
                    ChatAccessMode::HttpReadOnly {
                        url: url.clone(),
                        chat_id: None,
                    },
                ));
                state.feed_cancel = Some(spawn_http_subscriber(url, state.evt_tx.clone()));
            }
            KarCommand::DisconnectFeed => {
                if state.feed_cancel.is_some() {
                    state.leave_chat().await;
                }
            }
            KarCommand::StartHttpServer {
                chat_id,
                listen_addr,
            } => {
                if let Some(prev) = state.http_server.take() {
                    prev.stop().await;
                }
                match tokio::net::TcpListener::bind(&listen_addr).await {
                    Ok(listener) => {
                        let local_addr: SocketAddr = match listener.local_addr() {
                            Ok(a) => a,
                            Err(err) => {
                                state.emit(KarBridgeEvent::HttpServerState {
                                    listening_url: None,
                                    chat_id: None,
                                    message: Some(format!("无法开启共享：{err}")),
                                });
                                continue;
                            }
                        };
                        let url = format!("http://{local_addr}");
                        {
                            let now = now_epoch_ms();
                            let mut guard =
                                state.remote_link.lock().unwrap_or_else(|e| e.into_inner());
                            guard.listen_url = Some(url.clone());
                            guard.chat_id = (!chat_id.is_nil()).then_some(chat_id);
                            if guard.extended_snapshot.chat_id != guard.chat_id {
                                guard.extended_snapshot = RemoteExtendedSnapshot {
                                    chat_id: guard.chat_id,
                                    host_name: guard.host_name.clone(),
                                    ..Default::default()
                                };
                            }
                            if guard.ticket.as_ref().is_none_or(|t| now >= t.expires_ms) {
                                guard.issue_pairing_ticket(now);
                            }
                        }
                        let events = state.evt_tx.clone();
                        let link_router = remote_link_router_with_updates(
                            state.remote_link.clone(),
                            state
                                .store
                                .clone()
                                .filter(|_| !chat_id.is_nil())
                                .map(|store| (chat_id, store)),
                            move |snapshot| {
                                let _ = events
                                    .try_send(KarBridgeEvent::RemoteTelemetryUpdated(snapshot));
                            },
                        );
                        let router = if let Some(ref store) = state.store
                            && !chat_id.is_nil()
                        {
                            event_router(store.clone(), chat_id).merge(link_router)
                        } else {
                            link_router
                        };
                        let (stop_tx, stop_rx) = oneshot::channel::<()>();
                        let join = tokio::spawn(async move {
                            let _ = axum::serve(listener, router)
                                .with_graceful_shutdown(async move {
                                    let _ = stop_rx.await;
                                })
                                .await;
                        });
                        state.http_server = Some(ActiveHttpServer {
                            chat_id,
                            stop_tx,
                            join,
                        });
                        state.emit(KarBridgeEvent::HttpServerState {
                            listening_url: Some(url),
                            chat_id: (!chat_id.is_nil()).then_some(chat_id),
                            message: None,
                        });
                        state.emit_remote_telemetry();
                    }
                    Err(err) => {
                        state.emit(KarBridgeEvent::HttpServerState {
                            listening_url: None,
                            chat_id: None,
                            message: Some(format!("无法开启共享：{err}")),
                        });
                    }
                }
            }
            KarCommand::StopHttpServer => {
                if let Some(srv) = state.http_server.take() {
                    let chat_id = srv.chat_id;
                    srv.stop().await;
                    {
                        let mut guard = state.remote_link.lock().unwrap_or_else(|e| e.into_inner());
                        guard.listen_url = None;
                        guard.clear_ticket();
                    }
                    state.emit(KarBridgeEvent::HttpServerState {
                        listening_url: None,
                        chat_id: (!chat_id.is_nil()).then_some(chat_id),
                        message: None,
                    });
                    state.emit_remote_telemetry();
                }
            }
            KarCommand::IssuePairingCode => {
                let now = now_epoch_ms();
                {
                    let mut guard = state.remote_link.lock().unwrap_or_else(|e| e.into_inner());
                    guard.issue_pairing_ticket(now);
                }
                state.emit_remote_telemetry();
            }
            KarCommand::RevokePairedPeer { peer_id } => {
                {
                    let mut guard = state.remote_link.lock().unwrap_or_else(|e| e.into_inner());
                    guard.revoke_peer(peer_id);
                }
                state.emit_remote_telemetry();
            }
            KarCommand::SyncHostExtendedSnapshot { mut snapshot } => {
                {
                    let mut guard = state.remote_link.lock().unwrap_or_else(|e| e.into_inner());
                    if guard.chat_id != snapshot.chat_id {
                        continue;
                    }
                    snapshot.host_name = guard.host_name.clone();
                    snapshot.revision = guard.extended_snapshot.revision.saturating_add(1);
                    guard.extended_snapshot = snapshot;
                    guard.total_events_broadcast = guard.total_events_broadcast.saturating_add(1);
                }
                state.emit_remote_telemetry();
            }
            KarCommand::PairWithRemoteHost {
                url,
                code,
                device_name,
                role,
            } => {
                let evt_tx = state.evt_tx.clone();
                let u = url.clone();
                let pair_res = tokio::task::spawn_blocking(move || {
                    client_redeem_pair_code(&u, &code, &device_name, role)
                })
                .await;
                match pair_res {
                    Ok(Ok(resp)) => {
                        state.in_demo = false;
                        state.selected_chat_id = resp.chat_id;
                        state.stop_feed();
                        state.close_controller(false).await;
                        let session = ActiveRemoteSession {
                            url: url.clone(),
                            token: resp.token.clone(),
                            peer_id: resp.peer_id,
                            host_name: resp.host_name.clone(),
                            role: resp.role,
                            chat_id: resp.chat_id,
                        };
                        state.remote_session = Some(session.clone());
                        state.emit(KarBridgeEvent::AccessModeChanged(
                            ChatAccessMode::RemoteController {
                                url: url.clone(),
                                peer_id: resp.peer_id,
                                host_name: resp.host_name.clone(),
                                token: resp.token,
                                role: resp.role,
                                chat_id: resp.chat_id,
                            },
                        ));
                        state.feed_cancel = Some(spawn_paired_http_subscriber(session, evt_tx));
                    }
                    Ok(Err(err)) => {
                        let code_rejected = matches!(
                            err.downcast_ref::<ureq::Error>(),
                            Some(ureq::Error::StatusCode(401 | 410 | 429))
                        );
                        state.emit(KarBridgeEvent::RemotePairFailed { code_rejected });
                    }
                    Err(_) => state.emit(KarBridgeEvent::RemotePairFailed {
                        code_rejected: false,
                    }),
                }
            }
            KarCommand::HandleRemoteCommand { peer_id: _, env } => {
                state.emit_remote_telemetry();
                match env.action {
                    RemoteAction::SendPrompt { text, target } => {
                        let payload = match &target {
                            Some(tgt) => tgt.compose_prompt(&text),
                            None => text.clone(),
                        };
                        if state.in_demo {
                            let _ = state
                                .cmd_tx
                                .send(KarCommand::SendDemoPrompt {
                                    request_id: env.request_id,
                                    raw_user_text: text,
                                    target,
                                })
                                .await;
                        } else if state.turn_appears_busy() {
                            let _ = state
                                .cmd_tx
                                .send(KarCommand::SteerOrQueuePrompt {
                                    request_id: env.request_id,
                                    raw_user_text: text,
                                    payload,
                                    target,
                                    attachments: Vec::new(),
                                })
                                .await;
                        } else {
                            let _ = state
                                .cmd_tx
                                .send(KarCommand::SendPrompt {
                                    request_id: env.request_id,
                                    raw_user_text: text,
                                    payload,
                                    target,
                                    attachments: Vec::new(),
                                })
                                .await;
                        }
                    }
                    RemoteAction::SteerOrQueue { text, target } => {
                        let payload = match &target {
                            Some(tgt) => tgt.compose_prompt(&text),
                            None => text.clone(),
                        };
                        let _ = state
                            .cmd_tx
                            .send(KarCommand::SteerOrQueuePrompt {
                                request_id: env.request_id,
                                raw_user_text: text,
                                payload,
                                target,
                                attachments: Vec::new(),
                            })
                            .await;
                    }
                    RemoteAction::WithdrawQueued {
                        input_id,
                        chat_id: expected,
                    } => {
                        if let Some(chat_id) = state
                            .http_server
                            .as_ref()
                            .map(|srv| srv.chat_id)
                            .filter(|id| {
                                !id.is_nil() && expected.is_none_or(|expected| expected == *id)
                            })
                        {
                            let _ = state
                                .cmd_tx
                                .send(KarCommand::WithdrawQueued {
                                    chat_id,
                                    input_id,
                                    edit: false,
                                })
                                .await;
                        }
                    }
                    RemoteAction::AbortTurn => {
                        let _ = state.cmd_tx.send(KarCommand::AbortTurn).await;
                    }
                    RemoteAction::DecidePermission { pending_id, allow } => {
                        if let Some(ref session) = state.active_controller
                            && let Ok(h) = session.history()
                            && let Some(req) = h
                                .pending_requests
                                .into_iter()
                                .find(|r| r.id == pending_id)
                        {
                            let decision = if allow {
                                kar::PermissionDecision::AllowOnce
                            } else {
                                kar::PermissionDecision::Deny
                            };
                            let _ = state
                                .cmd_tx
                                .send(KarCommand::DecidePermission { request: req, decision })
                                .await;
                        }
                    }
                    RemoteAction::SubmitBriefFeedback {
                        brief_id,
                        revision,
                        formatted_reply,
                    } => {
                        state.emit(KarBridgeEvent::RemoteBriefFeedbackReceived {
                            brief_id,
                            revision,
                            summary_markdown: formatted_reply,
                        });
                    }
                    RemoteAction::Ping { .. } => {}
                }
            }
            KarCommand::CreateScheduledChat {
                request_id,
                task,
                workspace,
            } => {
                if state.in_demo {
                    let fake_chat = Uuid::new_v4();
                    state.emit(KarBridgeEvent::ScheduledChatCreated {
                        request_id,
                        task_id: task.id.clone(),
                        chat_id: fake_chat,
                    });
                    state.emit(KarBridgeEvent::ScheduleRunFinished { request_id, outcome: ScheduleRunOutcome::Completed });
                    let _ = state
                        .cmd_tx
                        .send(KarCommand::SendDemoPrompt {
                            request_id,
                            raw_user_text: format!("【定时任务：{}】{}", task.title, task.prompt),
                            target: None,
                        })
                        .await;
                    continue;
                }
                let Some(store) = state.store.clone() else {
                    state.emit(KarBridgeEvent::ScheduleRunFinished { request_id, outcome: ScheduleRunOutcome::Failed });
                    continue;
                };
                if !workspace.is_dir()
                    || task.office_template.as_ref().is_some_and(|p| !p.is_file())
                {
                    state.emit(KarBridgeEvent::ScheduleRunFinished {
                        request_id,
                        outcome: ScheduleRunOutcome::Failed,
                    });
                    state.emit(KarBridgeEvent::ToastMessage {
                        text: "无法运行定时任务：工作目录或绑定的文档已移走。".into(),
                        warning: true,
                    });
                    continue;
                }
                let mut opts = SessionOptions::new(&workspace);
                opts.approvals = ApprovalMode::Host;
                opts.model = task.model.clone().filter(|m| !m.trim().is_empty());
                opts.settings.effort = task.effort.clone();
                configure_runtime_path(&task.runtime_id, &mut opts);
                state.attach_office_mcp(&task.runtime_id, &mut opts);
                match store.create(&task.runtime_id, None, opts).await {
                    Ok(session) => {
                        let chat_id = session.info().id;
                        save_persisted_title(
                            &state.store_dir,
                            chat_id,
                            Some(&format!("⏱ {}", task.title)),
                        );
                        state.emit(KarBridgeEvent::ScheduledChatCreated {
                            request_id,
                            task_id: task.id.clone(),
                            chat_id,
                        });
                        let attachments: Vec<PathBuf> = task
                            .office_template
                            .iter()
                            .cloned()
                            .collect();
                        let session = Arc::new(session);
                        let evt_tx = state.evt_tx.clone();
                        let store_dir = state.store_dir.clone();
                        let store_clone = store.clone();
                        let prompt = task.prompt.clone();
                        tokio::spawn(async move {
                            let res = if attachments.is_empty() {
                                session.send(request_id, prompt).await
                            } else {
                                match persist_attachment_paths(&store_clone, chat_id, &attachments) {
                                    Ok(ids) => session.send_attached(request_id, prompt, ids).await,
                                    Err(e) => Err(e),
                                }
                            };
                            let outcome = match &res {
                                Ok(turn) if turn.status == TurnStatus::Completed => ScheduleRunOutcome::Completed,
                                _ => ScheduleRunOutcome::Failed,
                            };
                            let _ = evt_tx.send(KarBridgeEvent::ScheduleRunFinished { request_id, outcome }).await;
                            if let Err(err) = res {
                                let _ = evt_tx.send(KarBridgeEvent::ToastMessage {
                                    text: format!("定时任务投递失败：{err}"),
                                    warning: true,
                                }).await;
                            }
                            if let Ok(infos) = store_clone.list() {
                                let custom_titles = load_persisted_titles(&store_dir);
                                let entries = infos
                                    .into_iter()
                                    .map(|info| {
                                        let title_override =
                                            custom_titles.get(&info.id).map(String::as_str);
                                        let turns = read_chat_history_resilient(
                                            &store_clone,
                                            &store_dir,
                                            info.id,
                                        )
                                        .map(|h| h.turns)
                                        .unwrap_or_default();
                                        ChatListEntry::from_info_and_turns_with_title(
                                            info,
                                            &turns,
                                            title_override,
                                        )
                                    })
                                    .collect();
                                let _ = evt_tx.send(KarBridgeEvent::ChatsListed(entries)).await;
                            }
                            if let Ok(owned) = Arc::try_unwrap(session) {
                                let _ = owned.close().await;
                            }
                        });
                    }
                    Err(err) => {
                        state.emit(KarBridgeEvent::ScheduleRunFinished { request_id, outcome: ScheduleRunOutcome::Failed });
                        state.emit(KarBridgeEvent::ToastMessage {
                            text: format!("定时任务「{}」未能启动智能体：{err}", task.title),
                            warning: true,
                        });
                    }
                }
            }
            KarCommand::EnterDemoShowcase => {
                state.stop_feed();
                state.close_controller(false).await;
                state.selected_chat_id = None;
                state.in_demo = true;
                state.emit(KarBridgeEvent::AccessModeChanged(
                    ChatAccessMode::DemoShowcase,
                ));
                state.emit(KarBridgeEvent::ConnectionStateChanged(
                    FeedConnectionState::Idle,
                ));
                state.emit(KarBridgeEvent::ChatStateUpdated(ProjectedChatState {
                    turns: state.demo_turns.clone(),
                    is_demo: true,
                    ..ProjectedChatState::default()
                }));
            }
            KarCommand::SendDemoPrompt {
                request_id,
                raw_user_text,
                target,
            } => {
                if !state.in_demo {
                    continue;
                }
                if let Some(ref tgt) = target {
                    save_persisted_target(&state.store_dir, request_id, tgt.clone());
                    state.emit(KarBridgeEvent::TurnTargetsLoaded(load_persisted_targets(
                        &state.store_dir,
                    )));
                }
                let root_agent = Agent {
                    session_id: Some("demo-session-01".into()),
                    path: Vec::new(),
                };
                let tool_call_id = format!("demo_call_{}", &request_id.to_string()[..8]);
                let running_tool = Tool {
                    elided: false,
                    name: Some("office_get_selection".into()),
                    native_type: Some("demo".into()),
                    server: Some("demo".into()),
                    tool_name: Some("office_get_selection".into()),
                    mcp_app_uri: None,
                    finished: false,
                    input: Some(serde_json::json!({
                        "instruction": raw_user_text,
                        "target": target.as_ref().map(SelectionTarget::short_badge),
                    })),
                    preview: "正在读取选区…".into(),
                    input_partial: false,
                    result: None,
                };
                let turn = ChatTurn {
                    app_context: None,
                    merged_context: None,
                    requested_output_schema: None,
                    background_note: None,
                    id: request_id,
                    started_ms: 1_763_963_500_000,
                    finished_ms: None,
                    status: TurnStatus::Running,
                    native_input_receipt: None,
                    process_cleanups: Vec::new(),
                    native_result_id: None,
                    native_result_inputs: Vec::new(),
                    failure: None,
                    structured_output: None,
                    native_continuation: false,
                    steered_from: None,
                    native_child_origin: None,
                    child_sessions: BTreeMap::new(),
                    attachments: Vec::new(),
                    steered_attachments: BTreeMap::new(),
                    items: vec![
                        TimelineItem {
                            source: root_agent.clone(),
                            item: Item::Message {
                                role: Role::User,
                                text: raw_user_text.clone(),
                                provider_id: None,
                                complete: true,
                            },
                        },
                        TimelineItem {
                            source: root_agent,
                            item: Item::Tool {
                                id: tool_call_id.clone(),
                                activity: running_tool,
                            },
                        },
                    ],
                    steered_inputs: BTreeMap::new(),
                    fork_anchor: None,
                    native_turn_id: None,
                    pi_input_timestamp_ms: None,
                    pi_image_content_sha256: None,
                    input_sent: true,
                    checkpoint: Some(Checkpoint {
                        parent: None,
                        before_tree: "demo_before".into(),
                        after_tree: None,
                        files: Vec::new(),
                        repositories: Vec::new(),
                    }),
                };
                state.demo_turns.push(turn);
                state.emit(KarBridgeEvent::ChatStateUpdated(ProjectedChatState {
                    turns: state.demo_turns.clone(),
                    is_demo: true,
                    ..ProjectedChatState::default()
                }));

                // Complete asynchronously without blocking `worker_loop`.
                let cmd_tx = state.cmd_tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(320)).await;
                    let _ = cmd_tx
                        .send(KarCommand::FinishDemoPrompt {
                            request_id,
                            raw_user_text,
                            target,
                            tool_call_id,
                        })
                        .await;
                });
            }
            KarCommand::FinishDemoPrompt {
                request_id,
                raw_user_text,
                target,
                tool_call_id,
            } => {
                if !state.in_demo {
                    continue;
                }
                let root_agent = Agent {
                    session_id: Some("demo-session-01".into()),
                    path: Vec::new(),
                };
                if let Some(turn) = state.demo_turns.iter_mut().find(|t| t.id == request_id) {
                    turn.status = TurnStatus::Completed;
                    turn.finished_ms = Some(1_763_963_500_320);
                    let finished_tool = Tool {
                        elided: false,
                        name: Some("office_get_selection".into()),
                        native_type: Some("demo".into()),
                        server: Some("demo".into()),
                        tool_name: Some("office_get_selection".into()),
                        mcp_app_uri: None,
                        finished: true,
                        input: Some(serde_json::json!({
                            "instruction": raw_user_text,
                            "target": target.as_ref().map(SelectionTarget::short_badge),
                        })),
                        preview: "已读取选区".into(),
                        input_partial: false,
                        result: Some(kar::ToolResult {
                            status: Some(kar::ToolStatus::Succeeded),
                            exit_code: Some(0),
                            content: Some(vec![kar::OutputPart::Text {
                                text: "已读取选区内容".into(),
                            }]),
                            file_changes: Vec::new(),
                        }),
                    };
                    if turn.items.len() > 1 {
                        turn.items[1] = TimelineItem {
                            source: root_agent.clone(),
                            item: Item::Tool {
                                id: tool_call_id,
                                activity: finished_tool,
                            },
                        };
                    }
                    turn.items.push(TimelineItem {
                        source: root_agent,
                        item: Item::Message {
                            role: Role::Assistant,
                            text: match target.as_ref() {
                                Some(tgt) => format!(
                                    "这是示例回复：收到你对「{}」的要求——{}。\n\n要让智能体真正执行，请新建一个对话。",
                                    tgt.short_badge(),
                                    raw_user_text
                                ),
                                None => format!(
                                    "这是示例回复：收到你的要求——{raw_user_text}。\n\n要让智能体真正执行，请新建一个对话。"
                                ),
                            },
                            provider_id: None,
                            complete: true,
                        },
                    });
                    state.emit(KarBridgeEvent::ChatStateUpdated(ProjectedChatState {
                        turns: state.demo_turns.clone(),
                        is_demo: true,
                        ..ProjectedChatState::default()
                    }));
                }
            }
            KarCommand::RefreshGitWorkspace { workspace } => {
                let evt_tx = state.evt_tx.clone();
                let in_demo = state.in_demo;
                tokio::task::spawn_blocking(move || {
                    let mut snapshot = discover_git_workspace(&workspace);
                    if !snapshot.is_repo && in_demo {
                        snapshot = crate::studio_hub::demo_git_snapshot();
                    }
                    let tree = scan_workspace_tree(&workspace, 180);
                    let _ = evt_tx.send_blocking(KarBridgeEvent::GitWorkspaceUpdated {
                        workspace: workspace.clone(), snapshot,
                        tree,
                    });
                });
            }
            KarCommand::GitStagePaths { workspace, paths } => {
                let evt_tx = state.evt_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let res = kar::workbench_git::Repository::open(&workspace)
                        .and_then(|repo| repo.stage(&paths));
                    match res {
                        Ok(()) => {
                            emit_diag_log(
                                &evt_tx,
                                DiagLevel::Info,
                                "git",
                                Some("STAGE"),
                                format!("已暂存 {} 个工作区文件", paths.len()),
                            );
                            let snapshot = discover_git_workspace(&workspace);
                            let tree = scan_workspace_tree(&workspace, 180);
                            let _ = evt_tx.send_blocking(KarBridgeEvent::GitWorkspaceUpdated {
                                workspace: workspace.clone(), snapshot,
                                tree,
                            });
                        }
                        Err(err) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("Git 暂存失败：{err}"),
                                warning: true,
                            });
                        }
                    }
                });
            }
            KarCommand::GitUnstagePaths { workspace, paths } => {
                let evt_tx = state.evt_tx.clone();
                tokio::task::spawn_blocking(move || match unstage_git_paths(&workspace, &paths) {
                    Ok(()) => {
                        emit_diag_log(
                            &evt_tx,
                            DiagLevel::Info,
                            "git",
                            Some("UNSTAGE"),
                            format!("已取消暂存 {} 个文件", paths.len()),
                        );
                        let snapshot = discover_git_workspace(&workspace);
                        let tree = scan_workspace_tree(&workspace, 180);
                        let _ = evt_tx.send_blocking(KarBridgeEvent::GitWorkspaceUpdated {
                            workspace: workspace.clone(), snapshot,
                            tree,
                        });
                    }
                    Err(err) => {
                        let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                            text: format!("取消暂存失败：{err}"),
                            warning: true,
                        });
                    }
                });
            }
            KarCommand::GitSwitchBranch {
                workspace,
                branch,
                create,
            } => {
                let evt_tx = state.evt_tx.clone();
                tokio::task::spawn_blocking(move || {
                    match switch_or_create_git_branch(&workspace, &branch, create) {
                        Ok(target) => {
                            emit_diag_log(
                                &evt_tx,
                                DiagLevel::Info,
                                "git",
                                Some("BRANCH"),
                                format!(
                                    "{}分支 `{target}`",
                                    if create { "已新建并切换到" } else { "已切换到" }
                                ),
                            );
                            let snapshot = discover_git_workspace(&workspace);
                            let tree = scan_workspace_tree(&workspace, 180);
                            let _ = evt_tx.send_blocking(KarBridgeEvent::GitWorkspaceUpdated {
                                workspace: workspace.clone(), snapshot,
                                tree,
                            });
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("当前分支：{target}"),
                                warning: false,
                            });
                        }
                        Err(err) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("切换分支未完成：{err}"),
                                warning: true,
                            });
                        }
                    }
                });
            }
            KarCommand::LoadGitFileDiff {
                staged,
                workspace,
                rel_path,
                is_demo,
            } => {
                let evt_tx = state.evt_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let root = kar::workbench_git::Repository::open(&workspace)
                        .map(|repo| repo.root().to_path_buf())
                        .unwrap_or_else(|_| workspace.clone());
                    let side = if staged { GitDiffSide::Staged } else { GitDiffSide::WorkingTree };
                    let hunks = read_diff_side(&root, &rel_path, is_demo, side);
                    let _ = evt_tx.send_blocking(KarBridgeEvent::GitFileDiffLoaded {
                        staged,
                        workspace,
                        rel_path,
                        hunks,
                    });
                });
            }
            KarCommand::RefreshWorkspaceTree {
                workspace,
                show_hidden,
            } => {
                let evt_tx = state.evt_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let tree = scan_workspace_tree_with_options(&workspace, 220, show_hidden);
                    let _ = evt_tx.send_blocking(KarBridgeEvent::WorkspaceTreeUpdated { workspace, show_hidden, tree });
                });
            }
            KarCommand::RunWorkspaceCommand { id, cwd, command } => {
                let evt_tx = state.evt_tx.clone();
                let (kill_tx, mut kill_rx) = watch::channel(false);
                state.running_terminals.insert(id, kill_tx);
                emit_diag_log(
                    &evt_tx,
                    DiagLevel::Info,
                    "terminal",
                    Some("EXEC"),
                    format!("$ {command}  (cwd: {})", cwd.display()),
                );
                tokio::spawn(async move {
                    use tokio::io::{AsyncBufReadExt, BufReader};
                    let shell = if cfg!(unix) { "/bin/zsh" } else { "cmd" };
                    let shell_flag = if cfg!(unix) { "-lc" } else { "/C" };
                    let mut child_cmd = tokio::process::Command::new(shell);
                    child_cmd
                        .arg(shell_flag)
                        .arg(&command)
                        .current_dir(&cwd)
                        .env("PATH", augmented_path_os())
                        .env("GIT_TERMINAL_PROMPT", "0")
                        .env("TERM", "dumb")
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .kill_on_drop(true);

                    let mut child = match child_cmd.spawn() {
                        Ok(c) => c,
                        Err(err) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::TerminalOutputChunk {
                                id,
                                chunk: format!("启动命令失败：{err}\n"),
                            });
                            let _ = evt_tx.send_blocking(KarBridgeEvent::TerminalCommandFinished {
                                id,
                                exit_code: 127,
                            });
                            return;
                        }
                    };

                    let stdout = child.stdout.take();
                    let stderr = child.stderr.take();
                    let tx_out = evt_tx.clone();
                    let out_task = tokio::spawn(async move {
                        if let Some(out) = stdout {
                            let mut lines = BufReader::new(out).lines();
                            while let Ok(Some(line)) = lines.next_line().await {
                                let _ = tx_out
                                    .send(KarBridgeEvent::TerminalOutputChunk {
                                        id,
                                        chunk: format!("{line}\n"),
                                    })
                                    .await;
                            }
                        }
                    });
                    let tx_err = evt_tx.clone();
                    let err_task = tokio::spawn(async move {
                        if let Some(err) = stderr {
                            let mut lines = BufReader::new(err).lines();
                            while let Ok(Some(line)) = lines.next_line().await {
                                let _ = tx_err
                                    .send(KarBridgeEvent::TerminalOutputChunk {
                                        id,
                                        chunk: format!("{line}\n"),
                                    })
                                    .await;
                            }
                        }
                    });

                    let exit_code = tokio::select! {
                        _ = kill_rx.changed() => {
                            let _ = child.kill().await;
                            let _ = evt_tx.send(KarBridgeEvent::TerminalOutputChunk {
                                id,
                                chunk: "\n^C 已终止命令执行\n".into(),
                            }).await;
                            130
                        }
                        st = child.wait() => {
                            st.ok().and_then(|s| s.code()).unwrap_or(-1)
                        }
                    };
                    let _ = out_task.await;
                    let _ = err_task.await;
                    let _ = evt_tx
                        .send(KarBridgeEvent::TerminalCommandFinished { id, exit_code })
                        .await;
                });
            }
            KarCommand::KillWorkspaceCommand { id } => {
                if let Some(tx) = state.running_terminals.remove(&id) {
                    let _ = tx.send(true);
                }
            }
            KarCommand::GitStageAll { workspace } => {
                let evt_tx = state.evt_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let res = kar::workbench_git::Repository::open(&workspace).and_then(|repo| {
                        let st = repo.status()?;
                        let all_paths: Vec<PathBuf> = st.files.into_iter().map(|f| f.path).collect();
                        if !all_paths.is_empty() {
                            repo.stage(&all_paths)?;
                        }
                        Ok(all_paths.len())
                    });
                    match res {
                        Ok(n) => {
                            emit_diag_log(
                                &evt_tx,
                                DiagLevel::Info,
                                "git",
                                Some("STAGE"),
                                format!("已全部暂存 {n} 个变更文件"),
                            );
                            let snapshot = discover_git_workspace(&workspace);
                            let tree = scan_workspace_tree(&workspace, 180);
                            let _ = evt_tx.send_blocking(KarBridgeEvent::GitWorkspaceUpdated {
                                workspace: workspace.clone(), snapshot,
                                tree,
                            });
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("已暂存 {n} 个变更文件"),
                                warning: false,
                            });
                        }
                        Err(err) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("全部暂存失败：{err}"),
                                warning: true,
                            });
                        }
                    }
                });
            }
            KarCommand::GitCommit { workspace, message, stage_paths } => {
                let evt_tx = state.evt_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let res = kar::workbench_git::Repository::open(&workspace)
                        .and_then(|repo| {
                            if !stage_paths.is_empty() {
                                repo.stage(&stage_paths)?;
                            }
                            repo.commit(&message)
                        });
                    let _ = evt_tx.send_blocking(KarBridgeEvent::GitCommitFinished {
                        workspace: workspace.clone(), message: message.clone(), success: res.is_ok(),
                    });
                    match res {
                        Ok(oid) => {
                            let short = &oid[..oid.len().min(7)];
                            emit_diag_log(
                                &evt_tx,
                                DiagLevel::Info,
                                "git",
                                Some("COMMIT"),
                                format!("已创建提交 {short}: {message}"),
                            );
                            let snapshot = discover_git_workspace(&workspace);
                            let tree = scan_workspace_tree(&workspace, 180);
                            let _ = evt_tx.send_blocking(KarBridgeEvent::GitWorkspaceUpdated {
                                workspace: workspace.clone(), snapshot,
                                tree,
                            });
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("提交成功（{short}）"),
                                warning: false,
                            });
                        }
                        Err(err) => {
                            emit_diag_log(
                                &evt_tx,
                                DiagLevel::Warn,
                                "git",
                                Some("COMMIT"),
                                format!("Git 提交未完成：{err}"),
                            );
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("Git 提交失败：{err}"),
                                warning: true,
                            });
                        }
                    }
                });
            }
            KarCommand::GitPull { workspace } => {
                let evt_tx = state.evt_tx.clone();
                tokio::task::spawn_blocking(move || {
                    match kar::workbench_git::Repository::open(&workspace).and_then(|r| r.pull()) {
                        Ok(head) => {
                            let short = &head[..head.len().min(7)];
                            emit_diag_log(
                                &evt_tx,
                                DiagLevel::Info,
                                "git",
                                Some("PULL"),
                                format!("快进拉取完成，HEAD={short}"),
                            );
                            let snapshot = discover_git_workspace(&workspace);
                            let tree = scan_workspace_tree(&workspace, 180);
                            let _ = evt_tx.send_blocking(KarBridgeEvent::GitWorkspaceUpdated {
                                workspace: workspace.clone(), snapshot,
                                tree,
                            });
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("已同步上游分支（{short}）"),
                                warning: false,
                            });
                        }
                        Err(err) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("拉取未执行：{err}"),
                                warning: true,
                            });
                        }
                    }
                });
            }
            KarCommand::GitPush { workspace } => {
                let evt_tx = state.evt_tx.clone();
                tokio::task::spawn_blocking(move || {
                    match kar::workbench_git::Repository::open(&workspace).and_then(|r| r.push()) {
                        Ok(()) => {
                            emit_diag_log(
                                &evt_tx,
                                DiagLevel::Info,
                                "git",
                                Some("PUSH"),
                                "当前分支已推送至远端 origin",
                            );
                            let snapshot = discover_git_workspace(&workspace);
                            let tree = scan_workspace_tree(&workspace, 180);
                            let _ = evt_tx.send_blocking(KarBridgeEvent::GitWorkspaceUpdated {
                                workspace: workspace.clone(), snapshot,
                                tree,
                            });
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: "分支已推送到远端 origin".into(),
                                warning: false,
                            });
                        }
                        Err(err) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("推送未完成：{err}"),
                                warning: true,
                            });
                        }
                    }
                });
            }
            KarCommand::ReadWorkspaceFile {
                workspace,
                rel_path,
            } => {
                let evt_tx = state.evt_tx.clone();
                tokio::task::spawn_blocking(move || {
                    match read_workspace_file_preview(&workspace, &rel_path) {
                        Ok(preview) => {
                            let _ = evt_tx
                                .send_blocking(KarBridgeEvent::WorkspaceFileLoaded { workspace, preview });
                        }
                        Err(err) => {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ToastMessage {
                                text: format!("无法打开文件：{err}"),
                                warning: true,
                            });
                        }
                    }
                });
            }
            KarCommand::ProbeWorkspaceInventory {
                workspace,
                runtime_id,
            } => {
                let evt_tx = state.evt_tx.clone();
                let office_available = state.mcp_office.is_some() && state.mcp_supported_runtimes.get(&runtime_id)
                    .copied().unwrap_or_else(|| default_runtime_mcp_support(&runtime_id));
                tokio::spawn(async move {
                    let inv = probe_inventory_with_office(&workspace, &runtime_id, office_available).await;
                    emit_diag_log(
                        &evt_tx,
                        DiagLevel::Info,
                        "inventory",
                        Some("DISCOVER"),
                        format!(
                            "已同步 {} 能力清单：{} 项技能 · {} 个 MCP 服务 · {} 条指令",
                            runtime_display_name(&runtime_id),
                            inv.skills.len(),
                            inv.mcp_servers.len(),
                            inv.commands.len(),
                        ),
                    );
                    let _ = evt_tx.send(KarBridgeEvent::WorkspaceInventoryUpdated { workspace, inventory: inv }).await;
                });
            }
            KarCommand::GlobalSearch { workspace, query } => {
                let evt_tx = state.evt_tx.clone();
                let store_dir = state.store_dir.clone();
                tokio::task::spawn_blocking(move || {
                    let store = ChatStore::open(&store_dir).ok();
                    let hits = run_global_search(&workspace, store.as_ref(), &query, 16);
                    let _ = evt_tx.send_blocking(KarBridgeEvent::GlobalSearchResults {
                        workspace,
                        query,
                        hits,
                    });
                });
            }
            KarCommand::RefreshUsageAnalytics {
                providers,
            } => {
                let evt_tx = state.evt_tx.clone();
                let store_dir = state.store_dir.clone();
                tokio::task::spawn_blocking(move || {
                    let store = ChatStore::open(&store_dir).ok();
                    let analytics =
                        compute_usage_analytics(store.as_ref(), &[], &providers);
                    let _ = evt_tx
                        .send_blocking(KarBridgeEvent::UsageAnalyticsUpdated(analytics));
                });
            }
            KarCommand::HandoffChat {
                source_chat,
                target_runtime,
                target_model,
                user_note,
                budget_bytes,
                workspace,
            } => {
                let history;
                let (source_runtime, turns) = if state.in_demo && source_chat.is_none() {
                    ("codex".to_owned(), state.demo_turns.as_slice())
                } else if let Some(session) = &state.active_controller
                    && !state.in_demo
                    && Some(session.info().id) == source_chat
                    && state.selected_chat_id == source_chat
                    && let Ok(snapshot) = session.history()
                    && snapshot
                        .turns
                        .last()
                        .is_some_and(|turn| turn.status != TurnStatus::Running)
                {
                    history = snapshot;
                    (session.info().runtime.clone(), history.turns.as_slice())
                } else {
                    state.emit(KarBridgeEvent::ToastMessage {
                        text: "原对话已切换或正在执行任务，请重新打开交接。".into(),
                        warning: true,
                    });
                    continue;
                };
                let mut turn_pairs = Vec::new();
                for turn in turns {
                    let (mut user, mut assistant) = (String::new(), String::new());
                    for item in &turn.items {
                        if let Item::Message { role, text, .. } = &item.item {
                            match role {
                                Role::User if user.is_empty() => user = text.clone(),
                                Role::Assistant => assistant = text.clone(),
                                _ => {}
                            }
                        }
                    }
                    if !user.is_empty() || !assistant.is_empty() {
                        turn_pairs.push((user, assistant));
                    }
                }
                let packed = build_portable_handoff_context(
                    &source_runtime,
                    &target_runtime,
                    &turn_pairs,
                    &user_note,
                    budget_bytes,
                );

                if state.in_demo {
                    emit_diag_log(
                        &state.evt_tx,
                        DiagLevel::Info,
                        "handoff",
                        Some("RELAY"),
                        format!(
                            "已完成演示会话接力：{} -> {}",
                            runtime_display_name(&source_runtime),
                            runtime_display_name(&target_runtime)
                        ),
                    );
                    state.emit(KarBridgeEvent::HandoffCompleted {
                        chat_id: None,
                        target_runtime: target_runtime.clone(),
                        summary_message: packed.clone(),
                    });
                    let _ = state
                        .cmd_tx
                        .send(KarCommand::SendDemoPrompt {
                            request_id: Uuid::new_v4(),
                            raw_user_text: if user_note.trim().is_empty() {
                                format!(
                                    "已由 {} 接力切换至 {}，请确认上下文交接",
                                    runtime_display_name(&source_runtime),
                                    runtime_display_name(&target_runtime)
                                )
                            } else {
                                user_note
                            },
                            target: None,
                        })
                        .await;
                    continue;
                }

                let Some(store) = state.store.clone() else {
                    state.emit(KarBridgeEvent::ToastMessage {
                        text: "无法访问本地对话存储。".into(),
                        warning: true,
                    });
                    continue;
                };

                let mut opts = SessionOptions::new(&workspace);
                opts.approvals = ApprovalMode::Host;
                opts.model = target_model.filter(|m| !m.trim().is_empty());
                configure_runtime_path(&target_runtime, &mut opts);
                state.attach_office_mcp(&target_runtime, &mut opts);
                match store.create(&target_runtime, None, opts).await {
                    Ok(session) => {
                        state.close_controller(false).await;
                        state.stop_feed();
                        let new_id = session.info().id;
                        save_persisted_title(
                            &state.store_dir,
                            new_id,
                            Some(&format!(
                                "⇄ {} 接力对话",
                                runtime_display_name(&target_runtime)
                            )),
                        );
                        state.selected_chat_id = Some(new_id);
                        let caps = Some(session.capabilities());
                        state.active_controller = Some(Arc::new(session));
                        state.emit(KarBridgeEvent::AccessModeChanged(
                            ChatAccessMode::WriteController { chat_id: new_id },
                        ));
                        state.feed_cancel = Some(spawn_local_follower(
                            store.clone(),
                            state.store_dir.clone(),
                            new_id,
                            caps,
                            state.evt_tx.clone(),
                        ));
                        state.refresh_chat_list();
                        emit_diag_log(
                            &state.evt_tx,
                            DiagLevel::Info,
                            "handoff",
                            Some("RELAY"),
                            format!(
                                "已创建跨智能体接力会话 {new_id}（{} -> {}）",
                                runtime_display_name(&source_runtime),
                                runtime_display_name(&target_runtime)
                            ),
                        );
                        state.emit(KarBridgeEvent::HandoffCompleted {
                            chat_id: Some(new_id),
                            target_runtime: target_runtime.clone(),
                            summary_message: packed.clone(),
                        });
                        let title = if user_note.trim().is_empty() {
                            format!(
                                "接力自 {}，请继续当前工作",
                                runtime_display_name(&source_runtime)
                            )
                        } else {
                            user_note
                        };
                        state.send_prompt(Uuid::new_v4(), title, packed, None, Vec::new());
                    }
                    Err(err) => {
                        state.emit(KarBridgeEvent::ToastMessage {
                            text: format!(
                                "接力切换至 {} 失败：{err}",
                                runtime_display_name(&target_runtime)
                            ),
                            warning: true,
                        });
                    }
                }
            }
            KarCommand::AskSideQuestion {
                request_id,
                runtime_id,
                native_session_id,
                workspace,
                question,
                context_excerpt,
            } => {
                let evt_tx = state.evt_tx.clone();
                tokio::spawn(async move {
                    if runtime_id == "claude"
                        && let Some(ref sid) = native_session_id
                        && let Ok(rt) = kar::runtime("claude")
                    {
                        let mut sq_opts = kar::SideQuestionOptions::new(&workspace);
                        sq_opts.timeout = Duration::from_secs(30);
                        if let Ok(answer) = rt.side_question(sid, &question, &sq_opts).await {
                            emit_diag_log(
                                &evt_tx,
                                DiagLevel::Info,
                                "side_question",
                                Some("FORK"),
                                "通过 Claude 无工具临时分叉完成旁路快问（零主线程上下文污染）",
                            );
                            let _ = evt_tx
                                .send(KarBridgeEvent::SideQuestionAnswered {
                                    request_id,
                                    question,
                                    answer,
                                    via_native_fork: true,
                                })
                                .await;
                            return;
                        }
                    }

                    // Clean read-only local context answer when runtime does not expose native Claude side-fork
                    let fallback = if context_excerpt.trim().is_empty() {
                        format!(
                            "【旁路快问 · 零上下文污染模式】\n针对「{question}」：\n当前主对话尚未产生历史摘要（或当前智能体 `{}` 暂不支持原生 `--fork-session` 无工具旁路进程）。你可在不改动主对话记录的前提下在此核对上下文，或点击「转入主对话提问」直接发送。",
                            runtime_display_name(&runtime_id)
                        )
                    } else {
                        format!(
                            "【旁路快问 · 基于当前会话只读摘要】\n问题：{question}\n\n当前会话关键线索：\n{}\n\n（注：此旁路面板不会向主会话追加任何回合或消耗上下文窗口；原生无工具进程分叉在 Claude Code 2.1.326+ 会话下自动激活。）",
                            context_excerpt.trim()
                        )
                    };
                    let _ = evt_tx
                        .send(KarBridgeEvent::SideQuestionAnswered {
                            request_id,
                            question,
                            answer: fallback,
                            via_native_fork: false,
                        })
                        .await;
                });
            }
            KarCommand::Shutdown => {
                state.stop_feed();
                state.close_controller(true).await;
                if let Some(srv) = state.http_server.take() {
                    srv.stop().await;
                }
                if let Some(mcp) = state.mcp_office.take() {
                    mcp.close().await;
                }
                break;
            }
        }
    }
}

fn init_store_and_providers(
    state: &mut WorkerState,
    store_dir: PathBuf,
    workspace: PathBuf,
) {
    let _ = std::fs::create_dir_all(&store_dir);
    let _ = std::fs::create_dir_all(&workspace);
    state.store_dir = store_dir.clone();
    state.workspace = workspace.clone();

    match ChatStore::open(&store_dir) {
        Ok(store) => {
            state.store = Some(store);
            state.emit(KarBridgeEvent::StoreReady {
                store_dir: store_dir.clone(),
                workspace: workspace.clone(),
            });
            state.emit(KarBridgeEvent::TurnTargetsLoaded(load_persisted_targets(
                &store_dir,
            )));
            state.refresh_chat_list();
        }
        Err(err) => {
            state.emit(KarBridgeEvent::BannerNotice {
                title: "打开对话目录失败".into(),
                detail: format!("{}: {err}", store_dir.display()),
                is_error: true,
            });
        }
    }

    let fast_providers = fast_seed_providers_from_path();
    let any_fast_ready = fast_providers.iter().any(ProviderStatusInfo::is_ready);
    state.mcp_supported_runtimes = fast_providers
        .iter()
        .map(|p| (p.id.clone(), p.mcp_office_supported))
        .collect();
    state.goals_supported_runtimes = fast_providers
        .iter()
        .map(|p| (p.id.clone(), p.goals_supported))
        .collect();
    if any_fast_ready {
        state.emit(KarBridgeEvent::ProvidersProbing(false));
    }
    state.emit(KarBridgeEvent::ProvidersUpdated(fast_providers));

    // Phase-2 silent enrichment (`Depth::Models`, auth account & quota)
    state.spawn_provider_probe(!any_fast_ready);

    let cmd_tx = state.cmd_tx.clone();
    let ws = workspace;
    tokio::spawn(async move {
        let _ = cmd_tx
            .send(KarCommand::RefreshGitWorkspace {
                workspace: ws.clone(),
            })
            .await;
        let _ = cmd_tx
            .send(KarCommand::ProbeWorkspaceInventory {
                workspace: ws,
                runtime_id: "codex".into(),
            })
            .await;
    });
}

fn spawn_prompt_suggestion_watcher(
    hub: kar::PromptSuggestions,
    source_id: Uuid,
    chat_id: Uuid,
    evt_tx: async_channel::Sender<KarBridgeEvent>,
) -> tokio::task::JoinHandle<()> {
    let mut rx = hub.changes();
    tokio::spawn(async move {
        let mut last: Option<kar::PromptSuggestion> = None;
        loop {
            let cur = hub.current(source_id);
            if cur != last {
                last = cur.clone();
                if evt_tx
                    .send(KarBridgeEvent::PromptSuggestionUpdated {
                        chat_id,
                        suggestion: cur,
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            if rx.changed().await.is_err() {
                break;
            }
        }
    })
}

fn emit_diag_log(
    evt_tx: &async_channel::Sender<KarBridgeEvent>,
    level: DiagLevel,
    source: &str,
    tag: Option<&str>,
    message: impl Into<String>,
) {
    let rec = DiagLogRecord {
        id: format!("log-{}", Uuid::new_v4()),
        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        level,
        source: source.to_owned(),
        tag: tag.map(str::to_owned),
        message: message.into(),
        fields: Vec::new(),
    };
    let _ = evt_tx.try_send(KarBridgeEvent::DiagLogAppended(rec));
}
