//! Unified single-window GPUI Workbench: KUI Sidebar + Kar Chat Timeline + Kantor Office + Co-Edit.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gpui::{prelude::*, *};
use kar::{
    ApprovalMode, ChangeKind as ToolChangeKind,
    chat::{ChangeKind as CheckpointChangeKind, ChatTurn, Item, Role, TurnStatus},
    observe::Tool,
};
use kui::{
    ThemeMode,
    components::{
        alert::{Alert, AlertAction, Severity},
        badge::{Badge, BadgeTone},
        button::Button,
        dialog::Dialog,
        dropdown_menu::DropdownMenu,
        input::Input,
        resizable_panels::{Pane, ResizablePanels},
        textarea::Textarea,
        tool_call::{Invocation, InvocationInspector, ToolState},
    },
    primitives::{
        Size,
        disclosure::SurfaceEvent,
        field::FieldEvent,
        menu::{MenuItem, MenuSelection},
    },
};
use uuid::Uuid;

use crate::{
    apply_unified_theme,
    coedit::{
        CoEditProposal, CoEditService, CoEditStatus, DemoCoEditEngine, DemoEditorMutation,
        LiveTargetState, ScenarioKey, SubmissionOutcome, UserResolutionChoice,
    },
    kar_bridge::{KarBridgeEvent, KarBridgeHandle, KarCommand, ProjectedChatState},
    office_host::{OfficeEditorView, OfficeTab, clean_doc_para_text, materialize_coedit_demo_files},
    types::{
        ChatAccessMode, ChatListEntry, FeedConnectionState, OfficeKind, PendingPromptAttempt,
        ProviderStatusInfo, SelectionLocator, SelectionTarget, format_rev,
    },
};

#[derive(Clone, Debug)]
pub struct WorkbenchConfig {
    pub chat_dir: PathBuf,
    pub workspace_dir: PathBuf,
    pub docs_dir: PathBuf,
    pub open_files: Vec<PathBuf>,
    pub subscribe_http: Option<String>,
    pub start_demo_chat: bool,
    pub start_coedit_demo: bool,
    pub initial_coedit_scenario: Option<ScenarioKey>,
    pub dark_mode: bool,
}

impl Default for WorkbenchConfig {
    fn default() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let chat_dir = std::env::var_os("KAR_CHAT_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| cwd.join(".kar-desktop-chats"));
        Self {
            chat_dir,
            workspace_dir: cwd.clone(),
            docs_dir: cwd.join(".kar-desktop-docs"),
            open_files: Vec::new(),
            subscribe_http: None,
            start_demo_chat: false,
            start_coedit_demo: false,
            initial_coedit_scenario: None,
            dark_mode: false,
        }
    }
}

pub struct KarWorkbench {
    pub config: WorkbenchConfig,
    pub bridge: KarBridgeHandle,
    pub theme_mode: ThemeMode,

    // KUI structural & interactive entities
    panels: Entity<ResizablePanels>,
    runtime_menu: Entity<DropdownMenu>,
    chat_actions_menu: Entity<DropdownMenu>,
    office_menu: Entity<DropdownMenu>,
    mode_badge: Entity<Badge>,
    phase_badge: Entity<Badge>,
    status_alert: Entity<Alert>,
    show_alert: bool,
    alert_opens_demo: bool,

    composer_input: Entity<Textarea>,
    http_dialog: Entity<Dialog>,
    http_url_input: Entity<Input>,
    open_path_dialog: Entity<Dialog>,
    open_path_input: Entity<Input>,
    quick_edit_dialog: Entity<Dialog>,
    quick_edit_primary: Entity<Input>,
    quick_edit_secondary: Entity<Input>,
    coedit_manual_input: Entity<Input>,

    // Kar runtime & chat state
    pub selected_runtime: String,
    pub providers: Vec<ProviderStatusInfo>,
    pub chat_list: Vec<ChatListEntry>,
    pub access_mode: ChatAccessMode,
    pub connection_state: FeedConnectionState,
    pub projected: ProjectedChatState,
    pub turn_targets: BTreeMap<Uuid, SelectionTarget>,
    pub prompt_attempts: BTreeMap<Uuid, PendingPromptAttempt>,
    pub active_http_server_url: Option<String>,
    pub tool_inspectors: BTreeMap<(Uuid, String), Entity<InvocationInspector>>,

    // Office tabs & selection binding
    pub office_tabs: Vec<OfficeTab>,
    pub active_office_tab: Option<usize>,
    next_tab_id: u64,
    pub pinned_target: Option<SelectionTarget>,
    pub office_notice: Option<String>,

    // Human-AI Co-Editing (replaceable trait + isolated demo state)
    pub coedit_engine: Box<dyn CoEditService>,
    pub coedit_panel_open: bool,
    pub active_coedit_scenario: ScenarioKey,
    pub coedit_demo_dir: Option<PathBuf>,
    pub coedit_feedback: Option<String>,
}

impl KarWorkbench {
    pub fn new(config: WorkbenchConfig, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme_mode = if config.dark_mode {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        apply_unified_theme(theme_mode, cx);

        let bridge =
            KarBridgeHandle::spawn(config.chat_dir.clone(), config.workspace_dir.clone());

        let runtime_items = ["codex", "grok", "claude", "opencode", "kimi", "pi", "qwen", "cursor"]
            .into_iter()
            .map(|id| MenuItem::new(id, format!("运行时: {id}")))
            .collect();
        let runtime_menu =
            cx.new(|cx| DropdownMenu::new("kar-runtime-menu", "运行时: codex", runtime_items, cx));

        let chat_action_items = vec![
            MenuItem::new("start_http", "启动本聊天只读 HTTP/SSE 端点")
                .with_detail("127.0.0.1 随机端口"),
            MenuItem::new("stop_http", "停止本聊天 HTTP/SSE 端点")
                .with_detail("模拟断线后可再次启动重连"),
            MenuItem::new("open_second_window", "打开第二个客户端窗口观察此聊天")
                .with_detail("验证单控制器 + 只读订阅"),
            MenuItem::new("refresh_all", "刷新供应商探测与聊天列表").with_separator(),
            MenuItem::new("abort_turn", "中止当前运行回合"),
        ];
        let chat_actions_menu =
            cx.new(|cx| DropdownMenu::new("kar-chat-actions", "会话操作", chat_action_items, cx));

        let office_menu_items = vec![
            MenuItem::new("sample_docx", "打开 Word 样本副本 (.docx)"),
            MenuItem::new("sample_xlsx", "打开 Excel 样本副本 (.xlsx)"),
            MenuItem::new("sample_pptx", "打开 PPT 样本副本 (.pptx)"),
            MenuItem::new("blank_docx", "新建空白 Word (.docx)").with_separator(),
            MenuItem::new("blank_xlsx", "新建空白 Excel (.xlsx)"),
            MenuItem::new("blank_pptx", "新建空白 PPT (.pptx)"),
            MenuItem::new("coedit_demo", "进入人机双写演示工坊 (演示数据)").with_separator(),
        ];
        let office_menu =
            cx.new(|cx| DropdownMenu::new("kar-office-menu", "文档与演示", office_menu_items, cx));

        let mode_badge = cx.new(|_| Badge::new("未选择聊天", BadgeTone::Neutral));
        let phase_badge = cx.new(|_| Badge::new("空聊天", BadgeTone::Neutral));
        let status_alert = cx.new(|_| {
            Alert::new(
                "正在探测本机 Kar 供应商环境…",
                "文件与网络操作均在独立 Tokio 工作线程运行。",
                Severity::Info,
            )
        });

        let composer_input = cx.new(|cx| {
            Textarea::new("kar-composer", "向 Kar 发送消息 (Cmd+Enter 发送)", window, cx)
                .with_placeholder(
                    "输入任务指令，或在右侧选中文档段落 / 单元格 / 幻灯片对象后点击「让 AI 修改选区」…",
                    cx,
                )
        });

        let http_url_input = cx.new(|cx| {
            let initial = config
                .subscribe_http
                .clone()
                .unwrap_or_else(|| "http://127.0.0.1:18090".into());
            Input::new("kar-http-url-input", "Kar 聊天 HTTP 端点地址", window, cx)
                .with_value(initial, window, cx)
                .with_placeholder("例如 http://127.0.0.1:18090", cx)
                .with_description("先请求 GET /snapshot 获取完整历史与游标，再通过 GET /events 实时续收（只读模式）")
        });
        let url_field_for_dialog = http_url_input.clone();
        let http_dialog = cx.new(|cx| {
            Dialog::new(
                "kar-http-dialog",
                "连接 HTTP 订阅",
                "连接到已有聊天的只读 HTTP/SSE 服务",
                cx,
            )
            .with_description(
                "该端点仅提供快照与实时订阅（HttpChatFeed + ChatReplica）。连接后界面将明确显示只读状态。",
            )
            .with_body(move |_, _| div().w_full().py_2().child(url_field_for_dialog.clone()))
            .with_cancel("取消")
            .with_confirm("连接只读订阅")
        });

        let open_path_input = cx.new(|cx| {
            Input::new("kar-open-path-input", "本地 Office 文档路径 (.docx / .xlsx / .pptx)", window, cx)
                .with_placeholder("/path/to/document.docx", cx)
                .with_description("建议使用测试副本路径，直接在右侧真实 Kantor 编辑器中打开")
        });
        let path_field_for_dialog = open_path_input.clone();
        let open_path_dialog = cx.new(|cx| {
            Dialog::new("kar-open-path-dialog", "打开路径…", "打开本地 Office 文档", cx)
                .with_body(move |_, _| div().w_full().py_2().child(path_field_for_dialog.clone()))
                .with_cancel("取消")
                .with_confirm("打开文档")
        });

        let quick_edit_primary = cx.new(|cx| {
            Input::new(
                "kar-quick-edit-primary",
                "目标新内容（Word 段落文本 / Excel 数值或公式 / PPT 对象文字）",
                window,
                cx,
            )
            .with_placeholder("输入修改后的文本，Excel 支持 =SUM(...) 等公式", cx)
        });
        let quick_edit_secondary = cx.new(|cx| {
            Input::new(
                "kar-quick-edit-secondary",
                "附加定位参数（Excel 单元格如 B4；PPT 新坐标 x,y 厘米如 5.2,3.8）",
                window,
                cx,
            )
            .with_placeholder("Word 留空；Excel 填单元格地址；PPT 可选填 x,y (cm)", cx)
        });
        let qe1 = quick_edit_primary.clone();
        let qe2 = quick_edit_secondary.clone();
        let quick_edit_dialog = cx.new(|cx| {
            Dialog::new(
                "kar-quick-edit-dialog",
                "编辑选区",
                "编辑当前 Kantor 文档选区内容",
                cx,
            )
            .with_description("除直接在下方 Kantor 画布内编辑外，也可在此精确输入段落、公式或坐标。")
            .with_body(move |_, _| {
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .py_2()
                    .child(qe1.clone())
                    .child(qe2.clone())
            })
            .with_cancel("取消")
            .with_confirm("应用修改")
        });

        let coedit_manual_input = cx.new(|cx| {
            Input::new(
                "kar-coedit-manual-input",
                "手工合并值（用于冲突时手动输入合并结果或公式）",
                window,
                cx,
            )
            .with_value("142000", window, cx)
            .with_placeholder("输入手工合并后的值，例如 142000 或 =(135000+148000)/2", cx)
        });

        // Build 3-pane KUI ResizablePanels delegating rendering back to WeakEntity<KarWorkbench>
        let weak_left = cx.weak_entity();
        let weak_center = cx.weak_entity();
        let weak_right = cx.weak_entity();
        let panels = cx.new(|cx| {
            ResizablePanels::new(
                "kar-workbench-panels",
                "Kar Desktop Workbench",
                vec![
                    Pane::new("kar-sidebar", "聊天与文档侧栏", 0.21, move |_, window, cx| {
                        weak_left
                            .upgrade()
                            .map(|e| e.update(cx, |this, cx| this.render_sidebar(window, cx)))
                            .unwrap_or_else(|| div().into_any_element())
                    })
                    .with_limits(230., Some(340.))
                    .collapsible(),
                    Pane::new("kar-chat-pane", "Kar 聊天工作台", 0.37, move |_, window, cx| {
                        weak_center
                            .upgrade()
                            .map(|e| e.update(cx, |this, cx| this.render_chat_pane(window, cx)))
                            .unwrap_or_else(|| div().into_any_element())
                    })
                    .with_limits(350., None),
                    Pane::new(
                        "kar-office-pane",
                        "Kantor Office 编辑器",
                        0.42,
                        move |_, window, cx| {
                            weak_right
                                .upgrade()
                                .map(|e| {
                                    e.update(cx, |this, cx| this.render_office_pane(window, cx))
                                })
                                .unwrap_or_else(|| div().into_any_element())
                        },
                    )
                    .with_limits(400., None)
                    .collapsible(),
                ],
                cx,
            )
        });

        // Wire KUI component subscriptions
        cx.subscribe_in(
            &runtime_menu,
            window,
            |this, menu, sel: &MenuSelection, _, cx| {
                let id = sel.0.to_string();
                this.selected_runtime = id.clone();
                menu.update(cx, |m, cx| m.set_label(format!("运行时: {id}"), cx));
                this.refresh_provider_alert(cx);
                cx.notify();
            },
        )
        .detach();

        cx.subscribe_in(
            &chat_actions_menu,
            window,
            |this, _, sel: &MenuSelection, window, cx| {
                this.handle_chat_menu_action(sel.0.as_ref(), window, cx);
            },
        )
        .detach();

        cx.subscribe_in(
            &office_menu,
            window,
            |this, _, sel: &MenuSelection, window, cx| {
                this.handle_office_menu_action(sel.0.as_ref(), window, cx);
            },
        )
        .detach();

        cx.subscribe_in(&status_alert, window, |this, _, _: &AlertAction, _, cx| {
            if this.alert_opens_demo {
                this.bridge.send(KarCommand::EnterDemoShowcase);
            }
            cx.notify();
        })
        .detach();

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

        cx.subscribe_in(
            &open_path_dialog,
            window,
            |this, _, ev: &SurfaceEvent, window, cx| {
                if *ev == SurfaceEvent::Confirmed {
                    let raw = this.open_path_input.read(cx).value(cx).trim().to_owned();
                    if !raw.is_empty() {
                        this.open_office_file_tab(PathBuf::from(raw), false, None, window, cx);
                    }
                }
            },
        )
        .detach();

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

        cx.subscribe_in(
            &composer_input,
            window,
            |this, _, ev: &FieldEvent, window, cx| {
                if matches!(ev, FieldEvent::Submitted) {
                    this.submit_composer_prompt(window, cx);
                }
            },
        )
        .detach();

        let mut this = Self {
            config: config.clone(),
            bridge: bridge.clone(),
            theme_mode,
            panels,
            runtime_menu,
            chat_actions_menu,
            office_menu,
            mode_badge,
            phase_badge,
            status_alert,
            show_alert: false,
            alert_opens_demo: false,
            composer_input,
            http_dialog,
            http_url_input,
            open_path_dialog,
            open_path_input,
            quick_edit_dialog,
            quick_edit_primary,
            quick_edit_secondary,
            coedit_manual_input,
            selected_runtime: "codex".into(),
            providers: Vec::new(),
            chat_list: Vec::new(),
            access_mode: ChatAccessMode::NoneSelected,
            connection_state: FeedConnectionState::Idle,
            projected: ProjectedChatState {
                info: None,
                turns: Vec::new(),
                pending_requests: Vec::new(),
                cursor: None,
                is_demo: false,
            },
            turn_targets: BTreeMap::new(),
            prompt_attempts: BTreeMap::new(),
            active_http_server_url: None,
            tool_inspectors: BTreeMap::new(),
            office_tabs: Vec::new(),
            active_office_tab: None,
            next_tab_id: 1,
            pinned_target: None,
            office_notice: None,
            coedit_engine: Box::new(DemoCoEditEngine::new()),
            coedit_panel_open: false,
            active_coedit_scenario: config
                .initial_coedit_scenario
                .unwrap_or(ScenarioKey::WordSameParagraph),
            coedit_demo_dir: None,
            coedit_feedback: None,
        };

        // Open initial Office files requested on command line
        for path in &config.open_files {
            this.open_office_file_tab(path.clone(), false, None, window, cx);
        }

        // Launch requested startup mode
        if let Some(url) = &config.subscribe_http {
            this.bridge
                .send(KarCommand::ConnectHttpFeed { url: url.clone() });
        } else if config.start_demo_chat {
            this.bridge.send(KarCommand::EnterDemoShowcase);
        }

        if config.start_coedit_demo {
            this.enter_coedit_demo_workshop(window, cx);
            if let Some(sc) = config.initial_coedit_scenario {
                this.switch_coedit_scenario(sc, window, cx);
            }
        }

        // Consume off-thread Kar bridge events
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

        this
    }

    // ── Bridge Event Handling ───────────────────────────────────────────────

    fn on_bridge_event(&mut self, ev: KarBridgeEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            KarBridgeEvent::StoreReady { .. } => {}
            KarBridgeEvent::ProvidersUpdated(list) => {
                self.providers = list;
                self.refresh_provider_alert(cx);
            }
            KarBridgeEvent::ChatsListed(entries) => {
                let should_auto_open = matches!(self.access_mode, ChatAccessMode::NoneSelected)
                    && self.config.subscribe_http.is_none()
                    && !self.config.start_demo_chat
                    && !entries.is_empty();
                let first_id = entries.first().map(|e| e.info.id);
                self.chat_list = entries;
                if should_auto_open && let Some(chat_id) = first_id {
                    self.bridge.send(KarCommand::SelectLocalChat {
                        chat_id,
                        prefer_read_only: false,
                    });
                }
            }
            KarBridgeEvent::TurnTargetsLoaded(map) => {
                self.turn_targets = map;
            }
            KarBridgeEvent::AccessModeChanged(mode) => {
                let writable = mode.is_writable();
                self.composer_input.update(cx, |inp, cx| {
                    inp.set_disabled(!writable, cx);
                    let ph = match &mode {
                        ChatAccessMode::HttpReadOnly { url, .. } => format!(
                            "只读模式：当前仅连接 HTTP 快照与事件端点 ({url})，不支持发送消息"
                        ),
                        ChatAccessMode::LocalReadOnlySubscriber { reason, .. } => {
                            format!("只读模式：{reason}")
                        }
                        ChatAccessMode::NoneSelected => {
                            "请先点击左侧「+ 新建聊天」创建真实会话，或进入「独立演示入口」…".into()
                        }
                        ChatAccessMode::DemoShowcase => {
                            "【演示模式】输入任意文字体验回合时间线与工具卡片更新（不写真实存储）…".into()
                        }
                        ChatAccessMode::WriteController { .. } => {
                            "向真实 Kar 发送消息 (Cmd+Enter 发送)，或点击右侧「让 AI 修改选区」绑定文档目标…".into()
                        }
                    };
                    inp.set_description(ph, cx);
                });
                self.access_mode = mode;
                self.sync_badges_and_alerts(cx);
            }
            KarBridgeEvent::ConnectionStateChanged(conn) => {
                self.connection_state = conn;
                self.sync_badges_and_alerts(cx);
            }
            KarBridgeEvent::ChatStateUpdated(proj) => {
                self.sync_tool_inspectors(&proj.turns, cx);
                self.projected = proj;
                self.sync_badges_and_alerts(cx);
            }
            KarBridgeEvent::PromptLifecycle { attempt } => {
                self.prompt_attempts.insert(attempt.request_id, attempt);
                self.sync_badges_and_alerts(cx);
            }
            KarBridgeEvent::HttpServerState {
                listening_url,
                message,
                ..
            } => {
                self.active_http_server_url = listening_url;
                if let Some(msg) = message {
                    self.show_custom_alert("HTTP 实时订阅服务端点", &msg, Severity::Info, false, cx);
                }
            }
            KarBridgeEvent::BannerNotice {
                title,
                detail,
                is_error,
            } => {
                let sev = if is_error {
                    Severity::Error
                } else {
                    Severity::Info
                };
                self.show_custom_alert(&title, &detail, sev, is_error, cx);
            }
        }
        let _ = window;
        cx.notify();
    }

