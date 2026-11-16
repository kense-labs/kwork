//! Workspace group rows, hover actions and native-path context menus.

use super::workspace::{composer_bg, menu_bg, path_name};
use super::workspace_accessibility::{
    AccessibleMenuItem, AccessibleRow, AccessibleSurface, AccessibleTarget, RowAction,
};
use super::*;
use crate::theme_preference::ThemePreference;
use crate::workspace_tree::clear_workspace_watermarks;
pub(super) use crate::workspace_tree::{WorkspaceGroup, WorkspaceTree, owning_parent_folder};
use gpui::{Transformation, svg};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkspaceMenuAction {
    Rename,
    Reveal,
    Remove,
}

impl WorkspaceMenuAction {
    const ITEMS: [Self; 3] = [Self::Rename, Self::Reveal, Self::Remove];

    fn presentation(self) -> (&'static str, &'static str, &'static [u8]) {
        match self {
            Self::Rename => (
                "rename",
                "重命名",
                include_bytes!("../../assets/workspace-edit.svg"),
            ),
            Self::Reveal => (
                "reveal",
                if bridge::is_mac() {
                    "在 Finder 中显示"
                } else if bridge::is_windows() {
                    "在资源管理器中显示"
                } else {
                    "在文件夹中显示"
                },
                include_bytes!("../../assets/workspace-reveal.svg"),
            ),
            Self::Remove => (
                "delete",
                "移除工作区",
                include_bytes!("../../assets/workspace-trash.svg"),
            ),
        }
    }
}

impl DshApp {
    pub(super) fn toggle_workspace_group(
        &mut self,
        cwd: &str,
        descendants: &[String],
        cx: &mut Context<Self>,
    ) {
        self.workspace_context_menu = None;
        if !self.collapsed_workspaces.remove(cwd) {
            self.collapsed_workspaces.insert(cwd.to_owned());
            clear_workspace_watermarks(&mut self.workspace_watermarks, descendants);
        }
        self.save_workspace_preferences(cx);
        cx.notify();
    }

    pub(super) fn set_workspace_watermark(
        &mut self,
        cwd: &str,
        limit: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        if let Some(limit) = limit {
            self.workspace_watermarks.insert(cwd.to_owned(), limit);
        } else {
            self.workspace_watermarks.remove(cwd);
        }
        cx.notify();
    }

    pub(super) fn open_workspace_session(
        &mut self,
        session: SessionSummary,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.archived_sessions.contains(&session.id) {
            self.push_toast(workspace_search::ARCHIVED_RESULT, cx);
            return;
        }
        let from_search = !self.workspace_search.query.is_empty();
        if from_search {
            self.workspace_search.reveal = Some(session.id.clone());
        }
        if self.active_session.as_deref() == Some(session.id.as_str()) {
            self.close_workspace_search(false, w, cx);
            if from_search {
                self.expand_session_workspace(&session);
            }
            self.open_page(WorkspacePage::Conversation, cx);
        } else {
            self.resume(session, w, cx);
        }
    }

    pub(super) fn render_workspace_group(
        &mut self,
        group: WorkspaceGroup,
        depth: usize,
        w: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let cwd = group.cwd.clone();
        let expanded = !self.collapsed_workspaces.contains(&cwd);
        let active = self
            .sessions
            .iter()
            .find(|session| Some(session.id.as_str()) == self.active_session.as_deref())
            .is_some_and(|session| self.workspace_account_key(session) == cwd);
        let descendants = vec![cwd.clone()];
        let mut rendered = column()
            .relative()
            .w_full()
            .gap(px(2.))
            .when(!cwd.is_empty(), |rendered| {
                rendered.child(self.workspace_row_geometry.measure(cwd.clone()))
            })
            .child(self.render_workspace_heading(&cwd, depth, expanded, active, descendants, cx));
        if expanded {
            rendered = rendered.child(self.render_workspace_sessions(group, depth, w, cx));
        }
        rendered
    }

    pub(super) fn render_workspace_tree_group(
        &mut self,
        groups: &mut [WorkspaceGroup],
        tree: &WorkspaceTree,
        index: usize,
        depth: usize,
        w: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let cwd = groups[index].cwd.clone();
        let expanded = !self.collapsed_workspaces.contains(&cwd);
        let active_index = self
            .sessions
            .iter()
            .find(|session| Some(session.id.as_str()) == self.active_session.as_deref())
            .and_then(|session| {
                groups
                    .iter()
                    .position(|group| group.cwd == self.workspace_account_key(session))
            });
        let active = active_index.is_some_and(|current| {
            (current != index && tree.contains(index, current)) || (expanded && current == index)
        });
        let descendants = tree.descendants(index, groups);
        let mut rendered = column()
            .relative()
            .w_full()
            .gap(px(2.))
            .when(!cwd.is_empty(), |rendered| {
                rendered.child(self.workspace_row_geometry.measure(cwd.clone()))
            })
            .child(self.render_workspace_heading(&cwd, depth, expanded, active, descendants, cx));
        if expanded {
            if !tree.children[index].is_empty() {
                let mut children = column().w_full();
                for child in &tree.children[index] {
                    children = children.child(self.render_workspace_tree_group(
                        groups,
                        tree,
                        *child,
                        depth + 1,
                        w,
                        cx,
                    ));
                }
                rendered = rendered.child(
                    div()
                        .id(gpui::ElementId::Name(
                            format!("workspace-children-{cwd}").into(),
                        ))
                        .role(gpui::Role::Group)
                        .child(children),
                );
            }
            let group = WorkspaceGroup {
                cwd,
                sessions: std::mem::take(&mut groups[index].sessions),
            };
            rendered = rendered.child(self.render_workspace_sessions(group, depth, w, cx));
        }
        rendered
    }

    pub(super) fn workspace_name(&self, cwd: &str) -> String {
        self.workspace_registry
            .title(cwd)
            .map(|title| {
                if title == "default-workspace" {
                    "默认工作区"
                } else {
                    title
                }
                .to_owned()
            })
            .or_else(|| self.workspace_aliases.get(cwd).cloned())
            .unwrap_or_else(|| {
                if cwd.is_empty() {
                    "未分组".to_string()
                } else {
                    path_name(cwd)
                }
            })
    }

