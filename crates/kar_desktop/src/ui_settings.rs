//! Workspace settings with a 7-page product-oriented KUI navigation rail and deep `/` search.

use crate::ui_motion::selection::{Group, SelectionStrip};
use gpui::{prelude::*, *};
use kui::{
    components::pro_chrome::{
        pro_mark_selected, pro_pill_indicator, pro_presence, pro_selection_sensor, pro_spring_f32,
        pro_view_transition_fill,
    },
    primitives::{
        editor::{Editor, InputMode},
        family::{Slide, press},
        pro_icons::{ProIcon, pro_icon},
    },
};

use crate::{
    kar_bridge::KarCommand,
    prefs::{DesktopPreferences, NotificationLevel, ProjectSortOrder, QuitShortcutMode},
    ui_motion::{Tactile, icon_button, outline_button, soft_button, text_button},
    ui_primitives::{
        FS_META, FS_TAG, FS_UI, R_MODAL, R_PILL, R_SM, StudioPalette, UiLocale, card_box,
        render_kbd, render_schedule_repeat_icon, render_skills_puzzle_icon,
        render_usage_pulse_icon, settings_divider, settings_page_shell, settings_row_item,
        settings_section_title,
    },
    workbench::KarWorkbench,
};

struct SettingsSwitchInteraction {
    focus: FocusHandle,
    hovered: bool,
    stretch: Slide,
    _blur: Subscription,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsTab {
    #[default]
    General,
    Appearance,
    Keybindings,
    Voice,
    Agents,
    SkillsMcp,
    Usage,
    Schedules,
    ProjectsGit,
    Devices,
    Remote,
    Logs,
}

impl SettingsTab {
    pub const ALL: [Self; 12] = [
        Self::General,
        Self::Appearance,
        Self::Keybindings,
        Self::Voice,
        Self::Agents,
        Self::SkillsMcp,
        Self::Usage,
        Self::Schedules,
        Self::ProjectsGit,
        Self::Devices,
        Self::Remote,
        Self::Logs,
    ];

    pub const PRIMARY_NAV: [Self; 7] = [
        Self::General,
        Self::Appearance,
        Self::Keybindings,
        Self::Agents,
        Self::SkillsMcp,
        Self::Schedules,
        Self::Usage,
    ];

    pub fn rail_parent(self) -> Self {
        match self {
            Self::Schedules | Self::Remote | Self::Voice => Self::Schedules,
            Self::Usage | Self::ProjectsGit | Self::Devices | Self::Logs => Self::Usage,
            other => other,
        }
    }

    pub fn rail_title(self, locale: UiLocale) -> &'static str {
        match self.rail_parent() {
            Self::General => locale.pick("通用偏好", "General"),
            Self::Appearance => locale.pick("外观与字号", "Appearance"),
            Self::Keybindings => locale.pick("快捷键", "Shortcuts"),
            Self::Agents => locale.pick("模型与权限", "Models & Access"),
            Self::SkillsMcp => locale.pick("技能与工具", "Skills & Tools"),
            Self::Schedules => locale.pick("自动化与协同", "Automation & Sync"),
            Self::Usage => locale.pick("工程与维护", "Projects & System"),
            _ => self.title(locale),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::General => "settings-general",
            Self::Appearance => "settings-appearance",
            Self::Keybindings => "settings-keybindings",
            Self::Voice => "settings-voice",
            Self::Agents => "settings-agents",
            Self::SkillsMcp => "settings-skills",
            Self::Usage => "settings-usage",
            Self::Schedules => "settings-schedules",
            Self::ProjectsGit => "settings-projects-git",
            Self::Devices => "settings-devices",
            Self::Remote => "settings-remote",
            Self::Logs => "settings-logs",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "通用偏好",
            Self::Appearance => "外观与字号",
            Self::Keybindings => "快捷键",
            Self::Voice => "语音听写",
            Self::Agents => "模型与权限",
            Self::SkillsMcp => "技能与工具",
            Self::Usage => "用量统计",
            Self::Schedules => "定时任务",
            Self::ProjectsGit => "项目与 Git",
            Self::Devices => "移动与预览",
            Self::Remote => "多端扫码协同",
            Self::Logs => "清理与关于",
        }
    }

    pub fn title(self, locale: UiLocale) -> &'static str {
        let english = match self {
            Self::General => "General",
            Self::Appearance => "Appearance & Themes",
            Self::Keybindings => "Keybindings",
            Self::Voice => "Voice & Dictation",
            Self::Agents => "Providers & Models",
            Self::SkillsMcp => "Skills & MCP",
            Self::Usage => "Usage & Cache",
            Self::Schedules => "Scheduled Tasks",
            Self::ProjectsGit => "Projects & Git",
            Self::Devices => "Devices & Preview",
            Self::Remote => "Sharing & Remote",
            Self::Logs => "Maintenance & About",
        };
        locale.pick(self.label(), english)
    }
}

struct DeepSettingItem {
    tab: SettingsTab,
    title_zh: &'static str,
    title_en: &'static str,
    keywords: &'static str,
}