    fn sync_tool_inspectors(&mut self, turns: &[ChatTurn], cx: &mut Context<Self>) {
        for turn in turns {
            for item in &turn.items {
                if let Item::Tool { id, activity } = &item.item {
                    let key = (turn.id, id.clone());
                    let inv = tool_to_invocation(id, activity);
                    if let Some(existing) = self.tool_inspectors.get(&key) {
                        // Reuse the EXACT same InvocationInspector card for the same call ID!
                        existing.update(cx, |inspector, cx| {
                            if let Some(first) = inspector.calls.first_mut() {
                                *first = inv;
                            } else {
                                inspector.calls.push(inv);
                            }
                            cx.notify();
                        });
                    } else {
                        let entity = cx.new(|_| {
                            InvocationInspector::new(format!("tool-{}-{id}", &turn.id.to_string()[..8]), vec![inv])
                        });
                        self.tool_inspectors.insert(key, entity);
                    }
                }
            }
        }
    }

    fn sync_badges_and_alerts(&mut self, cx: &mut Context<Self>) {
        // 1. Mode badge
        let (mode_label, mode_tone) = match &self.access_mode {
            ChatAccessMode::NoneSelected => ("未选聊天".to_owned(), BadgeTone::Neutral),
            ChatAccessMode::WriteController { chat_id } => (
                format!("写入控制器 · {}", &chat_id.to_string()[..8]),
                BadgeTone::Success,
            ),
            ChatAccessMode::LocalReadOnlySubscriber { chat_id, .. } => (
                format!("只读订阅 · {}", &chat_id.to_string()[..8]),
                BadgeTone::Warning,
            ),
            ChatAccessMode::HttpReadOnly { .. } => {
                ("HTTP 只读模式".to_owned(), BadgeTone::Info)
            }
            ChatAccessMode::DemoShowcase => ("演示数据 · 样例会话".to_owned(), BadgeTone::Warning),
        };
        self.mode_badge
            .update(cx, |b, cx| b.set(mode_label, mode_tone, cx));

        // 2. Phase / Connection badge (covers 空聊天 / 发送中 / 工具运行中 / 已完成 / 中断 / 重连)
        let in_flight_send = self.prompt_attempts.values().any(|a| a.in_flight);
        let (phase_label, phase_tone) = match &self.connection_state {
            FeedConnectionState::Reconnecting { attempt, .. } => (
                format!("重连中 (#{attempt})"),
                BadgeTone::Warning,
            ),
            FeedConnectionState::Disconnected { .. } => {
                ("已断开".to_owned(), BadgeTone::Danger)
            }
            FeedConnectionState::Connecting { .. } => {
                ("连接中…".to_owned(), BadgeTone::Info)
            }
            _ => {
                if self.projected.turns.is_empty() && !in_flight_send {
                    ("空聊天".to_owned(), BadgeTone::Neutral)
                } else if let Some(last) = self.projected.turns.last() {
                    let any_tool_running = last.items.iter().any(|it| {
                        matches!(&it.item, Item::Tool { activity, .. } if !activity.finished)
                    });
                    if any_tool_running && last.status == TurnStatus::Running {
                        ("工具运行中".to_owned(), BadgeTone::Info)
                    } else if last.status == TurnStatus::Running || in_flight_send {
                        ("发送中 / 回复中".to_owned(), BadgeTone::Info)
                    } else {
                        match last.status {
                            TurnStatus::Completed => ("已完成".to_owned(), BadgeTone::Success),
                            TurnStatus::Interrupted => ("已中断".to_owned(), BadgeTone::Warning),
                            TurnStatus::Failed => ("失败".to_owned(), BadgeTone::Danger),
                            TurnStatus::Aborted => ("已中止".to_owned(), BadgeTone::Warning),
                            TurnStatus::Running => ("运行中".to_owned(), BadgeTone::Info),
                        }
                    }
                } else if in_flight_send {
                    ("发送中…".to_owned(), BadgeTone::Info)
                } else {
                    ("就绪".to_owned(), BadgeTone::Neutral)
                }
            }
        };
        self.phase_badge
            .update(cx, |b, cx| b.set(phase_label, phase_tone, cx));

        // 3. Connection / Read-only alerts (never auto-switches to demo on disconnect!)
        match &self.connection_state {
            FeedConnectionState::Reconnecting {
                last_cursor,
                reason,
                ..
            } => {
                let cur = last_cursor.as_deref().unwrap_or("无");
                self.show_custom_alert(
                    "实时连接已中断 · 正在重连",
                    &format!("{reason}（已应用游标：{cur}；保持真实历史显示，不切换为演示数据）"),
                    Severity::Warning,
                    false,
                    cx,
                );
            }
            FeedConnectionState::Disconnected {
                last_cursor,
                reason,
            } => {
                let cur = last_cursor.as_deref().unwrap_or("无");
                self.show_custom_alert(
                    "连接已断开",
                    &format!("{reason}（最后游标：{cur}；不会自动切换至演示结果）"),
                    Severity::Error,
                    false,
                    cx,
                );
            }
            FeedConnectionState::Connected {
                cursor,
                duplicates_ignored,
                gaps_recovered,
            } if self.access_mode.is_http_read_only() => {
                let cur = cursor.as_deref().unwrap_or("-");
                self.show_custom_alert(
                    "HTTP 只读订阅模式（HttpChatFeed + ChatReplica）",
                    &format!(
                        "该 HTTP 服务仅提供 GET /snapshot 与 GET /events 订阅，界面处于只读观察状态。当前游标：{cur} · 忽略重复事件：{duplicates_ignored} · 游标缺口恢复：{gaps_recovered}"
                    ),
                    Severity::Info,
                    false,
                    cx,
                );
            }
            _ => {
                let local_ro_reason = match &self.access_mode {
                    ChatAccessMode::LocalReadOnlySubscriber { reason, .. } => Some(reason.clone()),
                    _ => None,
                };
                if let Some(reason) = local_ro_reason {
                    self.show_custom_alert(
                        "只读订阅模式（单写入控制器保护）",
                        &reason,
                        Severity::Warning,
                        false,
                        cx,
                    );
                } else {
                    self.refresh_provider_alert(cx);
                }
            }
        }
    }

