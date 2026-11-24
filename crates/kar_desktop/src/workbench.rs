//! Unified single-window GPUI Workbench: Kense Architectural Shell + Tailored KUI Micro-Components
//! + Real Kar SDK (`ChatStore`/`ChatSession`/`HttpChatFeed`/`ChatReplica`) + Native Kantor Office.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui::{prelude::*, *};
use kar::{
    ApprovalMode,
    chat::{ChatTurn, Item, Role, TurnStatus},
    observe::Tool,
};
use kui::{
    ThemeMode,
    components::{
        agent_status::{Step as AgentStep, WorkBadge},
        ai_command::{Command, CommandDesk, CommandEvent},
        ai_spark::{AiSpark, SparkStatus, SparkTone},
        alert::{Alert, AlertAction, Severity},
        breadcrumb::{Breadcrumb, Navigated},
        code_suggestion::{Decision, Hunk, PatchReview},
        confirm_morph::{ConfirmMorph, ConfirmRequest, Intent as ConfirmIntent},
        copy_button::CopyButton,
        dialog::Dialog,
        dropdown_menu::DropdownMenu,
        glass_segmented::{GlassSegmentChanged, GlassSegmentOption, GlassSegmented},
        input::Input,
        pro_chrome::pro_presence,
        provider_status::{
            ProviderDay, ProviderHealth, ProviderIncident, ProviderIncidentUpdate, ProviderStatus,
            ProviderStatusEvent, ProviderStatusItem,
        },
        reasoning_block::{ExplanationNote, NoteState},
        search_field::SearchField,
        theme_switch::{ThemeChange, ThemeSwitch},
        toast::Toast,
        tool_call::{Invocation, InvocationInspector, ToolState},
    },
    primitives::{
        disclosure::SurfaceEvent,
        editor::{EditEvent, Editor, EditorShortcutEvent, InputMode},
        field::FieldEvent,
        glyph::Glyph,
        menu::{MenuAlign, MenuItem, MenuSelection},
        pro_icons::ProIcon,
    },
};
use uuid::Uuid;