const SETTINGS_DEEP_INDEX: &[DeepSettingItem] = &[
    DeepSettingItem {
        tab: SettingsTab::General,
        title_zh: "系统通知与提示音",
        title_en: "Thread & system notifications",
        keywords: "notification toast sound alert 通知 提醒 气泡 声音",
    },
    DeepSettingItem {
        tab: SettingsTab::General,
        title_zh: "侧边栏分组与排序",
        title_en: "Sidebar grouping and order",
        keywords: "sidebar group project sort working 侧栏 分组 排序 运行中 隐藏文件",
    },
    DeepSettingItem {
        tab: SettingsTab::General,
        title_zh: "删除/归档二次确认与 ⌘Q 防误触退出",
        title_en: "Delete/archive confirmations & ⌘Q quit mode",
        keywords: "confirm delete archive unpin terminal quit hold 二次确认 删除 归档 退出 防误触",
    },
    DeepSettingItem {
        tab: SettingsTab::Appearance,
        title_zh: "多彩主题画廊（Catppuccin / Tokyo Night / Nord 等）",
        title_en: "Color theme gallery (900+ Ghostty & Omarchy themes)",
        keywords: "theme dark light catppuccin tokyo gruvbox nord dracula 主题 配色 深色 浅色",
    },
    DeepSettingItem {
        tab: SettingsTab::Appearance,
        title_zh: "界面字号、代码字体与聊天栏宽",
        title_en: "UI font scale, monospace font & chat width",
        keywords: "font size scale mono jetbrains width 字体 字号 缩放 宽度",
    },
    DeepSettingItem {
        tab: SettingsTab::Appearance,
        title_zh: "Diff 分栏/统一对比、发送键与追问排队",
        title_en: "Split vs Unified diff, Enter/⌘Enter & Queue/Steer",
        keywords: "diff split unified whitespace enter send queue steer 差异 分栏 并排 回车 发送 插话",
    },
    DeepSettingItem {
        tab: SettingsTab::Keybindings,
        title_zh: "自定义全局快捷键与冲突检测",
        title_en: "Customize keyboard shortcuts & keybindings.json",
        keywords: "keybindings shortcut hotkey cmd ctrl 快捷键 改键 按键 冲突 keybindings.json",
    },
    DeepSettingItem {
        tab: SettingsTab::Voice,
        title_zh: "本地离线 Whisper 与云端语音听写",
        title_en: "Local Whisper & cloud voice dictation",
        keywords: "voice mic stt whisper openai grok dictation 语音 麦克风 听写 口头禅 词库",
    },
    DeepSettingItem {
        tab: SettingsTab::Agents,
        title_zh: "智能体应用内登录、退出与检查升级",
        title_en: "Provider sign-in, logout, API keys & CLI upgrades",
        keywords: "login logout oauth api key update upgrade codex claude gemini 登录 退出 账号 升级 安装",
    },
    DeepSettingItem {
        tab: SettingsTab::Agents,
        title_zh: "模型搜索、收藏、自定义模型与全自动权限",
        title_en: "Model search, favorites, custom models & full-auto permissions",
        keywords: "model favorite custom allow permission fallback quota 模型 收藏 隐藏 全自动 权限 兜底 额度",
    },
    DeepSettingItem {
        tab: SettingsTab::Agents,
        title_zh: "第三方社区 ACP 智能体商店",
        title_en: "Community ACP agent registry",
        keywords: "acp community goose aider cline roo 第三方 商店 注册",
    },
    DeepSettingItem {
        tab: SettingsTab::SkillsMcp,
        title_zh: "技能（SKILL.md）、斜杠命令与 MCP 工具箱",
        title_en: "Skills, slash commands & MCP servers",
        keywords: "skill mcp tool command 技能 插件 工具",
    },
    DeepSettingItem {
        tab: SettingsTab::Usage,
        title_zh: "Token 消耗、缓存命中率与账号额度监控",
        title_en: "Token usage, prompt cache & account quotas",
        keywords: "usage token cost cache quota 用量 缓存 额度 统计",
    },
    DeepSettingItem {
        tab: SettingsTab::Schedules,
        title_zh: "自定义时间、星期、分钟间隔与 Webhook 定时任务",
        title_en: "Custom schedules, cron, intervals & webhooks",
        keywords: "schedule cron timer webhook daily 定时 自动 排程 星期",
    },
    DeepSettingItem {
        tab: SettingsTab::ProjectsGit,
        title_zh: "项目管理、克隆仓库、外部编辑器与快捷命令",
        title_en: "Projects, Git clone, VS Code / Cursor / Zed & scripts",
        keywords: "project clone vscode cursor zed script 项目 克隆 编辑器 脚本",
    },
    DeepSettingItem {
        tab: SettingsTab::ProjectsGit,
        title_zh: "GitHub / GitLab / Linear 令牌、PR 策略与孤儿工作树清理",
        title_en: "GitHub / GitLab / Linear tokens, PR merge & worktree cleanup",
        keywords: "github gitlab linear pr issue commit worktree 代码托管 提交 合并 工作树 清理",
    },
    DeepSettingItem {
        tab: SettingsTab::Devices,
        title_zh: "iOS 模拟器、Android 设备与本地 localhost 网页预览",
        title_en: "iOS Simulator, Android Emulator & localhost preview",
        keywords: "ios simulator android adb ssh localhost preview 模拟器 真机 截图 预览 端口",
    },
    DeepSettingItem {
        tab: SettingsTab::Remote,
        title_zh: "局域网扫码配对二维码、多远端环境与限时访客分享链接",
        title_en: "LAN QR pairing, saved environments & guest web share links",
        keywords: "remote pair qr lan share guest 远端 配对 二维码 局域网 访客 分享",
    },
    DeepSettingItem {
        tab: SettingsTab::Logs,
        title_zh: "诊断日志、缓存清理、防休眠、旧会话导入与软件更新",
        title_en: "Diagnostics, storage cleanup, caffeinate, import & updates",
        keywords: "logs cache cleanup caffeinate awake import update about version 日志 清理 缓存 防休眠 导入 更新 关于 版本",
    },
];

/// Keep the keyboard destination in the same order as the visible search results.
struct SettingsMatches {
    pages: Vec<SettingsTab>,
    items: Vec<&'static DeepSettingItem>,
}

impl SettingsMatches {
    fn for_query(query: &str) -> Self {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Self { pages: Vec::new(), items: Vec::new() };
        }
        let matches = |text: &str| text.to_lowercase().contains(&query);
        let pages = SettingsTab::ALL
            .into_iter()
            .filter(|&tab| {
                [UiLocale::Zh, UiLocale::En].into_iter().any(|locale| {
                    matches(tab.title(locale))
                        || (tab == tab.rail_parent() && matches(tab.rail_title(locale)))
                })
            })
            .collect();
        let items = SETTINGS_DEEP_INDEX
            .iter()
            .filter(|item| [item.title_zh, item.title_en, item.keywords].into_iter().any(matches))
            .take(8)
            .collect();
        Self { pages, items }
    }

    fn first(&self) -> Option<SettingsTab> {
        self.pages
            .first()
            .copied()
            .or_else(|| self.items.first().map(|item| item.tab))
    }
}