    fn refresh_provider_alert(&mut self, cx: &mut Context<Self>) {
        if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            self.show_custom_alert(
                "独立演示模式（演示数据）",
                "当前时间线由内置样例驱动，不会向真实 ChatStore 或供应商发送请求。点击左侧「+ 新建聊天」可随时回到真实 Kar。",
                Severity::Warning,
                false,
                cx,
            );
            return;
        }
        if let Some(prov) = self.providers.iter().find(|p| p.id == self.selected_runtime)
            && let Some(reason) = prov.diagnostic_reason()
        {
            self.alert_opens_demo = true;
            self.show_alert = true;
            self.status_alert.update(cx, |a, cx| {
                a.replace(
                    Alert::new(
                        format!("供应商 `{}` 当前不可直接发起会话", prov.id),
                        reason,
                        Severity::Warning,
                    )
                    .with_action("进入独立演示模式"),
                    cx,
                );
            });
            return;
        }
        if self.active_http_server_url.is_none() {
            self.show_alert = false;
        }
    }

    fn show_custom_alert(
        &mut self,
        title: &str,
        detail: &str,
        severity: Severity,
        offer_demo: bool,
        cx: &mut Context<Self>,
    ) {
        self.alert_opens_demo = offer_demo;
        self.show_alert = true;
        let mut next = Alert::new(title.to_owned(), detail.to_owned(), severity);
        if offer_demo {
            next = next.with_action("打开独立演示入口");
        }
        self.status_alert.update(cx, |a, cx| a.replace(next, cx));
    }

    // ── Actions & Workflows ─────────────────────────────────────────────────

    pub fn select_runtime_by_id(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected_runtime = id.to_owned();
        self.runtime_menu
            .update(cx, |m, cx| m.set_label(format!("运行时: {id}"), cx));
        self.refresh_provider_alert(cx);
        cx.notify();
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

    pub fn create_new_real_chat(&mut self, cx: &mut Context<Self>) {
        if let Some(prov) = self.providers.iter().find(|p| p.id == self.selected_runtime)
            && let Some(reason) = prov.diagnostic_reason()
        {
            self.show_custom_alert(
                &format!("无法用 `{}` 新建真实聊天", prov.id),
                &reason,
                Severity::Error,
                true,
                cx,
            );
            cx.notify();
            return;
        }
        let program = self
            .providers
            .iter()
            .find(|p| p.id == self.selected_runtime)
            .and_then(|p| match &p.availability {
                crate::types::ProviderAvailability::Available { command, .. } => command.clone(),
                _ => None,
            });
        self.bridge.send(KarCommand::CreateChat {
            runtime_id: self.selected_runtime.clone(),
            program,
            workspace: self.config.workspace_dir.clone(),
            approval_mode: ApprovalMode::Host,
        });
    }

    pub fn submit_composer_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.access_mode.is_writable() {
            return;
        }
        let raw = self.composer_input.read(cx).value(cx).trim().to_owned();
        if raw.is_empty() {
            return;
        }
        let request_id = Uuid::new_v4();
        let target = self.pinned_target.clone();
        let payload = match &target {
            Some(tgt) => tgt.compose_prompt(&raw),
            None => raw.clone(),
        };
        if let Some(ref tgt) = target {
            self.turn_targets.insert(request_id, tgt.clone());
        }
        self.composer_input
            .update(cx, |inp, cx| inp.set_value("", window, cx));

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
            });
        }
        cx.notify();
    }

    /// Retry an interrupted or failed turn using the EXACT same `request_id` UUID!
    pub fn retry_turn_with_same_uuid(&mut self, turn_id: Uuid, fallback_user_text: String, cx: &mut Context<Self>) {
        let (raw_user_text, payload, target) = if let Some(prev) = self.prompt_attempts.get(&turn_id) {
            (
                prev.raw_user_text.clone(),
                prev.sent_payload.clone(),
                prev.target.clone(),
            )
        } else {
            (
                fallback_user_text.clone(),
                fallback_user_text,
                self.turn_targets.get(&turn_id).cloned(),
            )
        };
        self.bridge.send(KarCommand::SendPrompt {
            request_id: turn_id,
            raw_user_text,
            payload,
            target,
        });
        cx.notify();
    }

    fn handle_chat_menu_action(&mut self, action_id: &str, _window: &mut Window, cx: &mut Context<Self>) {
        match action_id {
            "start_http" => {
                if let Some(chat_id) = self.current_chat_id() {
                    self.bridge.send(KarCommand::StartHttpServer {
                        chat_id,
                        listen_addr: "127.0.0.1:0".into(),
                    });
                } else {
                    self.show_custom_alert(
                        "请先选择或新建一个真实聊天",
                        "HTTP/SSE 端点需要绑定一个具体的持久化聊天 ID。",
                        Severity::Warning,
                        false,
                        cx,
                    );
                }
            }
            "stop_http" => {
                self.bridge.send(KarCommand::StopHttpServer);
            }
            "open_second_window" => {
                self.open_subscriber_second_window(cx);
            }
            "refresh_all" => {
                self.bridge.send(KarCommand::RefreshStoreAndProviders {
                    store_dir: self.config.chat_dir.clone(),
                    workspace: self.config.workspace_dir.clone(),
                });
            }
            "abort_turn" => {
                self.bridge.send(KarCommand::AbortTurn);
            }
            _ => {}
        }
        cx.notify();
    }

    fn open_subscriber_second_window(&mut self, cx: &mut Context<Self>) {
        let mut sub_cfg = self.config.clone();
        sub_cfg.open_files.clear();
        sub_cfg.start_coedit_demo = false;
        if let Some(url) = &self.active_http_server_url {
            sub_cfg.subscribe_http = Some(url.clone());
        }
        let title = match &sub_cfg.subscribe_http {
            Some(url) => format!("Kar Desktop · 第二客户端只读观察 ({url})"),
            None => "Kar Desktop · 第二客户端窗口".into(),
        };
        let options = WindowOptions {
            app_id: Some("kar-desktop".into()),
            titlebar: Some(TitlebarOptions {
                title: Some(title.into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1240.), px(780.)),
                cx,
            ))),
            window_min_size: Some(size(px(820.), px(520.))),
            ..Default::default()
        };
        let _ = gpui_kit::open_window(options, cx, move |w, cx| {
            cx.new(|cx| KarWorkbench::new(sub_cfg, w, cx))
        });
    }

    fn current_chat_id(&self) -> Option<Uuid> {
        match &self.access_mode {
            ChatAccessMode::WriteController { chat_id }
            | ChatAccessMode::LocalReadOnlySubscriber { chat_id, .. } => Some(*chat_id),
            ChatAccessMode::HttpReadOnly { chat_id, .. } => {
                chat_id.or_else(|| self.projected.info.as_ref().map(|i| i.id))
            }
            _ => self.projected.info.as_ref().map(|i| i.id),
        }
    }

    // ── Office Host & Selection Binding ─────────────────────────────────────

    fn handle_office_menu_action(&mut self, action_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match action_id {
            "sample_docx" => self.open_working_sample("sample_minutes.docx", window, cx),
            "sample_xlsx" => self.open_working_sample("sample_formulas.xlsx", window, cx),
            "sample_pptx" => self.open_working_sample("sample_deck.pptx", window, cx),
            "blank_docx" => self.open_blank_office_tab(OfficeKind::Word, window, cx),
            "blank_xlsx" => self.open_blank_office_tab(OfficeKind::Excel, window, cx),
            "blank_pptx" => self.open_blank_office_tab(OfficeKind::Ppt, window, cx),
            "coedit_demo" => self.enter_coedit_demo_workshop(window, cx),
            _ => {}
        }
    }

    fn open_working_sample(&mut self, filename: &str, window: &mut Window, cx: &mut Context<Self>) {
        let candidate = self.config.docs_dir.join(filename);
        if candidate.is_file() {
            self.open_office_file_tab(candidate, false, None, window, cx);
        } else {
            self.office_notice = Some(format!(
                "未在工作副本目录 {} 找到 {filename}，可使用「打开路径…」加载任意副本",
                self.config.docs_dir.display()
            ));
            cx.notify();
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
        let canonical = path.canonicalize().unwrap_or(path);
        if let Some(idx) = self.office_tabs.iter().position(|t| {
            t.is_demo == is_demo
                && t.path(cx)
                    .map(|p| p.canonicalize().unwrap_or(p) == canonical)
                    .unwrap_or(false)
        }) {
            self.active_office_tab = Some(idx);
            cx.notify();
            return;
        }
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        match OfficeTab::open_file(id, canonical.clone(), is_demo, demo_key, window, cx) {
            Ok(tab) => {
                if !is_demo {
                    self.coedit_panel_open = false;
                }
                self.office_tabs.push(tab);
                self.active_office_tab = Some(self.office_tabs.len() - 1);
                self.office_notice = Some(format!("已打开 {}", canonical.display()));
            }
            Err(err) => {
                self.office_notice = Some(format!("打开失败: {err:#}"));
            }
        }
        cx.notify();
    }

    fn open_blank_office_tab(&mut self, kind: OfficeKind, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = OfficeTab::new_blank(id, kind, window, cx);
        self.coedit_panel_open = false;
        self.office_tabs.push(tab);
        self.active_office_tab = Some(self.office_tabs.len() - 1);
        cx.notify();
    }

    pub fn bind_active_selection_to_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(idx) = self.active_office_tab else {
            return;
        };
        let Some(tab) = self.office_tabs.get(idx) else {
            return;
        };
        let target = tab.capture_selection(cx);
        self.office_notice = Some(format!("已将选区「{}」绑定至聊天输入栏", target.short_badge()));
        self.pinned_target = Some(target);
        let focus = self.composer_input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    pub fn jump_to_selection_target(
        &mut self,
        target: &SelectionTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let found_idx = self
            .office_tabs
            .iter()
            .position(|t| t.id == target.tab_id)
            .or_else(|| {
                let target_path = target.file_path.as_ref()?;
                let canon = target_path.canonicalize().unwrap_or_else(|_| target_path.clone());
                self.office_tabs.iter().position(|t| {
                    t.path(cx)
                        .map(|p| p.canonicalize().unwrap_or(p) == canon)
                        .unwrap_or(false)
                })
            });

        let tab_idx = match found_idx {
            Some(i) => Some(i),
            None => {
                if let Some(ref path) = target.file_path
                    && path.is_file()
                {
                    self.open_office_file_tab(path.clone(), false, None, window, cx);
                    self.active_office_tab
                } else {
                    None
                }
            }
        };

        if let Some(i) = tab_idx
            && let Some(tab) = self.office_tabs.get(i).cloned()
        {
            self.active_office_tab = Some(i);
            tab.reveal_locator(&target.locator, window, cx);
            self.office_notice = Some(format!("已定位回原目标：{}", target.short_badge()));
            cx.notify();
        }
    }

    fn populate_quick_edit_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .cloned()
        else {
            return;
        };
        let sel = tab.capture_selection(cx);
        match &sel.locator {
            SelectionLocator::WordParagraph { start_para, .. } => {
                let text = sel.excerpt.clone();
                let p_num = format!("{}", start_para + 1);
                self.quick_edit_primary
                    .update(cx, |inp, cx| inp.set_value(text, window, cx));
                self.quick_edit_secondary
                    .update(cx, |inp, cx| inp.set_value(p_num, window, cx));
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
                let addr = active_cell.clone();
                self.quick_edit_primary
                    .update(cx, |inp, cx| inp.set_value(initial, window, cx));
                self.quick_edit_secondary
                    .update(cx, |inp, cx| inp.set_value(addr, window, cx));
            }
            SelectionLocator::PptObject {
                bounds_cm,
                ..
            } => {
                let txt = sel.excerpt.clone();
                let coords = bounds_cm
                    .map(|(x, y, _, _)| format!("{x:.2},{y:.2}"))
                    .unwrap_or_else(|| "2.00,3.00".into());
                self.quick_edit_primary
                    .update(cx, |inp, cx| inp.set_value(txt, window, cx));
                self.quick_edit_secondary
                    .update(cx, |inp, cx| inp.set_value(coords, window, cx));
            }
        }
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
                    .map(|()| format!("已更新段落 #{}", idx + 1))
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
                    .map(|calc| format!("已写入 {cell} = `{primary}` → 计算结果 `{calc}`"))
            }
            SelectionLocator::PptObject {
                slide_index,
                shape_id,
                ..
            } => {
                let Some(sid) = *shape_id else {
                    self.office_notice = Some("当前幻灯片没有可编辑的形状对象".into());
                    cx.notify();
                    return;
                };
                let parsed_xy = secondary.split_once(',').and_then(|(xs, ys)| {
                    Some((xs.trim().parse::<f64>().ok()?, ys.trim().parse::<f64>().ok()?))
                });
                let new_txt = (!primary.is_empty()).then_some(primary.as_str());
                tab.update_ppt_shape(*slide_index, sid, new_txt, parsed_xy, window, cx)
                    .map(|()| format!("已更新第 {} 页对象 #{sid}", slide_index + 1))
            }
        };

        match outcome {
            Ok(msg) => self.office_notice = Some(msg),
            Err(err) => self.office_notice = Some(format!("修改未生效: {err:#}")),
        }
        cx.notify();
    }

    fn save_close_and_reopen_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(idx) = self.active_office_tab else {
            return;
        };
        let Some(tab) = self.office_tabs.get(idx).cloned() else {
            return;
        };
        if tab.is_demo {
            self.office_notice = Some("演示文档仅限内存/临时演示沙盒，不写回用户文件".into());
            cx.notify();
            return;
        }
        let target_path = match tab.path(cx) {
            Some(existing) => existing,
            None => {
                let ext = match tab.kind {
                    OfficeKind::Word => "docx",
                    OfficeKind::Excel => "xlsx",
                    OfficeKind::Ppt => "pptx",
                };
                self.config
                    .docs_dir
                    .join(format!("saved_untitled_{}.{ext}", tab.id))
            }
        };
        let copy_dest = build_saved_copy_path(&target_path, &self.config.docs_dir);
        match tab.save_copy_to(&copy_dest, window, cx) {
            Ok(_) => {
                // Close tab completely (drops Kantor view entity) and reopen from disk!
                self.office_tabs.remove(idx);
                self.active_office_tab = None;
                self.open_office_file_tab(copy_dest.clone(), false, None, window, cx);
                self.office_notice = Some(format!(
                    "已完成「保存副本 → 关闭视图 → 重新打开验证」：{}",
                    copy_dest.display()
                ));
            }
            Err(err) => {
                self.office_notice = Some(format!("保存副本失败: {err:#}"));
            }
        }
        cx.notify();
    }

    // ── Co-Editing Demo Workshop (Strictly Isolated Demo Tabs) ──────────────

    pub fn enter_coedit_demo_workshop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let demo_dir = self.coedit_demo_dir.clone().unwrap_or_else(|| {
            std::env::temp_dir().join(format!("kar_desktop_coedit_demo_{}", std::process::id()))
        });
        match materialize_coedit_demo_files(&demo_dir) {
            Ok((word_p, excel_p, ppt_p)) => {
                self.coedit_demo_dir = Some(demo_dir);
                self.open_office_file_tab(
                    word_p,
                    true,
                    Some(ScenarioKey::WordSameParagraph.as_str()),
                    window,
                    cx,
                );
                self.open_office_file_tab(
                    excel_p,
                    true,
                    Some(ScenarioKey::ExcelSameCellConflict.as_str()),
                    window,
                    cx,
                );
                self.open_office_file_tab(
                    ppt_p,
                    true,
                    Some(ScenarioKey::PptMoveAndText.as_str()),
                    window,
                    cx,
                );
                // Reset engine base revisions to match freshly opened demo views
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
                        let shape_id = if sc == ScenarioKey::PptMoveAndText {
                            if let OfficeEditorView::Ppt(e) = &tab.view {
                                e.read(cx)
                                    .agent_read(|p, _| p.slide(0).shapes.first().map(|s| s.id))
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        self.coedit_engine.reset_scenario(sc, base_rev, shape_id);
                    }
                }
                self.coedit_panel_open = true;
                self.switch_coedit_scenario(self.active_coedit_scenario, window, cx);
                self.coedit_feedback = Some(
                    "已加载三份隔离演示文档（Word / Excel / PPT）。所有操作均标注「演示数据」，绝不写回用户文件。"
                        .into(),
                );
            }
            Err(err) => {
                self.coedit_feedback = Some(format!("初始化双写演示文档失败: {err:#}"));
            }
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
        if let Some(idx) = self
            .office_tabs
            .iter()
            .position(|t| t.demo_scenario_key == Some(scenario.as_str()))
        {
            self.active_office_tab = Some(idx);
        }
        self.coedit_panel_open = true;
        cx.notify();
    }

    fn read_live_coedit_state(&self, scenario: ScenarioKey, cx: &App) -> Option<LiveTargetState> {
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
                let target_id = self
                    .coedit_engine
                    .proposals()
                    .iter()
                    .find(|p| p.scenario == ScenarioKey::PptMoveAndText)
                    .and_then(|p| p.target_shape_id)
                    .unwrap_or(2);
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

        // Snapshot current acknowledged state before human edits during conflict resolution
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
                let sid = self
                    .coedit_engine
                    .proposals()
                    .iter()
                    .find(|p| p.scenario == ScenarioKey::PptMoveAndText)
                    .and_then(|p| p.target_shape_id)
                    .unwrap_or(2);
                tab.update_ppt_shape(0, sid, None, Some((6.50, 4.80)), window, cx)
            }
            "ppt_mid_conflict_move" => {
                let sid = self
                    .coedit_engine
                    .proposals()
                    .iter()
                    .find(|p| p.scenario == ScenarioKey::PptMoveAndText)
                    .and_then(|p| p.target_shape_id)
                    .unwrap_or(2);
                tab.update_ppt_shape(0, sid, None, Some((8.20, 5.40)), window, cx)
            }
            "ppt_human_delete" => {
                let sid = self
                    .coedit_engine
                    .proposals()
                    .iter()
                    .find(|p| p.scenario == ScenarioKey::PptMoveAndText)
                    .and_then(|p| p.target_shape_id)
                    .unwrap_or(2);
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
            self.coedit_feedback = Some(format!("演示操作提示: {err:#}"));
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
            Ok(SubmissionOutcome::NeedsReconfirm {
                previous_revision,
                latest_revision,
                latest_value,
            }) => {
                self.coedit_feedback = Some(format!(
                    "⚠️ 解决期间文档再次变化（rev #{previous_revision} → rev #{latest_revision}，最新值：{latest_value}）。请核对并点击「确认新修订」后再提交！"
                ));
            }
            Ok(SubmissionOutcome::Accepted { proposal, mutation }) => {
                let apply_res = match mutation {
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
                    } => tab.update_ppt_shape(
                        slide,
                        shape_id,
                        text.as_deref(),
                        xy_cm,
                        window,
                        cx,
                    ),
                };
                match apply_res {
                    Ok(()) => {
                        self.coedit_feedback = proposal.resolution_result;
                    }
                    Err(err) => {
                        self.coedit_feedback = Some(format!("应用演示决议失败: {err:#}"));
                    }
                }
            }
            Err(err) => {
                self.coedit_feedback = Some(format!("无法提交决议: {err:#}"));
            }
        }
        cx.notify();
    }

    // ── Rendering (100% Pure Rust GPUI + KUI + Kantor) ──────────────────────

    fn palette(&self) -> UiColors {
        UiColors::for_mode(self.theme_mode)
    }

    fn render_sidebar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = self.palette();
        let weak = cx.weak_entity();
        let active_id = self.current_chat_id();

        let selected_prov = self
            .providers
            .iter()
            .find(|p| p.id == self.selected_runtime)
            .cloned();

        let mut chat_rows = div().flex().flex_col().gap_1();
        if self.chat_list.is_empty() {
            chat_rows = chat_rows.child(
                div()
                    .px_2()
                    .py_3()
                    .rounded(px(6.))
                    .bg(c.surface_subtle)
                    .text_size(px(12.))
                    .text_color(c.muted)
                    .child("当前隔离 ChatStore 暂无历史聊天，点击上方「+ 新建聊天」创建。"),
            );
        } else {
            for entry in &self.chat_list {
                let chat_id = entry.info.id;
                let is_selected = active_id == Some(chat_id)
                    && !matches!(self.access_mode, ChatAccessMode::DemoShowcase);
                let title = entry.title.clone();
                let meta = format!(
                    "{} · {} 回合 · {}",
                    entry.info.runtime,
                    entry.turn_count,
                    status_short_text(entry.last_status)
                );
                let weak_open = weak.clone();
                let weak_ro = weak.clone();

                chat_rows = chat_rows.child(
                    div()
                        .id(SharedString::from(format!("chat-row-{chat_id}")))
                        .px_2()
                        .py(px(6.))
                        .rounded(px(7.))
                        .border_1()
                        .border_color(if is_selected { c.border_strong } else { gpui::transparent_black() })
                        .bg(if is_selected { c.selected_bg } else { gpui::transparent_black() })
                        .hover(move |s| s.bg(c.hover_bg))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_1()
                        .on_click(move |_, _, cx| {
                            if let Some(e) = weak_open.upgrade() {
                                e.update(cx, |this, _| {
                                    this.bridge.send(KarCommand::SelectLocalChat {
                                        chat_id,
                                        prefer_read_only: false,
                                    });
                                });
                            }
                        })
                        .child(
                            div()
                                .flex_1()
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(c.ink)
                                        .truncate()
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(c.muted)
                                        .truncate()
                                        .child(meta),
                                ),
                        )
                        .child(
                            Button::new(SharedString::from(format!("ro-{chat_id}")))
                                .ghost()
                                .with_size(Size::Sm)
                                .label("只读")
                                .on_click(move |_, _, cx| {
                                    if let Some(e) = weak_ro.upgrade() {
                                        e.update(cx, |this, _| {
                                            this.bridge.send(KarCommand::SelectLocalChat {
                                                chat_id,
                                                prefer_read_only: true,
                                            });
                                        });
                                    }
                                }),
                        ),
                );
            }
        }

        // Open Office document list in sidebar
        let mut doc_rows = div().flex().flex_col().gap_1();
        for (i, tab) in self.office_tabs.iter().enumerate() {
            let is_active = self.active_office_tab == Some(i);
            let name = tab.doc_name(cx);
            let dirty = tab.is_dirty(cx);
            let tag = tab.kind.short_tag();
            let weak_tab = weak.clone();
            doc_rows = doc_rows.child(
                div()
                    .id(SharedString::from(format!("side-doc-{}", tab.id)))
                    .px_2()
                    .py(px(5.))
                    .rounded(px(6.))
                    .bg(if is_active { c.selected_bg } else { gpui::transparent_black() })
                    .hover(move |s| s.bg(c.hover_bg))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .on_click(move |_, _, cx| {
                        if let Some(e) = weak_tab.upgrade() {
                            e.update(cx, |this, cx| {
                                this.active_office_tab = Some(i);
                                cx.notify();
                            });
                        }
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .overflow_hidden()
                            .child(
                                div()
                                    .px(px(5.))
                                    .py(px(1.))
                                    .rounded(px(4.))
                                    .bg(c.surface_subtle)
                                    .text_size(px(10.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c.muted)
                                    .child(tag),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(c.ink)
                                    .truncate()
                                    .child(if dirty { format!("● {name}") } else { name }),
                            ),
                    ),
            );
        }

        let weak_new = weak.clone();
        let weak_demo = weak.clone();
        let weak_coedit = weak.clone();
        let weak_theme = weak.clone();
        let theme_btn_label = match self.theme_mode {
            ThemeMode::Light => "深色",
            ThemeMode::Dark => "浅色",
        };

        div()
            .size_full()
            .bg(c.sidebar_bg)
            .border_r_1()
            .border_color(c.border)
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .overflow_hidden()
            // Brand + Theme Toggle
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(c.ink)
                                    .child("Kar Studio"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(c.muted)
                                    .child("Rust · GPUI · KUI · Kantor"),
                            ),
                    )
                    .child(
                        Button::new("kar-theme-toggle")
                            .secondary()
                            .with_size(Size::Sm)
                            .label(theme_btn_label)
                            .on_click(move |_, _, cx| {
                                if let Some(e) = weak_theme.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.theme_mode = this.theme_mode.toggle();
                                        apply_unified_theme(this.theme_mode, cx);
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            // Runtime Dropdown + Live Probe Summary
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(self.runtime_menu.clone())
                    .when_some(selected_prov, |el, prov| {
                        let (badge_txt, ok) = prov.status_badge();
                        el.child(
                            div()
                                .px_2()
                                .py_1()
                                .rounded(px(6.))
                                .bg(c.surface_subtle)
                                .text_size(px(11.))
                                .text_color(if ok { c.ok } else { c.warn })
                                .child(format!("{} · {badge_txt}", prov.id)),
                        )
                    }),
            )
            // Primary Actions (KUI Buttons + KUI Modal Dialog Trigger)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        Button::new("btn-new-chat")
                            .primary()
                            .with_size(Size::Sm)
                            .label("+ 新建真实 Kar 聊天")
                            .on_click(move |_, _, cx| {
                                if let Some(e) = weak_new.upgrade() {
                                    e.update(cx, |this, cx| this.create_new_real_chat(cx));
                                }
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(self.http_dialog.clone())
                            .child(
                                Button::new("btn-demo-entry")
                                    .secondary()
                                    .with_size(Size::Sm)
                                    .label("演示聊天")
                                    .on_click(move |_, _, cx| {
                                        if let Some(e) = weak_demo.upgrade() {
                                            e.update(cx, |this, _| {
                                                this.bridge.send(KarCommand::EnterDemoShowcase);
                                            });
                                        }
                                    }),
                            ),
                    )
                    .child(
                        Button::new("btn-coedit-entry")
                            .secondary()
                            .with_size(Size::Sm)
                            .label("双写冲突演示工坊 (演示数据)")
                            .on_click(move |_, window, cx| {
                                if let Some(e) = weak_coedit.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.enter_coedit_demo_workshop(window, cx);
                                    });
                                }
                            }),
                    ),
            )
            // Chat List Section
            .child(
                div()
                    .flex_1()
                    .id("sidebar-scroll")
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c.muted)
                                    .child(format!("本地会话 ({})", self.chat_list.len())),
                            )
                            .child(chat_rows),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c.muted)
                                    .child(format!("已打开文档 ({})", self.office_tabs.len())),
                            )
                            .child(doc_rows),
                    ),
            )
            // Store isolation footer
            .child(
                div()
                    .pt_2()
                    .border_t_1()
                    .border_color(c.border)
                    .text_size(px(10.5))
                    .text_color(c.muted)
                    .truncate()
                    .child(format!("KAR_CHAT_DIR: {}", self.config.chat_dir.display())),
            )
            .into_any_element()
    }

    fn render_chat_pane(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = self.palette();
        let weak = cx.weak_entity();

        let header_title = match &self.access_mode {
            ChatAccessMode::DemoShowcase => "【演示数据】样例对话时间线".to_owned(),
            ChatAccessMode::HttpReadOnly { url, .. } => format!("HTTP 只读订阅 · {url}"),
            _ => self
                .projected
                .info
                .as_ref()
                .map(|i| format!("{} · {}", i.runtime, &i.id.to_string()[..8]))
                .unwrap_or_else(|| "Kar 聊天工作台".into()),
        };
        let header_sub = self
            .projected
            .info
            .as_ref()
            .map(|i| format!("工作区: {}", i.workspace.display()))
            .unwrap_or_else(|| format!("默认工作区: {}", self.config.workspace_dir.display()));

        // Timeline body
        let timeline_body = if self.projected.turns.is_empty() {
            self.render_empty_chat_state(&c, cx)
        } else {
            let mut list = div().flex().flex_col().gap_3().pb_4();
            for (idx, turn) in self.projected.turns.iter().enumerate() {
                list = list.child(self.render_turn_card(idx + 1, turn, &c, cx));
            }
            list.into_any_element()
        };

        // Pinned selection target strip above composer
        let pinned_banner = self.pinned_target.clone().map(|tgt| {
            let weak_jump = weak.clone();
            let weak_clear = weak.clone();
            let tgt_for_jump = tgt.clone();
            div()
                .px_3()
                .py_2()
                .rounded(px(8.))
                .border_1()
                .border_color(c.accent_border)
                .bg(c.accent_bg)
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c.ink)
                                .truncate()
                                .child(format!("🎯 本次修改目标：{}", tgt.detail_line())),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(c.muted)
                                .truncate()
                                .child(format!(
                                    "当前内容快照：{}",
                                    if tgt.excerpt.is_empty() {
                                        "(空白)"
                                    } else {
                                        &tgt.excerpt
                                    }
                                )),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new("btn-jump-pinned-target")
                                .secondary()
                                .with_size(Size::Sm)
                                .label("定位原目标")
                                .on_click(move |_, window, cx| {
                                    if let Some(e) = weak_jump.upgrade() {
                                        let t = tgt_for_jump.clone();
                                        e.update(cx, |this, cx| {
                                            this.jump_to_selection_target(&t, window, cx);
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new("btn-clear-pinned-target")
                                .ghost()
                                .with_size(Size::Sm)
                                .label("移除")
                                .on_click(move |_, _, cx| {
                                    if let Some(e) = weak_clear.upgrade() {
                                        e.update(cx, |this, cx| {
                                            this.pinned_target = None;
                                            cx.notify();
                                        });
                                    }
                                }),
                        ),
                )
        });

        let can_send = self.access_mode.is_writable();
        let is_running = self
            .projected
            .turns
            .last()
            .is_some_and(|t| t.status == TurnStatus::Running)
            || self.prompt_attempts.values().any(|a| a.in_flight);
        let weak_send = weak.clone();
        let weak_abort = weak.clone();
        let weak_key = weak;

        div()
            .size_full()
            .bg(c.canvas_bg)
            .border_r_1()
            .border_color(c.border)
            .flex()
            .flex_col()
            .overflow_hidden()
            // Top Header
            .child(
                div()
                    .px_4()
                    .py(px(10.))
                    .border_b_1()
                    .border_color(c.border)
                    .bg(c.header_bg)
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c.ink)
                                    .truncate()
                                    .child(header_title),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(c.muted)
                                    .truncate()
                                    .child(header_sub),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(self.mode_badge.clone())
                            .child(self.phase_badge.clone())
                            .child(self.chat_actions_menu.clone()),
                    ),
            )
            // Optional HTTP Server pill
            .when_some(self.active_http_server_url.clone(), |el, srv_url| {
                let weak_stop = cx.weak_entity();
                let weak_win = cx.weak_entity();
                el.child(
                    div()
                        .px_4()
                        .py_2()
                        .border_b_1()
                        .border_color(c.border)
                        .bg(c.surface_subtle)
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(c.ink)
                                .truncate()
                                .child(format!("📡 实时订阅服务运行中: {srv_url}")),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Button::new("btn-spawn-sub-win")
                                        .secondary()
                                        .with_size(Size::Sm)
                                        .label("打开第二窗口观察")
                                        .on_click(move |_, _, cx| {
                                            if let Some(e) = weak_win.upgrade() {
                                                e.update(cx, |this, cx| {
                                                    this.open_subscriber_second_window(cx);
                                                });
                                            }
                                        }),
                                )
                                .child(
                                    Button::new("btn-stop-srv-inline")
                                        .danger()
                                        .with_size(Size::Sm)
                                        .label("断开服务")
                                        .on_click(move |_, _, cx| {
                                            if let Some(e) = weak_stop.upgrade() {
                                                e.update(cx, |this, _| {
                                                    this.bridge.send(KarCommand::StopHttpServer);
                                                });
                                            }
                                        }),
                                ),
                        ),
                )
            })
            // Contextual KUI Alert
            .when(self.show_alert, |el| {
                el.child(
                    div()
                        .px_4()
                        .pt_3()
                        .child(self.status_alert.clone()),
                )
            })
            // Turn Timeline Scroll Area
            .child(
                div()
                    .id("kar-timeline-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_4()
                    .child(timeline_body),
            )
            // Pending Permission Requests (when ApprovalMode::Host receives a runtime request)
            .when(
                !self.projected.pending_requests.is_empty() && self.access_mode.is_writable(),
                |el| {
                    let mut req_list = div()
                        .px_4()
                        .py_2()
                        .border_t_1()
                        .border_color(c.border)
                        .bg(c.accent_bg)
                        .flex()
                        .flex_col()
                        .gap_2();
                    for (rix, req) in self.projected.pending_requests.iter().enumerate() {
                        let req_allow = req.clone();
                        let req_deny = req.clone();
                        let allow_choice = req
                            .controls()
                            .decisions
                            .first()
                            .copied()
                            .unwrap_or(kar::PermissionDecision::AllowOnce);
                        let w_allow = cx.weak_entity();
                        let w_deny = cx.weak_entity();
                        req_list = req_list.child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(12.))
                                        .text_color(c.ink)
                                        .truncate()
                                        .child(format!("🔐 工具权限确认 ({}): {}", req.method, req.body)),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "perm-allow-{rix}"
                                            )))
                                            .primary()
                                            .with_size(Size::Sm)
                                            .label("允许执行")
                                            .on_click(move |_, _, cx| {
                                                if let Some(e) = w_allow.upgrade() {
                                                    let r = req_allow.clone();
                                                    e.update(cx, |this, _| {
                                                        this.bridge.send(
                                                            KarCommand::DecidePermission {
                                                                request: r,
                                                                decision: allow_choice,
                                                            },
                                                        );
                                                    });
                                                }
                                            }),
                                        )
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "perm-deny-{rix}"
                                            )))
                                            .danger()
                                            .with_size(Size::Sm)
                                            .label("拒绝")
                                            .on_click(move |_, _, cx| {
                                                if let Some(e) = w_deny.upgrade() {
                                                    let r = req_deny.clone();
                                                    e.update(cx, |this, _| {
                                                        this.bridge.send(
                                                            KarCommand::DecidePermission {
                                                                request: r,
                                                                decision:
                                                                    kar::PermissionDecision::Deny,
                                                            },
                                                        );
                                                    });
                                                }
                                            }),
                                        ),
                                ),
                        );
                    }
                    el.child(req_list)
                },
            )
            // Bottom Composer
            .child(
                div()
                    .p_3()
                    .border_t_1()
                    .border_color(c.border)
                    .bg(c.header_bg)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .on_key_down(move |ev, window, cx| {
                        let cmd_enter = (ev.keystroke.modifiers.platform
                            || ev.keystroke.modifiers.control)
                            && ev.keystroke.key == "enter";
                        if cmd_enter && let Some(e) = weak_key.upgrade() {
                            e.update(cx, |this, cx| this.submit_composer_prompt(window, cx));
                        }
                    })
                    .when_some(pinned_banner, |el, b| el.child(b))
                    .child(self.composer_input.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(c.muted)
                                    .child(if can_send {
                                        "每次发送自动生成幂等 Request UUID，失败/中断重试沿用同一 UUID"
                                    } else {
                                        "🔒 当前处于只读观察模式，发送入口已锁定"
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .when(is_running && can_send, |el| {
                                        el.child(
                                            Button::new("btn-composer-abort")
                                                .danger()
                                                .with_size(Size::Sm)
                                                .label("中止回合")
                                                .on_click(move |_, _, cx| {
                                                    if let Some(e) = weak_abort.upgrade() {
                                                        e.update(cx, |this, _| {
                                                            this.bridge.send(KarCommand::AbortTurn);
                                                        });
                                                    }
                                                }),
                                        )
                                    })
                                    .child(
                                        Button::new("btn-composer-send")
                                            .primary()
                                            .with_size(Size::Sm)
                                            .disabled(!can_send)
                                            .loading(is_running && can_send)
                                            .label(if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
                                                "发送演示消息"
                                            } else {
                                                "发送 (Cmd+Enter)"
                                            })
                                            .on_click(move |_, window, cx| {
                                                if let Some(e) = weak_send.upgrade() {
                                                    e.update(cx, |this, cx| {
                                                        this.submit_composer_prompt(window, cx);
                                                    });
                                                }
                                            }),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_empty_chat_state(&self, c: &UiColors, cx: &mut Context<Self>) -> AnyElement {
        let weak_new = cx.weak_entity();
        let weak_demo = cx.weak_entity();
        let has_controller = matches!(self.access_mode, ChatAccessMode::WriteController { .. });

        div()
            .w_full()
            .p_5()
            .rounded(px(10.))
            .border_1()
            .border_color(c.border)
            .bg(c.card_bg)
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c.ink)
                    .child(if has_controller {
                        "新会话已就绪（空聊天状态）"
                    } else {
                        "欢迎使用 Kar 原生桌面工作台"
                    }),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(c.muted)
                    .child(if has_controller {
                        "当前聊天已绑定真实写入控制器，尚未发送任何回合。可直接在下方输入框发送消息，或在右侧选中文档内容后点击「让 AI 修改选区」。"
                    } else {
                        "左侧使用真实 Kar Rust SDK (ChatStore) 管理持久化聊天；中间按回合展示消息、工具卡片（按原调用 ID 原地更新）与文件变更；右侧嵌入真实 Kantor GPUI 编辑器（Word / Excel / PPT）。"
                    }),
            )
            .when(!has_controller && !self.access_mode.is_http_read_only(), |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Button::new("empty-create-chat")
                                .primary()
                                .with_size(Size::Sm)
                                .label(format!("用 {} 新建真实聊天", self.selected_runtime))
                                .on_click(move |_, _, cx| {
                                    if let Some(e) = weak_new.upgrade() {
                                        e.update(cx, |this, cx| this.create_new_real_chat(cx));
                                    }
                                }),
                        )
                        .child(
                            Button::new("empty-enter-demo")
                                .secondary()
                                .with_size(Size::Sm)
                                .label("查看独立演示样例")
                                .on_click(move |_, _, cx| {
                                    if let Some(e) = weak_demo.upgrade() {
                                        e.update(cx, |this, _| {
                                            this.bridge.send(KarCommand::EnterDemoShowcase);
                                        });
                                    }
                                }),
                        ),
                )
            })
            .into_any_element()
    }

    fn render_turn_card(
        &self,
        turn_number: usize,
        turn: &ChatTurn,
        c: &UiColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let turn_id = turn.id;
        let short_uuid = turn_id.to_string()[..8].to_owned();
        let (st_text, st_color) = match turn.status {
            TurnStatus::Running => ("运行中", c.accent_ink),
            TurnStatus::Completed => ("已完成", c.ok),
            TurnStatus::Interrupted => ("已中断", c.warn),
            TurnStatus::Failed => ("失败", c.danger),
            TurnStatus::Aborted => ("已中止", c.warn),
        };

        let first_user_msg = turn
            .items
            .iter()
            .find_map(|i| match &i.item {
                Item::Message {
                    role: Role::User,
                    text,
                    ..
                } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default();

        let can_retry = matches!(
            turn.status,
            TurnStatus::Failed | TurnStatus::Interrupted | TurnStatus::Aborted
        ) && matches!(self.access_mode, ChatAccessMode::WriteController { .. });
        let weak_retry = cx.weak_entity();
        let retry_text = first_user_msg;

        // Linked Office Target Chip (persists even after switching tabs!)
        let target_chip = self.turn_targets.get(&turn_id).cloned().map(|tgt| {
            let weak_jump = cx.weak_entity();
            let badge_label = format!("🎯 原请求目标：{}（点击定位）", tgt.detail_line());
            div()
                .id(SharedString::from(format!("turn-tgt-{turn_id}")))
                .px_2()
                .py_1()
                .rounded(px(6.))
                .border_1()
                .border_color(c.accent_border)
                .bg(c.accent_bg)
                .cursor_pointer()
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c.ink)
                .on_click(move |_, window, cx| {
                    if let Some(e) = weak_jump.upgrade() {
                        let t = tgt.clone();
                        e.update(cx, |this, cx| this.jump_to_selection_target(&t, window, cx));
                    }
                })
                .child(badge_label)
        });

        // Items inside the turn
        let mut items_col = div().flex().flex_col().gap_2();
        let mut tool_file_changes: Vec<(String, &'static str)> = Vec::new();

        for (item_ix, tl_item) in turn.items.iter().enumerate() {
            match &tl_item.item {
                Item::Message {
                    role: Role::User,
                    text,
                    ..
                } => {
                    items_col = items_col.child(
                        div()
                            .p_3()
                            .rounded(px(8.))
                            .bg(c.user_bubble_bg)
                            .text_size(px(13.))
                            .text_color(c.ink)
                            .child(text.clone()),
                    );
                }
                Item::Message {
                    role: Role::Reasoning,
                    text,
                    complete,
                    ..
                } => {
                    if !text.trim().is_empty() {
                        items_col = items_col.child(
                            div()
                                .px_3()
                                .py_2()
                                .rounded(px(6.))
                                .border_l_2()
                                .border_color(c.border_strong)
                                .bg(c.surface_subtle)
                                .text_size(px(12.))
                                .text_color(c.muted)
                                .child(format!(
                                    "思考{}：{}",
                                    if *complete { "" } else { "中…" },
                                    text.trim()
                                )),
                        );
                    }
                }
                Item::Message {
                    role: Role::Assistant,
                    text,
                    complete,
                    ..
                } => {
                    items_col = items_col.child(
                        div()
                            .px_1()
                            .py_1()
                            .text_size(px(13.5))
                            .line_height(rems(1.45))
                            .text_color(c.ink)
                            .child(if *complete {
                                text.clone()
                            } else {
                                format!("{text} ▍")
                            }),
                    );
                }
                Item::Tool { id, activity } => {
                    if let Some(res) = &activity.result {
                        for fc in &res.file_changes {
                            let kind_tag = match fc.kind {
                                ToolChangeKind::Added => "新增",
                                ToolChangeKind::Modified => "修改",
                                ToolChangeKind::Deleted => "删除",
                                ToolChangeKind::Unknown => "变更",
                            };
                            tool_file_changes.push((fc.path.clone(), kind_tag));
                        }
                    }
                    let key = (turn_id, id.clone());
                    if let Some(inspector) = self.tool_inspectors.get(&key) {
                        items_col = items_col.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .text_color(c.muted)
                                        .child(format!("工具调用 ID: {id}")),
                                )
                                .child(inspector.clone()),
                        );
                    }
                }
                Item::Compaction { report } => {
                    items_col = items_col.child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded(px(6.))
                            .bg(c.surface_subtle)
                            .text_size(px(11.5))
                            .text_color(c.muted)
                            .child(format!("上下文压缩边界：{:?}", report.state)),
                    );
                }
                Item::Diff { diff, .. } => {
                    let preview: String = diff.lines().take(12).collect::<Vec<_>>().join("\n");
                    items_col = items_col.child(
                        div()
                            .id(SharedString::from(format!("diff-{turn_id}-{item_ix}")))
                            .p_2()
                            .rounded(px(6.))
                            .bg(c.surface_subtle)
                            .text_size(px(11.5))
                            .text_color(c.muted)
                            .child(format!("补丁摘要:\n{preview}")),
                    );
                }
            }
        }

        // Merge checkpoint file changes + tool-reported file changes
        let mut all_files: Vec<(String, &'static str)> = Vec::new();
        if let Some(cp) = &turn.checkpoint {
            for fc in &cp.files {
                let kind_tag = match fc.kind {
                    CheckpointChangeKind::Added => "新增",
                    CheckpointChangeKind::Modified => "修改",
                    CheckpointChangeKind::Deleted => "删除",
                };
                all_files.push((fc.path.display().to_string(), kind_tag));
            }
        }
        for tf in tool_file_changes {
            if !all_files.iter().any(|(p, _)| p == &tf.0) {
                all_files.push(tf);
            }
        }

        let ws_root = self
            .projected
            .info
            .as_ref()
            .map(|i| i.workspace.clone())
            .unwrap_or_else(|| self.config.workspace_dir.clone());

        div()
            .w_full()
            .p_3()
            .rounded(px(10.))
            .border_1()
            .border_color(c.border)
            .bg(c.card_bg)
            .flex()
            .flex_col()
            .gap_2()
            // Turn Header
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c.ink)
                                    .child(format!("回合 #{turn_number}")),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(c.muted)
                                    .child(format!("Request UUID: {short_uuid}")),
                            )
                            .when(self.projected.is_demo, |el| {
                                el.child(
                                    div()
                                        .px_1()
                                        .rounded(px(4.))
                                        .bg(c.accent_bg)
                                        .text_size(px(10.5))
                                        .text_color(c.warn)
                                        .child("演示数据"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(st_color)
                                    .child(st_text),
                            )
                            .when(can_retry, |el| {
                                el.child(
                                    Button::new(SharedString::from(format!("retry-{turn_id}")))
                                        .secondary()
                                        .with_size(Size::Sm)
                                        .label(format!("重试 (复用 {short_uuid})"))
                                        .on_click(move |_, _, cx| {
                                            if let Some(e) = weak_retry.upgrade() {
                                                let txt = retry_text.clone();
                                                e.update(cx, |this, cx| {
                                                    this.retry_turn_with_same_uuid(turn_id, txt, cx);
                                                });
                                            }
                                        }),
                                )
                            }),
                    ),
            )
            .when_some(target_chip, |el, chip| el.child(chip))
            .child(items_col)
            .when_some(turn.failure.as_ref(), |el, fail| {
                el.child(
                    div()
                        .p_2()
                        .rounded(px(6.))
                        .bg(c.surface_subtle)
                        .text_size(px(12.))
                        .text_color(c.danger)
                        .child(format!("回合失败详情：{:?}", fail)),
                )
            })
            // Recorded File Changes Footer
            .when(!all_files.is_empty(), |el| {
                let mut files_row = div().flex().flex_wrap().gap_2();
                for (rel_path, kind_tag) in all_files {
                    let full = ws_root.join(&rel_path);
                    let is_office = OfficeKind::for_path(Path::new(&rel_path)).is_some();
                    let weak_open_fc = cx.weak_entity();
                    files_row = files_row.child(
                        div()
                            .id(SharedString::from(format!("fc-{turn_id}-{rel_path}")))
                            .px_2()
                            .py(px(3.))
                            .rounded(px(5.))
                            .border_1()
                            .border_color(c.border)
                            .bg(c.surface_subtle)
                            .when(is_office && full.is_file(), |d| {
                                d.cursor_pointer().on_click(move |_, window, cx| {
                                    if let Some(e) = weak_open_fc.upgrade() {
                                        let p = full.clone();
                                        e.update(cx, |this, cx| {
                                            this.open_office_file_tab(p, false, None, window, cx);
                                        });
                                    }
                                })
                            })
                            .text_size(px(11.))
                            .text_color(c.ink)
                            .child(format!("[{kind_tag}] {rel_path}")),
                    );
                }
                el.child(
                    div()
                        .pt_2()
                        .border_t_1()
                        .border_color(c.border)
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c.muted)
                                .child("已记录的文件变更："),
                        )
                        .child(files_row),
                )
            })
    }

    fn render_office_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = self.palette();
        let weak = cx.weak_entity();

        // Tab strip
        let mut tab_strip = div().flex().items_center().gap_1().overflow_x_hidden();
        for (i, tab) in self.office_tabs.iter().enumerate() {
            let active = self.active_office_tab == Some(i);
            let label = format!(
                "{}{} {}",
                if tab.is_dirty(cx) { "● " } else { "" },
                tab.kind.short_tag(),
                tab.doc_name(cx)
            );
            let weak_sel = weak.clone();
            let weak_close = weak.clone();
            tab_strip = tab_strip.child(
                div()
                    .id(SharedString::from(format!("office-tab-{}", tab.id)))
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(if active { c.border_strong } else { c.border })
                    .bg(if active { c.card_bg } else { c.surface_subtle })
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap_1()
                    .on_click(move |_, _, cx| {
                        if let Some(e) = weak_sel.upgrade() {
                            e.update(cx, |this, cx| {
                                this.active_office_tab = Some(i);
                                if let Some(sc_str) = this.office_tabs.get(i).and_then(|t| t.demo_scenario_key) {
                                    this.coedit_panel_open = true;
                                    this.active_coedit_scenario = match sc_str {
                                        "excel_same_cell" => ScenarioKey::ExcelSameCellConflict,
                                        "ppt_move_text" => ScenarioKey::PptMoveAndText,
                                        _ => ScenarioKey::WordSameParagraph,
                                    };
                                }
                                cx.notify();
                            });
                        }
                    })
                    .child(
                        div()
                            .max_w(px(165.))
                            .text_size(px(12.))
                            .font_weight(if active {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(c.ink)
                            .truncate()
                            .child(label),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("office-tab-close-{}", tab.id)))
                            .px_1()
                            .text_size(px(12.))
                            .text_color(c.muted)
                            .on_click(move |_, _, cx| {
                                if let Some(e) = weak_close.upgrade() {
                                    e.update(cx, |this, cx| {
                                        if i < this.office_tabs.len() {
                                            this.office_tabs.remove(i);
                                            this.active_office_tab =
                                                this.office_tabs.len().checked_sub(1);
                                            cx.notify();
                                        }
                                    });
                                }
                            })
                            .child("×"),
                    ),
            );
        }

        let active_tab = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .cloned();

        // Populate quick edit dialog fields softly if closed so opening it shows live selection!
        if !self.quick_edit_dialog.read(cx).is_open() && active_tab.is_some() {
            self.populate_quick_edit_dialog(window, cx);
        }

        let content = match active_tab {
            None => self.render_empty_office_state(&c, cx),
            Some(tab) => {
                let sel = tab.capture_selection(cx);
                let weak_ai = weak.clone();
                let weak_save = weak.clone();
                let weak_roundtrip = weak.clone();
                let weak_coedit_toggle = weak.clone();
                let is_demo = tab.is_demo;

                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    // Selection & Action Bar
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .border_b_1()
                            .border_color(c.border)
                            .bg(c.surface_subtle)
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .overflow_hidden()
                                            .text_size(px(12.))
                                            .text_color(c.ink)
                                            .truncate()
                                            .child(format!(
                                                "当前选区：{} · {}",
                                                sel.locator.target_label(),
                                                if sel.excerpt.is_empty() {
                                                    "(空白)"
                                                } else {
                                                    &sel.excerpt
                                                }
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(c.muted)
                                            .child(format_rev(sel.base_revision.unwrap_or(0))),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Button::new("btn-ask-ai-selection")
                                            .primary()
                                            .with_size(Size::Sm)
                                            .label("✨ 让 AI 修改选区")
                                            .on_click(move |_, window, cx| {
                                                if let Some(e) = weak_ai.upgrade() {
                                                    e.update(cx, |this, cx| {
                                                        this.bind_active_selection_to_chat(
                                                            window, cx,
                                                        );
                                                    });
                                                }
                                            }),
                                    )
                                    .child(self.quick_edit_dialog.clone())
                                    .when(!is_demo, |el| {
                                        el.child(
                                            Button::new("btn-office-save")
                                                .secondary()
                                                .with_size(Size::Sm)
                                                .label("保存")
                                                .on_click(move |_, window, cx| {
                                                    if let Some(e) = weak_save.upgrade() {
                                                        e.update(cx, |this, cx| {
                                                            if let Some(i) = this.active_office_tab
                                                                && let Some(t) =
                                                                    this.office_tabs.get(i).cloned()
                                                            {
                                                                match t.save_in_place(window, cx) {
                                                                    Ok(msg) => {
                                                                        this.office_notice =
                                                                            Some(msg)
                                                                    }
                                                                    Err(err) => {
                                                                        this.office_notice = Some(
                                                                            format!("{err:#}"),
                                                                        )
                                                                    }
                                                                }
                                                                cx.notify();
                                                            }
                                                        });
                                                    }
                                                }),
                                        )
                                        .child(
                                            Button::new("btn-office-roundtrip")
                                                .secondary()
                                                .with_size(Size::Sm)
                                                .label("保存副本→关闭→重开验证")
                                                .on_click(move |_, window, cx| {
                                                    if let Some(e) = weak_roundtrip.upgrade() {
                                                        e.update(cx, |this, cx| {
                                                            this.save_close_and_reopen_active_tab(
                                                                window, cx,
                                                            );
                                                        });
                                                    }
                                                }),
                                        )
                                    })
                                    .when(is_demo, |el| {
                                        el.child(
                                            Button::new("btn-toggle-coedit-panel")
                                                .secondary()
                                                .with_size(Size::Sm)
                                                .label(if self.coedit_panel_open {
                                                    "收起双写裁决面板"
                                                } else {
                                                    "展开双写裁决面板"
                                                })
                                                .on_click(move |_, _, cx| {
                                                    if let Some(e) = weak_coedit_toggle.upgrade() {
                                                        e.update(cx, |this, cx| {
                                                            this.coedit_panel_open =
                                                                !this.coedit_panel_open;
                                                            cx.notify();
                                                        });
                                                    }
                                                }),
                                        )
                                    }),
                            )
                            .when_some(self.office_notice.clone(), |el, note| {
                                el.child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(c.ok)
                                        .truncate()
                                        .child(note),
                                )
                            }),
                    )
                    // Co-Editing Demo Panel (strictly isolated demo data)
                    .when(self.coedit_panel_open, |el| {
                        el.child(self.render_coedit_panel(&c, cx))
                    })
                    // Real Kantor GPUI Editor View (`DocsView` / `SheetsView` / `SlidesView`)
                    .child(div().flex_1().overflow_hidden().child(tab.any_view()))
                    .into_any_element()
            }
        };

        div()
            .size_full()
            .bg(c.canvas_bg)
            .flex()
            .flex_col()
            .overflow_hidden()
            // Office Header
            .child(
                div()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(c.border)
                    .bg(c.header_bg)
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(div().flex_1().overflow_hidden().child(tab_strip))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(self.open_path_dialog.clone())
                            .child(self.office_menu.clone()),
                    ),
            )
            .child(div().flex_1().overflow_hidden().child(content))
            .into_any_element()
    }

    fn render_empty_office_state(&self, c: &UiColors, cx: &mut Context<Self>) -> AnyElement {
        let w_docx = cx.weak_entity();
        let w_xlsx = cx.weak_entity();
        let w_pptx = cx.weak_entity();
        let w_coedit = cx.weak_entity();

        div()
            .size_full()
            .p_6()
            .flex()
            .flex_col()
            .items_start()
            .justify_center()
            .gap_4()
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c.ink)
                    .child("Kantor 原生 GPUI Office 工作区"),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(c.muted)
                    .child("在同一原生窗口内直接编辑 .docx、.xlsx、.pptx 副本，支持段落修改、公式实时重算、幻灯片对象移动与文字编辑，并可一键将选区关联至左侧 Kar 聊天。"),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("open-sample-docx")
                            .primary()
                            .with_size(Size::Sm)
                            .label("打开 Word 样本副本 (.docx)")
                            .on_click(move |_, window, cx| {
                                if let Some(e) = w_docx.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.open_working_sample("sample_minutes.docx", window, cx);
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("open-sample-xlsx")
                            .secondary()
                            .with_size(Size::Sm)
                            .label("打开 Excel 样本副本 (.xlsx)")
                            .on_click(move |_, window, cx| {
                                if let Some(e) = w_xlsx.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.open_working_sample(
                                            "sample_formulas.xlsx",
                                            window,
                                            cx,
                                        );
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("open-sample-pptx")
                            .secondary()
                            .with_size(Size::Sm)
                            .label("打开 PPT 样本副本 (.pptx)")
                            .on_click(move |_, window, cx| {
                                if let Some(e) = w_pptx.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.open_working_sample("sample_deck.pptx", window, cx);
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("open-coedit-empty")
                            .secondary()
                            .with_size(Size::Sm)
                            .label("启动人机双写演示 (演示数据)")
                            .on_click(move |_, window, cx| {
                                if let Some(e) = w_coedit.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.enter_coedit_demo_workshop(window, cx);
                                    });
                                }
                            }),
                    ),
            )
            .into_any_element()
    }

    fn render_coedit_panel(&mut self, c: &UiColors, cx: &mut Context<Self>) -> AnyElement {
        let sc = self.active_coedit_scenario;
        if let Some(live) = self.read_live_coedit_state(sc, cx) {
            let _ = self.coedit_engine.sync_live_state(&live);
        }
        let Some(prop): Option<CoEditProposal> = self
            .coedit_engine
            .proposals()
            .iter()
            .find(|p| p.scenario == sc)
            .cloned()
        else {
            return div().into_any_element();
        };

        let weak = cx.weak_entity();
        let w_s1 = weak.clone();
        let w_s2 = weak.clone();
        let w_s3 = weak.clone();
        let w_reset = weak.clone();

        // Scenario-specific simulation buttons (complementing live editing directly in Kantor canvas!)
        let sim_buttons = match sc {
            ScenarioKey::WordSameParagraph => {
                let w1 = weak.clone();
                let w2 = weak.clone();
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("sim-word-date")
                            .secondary()
                            .with_size(Size::Sm)
                            .label("① 人工修改同段日期 → 2026-12-01")
                            .on_click(move |_, window, cx| {
                                if let Some(e) = w1.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.trigger_coedit_human_step(
                                            "word_human_date",
                                            window,
                                            cx,
                                        );
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("sim-word-mid-change")
                            .ghost()
                            .with_size(Size::Sm)
                            .label("② 模拟解决期间再次改动段落（触发新修订确认）")
                            .on_click(move |_, window, cx| {
                                if let Some(e) = w2.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.trigger_coedit_human_step(
                                            "word_mid_conflict_edit",
                                            window,
                                            cx,
                                        );
                                    });
                                }
                            }),
                    )
            }
            ScenarioKey::ExcelSameCellConflict => {
                let w1 = weak.clone();
                let w2 = weak.clone();
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("sim-excel-b3")
                            .secondary()
                            .with_size(Size::Sm)
                            .label("① 人工改 B3 → 135000（与 AI 提议 148000 冲突）")
                            .on_click(move |_, window, cx| {
                                if let Some(e) = w1.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.trigger_coedit_human_step(
                                            "excel_human_b3",
                                            window,
                                            cx,
                                        );
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("sim-excel-mid-change")
                            .ghost()
                            .with_size(Size::Sm)
                            .label("② 模拟冲突解决期间再次改 B3 → 139000（触发重确认）")
                            .on_click(move |_, window, cx| {
                                if let Some(e) = w2.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.trigger_coedit_human_step(
                                            "excel_mid_conflict_edit",
                                            window,
                                            cx,
                                        );
                                    });
                                }
                            }),
                    )
            }
            ScenarioKey::PptMoveAndText => {
                let w1 = weak.clone();
                let w2 = weak.clone();
                let w3 = weak.clone();
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("sim-ppt-move")
                            .secondary()
                            .with_size(Size::Sm)
                            .label("①A 人工移动文本框 → (6.50cm, 4.80cm)")
                            .on_click(move |_, window, cx| {
                                if let Some(e) = w1.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.trigger_coedit_human_step(
                                            "ppt_human_move",
                                            window,
                                            cx,
                                        );
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("sim-ppt-delete")
                            .danger()
                            .with_size(Size::Sm)
                            .label("①B 人工删除文本框（触发目标冲突）")
                            .on_click(move |_, window, cx| {
                                if let Some(e) = w2.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.trigger_coedit_human_step(
                                            "ppt_human_delete",
                                            window,
                                            cx,
                                        );
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("sim-ppt-mid-move")
                            .ghost()
                            .with_size(Size::Sm)
                            .label("② 解决期间再次移动（触发新修订重确认）")
                            .on_click(move |_, window, cx| {
                                if let Some(e) = w3.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.trigger_coedit_human_step(
                                            "ppt_mid_conflict_move",
                                            window,
                                            cx,
                                        );
                                    });
                                }
                            }),
                    )
            }
        };

        // Decision buttons depending on `prop.status`
        let op_id = prop.operation_id;
        let decision_row = match &prop.status {
            CoEditStatus::StaleNeedsReconfirm {
                seen_revision,
                latest_revision,
            } => {
                let w_ack = weak.clone();
                let latest = *latest_revision;
                div()
                    .p_2()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(c.warn)
                    .bg(c.accent_bg)
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_2()
                    .child(
                        div()
                            .w_full()
                            .text_size(px(12.))
                            .text_color(c.warn)
                            .child(format!(
                                "⚠️ 冲突解决期间文档已再次变化（已读 {} → 最新 {}）。请核对最新「当前值」后重新确认！",
                                format_rev(*seen_revision),
                                format_rev(*latest_revision),
                            )),
                    )
                    .child(
                        Button::new("btn-coedit-reconfirm")
                            .primary()
                            .with_size(Size::Sm)
                            .label(format!("确认新修订 {}", format_rev(*latest_revision)))
                            .on_click(move |_, _, cx| {
                                if let Some(e) = w_ack.upgrade() {
                                    e.update(cx, |this, cx| {
                                        let _ = this
                                            .coedit_engine
                                            .acknowledge_latest_revision(op_id, latest);
                                        this.coedit_feedback = Some(format!(
                                            "已确认最新修订 {}，现在可提交最终裁决选择。",
                                            format_rev(latest)
                                        ));
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .into_any_element()
            }
            CoEditStatus::TargetDeletedConflict => {
                let w_dismiss = weak;
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c.danger)
                            .child("❌ 目标对象已被删除，AI 提议无法直接落盘到已删除对象"),
                    )
                    .child(
                        Button::new("btn-coedit-dismiss-deleted")
                            .danger()
                            .with_size(Size::Sm)
                            .label("确认目标冲突并放弃提议")
                            .on_click(move |_, window, cx| {
                                if let Some(e) = w_dismiss.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.submit_coedit_choice(
                                            op_id,
                                            UserResolutionChoice::DismissDeletedTarget,
                                            window,
                                            cx,
                                        );
                                    });
                                }
                            }),
                    )
                    .into_any_element()
            }
            CoEditStatus::Resolved { summary } => div()
                .p_2()
                .rounded(px(6.))
                .bg(c.accent_bg)
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c.ok)
                .child(format!("✅ 处理结果：{summary}"))
                .into_any_element(),
            CoEditStatus::ReadyDualKeep | CoEditStatus::ConflictNeedsChoice => {
                let w_dual = weak.clone();
                let w_keep = weak.clone();
                let w_adopt = weak.clone();
                let w_manual = weak;
                let has_dual = prop.merged_preview.is_some();
                let show_manual = sc == ScenarioKey::ExcelSameCellConflict
                    || !has_dual;

                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .when(show_manual, |el| el.child(self.coedit_manual_input.clone()))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .when(has_dual, |el| {
                                el.child(
                                    Button::new("btn-coedit-dual-keep")
                                        .primary()
                                        .with_size(Size::Sm)
                                        .label("应用双项保留结果")
                                        .on_click(move |_, window, cx| {
                                            if let Some(e) = w_dual.upgrade() {
                                                e.update(cx, |this, cx| {
                                                    this.submit_coedit_choice(
                                                        op_id,
                                                        UserResolutionChoice::ApplyDualKeep,
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            }
                                        }),
                                )
                            })
                            .child(
                                Button::new("btn-coedit-keep-current")
                                    .secondary()
                                    .with_size(Size::Sm)
                                    .label("保留当前值")
                                    .on_click(move |_, window, cx| {
                                        if let Some(e) = w_keep.upgrade() {
                                            e.update(cx, |this, cx| {
                                                this.submit_coedit_choice(
                                                    op_id,
                                                    UserResolutionChoice::KeepCurrent,
                                                    window,
                                                    cx,
                                                );
                                            });
                                        }
                                    }),
                            )
                            .when(sc != ScenarioKey::PptMoveAndText, |el| {
                                el.child(
                                    Button::new("btn-coedit-adopt-ai")
                                        .secondary()
                                        .with_size(Size::Sm)
                                        .label("采用 AI 提议")
                                        .on_click(move |_, window, cx| {
                                            if let Some(e) = w_adopt.upgrade() {
                                                e.update(cx, |this, cx| {
                                                    this.submit_coedit_choice(
                                                        op_id,
                                                        UserResolutionChoice::AdoptProposal,
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            }
                                        }),
                                )
                            })
                            .when(show_manual, |el| {
                                el.child(
                                    Button::new("btn-coedit-manual-merge")
                                        .primary()
                                        .with_size(Size::Sm)
                                        .label("提交手工合并")
                                        .on_click(move |_, window, cx| {
                                            if let Some(e) = w_manual.upgrade() {
                                                e.update(cx, |this, cx| {
                                                    let val = this
                                                        .coedit_manual_input
                                                        .read(cx)
                                                        .value(cx)
                                                        .to_owned();
                                                    this.submit_coedit_choice(
                                                        op_id,
                                                        UserResolutionChoice::ManualMerge {
                                                            merged_value: val,
                                                        },
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            }
                                        }),
                                )
                            }),
                    )
                    .into_any_element()
            }
        };

        div()
            .id("coedit-panel-scroll")
            .max_h(px(480.))
            .overflow_y_scroll()
            .p_3()
            .border_b_1()
            .border_color(c.border_strong)
            .bg(c.card_bg)
            .flex()
            .flex_col()
            .gap_2()
            // Header with unmistakable Demo Badge
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .px_2()
                                    .py(px(2.))
                                    .rounded(px(4.))
                                    .bg(c.accent_bg)
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(c.warn)
                                    .child(prop.data_source_badge.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c.ink)
                                    .child(self.coedit_engine.backend_label()),
                            ),
                    )
                    .child(
                        Button::new("btn-coedit-reset-all")
                            .ghost()
                            .with_size(Size::Sm)
                            .label("重置三组演示")
                            .on_click(move |_, window, cx| {
                                if let Some(e) = w_reset.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.trigger_coedit_human_step("reset_all", window, cx);
                                    });
                                }
                            }),
                    ),
            )
            // Scenario switcher
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        style_tab_btn(
                            Button::new("tab-sc-word")
                                .with_size(Size::Sm)
                                .label("1. Word 同段双写"),
                            sc == ScenarioKey::WordSameParagraph,
                        )
                        .on_click(move |_, window, cx| {
                            if let Some(e) = w_s1.upgrade() {
                                e.update(cx, |this, cx| {
                                    this.switch_coedit_scenario(
                                        ScenarioKey::WordSameParagraph,
                                        window,
                                        cx,
                                    );
                                });
                            }
                        }),
                    )
                    .child(
                        style_tab_btn(
                            Button::new("tab-sc-excel")
                                .with_size(Size::Sm)
                                .label("2. Excel 同格冲突"),
                            sc == ScenarioKey::ExcelSameCellConflict,
                        )
                        .on_click(move |_, window, cx| {
                            if let Some(e) = w_s2.upgrade() {
                                e.update(cx, |this, cx| {
                                    this.switch_coedit_scenario(
                                        ScenarioKey::ExcelSameCellConflict,
                                        window,
                                        cx,
                                    );
                                });
                            }
                        }),
                    )
                    .child(
                        style_tab_btn(
                            Button::new("tab-sc-ppt")
                                .with_size(Size::Sm)
                                .label("3. PPT 移动+改字 / 删除冲突"),
                            sc == ScenarioKey::PptMoveAndText,
                        )
                        .on_click(move |_, window, cx| {
                            if let Some(e) = w_s3.upgrade() {
                                e.update(cx, |this, cx| {
                                    this.switch_coedit_scenario(
                                        ScenarioKey::PptMoveAndText,
                                        window,
                                        cx,
                                    );
                                });
                            }
                        }),
                    ),
            )
            .child(sim_buttons)
            // Protocol fields grid
            .child(
                div()
                    .p_2()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(c.border)
                    .bg(c.surface_subtle)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .text_size(px(11.5))
                    .text_color(c.ink)
                    .child(format!(
                        "选区: {}  |  操作 ID: {}  |  基准修订: {}  |  当前修订: {}",
                        prop.selection_desc,
                        &prop.operation_id.to_string()[..13],
                        format_rev(prop.base_revision),
                        format_rev(prop.latest_revision),
                    ))
                    .child(format!("原值 (Base): {}", prop.base_value))
                    .child(format!("当前值 (Current): {}", prop.current_value))
                    .child(format!("AI 提议值 (Proposed): {}", prop.proposed_value))
                    .when_some(prop.merged_preview.clone(), |el, mp| {
                        el.child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c.ok)
                                .child(format!("双项保留预览 (Merged): {mp}")),
                        )
                    })
                    .when_some(prop.conflict_reason.clone(), |el, reason| {
                        el.child(
                            div()
                                .text_color(c.warn)
                                .child(format!("冲突/合并说明: {reason}")),
                        )
                    }),
            )
            .child(decision_row)
            .when_some(self.coedit_feedback.clone(), |el, fb| {
                el.child(
                    div()
                        .text_size(px(11.5))
                        .text_color(c.muted)
                        .child(fb),
                )
            })
            .into_any_element()
    }
}