use crate::{
    apply_unified_theme,
    coedit::{
        CoEditService, DemoCoEditEngine, DemoEditorMutation, LiveTargetState, ScenarioKey,
        SubmissionOutcome, UserResolutionChoice,
    },
    kar_bridge::{KarBridgeEvent, KarBridgeHandle, KarCommand, ProjectedChatState},
    office_host::{OfficeEditorView, OfficeTab, clean_doc_para_text, materialize_coedit_demo_files},
    studio_window_options,
    types::{
        ChatAccessMode, ChatListEntry, FeedConnectionState, OfficeKind, PendingPromptAttempt,
        ProviderAvailability, ProviderStatusInfo, SelectionLocator, SelectionTarget,
    },
    ui_primitives::StudioPalette,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceLayoutMode {
    /// Focused single-column conversational workspace.
    ChatFocus,
    /// Side-by-side split view: conversational thread + Kantor Office canvas.
    Split,
    /// Expanded Office canvas (~67% width) with compact AI sidekick thread.
    CanvasWide,
}

impl WorkspaceLayoutMode {
    pub fn as_segment_key(self) -> &'static str {
        match self {
            Self::ChatFocus => "chat",
            Self::Split => "split",
            Self::CanvasWide => "wide",
        }
    }

    pub fn from_segment_key(key: &str) -> Option<Self> {
        match key {
            "chat" => Some(Self::ChatFocus),
            "split" => Some(Self::Split),
            "wide" => Some(Self::CanvasWide),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComposerPopover {
    AddContext,
    ApprovalPolicy,
    RuntimePicker,
}

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
    pub focus_handle: FocusHandle,

    // Spatial shell geometry & overlays
    pub sidebar_open: bool,
    pub sidebar_filter: String,
    pub layout_mode: WorkspaceLayoutMode,
    pub split_ratio: f32,
    pub(crate) dragging_split: bool,
    pub(crate) inset_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub composer_popover: Option<ComposerPopover>,
    pub approval_mode: ApprovalMode,
    pub command_palette_open: bool,
    pub provider_sheet_open: bool,
    pub hovered_starter_chip: Option<usize>,

    // Tailored KUI micro-components & structural surfaces
    pub(crate) home_spark: Entity<AiSpark>,
    pub(crate) sidebar_search: Entity<SearchField>,
    pub(crate) sidebar_docs_menu: Entity<DropdownMenu>,
    pub(crate) sidebar_tools_menu: Entity<DropdownMenu>,
    pub(crate) layout_segmented: Entity<GlassSegmented>,
    pub(crate) coedit_segmented: Entity<GlassSegmented>,
    pub(crate) office_breadcrumb: Entity<Breadcrumb>,
    pub(crate) coedit_dismiss_morph: Entity<ConfirmMorph>,
    pub(crate) theme_switch: Entity<ThemeSwitch>,
    pub(crate) command_desk: Entity<CommandDesk>,
    pub(crate) provider_status_view: Entity<ProviderStatus>,
    pub(crate) toast: Entity<Toast>,
    pub(crate) chat_actions_menu: Entity<DropdownMenu>,
    pub(crate) office_menu: Entity<DropdownMenu>,
    pub(crate) status_alert: Entity<Alert>,
    pub(crate) show_alert: bool,
    pub(crate) alert_opens_demo: bool,

    // KUI composer & inline editors + triggerless modal dialogs
    pub(crate) composer_editor: Entity<Editor>,
    pub(crate) http_dialog: Entity<Dialog>,
    pub(crate) http_url_input: Entity<Input>,
    pub(crate) open_path_dialog: Entity<Dialog>,
    pub(crate) open_path_input: Entity<Input>,
    pub(crate) quick_edit_dialog: Entity<Dialog>,
    pub(crate) quick_edit_primary: Entity<Input>,
    pub(crate) quick_edit_secondary: Entity<Input>,
    pub(crate) coedit_manual_editor: Entity<Editor>,

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

    // Keyed KUI rich timeline components (updated in place by turn / call ID)
    pub turn_sparks: BTreeMap<Uuid, Entity<AiSpark>>,
    pub turn_badges: BTreeMap<Uuid, Entity<WorkBadge>>,
    pub tool_inspectors: BTreeMap<(Uuid, String), Entity<InvocationInspector>>,
    pub settled_tool_cards: BTreeSet<(Uuid, String)>,
    pub reasoning_blocks: BTreeMap<(Uuid, usize), Entity<ExplanationNote>>,
    pub diff_reviews: BTreeMap<(Uuid, usize), Entity<PatchReview>>,
    pub copy_buttons: BTreeMap<Uuid, Entity<CopyButton>>,
    pub chat_scroll: ScrollHandle,

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

        let initial_layout = if !config.open_files.is_empty() || config.start_coedit_demo {
            WorkspaceLayoutMode::Split
        } else {
            WorkspaceLayoutMode::ChatFocus
        };

        let initial_scenario = config
            .initial_coedit_scenario
            .unwrap_or(ScenarioKey::WordSameParagraph);

        // 1. KUI AiSpark (living 8-ray monochrome mark on Start Screen)
        let home_spark = cx.new(|_| {
            AiSpark::new("kar-home-spark")
                .with_size(22.)
                .with_tone(SparkTone::Foreground)
                .with_status(SparkStatus::Idle)
                .with_twinkle(true)
        });

        // 2. KUI SearchField (compact 30px live filter bar in Sidebar)
        let sidebar_search = cx.new(|cx| {
            SearchField::new("kar-sidebar-search", "筛选对话与文档", window, cx)
                .with_compact_bar(30.)
                .with_placeholder("筛选对话与文档…", cx)
                .with_trailing_hint("⌘K")
        });
        cx.subscribe(&sidebar_search, |this, _, ev: &FieldEvent, cx| {
            if let FieldEvent::Changed(q) = ev {
                this.sidebar_filter = q.to_string();
                cx.notify();
            }
        })
        .detach();

        // 3. KUI ThemeSwitch (28x28 subtle borderless Sun/Moon morph)
        let theme_switch = cx.new(|_| {
            ThemeSwitch::new("kar-sidebar-theme-switch")
                .with_icon_only(true)
                .with_compact(true)
        });
        cx.subscribe(&theme_switch, |this, _, ev: &ThemeChange, cx| {
            this.theme_mode = ev.next;
            apply_unified_theme(ev.next, cx);
            cx.notify();
        })
        .detach();

        // 4. KUI GlassSegmented — Workspace Layout Switcher (28px compact)
        let layout_segmented = cx.new(|cx| {
            GlassSegmented::new(
                "工作区视图",
                vec![
                    GlassSegmentOption::new("chat", "对话"),
                    GlassSegmentOption::new("split", "分栏"),
                    GlassSegmentOption::new("wide", "画布"),
                ],
                cx,
            )
            .with_compact(true)
            .with_width(164.)
            .with_value(initial_layout.as_segment_key())
        });
        cx.subscribe(&layout_segmented, |this, _, ev: &GlassSegmentChanged, cx| {
            if let Some(next) = WorkspaceLayoutMode::from_segment_key(ev.0.as_ref()) {
                this.layout_mode = next;
                cx.notify();
            }
        })
        .detach();

        // 5. KUI GlassSegmented — Co-Edit Scenario Switcher (28px compact)
        let coedit_segmented = cx.new(|cx| {
            GlassSegmented::new(
                "双写演练场景",
                vec![
                    GlassSegmentOption::new(ScenarioKey::WordSameParagraph.as_str(), "Word 同段"),
                    GlassSegmentOption::new(
                        ScenarioKey::ExcelSameCellConflict.as_str(),
                        "Excel 同格",
                    ),
                    GlassSegmentOption::new(ScenarioKey::PptMoveAndText.as_str(), "PPT 移位"),
                ],
                cx,
            )
            .with_compact(true)
            .with_width(248.)
            .with_value(initial_scenario.as_str())
        });
        cx.subscribe_in(
            &coedit_segmented,
            window,
            |this, _, ev: &GlassSegmentChanged, window, cx| {
                let sc = match ev.0.as_ref() {
                    "excel_same_cell" => ScenarioKey::ExcelSameCellConflict,
                    "ppt_move_text" => ScenarioKey::PptMoveAndText,
                    _ => ScenarioKey::WordSameParagraph,
                };
                this.switch_coedit_scenario(sc, window, cx);
            },
        )
        .detach();

        // 6. KUI Breadcrumb — Live Office Selection Trail (22px compact)
        let office_breadcrumb = cx.new(|_| {
            Breadcrumb::new(vec!["Kantor".into(), "选区".into()]).with_compact(true)
        });
        cx.subscribe_in(
            &office_breadcrumb,
            window,
            |this, _, ev: &Navigated, window, cx| {
                if ev.0 == 1 {
                    this.open_quick_edit_modal(window, cx);
                }
            },
        )
        .detach();

        // 7. KUI ConfirmMorph — Tactile Morphing Confirmation for Conflict Dismissal
        let coedit_dismiss_morph = cx.new(|cx| {
            ConfirmMorph::new(
                "coedit-dismiss-morph",
                "丢弃冲突提议",
                "确认丢弃？",
                window,
                cx,
            )
            .with_compact(true)
            .with_labels("确认", "已丢弃")
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

        // 8. KUI CommandDesk (⌘K) & ProviderStatus & Toast
        let command_desk = cx.new(|cx| {
            CommandDesk::new(
                "kar-command-desk",
                build_default_commands(),
                window,
                cx,
            )
            .with_model("Kense · Kar")
        });

        let provider_status_view = cx.new(|_| {
            ProviderStatus::new("本机 Kar 供应商与凭据健康诊断", Vec::new())
                .footnote("通过真实 Kar SDK 探测本机 CLI 可执行程序与登录凭据状态")
        });

        let toast = cx.new(|_| Toast::new());

        // 9. Compact 28x28 KUI DropdownMenus
        let chat_action_items = vec![
            MenuItem::new("start_http", "开启只读 HTTP/SSE 订阅服务")
                .with_icon(Glyph::Globe)
                .with_detail("127.0.0.1 随机端口"),
            MenuItem::new("open_second_window", "新开第二窗口只读观察")
                .with_icon(Glyph::Eye)
                .with_detail("验证单写入锁"),
            MenuItem::new("stop_http", "断开本会话 HTTP/SSE 端点")
                .with_icon(Glyph::Close)
                .with_detail("模拟断线续收"),
            MenuItem::new("provider_health", "供应商与凭据诊断…")
                .with_icon(Glyph::Settings)
                .with_separator(),
            MenuItem::new("refresh_all", "重新扫描会话与供应商")
                .with_icon(Glyph::Settings),
            MenuItem::new("abort_turn", "中止当前运行回合")
                .with_icon(Glyph::Pause)
                .destructive()
                .with_separator(),
        ];
        let chat_actions_menu = cx.new(|cx| {
            DropdownMenu::new("kar-chat-actions", "会话操作", chat_action_items, cx)
                .with_icon(Glyph::More)
                .icon_only()
                .with_compact(true)
                .with_align(MenuAlign::End)
        });

        let make_office_items = || {
            vec![
                MenuItem::new("sample_docx", "打开 Word 会议纪要样本 (.docx)")
                    .with_icon(Glyph::FileText),
                MenuItem::new("sample_xlsx", "打开 Excel 公式预算表 (.xlsx)")
                    .with_icon(Glyph::FileText),
                MenuItem::new("sample_pptx", "打开 PPT 演示文稿样本 (.pptx)")
                    .with_icon(Glyph::FileText),
                MenuItem::new("open_custom_path", "按本地路径打开文档…")
                    .with_icon(Glyph::Folder)
                    .with_separator(),
                MenuItem::new("blank_docx", "新建空白 Word (.docx)")
                    .with_icon(Glyph::Plus)
                    .with_separator(),
                MenuItem::new("blank_xlsx", "新建空白 Excel (.xlsx)").with_icon(Glyph::Plus),
                MenuItem::new("blank_pptx", "新建空白 PPT (.pptx)").with_icon(Glyph::Plus),
                MenuItem::new("coedit_demo", "人机双写协议演练 (演示数据)")
                    .with_icon(Glyph::Bulb)
                    .with_separator(),
            ]
        };
        let office_menu = cx.new(|cx| {
            DropdownMenu::new("kar-office-menu", "打开或新建文档", make_office_items(), cx)
                .with_icon(Glyph::Plus)
                .icon_only()
                .with_compact(true)
                .with_align(MenuAlign::End)
        });
        let sidebar_docs_menu = cx.new(|cx| {
            DropdownMenu::new(
                "kar-side-docs-menu",
                "打开或新建文档",
                make_office_items(),
                cx,
            )
            .with_icon(Glyph::Plus)
            .icon_only()
            .with_compact(true)
            .with_align(MenuAlign::End)
        });

        let tools_items = vec![
            MenuItem::new("coedit_demo", "人机双写协议演练")
                .with_icon(Glyph::Bulb)
                .with_detail("演示数据"),
            MenuItem::new("demo_chat", "独立演示对话沙盒")
                .with_icon(Glyph::Comment)
                .with_detail("演示数据"),
            MenuItem::new("http_sub", "连接远程 HTTP 只读流…")
                .with_icon(Glyph::Globe)
                .with_separator(),
            MenuItem::new("start_http", "开启当前会话 SSE 广播")
                .with_icon(Glyph::Link),
            MenuItem::new("second_win", "新开第二观察窗口")
                .with_icon(Glyph::Eye),
            MenuItem::new("provider_health", "供应商与凭据诊断…")
                .with_icon(Glyph::Settings)
                .with_separator(),
            MenuItem::new("cmd_palette", "打开全局命令面板")
                .with_icon(Glyph::Search)
                .with_detail("⌘K"),
        ];
        let sidebar_tools_menu = cx.new(|cx| {
            DropdownMenu::new("kar-side-tools-menu", "演练与连接工具", tools_items, cx)
                .with_icon(Glyph::Settings)
                .icon_only()
                .with_compact(true)
                .with_align(MenuAlign::End)
        });

        // 10. KUI Inline Alert
        let status_alert = cx.new(|_| {
            Alert::new(
                "正在探测本机 Kar 供应商环境…",
                "存储与网络任务均在独立后台线程运行。",
                Severity::Info,
            )
            .with_compact_inline(true)
        });

        let composer_editor = cx.new(|cx| {
            let mut ed = Editor::new("kar-composer", InputMode::InlineMulti, window, cx);
            ed.set_placeholder("输入任务开启对话，或点击 + 关联右侧 Office 文档选区…");
            ed.set_command_submit(true);
            ed
        });

        let coedit_manual_editor = cx.new(|cx| {
            let mut ed = Editor::new("kar-coedit-manual", InputMode::Single, window, cx);
            ed.set_placeholder("手工合并值，如 142000");
            ed.set_value("142000", window, cx);
            ed
        });

        let http_url_input = cx.new(|cx| {
            let initial = config
                .subscribe_http
                .clone()
                .unwrap_or_else(|| "http://127.0.0.1:18090".into());
            Input::new("kar-http-url-input", "HTTP 快照与事件流地址", window, cx)
                .with_value(initial, window, cx)
                .with_placeholder("http://127.0.0.1:18090", cx)
                .with_description("连接后通过 HttpChatFeed + ChatReplica 接收快照与 SSE 增量")
        });
        let url_field_for_dialog = http_url_input.clone();
        let http_dialog = cx.new(|cx| {
            Dialog::new(
                "kar-http-dialog",
                "连接 HTTP 订阅",
                "连接只读 HTTP/SSE 会话流",
                cx,
            )
            .without_trigger()
            .with_description(
                "先请求 GET /snapshot 获取历史快照与游标，再通过 GET /events 续收实时事件；界面自动锁定为只读观察模式。",
            )
            .with_body(move |_, _| div().w_full().py_1().child(url_field_for_dialog.clone()))
            .with_cancel("取消")
            .with_confirm("连接订阅")
        });

        let open_path_input = cx.new(|cx| {
            Input::new(
                "kar-open-path-input",
                "本地 Office 文档路径 (.docx / .xlsx / .pptx)",
                window,
                cx,
            )
            .with_placeholder("/path/to/working-copy.docx", cx)
            .with_description("建议使用测试副本，直接在右侧原生 Kantor 编辑器中打开")
        });
        let path_field_for_dialog = open_path_input.clone();
        let open_path_dialog = cx.new(|cx| {
            Dialog::new("kar-open-path-dialog", "打开文档", "打开本地 Office 文档", cx)
                .without_trigger()
                .with_body(move |_, _| div().w_full().py_1().child(path_field_for_dialog.clone()))
                .with_cancel("取消")
                .with_confirm("打开")
        });

        let quick_edit_primary = cx.new(|cx| {
            Input::new(
                "kar-quick-edit-primary",
                "新内容（段落文本 / 单元格值或公式 / 幻灯片文字）",
                window,
                cx,
            )
            .with_placeholder("支持 =SUM(...) 等 Excel 公式", cx)
        });
        let quick_edit_secondary = cx.new(|cx| {
            Input::new(
                "kar-quick-edit-secondary",
                "目标定位（Word 段落号 / Excel 单元格如 B4 / PPT 坐标 x,y cm）",
                window,
                cx,
            )
            .with_placeholder("例如 B4 或 5.20,3.80", cx)
        });
        let qe1 = quick_edit_primary.clone();
        let qe2 = quick_edit_secondary.clone();
        let quick_edit_dialog = cx.new(|cx| {
            Dialog::new(
                "kar-quick-edit-dialog",
                "精确编辑选区",
                "精确修改当前 Kantor 文档选区",
                cx,
            )
            .without_trigger()
            .with_description("除直接在下方画布内交互编辑外，也可精确指定段落文本、单元格公式或对象坐标。")
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
            .with_confirm("应用修改")
        });

        // Subscriptions
        cx.subscribe_in(
            &command_desk,
            window,
            |this, desk, ev: &CommandEvent, window, cx| match ev {
                CommandEvent::Open(cmd_id) => {
                    let id = cmd_id.clone();
                    this.command_palette_open = false;
                    this.execute_command_id(&id, window, cx);
                }
                CommandEvent::Ask { ticket, query } => {
                    let q = query.clone();
                    let t = *ticket;
                    desk.update(cx, |d, cx| {
                        d.receive(
                            t,
                            Ok(format!(
                                "已将「{q}」填入对话输入框，按 ⌘↵ 即可由当前供应商执行。"
                            )),
                            true,
                            cx,
                        );
                    });
                    this.fill_starter_prompt(&q, window, cx);
                    this.command_palette_open = false;
                    cx.notify();
                }
                CommandEvent::Cancel => {
                    this.command_palette_open = false;
                    cx.notify();
                }
            },
        )
        .detach();

        cx.subscribe(&provider_status_view, |this, _, ev: &ProviderStatusEvent, cx| {
            if *ev == ProviderStatusEvent::Refresh {
                this.bridge.send(KarCommand::RefreshStoreAndProviders {
                    store_dir: this.config.chat_dir.clone(),
                    workspace: this.config.workspace_dir.clone(),
                });
                this.notify_toast("正在重新探测本机供应商与凭据…", cx);
            }
        })
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

        cx.subscribe_in(
            &sidebar_docs_menu,
            window,
            |this, _, sel: &MenuSelection, window, cx| {
                this.handle_office_menu_action(sel.0.as_ref(), window, cx);
            },
        )
        .detach();

        cx.subscribe_in(
            &sidebar_tools_menu,
            window,
            |this, _, sel: &MenuSelection, window, cx| match sel.0.as_ref() {
                "coedit_demo" => this.enter_coedit_demo_workshop(window, cx),
                "demo_chat" => this.bridge.send(KarCommand::EnterDemoShowcase),
                "http_sub" => {
                    this.http_dialog
                        .update(cx, |d, cx| d.set_open(true, window, cx));
                }
                "start_http" => this.handle_chat_menu_action("start_http", window, cx),
                "second_win" => this.open_subscriber_second_window(cx),
                "provider_health" => this.toggle_provider_sheet(cx),
                "cmd_palette" => this.toggle_command_palette(window, cx),
                _ => {}
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

        cx.subscribe(&composer_editor, |_, _, ev: &EditEvent, cx| {
            if *ev == EditEvent::Changed {
                cx.notify();
            }
        })
        .detach();

        cx.subscribe_in(
            &composer_editor,
            window,
            |this, _, _: &EditorShortcutEvent, window, cx| {
                this.submit_composer_prompt(window, cx);
            },
        )
        .detach();

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

        let mut this = Self {
            config: config.clone(),
            bridge: bridge.clone(),
            theme_mode,
            focus_handle: cx.focus_handle(),
            sidebar_open: true,
            sidebar_filter: String::new(),
            layout_mode: initial_layout,
            split_ratio: 0.42,
            dragging_split: false,
            inset_bounds: Rc::new(Cell::new(None)),
            composer_popover: None,
            approval_mode: ApprovalMode::Host,
            command_palette_open: false,
            provider_sheet_open: false,
            hovered_starter_chip: None,
            home_spark,
            sidebar_search,
            sidebar_docs_menu,
            sidebar_tools_menu,
            layout_segmented,
            coedit_segmented,
            office_breadcrumb,
            coedit_dismiss_morph,
            theme_switch,
            command_desk,
            provider_status_view,
            toast,
            chat_actions_menu,
            office_menu,
            status_alert,
            show_alert: false,
            alert_opens_demo: false,
            composer_editor,
            http_dialog,
            http_url_input,
            open_path_dialog,
            open_path_input,
            quick_edit_dialog,
            quick_edit_primary,
            quick_edit_secondary,
            coedit_manual_editor,
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
            turn_sparks: BTreeMap::new(),
            turn_badges: BTreeMap::new(),
            tool_inspectors: BTreeMap::new(),
            settled_tool_cards: BTreeSet::new(),
            reasoning_blocks: BTreeMap::new(),
            diff_reviews: BTreeMap::new(),
            copy_buttons: BTreeMap::new(),
            chat_scroll: ScrollHandle::new(),
            office_tabs: Vec::new(),
            active_office_tab: None,
            next_tab_id: 1,
            pinned_target: None,
            office_notice: None,
            coedit_engine: Box::new(DemoCoEditEngine::new()),
            coedit_panel_open: false,
            active_coedit_scenario: initial_scenario,
            coedit_demo_dir: None,
            coedit_feedback: None,
        };

        for path in &config.open_files {
            this.open_office_file_tab(path.clone(), false, None, window, cx);
        }
        this.office_notice = None;

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

        let quit_tx = bridge.tx.clone();
        cx.on_app_quit(move |_, _| {
            let _ = quit_tx.try_send(KarCommand::Shutdown);
            async {}
        })
        .detach();

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

    pub fn set_layout_mode(&mut self, mode: WorkspaceLayoutMode, cx: &mut Context<Self>) {
        if self.layout_mode != mode {
            self.layout_mode = mode;
            let key = mode.as_segment_key();
            self.layout_segmented
                .update(cx, |seg, cx| seg.sync_value(key, cx));
            cx.notify();
        }
    }

    pub fn notify_toast(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        let message = msg.into();
        self.toast.update(cx, |t, cx| {
            t.show(message, None, Duration::from_millis(2800), cx);
        });
    }

    pub fn toggle_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.command_palette_open = !self.command_palette_open;
        self.composer_popover = None;
        if self.command_palette_open {
            self.provider_sheet_open = false;
        }
        let _ = window;
        cx.notify();
    }

    pub fn toggle_provider_sheet(&mut self, cx: &mut Context<Self>) {
        self.provider_sheet_open = !self.provider_sheet_open;
        self.composer_popover = None;
        if self.provider_sheet_open {
            self.command_palette_open = false;
        }
        cx.notify();
    }

    fn execute_command_id(
        &mut self,
        cmd_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match cmd_id {
            "new_chat" => self.create_new_real_chat(cx),
            "open_docx" => self.open_working_sample("sample_minutes.docx", window, cx),
            "open_xlsx" => self.open_working_sample("sample_formulas.xlsx", window, cx),
            "open_pptx" => self.open_working_sample("sample_deck.pptx", window, cx),
            "open_path" => {
                self.open_path_dialog
                    .update(cx, |d, cx| d.set_open(true, window, cx));
            }
            "bind_selection" => self.bind_active_selection_to_chat(window, cx),
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
            "demo_chat" => self.bridge.send(KarCommand::EnterDemoShowcase),
            "http_sub" => {
                self.http_dialog
                    .update(cx, |d, cx| d.set_open(true, window, cx));
            }
            "start_http" => self.handle_chat_menu_action("start_http", window, cx),
            "second_win" => self.open_subscriber_second_window(cx),
            "provider_health" => self.toggle_provider_sheet(cx),
            "toggle_theme" => {
                self.theme_mode = self.theme_mode.toggle();
                apply_unified_theme(self.theme_mode, cx);
            }
            "toggle_sidebar" => {
                self.sidebar_open = !self.sidebar_open;
            }
            _ => {}
        }
        cx.notify();
    }

    // ── Bridge Event Handling ───────────────────────────────────────────────

    fn on_bridge_event(&mut self, ev: KarBridgeEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            KarBridgeEvent::StoreReady { .. } => {}
            KarBridgeEvent::ProvidersUpdated(list) => {
                let status_items = build_kui_provider_items(&list);
                self.provider_status_view.update(cx, |ps, cx| {
                    ps.set_providers(status_items, cx);
                });
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
                self.composer_editor.update(cx, |ed, cx| {
                    ed.disabled = !writable;
                    let ph = match &mode {
                        ChatAccessMode::HttpReadOnly { .. } => {
                            "只读观察模式 — 当前连接 HTTP 快照与事件流，输入已锁定"
                        }
                        ChatAccessMode::LocalReadOnlySubscriber { .. } => {
                            "只读观察模式 — 另一窗口持有写入控制器，当前仅实时订阅"
                        }
                        ChatAccessMode::NoneSelected => {
                            "输入任务开启对话，或点击 + 关联右侧 Office 文档选区…"
                        }
                        ChatAccessMode::DemoShowcase => {
                            "演示沙盒（演示数据）— 输入任意问题体验回合流与工具卡片更新…"
                        }
                        ChatAccessMode::WriteController { .. } => {
                            "继续提问，或在右侧选中文档内容后点击「AI 改选区」…"
                        }
                    };
                    ed.set_placeholder(ph);
                    cx.notify();
                });
                self.access_mode = mode;
                self.sync_alerts(cx);
            }
            KarBridgeEvent::ConnectionStateChanged(conn) => {
                self.connection_state = conn;
                self.sync_alerts(cx);
            }
            KarBridgeEvent::ChatStateUpdated(proj) => {
                let rt_id = proj
                    .info
                    .as_ref()
                    .map(|i| i.runtime.clone())
                    .unwrap_or_else(|| self.selected_runtime.clone());
                self.sync_timeline_entities(&proj.turns, &rt_id, cx);
                self.projected = proj;
                self.sync_alerts(cx);
            }
            KarBridgeEvent::PromptLifecycle { attempt } => {
                self.prompt_attempts.insert(attempt.request_id, attempt);
            }
            KarBridgeEvent::HttpServerState {
                listening_url,
                message,
                ..
            } => {
                if let Some(ref url) = listening_url {
                    self.notify_toast(format!("只读 HTTP/SSE 已启动 · {url}"), cx);
                }
                self.active_http_server_url = listening_url;
                if let Some(msg) = message
                    && self.active_http_server_url.is_none()
                {
                    self.show_custom_alert("HTTP 订阅服务状态", &msg, Severity::Info, false, cx);
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

    fn sync_timeline_entities(
        &mut self,
        turns: &[ChatTurn],
        runtime_id: &str,
        cx: &mut Context<Self>,
    ) {
        let brand_name = match runtime_id {
            "codex" => "OpenAI",
            "claude" => "Claude",
            "grok" => "Grok",
            "qwen" => "Qwen",
            _ => runtime_display_name(runtime_id),
        };

        for turn in turns {
            let mut assistant_concat = String::new();
            let mut badge_steps: Vec<AgentStep> = Vec::new();
            let mut streaming_text = false;

            // Update per-turn KUI `AiSpark` status
            for (item_ix, item) in turn.items.iter().enumerate() {
                match &item.item {
                    Item::Tool { id, activity } => {
                        let key = (turn.id, id.clone());
                        let inv = tool_to_invocation(id, activity);
                        let is_failed = inv.state == ToolState::Failed;
                        badge_steps.push(AgentStep {
                            label: format!("调用工具 · {}", inv.name),
                            result: if activity.finished {
                                "执行完成".into()
                            } else {
                                "正在执行…".into()
                            },
                            complete: activity.finished,
                        });
                        let should_collapse =
                            turn.status == TurnStatus::Completed && activity.finished && !is_failed;
                        if let Some(existing) = self.tool_inspectors.get(&key) {
                            let collapse_once =
                                should_collapse && self.settled_tool_cards.insert(key.clone());
                            existing.update(cx, |inspector, cx| {
                                if let Some(first) = inspector.calls.first_mut() {
                                    *first = inv;
                                } else {
                                    inspector.calls.push(inv);
                                }
                                if collapse_once {
                                    inspector.set_group_open(false, cx);
                                }
                                cx.notify();
                            });
                        } else {
                            if should_collapse {
                                self.settled_tool_cards.insert(key.clone());
                            }
                            let entity = cx.new(|cx| {
                                let mut inspector = InvocationInspector::new(
                                    format!("tool-{}-{id}", &turn.id.to_string()[..8]),
                                    vec![inv],
                                )
                                .with_full_width(true)
                                .with_compact_rows(true);
                                if should_collapse {
                                    inspector.set_group_open(false, cx);
                                }
                                inspector
                            });
                            self.tool_inspectors.insert(key, entity);
                        }
                    }
                    Item::Message {
                        role: Role::Reasoning,
                        text,
                        complete,
                        ..
                    } => {
                        if text.trim().is_empty() {
                            continue;
                        }
                        badge_steps.push(AgentStep {
                            label: "思考与上下文规划".into(),
                            result: if *complete {
                                "规划完成".into()
                            } else {
                                "推理中…".into()
                            },
                            complete: *complete,
                        });
                        let key = (turn.id, item_ix);
                        let note_state = if !*complete {
                            NoteState::Live
                        } else if matches!(
                            turn.status,
                            TurnStatus::Interrupted | TurnStatus::Aborted | TurnStatus::Failed
                        ) {
                            NoteState::Interrupted
                        } else {
                            NoteState::Finished
                        };
                        if let Some(existing) = self.reasoning_blocks.get(&key) {
                            existing.update(cx, |note, cx| {
                                note.set_transcript(text.trim(), cx);
                                note.set_state(note_state, cx);
                                if note_state == NoteState::Finished {
                                    note.set_expanded(false, cx);
                                }
                            });
                        } else {
                            let initial_text = text.trim().to_owned();
                            let entity = cx.new(|cx| {
                                let mut note = ExplanationNote::new(
                                    format!("reason-{}-{item_ix}", &turn.id.to_string()[..8]),
                                    Vec::new(),
                                )
                                .plain()
                                .with_compact(true);
                                note.set_transcript(&initial_text, cx);
                                note.set_state(note_state, cx);
                                if note_state == NoteState::Finished {
                                    note.set_expanded(false, cx);
                                }
                                note
                            });
                            self.reasoning_blocks.insert(key, entity);
                        }
                    }
                    Item::Message {
                        role: Role::Assistant,
                        text,
                        complete,
                        ..
                    } => {
                        if !text.is_empty() {
                            if !assistant_concat.is_empty() {
                                assistant_concat.push('\n');
                            }
                            assistant_concat.push_str(text);
                        }
                        if !*complete {
                            streaming_text = true;
                            badge_steps.push(AgentStep {
                                label: "生成回复流".into(),
                                result: "输出中…".into(),
                                complete: false,
                            });
                        }
                    }
                    Item::Diff { diff, .. } => {
                        let key = (turn.id, item_ix);
                        let (file, hunk) = parse_diff_to_hunk(diff);
                        badge_steps.push(AgentStep {
                            label: format!("生成补丁 · {file}"),
                            result: "已记录变更".into(),
                            complete: true,
                        });
                        if let Some(existing) = self.diff_reviews.get(&key) {
                            existing.update(cx, |pr, cx| {
                                pr.file = file;
                                pr.hunks = vec![hunk];
                                cx.notify();
                            });
                        } else {
                            let entity = cx.new(|cx| {
                                let mut pr = PatchReview::new(
                                    format!("diff-{}-{item_ix}", &turn.id.to_string()[..8]),
                                    file,
                                    vec![hunk],
                                    cx,
                                );
                                pr.summary = "工作区补丁摘要".into();
                                pr
                            });
                            self.diff_reviews.insert(key, entity);
                        }
                    }
                    _ => {}
                }
            }

            let spark_status = match turn.status {
                TurnStatus::Running => {
                    if streaming_text {
                        SparkStatus::Writing
                    } else {
                        SparkStatus::Thinking
                    }
                }
                TurnStatus::Completed => SparkStatus::Done,
                TurnStatus::Failed => SparkStatus::Error,
                TurnStatus::Interrupted | TurnStatus::Aborted => SparkStatus::Idle,
            };
            if let Some(sp) = self.turn_sparks.get(&turn.id) {
                sp.update(cx, |s, cx| s.set_status(spark_status, cx));
            } else {
                let sp_entity = cx.new(|_| {
                    AiSpark::new(SharedString::from(format!(
                        "turn-spark-{}",
                        &turn.id.to_string()[..8]
                    )))
                    .with_size(14.5)
                    .with_tone(SparkTone::Foreground)
                    .with_status(spark_status)
                    .decorative(true)
                });
                self.turn_sparks.insert(turn.id, sp_entity);
            }

            if turn.status == TurnStatus::Running && badge_steps.is_empty() {
                badge_steps.push(AgentStep {
                    label: "正在连接会话并调度推理…".into(),
                    result: String::new(),
                    complete: false,
                });
            }

            if !badge_steps.is_empty() {
                let activity_summary = match turn.status {
                    TurnStatus::Running => format!("正在执行 {} 个步骤…", badge_steps.len()),
                    TurnStatus::Completed => format!("已完成 {} 个步骤", badge_steps.len()),
                    TurnStatus::Interrupted | TurnStatus::Aborted => "执行已中断".into(),
                    TurnStatus::Failed => "执行遇到错误".into(),
                };
                if let Some(existing_badge) = self.turn_badges.get(&turn.id) {
                    existing_badge.update(cx, |wb, cx| {
                        wb.agent = brand_name.to_owned();
                        wb.activity = activity_summary;
                        wb.steps = badge_steps;
                        cx.notify();
                    });
                } else {
                    let entity = cx.new(|_| {
                        WorkBadge::new(
                            format!("wb-{}", &turn.id.to_string()[..8]),
                            brand_name.to_owned(),
                            activity_summary,
                            badge_steps,
                        )
                        .with_compact_left(true)
                    });
                    self.turn_badges.insert(turn.id, entity);
                }
            }

            if !assistant_concat.is_empty() {
                if let Some(existing_copy) = self.copy_buttons.get(&turn.id) {
                    existing_copy.update(cx, |cb, _| cb.set_text(assistant_concat));
                } else {
                    let copy_entity = cx.new(|_| {
                        CopyButton::new(
                            format!("copy-{}", &turn.id.to_string()[..8]),
                            assistant_concat,
                            "复制回复",
                        )
                        .icon_only()
                        .with_compact(true)
                    });
                    self.copy_buttons.insert(turn.id, copy_entity);
                }
            }
        }
    }

    fn sync_alerts(&mut self, cx: &mut Context<Self>) {
        // Never auto-switch to demo on disconnect!
        match &self.connection_state {
            FeedConnectionState::Reconnecting {
                last_cursor,
                reason,
                ..
            } => {
                let cur = last_cursor.as_deref().unwrap_or("无");
                self.show_custom_alert(
                    "实时事件流中断 · 正在按游标重连",
                    &format!("{reason}（已确认游标：{cur}；保留真实历史，不自动切换至演示数据）"),
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
                    "HTTP 订阅连接已断开",
                    &format!("{reason}（最后确认游标：{cur}；界面保持断开状态，不切换为演示结果）"),
                    Severity::Error,
                    false,
                    cx,
                );
            }
            FeedConnectionState::Connected { .. } if self.access_mode.is_http_read_only() => {
                self.show_alert = false;
            }
            _ => {
                let local_ro_reason = match &self.access_mode {
                    ChatAccessMode::LocalReadOnlySubscriber { reason, .. } => Some(reason.clone()),
                    _ => None,
                };
                if let Some(reason) = local_ro_reason {
                    self.show_custom_alert(
                        "只读观察模式（另一窗口持有写入锁）",
                        &reason,
                        Severity::Info,
                        false,
                        cx,
                    );
                } else {
                    self.refresh_provider_alert(cx);
                }
            }
        }
    }

    pub(crate) fn refresh_provider_alert(&mut self, cx: &mut Context<Self>) {
        if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            self.show_alert = false;
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
                        format!("供应商 `{}` 当前不可直接发起真实会话", prov.id),
                        reason,
                        Severity::Warning,
                    )
                    .with_compact_inline(true)
                    .with_action("进入演示沙盒"),
                    cx,
                );
            });
            return;
        }
        self.show_alert = false;
    }

    pub(crate) fn show_custom_alert(
        &mut self,
        title: &str,
        detail: &str,
        severity: Severity,
        offer_demo: bool,
        cx: &mut Context<Self>,
    ) {
        self.alert_opens_demo = offer_demo;
        self.show_alert = true;
        let mut next =
            Alert::new(title.to_owned(), detail.to_owned(), severity).with_compact_inline(true);
        if offer_demo {
            next = next.with_action("进入演示沙盒");
        }
        self.status_alert.update(cx, |a, cx| a.replace(next, cx));
    }

    // ── Actions & Workflows ─────────────────────────────────────────────────

    pub fn select_runtime_by_id(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected_runtime = id.to_owned();
        self.composer_popover = None;
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
        self.composer_popover = None;
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
                ProviderAvailability::Available { command, .. } => command.clone(),
                _ => None,
            });
        self.bridge.send(KarCommand::CreateChat {
            runtime_id: self.selected_runtime.clone(),
            program,
            workspace: self.config.workspace_dir.clone(),
            approval_mode: self.approval_mode,
        });
    }

    pub fn fill_starter_prompt(
        &mut self,
        prompt: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composer_editor.update(cx, |ed, cx| {
            ed.set_value(prompt, window, cx);
            ed.focus(window, cx);
        });
        cx.notify();
    }

    pub fn submit_composer_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer_popover = None;
        let raw = self.composer_editor.read(cx).value().trim().to_owned();
        if raw.is_empty() {
            return;
        }
        if matches!(self.access_mode, ChatAccessMode::NoneSelected) {
            if let Some(prov) = self.providers.iter().find(|p| p.id == self.selected_runtime)
                && let Some(reason) = prov.diagnostic_reason()
            {
                self.show_custom_alert(
                    &format!("无法用 `{}` 发送真实消息", prov.id),
                    &reason,
                    Severity::Error,
                    true,
                    cx,
                );
                cx.notify();
                return;
            }
            self.create_new_real_chat(cx);
            return;
        }
        if !self.access_mode.is_writable() {
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
        self.composer_editor
            .update(cx, |ed, cx| ed.set_value("", window, cx));

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

    /// Retry an interrupted or failed turn using the EXACT same `request_id` UUID.
    pub fn retry_turn_with_same_uuid(
        &mut self,
        turn_id: Uuid,
        fallback_user_text: String,
        cx: &mut Context<Self>,
    ) {
        let (raw_user_text, payload, target) = if let Some(prev) = self.prompt_attempts.get(&turn_id)
        {
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

    pub(crate) fn handle_chat_menu_action(
        &mut self,
        action_id: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action_id {
            "start_http" => {
                if let Some(chat_id) = self.current_chat_id() {
                    self.bridge.send(KarCommand::StartHttpServer {
                        chat_id,
                        listen_addr: "127.0.0.1:0".into(),
                    });
                } else {
                    self.show_custom_alert(
                        "请先选择或新建一个真实会话",
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
            "provider_health" => {
                self.toggle_provider_sheet(cx);
            }
            "refresh_all" => {
                self.bridge.send(KarCommand::RefreshStoreAndProviders {
                    store_dir: self.config.chat_dir.clone(),
                    workspace: self.config.workspace_dir.clone(),
                });
                self.notify_toast("已刷新会话存储与供应商状态", cx);
            }
            "abort_turn" => {
                self.bridge.send(KarCommand::AbortTurn);
            }
            _ => {}
        }
        cx.notify();
    }

    pub(crate) fn open_subscriber_second_window(&mut self, cx: &mut Context<Self>) {
        let mut sub_cfg = self.config.clone();
        sub_cfg.open_files.clear();
        sub_cfg.start_coedit_demo = false;
        sub_cfg.dark_mode = self.theme_mode == ThemeMode::Dark;
        if let Some(url) = &self.active_http_server_url {
            sub_cfg.subscribe_http = Some(url.clone());
        }
        let title = match &sub_cfg.subscribe_http {
            Some(url) => format!("Kar · 只读订阅 ({url})"),
            None => "Kar · 第二客户端".into(),
        };
        let options = studio_window_options(
            title,
            size(px(1260.), px(800.)),
            size(px(860.), px(540.)),
            cx,
        );
        let _ = gpui_kit::open_window(options, cx, move |w, cx| {
            cx.new(|cx| KarWorkbench::new(sub_cfg, w, cx))
        });
    }

    pub(crate) fn current_chat_id(&self) -> Option<Uuid> {
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

    pub(crate) fn handle_office_menu_action(
        &mut self,
        action_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action_id {
            "sample_docx" => self.open_working_sample("sample_minutes.docx", window, cx),
            "sample_xlsx" => self.open_working_sample("sample_formulas.xlsx", window, cx),
            "sample_pptx" => self.open_working_sample("sample_deck.pptx", window, cx),
            "open_custom_path" => {
                self.open_path_dialog
                    .update(cx, |d, cx| d.set_open(true, window, cx));
            }
            "blank_docx" => self.open_blank_office_tab(OfficeKind::Word, window, cx),
            "blank_xlsx" => self.open_blank_office_tab(OfficeKind::Excel, window, cx),
            "blank_pptx" => self.open_blank_office_tab(OfficeKind::Ppt, window, cx),
            "coedit_demo" => self.enter_coedit_demo_workshop(window, cx),
            _ => {}
        }
    }

    pub(crate) fn open_working_sample(
        &mut self,
        filename: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composer_popover = None;
        let saved_variant = filename.replace('.', ".saved.");
        let candidate = if self.config.docs_dir.join(filename).is_file() {
            self.config.docs_dir.join(filename)
        } else if self.config.docs_dir.join(&saved_variant).is_file() {
            self.config.docs_dir.join(&saved_variant)
        } else {
            let corpus_fallback = match filename {
                "sample_minutes.docx" => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|r| r.join("tests/corpus/docx/minutes.docx")),
                "sample_formulas.xlsx" => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|r| r.join("tests/corpus/xlsx/01_formulas.xlsx")),
                "sample_deck.pptx" => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|r| r.join("tests/corpus/pptx/deck.pptx")),
                _ => None,
            };
            if let Some(src) = corpus_fallback.filter(|p| p.is_file()) {
                let _ = std::fs::create_dir_all(&self.config.docs_dir);
                let dst = self.config.docs_dir.join(filename);
                let _ = std::fs::copy(&src, &dst);
                dst
            } else {
                self.config.docs_dir.join(filename)
            }
        };

        if candidate.is_file() {
            self.open_office_file_tab(candidate, false, None, window, cx);
        } else {
            self.office_notice = Some(format!("未找到样本 {filename}，可点击 + 按本地路径打开"));
            self.set_layout_mode(WorkspaceLayoutMode::Split, cx);
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
        if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
            self.set_layout_mode(WorkspaceLayoutMode::Split, cx);
        }
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
        match OfficeTab::open_file(id, canonical, is_demo, demo_key, window, cx) {
            Ok(tab) => {
                if !is_demo {
                    self.coedit_panel_open = false;
                }
                self.office_tabs.push(tab);
                self.active_office_tab = Some(self.office_tabs.len() - 1);
                self.office_notice = None;
            }
            Err(err) => {
                self.office_notice = Some(format!("打开失败: {err:#}"));
            }
        }
        cx.notify();
    }

    pub(crate) fn open_blank_office_tab(
        &mut self,
        kind: OfficeKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
            self.set_layout_mode(WorkspaceLayoutMode::Split, cx);
        }
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = OfficeTab::new_blank(id, kind, window, cx);
        self.coedit_panel_open = false;
        self.office_tabs.push(tab);
        self.active_office_tab = Some(self.office_tabs.len() - 1);
        self.office_notice = None;
        cx.notify();
    }

    pub fn bind_active_selection_to_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer_popover = None;
        let Some(idx) = self.active_office_tab else {
            return;
        };
        let Some(tab) = self.office_tabs.get(idx) else {
            return;
        };
        let target = tab.capture_selection(cx);
        let label = target.locator.target_label();
        self.office_notice = Some(format!("已引用 · {label}"));
        self.pinned_target = Some(target);
        self.notify_toast(format!("已将「{label}」关联至对话输入框"), cx);
        self.composer_editor
            .update(cx, |ed, cx| ed.focus(window, cx));
        cx.notify();
    }

    pub fn jump_to_selection_target(
        &mut self,
        target: &SelectionTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
            self.set_layout_mode(WorkspaceLayoutMode::Split, cx);
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
            self.office_notice = Some(format!("已定位 · {}", target.locator.target_label()));
            cx.notify();
        }
    }

    pub(crate) fn open_quick_edit_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            SelectionLocator::PptObject { bounds_cm, .. } => {
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
                    .map(|calc| format!("{cell} = {primary} → {calc}"))
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
            Ok(msg) => {
                self.notify_toast(msg.clone(), cx);
                self.office_notice = Some(msg);
            }
            Err(err) => self.office_notice = Some(format!("修改未生效: {err:#}")),
        }
        cx.notify();
    }

    pub(crate) fn save_close_and_reopen_active_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(idx) = self.active_office_tab else {
            return;
        };
        let Some(tab) = self.office_tabs.get(idx).cloned() else {
            return;
        };
        if tab.is_demo {
            self.office_notice = Some("演示沙盒文档不写回用户文件".into());
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
                let fname = copy_dest
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("saved")
                    .to_owned();
                self.office_tabs.remove(idx);
                self.active_office_tab = None;
                self.open_office_file_tab(copy_dest, false, None, window, cx);
                let msg = format!("已保存副本并重开验证 · {fname}");
                self.office_notice = Some(msg.clone());
                self.notify_toast(msg, cx);
            }
            Err(err) => {
                self.office_notice = Some(format!("保存副本失败: {err:#}"));
            }
        }
        cx.notify();
    }

    // ── Co-Editing Demo Workshop (Strictly Isolated Demo Tabs) ──────────────

    pub fn enter_coedit_demo_workshop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
            self.set_layout_mode(WorkspaceLayoutMode::Split, cx);
        }
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
                self.coedit_dismiss_morph
                    .update(cx, |m, cx| m.restore(cx));
                self.coedit_panel_open = true;
                self.switch_coedit_scenario(self.active_coedit_scenario, window, cx);
                self.coedit_feedback = None;
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
                self.coedit_dismiss_morph
                    .update(cx, |m, cx| m.restore(cx));
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
                    "解决期间文档再次变化（rev #{previous_revision} → rev #{latest_revision}，最新值：{latest_value}），请先确认最新修订"
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
                        if let Some(ref summary) = proposal.resolution_result {
                            self.notify_toast(summary.clone(), cx);
                        }
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

    fn handle_global_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let cmd = ev.keystroke.modifiers.platform || ev.keystroke.modifiers.control;
        let key = ev.keystroke.key.as_str();
        if cmd && key.eq_ignore_ascii_case("k") {
            self.toggle_command_palette(window, cx);
            cx.stop_propagation();
            return;
        }
        if cmd && (key == "." || key.eq_ignore_ascii_case("b")) {
            self.sidebar_open = !self.sidebar_open;
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if key == "escape"
            && (self.command_palette_open
                || self.provider_sheet_open
                || self.composer_popover.is_some())
        {
            self.command_palette_open = false;
            self.provider_sheet_open = false;
            self.composer_popover = None;
            cx.notify();
            cx.stop_propagation();
        }
    }

    fn render_command_palette_modal(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let desk = self.command_desk.clone();
        let shadows = s.popover_shadows();
        let weak_close = cx.weak_entity();
        pro_presence(
            "kar-cmd-modal",
            self.command_palette_open,
            0.18,
            0.13,
            move |amt| {
                div()
                    .id("kar-cmd-backdrop")
                    .absolute()
                    .inset_0()
                    .bg(hsla(0., 0., 0., 0.32 * amt))
                    .flex()
                    .flex_col()
                    .items_center()
                    .pt(px(86. + 8. * (1. - amt)))
                    .occlude()
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        if let Some(e) = weak_close.upgrade() {
                            e.update(cx, |this, cx| {
                                this.command_palette_open = false;
                                cx.notify();
                            });
                        }
                    })
                    .child(
                        div()
                            .id("kar-cmd-card")
                            .w(px(522.))
                            .max_w(relative(0.92))
                            .opacity(amt)
                            .rounded(px(10.))
                            .shadow(shadows)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(desk),
                    )
                    .into_any_element()
            },
        )
    }

    fn render_provider_health_modal(
        &self,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ps = self.provider_status_view.clone();
        let card_bg = s.card_bg;
        let border_c = s.border_strong;
        let muted_c = s.muted;
        let hover_c = s.stage_hover;
        let shadows = s.popover_shadows();
        let weak_close = cx.weak_entity();
        let weak_btn_close = cx.weak_entity();
        pro_presence(
            "kar-provider-modal",
            self.provider_sheet_open,
            0.18,
            0.13,
            move |amt| {
                div()
                    .id("kar-prov-backdrop")
                    .absolute()
                    .inset_0()
                    .bg(hsla(0., 0., 0., 0.32 * amt))
                    .flex()
                    .flex_col()
                    .items_center()
                    .pt(px(64. + 8. * (1. - amt)))
                    .occlude()
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        if let Some(e) = weak_close.upgrade() {
                            e.update(cx, |this, cx| {
                                this.provider_sheet_open = false;
                                cx.notify();
                            });
                        }
                    })
                    .child(
                        div()
                            .id("kar-prov-card")
                            .relative()
                            .w(px(556.))
                            .max_w(relative(0.92))
                            .max_h(px(640.))
                            .opacity(amt)
                            .rounded(px(12.))
                            .bg(card_bg)
                            .border_1()
                            .border_color(border_c)
                            .shadow(shadows)
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .id("kar-prov-scroll")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .pt(px(14.))
                                .pl(px(16.))
                                .pr(px(48.))
                                .pb(px(16.))
                                .child(ps),
                        )
                        .child(
                            div()
                                .id("kar-prov-modal-close")
                                .absolute()
                                .top(px(14.))
                                .right(px(12.))
                                .size(px(28.))
                                .rounded(px(7.))
                                .bg(card_bg)
                                .hover(move |st| st.bg(hover_c))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .on_click(move |_, _, cx| {
                                    if let Some(e) = weak_btn_close.upgrade() {
                                        e.update(cx, |this, cx| {
                                            this.provider_sheet_open = false;
                                            cx.notify();
                                        });
                                    }
                                })
                                .child(kui::primitives::pro_icons::pro_icon(
                                    ProIcon::Close,
                                    14.,
                                    muted_c,
                                )),
                        ),
                    )
                    .into_any_element()
            },
        )
    }
}