impl KarWorkbench {
    pub fn open_settings(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        self.close_all_overlays(cx);
        self.settings_open = true;
        self.select_settings_tab(tab, cx);
    }

    pub fn toggle_settings(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        if self.settings_open && self.active_settings_tab == tab {
            self.close_all_overlays(cx);
        } else {
            self.open_settings(tab, cx);
        }
    }

    pub fn select_settings_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        self.active_settings_tab = tab;
        match tab {
            SettingsTab::Schedules => {
                self.refresh_schedules(crate::remote_link::now_epoch_ms(), true, cx)
            }
            SettingsTab::SkillsMcp => {
                let rt = self.active_or_selected_runtime_id().to_owned();
                self.bridge.send(KarCommand::ProbeWorkspaceInventory {
                    workspace: self.effective_workspace_dir(),
                    runtime_id: rt,
                });
            }
            SettingsTab::Usage => {
                self.bridge.send(KarCommand::RefreshUsageAnalytics {
                    providers: self.providers.clone(),
                });
            }
            SettingsTab::Voice => {
                self.microphone_name = crate::hardware::Recording::microphone();
                self.refresh_voice();
            }
            SettingsTab::Devices => {
                self.device_command(None);
                if self.detected_dev_ports.is_empty() {
                    self.detected_dev_ports =
                        crate::forge_devices_voice::probe_local_dev_ports(&[]);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    pub(crate) fn settings_switch(
        id: &'static str,
        label: &'static str,
        checked: bool,
        change: fn(&mut Self, &mut Context<Self>),
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let phase = pro_spring_f32((id, 0usize), u8::from(checked) as f32, 0.16, window, cx)
            .clamp(0., 1.);
        let interaction = window.use_keyed_state((id, 1usize), cx, |window, cx| {
            let focus = cx.focus_handle().tab_stop(true);
            let blur = cx.on_blur(
                &focus,
                window,
                |state: &mut SettingsSwitchInteraction, _, cx| {
                    state.stretch.set(0., cx);
                    cx.notify();
                },
            );
            SettingsSwitchInteraction {
                focus,
                hovered: false,
                stretch: Slide::snappy(0.),
                _blur: blur,
            }
        });
        let (focus, hovered, stretch) = interaction.update(cx, |state, cx| {
            (
                state.focus.clone(),
                state.hovered,
                state.stretch.get(window, cx).clamp(0., 1.) * 3.,
            )
        });
        let keyboard_focus = focus.is_focused(window) && window.last_input_was_keyboard();
        let (ink, thumb) = if s.dark { (white(), black()) } else { (black(), white()) };
        let hover_tint = if hovered { 0.12 } else { 0. };
        let track = hsla(0., 0., 0.5, 1.).blend(ink.opacity(0.78 * phase + hover_tint));
        let control = div()
            .id(id)
            .role(Role::Switch)
            .aria_label(label)
            .aria_toggled(checked.into())
            .track_focus(&focus)
            .tab_stop(true)
            .size(px(32.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(
                div()
                    .relative()
                    .w(px(30.))
                    .h(px(18.))
                    .rounded_full()
                    .bg(track)
                    .child(
                        div()
                            .absolute()
                            .left(px(2. + phase * (12. - stretch)))
                            .top(px(2.))
                            .w(px(14. + stretch))
                            .h(px(14.))
                            .rounded_full()
                            .bg(thumb),
                    )
                    .when(keyboard_focus, |track| {
                        track.child(
                            div()
                                .absolute()
                                .top(px(-2.))
                                .bottom(px(-2.))
                                .left(px(-2.))
                                .right(px(-2.))
                                .rounded_full()
                                .border_1()
                                .border_color(ink),
                        )
                    }),
            )
            .on_key_down(move |ev, window, cx| {
                if ev.keystroke.key == "tab" {
                    if ev.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    window.prevent_default();
                    cx.stop_propagation();
                }
            })
            .on_click(cx.listener(move |wb, _, _, cx| change(wb, cx)));
        interaction.update(cx, |_, cx| {
            press(control, cx, |state| &mut state.stretch)
                .on_hover(cx.listener(|state, hovered, _, cx| {
                    state.hovered = *hovered;
                    if !hovered {
                        state.stretch.set(0., cx);
                    }
                    cx.notify();
                }))
                .into_any_element()
        })
    }

    fn render_general_settings(
        &self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let stacked = f32::from(window.viewport_size().width) < 620.;

        // 1. Notifications
        let notif_strip = SelectionStrip::options(
            "general-notif-level",
            loc.pick("系统通知提醒", "System notifications"),
            Group::Choices,
            NotificationLevel::ALL.map(|lv| (lv, lv.label(loc))),
            self.prefs.system_notifications,
            s,
            cx,
            |wb, lv, _, cx| {
                wb.prefs.system_notifications = lv;
                wb.save_prefs();
                cx.notify();
            },
        );
        let in_app_sw = Self::settings_switch(
            "general-in-app-notif",
            loc.pick("应用内气泡提醒", "In-app toast notifications"),
            self.prefs.in_app_notifications,
            |wb, cx| {
                wb.prefs.in_app_notifications = !wb.prefs.in_app_notifications;
                wb.save_prefs();
                cx.notify();
            },
            s,
            window,
            cx,
        );
        let sound_sw = Self::settings_switch(
            "general-notif-sound",
            loc.pick("通知提示音", "Notification sound"),
            self.prefs.notification_sound,
            |wb, cx| {
                wb.prefs.notification_sound = !wb.prefs.notification_sound;
                wb.save_prefs();
                cx.notify();
            },
            s,
            window,
            cx,
        );

        let notif_box = card_box(s)
            .flex()
            .flex_col()
            .child(settings_row_item(
                loc.pick("对话状态系统通知", "Thread notifications"),
                loc.pick(
                    "后台对话完成、报错或需要你审批权限时发送 macOS 系统横幅通知。",
                    "System alerts when a background thread finishes, fails, or needs approval.",
                ),
                notif_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("应用内消息气泡", "In-app notifications"),
                loc.pick(
                    "应用处于前台时展示轻量状态提示条。",
                    "Show subtle toast alerts while Kwrok has focus.",
                ),
                in_app_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("伴随系统提示音", "Notification sound"),
                loc.pick(
                    "触发系统通知时播放清脆提示音（可点击右侧发送一条测试通知）。",
                    "Play Glass sound when sending system notifications.",
                ),
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        soft_button("general-test-notif", loc.pick("测试通知", "Test"), s).on_press(
                            cx,
                            |wb, _, cx| {
                                crate::forge_devices_voice::send_macos_notification(
                                    "Kwrok 通知测试",
                                    "后台对话需要确认或已完成时将在此处提醒你",
                                    wb.prefs.notification_sound,
                                );
                                wb.notify_toast("已发送系统测试通知", cx);
                            },
                        ),
                    )
                    .child(sound_sw)
                    .into_any_element(),
                stacked,
                s,
            ));

        // 2. Sidebar & Workspace
        let sidebar_sw = Self::settings_switch(
            "settings-show-sidebar",
            loc.pick("显示左侧导航栏", "Show sidebar"),
            self.sidebar_open,
            |wb, cx| wb.toggle_sidebar(cx),
            s,
            window,
            cx,
        );
        let group_proj_sw = Self::settings_switch(
            "general-group-by-proj",
            loc.pick("按项目文件夹分组折叠对话", "Group threads by project"),
            self.prefs.sidebar_group_by_project,
            |wb, cx| {
                wb.prefs.sidebar_group_by_project = !wb.prefs.sidebar_group_by_project;
                wb.save_prefs();
                cx.notify();
            },
            s,
            window,
            cx,
        );
        let running_first_switch = Self::settings_switch(
            "general-working-section",
            loc.pick("优先显示进行中的对话", "Show working chats first"),
            self.prefs.sidebar_working_section,
            |wb, cx| {
                wb.prefs.sidebar_working_section = !wb.prefs.sidebar_working_section;
                wb.save_prefs();
                cx.notify();
            },
            s,
            window,
            cx,
        );
        let proj_order_strip = SelectionStrip::options(
            "general-proj-order",
            loc.pick("项目排序方式", "Project order"),
            Group::Choices,
            ProjectSortOrder::ALL.map(|o| (o, o.label(loc))),
            self.prefs.sidebar_project_order,
            s,
            cx,
            |wb, order, _, cx| {
                wb.prefs.sidebar_project_order = order;
                wb.save_prefs();
                cx.notify();
            },
        );
        let hidden_files_sw = Self::settings_switch(
            "settings-hidden-files",
            loc.pick("文件树显示隐藏文件", "Show hidden files"),
            self.tree_show_hidden,
            |wb, cx| {
                wb.tree_show_hidden = !wb.tree_show_hidden;
                wb.prefs.tree_show_hidden = wb.tree_show_hidden;
                wb.save_prefs();
                wb.bridge.send(KarCommand::RefreshWorkspaceTree {
                    workspace: wb.effective_workspace_dir(),
                    show_hidden: wb.tree_show_hidden,
                });
                cx.notify();
            },
            s,
            window,
            cx,
        );

        let unpin_sw = Self::settings_switch(
            "general-confirm-unpin",
            loc.pick("取消置顶前二次确认", "Unpin confirmation"),
            self.prefs.confirm_unpin,
            |wb, cx| {
                wb.prefs.confirm_unpin = !wb.prefs.confirm_unpin;
                wb.save_prefs();
                cx.notify();
            },
            s,
            window,
            cx,
        );
        let archive_sw = Self::settings_switch(
            "general-confirm-archive",
            loc.pick("归档对话前二次确认", "Archive confirmation"),
            self.prefs.confirm_archive,
            |wb, cx| {
                wb.prefs.confirm_archive = !wb.prefs.confirm_archive;
                wb.save_prefs();
                cx.notify();
            },
            s,
            window,
            cx,
        );
        let delete_sw = Self::settings_switch(
            "general-confirm-delete",
            loc.pick("删除对话前确认", "Delete confirmation"),
            self.prefs.confirm_delete,
            |wb, cx| {
                wb.prefs.confirm_delete = !wb.prefs.confirm_delete;
                wb.save_prefs();
                cx.notify();
            },
            s,
            window,
            cx,
        );
        let term_close_sw = Self::settings_switch(
            "general-confirm-terminal-close",
            loc.pick("关闭运行中的终端前确认", "Terminal close confirmation"),
            self.prefs.confirm_terminal_close,
            |wb, cx| {
                wb.prefs.confirm_terminal_close = !wb.prefs.confirm_terminal_close;
                wb.save_prefs();
                cx.notify();
            },
            s,
            window,
            cx,
        );
        let quit_strip = SelectionStrip::options(
            "general-quit-shortcut",
            loc.pick("退出防误触", "Quit shortcut"),
            Group::Choices,
            QuitShortcutMode::ALL.map(|m| (m, m.label(loc))),
            self.prefs.quit_shortcut_mode,
            s,
            cx,
            |wb, mode, _, cx| {
                wb.prefs.quit_shortcut_mode = mode;
                wb.save_prefs();
                cx.notify();
            },
        );

        let more_open_state =
            window.use_keyed_state("settings-general-more-open", cx, |_, _| false);
        let more_open = *more_open_state.read(cx);
        let more_toggle_state = more_open_state.clone();

        let sidebar_box = card_box(s)
            .flex()
            .flex_col()
            .child(settings_row_item(
                loc.pick("显示左侧会话栏", "Show sidebar"),
                loc.pick("在主工作区左侧常驻显示项目筛选与历史对话列表。", "Keep projects and threads visible beside the stage."),
                sidebar_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("把进行中的对话置顶", "Show working chats first"),
                loc.pick("智能体正在思考或等待确认的对话自动排在最前面。", "Move working chats to the top of the list."),
                running_first_switch,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("按项目文件夹整理对话", "Group by project"),
                loc.pick("将同一个代码仓库下的多条对话自动归入同一个文件夹。", "Group threads under collapsible project folders."),
                group_proj_sw,
                stacked,
                s,
            ));

        let confirm_box = card_box(s)
            .flex()
            .flex_col()
            .child(settings_row_item(
                loc.pick("删除对话前确认", "Delete confirmation"),
                loc.pick("移除本地对话记录前开启二次确认保护，防止手滑误删。", "Protect threads from accidental deletion."),
                delete_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("退出应用防误触（⌘Q）", "Quit shortcut safety"),
                loc.pick("防止切换窗口时误按 ⌘Q 打断正在执行的任务。", "Prevent accidental ⌘Q while tasks are running."),
                quit_strip.into_any_element(),
                stacked,
                s,
            ));

        let advanced_box = card_box(s)
            .flex()
            .flex_col()
            .child(settings_row_item(
                loc.pick("项目列表排序方式", "Project order"),
                loc.pick("侧边栏项目文件夹的排列顺序。", "Order of projects in the sidebar."),
                proj_order_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("文件列表显示隐藏文件", "Show hidden files"),
                loc.pick("在文件浏览器中显示以点开头的配置文件（如 .env、.github）。", "Include dotfiles in the workspace file tree."),
                hidden_files_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("取消置顶前二次确认", "Unpin confirmation"),
                loc.pick("将置顶的重要对话移出前提示确认。", "Confirm before unpinning a thread."),
                unpin_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("归档对话前二次确认", "Archive confirmation"),
                loc.pick("点击归档按钮时需再点一次确认。", "Confirm before archiving a thread."),
                archive_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("关闭运行中的终端前确认", "Terminal close confirmation"),
                loc.pick("终端仍有命令运行时，关闭前提示确认。", "Ask before closing active terminals."),
                term_close_sw,
                stacked,
                s,
            ));

        // 4. Language & Time Format
        let language_btn = soft_button(
            "settings-ui-language",
            loc.pick("简体中文 · 点击切换 English", "English · Switch to 中文"),
            s,
        )
        .on_press(cx, |wb, window, cx| wb.toggle_ui_locale(window, cx));

        let time_24h_sw = Self::settings_switch(
            "general-24h-time",
            loc.pick("使用 24 小时制时间", "Use 24-hour clock"),
            self.prefs.time_format_24h,
            |wb, cx| {
                wb.prefs.time_format_24h = !wb.prefs.time_format_24h;
                wb.save_prefs();
                cx.notify();
            },
            s,
            window,
            cx,
        );

        let locale_box = card_box(s)
            .flex()
            .flex_col()
            .child(settings_row_item(
                loc.pick("界面显示语言", "Interface language"),
                loc.pick("即时切换工作台导航、办公套件工具栏与提示文案语言。", "Switch workbench and Office editor language."),
                language_btn.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("24 小时制时间戳", "24-hour timestamps"),
                loc.pick("在对话历史、定时任务与诊断日志中使用 24 小时格式。", "Format timestamps in 24-hour notation."),
                time_24h_sw,
                stacked,
                s,
            ));

        div()
            .id("settings-general-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                settings_page_shell()
                    .child(
                        div()
                            .child(settings_section_title(loc.pick("语言与时间显示", "Language & Time"), s))
                            .child(locale_box),
                    )
                    .child(
                        div()
                            .child(settings_section_title(loc.pick("对话列表与侧边栏", "Sidebar"), s))
                            .child(sidebar_box),
                    )
                    .child(
                        div()
                            .child(settings_section_title(loc.pick("消息通知提醒", "Notifications"), s))
                            .child(notif_box),
                    )
                    .child(
                        div()
                            .child(settings_section_title(loc.pick("防误触保护", "Safety"), s))
                            .child(confirm_box),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                soft_button(
                                    "general-toggle-more",
                                    if more_open {
                                        loc.pick("收起更多细节选项 ▴", "Hide extra options ▴")
                                    } else {
                                        loc.pick("展开更多细节选项（隐藏文件 / 排序 / 细粒度确认） ▾", "More options ▾")
                                    },
                                    s,
                                )
                                .on_press(cx, move |_, _, cx| {
                                    more_toggle_state.update(cx, |v, cx| {
                                        *v = !*v;
                                        cx.notify();
                                    });
                                }),
                            )
                            .when(more_open, |d| d.child(advanced_box)),
                    ),
            )
            .into_any_element()
    }

    fn settings_header(&self, tab: SettingsTab, s: &StudioPalette, cx: &mut Context<Self>) -> Div {
        let loc = self.ui_locale;
        let action: Option<AnyElement> = match tab {
            SettingsTab::General => Some(
                outline_button(
                    "settings-restore-defaults",
                    loc.pick("恢复默认偏好", "Restore defaults"),
                    s,
                )
                .on_press(cx, |wb, _, cx| {
                    let keep_settled = wb.prefs.settled_chats.clone();
                    wb.prefs = DesktopPreferences {
                        settled_chats: keep_settled,
                        ..DesktopPreferences::default()
                    };
                    wb.save_prefs();
                    crate::apply_theme_and_typography(wb.theme_mode, &wb.prefs, cx);
                    wb.notify_toast("已将设备设置恢复为默认值", cx);
                })
                .into_any_element(),
            ),
            SettingsTab::Agents => Some(
                text_button(
                    "agents-recheck",
                    if self.providers_probing {
                        loc.pick("检查中…", "Checking…")
                    } else {
                        loc.pick("重新扫描智能体", "Rescan agents")
                    },
                    s,
                )
                .disabled(self.providers_probing)
                .on_press(cx, |wb, _, cx| wb.refresh_agents(cx))
                .into_any_element(),
            ),
            SettingsTab::SkillsMcp | SettingsTab::Usage | SettingsTab::Devices => Some(
                text_button("settings-refresh", loc.pick("刷新", "Refresh"), s)
                    .on_press(cx, move |wb, _, cx| wb.select_settings_tab(tab, cx))
                    .into_any_element(),
            ),
            SettingsTab::Schedules => Some(
                soft_button(
                    "sched-toggle-creator",
                    if self.schedule_creator_open {
                        loc.pick("收起编辑器", "Close editor")
                    } else {
                        loc.pick("新建定时任务", "New task")
                    },
                    s,
                )
                .disabled(self.schedule_save_pending)
                .on_press(cx, |wb, window, cx| wb.toggle_schedule_form(window,cx))
                .into_any_element(),
            ),
            _ => None,
        };

        div()
            .w_full()
            .flex_none()
            .h(px(48.))
            .pl(px(20.))
            .pr(px(10.))
            .flex()
            .items_center()
            .gap(px(10.))
            .border_b_1()
            .border_color(s.line_soft)
            .text_size(s.font_size(FS_UI))
            .child(div().text_color(s.muted).child(loc.pick("设置", "Settings")))
            .child(pro_icon(ProIcon::ChevronRight, 12., s.faint))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.ink)
                    .child(tab.title(loc)),
            )
            .when_some(action, |row, el| row.child(el))
            .child(
                icon_button(
                    "settings-close",
                    ProIcon::Close,
                    loc.pick("关闭设置 · Esc", "Close settings · Esc"),
                    s,
                )
                .on_press(cx, |wb, window, cx| {
                    wb.close_all_overlays(cx);
                    window.focus(&wb.focus_handle, cx);
                }),
            )
    }

    pub(crate) fn render_settings_modal(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.settings_open {
            return self.render_modal(
                "kar-unified-settings-modal",
                false,
                44.,
                div().into_any_element(),
                |this, _, cx| this.close_all_overlays(cx),
                cx,
            );
        }

        let active_tab = self.active_settings_tab;
        let loc = self.ui_locale;
        let viewport = window.viewport_size();
        let width = f32::from(viewport.width);
        let height = f32::from(viewport.height);
        let small_screen = width < 768.;
        let inset = if width >= 1024. {
            36.
        } else if width >= 640. {
            20.
        } else {
            0.
        };
        let card_h = (height - 2. * inset).clamp(1., 980.);
        let rail_top_padding = if cfg!(target_os = "macos") && !window.is_fullscreen() {
            let card_top = (height - card_h) * 0.5;
            (40. - card_top).max(10.)
        } else {
            10.
        };
        let expanded = window.use_keyed_state("settings-nav-expanded", cx, |_, _| false);
        let compact = small_screen && !*expanded.read(cx);
        let modal_focus = window
            .use_keyed_state("settings-focus", cx, |window, cx| {
                let handle = cx.focus_handle();
                window.focus(&handle, cx);
                handle
            })
            .read(cx)
            .clone();
        let search = window.use_keyed_state("kar-settings-search", cx, |window, cx| {
            let mut editor = Editor::new(
                loc.pick("搜索全部设置项", "Search all settings"),
                InputMode::Search,
                window,
                cx,
            );
            editor.font_size = FS_UI;
            editor.set_placeholder(loc.pick("搜索任意设置…", "Search settings…"));
            editor
        });
        let input = search.read(cx).value().trim();
        let searching = !input.is_empty();
        let matches = SettingsMatches::for_query(input);
        let pages = if !searching { &SettingsTab::PRIMARY_NAV[..] } else { &matches.pages };
        let search_editor = search.clone();
        let search_rail = expanded.clone();
        let section_focus = modal_focus.clone();
        let open_result = move |tab, wb: &mut Self, win: &mut Window, cx: &mut Context<Self>| {
            wb.select_settings_tab(tab, cx);
            search_editor.update(cx, |editor, cx| editor.set_value("", win, cx));
            search_rail.update(cx, |open, cx| {
                *open = false;
                cx.notify();
            });
            win.focus(&section_focus, cx);
        };
        let rail_active = active_tab.rail_parent();
        let nav_sensor = pro_selection_sensor();
        let mut nav_items = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(pro_pill_indicator(
                "kar-settings-rail-pill",
                rail_active.key(),
                nav_sensor.clone(),
                R_SM,
                s.sidebar_selected,
                None,
                false,
            ));

        for (index, &tab) in pages.iter().enumerate() {
            let is_primary = tab == tab.rail_parent();
            let title = if is_primary { tab.rail_title(loc) } else { tab.title(loc) };
            if !compact && !searching {
                let group = match tab {
                    SettingsTab::General => Some(loc.pick("日常体验", "Basics")),
                    SettingsTab::Agents => Some(loc.pick("AI 与扩展", "AI & Capabilities")),
                    SettingsTab::Usage => Some(loc.pick("工程与维护", "Projects & System")),
                    _ => None,
                };
                if let Some(group) = group {
                    nav_items = nav_items.child(
                        div()
                            .px(px(10.))
                            .pt(px(if index == 0 { 4. } else { 14. }))
                            .pb(px(4.))
                            .text_size(s.font_size(FS_TAG))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(s.muted)
                            .child(group),
                    );
                }
            }
            let selected = if !searching { tab == rail_active } else { tab == active_tab };
            let color = if selected { s.ink } else { s.muted };
            let icon = match tab {
                SettingsTab::General => pro_icon(ProIcon::Sliders, 15., color),
                SettingsTab::Appearance => pro_icon(ProIcon::Eye, 15., color),
                SettingsTab::Keybindings => pro_icon(ProIcon::Code, 15., color),
                SettingsTab::Agents => pro_icon(ProIcon::Users, 15., color),
                SettingsTab::SkillsMcp => render_skills_puzzle_icon(15., color),
                SettingsTab::Schedules => render_schedule_repeat_icon(15., color),
                SettingsTab::Usage => render_usage_pulse_icon(15., color),
                _ => pro_icon(ProIcon::Settings, 15., color),
            };
            let open = open_result.clone();
            nav_items = nav_items.child(
                div()
                    .relative()
                    .w_full()
                    .h(px(32.))
                    .when(selected, |d| d.child(pro_mark_selected(&nav_sensor)))
                    .child(
                        Tactile::ghost(tab.key(), s)
                            .w_full()
                            .h_full()
                            .px(px(10.))
                            .gap(px(8.))
                            .radius(R_SM)
                            .when(compact, |b| b.justify_center())
                            .tip(title)
                            .child(icon)
                            .when(!compact, |b| {
                                b.child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(s.font_size(FS_UI))
                                        .font_weight(if selected {
                                            FontWeight::MEDIUM
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(if selected { s.ink } else { s.text })
                                        .child(title),
                                )
                            })
                            .on_press(cx, move |wb, win, cx| open(tab, wb, win, cx)),
                    ),
            );
        }

        // Deep item-level search hits directly in the rail so any sub-setting is 1 click away!
        if !compact && !matches.items.is_empty() {
            nav_items = nav_items.child(
                div()
                    .px(px(10.))
                    .pt(px(12.))
                    .pb(px(4.))
                    .text_size(s.font_size(FS_TAG))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(s.muted)
                    .child(loc.pick("匹配的具体设置项", "Matching settings")),
            );
            for (hix, hit) in matches.items.iter().enumerate() {
                let target_tab = hit.tab;
                let open = open_result.clone();
                nav_items = nav_items.child(
                    Tactile::ghost(("settings-deep-hit", hix), s)
                        .w_full()
                        .py(px(6.))
                        .px(px(10.))
                        .radius(R_SM)
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(s.font_size(FS_META))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(s.ink)
                                        .child(loc.pick(hit.title_zh, hit.title_en)),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .child(
                                            div()
                                                .px(px(5.))
                                                .rounded(px(R_PILL))
                                                .bg(s.soft)
                                                .text_size(s.font_size(FS_TAG))
                                                .text_color(s.muted)
                                                .child(target_tab.title(loc)),
                                        ),
                                ),
                        )
                        .on_press(cx, move |wb, win, cx| open(target_tab, wb, win, cx)),
                );
            }
        }

        if pages.is_empty() && matches.items.is_empty() {
            nav_items = nav_items.child(
                div()
                    .px(px(10.))
                    .py(px(16.))
                    .text_size(s.font_size(FS_META))
                    .text_color(s.muted)
                    .child(loc.pick("未找到相关设置项", "No matching settings")),
            );
        }

        let search_input_focus = search.focus_handle(cx);
        let compact_search = expanded.clone();
        let search_row = div()
            .h(px(32.))
            .w_full()
            .rounded(px(R_SM))
            .bg(s.card)
            .border_1()
            .border_color(if search_input_focus.is_focused(window) {
                s.line_strong
            } else {
                s.line_soft
            })
            .flex()
            .items_center()
            .px(px(8.))
            .gap(px(7.))
            .child(pro_icon(ProIcon::Search, 14., s.muted))
            .child(div().flex_1().min_w_0().h(px(20.)).child(search.clone()))
            .when(!searching, |d| d.child(render_kbd("/", s)));

        let rail = div()
            .map(|d| {
                if small_screen && !compact {
                    d.w_full()
                } else {
                    d.w(px(if compact { 56. } else { 244. }))
                }
            })
            .h_full()
            .flex_none()
            .bg(if s.dark { s.tray } else { s.stage })
            .when(inset > 0., |d| {
                let radius = px(R_MODAL - 1.);
                d.rounded_l(radius)
                    .when(small_screen && !compact, |d| d.rounded_r(radius))
            })
            .border_r_1()
            .border_color(s.line_soft)
            .p(px(10.))
            .pt(px(rail_top_padding))
            .flex()
            .flex_col()
            .gap(px(8.))
            .when(!compact, |d| d.child(search_row))
            .when(compact, |d| {
                d.child(
                    icon_button(
                        "settings-expand-search",
                        ProIcon::Search,
                        loc.pick("搜索设置", "Search settings"),
                        s,
                    )
                    .on_press(cx, move |_, win, cx| {
                        compact_search.update(cx, |open, cx| {
                            *open = true;
                            cx.notify();
                        });
                        win.focus(&search_input_focus, cx);
                    }),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .id("settings-rail-scroll")
                    .overflow_y_scroll()
                    .child(nav_items),
            );

        let content: AnyElement = match active_tab {
            SettingsTab::General => self.render_general_settings(s, window, cx),
            SettingsTab::Appearance => self.render_appearance_settings(s, window, cx),
            SettingsTab::Keybindings => self.render_keybindings_settings(s, window, cx),
            SettingsTab::Voice => self.render_voice_settings(s, window, cx),
            SettingsTab::Agents => div()
                .id("kar-settings-agents-scroll")
                .size_full()
                .overflow_y_scroll()
                .child(self.render_agents_panel(s, window, cx))
                .into_any_element(),
            SettingsTab::SkillsMcp => self.render_skills_mcp_panel(s, window, cx),
            SettingsTab::Usage => self.render_usage_panel(s, window, cx),
            SettingsTab::Schedules => div()
                .id("kar-settings-schedules-scroll")
                .size_full()
                .overflow_y_scroll()
                .child(self.render_schedules_panel(s, window, cx))
                .into_any_element(),
            SettingsTab::ProjectsGit => self.render_projects_git_settings(s, window, cx),
            SettingsTab::Devices => self.render_devices_preview_settings(s, window, cx),
            SettingsTab::Remote => div()
                .id("kar-settings-remote-scroll")
                .size_full()
                .overflow_y_scroll()
                .child(self.render_remote_panel(s, window, cx))
                .into_any_element(),
            SettingsTab::Logs => self.render_maintenance_about_settings(s, window, cx),
        };

        let sub_strip: Option<Div> = match rail_active {
            SettingsTab::Schedules => Some(
                div()
                    .px(px(20.))
                    .py(px(10.))
                    .border_b_1()
                    .border_color(s.line_soft)
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(SelectionStrip::options(
                        "settings-automation-subtabs",
                        loc.pick("自动化与协同分类", "Automation & Sync"),
                        Group::Tabs,
                        [
                            (SettingsTab::Schedules, loc.pick("定时任务", "Schedules")),
                            (SettingsTab::Remote, loc.pick("手机扫码协同", "Remote Sync")),
                            (SettingsTab::Voice, loc.pick("语音听写", "Voice Input")),
                        ],
                        active_tab,
                        s,
                        cx,
                        |wb, t, _, cx| wb.select_settings_tab(t, cx),
                    )),
            ),
            SettingsTab::Usage => Some(
                div()
                    .px(px(20.))
                    .py(px(10.))
                    .border_b_1()
                    .border_color(s.line_soft)
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(SelectionStrip::options(
                        "settings-system-subtabs",
                        loc.pick("工程与维护分类", "Projects & System"),
                        Group::Tabs,
                        [
                            (SettingsTab::Usage, loc.pick("用量统计", "Usage")),
                            (SettingsTab::ProjectsGit, loc.pick("项目与 Git", "Projects & Git")),
                            (SettingsTab::Devices, loc.pick("移动与预览", "Devices & Preview")),
                            (SettingsTab::Logs, loc.pick("清理与更新", "Maintenance & About")),
                        ],
                        active_tab,
                        s,
                        cx,
                        |wb, t, _, cx| wb.select_settings_tab(t, cx),
                    )),
            ),
            _ => None,
        };

        let focus_before_close = modal_focus.clone();
        let key_search = search.clone();
        let key_expand = expanded.clone();
        let card = div()
            .id("settings-panel")
            .track_focus(&modal_focus)
            .capture_key_down(cx.listener(move |wb, ev: &KeyDownEvent, win, cx| {
                let focus = key_search.focus_handle(cx);
                let mods = ev.keystroke.modifiers;
                if ev.keystroke.key == "escape" {
                    if ev.is_held {
                        cx.stop_propagation();
                        return;
                    }
                    if !key_search.read(cx).value().is_empty() {
                        key_search.update(cx, |ed, cx| ed.set_value("", win, cx));
                        win.focus(&focus, cx);
                    } else if *key_expand.read(cx) {
                        key_expand.update(cx, |open, cx| {
                            *open = false;
                            cx.notify();
                        });
                        win.focus(&focus_before_close, cx);
                    } else if wb.open_menu.is_none() {
                        wb.close_all_overlays(cx);
                        win.focus(&wb.focus_handle, cx);
                    } else {
                        return;
                    }
                    cx.stop_propagation();
                } else if ev.keystroke.key == "/"
                    && !mods.platform
                    && !mods.control
                    && !mods.alt
                    && focus_before_close.is_focused(win)
                {
                    if small_screen {
                        key_expand.update(cx, |open, cx| {
                            *open = true;
                            cx.notify();
                        });
                    }
                    win.focus(&focus, cx);
                    cx.stop_propagation();
                } else if ev.keystroke.key == "enter" && focus.is_focused(win) {
                    let input = key_search.read(cx).value();
                    if let Some(tab) = SettingsMatches::for_query(input).first() {
                        open_result(tab, wb, win, cx);
                    }
                    cx.stop_propagation();
                }
            }))
            .w(px((width - 2. * inset).clamp(1., 1280.)))
            .h(px(card_h))
            .max_w_full()
            .when(inset > 0., |d| d.rounded(px(R_MODAL)).border_1())
            .bg(s.surface)
            .border_color(s.line)
            .shadow(s.popover_shadow())
            .overflow_hidden()
            .flex()
            .flex_row()
            .child(rail)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.settings_header(active_tab, s, cx))
                    .when_some(sub_strip, |d, bar| d.child(bar))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .child(pro_view_transition_fill(
                                "kar-settings-pane-transition",
                                active_tab.key(),
                                content,
                            )),
                    ),
            )
            .into_any_element();

