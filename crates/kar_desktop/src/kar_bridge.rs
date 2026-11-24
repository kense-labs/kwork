//! Off-render-thread Kar SDK runtime bridge (`ChatStore`, `ChatSession`, `HttpChatFeed`, `ChatReplica`).
//!
//! Every disk, subprocess, and HTTP/SSE operation runs on a dedicated Tokio runtime thread,
//! communicating with GPUI strictly over `async_channel`.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use kar::{
    ApprovalMode, Error as KarError, InstallationOptions, InstallationSnapshot, PendingRequest,
    SessionOptions,
    account::{AuthStatus, AuthStatusOptions},
    chat::{
        ChatCursor, ChatEvent, ChatHistory, ChatInfo, ChatReplica, ChatSession, ChatStore,
        ChatTurn, ChangeKind, Checkpoint, CheckpointPoint, FileChange as CheckpointFileChange,
        HttpChatFeed, Item, Role, TimelineItem, TurnStatus, event_router,
    },
    observe::{Agent, Tool},
};
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::types::{
    ChatAccessMode, ChatListEntry, FeedConnectionState, PendingPromptAttempt, ProviderAuthState,
    ProviderAvailability, ProviderStatusInfo, SelectionTarget,
};

#[derive(Debug)]
pub enum KarCommand {
    RefreshStoreAndProviders {
        store_dir: PathBuf,
        workspace: PathBuf,
    },
    ListChats,
    CreateChat {
        runtime_id: String,
        program: Option<PathBuf>,
        workspace: PathBuf,
        approval_mode: ApprovalMode,
    },
    SelectLocalChat {
        chat_id: Uuid,
        prefer_read_only: bool,
    },
    SendPrompt {
        request_id: Uuid,
        raw_user_text: String,
        payload: String,
        target: Option<SelectionTarget>,
    },
    AbortTurn,
    DecidePermission {
        request: PendingRequest,
        decision: kar::PermissionDecision,
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
    EnterDemoShowcase,
    SendDemoPrompt {
        request_id: Uuid,
        raw_user_text: String,
        target: Option<SelectionTarget>,
    },
    Shutdown,
}

#[derive(Clone, Debug)]
pub struct ProjectedChatState {
    pub info: Option<ChatInfo>,
    pub turns: Vec<ChatTurn>,
    pub pending_requests: Vec<PendingRequest>,
    pub cursor: Option<ChatCursor>,
    pub is_demo: bool,
}

#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum KarBridgeEvent {
    StoreReady {
        store_dir: PathBuf,
        workspace: PathBuf,
    },
    ProvidersUpdated(Vec<ProviderStatusInfo>),
    ChatsListed(Vec<ChatListEntry>),
    TurnTargetsLoaded(BTreeMap<Uuid, SelectionTarget>),
    AccessModeChanged(ChatAccessMode),
    ConnectionStateChanged(FeedConnectionState),
    ChatStateUpdated(ProjectedChatState),
    PromptLifecycle {
        attempt: PendingPromptAttempt,
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

struct ActiveHttpServer {
    url: String,
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

struct WorkerState {
    store_dir: PathBuf,
    workspace: PathBuf,
    store: Option<ChatStore>,
    active_controller: Option<Arc<ChatSession>>,
    feed_cancel: Option<watch::Sender<bool>>,
    http_server: Option<ActiveHttpServer>,
    demo_turns: Vec<ChatTurn>,
    in_demo: bool,
    evt_tx: async_channel::Sender<KarBridgeEvent>,
}

impl WorkerState {
    fn emit(&self, ev: KarBridgeEvent) {
        let _ = self.evt_tx.send_blocking(ev);
    }

    fn stop_feed(&mut self) {
        if let Some(cancel) = self.feed_cancel.take() {
            let _ = cancel.send(true);
        }
    }

    async fn close_controller(&mut self) {
        if let Some(arc_session) = self.active_controller.take()
            && let Ok(owned) = Arc::try_unwrap(arc_session)
        {
            let _ = owned.close().await;
        }
    }

    fn refresh_chat_list(&self) {
        let Some(store) = self.store.clone() else {
            return;
        };
        match store.list() {
            Ok(infos) => {
                let entries = infos
                    .into_iter()
                    .map(|info| {
                        let turns = store.history(info.id).map(|h| h.turns).unwrap_or_default();
                        ChatListEntry::from_info_and_turns(info, &turns)
                    })
                    .collect();
                self.emit(KarBridgeEvent::ChatsListed(entries));
            }
            Err(err) => {
                self.emit(KarBridgeEvent::BannerNotice {
                    title: "读取聊天列表失败".into(),
                    detail: err.to_string(),
                    is_error: true,
                });
            }
        }
    }
}

async fn probe_all_providers(workspace: PathBuf) -> Vec<ProviderStatusInfo> {
    let prioritized = ["codex", "claude", "grok", "opencode", "kimi", "pi", "qwen", "cursor"];
    let mut catalog = kar::runtimes();
    catalog.sort_by_key(|rt| {
        prioritized
            .iter()
            .position(|&p| p == rt.id)
            .unwrap_or(usize::MAX)
    });

    let install_opts = InstallationOptions {
        cwd: Some(workspace.clone()),
        ..Default::default()
    };

    let mut out = Vec::with_capacity(catalog.len());
    for rt in catalog {
        let id = rt.id.clone();
        let protocol = format!("{:?}", rt.protocol).to_ascii_lowercase();
        let snap = rt.probe_installation(&install_opts).await;
        match snap {
            Ok(InstallationSnapshot::Available {
                command, version, ..
            }) => {
                let auth_opts = AuthStatusOptions {
                    installation: install_opts.clone(),
                    timeout: Some(Duration::from_secs(5)),
                };
                let auth = match rt.auth_status(&auth_opts).await {
                    Ok(AuthStatus::LoggedIn { source, account }) => {
                        let method = account
                            .as_ref()
                            .and_then(|a| a.method.clone().or_else(|| a.email.clone()))
                            .unwrap_or(source);
                        ProviderAuthState::LoggedIn { summary: method }
                    }
                    Ok(AuthStatus::LoggedOut { source, detail, .. }) => {
                        let src = match detail {
                            Some(d) if !d.is_empty() => format!("{source} ({d})"),
                            _ => source,
                        };
                        ProviderAuthState::LoggedOut { source: src }
                    }
                    Ok(AuthStatus::Unknown { detail, .. }) => {
                        ProviderAuthState::Unknown { note: detail }
                    }
                    Err(err) => ProviderAuthState::Unknown {
                        note: err.to_string(),
                    },
                };
                out.push(ProviderStatusInfo {
                    id,
                    protocol,
                    availability: ProviderAvailability::Available {
                        version: version.unwrap_or_else(|| "installed".into()),
                        command,
                    },
                    auth,
                });
            }
            Ok(InstallationSnapshot::Missing) => {
                out.push(ProviderStatusInfo {
                    id,
                    protocol,
                    availability: ProviderAvailability::Missing,
                    auth: ProviderAuthState::NotChecked,
                });
            }
            Ok(InstallationSnapshot::Failed { reason, .. }) => {
                out.push(ProviderStatusInfo {
                    id,
                    protocol,
                    availability: ProviderAvailability::Failed { reason },
                    auth: ProviderAuthState::NotChecked,
                });
            }
            Err(err) => {
                out.push(ProviderStatusInfo {
                    id,
                    protocol,
                    availability: ProviderAvailability::Failed {
                        reason: err.to_string(),
                    },
                    auth: ProviderAuthState::NotChecked,
                });
            }
        }
    }
    out
}

fn state_from_replica(info: Option<ChatInfo>, replica: &ChatReplica) -> ProjectedChatState {
    ProjectedChatState {
        info,
        turns: replica.turns().to_vec(),
        pending_requests: replica.pending_requests().to_vec(),
        cursor: Some(replica.cursor()),
        is_demo: false,
    }
}

fn state_from_history(history: ChatHistory) -> ProjectedChatState {
    ProjectedChatState {
        info: Some(history.chat),
        turns: history.turns,
        pending_requests: history.pending_requests,
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
    chat_id: Uuid,
    evt_tx: async_channel::Sender<KarBridgeEvent>,
) -> watch::Sender<bool> {
    let (cancel_tx, mut cancel_rx) = watch::channel(false);
    tokio::spawn(async move {
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

        let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(state_from_replica(
            info.clone(),
            &replica,
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
                                    let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                        state_from_replica(info.clone(), &replica),
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
                                        let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                            state_from_replica(info.clone(), &replica),
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
                run_http_subscriber_loop(url, evt_tx, &mut cancel_rx).await;
            }));
        });
    cancel_tx
}

async fn run_http_subscriber_loop(
    url: String,
    evt_tx: async_channel::Sender<KarBridgeEvent>,
    cancel_rx: &mut watch::Receiver<bool>,
) {
        let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
            FeedConnectionState::Connecting {
                endpoint: url.clone(),
            },
        ));

        let mut feed = match HttpChatFeed::new(&url) {
            Ok(f) => f,
            Err(err) => {
                let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                    FeedConnectionState::Disconnected {
                        last_cursor: None,
                        reason: format!("无效的 HTTP 端点 `{url}`: {err}"),
                    },
                ));
                return;
            }
        };

        // Phase 1: Acquire initial `GET /snapshot` with retry backoff while offline.
        let mut attempt = 0u32;
        let initial_snapshot = loop {
            if *cancel_rx.borrow() {
                return;
            }
            match feed.snapshot().await {
                Ok(snap) => break snap,
                Err(err) => {
                    attempt += 1;
                    let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                        FeedConnectionState::Reconnecting {
                            last_cursor: None,
                            reason: format!("GET /snapshot 连接失败: {err}"),
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

        let chat_id = initial_snapshot.history.chat.id;
        let mut info = Some(initial_snapshot.history.chat.clone());
        let mut replica = ChatReplica::from_snapshot(initial_snapshot);
        let mut duplicates_ignored = 0u64;
        let mut gaps_recovered = 0u64;
        let mut offline_attempt = 0u32;
        let mut is_currently_offline = false;

        let _ = evt_tx.send_blocking(KarBridgeEvent::AccessModeChanged(
            ChatAccessMode::HttpReadOnly {
                url: url.clone(),
                chat_id: Some(chat_id),
            },
        ));
        let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(state_from_replica(
            info.clone(),
            &replica,
        )));
        let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
            FeedConnectionState::Connected {
                cursor: Some(replica.cursor().to_string()),
                duplicates_ignored,
                gaps_recovered,
            },
        ));

        // Phase 2: Stream `GET /events` via `HttpChatFeed::recv(committed_cursor)` while
        // monitoring TCP liveness so mid-turn disconnects surface immediately in the UI.
        let mut health_tick = tokio::time::interval(Duration::from_millis(350));
        health_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

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
                        }
                        Err(reason) => {
                            is_currently_offline = true;
                            offline_attempt += 1;
                            let _ = evt_tx.send_blocking(KarBridgeEvent::ConnectionStateChanged(
                                FeedConnectionState::Reconnecting {
                                    last_cursor: Some(replica.cursor().to_string()),
                                    reason: format!("SSE 端点连接中断 ({reason})，正从游标 {} 自动续收", replica.cursor()),
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
                                    state_from_replica(info.clone(), &replica),
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
                                    reason: format!("事件流异常: {err}，正从游标 {} 重试", replica.cursor()),
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
    let mut read_tool = Tool {
        elided: false,
        name: Some("docs_get_paragraphs".into()),
        native_type: Some("mcp".into()),
        server: Some("kantor".into()),
        tool_name: Some("docs_get_paragraphs".into()),
        finished: true,
        input: Some(serde_json::json!({"limit": 10})),
        preview: String::new(),
        input_partial: false,
        result: Some(kar::ToolResult {
            status: Some(kar::ToolStatus::Succeeded),
            exit_code: Some(0),
            content: Some(vec![kar::OutputPart::Text {
                text: "{\"total\": 4, \"paragraphs\": [{\"index\": 0, \"text\": \"Meeting minutes — 10 Nov 2026\"}]}".into(),
            }]),
            file_changes: vec![kar::FileChange {
                path: "docs/minutes_copy.docx".into(),
                kind: kar::ChangeKind::Modified,
                move_to: None,
            }],
        }),
    };
    read_tool.finished = true;

    vec![ChatTurn {
        id: turn_id,
        started_ms: 1_763_963_000_000,
        status: TurnStatus::Completed,
        native_input_receipt: None,
        process_cleanups: Vec::new(),
        native_result_id: None,
        native_result_inputs: Vec::new(),
        failure: None,
        structured_output: None,
        native_continuation: false,
        steered_from: None,
        child_sessions: BTreeMap::new(),
        items: vec![
            TimelineItem {
                source: root_agent.clone(),
                item: Item::Message {
                    role: Role::User,
                    text: "【演示样例】帮我检查会议纪要副本并汇总行动项".into(),
                    provider_id: None,
                    complete: true,
                },
            },
            TimelineItem {
                source: root_agent.clone(),
                item: Item::Message {
                    role: Role::Reasoning,
                    text: "正在读取打开的会议纪要段落结构并核对负责人与截止日期…".into(),
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
                    text: "这是独立【演示模式】样例时间线（不写入真实 ChatStore，也不会覆盖任何用户文档）。\n\n- 会议主题：供应商合同续签一年\n- 行动项：Budi 于周五前发出签署版草案\n- 您可在右侧切换至「双写演示」体验 Word / Excel / PPT 三种人机并发冲突与合并交互。".into(),
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
            files: vec![CheckpointFileChange {
                path: PathBuf::from("docs/minutes_copy.docx"),
                kind: ChangeKind::Modified,
            }],
        }),
    }]
}

async fn worker_loop(
    initial_store_dir: PathBuf,
    initial_workspace: PathBuf,
    cmd_rx: async_channel::Receiver<KarCommand>,
    evt_tx: async_channel::Sender<KarBridgeEvent>,
) {
    let mut state = WorkerState {
        store_dir: initial_store_dir.clone(),
        workspace: initial_workspace.clone(),
        store: None,
        active_controller: None,
        feed_cancel: None,
        http_server: None,
        demo_turns: build_demo_seed_turns(),
        in_demo: false,
        evt_tx: evt_tx.clone(),
    };

    // Bootstrap store + providers immediately on worker startup.
    init_store_and_providers(&mut state, initial_store_dir, initial_workspace).await;

    while let Ok(cmd) = cmd_rx.recv().await {
        match cmd {
            KarCommand::RefreshStoreAndProviders {
                store_dir,
                workspace,
            } => {
                init_store_and_providers(&mut state, store_dir, workspace).await;
            }
            KarCommand::ListChats => {
                state.refresh_chat_list();
            }
            KarCommand::CreateChat {
                runtime_id,
                program,
                workspace,
                approval_mode,
            } => {
                let Some(store) = state.store.clone() else {
                    state.emit(KarBridgeEvent::BannerNotice {
                        title: "ChatStore 未就绪".into(),
                        detail: "请检查隔离 KAR_CHAT_DIR 目录权限".into(),
                        is_error: true,
                    });
                    continue;
                };
                state.in_demo = false;
                state.stop_feed();
                state.close_controller().await;

                let _ = std::fs::create_dir_all(&workspace);
                let mut opts = SessionOptions::new(&workspace);
                opts.approvals = approval_mode;
                match store
                    .create(&runtime_id, program.as_deref(), opts)
                    .await
                {
                    Ok(session) => {
                        let chat_id = session.info().id;
                        let history = session.history().ok();
                        let session = Arc::new(session);
                        state.active_controller = Some(session);
                        state.emit(KarBridgeEvent::AccessModeChanged(
                            ChatAccessMode::WriteController { chat_id },
                        ));
                        if let Some(h) = history {
                            state.emit(KarBridgeEvent::ChatStateUpdated(state_from_history(h)));
                        }
                        state.feed_cancel = Some(spawn_local_follower(
                            store,
                            chat_id,
                            state.evt_tx.clone(),
                        ));
                        state.refresh_chat_list();
                    }
                    Err(err) => {
                        state.emit(KarBridgeEvent::BannerNotice {
                            title: format!("无法创建 `{runtime_id}` 聊天会话"),
                            detail: format!("{err}"),
                            is_error: true,
                        });
                    }
                }
            }
            KarCommand::SelectLocalChat {
                chat_id,
                prefer_read_only,
            } => {
                let Some(store) = state.store.clone() else {
                    continue;
                };
                state.in_demo = false;
                state.stop_feed();
                state.close_controller().await;

                if prefer_read_only {
                    state.emit(KarBridgeEvent::AccessModeChanged(
                        ChatAccessMode::LocalReadOnlySubscriber {
                            chat_id,
                            reason: "手动选择了本地只读观察模式".into(),
                        },
                    ));
                    state.feed_cancel = Some(spawn_local_follower(
                        store,
                        chat_id,
                        state.evt_tx.clone(),
                    ));
                    continue;
                }

                match store.resume(chat_id).await {
                    Ok(session) => {
                        let session = Arc::new(session);
                        state.active_controller = Some(session);
                        state.emit(KarBridgeEvent::AccessModeChanged(
                            ChatAccessMode::WriteController { chat_id },
                        ));
                        state.feed_cancel = Some(spawn_local_follower(
                            store,
                            chat_id,
                            state.evt_tx.clone(),
                        ));
                        state.refresh_chat_list();
                    }
                    Err(err) => {
                        let err_text = err.to_string();
                        let reason = if err_text.contains("another controller") {
                            "该聊天已在另一窗口持有写入控制器，本窗口自动以只读订阅模式跟随".to_owned()
                        } else {
                            format!("未能获取写入控制器（{err_text}），当前以只读历史/订阅模式查看")
                        };
                        state.emit(KarBridgeEvent::AccessModeChanged(
                            ChatAccessMode::LocalReadOnlySubscriber { chat_id, reason },
                        ));
                        state.feed_cancel = Some(spawn_local_follower(
                            store,
                            chat_id,
                            state.evt_tx.clone(),
                        ));
                    }
                }
            }
            KarCommand::SendPrompt {
                request_id,
                raw_user_text,
                payload,
                target,
            } => {
                if let Some(ref tgt) = target {
                    save_persisted_target(&state.store_dir, request_id, tgt.clone());
                    state.emit(KarBridgeEvent::TurnTargetsLoaded(load_persisted_targets(
                        &state.store_dir,
                    )));
                }
                let Some(session) = state.active_controller.clone() else {
                    state.emit(KarBridgeEvent::BannerNotice {
                        title: "当前会话不可写入".into(),
                        detail: "当前处于只读订阅或未打开控制器，请先新建聊天或切换到持有写入控制器的聊天。".into(),
                        is_error: true,
                    });
                    continue;
                };

                let mut attempt = PendingPromptAttempt {
                    request_id,
                    raw_user_text,
                    sent_payload: payload.clone(),
                    target,
                    attempts: 1,
                    in_flight: true,
                    last_error: None,
                };
                state.emit(KarBridgeEvent::PromptLifecycle {
                    attempt: attempt.clone(),
                });

                let evt_tx = state.evt_tx.clone();
                let store_opt = state.store.clone();
                tokio::spawn(async move {
                    let send_res = session.send(request_id, payload).await;
                    attempt.in_flight = false;
                    match send_res {
                        Ok(_turn) => {
                            attempt.last_error = None;
                            let _ = evt_tx.send_blocking(KarBridgeEvent::PromptLifecycle { attempt });
                            if let Ok(h) = session.history() {
                                let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                    state_from_history(h),
                                ));
                            }
                        }
                        Err(err) => {
                            attempt.last_error = Some(err.to_string());
                            let _ = evt_tx.send_blocking(KarBridgeEvent::PromptLifecycle {
                                attempt: attempt.clone(),
                            });
                            let _ = evt_tx.send_blocking(KarBridgeEvent::BannerNotice {
                                title: format!("回合发送未完成 (UUID {})", &request_id.to_string()[..8]),
                                detail: format!(
                                    "{err} — 点击「重试本回合」将复用同一请求 UUID ({request_id})"
                                ),
                                is_error: true,
                            });
                            if let Ok(h) = session.history() {
                                let _ = evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(
                                    state_from_history(h),
                                ));
                            }
                        }
                    }
                    if let Some(store) = store_opt
                        && let Ok(infos) = store.list()
                    {
                        let entries = infos
                            .into_iter()
                            .map(|info| {
                                let turns =
                                    store.history(info.id).map(|h| h.turns).unwrap_or_default();
                                ChatListEntry::from_info_and_turns(info, &turns)
                            })
                            .collect();
                        let _ = evt_tx.send_blocking(KarBridgeEvent::ChatsListed(entries));
                    }
                });
            }
            KarCommand::AbortTurn => {
                if let Some(session) = state.active_controller.clone() {
                    let evt_tx = state.evt_tx.clone();
                    tokio::spawn(async move {
                        if let Err(err) = session.abort().await {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::BannerNotice {
                                title: "中止回合失败".into(),
                                detail: err.to_string(),
                                is_error: true,
                            });
                        }
                    });
                }
            }
            KarCommand::DecidePermission { request, decision } => {
                if let Some(session) = state.active_controller.clone() {
                    let evt_tx = state.evt_tx.clone();
                    tokio::spawn(async move {
                        if let Err(err) = session.decide(&request, decision).await {
                            let _ = evt_tx.send_blocking(KarBridgeEvent::BannerNotice {
                                title: "提交权限响应失败".into(),
                                detail: err.to_string(),
                                is_error: true,
                            });
                        } else if let Ok(h) = session.history() {
                            let _ =
                                evt_tx.send_blocking(KarBridgeEvent::ChatStateUpdated(state_from_history(h)));
                        }
                    });
                }
            }
            KarCommand::ConnectHttpFeed { url } => {
                state.in_demo = false;
                state.stop_feed();
                state.close_controller().await;
                state.emit(KarBridgeEvent::AccessModeChanged(
                    ChatAccessMode::HttpReadOnly {
                        url: url.clone(),
                        chat_id: None,
                    },
                ));
                state.feed_cancel = Some(spawn_http_subscriber(url, state.evt_tx.clone()));
            }
            KarCommand::DisconnectFeed => {
                state.stop_feed();
                state.emit(KarBridgeEvent::ConnectionStateChanged(
                    FeedConnectionState::Disconnected {
                        last_cursor: None,
                        reason: "已手动断开订阅连接".into(),
                    },
                ));
            }
            KarCommand::StartHttpServer {
                chat_id,
                listen_addr,
            } => {
                let Some(store) = state.store.clone() else {
                    continue;
                };
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
                                    message: Some(format!("获取监听地址失败: {err}")),
                                });
                                continue;
                            }
                        };
                        let url = format!("http://{local_addr}");
                        let router = event_router(store, chat_id);
                        let (stop_tx, stop_rx) = oneshot::channel::<()>();
                        let join = tokio::spawn(async move {
                            let _ = axum::serve(listener, router)
                                .with_graceful_shutdown(async move {
                                    let _ = stop_rx.await;
                                })
                                .await;
                        });
                        state.http_server = Some(ActiveHttpServer {
                            url: url.clone(),
                            chat_id,
                            stop_tx,
                            join,
                        });
                        state.emit(KarBridgeEvent::HttpServerState {
                            listening_url: Some(url.clone()),
                            chat_id: Some(chat_id),
                            message: Some(format!(
                                "本聊天只读 HTTP/SSE 服务已启动：{url}（提供 GET /snapshot 与 GET /events）"
                            )),
                        });
                    }
                    Err(err) => {
                        state.emit(KarBridgeEvent::HttpServerState {
                            listening_url: None,
                            chat_id: None,
                            message: Some(format!("绑定 {listen_addr} 失败: {err}")),
                        });
                    }
                }
            }
            KarCommand::StopHttpServer => {
                if let Some(srv) = state.http_server.take() {
                    let old_url = srv.url.clone();
                    let chat_id = srv.chat_id;
                    srv.stop().await;
                    state.emit(KarBridgeEvent::HttpServerState {
                        listening_url: None,
                        chat_id: Some(chat_id),
                        message: Some(format!("已停止本聊天 HTTP/SSE 服务 ({old_url})")),
                    });
                }
            }
            KarCommand::EnterDemoShowcase => {
                state.stop_feed();
                state.close_controller().await;
                state.in_demo = true;
                state.emit(KarBridgeEvent::AccessModeChanged(
                    ChatAccessMode::DemoShowcase,
                ));
                state.emit(KarBridgeEvent::ConnectionStateChanged(
                    FeedConnectionState::Idle,
                ));
                state.emit(KarBridgeEvent::ChatStateUpdated(ProjectedChatState {
                    info: None,
                    turns: state.demo_turns.clone(),
                    pending_requests: Vec::new(),
                    cursor: None,
                    is_demo: true,
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
                    name: Some("inspect_office_selection".into()),
                    native_type: Some("demo".into()),
                    server: Some("demo".into()),
                    tool_name: Some("inspect_office_selection".into()),
                    finished: false,
                    input: Some(serde_json::json!({
                        "instruction": raw_user_text,
                        "target": target.as_ref().map(SelectionTarget::short_badge),
                    })),
                    preview: "正在读取选区上下文…".into(),
                    input_partial: false,
                    result: None,
                };
                let mut turn = ChatTurn {
                    id: request_id,
                    started_ms: 1_763_963_500_000,
                    status: TurnStatus::Running,
                    native_input_receipt: None,
                    process_cleanups: Vec::new(),
                    native_result_id: None,
                    native_result_inputs: Vec::new(),
                    failure: None,
                    structured_output: None,
                    native_continuation: false,
                    steered_from: None,
                    child_sessions: BTreeMap::new(),
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
                            source: root_agent.clone(),
                            item: Item::Tool {
                                id: tool_call_id.clone(),
                                activity: running_tool.clone(),
                            },
                        },
                    ],
                    steered_inputs: BTreeMap::new(),
                    fork_anchor: None,
                    native_turn_id: None,
                    pi_input_timestamp_ms: None,
                    input_sent: true,
                    checkpoint: Some(CheckpointPoint {
                        turn: request_id,
                        before: true,
                    })
                    .map(|_| Checkpoint {
                        parent: None,
                        before_tree: "demo_before".into(),
                        after_tree: None,
                        files: Vec::new(),
                    }),
                };
                state.demo_turns.push(turn.clone());
                state.emit(KarBridgeEvent::ChatStateUpdated(ProjectedChatState {
                    info: None,
                    turns: state.demo_turns.clone(),
                    pending_requests: Vec::new(),
                    cursor: None,
                    is_demo: true,
                }));

                tokio::time::sleep(Duration::from_millis(320)).await;
                let mut finished_tool = running_tool;
                finished_tool.finished = true;
                finished_tool.result = Some(kar::ToolResult {
                    status: Some(kar::ToolStatus::Succeeded),
                    exit_code: Some(0),
                    content: Some(vec![kar::OutputPart::Text {
                        text: "演示工具调用已完成（同一调用 ID 原地更新工具卡片）".into(),
                    }]),
                    file_changes: Vec::new(),
                });
                turn.status = TurnStatus::Completed;
                turn.items[1] = TimelineItem {
                    source: root_agent.clone(),
                    item: Item::Tool {
                        id: tool_call_id,
                        activity: finished_tool,
                    },
                };
                turn.items.push(TimelineItem {
                    source: root_agent,
                    item: Item::Message {
                        role: Role::Assistant,
                        text: format!(
                            "【演示数据回复】已收到针对 {} 的指令：「{}」。正式执行与文件持久化请切换至真实 Kar 会话。",
                            target
                                .as_ref()
                                .map_or_else(|| "当前对话".to_owned(), SelectionTarget::short_badge),
                            raw_user_text
                        ),
                        provider_id: None,
                        complete: true,
                    },
                });
                if let Some(last) = state.demo_turns.last_mut()
                    && last.id == request_id
                {
                    *last = turn;
                }
                state.emit(KarBridgeEvent::ChatStateUpdated(ProjectedChatState {
                    info: None,
                    turns: state.demo_turns.clone(),
                    pending_requests: Vec::new(),
                    cursor: None,
                    is_demo: true,
                }));
            }
            KarCommand::Shutdown => {
                state.stop_feed();
                if let Some(srv) = state.http_server.take() {
                    srv.stop().await;
                }
                state.close_controller().await;
                break;
            }
        }
    }
}

async fn init_store_and_providers(
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
                title: "打开隔离 ChatStore 失败".into(),
                detail: format!("{}: {err}", store_dir.display()),
                is_error: true,
            });
        }
    }

    let providers = probe_all_providers(workspace).await;
    state.emit(KarBridgeEvent::ProvidersUpdated(providers));
}