impl Render for KarWorkbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = StudioPalette::get(cx);
        let bounds_cell = self.inset_bounds.clone();
        let show_office = self.layout_mode != WorkspaceLayoutMode::ChatFocus;
        let chat_frac = match self.layout_mode {
            WorkspaceLayoutMode::ChatFocus => 1.0,
            WorkspaceLayoutMode::Split => self.split_ratio.clamp(0.30, 0.68),
            WorkspaceLayoutMode::CanvasWide => 0.33,
        };
        let office_frac = (1.0 - chat_frac).max(0.28);

        let chat_view = self.render_chat_pane(window, cx);
        let office_view = show_office.then(|| self.render_office_pane(window, cx));
        let sidebar_el = self.sidebar_open.then(|| self.render_sidebar(window, cx));

        let hover_divider = s.ink.opacity(0.28);
        let rest_divider = s.border;
        let is_dragging = self.dragging_split;

        // Kense Inset Workspace Frame (`p(6)`, `rounded(12)`, crisp monochrome elevation)
        let inset_workspace = div()
            .id("kar-inset-workspace")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .p(px(6.))
            .when(self.sidebar_open, |d| d.pl_0())
            .child(
                div()
                    .relative()
                    .size_full()
                    .rounded(px(12.))
                    .bg(s.inset_bg)
                    .border_1()
                    .border_color(s.border)
                    .shadow(s.inset_shadows())
                    .overflow_hidden()
                    .flex()
                    .flex_row()
                    .child(
                        canvas(
                            move |b, _, _| {
                                bounds_cell.set(Some(b));
                            },
                            |_, _, _, _| (),
                        )
                        .absolute()
                        .size_0(),
                    )
                    .child(
                        div()
                            .h_full()
                            .min_w(px(336.))
                            .when(!show_office, |d| d.flex_1())
                            .when(show_office, |d| d.w(relative(chat_frac)))
                            .overflow_hidden()
                            .child(chat_view),
                    )
                    .when_some(office_view, |d, ov| {
                        d.child(
                            div()
                                .id("kar-split-divider")
                                .w(px(6.))
                                .h_full()
                                .flex_none()
                                .cursor_col_resize()
                                .flex()
                                .items_center()
                                .justify_center()
                                .group("split-handle")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| {
                                        this.dragging_split = true;
                                        if this.layout_mode == WorkspaceLayoutMode::CanvasWide {
                                            this.split_ratio = 0.33;
                                            this.set_layout_mode(WorkspaceLayoutMode::Split, cx);
                                        }
                                        cx.notify();
                                    }),
                                )
                                .child(
                                    div()
                                        .w(px(if is_dragging { 2. } else { 1. }))
                                        .h_full()
                                        .bg(if is_dragging {
                                            hover_divider
                                        } else {
                                            rest_divider
                                        })
                                        .group_hover("split-handle", move |st| {
                                            st.w(px(2.)).bg(hover_divider)
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .h_full()
                                .w(relative(office_frac))
                                .min_w(px(400.))
                                .flex_1()
                                .overflow_hidden()
                                .child(ov),
                        )
                    }),
            );

        let cmd_modal = self.render_command_palette_modal(&s, cx);
        let prov_modal = self.render_provider_health_modal(&s, cx);

        div()
            .id("kar-workbench-root")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_global_key))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if this.dragging_split
                    && let Some(b) = this.inset_bounds.get()
                {
                    let total_w = f32::from(b.size.width).max(600.);
                    let rel_x = f32::from(ev.position.x - b.origin.x);
                    this.split_ratio = (rel_x / total_w).clamp(0.30, 0.68);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.dragging_split {
                        this.dragging_split = false;
                        cx.notify();
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.composer_popover.is_some() {
                        this.composer_popover = None;
                        cx.notify();
                    }
                }),
            )
            .size_full()
            .relative()
            .bg(s.stage)
            .flex()
            .flex_row()
            .overflow_hidden()
            .when_some(sidebar_el, |d, sb| {
                d.child(
                    div()
                        .w(px(236.))
                        .h_full()
                        .flex_none()
                        .overflow_hidden()
                        .child(sb),
                )
            })
            .child(inset_workspace)
            .child(
                div()
                    .absolute()
                    .right(px(20.))
                    .bottom(px(18.))
                    .child(self.toast.clone()),
            )
            .child(cmd_modal)
            .child(prov_modal)
            .child(self.http_dialog.clone())
            .child(self.open_path_dialog.clone())
            .child(self.quick_edit_dialog.clone())
    }
}