        let weak = cx.weak_entity();
        pro_presence("kar-unified-settings-modal", true, 0.15, 0.12, move |phase| {
            div()
                .absolute()
                .inset_0()
                .child(
                    deferred(
                        div()
                            .id("settings-backdrop")
                            .size_full()
                            .occlude()
                            .bg(hsla(0., 0., 0., 0.45 * phase))
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                if let Some(wb) = weak.upgrade() {
                                    wb.update(cx, |wb, cx| {
                                        wb.close_all_overlays(cx);
                                        window.focus(&wb.focus_handle, cx);
                                    });
                                }
                            })
                            .child(
                                div()
                                    .opacity(phase)
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .child(card),
                            ),
                    )
                    .with_priority(4),
                )
                .into_any_element()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{SettingsMatches, SettingsTab};

    #[test]
    fn settings_search_uses_visible_labels_and_has_no_empty_destination() {
        for (input, target) in [
            ("  ", None),
            ("missing-setting-9377", None),
            ("models & ACCESS", Some(SettingsTab::Agents)),
            ("工程与维护", Some(SettingsTab::Usage)),
            ("Voice & Dictation", Some(SettingsTab::Voice)),
            ("whisper", Some(SettingsTab::Voice)),
        ] {
            assert_eq!(SettingsMatches::for_query(input).first(), target, "{input}");
        }
    }
}
