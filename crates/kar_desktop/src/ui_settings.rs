//! Workspace settings with a searchable KUI navigation rail.

use crate::ui_motion::selection::{Group, SelectionStrip};
use gpui::{prelude::*, *};
use kui::{
    ThemeMode,
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
    ui_motion::{Tactile, icon_button, soft_button, text_button},
    ui_primitives::{
        FS_META, FS_UI, StudioPalette, UiLocale, render_radar_link_icon,
        render_schedule_repeat_icon, render_skills_puzzle_icon, render_terminal_logs_icon,
        render_usage_pulse_icon,
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
    Agents,
    SkillsMcp,
    Usage,
    Schedules,
    Remote,
    Logs,
}

impl SettingsTab {
    pub const ALL: [Self; 7] = [
        Self::General,
        Self::Agents,
        Self::SkillsMcp,
        Self::Usage,
        Self::Schedules,
        Self::Remote,
        Self::Logs,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::General => "settings-general",
            Self::Agents => "settings-agents",
            Self::SkillsMcp => "settings-skills",
            Self::Usage => "settings-usage",
            Self::Schedules => "settings-schedules",
            Self::Remote => "settings-remote",
            Self::Logs => "settings-logs",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::Agents => "智能体与账号",
            Self::SkillsMcp => "技能与 MCP",
            Self::Usage => "用量与缓存",
            Self::Schedules => "定时自动化",
            Self::Remote => "远端配对",
            Self::Logs => "诊断日志",
        }
    }
    fn title(self, locale: UiLocale) -> &'static str {
        let english = match self {
            Self::General => "General",
            Self::Agents => "Providers",
            Self::SkillsMcp => "Skills & MCP",
            Self::Usage => "Usage",
            Self::Schedules => "Scheduled tasks",
            Self::Remote => "Connections",
            Self::Logs => "Diagnostics",
        };
        locale.pick(self.label(), english)
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
            _ => {}
        }
        cx.notify();
    }

    fn settings_switch(
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
            let blur = cx.on_blur(&focus, window, |state: &mut SettingsSwitchInteraction, _, cx| {
                state.stretch.set(0., cx);
                cx.notify();
            });
            SettingsSwitchInteraction {
                focus,
                hovered: false,
                stretch: Slide::snappy(0.),
                _blur: blur,
            }
        });
        let (focus, hovered, stretch) = interaction.update(cx, |state, cx| {
            (state.focus.clone(), state.hovered, state.stretch.get(window, cx).clamp(0., 1.) * 3.)
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
                            .bg(if checked {
                                s.on_brand_action
                            } else {
                                s.surface
                            }),
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
        let heading = |label: &'static str| {
            div()
                .px(px(16.))
                .pb(px(10.))
                .text_color(s.ink)
                .text_size(px(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(label)
        };
        let box_group = || {
            div()
                .rounded(px(12.))
                .bg(s.card.opacity(0.5))
                .border_1()
                .border_color(s.line_soft)
                .overflow_hidden()
                .flex()
                .flex_col()
        };
        let stacked = f32::from(window.viewport_size().width) < 570.;
        let row = |title: &'static str, description: &'static str, control: AnyElement| {
            div()
                .min_h(px(72.))
                .px(px(16.))
                .py(px(12.))
                .flex()
                .map(|d| {
                    if stacked {
                        d.flex_col().items_start().gap(px(8.))
                    } else {
                        d.items_center().gap(px(16.))
                    }
                })
                .child(
                    div()
                        .map(|d| {
                            if stacked {
                                d.w_full()
                            } else {
                                d.flex_1().min_w_0()
                            }
                        })
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(FS_UI))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(s.ink)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(FS_META))
                                .line_height(px(17.))
                                .text_color(s.muted)
                                .child(description),
                        ),
                )
                .child(control)
        };
        let language = Tactile::soft("settings-ui-language", s)
            .h(px(30.))
            .px(px(12.))
            .radius(7.)
            .text_size(px(FS_META))
            .child(loc.pick("简体中文", "English"))
            .tip(loc.pick("切换至 English", "Switch to 简体中文"))
            .on_press(cx, |wb, window, cx| wb.toggle_ui_locale(window, cx));
        let sidebar = Self::settings_switch(
            "settings-show-sidebar",
            loc.pick("显示侧栏", "Show sidebar"),
            self.sidebar_open,
            |wb, cx| wb.toggle_sidebar(cx),
            s, window, cx,
        );
        let hidden_files = Self::settings_switch(
            "settings-hidden-files",
            loc.pick("显示隐藏文件", "Show hidden files"),
            self.tree_show_hidden,
            |wb, cx| {
                wb.tree_show_hidden = !wb.tree_show_hidden;
                wb.bridge.send(KarCommand::RefreshWorkspaceTree {
                    workspace: wb.effective_workspace_dir(),
                    show_hidden: wb.tree_show_hidden,
                });
                cx.notify();
            },
            s, window, cx,
        );
        let rule = || div().mx(px(16.)).h(px(1.)).bg(s.line_soft);
        let selected = (!self.follows_system_appearance()).then_some(self.theme_mode);
        let appearance_picker = SelectionStrip::options(
            "settings-appearance",
            loc.pick("主题", "Theme"),
            Group::Choices,
            [
                (None, loc.pick("系统", "System")),
                (Some(ThemeMode::Light), loc.pick("浅色", "Light")),
                (Some(ThemeMode::Dark), loc.pick("深色", "Dark")),
            ],
            selected,
            s,
            cx,
            |wb, mode, _, cx| match mode {
                Some(mode) => wb.select_appearance(mode, cx),
                None => wb.set_follow_system_appearance(cx),
            },
        );

        let appearance = box_group()
            .child(row(
                loc.pick("主题", "Theme"),
                loc.pick(
                    "跟随系统，或为 Kwrok 选择固定外观。",
                    "Follow the system or choose an appearance for Kwrok.",
                ),
                appearance_picker.into_any_element(),
            ))
            .child(rule())
            .child(row(
                loc.pick("界面语言", "Interface language"),
                loc.pick(
                    "选择工作台导航和控件的语言。",
                    "Choose the language for workbench navigation.",
                ),
                language.into_any_element(),
            ));
        let workspace = box_group()
            .child(row(
                loc.pick("显示侧栏", "Show sidebar"),
                loc.pick(
                    "在工作区旁显示项目和对话。",
                    "Show projects and threads beside the workspace.",
                ),
                sidebar,
            ))
            .child(rule())
            .child(row(
                loc.pick("显示隐藏文件", "Show hidden files"),
                loc.pick(
                    "在文件树中显示以点开头的文件和文件夹。",
                    "Include dotfiles and hidden directories in the file tree.",
                ),
                hidden_files,
            ));
        div()
            .id("settings-general-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .max_w(px(960.))
                    .mx_auto()
                    .p(px(28.))
                    .flex()
                    .flex_col()
                    .gap(px(32.))
                    .child(
                        div()
                            .child(heading(loc.pick("外观", "Appearance")))
                            .child(appearance),
                    )
                    .child(
                        div()
                            .child(heading(loc.pick("工作区", "Workspace")))
                            .child(workspace),
                    ),
            )
            .into_any_element()
    }

    fn settings_header(&self, tab: SettingsTab, s: &StudioPalette, cx: &mut Context<Self>) -> Div {
        let action = match tab {
            SettingsTab::Agents => Some(
                text_button(
                    "agents-recheck",
                    if self.providers_probing {
                        self.ui_locale.pick("检查中…", "Checking…")
                    } else {
                        self.ui_locale.pick("重新检查", "Check again")
                    },
                    s,
                )
                .disabled(self.providers_probing)
                .on_press(cx, |wb, _, cx| wb.refresh_agents(cx)),
            ),
            SettingsTab::SkillsMcp | SettingsTab::Usage => Some(
                text_button(
                    "settings-refresh",
                    self.ui_locale.pick("刷新", "Refresh"),
                    s,
                )
                .on_press(cx, move |wb, _, cx| wb.select_settings_tab(tab, cx)),
            ),
            SettingsTab::Schedules => Some(
                soft_button(
                    "sched-toggle-creator",
                    if self.schedule_creator_open {
                        self.ui_locale.pick("取消新建", "Cancel")
                    } else {
                        self.ui_locale.pick("新建任务", "New task")
                    },
                    s,
                )
                .on_press(cx, |wb, _, cx| {
                    wb.schedule_creator_open = !wb.schedule_creator_open;
                    cx.notify();
                }),
            ),
            _ => None,
        };
        let loc = self.ui_locale;
        div()
            .w_full()
            .flex_none()
            .h(px(48.))
            .pl(px(20.))
            .pr(px(8.))
            .flex()
            .items_center()
            .gap(px(10.))
            .border_b_1()
            .border_color(s.line_soft)
            .text_size(px(FS_UI))
            .child(
                div()
                    .text_color(s.muted)
                    .child(loc.pick("设置", "Settings")),
            )
            .child(pro_icon(ProIcon::ChevronRight, 12., s.faint))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_color(s.ink)
                    .child(tab.title(loc)),
            )
            .when_some(action, |row, action| row.child(action))
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
            40.
        } else if width >= 640. {
            24.
        } else {
            0.
        };
        let card_h = (height - 2. * inset).clamp(1., 960.);
        let rail_top_padding = if cfg!(target_os = "macos") && !window.is_fullscreen() {
            // The traffic lights end at y=34 even when the panel covers the title bar.
            let card_top = (height - card_h) * 0.5;
            (40. - card_top).max(8.)
        } else {
            8.
        };
        let card_radius = 16.;
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
                loc.pick("搜索设置", "Search settings"),
                InputMode::Search,
                window,
                cx,
            );
            editor.font_size = FS_UI;
            editor.set_placeholder(loc.pick("搜索设置", "Search settings"));
            editor
        });
        let query = search.read(cx).value().trim().to_lowercase();
        let nav_sensor = pro_selection_sensor();
        let mut nav_items =
            div()
                .relative()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(pro_pill_indicator(
                    "kar-settings-rail-pill",
                    active_tab.key(),
                    nav_sensor.clone(),
                    6.,
                    s.sidebar_selected,
                    None,
                    false,
                ));
        let mut results = 0;
        for (index, tab) in SettingsTab::ALL.into_iter().enumerate() {
            if !query.is_empty()
                && !tab.label().contains(&query)
                && !tab.title(UiLocale::En).to_lowercase().contains(&query)
            {
                continue;
            }
            if !compact && query.is_empty() {
                let group = match tab {
                    SettingsTab::Agents => Some(loc.pick("智能体", "Agents")),
                    SettingsTab::Schedules => Some(loc.pick("工作区", "Workspace")),
                    _ => None,
                };
                if let Some(group) = group {
                    nav_items = nav_items.child(
                        div()
                            .px(px(8.))
                            .pt(px(14.))
                            .pb(px(4.))
                            .text_size(px(FS_META))
                            .text_color(s.muted)
                            .child(group),
                    );
                }
            }
            results += 1;
            let selected = tab == active_tab;
            let color = if selected { s.ink } else { s.muted };
            let icon = match tab {
                SettingsTab::General => pro_icon(ProIcon::Sliders, 16., color),
                SettingsTab::Agents => pro_icon(ProIcon::Users, 16., color),
                SettingsTab::SkillsMcp => render_skills_puzzle_icon(16., color),
                SettingsTab::Usage => render_usage_pulse_icon(16., color),
                SettingsTab::Schedules => render_schedule_repeat_icon(16., color),
                SettingsTab::Remote => render_radar_link_icon(16., color),
                SettingsTab::Logs => render_terminal_logs_icon(16., color),
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
                            .px(px(8.))
                            .gap(px(8.))
                            .radius(6.)
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
        if results == 0 {
            nav_items = nav_items.child(
                div()
                    .px(px(8.))
                    .py(px(16.))
                    .text_size(px(FS_META))
                    .text_color(s.muted)
                    .child(loc.pick("未找到设置", "No settings found")),
            );
        }

        let search_input_focus = search.focus_handle(cx);
        let compact_search = expanded.clone();
        let search_row = div()
            .h(px(32.))
            .w_full()
            .flex()
            .items_center()
            .px(px(8.))
            .gap(px(8.))
            .child(pro_icon(ProIcon::Search, 15., s.muted))
            .child(div().flex_1().min_w_0().h(px(22.)).child(search.clone()))
            .when(query.is_empty(), |d| {
                d.child(crate::ui_primitives::render_kbd("/", s))
            });
        let rail = div()
            .map(|d| {
                if small_screen && !compact {
                    d.w_full()
                } else {
                    d.w(px(if compact { 56. } else { 240. }))
                }
            })
            .h_full()
            .flex_none()
            .bg(if s.dark { s.tray } else { s.stage })
            // GPUI's overflow mask is rectangular. The rail paints its own outer corners.
            .when(inset > 0., |d| {
                let radius = px(card_radius - 1.);
                d.rounded_l(radius)
                    .when(small_screen && !compact, |d| d.rounded_r(radius))
            })
            .border_r_1()
            .border_color(s.line_soft)
            .p(px(8.))
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
                .child(self.render_schedules_panel(s, cx))
                .into_any_element(),
            SettingsTab::Remote => div()
                .id("kar-settings-remote-scroll")
                .size_full()
                .overflow_y_scroll()
                .child(self.render_remote_panel(s, window, cx))
                .into_any_element(),
            SettingsTab::Logs => {
                self.sync_log_stream_widget(window, cx);
                self.render_logs_panel()
            }
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
                    if let Some(first) = SettingsTab::ALL.into_iter().find(|tab| {
                        tab.label().contains(&query)
                            || tab.title(UiLocale::En).to_lowercase().contains(&query)
                    }) {
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
            .when(inset > 0., |d| d.rounded(px(card_radius)).border_1())
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
                    .child(div().flex_1().min_h_0().overflow_hidden().child(
                        pro_view_transition_fill(
                            "kar-settings-pane-transition",
                            active_tab.key(),
                            content,
                        ),
                    )),
            )
            .into_any_element();

        let weak = cx.weak_entity();
        pro_presence(
            "kar-unified-settings-modal",
            true,
            0.15,
            0.12,
            move |phase| {
                div()
                    .absolute()
                    .inset_0()
                    .child(
                        deferred(
                            div()
                                .id("settings-backdrop")
                                .size_full()
                                .occlude()
                                .bg(hsla(0., 0., 0., 0.48 * phase))
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
            },
        )
    }
}