pub(crate) fn runtime_display_name(id: &str) -> &'static str {
    match id {
        "codex" => "Codex",
        "claude" => "Claude Code",
        "grok" => "Grok CLI",
        "opencode" => "OpenCode",
        "kimi" => "Kimi CLI",
        "pi" => "Pi Agent",
        "qwen" => "Qwen Code",
        "cursor" => "Cursor Agent",
        _ => "Kar Agent",
    }
}

fn build_default_commands() -> Vec<Command> {
    vec![
        Command {
            id: "new_chat".into(),
            label: "新建真实 Kar 对话".into(),
            section: "对话工作台".into(),
            hint: "⌘N".into(),
        },
        Command {
            id: "bind_selection".into(),
            label: "将当前 Office 文档选区关联至对话".into(),
            section: "对话工作台".into(),
            hint: "AI 改选区".into(),
        },
        Command {
            id: "open_docx".into(),
            label: "打开 Word 会议纪要样本 (.docx)".into(),
            section: "Kantor Office 画布".into(),
            hint: "DocsView".into(),
        },
        Command {
            id: "open_xlsx".into(),
            label: "打开 Excel 季度预算公式表 (.xlsx)".into(),
            section: "Kantor Office 画布".into(),
            hint: "SheetsView".into(),
        },
        Command {
            id: "open_pptx".into(),
            label: "打开 PPT 产品发布演示文稿 (.pptx)".into(),
            section: "Kantor Office 画布".into(),
            hint: "SlidesView".into(),
        },
        Command {
            id: "open_path".into(),
            label: "按本地路径打开文档…".into(),
            section: "Kantor Office 画布".into(),
            hint: "Path".into(),
        },
        Command {
            id: "coedit_word".into(),
            label: "人机双写演练：Word 同段日期与负责人合并".into(),
            section: "双写协议演练 (演示数据)".into(),
            hint: "双项保留".into(),
        },
        Command {
            id: "coedit_excel".into(),
            label: "人机双写演练：Excel 同单元格三选一冲突裁决".into(),
            section: "双写协议演练 (演示数据)".into(),
            hint: "冲突裁决".into(),
        },
        Command {
            id: "coedit_ppt".into(),
            label: "人机双写演练：PPT 文本框移位与删除冲突".into(),
            section: "双写协议演练 (演示数据)".into(),
            hint: "对象定位".into(),
        },
        Command {
            id: "start_http".into(),
            label: "启动当前会话 HTTP/SSE 只读广播服务".into(),
            section: "连接与诊断".into(),
            hint: "/snapshot + /events".into(),
        },
        Command {
            id: "second_win".into(),
            label: "新开第二客户端窗口观察当前会话".into(),
            section: "连接与诊断".into(),
            hint: "单写入锁验证".into(),
        },
        Command {
            id: "provider_health".into(),
            label: "查看本机供应商与账号健康度诊断".into(),
            section: "连接与诊断".into(),
            hint: "ProviderStatus".into(),
        },
        Command {
            id: "demo_chat".into(),
            label: "进入独立演示对话沙盒 (演示数据)".into(),
            section: "连接与诊断".into(),
            hint: "离线可用".into(),
        },
        Command {
            id: "toggle_theme".into(),
            label: "切换浅色 / 深色主题 (KUI + Kantor 同步)".into(),
            section: "外观与布局".into(),
            hint: "Theme".into(),
        },
        Command {
            id: "toggle_sidebar".into(),
            label: "展开 / 收起左侧导航栏".into(),
            section: "外观与布局".into(),
            hint: "⌘.".into(),
        },
    ]
}