impl Render for KarWorkbench {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.panels.clone())
    }
}

fn style_tab_btn(btn: Button, active: bool) -> Button {
    if active {
        btn.primary()
    } else {
        btn.secondary()
    }
}

#[derive(Clone, Copy)]
struct UiColors {
    sidebar_bg: Hsla,
    header_bg: Hsla,
    canvas_bg: Hsla,
    card_bg: Hsla,
    surface_subtle: Hsla,
    hover_bg: Hsla,
    selected_bg: Hsla,
    user_bubble_bg: Hsla,
    accent_bg: Hsla,
    accent_border: Hsla,
    accent_ink: Hsla,
    border: Hsla,
    border_strong: Hsla,
    ink: Hsla,
    muted: Hsla,
    ok: Hsla,
    warn: Hsla,
    danger: Hsla,
}

impl UiColors {
    fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self {
                sidebar_bg: rgb(0xf7f7f8).into(),
                header_bg: rgb(0xffffff).into(),
                canvas_bg: rgb(0xfcfcfd).into(),
                card_bg: rgb(0xffffff).into(),
                surface_subtle: rgb(0xf2f2f5).into(),
                hover_bg: rgb(0xededf0).into(),
                selected_bg: rgb(0xe7e7ec).into(),
                user_bubble_bg: rgb(0xf1f3f7).into(),
                accent_bg: rgb(0xf4f6fb).into(),
                accent_border: rgb(0xc9d6ef).into(),
                accent_ink: rgb(0x1d4ed8).into(),
                border: rgb(0xe4e4e7).into(),
                border_strong: rgb(0xa1a1aa).into(),
                ink: rgb(0x111113).into(),
                muted: rgb(0x63636e).into(),
                ok: rgb(0x15803d).into(),
                warn: rgb(0xb45309).into(),
                danger: rgb(0xb91c1c).into(),
            },
            ThemeMode::Dark => Self {
                sidebar_bg: rgb(0x111318).into(),
                header_bg: rgb(0x151820).into(),
                canvas_bg: rgb(0x0d1016).into(),
                card_bg: rgb(0x161a23).into(),
                surface_subtle: rgb(0x1d222e).into(),
                hover_bg: rgb(0x232938).into(),
                selected_bg: rgb(0x283042).into(),
                user_bubble_bg: rgb(0x1e2534).into(),
                accent_bg: rgb(0x1a2438).into(),
                accent_border: rgb(0x334b77).into(),
                accent_ink: rgb(0x60a5fa).into(),
                border: rgb(0x262c3a).into(),
                border_strong: rgb(0x4b5568).into(),
                ink: rgb(0xf1f5f9).into(),
                muted: rgb(0x94a3b8).into(),
                ok: rgb(0x4ade80).into(),
                warn: rgb(0xfbbf24).into(),
                danger: rgb(0xf87171).into(),
            },
        }
    }
}