    pub(super) fn apply_workspace_preferences(&mut self, reply: Value, cx: &mut Context<Self>) {
        self.files_search_prefs = reply
            .get("filesSearch")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(workspace, preference)| {
                let rules = preference
                    .get("rules")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|rule| {
                        let path = rule.get("path").and_then(Value::as_str)?.trim();
                        (!path.is_empty()).then(|| crate::search_rules::SearchRule {
                            path: path.to_owned(),
                            enabled: rule.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                        })
                    })
                    .collect();
                let view = crate::web::FilesSearchPreferenceView {
                    rules,
                    show_hidden: preference.get("showHidden").and_then(Value::as_bool),
                    respect_gitignore: preference.get("respectGitignore").and_then(Value::as_bool),
                };
                (workspace.clone(), view)
            })
            .collect();
        self.apply_files_search_workspace();
        self.workspace_open_with = reply["workspaceOpenWith"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        self.workspace_aliases = reply
            .get("aliases")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(cwd, name)| {
                let name = name.as_str()?.trim();
                (!name.is_empty()).then(|| (cwd.clone(), name.to_string()))
            })
            .collect();
        self.hidden_workspaces = string_set(reply.get("hidden"));
        self.added_workspaces = reply["addedWorkspaces"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect();
        self.workspace_order = reply["workspaceOrder"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect();
        self.collapsed_workspaces = string_set(reply.get("collapsed"));
        if let Some(collapsed) = reply.get("sidebarCollapsed").and_then(Value::as_bool) {
            self.sidebar_collapsed = collapsed;
            // 启动恢复时不播放折叠动画，直接落到目标态。
            self.sidebar_slide = None;
        }
        if let Some(width) = reply
            .get("sidebarWidth")
            .and_then(Value::as_f64)
            .map(|width| width as f32)
            .filter(|width| (264. ..=420.).contains(width))
        {
            self.sidebar_width = width;
        }
        if let Some(group) = reply.get("groupBy").and_then(Value::as_str) {
            self.workspace_group_by = match group {
                "workspaceTree" => WorkspaceGroupBy::WorkspaceTree,
                "flat" => WorkspaceGroupBy::Flat,
                "workspace" => WorkspaceGroupBy::Workspace,
                _ => self.workspace_group_by,
            };
        }
        if let Some(order) = reply.get("orderBy").and_then(Value::as_str) {
            self.workspace_order_by = match order {
                "manual" => WorkspaceOrderBy::Manual,
                "updated" => WorkspaceOrderBy::Updated,
                _ => self.workspace_order_by,
            };
        }
        if let Some(orders) = reply
            .get("sessionOrderByAccount")
            .and_then(Value::as_object)
        {
            self.session_order_by_account = orders
                .into_iter()
                .filter_map(|(account, order)| {
                    let order = order
                        .as_array()?
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect();
                    Some((account.clone(), order))
                })
                .collect();
        }
        if let Some(archived) = reply.get("archivedFilter").and_then(Value::as_str) {
            self.workspace_archived_filter = match archived {
                "show" => WorkspaceArchivedFilter::Show,
                "only" => WorkspaceArchivedFilter::Only,
                "hide" => WorkspaceArchivedFilter::Hide,
                _ => self.workspace_archived_filter,
            };
        }
        if !self.workspace_registry.available()
            && let Some(pinned) = reply.get("pinned").and_then(Value::as_array)
        {
            self.pinned_sessions = pinned
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
        }
        if let Some(titles) = reply.get("sessionTitles").and_then(Value::as_object) {
            self.legacy_session_titles = titles
                .into_iter()
                .filter_map(|(id, name)| {
                    let name = name.as_str()?.trim();
                    (!name.is_empty()).then(|| (id.clone(), name.to_string()))
                })
                .collect();
        }
        if !self.session_archival.available
            && let Some(archived) = reply.get("archived").and_then(Value::as_array)
        {
            self.archived_sessions = archived
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
        }
        self.pinned_sessions
            .retain(|id| !self.archived_sessions.contains(id));
        if let Some(delta) = reply.get("contentFontDelta").and_then(Value::as_i64) {
            self.content_font_delta = (delta as i32).clamp(-2, 4);
        }
        if let Some(mode) = reply.get("workDetails").and_then(Value::as_str) {
            self.work_details = match mode {
                "compact" => WorkDetailsMode::Compact,
                "detailed" => WorkDetailsMode::Detailed,
                "verbose" => WorkDetailsMode::Verbose,
                "standard" => WorkDetailsMode::Standard,
                _ => self.work_details,
            };
        }
        if let Some(timing) = reply["collapseTiming"].as_str() {
            self.collapse_next_input = timing == "next-input";
        }
        if let Some(replies) = reply["foldedReplies"].as_str() {
            self.keep_folded_replies = replies != "final";
        }
        if let Some(destination) = reply["linkOpen"].as_str() {
            self.link_open_external = destination == "new-tab";
        }
        if let Some(mode) = reply["performanceUsage"].as_str() {
            self.performance_compact = mode == "compact";
        }
        if let Some(scratch) = reply
            .get("scratchSession")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            self.scratch_session = Some(scratch.to_owned());
        }
        if self.active_session.is_none() {
            self.selected_session = reply["selectedSession"]
                .as_str()
                .filter(|id| !id.is_empty())
                .map(str::to_owned);
        }
        if let Some(mode) = reply.get("themeMode").and_then(Value::as_str) {
            let preference = ThemePreference::from_id(mode);
            self.restore_theme_preference(preference, cx);
        } else {
            // Official DEFAULT_PREFERENCE is 跟随系统; a fresh profile follows
            // the platform appearance instead of pinning one theme.
            let preference = ThemePreference::System;
            self.restore_theme_preference(preference, cx);
        }
    }

    pub(super) fn save_workspace_preferences(&self, _cx: &App) {
        let mut hidden = self.hidden_workspaces.iter().cloned().collect::<Vec<_>>();
        let mut collapsed = self
            .collapsed_workspaces
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let mut archived = self.archived_sessions.iter().cloned().collect::<Vec<_>>();
        hidden.sort();
        collapsed.sort();
        archived.sort();
        let mut files_search: std::collections::HashMap<String, Value> = self
            .files_search_prefs
            .iter()
            .map(|(workspace, preference)| {
                (
                    workspace.clone(),
                    json!({
                        "rules": preference
                            .rules
                            .iter()
                            .map(|rule| json!({"path": rule.path, "enabled": rule.enabled}))
                            .collect::<Vec<_>>(),
                        "showHidden": preference.show_hidden,
                        "respectGitignore": preference.respect_gitignore,
                    }),
                )
            })
            .collect();
        let key = self.sidebar.workspace.trim_end_matches('/').to_owned();
        if !key.is_empty() {
            let current = self.files_search_preference_view();
            files_search.insert(
                key,
                json!({
                    "rules": current
                        .rules
                        .iter()
                        .map(|rule| json!({"path": rule.path, "enabled": rule.enabled}))
                        .collect::<Vec<_>>(),
                    "showHidden": current.show_hidden,
                    "respectGitignore": current.respect_gitignore,
                }),
            );
        }
        invoke(
            "save_workspace_preferences",
            json!({
                "onboardingRevision": self.desktop_onboarding.revision(),
                "preferences": {
                    "aliases": self.workspace_aliases,
                    "hidden": hidden,
                    "addedWorkspaces": self.added_workspaces,
                    "workspaceOrder": self.workspace_order,
                    "collapsed": collapsed,
                    "sidebarCollapsed": self.sidebar_collapsed,
                    "sidebarWidth": self.sidebar_width,
                    "groupBy": match self.workspace_group_by {
                        WorkspaceGroupBy::Workspace => "workspace",
                        WorkspaceGroupBy::WorkspaceTree => "workspaceTree",
                        WorkspaceGroupBy::Flat => "flat",
                    },
                    "orderBy": match self.workspace_order_by {
                        WorkspaceOrderBy::Manual => "manual",
                        WorkspaceOrderBy::Updated => "updated",
                    },
                    "sessionOrderByAccount": self.session_order_by_account,
                    "archivedFilter": match self.workspace_archived_filter {
                        WorkspaceArchivedFilter::Hide => "hide",
                        WorkspaceArchivedFilter::Show => "show",
                        WorkspaceArchivedFilter::Only => "only",
                    },
                    "pinned": self.pinned_sessions,
                    "sessionTitles": self.legacy_session_titles,
                    "archived": archived,
                    "themeMode": self.theme_preference.id(),
                    "contentFontDelta": self.content_font_delta,
                    "collapseTiming": if self.collapse_next_input { "next-input" } else { "completion" },
                    "foldedReplies": if self.keep_folded_replies { "all" } else { "final" },
                    "linkOpen": if self.link_open_external { "new-tab" } else { "sidebar" },
                    "performanceUsage": if self.performance_compact { "compact" } else { "detailed" },
                    "workspaceOpenWith": self.workspace_open_with,
                    "scratchSession": self.scratch_session,
                    "selectedSession": self.selected_session,
                    "workDetails": match self.work_details {
                        WorkDetailsMode::Compact => "compact",
                        WorkDetailsMode::Standard => "standard",
                        WorkDetailsMode::Detailed => "detailed",
                        WorkDetailsMode::Verbose => "verbose",
                    },
                    "filesSearch": files_search,
                }
            }),
        );
    }

    fn render_workspace_heading(
        &mut self,
        cwd: &str,
        depth: usize,
        expanded: bool,
        active: bool,
        descendants: Vec<String>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = dsh(cx);
        let hovered = self.hovered_workspace.as_deref() == Some(cwd);
        let menu_open = self
            .workspace_context_menu
            .as_ref()
            .is_some_and(|menu| menu.path == cwd);
        // 官方：工作区操作只在展开时显示；无 hover 的设备上展开行常显。
        let revealed = expanded && (hovered || menu_open || !bridge::has_pointer_hover());
        let label = self.workspace_name(cwd);
        let for_hover = cwd.to_owned();
        let for_toggle = cwd.to_owned();
        let for_menu = cwd.to_owned();
        let for_menu_key = cwd.to_owned();
        let for_control_click = cwd.to_owned();
        let for_create = cwd.to_owned();
        let for_drag = cwd.to_owned();
        let enabled = self.connected() && !self.creating_session;
        let folder_color = if active {
            p.state_business_primary
        } else {
            p.faint
        };
        let key = format!("group/{}", self.workspace_registry.id(cwd).unwrap_or(cwd));
        let can_open_menu = !cwd.is_empty();
        let focus = self
            .workspace_row_focus
            .entry(cwd.to_owned())
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let accessibility_focused = self.workspace_accessibility.focused(&key);
        self.workspace_accessibility.rendering.push(AccessibleRow {
            key,
            label: label.clone(),
            depth,
            expanded: Some(expanded),
            selected: None,
            description: None,
            menu: can_open_menu,
            unarchive: false,
            action: RowAction::ToggleGroup {
                path: cwd.to_owned(),
                descendants: descendants.clone(),
            },
        });
        row()
            .id(gpui::ElementId::Name(
                format!("workspace-group-{cwd}").into(),
            ))
            .relative()
            .role(gpui::Role::TreeItem)
            .aria_label(label.clone())
            .aria_expanded(expanded)
            .track_focus(&focus)
            .tab_index(0)
            .w_full()
            .h(px(32.))
            .flex_none()
            .pl(px(8. + depth as f32 * 12.))
            .pr(px(8.))
            .gap(px(6.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(accessibility_focused, |row| {
                row.border(px(1.)).border_color(p.brand_accent)
            })
            .when(self.is_dragged_workspace(cwd), |row| row.opacity(0.45))
            .when(revealed, |element| element.bg(p.hover))
            .hover(|style| style.bg(p.hover))
            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                let next = inside.then_some(for_hover.clone());
                if this.hovered_workspace != next {
                    this.hovered_workspace = next;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                if !event.is_right_click()
                    && !super::session_context_menu::mac_control_click(event.modifiers())
                {
                    this.toggle_workspace_group(&for_toggle, &descendants, cx);
                }
            }))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, w, cx| {
                    if can_open_menu
                        && super::session_context_menu::mac_control_click(event.modifiers)
                    {
                        this.open_workspace_context(&for_control_click, event.position.y, w, cx);
                    } else {
                        this.press_workspace_row(&for_drag, event, cx);
                    }
                }),
            )
            .when_some(self.workspace_drop_half(cwd), |row, half| {
                let line = div()
                    .absolute()
                    .left(px(6.))
                    .right(px(6.))
                    .h(px(2.))
                    .rounded_full()
                    .bg(p.brand_accent);
                row.child(match half {
                    crate::session_order::DropHalf::Before => line.top(px(-1.)),
                    crate::session_order::DropHalf::After => line.bottom(px(-1.)),
                })
            })
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, w, cx| {
                    this.open_workspace_context(&for_menu, event.position.y, w, cx);
                }),
            )
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, w, cx| {
                let stroke = &event.keystroke;
                if !event.is_held
                    && focus.is_focused(w)
                    && !w.default_prevented()
                    && !stroke.modifiers.control
                    && !stroke.modifiers.alt
                    && !stroke.modifiers.platform
                    && (stroke.key == "contextmenu"
                        || stroke.key == "f10" && stroke.modifiers.shift)
                    && let Some(bounds) = this.workspace_row_geometry.bounds(&for_menu_key)
                {
                    this.open_workspace_context(&for_menu_key, bounds.top(), w, cx);
                }
            }))
            .child(
                div()
                    .size(px(16.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(if revealed {
                        svg()
                            .data(include_bytes!("../../assets/workspace-triangle.svg"))
                            .size(px(14.))
                            .text_color(p.secondary)
                            .with_transformation(Transformation::default().with_rotation(
                                gpui::radians(if expanded {
                                    std::f32::consts::FRAC_PI_2
                                } else {
                                    0.
                                }),
                            ))
                    } else {
                        svg()
                            .data(if expanded {
                                include_bytes!("../../assets/workspace-folder-open.svg").as_slice()
                            } else {
                                include_bytes!("../../assets/workspace-folder.svg").as_slice()
                            })
                            .size(px(16.))
                            .text_color(folder_color)
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(p.ink)
                    .child(label),
            )
            .when(revealed && can_open_menu, |element| {
                let for_more = cwd.to_owned();
                element
                    .child(
                        div()
                            .id(gpui::ElementId::Name(
                                format!("workspace-more-{cwd}").into(),
                            ))
                            .role(gpui::Role::Button)
                            .aria_label("工作区更多操作")
                            .tab_index(0)
                            .size(px(24.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(|style| style.text_color(p.ink))
                            .child(
                                svg()
                                    .data(include_bytes!("../../assets/workspace-ellipsis.svg"))
                                    .size(px(16.))
                                    .text_color(p.secondary),
                            )
                            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, w, cx| {
                                if event.is_right_click()
                                    || super::session_context_menu::mac_control_click(
                                        event.modifiers(),
                                    )
                                {
                                    return;
                                }
                                this.open_workspace_context(&for_more, event.position().y, w, cx);
                            })),
                    )
                    .child(
                        div()
                            .id(gpui::ElementId::Name(
                                format!("workspace-new-session-{cwd}").into(),
                            ))
                            .role(gpui::Role::Button)
                            .aria_label("在此工作区新建会话")
                            .tab_index(0)
                            .tab_stop(enabled)
                            .size(px(24.))
                            .ml(px(-4.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .when(enabled, |element| {
                                element
                                    .cursor_pointer()
                                    .hover(|style| style.text_color(p.ink))
                            })
                            .when(!enabled, |element| element.opacity(0.4))
                            .child(
                                svg()
                                    .data(include_bytes!("../../assets/new-chat-outline.svg"))
                                    .size(px(14.))
                                    .text_color(p.secondary),
                            )
                            .on_click(cx.listener(move |this, _, w, cx| {
                                cx.stop_propagation();
                                this.workspace_context_menu = None;
                                this.collapsed_workspaces.remove(&for_create);
                                this.new_session_at(for_create.clone(), None, w, cx);
                            })),
                    )
            })
    }

    fn render_workspace_sessions(
        &mut self,
        group: WorkspaceGroup,
        depth: usize,
        w: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let p = dsh(cx);
        let WorkspaceGroup { cwd, sessions } = group;
        let mut rendered = column().w_full().gap(px(2.));
        // 官方水位线：初始 5 行；前 50 条里最后一条正在运行、等待用户、
        // 未读或在主视图打开的会话把水位线抬到它，之后不自行回落。
        const WATERMARK_BASE: usize = 5;
        let total = sessions.len();
        let active = self.active_session.clone();
        let mut limit = self
            .workspace_watermarks
            .get(&cwd)
            .copied()
            .unwrap_or(WATERMARK_BASE)
            .max(WATERMARK_BASE);
        for (index, session) in sessions.iter().take(50).enumerate() {
            let notable = self.session_indicator(&session.id, w, cx).0
                != crate::sessions::SessionIndicator::Idle
                || active.as_deref() == Some(session.id.as_str());
            if notable {
                limit = limit.max(index + 1);
            }
        }
        if let Some(id) = self.workspace_search.reveal.as_deref()
            && let Some(index) = sessions.iter().position(|session| session.id == id)
        {
            limit = limit.max(index + 1);
        }
        self.workspace_watermarks.insert(cwd.clone(), limit);
        // 当前选中的空白「新会话」是水位线之外的额外行。
        let extra_blank = active.as_deref().and_then(|id| {
            sessions
                .iter()
                .enumerate()
                .find(|(index, session)| session.id == id && session.blank && *index >= limit)
                .map(|(index, _)| index)
        });
        for (index, session) in sessions.into_iter().enumerate() {
            if index < limit || Some(index) == extra_blank {
                rendered =
                    rendered.child(self.render_session_row(session, depth, depth + 1, None, w, cx));
            }
        }
        let remaining = total.saturating_sub(limit);
        if remaining > 0 || limit > WATERMARK_BASE {
            // 剩余五条或更少时一次显示全部；最后一批之后提供「收起」。
            let expand = remaining > 0;
            let target = if remaining <= 5 { total } else { limit + 5 };
            let for_watermark = cwd.clone();
            let label = if expand {
                format!("展开其余（{remaining}）")
            } else {
                "收起".to_owned()
            };
            let next_limit = expand.then_some(target);
            let key = format!("watermark/{cwd}");
            let accessibility_focused = self.workspace_accessibility.focused(&key);
            self.workspace_accessibility.rendering.push(AccessibleRow {
                key,
                label: label.clone(),
                depth: depth + 1,
                expanded: None,
                selected: None,
                description: None,
                menu: false,
                unarchive: false,
                action: RowAction::SetWatermark {
                    path: cwd.clone(),
                    limit: next_limit,
                },
            });
            rendered = rendered.child(
                div()
                    .id(gpui::ElementId::Name(
                        format!("workspace-watermark-{cwd}").into(),
                    ))
                    .role(gpui::Role::Button)
                    .tab_index(0)
                    .w_full()
                    .h(px(28.))
                    .pl(px(32. + depth as f32 * 12.))
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .when(accessibility_focused, |row| {
                        row.border(px(1.)).border_color(p.brand_accent)
                    })
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(p.label_tertiary)
                    .hover(|style| style.text_color(p.label_primary))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_workspace_watermark(&for_watermark, next_limit, cx);
                    })),
            );
        }
        rendered
    }

    pub(super) fn render_workspace_context_menu(
        &self,
        menu: WorkspaceContextMenu,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = dsh(cx);
        let mut card = column()
            .id("workspace-context-menu")
            .role(gpui::Role::Menu)
            .w(px(216.))
            .gap_0()
            .p(px(4.))
            .rounded(px(12.))
            .bg(menu_bg(cx))
            .shadow(elevation_prominent())
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _, w, cx| {
                this.dismiss_workspace_context(false, w, cx);
            }))
            .on_key_down(cx.listener(Self::workspace_context_key));
        for (action, focus) in &menu.rows {
            let action = *action;
            if action == WorkspaceMenuAction::Remove {
                card = card.child(div().h(px(0.5)).mx(px(8.)).my(px(3.)).bg(p.line));
            }
            let path = menu.path.clone();
            let id = menu.id.clone();
            card = card.child(workspace_menu_row(
                action,
                focus,
                menu.accessible_focus == Some(action),
                cx,
                move |this, w, cx| {
                    if this.workspace_menu_matches(&path, id.as_deref()) {
                        this.run_workspace_menu_action(action, w, cx);
                    }
                },
            ));
        }
        card
    }

    fn workspace_target_current(&self, path: &str, id: Option<&str>) -> bool {
        !path.is_empty()
            && if self.workspace_registry.available() {
                id.is_some() && self.workspace_registry.id(path) == id
            } else {
                id.is_none() && !self.workspace_group_hidden(path)
            }
    }

    pub(super) fn workspace_menu_matches(&self, path: &str, id: Option<&str>) -> bool {
        self.workspace_context_menu
            .as_ref()
            .is_some_and(|menu| menu.path == path && menu.id.as_deref() == id)
            && self.workspace_target_current(path, id)
    }

    pub(super) fn open_workspace_context(
        &mut self,
        path: &str,
        y: gpui::Pixels,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.workspace_registry.id(path).map(str::to_owned);
        if !self.workspace_target_current(path, id.as_deref()) {
            return;
        }
        let trigger = self
            .workspace_row_focus
            .get(path)
            .cloned()
            .or_else(|| w.focused(cx));
        let rows = WorkspaceMenuAction::ITEMS
            .into_iter()
            .map(|action| (action, cx.focus_handle().tab_stop(true)))
            .collect::<Vec<_>>();
        rows[0].1.focus(w, cx);
        self.workspace_context_menu = Some(WorkspaceContextMenu {
            path: path.to_owned(),
            id,
            pointer_y: y.into(),
            trigger,
            rows,
            accessible_focus: None,
        });
        self.replace_header_menu(cx);
        self.show_workspace_view_options = false;
        self.show_workspace_menu = false;
        self.show_more_menu = false;
        self.composer_menu = None;
        w.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn dismiss_workspace_context(
        &mut self,
        restore_focus: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(menu) = self.workspace_context_menu.take() {
            if restore_focus && let Some(trigger) = menu.trigger {
                trigger.focus(w, cx);
            }
            cx.notify();
        }
    }

    fn workspace_context_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stroke = &event.keystroke;
        if stroke.modifiers.control || stroke.modifiers.alt || stroke.modifiers.platform {
            return;
        }
        let Some(menu) = self.workspace_context_menu.as_ref() else {
            return;
        };
        let active = menu.rows.iter().position(|(_, focus)| focus.is_focused(w));
        let last = menu.rows.len() - 1;
        match stroke.key.as_str() {
            "escape" | "tab" => self.dismiss_workspace_context(true, w, cx),
            key => {
                let index = match key {
                    "down" => active.map_or(0, |index| (index + 1) % menu.rows.len()),
                    "up" => active
                        .and_then(|index| index.checked_sub(1))
                        .unwrap_or(last),
                    "home" => 0,
                    "end" => last,
                    _ => return,
                };
                menu.rows[index].1.focus(w, cx);
            }
        }
        w.prevent_default();
        cx.stop_propagation();
    }

    fn run_workspace_menu_action(
        &mut self,
        action: WorkspaceMenuAction,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.workspace_context_menu.clone() else {
            return;
        };
        self.dismiss_workspace_context(true, w, cx);
        match action {
            WorkspaceMenuAction::Rename => {
                self.begin_workspace_dialog(&menu, WorkspaceDialogAction::Rename, w, cx)
            }
            WorkspaceMenuAction::Remove => {
                self.begin_workspace_dialog(&menu, WorkspaceDialogAction::Remove, w, cx)
            }
            WorkspaceMenuAction::Reveal => invoke(
                "open_start_tool",
                json!({"workspace": menu.path, "tool": "files"}),
            ),
        }
    }

    pub(super) fn accessible_workspace_menu(&self) -> Option<AccessibleSurface> {
        let menu = self.workspace_context_menu.as_ref()?;
        self.workspace_target_current(&menu.path, menu.id.as_deref())
            .then(|| AccessibleSurface::Menu {
                target: AccessibleTarget::Workspace {
                    path: menu.path.clone(),
                    id: menu.id.clone(),
                },
                label: format!("工作区“{}”的操作", self.workspace_name(&menu.path)),
                submenu: false,
                items: menu
                    .rows
                    .iter()
                    .map(|(action, _)| {
                        let (key, label, _) = action.presentation();
                        AccessibleMenuItem {
                            key: key.to_owned(),
                            label: label.to_owned(),
                            disabled: false,
                            submenu: false,
                        }
                    })
                    .collect(),
            })
    }

    pub(super) fn handle_accessible_group_menu(
        &mut self,
        key: &str,
        activate: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.workspace_context_menu.as_mut() else {
            return;
        };
        if let Some((action, focus)) = menu
            .rows
            .iter()
            .find(|(action, _)| action.presentation().0 == key)
        {
            if activate {
                let action = *action;
                self.run_workspace_menu_action(action, w, cx);
            } else {
                menu.accessible_focus = Some(*action);
                focus.focus(w, cx);
                cx.notify();
            }
        } else if !activate && key.is_empty() && menu.accessible_focus.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn close_workspace_dialog(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self
            .session_rename
            .as_ref()
            .is_some_and(|rename| rename.pending)
            || self
                .workspace_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.pending)
        {
            return;
        }
        let restore_focus = if let Some(dialog) = self.workspace_dialog.take() {
            dialog.restore_focus
        } else {
            self.session_rename
                .take()
                .and_then(|rename| rename.restore_focus)
        };
        if let Some(focus) = restore_focus {
            focus.focus(w, cx);
        }
    }

    fn begin_workspace_dialog(
        &mut self,
        menu: &WorkspaceContextMenu,
        action: WorkspaceDialogAction,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace_context_menu = None;
        if !self.workspace_target_current(&menu.path, menu.id.as_deref()) {
            cx.notify();
            return;
        }
        let title = self.workspace_name(&menu.path);
        let dialog = WorkspaceDialog {
            action,
            path: menu.path.clone(),
            id: menu.id.clone(),
            stored_title: self
                .workspace_registry
                .title(&menu.path)
                .unwrap_or(&title)
                .to_owned(),
            title,
            restore_focus: w.focused(cx),
            close: cx.focus_handle().tab_stop(true),
            cancel: cx.focus_handle().tab_stop(true),
            confirm: cx.focus_handle().tab_stop(true),
            pending: false,
            error: None,
        };
        super::document_html::restore_shell_focus();
        match action {
            WorkspaceDialogAction::Rename => {
                replace(&self.workspace_rename_editor, dialog.title.clone(), w, cx);
                self.workspace_rename_editor.update(cx, |editor, cx| {
                    editor.focus(w, cx);
                    editor.select_all(w, cx);
                });
            }
            WorkspaceDialogAction::Remove => dialog.cancel.focus(w, cx),
        }
        self.workspace_dialog = Some(dialog);
        cx.notify();
    }

    pub(super) fn begin_session_rename(
        &mut self,
        id: String,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.connected() || self.session_rename.is_some() || self.workspace_dialog.is_some() {
            return;
        }
        let Some(session) = self
            .sessions
            .iter()
            .find(|session| session.id == id && !session.blank)
        else {
            return;
        };
        let name = self.session_display_title(session);
        self.dismiss_header_menu(true, w, cx);
        let restore_focus = w.focused(cx);
        super::document_html::restore_shell_focus();
        replace(&self.session_rename_editor, name, w, cx);
        let mut rename = SessionRename {
            id,
            restore_focus,
            close: cx.focus_handle().tab_stop(true),
            cancel: cx.focus_handle().tab_stop(true),
            confirm: cx.focus_handle().tab_stop(true),
            pending: false,
            error: None,
        };
        rename.set_pending(&self.session_rename_editor, false, cx);
        self.session_rename = Some(rename);
        self.session_rename_editor.update(cx, |editor, cx| {
            editor.focus(w, cx);
            editor.select_all(w, cx);
        });
        cx.notify();
    }

    pub(super) fn confirm_session_rename(&mut self, cx: &mut Context<Self>) {
        let Some(rename) = self.session_rename.as_mut() else {
            return;
        };
        let editor = self.session_rename_editor.read(cx);
        let title = editor.value().trim();
        if rename.pending || title.is_empty() || editor.is_composing() {
            return;
        }
        invoke(
            "rename_session",
            json!({"sessionId": rename.id, "title": title}),
        );
        rename.error = None;
        rename.set_pending(&self.session_rename_editor, true, cx);
    }

    pub(super) fn finish_session_rename(
        &mut self,
        result: Result<Value, String>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(rename) = self.session_rename.as_mut().filter(|rename| rename.pending) else {
            return;
        };
        let id = rename.id.clone();
        rename.set_pending(&self.session_rename_editor, false, cx);
        match result.and_then(|reply| {
            if let (Some(title), Some(seq)) = (reply["title"].as_str(), reply["seq"].as_u64()) {
                Ok((title.to_owned(), seq))
            } else {
                Err("服务未返回重命名结果。".into())
            }
        }) {
            Ok((title, seq)) => {
                self.apply_durable_session_title(&id, &title, seq);
                if self.legacy_session_titles.remove(&id).is_some() {
                    self.save_workspace_preferences(cx);
                }
                self.close_workspace_dialog(w, cx);
            }
            Err(error) => {
                rename.error = Some(error);
                self.session_rename_editor
                    .update(cx, |editor, cx| editor.focus(w, cx));
            }
        }
    }

    fn workspace_rename_duplicate(&self, dialog: &WorkspaceDialog, name: &str) -> bool {
        if let Some(id) = &dialog.id {
            return self.workspace_registry.title_in_use(id, name);
        }
        let path = dialog.path.as_str();
        self.sessions
            .iter()
            .any(|session| session.cwd != path && self.workspace_name(&session.cwd) == name)
            || (self.workspace != path && self.workspace_name(&self.workspace) == name)
            || self
                .workspace_aliases
                .keys()
                .any(|cwd| cwd != path && self.workspace_name(cwd) == name)
    }

    fn workspace_remove_description(dialog: &WorkspaceDialog) -> String {
        let name = &dialog.title;
        if dialog.id.is_some() {
            format!(
                "将把“{name}”从工作区列表中移除。文件夹与会话记录会保留，其会话将显示在“未分组”下。"
            )
        } else {
            format!("将从侧栏移除“{name}”分组。文件夹和会话记录会保留。")
        }
    }

    fn workspace_dialog_error(&self, dialog: &WorkspaceDialog) -> Option<String> {
        if !dialog.pending && !self.workspace_target_current(&dialog.path, dialog.id.as_deref()) {
            Some("工作区已移除或替换，请重新选择。".into())
        } else {
            dialog.error.clone()
        }
    }

    pub(super) fn accessible_workspace_dialog(&self, cx: &App) -> Option<AccessibleSurface> {
        let dialog = self.workspace_dialog.as_ref()?;
        let target = AccessibleTarget::Workspace {
            path: dialog.path.clone(),
            id: dialog.id.clone(),
        };
        let current = self.workspace_target_current(&dialog.path, dialog.id.as_deref());
        let error = self.workspace_dialog_error(dialog);
        Some(match dialog.action {
            WorkspaceDialogAction::Rename => {
                let draft = self.workspace_rename_editor.read(cx).value();
                let name = draft.trim();
                let duplicate = !name.is_empty() && self.workspace_rename_duplicate(dialog, name);
                AccessibleSurface::Rename {
                    target,
                    draft: draft.to_owned(),
                    pending: dialog.pending,
                    blocked: !current
                        || name.is_empty()
                        || name == dialog.stored_title
                        || duplicate,
                    error: error
                        .or_else(|| duplicate.then(|| format!("已存在名为“{name}”的工作区"))),
                }
            }
            WorkspaceDialogAction::Remove => AccessibleSurface::RemoveWorkspace {
                target,
                description: Self::workspace_remove_description(dialog),
                pending: dialog.pending,
                blocked: !current,
                error,
            },
        })
    }

    pub(super) fn confirm_workspace_rename(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self
            .workspace_dialog
            .as_ref()
            .filter(|dialog| dialog.action == WorkspaceDialogAction::Rename && !dialog.pending)
        else {
            return;
        };
        let editor = self.workspace_rename_editor.read(cx);
        let name = editor.value().trim();
        if !self.workspace_target_current(&dialog.path, dialog.id.as_deref())
            || editor.is_composing()
            || name.is_empty()
            || name == dialog.stored_title
            || self.workspace_rename_duplicate(dialog, name)
        {
            return;
        }
        let name = name.to_owned();
        if dialog.id.is_some() {
            self.submit_registered_workspace_change(Some(name), w, cx);
        } else {
            self.workspace_aliases.insert(dialog.path.clone(), name);
            self.close_workspace_dialog(w, cx);
            self.save_workspace_preferences(cx);
        }
    }

    pub(super) fn confirm_workspace_remove(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self
            .workspace_dialog
            .as_ref()
            .filter(|dialog| dialog.action == WorkspaceDialogAction::Remove && !dialog.pending)
        else {
            return;
        };
        if !self.workspace_target_current(&dialog.path, dialog.id.as_deref()) {
            return;
        }
        if dialog.id.is_some() {
            self.submit_registered_workspace_change(None, w, cx);
        } else {
            self.hidden_workspaces.insert(dialog.path.clone());
            self.close_workspace_dialog(w, cx);
            self.save_workspace_preferences(cx);
            self.push_toast("已从侧栏移除工作区分组；可在「设置 → 通用」中恢复。", cx);
        }
    }

    fn submit_registered_workspace_change(
        &mut self,
        title: Option<String>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let available = self.connected() && !self.service_starting;
        let Some(dialog) = &mut self.workspace_dialog else {
            return;
        };
        let Some(id) = &dialog.id else { return };
        if self.workspace_registry.pending || !available {
            dialog.error = Some(if available {
                "正在保存另一项工作区更改，请稍后重试。".into()
            } else {
                "连接服务后可修改工作区。".into()
            });
            return;
        }
        let operation = if let Some(title) = title {
            json!({"action": "rename", "id": id, "title": title})
        } else {
            json!({"action": "delete", "id": id})
        };
        dialog.pending = true;
        dialog.error = None;
        dialog.close.focus(w, cx);
        self.workspace_registry.pending = true;
        self.workspace_rename_editor
            .update(cx, |editor, _| editor.disabled = true);
        cx.spawn_in(w, async move |this, cx| {
            let result =
                bridge::invoke("workspace_registry", json!({"operation": operation})).await;
            let _ = this.update_in(cx, |app, w, cx| {
                app.finish_registered_workspace_change(result, w, cx)
            });
        })
        .detach();
    }

    fn finish_registered_workspace_change(
        &mut self,
        result: Result<Value, String>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace_registry.pending = false;
        self.workspace_rename_editor
            .update(cx, |editor, _| editor.disabled = false);
        let Some(dialog) = self
            .workspace_dialog
            .as_mut()
            .filter(|dialog| dialog.pending)
        else {
            return;
        };
        dialog.pending = false;
        let path = dialog.path.clone();
        let id = dialog.id.clone();
        let removed = dialog.action == WorkspaceDialogAction::Remove;
        let result = result.and_then(|snapshot| {
            if !self.apply_workspace_registry(&snapshot)
                || (removed == (self.workspace_registry.id(&path) == id.as_deref()))
            {
                return Err("服务未返回工作区状态。".into());
            }
            Ok(())
        });
        match result {
            Ok(()) => {
                self.workspace_aliases.remove(&path);
                if removed && self.workspace_registry.id(&path).is_none() {
                    self.added_workspaces.retain(|candidate| candidate != &path);
                    self.hidden_workspaces.remove(&path);
                    self.collapsed_workspaces.remove(&path);
                    self.workspace_order.retain(|candidate| candidate != &path);
                    self.session_order_by_account.remove(&path);
                    self.workspace_watermarks.remove(&path);
                    self.workspace_row_focus.remove(&path);
                }
                self.close_workspace_dialog(w, cx);
                self.save_workspace_preferences(cx);
            }
            Err(error) => {
                if let Some(dialog) = &mut self.workspace_dialog {
                    dialog.error = Some(error);
                    if removed {
                        dialog.confirm.focus(w, cx);
                    } else {
                        self.workspace_rename_editor
                            .update(cx, |editor, cx| editor.focus(w, cx));
                    }
                }
            }
        }
        cx.notify();
    }

    pub(super) fn render_workspace_dialog(
        &mut self,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = dsh(cx);
        let session_rename = self.session_rename.is_some();
        let workspace_dialog = self.workspace_dialog.as_ref();
        let workspace_rename =
            workspace_dialog.is_some_and(|dialog| dialog.action == WorkspaceDialogAction::Rename);
        let pending = self
            .session_rename
            .as_ref()
            .is_some_and(|rename| rename.pending)
            || workspace_dialog.is_some_and(|dialog| dialog.pending);
        let error = self
            .session_rename
            .as_ref()
            .and_then(|rename| rename.error.clone())
            .or_else(|| workspace_dialog.and_then(|dialog| self.workspace_dialog_error(dialog)));
        let rename = workspace_rename || session_rename;
        let title = if session_rename {
            "重命名会话"
        } else if rename {
            "重命名工作区"
        } else {
            "移除工作区"
        };
        let remove_description = workspace_dialog.map(Self::workspace_remove_description);
        let stored_title = workspace_dialog.map_or("", |dialog| dialog.stored_title.as_str());
        let editor = if session_rename {
            self.session_rename_editor.clone()
        } else {
            self.workspace_rename_editor.clone()
        };
        let draft = editor.read(cx).value().trim().to_owned();
        // 重名检查只适用于工作区；会话标题允许与工作区无关。
        let duplicate = workspace_rename
            && !draft.is_empty()
            && workspace_dialog
                .is_some_and(|dialog| self.workspace_rename_duplicate(dialog, &draft));
        let blocked = pending
            || workspace_dialog.is_some_and(|dialog| {
                !self.workspace_target_current(&dialog.path, dialog.id.as_deref())
            })
            || (rename
                && (draft.is_empty() || (workspace_rename && draft == stored_title) || duplicate));
        let width = (f32::from(w.viewport_size().width) - 48.).clamp(0., 380.);
        let caption = if bridge::is_mac() && !self.shell_fullscreen(w) {
            40.
        } else {
            0.
        };
        let focus = self
            .session_rename
            .as_ref()
            .map(|dialog| (&dialog.close, &dialog.cancel, &dialog.confirm))
            .or_else(|| {
                workspace_dialog.map(|dialog| (&dialog.close, &dialog.cancel, &dialog.confirm))
            });
        div()
            .id("workspace-dialog")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, w, cx| {
                if event.keystroke.key == "escape" {
                    this.close_workspace_dialog(w, cx);
                    w.prevent_default();
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(div().absolute().inset_0().top(px(caption)).bg(p.bg_mask_1))
            .child(
                column()
                    .id("workspace-dialog-panel")
                    .role(gpui::Role::Dialog)
                    .aria_label(title)
                    .w(px(width))
                    .pb(px(24.))
                    .rounded(px(28.))
                    .bg(composer_bg(cx))
                    .shadow(elevation_prominent())
                    .occlude()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
                    .capture_key_down(cx.listener(
                        move |this, event: &gpui::KeyDownEvent, w, cx| {
                            let stroke = &event.keystroke;
                            if stroke.key != "tab"
                                || stroke.modifiers.control
                                || stroke.modifiers.alt
                                || stroke.modifiers.platform
                            {
                                return;
                            }
                            let (close, cancel, confirm, input) =
                                if let Some(dialog) = &this.session_rename {
                                    (
                                        &dialog.close,
                                        &dialog.cancel,
                                        &dialog.confirm,
                                        Some(&this.session_rename_editor.read(cx).focus),
                                    )
                                } else if let Some(dialog) = &this.workspace_dialog {
                                    let input = rename
                                        .then(|| &this.workspace_rename_editor.read(cx).focus);
                                    (&dialog.close, &dialog.cancel, &dialog.confirm, input)
                                } else {
                                    return;
                                };
                            let stops: Vec<_> = std::iter::once(close)
                                .chain(input.filter(|_| !pending))
                                .chain((!pending).then_some(cancel))
                                .chain((!blocked).then_some(confirm))
                                .collect();
                            let index = stops.iter().position(|stop| stop.is_focused(w));
                            let next = if stroke.modifiers.shift {
                                index.unwrap_or(0).checked_sub(1).unwrap_or(stops.len() - 1)
                            } else {
                                index.map_or(0, |index| (index + 1) % stops.len())
                            };
                            let target = stops[next].clone();
                            target.focus(w, cx);
                            w.prevent_default();
                            cx.stop_propagation();
                        },
                    ))
                    .child(super::permission_dialog::dialog_header(
                        title,
                        "close-workspace-dialog",
                        focus.map(|(close, _, _)| close),
                        cx,
                        |this, w, cx| {
                            this.close_workspace_dialog(w, cx);
                            cx.notify();
                        },
                    ))
                    .child(
                        column()
                            .w_full()
                            .mt(px(20.))
                            .px(px(24.))
                            .when(rename, |element| {
                                element
                                    .child(
                                        div()
                                            .w_full()
                                            .h(px(44.))
                                            .px(px(14.))
                                            .flex()
                                            .items_center()
                                            .rounded(px(16.))
                                            .border(px(0.5))
                                            .border_color(p.border_l4)
                                            .text_size(px(14.))
                                            .line_height(px(22.))
                                            .child(editor),
                                    )
                                    .when(duplicate, |element| {
                                        element.child(
                                            div()
                                                .mt(px(8.))
                                                .text_size(px(12.))
                                                .line_height(px(18.))
                                                .text_color(p.danger)
                                                .child(format!("已存在名为“{draft}”的工作区。")),
                                        )
                                    })
                            })
                            .when(!rename, |element| {
                                element
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(22.))
                                            .text_color(p.secondary)
                                            .child(remove_description.unwrap_or_default()),
                                    )
                                    .when(pending, |element| {
                                        element.child(
                                            div()
                                                .id("workspace-delete-status")
                                                .role(gpui::Role::Status)
                                                .mt(px(8.))
                                                .text_size(px(12.))
                                                .text_color(p.secondary)
                                                .child("正在移除工作区…"),
                                        )
                                    })
                            })
                            .when_some(error, |element, error| {
                                element.child(
                                    div()
                                        .id("workspace-dialog-error")
                                        .role(gpui::Role::Alert)
                                        .mt(px(8.))
                                        .text_size(px(12.))
                                        .line_height(px(18.))
                                        .text_color(p.danger)
                                        .child(error),
                                )
                            }),
                    )
                    .child(
                        row()
                            .w_full()
                            .mt(px(20.))
                            .px(px(24.))
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                super::permission_dialog::dialog_action(
                                    "workspace-dialog-cancel",
                                    "取消",
                                    false,
                                    pending,
                                    0.,
                                    cx,
                                    |this, w, cx| {
                                        this.close_workspace_dialog(w, cx);
                                        cx.notify();
                                    },
                                )
                                .when_some(focus, |button, (_, cancel, _)| {
                                    button.track_focus(cancel)
                                }),
                            )
                            .child(
                                super::permission_dialog::dialog_action(
                                    "workspace-dialog-confirm",
                                    if rename {
                                        "重命名"
                                    } else {
                                        "移除工作区"
                                    },
                                    rename,
                                    blocked,
                                    0.,
                                    cx,
                                    move |this, w, cx| {
                                        if session_rename {
                                            this.confirm_session_rename(cx);
                                        } else if rename {
                                            this.confirm_workspace_rename(w, cx);
                                        } else {
                                            this.confirm_workspace_remove(w, cx);
                                        }
                                        cx.notify();
                                    },
                                )
                                .when_some(focus, |button, (_, _, confirm)| {
                                    button.track_focus(confirm)
                                })
                                .when(!rename, |element| element.text_color(p.danger)),
                            ),
                    ),
            )
            .on_click(cx.listener(|this, _, w, cx| {
                this.close_workspace_dialog(w, cx);
                cx.notify();
            }))
    }
}