fn build_kui_provider_items(list: &[ProviderStatusInfo]) -> Vec<ProviderStatusItem> {
    let mut sorted = list.to_vec();
    sorted.sort_by_key(|p| {
        if p.is_ready() {
            0u8
        } else if matches!(p.availability, ProviderAvailability::Available { .. }) {
            1u8
        } else {
            2u8
        }
    });
    sorted
        .iter()
        .map(|p| {
            let (badge_txt, ready) = p.status_badge();
            let health = match &p.availability {
                ProviderAvailability::Available { .. } if ready => ProviderHealth::Operational,
                ProviderAvailability::Available { .. } => ProviderHealth::Degraded,
                ProviderAvailability::Missing => ProviderHealth::Unknown,
                ProviderAvailability::Failed { .. } => ProviderHealth::Outage,
            };
            let mut item = ProviderStatusItem::new(
                p.id.clone(),
                format!("{} ({})", runtime_display_name(&p.id), p.protocol),
                health,
            );
            item.detail = badge_txt.into();
            item.uptime = Some(if ready {
                100.0
            } else if matches!(p.availability, ProviderAvailability::Available { .. }) {
                65.0
            } else {
                0.0
            });
            item.days = (1..=14)
                .map(|d| ProviderDay::new(format!("11-{d:02}"), health, item.uptime).note(badge_txt))
                .collect();
            if let Some(reason) = p.diagnostic_reason() {
                item.incident = Some(ProviderIncident::new(
                    format!("{} 环境诊断提示", runtime_display_name(&p.id)),
                    vec![ProviderIncidentUpdate::new("本机探测", "实时", reason)],
                ));
            }
            item
        })
        .collect()
}