fn status_short_text(st: Option<TurnStatus>) -> &'static str {
    match st {
        Some(TurnStatus::Running) => "运行中",
        Some(TurnStatus::Completed) => "已完成",
        Some(TurnStatus::Interrupted) => "已中断",
        Some(TurnStatus::Failed) => "失败",
        Some(TurnStatus::Aborted) => "已中止",
        None => "空",
    }
}

fn tool_to_invocation(call_id: &str, tool: &Tool) -> Invocation {
    let name = tool
        .tool_name
        .clone()
        .or_else(|| tool.name.clone())
        .unwrap_or_else(|| "tool".into());
    let action = match (&tool.server, &tool.native_type) {
        (Some(srv), Some(nt)) => format!("{srv} ({nt}) · {call_id}"),
        (Some(srv), None) => format!("{srv} · {call_id}"),
        (None, Some(nt)) => format!("{nt} · {call_id}"),
        (None, None) => call_id.to_owned(),
    };
    let input = tool
        .input
        .as_ref()
        .map(|v| serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()))
        .unwrap_or_else(|| "(无参数)".into());

    let mut output = String::new();
    if let Some(res) = &tool.result {
        if let Some(txt) = res.text() {
            output = txt;
        } else if let Some(code) = res.exit_code {
            output = format!("exit code: {code}");
        }
        if !res.file_changes.is_empty() {
            let files = res
                .file_changes
                .iter()
                .map(|f| f.path.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&format!("[文件变更: {files}]"));
        }
    } else if !tool.preview.is_empty() {
        output.clone_from(&tool.preview);
    }

    let state = if !tool.finished {
        ToolState::Running
    } else {
        match tool.result.as_ref().and_then(|r| r.status.as_ref()) {
            Some(kar::ToolStatus::Failed) => ToolState::Failed,
            Some(kar::ToolStatus::Cancelled) => ToolState::Denied,
            _ => ToolState::Done,
        }
    };

    Invocation {
        name,
        action,
        input,
        output,
        state,
    }
}

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