impl SessionRename {
    fn set_pending(&mut self, editor: &gpui::Entity<Editor>, pending: bool, cx: &mut App) {
        self.pending = pending;
        let p = dsh(cx);
        let color = if pending {
            p.label_dimmed
        } else {
            p.label_primary
        };
        editor.update(cx, |editor, cx| {
            editor.disabled = pending;
            editor.set_run_styler(std::rc::Rc::new(move |_, run| {
                vec![gpui::TextRun {
                    color,
                    ..run.clone()
                }]
            }));
            cx.notify();
        });
    }
}

impl DshApp {
    pub(super) fn toggle_session_pin(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.service_starting {
            return;
        }
        if self.workspace_registry.available() {
            self.set_registered_session_pin(id, !self.session_is_pinned(id), cx);
            return;
        }
        if let Some(index) = self.pinned_sessions.iter().position(|pinned| pinned == id) {
            self.pinned_sessions.remove(index);
        } else {
            self.pinned_sessions.insert(0, id.to_owned());
            self.sync_session_pin_order(id);
        }
        self.save_workspace_preferences(cx);
        cx.notify();
    }
    /// Preserve pre-upgrade aliases until the user saves a durable title.
    /// User-assigned rename/alias only. The Host title is read through
    /// [`SessionSummary::display_title`] so raw goal injections never surface.
    pub(super) fn session_title_name(&self, session: &SessionSummary) -> Option<&str> {
        self.legacy_session_titles
            .get(&session.id)
            .map(String::as_str)
    }