fn parse_diff_to_hunk(diff: &str) -> (String, Hunk) {
    let mut file = "workspace.patch".to_owned();
    let mut before = Vec::new();
    let mut after = Vec::new();
    for line in diff.lines().take(24) {
        if let Some(rest) = line.strip_prefix("+++ b/") {
            file = rest.trim().to_owned();
        } else if line.starts_with("--- ") || line.starts_with("@@") || line.starts_with("diff ") {
            continue;
        } else if let Some(rem) = line.strip_prefix('-') {
            before.push(rem.to_owned());
        } else if let Some(add) = line.strip_prefix('+') {
            after.push(add.to_owned());
        } else if let Some(ctx) = line.strip_prefix(' ') {
            before.push(ctx.to_owned());
            after.push(ctx.to_owned());
        }
    }
    if before.is_empty() && after.is_empty() {
        after = diff.lines().take(8).map(str::to_owned).collect();
    }
    (
        file,
        Hunk {
            line: 1,
            before,
            after,
            decision: Decision::Accepted,
        },
    )
}

fn tool_to_invocation(call_id: &str, tool: &Tool) -> Invocation {
    let name = tool
        .tool_name
        .clone()
        .or_else(|| tool.name.clone())
        .unwrap_or_else(|| "shell".into());
    let short_call = if call_id.len() > 18 {
        format!("{}…", &call_id[..18])
    } else {
        call_id.to_owned()
    };
    let action = match (&tool.server, &tool.native_type) {
        (Some(srv), Some(nt)) => format!("{srv} ({nt}) · {short_call}"),
        (Some(srv), None) => format!("{srv} · {short_call}"),
        (None, Some(nt)) => format!("{nt} · {short_call}"),
        (None, None) => short_call,
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
