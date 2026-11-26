//! Workspace settings with a 12-page structured KUI navigation rail and deep item-level `/` search.

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
        render_kbd, render_radar_link_icon, render_schedule_repeat_icon, render_skills_puzzle_icon,
        render_terminal_logs_icon, render_usage_pulse_icon, settings_divider, settings_page_shell,
        settings_row_item, settings_section_title,
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
            Self::General => "通用",
            Self::Appearance => "外观与主题",
            Self::Keybindings => "快捷键",
            Self::Voice => "语音输入",
            Self::Agents => "智能体与账号",
            Self::SkillsMcp => "技能与 MCP",
            Self::Usage => "用量与缓存",
            Self::Schedules => "定时自动化",
            Self::ProjectsGit => "项目与代码托管",
            Self::Devices => "移动设备与预览",
            Self::Remote => "远端配对与分享",
            Self::Logs => "维护、日志与关于",
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
        let track = if checked { s.brand_action } else { s.line_strong };
        let track = if hovered {
            track.blend(s.ink.opacity(0.12))
        } else {
            track
        };
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
                            .bg(if checked { s.on_brand_action } else { s.surface }),
                    )
                    .when(keyboard_focus, |track| {
                        track.child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded_full()
                                .border_1()
                                .border_color(s.ink),
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

        let sidebar_box = card_box(s)
            .flex()
            .flex_col()
            .child(settings_row_item(
                loc.pick("显示左侧会话栏", "Show sidebar"),
                loc.pick("在主工作区左侧常驻显示项目筛选器与会话列表。", "Keep projects and threads visible beside the stage."),
                sidebar_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("优先显示进行中的对话", "Show working chats first"),
                loc.pick("启用后，进行中的对话排在各自列表的最前面。", "Move working chats to the top of their list or project group."),
                running_first_switch,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("按项目文件夹分组", "Project grouping"),
                loc.pick("把同一代码仓库（含其隔离工作树）下的对话收纳为可折叠项目组。", "Group threads under collapsible repository folders."),
                group_proj_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("项目排列顺序", "Project order"),
                loc.pick("控制侧边栏项目切换菜单与分组的排序规则。", "Order of projects in the sidebar and project picker."),
                proj_order_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("显示以点开头的隐藏文件", "Show dotfiles in file tree"),
                loc.pick("在工作区文件浏览器与画廊中包含 .github、.env 等隐藏条目（重启后保持）。", "Include dotfiles in the workspace file tree."),
                hidden_files_sw,
                stacked,
                s,
            ));

        // 3. Confirmations & Quit Safety
        let unpin_sw = Self::settings_switch(
            "confirm-unpin-sw",
            loc.pick("取消置顶确认", "Unpin confirmation"),
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
            "confirm-archive-sw",
            loc.pick("归档对话确认", "Archive confirmation"),
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
            "confirm-delete-sw",
            loc.pick("删除对话确认", "Delete confirmation"),
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
            "confirm-term-close-sw",
            loc.pick("关闭运行中终端确认", "Terminal close confirmation"),
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
            "confirm-quit-mode",
            loc.pick("退出快捷键防误触", "Quit shortcut behavior"),
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

        let confirm_box = card_box(s)
            .flex()
            .flex_col()
            .child(settings_row_item(
                loc.pick("取消置顶确认", "Unpin confirmation"),
                loc.pick("从置顶区移除重要对话前要求二次点击确认。", "Require confirmation before unpinning a thread."),
                unpin_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("归档对话确认", "Archive confirmation"),
                loc.pick("点击内联归档按钮时要求再次点击确认，避免误收起对话。", "Require a second click before archiving a thread."),
                archive_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("删除对话确认", "Delete confirmation"),
                loc.pick("永久移除本地对话记录前开启撤销缓冲保护。", "Protect threads from accidental deletion."),
                delete_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("终止运行中终端确认", "Terminal close confirmation"),
                loc.pick("关闭仍有活跃子进程运行的交互终端前提示确认。", "Ask before stopping active PTY terminal processes."),
                term_close_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("退出快捷键（⌘Q）防误触", "Quit shortcut safety"),
                loc.pick("防止编辑或切换窗口时误触 ⌘Q 中断正在执行的智能体任务。", "Prevent accidental ⌘Q while agents are running."),
                quit_strip.into_any_element(),
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
                            .child(settings_section_title(loc.pick("通知提醒", "Notifications"), s))
                            .child(notif_box),
                    )
                    .child(
                        div()
                            .child(settings_section_title(loc.pick("侧边栏与工作区", "Sidebar & Workspace"), s))
                            .child(sidebar_box),
                    )
                    .child(
                        div()
                            .child(settings_section_title(loc.pick("二次确认与退出保护", "Confirmations & Quit Safety"), s))
                            .child(confirm_box),
                    )
                    .child(
                        div()
                            .child(settings_section_title(loc.pick("语言与时间格式", "Language & Time"), s))
                            .child(locale_box),
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
            .text_size(px(FS_UI))
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
        let query = search.read(cx).value().trim().to_lowercase();

        let deep_hits: Vec<&DeepSettingItem> = if query.is_empty() {
            Vec::new()
        } else {
            SETTINGS_DEEP_INDEX
                .iter()
                .filter(|it| {
                    it.title_zh.to_lowercase().contains(&query)
                        || it.title_en.to_lowercase().contains(&query)
                        || it.keywords.to_lowercase().contains(&query)
                })
                .collect()
        };

        let nav_sensor = pro_selection_sensor();
        let mut nav_items = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(pro_pill_indicator(
                "kar-settings-rail-pill",
                active_tab.key(),
                nav_sensor.clone(),
                R_SM,
                s.sidebar_selected,
                None,
                false,
            ));

        let mut results = 0usize;
        for (index, tab) in SettingsTab::ALL.into_iter().enumerate() {
            let tab_matches_deep = deep_hits.iter().any(|h| h.tab == tab);
            if !query.is_empty()
                && !tab.label().to_lowercase().contains(&query)
                && !tab.title(UiLocale::En).to_lowercase().contains(&query)
                && !tab_matches_deep
            {
                continue;
            }
            if !compact && query.is_empty() {
                let group = match tab {
                    SettingsTab::General => Some(loc.pick("偏好与交互", "Preferences")),
                    SettingsTab::Agents => Some(loc.pick("智能体与自动化", "Agents")),
                    SettingsTab::ProjectsGit => Some(loc.pick("代码、设备与连接", "Code & Connections")),
                    SettingsTab::Logs => Some(loc.pick("系统维护", "System")),
                    _ => None,
                };
                if let Some(group) = group {
                    nav_items = nav_items.child(
                        div()
                            .px(px(10.))
                            .pt(px(if index == 0 { 4. } else { 14. }))
                            .pb(px(4.))
                            .text_size(px(FS_TAG))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(s.muted)
                            .child(group),
                    );
                }
            }
            results += 1;
            let selected = tab == active_tab;
            let color = if selected { s.ink } else { s.muted };
            let icon = match tab {
                SettingsTab::General => pro_icon(ProIcon::Sliders, 15., color),
                SettingsTab::Appearance => pro_icon(ProIcon::Eye, 15., color),
                SettingsTab::Keybindings => pro_icon(ProIcon::Code, 15., color),
                SettingsTab::Voice => pro_icon(ProIcon::Sparkles, 15., color),
                SettingsTab::Agents => pro_icon(ProIcon::Users, 15., color),
                SettingsTab::SkillsMcp => render_skills_puzzle_icon(15., color),
                SettingsTab::Usage => render_usage_pulse_icon(15., color),
                SettingsTab::Schedules => render_schedule_repeat_icon(15., color),
                SettingsTab::ProjectsGit => pro_icon(ProIcon::GitBranch, 15., color),
                SettingsTab::Devices => pro_icon(ProIcon::Expand, 15., color),
                SettingsTab::Remote => render_radar_link_icon(15., color),
                SettingsTab::Logs => render_terminal_logs_icon(15., color),
            };
            let query_editor = search.clone();
            let fold = expanded.clone();
            let section_focus = modal_focus.clone();
            nav_items = nav_items.child(
                div()
                    .relative()
                    .w_full()
                    .h(px(32.))
                    .when(selected, |d| d.child(pro_mark_selected(&nav_sensor)))
                    .child(
                        Tactile::ghost(("settings-nav", index), s)
                            .w_full()
                            .h_full()
                            .px(px(10.))
                            .gap(px(8.))
                            .radius(R_SM)
                            .when(compact, |b| b.justify_center())
                            .tip(tab.title(loc))
                            .child(icon)
                            .when(!compact, |b| {
                                b.child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(FS_UI))
                                        .font_weight(if selected {
                                            FontWeight::MEDIUM
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(if selected { s.ink } else { s.text })
                                        .child(tab.title(loc)),
                                )
                            })
                            .on_press(cx, move |wb, win, cx| {
                                wb.select_settings_tab(tab, cx);
                                query_editor.update(cx, |ed, cx| ed.set_value("", win, cx));
                                fold.update(cx, |open, cx| {
                                    *open = false;
                                    cx.notify();
                                });
                                win.focus(&section_focus, cx);
                            }),
                    ),
            );
        }

        // Deep item-level search hits directly in the rail so any sub-setting is 1 click away!
        if !compact && !deep_hits.is_empty() {
            nav_items = nav_items.child(
                div()
                    .px(px(10.))
                    .pt(px(12.))
                    .pb(px(4.))
                    .text_size(px(FS_TAG))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(s.muted)
                    .child(loc.pick("匹配的具体设置项", "Matching settings")),
            );
            for (hix, hit) in deep_hits.iter().take(8).enumerate() {
                let target_tab = hit.tab;
                let q_ed = search.clone();
                let sec_focus = modal_focus.clone();
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
                                        .text_size(px(FS_META))
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
                                                .text_size(px(FS_TAG))
                                                .text_color(s.muted)
                                                .child(target_tab.title(loc)),
                                        ),
                                ),
                        )
                        .on_press(cx, move |wb, win, cx| {
                            wb.select_settings_tab(target_tab, cx);
                            q_ed.update(cx, |ed, cx| ed.set_value("", win, cx));
                            win.focus(&sec_focus, cx);
                        }),
                );
            }
        }

        if results == 0 && deep_hits.is_empty() {
            nav_items = nav_items.child(
                div()
                    .px(px(10.))
                    .py(px(16.))
                    .text_size(px(FS_META))
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
            .when(query.is_empty(), |d| d.child(render_kbd("/", s)));

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
                    let query = key_search.read(cx).value().trim().to_lowercase();
                    let next_tab = SettingsTab::ALL
                        .into_iter()
                        .find(|tab| {
                            tab.label().to_lowercase().contains(&query)
                                || tab.title(UiLocale::En).to_lowercase().contains(&query)
                        })
                        .or_else(|| {
                            SETTINGS_DEEP_INDEX
                                .iter()
                                .find(|it| {
                                    it.title_zh.to_lowercase().contains(&query)
                                        || it.title_en.to_lowercase().contains(&query)
                                        || it.keywords.to_lowercase().contains(&query)
                                })
                                .map(|it| it.tab)
                        });
                    if let Some(first) = next_tab {
                        wb.select_settings_tab(first, cx);
                        key_search.update(cx, |ed, cx| ed.set_value("", win, cx));
                        key_expand.update(cx, |open, cx| {
                            *open = false;
                            cx.notify();
                        });
                        win.focus(&focus_before_close, cx);
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