    pub(super) fn session_display_title(&self, session: &SessionSummary) -> String {
        self.session_title_name(session)
            .map(str::to_owned)
            .unwrap_or_else(|| session.display_title())
    }
}

fn string_set(value: Option<&Value>) -> std::collections::HashSet<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn workspace_menu_row(
    item: WorkspaceMenuAction,
    focus: &gpui::FocusHandle,
    accessibility_focused: bool,
    cx: &mut Context<DshApp>,
    action: impl Fn(&mut DshApp, &mut Window, &mut Context<DshApp>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let p = dsh(cx);
    let (id, label, artwork) = item.presentation();
    let color = if item == WorkspaceMenuAction::Remove {
        p.danger
    } else {
        p.ink
    };
    let item = row()
        .id(gpui::ElementId::Name(format!("workspace-menu-{id}").into()))
        .role(gpui::Role::MenuItem)
        .aria_label(label)
        .track_focus(focus)
        .tab_index(0)
        .w_full()
        .h(px(32.))
        .flex_none()
        .px(px(8.))
        .gap(px(8.))
        .rounded(px(8.))
        .cursor_pointer()
        .text_size(px(14.))
        .line_height(px(20.))
        .text_color(color)
        .when(accessibility_focused, |item| item.bg(p.hover))
        .focus_visible(|style| style.bg(p.hover))
        .hover(|style| style.bg(p.hover))
        .child(
            svg()
                .data(artwork)
                .size(px(16.))
                .flex_none()
                .text_color(color),
        )
        .child(div().flex_1().min_w_0().truncate().child(label));
    dock_action_button(item, cx.listener(move |this, _, w, cx| action(this, w, cx)))
}
