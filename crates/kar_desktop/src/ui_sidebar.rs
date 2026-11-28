//! Native workspace chrome and thread navigation.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
    time::Duration,
};

use gpui::{prelude::*, *};
use gpui_kit::component::tooltip::Tooltip;
use kui::{
    components::pro_chrome::{
        pro_mark_selected, pro_pill_indicator, pro_progress_ring, pro_selection_sensor,
    },
    primitives::pro_icons::{ProIcon, pro_icon},
};

use crate::{
    remote_link::now_epoch_ms,
    studio_hub::{project_display_name, project_id_for_path},
    types::{
        ChatAccessMode, ChatListEntry, localized_relative_age, runtime_display_name,
    },
    ui_motion::{
        MenuPlacement, Tactile, close_button, icon_button_sm, menu_caption, menu_check, menu_divider, menu_hint,
        menu_host, menu_icon_item, soft_button,
    },
    ui_primitives::{
        CTRL_H, CTRL_SM_H, FS_META, FS_TAG, FS_UI, ICON, ICON_SM, LH_UI, R_CTRL, StudioPalette, TITLEBAR_BUTTON_GAP,
        clean_display_doc_name, macos_traffic_pad, render_approval_hand,
        render_circle_check_icon,
        sidebar_texture, render_office_chip,
        render_radar_link_icon, render_runtime_mark, render_sample_tag,
        render_schedule_repeat_icon,
        tnum_features,
    },
    ui_settings::SettingsTab,
    workbench::{KarWorkbench, MainStageView, Menu, RightPaneSurface, WorkspaceLayoutMode},
};

const DELETE_CONFIRM_TTL: Duration = Duration::from_secs(3);
const THREAD_ROW_H: f32 = CTRL_H;

struct SidebarFolder<'a> {
    id: String,
    root: PathBuf,
    name: String,
    last_activity: u64,
    entries: Vec<&'a ChatListEntry>,
}

impl SidebarFolder<'_> {
    fn new(id: String, root: &Path, name: &str) -> Self {
        Self {
            id,
            root: root.to_path_buf(),
            name: project_display_name(name).to_owned(),
            last_activity: 0,
            entries: Vec::new(),
        }
    }

    fn label(&self, parents: &HashMap<PathBuf, String>) -> String {
        match parents.get(&self.root) {
            Some(parent) => format!("{} · {parent}", self.name),
            None => self.name.clone(),
        }
    }
}

// Two folders named "work" should stay distinguishable even when their parents
// have the same name. Show just enough of the parent path in that case.
fn folder_disambiguators(folders: &[SidebarFolder<'_>]) -> HashMap<PathBuf, String> {
    let mut by_name = HashMap::<String, Vec<&Path>>::new();
    for folder in folders {
        by_name
            .entry(folder.name.to_lowercase())
            .or_default()
            .push(&folder.root);
    }
    let mut labels = HashMap::new();
    for roots in by_name.values().filter(|paths| paths.len() > 1) {
        for &root in roots {
            let Some(parent) = root.parent() else {
                continue;
            };
            let mut suffix = PathBuf::new();
            for part in parent.components().rev() {
                suffix = if suffix.as_os_str().is_empty() {
                    PathBuf::from(part.as_os_str())
                } else {
                    Path::new(part.as_os_str()).join(suffix)
                };
                let unique = roots.iter().all(|&other| {
                    other == root || !other.parent().is_some_and(|p| p.ends_with(&suffix))
                });
                if unique {
                    break;
                }
            }
            labels.insert(root.to_owned(), suffix.display().to_string());
        }
    }
    labels
}

/// A search includes settled chats. Outside search they appear only on the settled shelf.
fn filter_sidebar_folders<'a>(
    folders: &mut Vec<SidebarFolder<'a>>,
    scope: Option<&Path>,
    query: &str,
    parents: &HashMap<PathBuf, String>,
    settled: &BTreeSet<uuid::Uuid>,
    live: Option<&kar::chat::ChatInfo>,
) -> Vec<&'a ChatListEntry> {
    let mut shelf = Vec::new();
    folders.retain_mut(|folder| {
        if scope.is_some_and(|path| path != folder.root) {
            return false;
        }
        if query.is_empty() {
            folder.entries.retain(|&entry| {
                if settled.contains(&entry.info.id) {
                    shelf.push(entry);
                    false
                } else {
                    true
                }
            });
            return true;
        }
        if scope.is_none() && (folder.name.to_lowercase().contains(query)
            || parents.get(&folder.root).is_some_and(|p| p.to_lowercase().contains(query)))
        {
            return true;
        }
        folder.entries.retain(|e| {
            e.title.to_lowercase().contains(query)
                || runtime_display_name(e.runtime(live)).to_lowercase().contains(query)
        });
        !folder.entries.is_empty()
    });
    shelf.sort_by_key(|entry| std::cmp::Reverse(entry.last_activity_ms.unwrap_or(0)));
    shelf
}

impl KarWorkbench {
    pub(crate) fn workspace_toolbar(
        &self,
        title: &'static str,
        action: Option<AnyElement>,
        s: &StudioPalette,
        window: &Window,
    ) -> Div {
        let left = 20. + (macos_traffic_pad(window) - 20.) * (1. - self.inline_sidebar_reveal);
        crate::ui_primitives::titlebar_row(window)
            .w_full()
            .pl(px(left))
            .pr(px(12.))
            .gap(px(8.))
            .when(!self.sidebar_open, |row| {
                row.child(div().size(px(CTRL_SM_H)).flex_none())
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(s.font_size(FS_UI))
                    .line_height(s.line_height(20.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.ink)
                    .child(title),
            )
            .when_some(action, |row, button| row.child(button))
    }

    pub(crate) fn render_delete_undo(&self, s: &StudioPalette, cx: &mut Context<Self>) -> Option<Div> {
        let loc = self.ui_locale;
        self
            .deferred_delete_chat
            .as_ref()
            .map(|(_, del_title, started)| {
                let elapsed = started.elapsed().as_secs_f32();
                let remain = (1.0 - elapsed / 5.0).clamp(0.0, 1.0);
                div().px(px(10.)).pb(px(6.)).flex_none().child(
                    div()
                        .px(px(9.))
                        .py(px(6.))
                        .rounded(px(R_CTRL))
                        .bg(s.card)
                        .border_1()
                        .border_color(s.line)
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .child(pro_progress_ring(
                            remain,
                            14.,
                            2.,
                            s.pro.amber,
                            s.line_strong,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(s.font_size(FS_META))
                                .text_color(s.ink)
                                .child(format!(
                                    "{} \"{del_title}\"",
                                    loc.pick("即将删除", "Deleting")
                                )),
                        )
                        .child(
                            soft_button("undo-pending-thread-deletion", loc.pick("撤销", "Undo"), s)
                                .on_press(cx, |this, _, cx| this.undo_deferred_chat_delete(cx)),
                        ),
                )
            })
    }

    pub(crate) fn render_sidebar(
        &mut self,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let s = StudioPalette::get(cx);
        let loc = self.ui_locale;
        let now_ms = now_epoch_ms();
        let active_chat_id = self.current_chat_id();
        let has_query = !self.sidebar_filter.trim().is_empty();
        let query = self.sidebar_filter.trim().to_lowercase();
        let docs_shown = self.layout_mode != WorkspaceLayoutMode::ChatFocus;

        if self
            .pending_delete_chat
            .is_some_and(|(_, at)| at.elapsed() >= DELETE_CONFIRM_TTL)
        {
            self.pending_delete_chat = None;
        }

        let in_conv = self.main_stage_view == MainStageView::Conversation;
        let on_start_screen = in_conv
            && self.projected.turns.is_empty()
            && self.active_briefs.is_empty()
            && self.queued_prompt.is_none()
            && matches!(
                self.access_mode,
                ChatAccessMode::NoneSelected | ChatAccessMode::WriteController { .. }
            );

        let git_selected = in_conv && docs_shown && self.right_surface == RightPaneSurface::GitWorkbench;
        let browser_selected = in_conv && docs_shown && self.right_surface == RightPaneSurface::Browser;
        let show_chat_selection = in_conv && !on_start_screen && !git_selected && !self.settings_open;
        let nav_sensor = pro_selection_sensor();
        let left_inset = macos_traffic_pad(window);
        let top_bar = crate::ui_primitives::titlebar_row(window)
            .relative()
            .pl(px(left_inset))
            .pr(px(8.))
            .gap(px(TITLEBAR_BUTTON_GAP))
            .child(div().size(px(CTRL_SM_H)).flex_none())
            .child(
                icon_button_sm("side-back", ProIcon::ArrowLeft, loc.pick("后退", "Go back"), &s)
                .disabled(!self.can_navigate(false))
                .on_press(cx, |this, win, cx| this.navigate_history(false, win, cx)),
            )
            .child(
                icon_button_sm("side-forward", ProIcon::ArrowRight, loc.pick("前进", "Go forward"), &s)
                .disabled(!self.can_navigate(true))
                .on_press(cx, |this, win, cx| this.navigate_history(true, win, cx)),
            );

        let search_focused = self.sidebar_search.read(cx).focus.is_focused(window);
        let search_group = "sidebar-thread-search";
        let search = div()
            .id("side-search-field")
            .group(search_group)
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(32.))
            .px(px(8.))
            .gap(px(8.))
            .flex()
            .items_center()
            .cursor(CursorStyle::IBeam)
            .on_click(cx.listener(|this, _, win, cx| {
                this.sidebar_search.update(cx, |ed, cx| ed.focus(win, cx))
            }))
            .child(
                div()
                    .size(px(16.))
                    .flex_none()
                    .opacity(if search_focused || has_query { 1. } else { 0.65 })
                    .group_hover(search_group, |style| style.opacity(1.))
                    .child(pro_icon(ProIcon::Search, 16., s.ink)),
            )
            .when(search_focused, |row| {
                let alpha = if window.last_input_was_keyboard() {
                    0.34
                } else {
                    0.20
                };
                row.child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .right(px(8.))
                        .bottom(px(1.))
                        .h(px(1.))
                        .rounded_full()
                        .bg(s.ink.opacity(alpha)),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(21.))
                    .child(self.sidebar_search.clone()),
            )
            .when(!self.sidebar_filter.is_empty(), |d| {
                d.child(
                    close_button("side-search-clear", loc.pick("清空搜索", "Clear search"), &s)
                        .on_press(cx, |this, win, cx| {
                            this.sidebar_search.update(cx, |ed, cx| {
                                ed.set_value("", win, cx);
                                ed.focus(win, cx);
                            });
                        }),
                )
            });
        let search_bar = div()
            .w_full()
            .flex_none()
            .flex()
            .px(px(8.))
            .pb(px(4.))
            .child(search);
        let undo_banner = self
            .sidebar_open
            .then(|| self.render_delete_undo(&s, cx))
            .flatten();
        let running_threads = self.running_thread_ids();
        let mut folders: Vec<_> = self
            .project_registry
            .projects
            .iter()
            .map(|project| {
                SidebarFolder::new(project.id.clone(), &project.root_path, &project.name)
            })
            .collect();
        let mut by_root: HashMap<PathBuf, usize> = folders
            .iter()
            .enumerate()
            .map(|(i, folder)| (folder.root.clone(), i))
            .collect();
        let scope = self.prefs.active_project_filter.as_deref();
        let grouped = scope.is_none();
        for entry in &self.chat_list {
            if self
                .deferred_delete_chat
                .as_ref()
                .is_some_and(|(id, _, _)| *id == entry.info.id)
            {
                continue;
            }
            let root = self
                .project_registry
                .project_root_for_chat(&entry.info.workspace, entry.info.id);
            let index = if let Some(index) = by_root.get(root) {
                *index
            } else {
                // Resolve aliases once per distinct workspace, including unregistered history.
                let canonical = root.canonicalize().unwrap_or_else(|_| root.to_owned());
                let index = *by_root.entry(canonical.clone()).or_insert_with(|| {
                    let name = canonical
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("workspace");
                    folders.push(SidebarFolder::new(
                        project_id_for_path(&canonical),
                        &canonical,
                        name,
                    ));
                    folders.len() - 1
                });
                by_root.insert(root.to_owned(), index);
                index
            };
            let folder = &mut folders[index];
            folder.last_activity = folder
                .last_activity
                .max(entry.last_activity_ms.unwrap_or(0));
            folder.entries.push(entry);
        }
        let parents = folder_disambiguators(&folders);
        match self.prefs.sidebar_project_order {
            crate::prefs::ProjectSortOrder::Recent => {
                folders.sort_by_key(|f| std::cmp::Reverse(f.last_activity))
            }
            crate::prefs::ProjectSortOrder::Alphabetical => {
                folders.sort_by_cached_key(|f| f.name.to_lowercase())
            }
            crate::prefs::ProjectSortOrder::Manual => {}
        }
        let thread_header = div()
            .w_full()
            .h(px(CTRL_H))
            .mt(px(if has_query { 2. } else { 12. }))
            .pl(px(9.))
            .gap(px(6.))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(s.font_size(FS_META))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.muted)
                    .child(if has_query { loc.pick("搜索结果", "Search results") } else { loc.pick("对话", "Chats") }),
            )
            .child(self.sidebar_project_picker(&folders, &parents, &s, window, cx));

        let settled_entries = filter_sidebar_folders(
            &mut folders, scope, &query, &parents, &self.settled_chats, self.projected.info.as_ref(),
        );
        let inbox_attention_count = self.projected.pending_requests.len();
        let mut list_col = div().relative().flex().flex_col().gap(px(1.)).when(
            show_chat_selection,
            |list| {
                let active_key: SharedString = match &self.access_mode {
                    ChatAccessMode::DemoShowcase => "nav-sample".into(),
                    ChatAccessMode::HttpReadOnly { .. } | ChatAccessMode::RemoteController { .. } => "nav-shared".into(),
                    _ => active_chat_id.map(|id| format!("chat-{id}").into()).unwrap_or_default(),
                };
                list.child(pro_pill_indicator(
                    "kar-sidebar-nav-pill", active_key, nav_sensor.clone(), R_CTRL, s.sidebar_selected, None, false,
                ))
            },
        );

        // ── Special active Demo / Shared / Remote thread cards ──────────────
        let shared_agent = self.projected.info.as_ref()
            .filter(|info| Some(info.id) == self.access_mode.selected_chat_id());
        if grouped && matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            list_col = list_col.child(self.render_demo_showcase_cards(in_conv.then_some(&nav_sensor), &s, cx));
        } else if grouped && let ChatAccessMode::RemoteController { ref host_name, .. } = self.access_mode {
            list_col = list_col.child(
                div()
                    .relative()
                    .w_full()
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(px(R_CTRL))
                    .when(in_conv, |d| d.child(pro_mark_selected(&nav_sensor)))
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(match shared_agent {
                                        Some(info) => thread_agent_mark("remote-thread-agent", &info.runtime, &s, cx).into_any_element(),
                                        None => render_radar_link_icon(13., s.pro.green).into_any_element(),
                                    })
                                    .child(
                                        div()
                                            .text_size(s.font_size(FS_META))
                                            .text_color(s.muted)
                                            .child("remote"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(s.font_size(FS_TAG))
                                    .text_color(s.pro.green_ink)
                                    .child("live"),
                            ),
                    )
                    .child(
                        thread_title("remote-thread-title", format!("{} {host_name}", loc.pick("远端主机：", "Remote:")), &s)
                            .w_full()
                            .flex_none()
                            .font_weight(FontWeight::MEDIUM),
                    ),
            );
        } else if grouped && self.access_mode.is_http_read_only() {
            list_col = list_col.child(
                div()
                    .relative()
                    .w_full()
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(px(R_CTRL))
                    .when(in_conv, |d| d.child(pro_mark_selected(&nav_sensor)))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(match shared_agent {
                        Some(info) => thread_agent_mark("shared-thread-agent", &info.runtime, &s, cx).into_any_element(),
                        None => pro_icon(ProIcon::Link, ICON_SM, s.text),
                    })
                    .child(
                        thread_title("shared-thread-title", loc.pick("共享的实时对话", "Shared live thread"), &s)
                            .font_weight(FontWeight::MEDIUM),
                    ),
            );
        }

        // Searching temporarily opens matching folders without rewriting the user's saved tree.
        let search_folds = window.use_keyed_state("sidebar-search-folds", cx, |_, _| {
            (String::new(), HashSet::<String>::new())
        });
        if search_folds.read(cx).0 != query {
            search_folds.update(cx, |(previous, folded), _| {
                previous.clone_from(&query);
                folded.clear();
            });
        }
        let has_results = folders.iter().any(|folder| grouped || !folder.entries.is_empty());
        for mut folder in folders {
            let collapsed = if has_query {
                search_folds.read(cx).1.contains(&folder.id)
            } else {
                self.project_registry.is_collapsed(&folder.id)
            };
            let parent = parents.get(&folder.root);
            let name = folder.label(&parents);
            if grouped {
                let group = SharedString::from(format!("project-folder-{}", folder.id));
                let focus = window.use_keyed_state(group.clone(), cx, |_, cx| cx.focus_handle());
                let focus = focus.read(cx).clone();
                let show_action = folder.entries.is_empty()
                    || window.last_input_was_touch()
                    || window.last_input_was_keyboard() && focus.contains_focused(window, cx);
                let id = folder.id.clone();
                let search_state = search_folds.clone();
                let toggle = Tactile::ghost(SharedString::from(format!("side-folder-{id}")), &s)
                    .flex_1()
                    .min_w_0()
                    .h(px(30.))
                    .px(px(8.))
                    .gap(px(8.))
                    .expanded(!collapsed)
                    .aria(name.clone())
                    .child(pro_icon(
                        if collapsed {
                            ProIcon::Folder
                        } else {
                            ProIcon::FolderOpen
                        },
                        ICON,
                        s.muted,
                    ))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(s.font_size(FS_UI))
                            .text_color(s.text)
                            .child(folder.name.clone()),
                    )
                    .when_some(parent, |row, parent| {
                        row.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(s.font_size(FS_META))
                                .text_color(s.muted)
                                .child(parent.clone()),
                        )
                    })
                    .on_press(cx, move |wb, _, cx| {
                        if has_query {
                            search_state.update(cx, |(_, folded), _| {
                                if !folded.remove(&id) {
                                    folded.insert(id.clone());
                                }
                            });
                            cx.notify();
                        } else {
                            wb.toggle_project_folder_collapsed(&id, cx);
                        }
                    });
                let new_label = format!(
                    "{} · {name}",
                    loc.pick("在此项目中新建对话", "New chat in project")
                );
                let add = icon_button_sm(
                    SharedString::from(format!("side-folder-new-{}", folder.id)),
                    ProIcon::NewChat,
                    new_label,
                    &s,
                )
                .on_press(cx, move |wb, win, cx| {
                    wb.select_project_and_new_chat(folder.root.clone(), win, cx)
                });
                list_col = list_col.child(
                    div()
                        .group(group.clone())
                        .track_focus(&focus)
                        .w_full()
                        .mt(px(3.))
                        .flex()
                        .items_center()
                        .child(toggle)
                        .child(
                            div()
                                .flex_none()
                                .when(!show_action, |d| {
                                    d.opacity(0.).group_hover(group, |s| s.opacity(1.))
                                })
                                .child(add),
                        ),
                );
                if collapsed {
                    continue;
                }
            }
            folder.entries.sort_by_key(|e| {
                let snoozed = self
                    .prefs
                    .snoozed_chats
                    .get(&e.info.id)
                    .is_some_and(|&wake| wake > now_ms);
                (
                    snoozed,
                    self.prefs.sidebar_working_section && !running_threads.contains(&e.info.id),
                    !self.project_registry.is_pinned(e.info.id),
                    std::cmp::Reverse(e.last_activity_ms.unwrap_or(0)),
                )
            });
            for entry in folder.entries {
                list_col = list_col.child(self.render_sidebar_thread_card(
                    running_threads.contains(&entry.info.id),
                    entry.clone(),
                    "project",
                    active_chat_id,
                    now_ms,
                    self.settled_chats.contains(&entry.info.id),
                    Some(&name),
                    &nav_sensor,
                    &s,
                    window,
                    cx,
                ));
            }
        }

        // ── Open Kantor Office Documents shelf (shown when ≥ 1 doc is open) ─
        let doc_matches = |tab: &crate::office_host::OfficeTab| {
            clean_display_doc_name(&tab.doc_name(cx))
                .to_lowercase()
                .contains(&query)
        };
        let has_docs = self.office_tabs.iter().any(doc_matches);
        if !has_results && (has_query && !has_docs || !grouped && !has_query) {
            let message = if has_query {
                loc.pick("未找到匹配项", "No matches")
            } else if settled_entries.is_empty() {
                loc.pick("此项目还没有对话", "No chats in this project yet")
            } else {
                loc.pick("此项目没有活跃对话", "No active chats in this project")
            };
            list_col = list_col.child(
                div().px(px(9.)).py(px(16.)).text_size(s.font_size(FS_META)).text_color(s.muted)
                    .child(message),
            );
        }
        if has_docs {
            let mut docs_col = div()
                .mt(px(8.))
                .pt(px(8.))
                .border_t_1()
                .border_color(s.line_soft)
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .px(px(6.))
                        .pb(px(2.))
                        .text_size(s.font_size(FS_TAG))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.faint)
                        .child(loc.pick("打开的文档", "Open documents")),
                );
            for (idx, tab) in self.office_tabs.iter().enumerate() {
                let name = clean_display_doc_name(&tab.doc_name(cx));
                if !query.is_empty() && !name.to_lowercase().contains(&query) {
                    continue;
                }
                let in_front = docs_shown
                    && self.right_surface == RightPaneSurface::Office
                    && self.active_office_tab == Some(idx);
                let dirty = tab.is_dirty(cx);
                let tab_id = tab.id;
                let grp = SharedString::from(format!("side-doc-grp-{tab_id}"));
                let row_focus = window.use_keyed_state(grp.clone(), cx, |_, cx| cx.focus_handle());
                let row_focus = row_focus.read(cx).clone();
                let keyboard = window.last_input_was_keyboard() && row_focus.contains_focused(window, cx);
                docs_col = docs_col.child(
                    div()
                        .relative()
                        .w_full()
                        .h(px(34.))
                        .group(grp.clone())
                        .track_focus(&row_focus)
                        .flex()
                        .items_center()
                        .child(
                            Tactile::ghost(SharedString::from(format!("side-doc-{tab_id}")), &s)
                                .lit(in_front)
                                .lit_fill(s.sidebar_selected)
                                .flex_1()
                                .min_w_0()
                                .h(px(34.))
                                .pl(px(8.))
                                .pr(px(4.))
                                .gap(px(7.))
                                .child(render_office_chip(tab.kind, &s))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(s.font_size(FS_META))
                                        .font_weight(if in_front {
                                            FontWeight::MEDIUM
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(if in_front { s.ink } else { s.text })
                                        .child(name),
                                )
                                .when(dirty, |row| row.child(status_dot(s.pro.amber)))
                                .when(tab.is_demo, |row| row.child(render_sample_tag(&s)))
                                .on_press(cx, move |this, win, cx| this.activate_office_tab(idx, win, cx)),
                        )
                        .child(
                            div()
                                .flex_none()
                                .mr(px(4.))
                                .when(!keyboard, |d| d.opacity(0.).group_hover(grp, |st| st.opacity(1.)))
                                .child(
                                    close_button(
                                        SharedString::from(format!("side-doc-close-{tab_id}")),
                                        loc.pick("关闭文档", "Close document"),
                                        &s,
                                    )
                                    .on_press(cx, move |this, win, cx| {
                                        this.close_office_tab(tab_id, win, cx)
                                    }),
                                ),
                        ),
                );
            }
            list_col = list_col.child(docs_col);
        }

        let _ = settled_entries;

        let nav_row = |id, icon, label: &'static str, selected| {
            Tactile::ghost(id, &s)
                .w_full()
                .h(px(CTRL_H))
                .px(px(9.))
                .gap(px(10.))
                .flex_none()
                .aria(label)
                .lit(selected)
                .lit_fill(s.sidebar_selected)
                .radius(R_CTRL)
                .child(pro_icon(icon, ICON, s.text))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(s.font_size(FS_UI))
                        .line_height(s.line_height(LH_UI))
                        .text_color(s.ink)
                        .child(label),
                )
        };
        let more = menu_host(
            nav_row(
                "side-more",
                ProIcon::MoreHorizontal,
                loc.pick("更多操作", "More actions"),
                self.open_menu == Some(Menu::SidebarMore),
            )
            .on_press(cx, |this, _, cx| this.toggle_menu(Menu::SidebarMore, cx)),
            self.render_sidebar_more_menu(&s, cx),
        );
        let navigation = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(
                nav_row(
                    "side-new-thread-btn",
                    ProIcon::Plus,
                    loc.pick("新建对话", "New chat"),
                    on_start_screen && !git_selected && !browser_selected && !self.settings_open,
                )
                .on_press(cx, |this, win, cx| {
                    if let Some(root) = this.prefs.active_project_filter.clone() {
                        this.select_project_and_new_chat(root, win, cx);
                    } else {
                        this.start_new_chat(win, cx);
                    }
                }),
            )
            .child(
                nav_row(
                    "side-nav-tasks-inbox",
                    ProIcon::Inbox,
                    loc.pick("收件箱", "Inbox"),
                    self.main_stage_view == MainStageView::TasksInbox,
                )
                .when(inbox_attention_count > 0, |row| {
                    row.child(
                        div()
                            .text_size(s.font_size(FS_META))
                            .font_features(tnum_features())
                            .text_color(s.pro.amber_ink)
                            .child(inbox_attention_count.to_string()),
                    )
                })
                .on_press(cx, |this, _, cx| this.open_tasks_inbox_stage(cx)),
            )
            .child(
                nav_row(
                    "side-nav-office-docs",
                    ProIcon::PageDoc,
                    loc.pick("文档", "Documents"),
                    self.main_stage_view == MainStageView::OfficeGallery,
                )
                .on_press(cx, |this, _, cx| this.open_office_gallery_stage(cx)),
            )
            .child(
                nav_row(
                    "side-nav-browser",
                    ProIcon::Globe,
                    loc.pick("浏览器", "Browser"),
                    browser_selected && !self.settings_open,
                )
                .when(!self.detected_dev_ports.is_empty(), |row| {
                    row.child(status_dot(s.pro.green))
                })
                .on_press(cx, |this, _, cx| this.open_browser_dock_tab(cx)),
            )
            .child(
                nav_row(
                    "side-nav-git-changes",
                    ProIcon::GitBranch,
                    loc.pick("Git 变更", "Git changes"),
                    git_selected && !self.settings_open,
                )
                .on_press(cx, |this, _, cx| this.open_git_workbench_tab(cx)),
            )
            .child(more);
        let settings_footer = div()
            .flex_none()
            .mx(px(8.))
            .border_t_1()
            .border_color(s.line_soft)
            .py(px(6.))
            .child(
                nav_row(
                    "side-footer-settings",
                    ProIcon::Settings,
                    loc.pick("设置", "Settings"),
                    self.settings_open,
                )
                .on_press(cx, |this, _, cx| this.toggle_settings(SettingsTab::General, cx)),
            );

        // Keep search pinned. On short windows scrolling reaches both navigation and history;
        // a search brings matching threads directly beneath the field.
        let scroll = window.use_keyed_state("sidebar-query-scroll", cx, |_, _| {
            (String::new(), None::<PathBuf>, ScrollHandle::new())
        });
        let scroll = scroll.update(cx, |(previous, previous_scope, scroll), _| {
            if previous != &query || previous_scope.as_deref() != scope {
                previous.clone_from(&query);
                *previous_scope = scope.map(Path::to_owned);
                scroll.set_offset(point(px(0.), px(0.)));
            }
            scroll.clone()
        });
        let body_scroll = div()
            .id("kar-sidebar-scroll")
            .track_scroll(&scroll)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(8.))
            .pb(px(8.))
            .when(!has_query, |d| d.child(navigation))
            .child(thread_header)
            .when_some(undo_banner, |d, b| d.child(b))
            .child(list_col);

        let has_running_task = self.sidebar_open && self.has_running_task();
        div()
            .track_focus(&self.sidebar_focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_r_1()
            .border_color(s.line)
            .child(sidebar_texture(&s, width, has_running_task, window))
            .child(top_bar)
            .child(search_bar)
            .child(body_scroll)
            .child(settings_footer)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_sidebar_thread_card(
        &self,
        running: bool,
        entry: ChatListEntry,
        prefix: &'static str,
        active_chat_id: Option<uuid::Uuid>,
        now_ms: u64,
        is_settled: bool,
        folder_name: Option<&str>,
        nav_sensor: &kui::components::pro_chrome::ProSelectionSensor,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let loc = self.ui_locale;
        let chat_id = entry.info.id;
        let selected = active_chat_id == Some(chat_id)
            && matches!(
                self.access_mode,
                ChatAccessMode::WriteController { .. }
                    | ChatAccessMode::LocalReadOnlySubscriber { .. }
                    | ChatAccessMode::ArchivedReadOnly { .. }
            );
        let unread = !running && self.unread_completion(&entry).is_some();
        let needs_approval = selected && !self.projected.pending_requests.is_empty();
        let pinned = self.project_registry.is_pinned(chat_id);
        let has_assoc = self.client_reducer.association_for(&chat_id).is_some();
        let snoozed = self.prefs.snoozed_chats.get(&chat_id).is_some_and(|&wake| wake > now_ms);

        let proj_name = folder_name.unwrap_or_else(|| {
            self.project_registry.project_for_chat(&entry.info.workspace, chat_id)
                .map(|p| p.name.as_str())
                .or_else(|| entry.info.workspace.file_name()?.to_str())
                .map_or("Kwork", project_display_name)
        });

        let display_title = if entry.turn_count == 0 && entry.title == "新对话" {
            loc.pick("新对话", "New thread").to_owned()
        } else {
            entry.title.clone()
        };
        let age_txt = localized_relative_age(entry.last_activity_ms, now_ms, loc);
        let row_id = SharedString::from(format!("thread-{prefix}-{chat_id}"));
        let row_focus = window.use_keyed_state(row_id.clone(), cx, |_, cx| cx.focus_handle());
        let row_focus = row_focus.read(cx).clone();
        let keyboard = window.last_input_was_keyboard() && row_focus.contains_focused(window, cx);
        let menu = Menu::ThreadActions(chat_id);
        let menu_open = self.open_menu == Some(menu);
        let show_actions = keyboard || menu_open || window.last_input_was_touch();
        let in_conv = self.main_stage_view == MainStageView::Conversation;
        let archived = self
            .sidebar_metadata
            .get(&chat_id)
            .is_some_and(|meta| meta.archived);
        let location = if archived {
            loc.pick("已归档", "Archived")
        } else if entry.info.worktree_id.is_some() {
            loc.pick("工作树", "Worktree")
        } else {
            loc.pick("本机", "Local")
        };
        let runtime = entry.runtime(self.projected.info.as_ref());
        let context = format!("{proj_name} · {location} · {}", runtime_display_name(runtime));
        let mut row_label = format!("{display_title} · {context}");
        for (visible, label) in [
            (pinned, loc.pick("已置顶", "Pinned")),
            (has_assoc, loc.pick("已关联任务", "Linked task")),
            (snoozed, loc.pick("稍后提醒", "Snoozed")),
            (needs_approval, loc.pick("待确认", "Approval needed")),
            (running && !needs_approval, loc.pick("运行中", "Working")),
            (unread, loc.pick("新回复", "New reply")),
        ] {
            if visible {
                row_label.push_str(" · ");
                row_label.push_str(label);
            }
        }
        let main = Tactile::ghost(
            SharedString::from(format!("chat-card-{prefix}-{chat_id}")),
            s,
        )
        .aria(row_label)
        .lit(in_conv && selected && is_settled)
        .lit_fill(s.sidebar_selected)
        .w_full()
        .h(px(THREAD_ROW_H))
        .px(px(8.))
        .gap(px(8.))
        .child(thread_agent_mark(
            SharedString::from(format!("thread-agent-{prefix}-{chat_id}")), runtime, s, cx,
        ))
        .child(thread_title(
            SharedString::from(format!("thread-title-{prefix}-{chat_id}")), display_title.clone(), s,
        ))
        .when(snoozed, |row| row.child(pro_icon(ProIcon::Clock, ICON_SM, s.muted)))
        .when(pinned, |row| row.child(pro_icon(ProIcon::Pin, ICON_SM, s.muted)))
        .when(has_assoc, |row| row.child(render_schedule_repeat_icon(ICON_SM, s.muted)))
        .child({
            let trailing = div().flex_none().h(px(ICON)).min_w(px(ICON))
                .flex().items_center().justify_center()
                .text_size(s.font_size(FS_META)).font_features(tnum_features()).text_color(s.muted);
            if needs_approval {
                trailing.text_color(s.pro.amber_ink).child(loc.pick("待确认", "Approval"))
            } else if running {
                trailing.child(crate::ui_motion::spinner(ICON, s.muted, window, cx))
            } else if unread {
                trailing
                    .when(show_actions, |d| d.ml(px(CTRL_SM_H)))
                    .when(!show_actions, |d| d.group_hover(row_id.clone(), |st| st.ml(px(CTRL_SM_H))))
                    .child(div().size(px(7.)).rounded_full().bg(s.ink))
            } else {
                trailing.when(show_actions, |d| d.invisible())
                    .when(!show_actions, |d| d.group_hover(row_id.clone(), |st| st.invisible()))
                    .child(age_txt)
            }
        })
        .on_press(cx, move |this, win, cx| {
            this.pending_delete_chat = None;
            this.open_chat(chat_id, win, cx);
        });
        let mut row = div()
            .id(row_id.clone())
            .group(row_id.clone())
            .track_focus(&row_focus)
            .relative()
            .w_full()
            .rounded(px(R_CTRL))
            .when(in_conv && selected && !is_settled, |d| {
                d.child(pro_mark_selected(nav_sensor))
            })
            .child(main);
        if !running {
            let trigger =
                Tactile::ghost(SharedString::from(format!("thread-actions-{chat_id}")), s)
                    .size(px(CTRL_SM_H))
                    .justify_center()
                    .radius(R_CTRL)
                    .aria(format!(
                        "{} · {display_title}",
                        loc.pick("对话操作", "Chat actions")
                    ))
                    .lit(menu_open)
                    .child(pro_icon(ProIcon::MoreHorizontal, ICON, s.text))
                    .on_press(cx, move |this, _, cx| {
                        cx.stop_propagation();
                        this.pending_delete_chat = None;
                        this.toggle_menu(menu, cx);
                    });
            let mut actions = div()
                .absolute()
                .right(px(if unread { 3. + CTRL_SM_H } else { 3. }))
                .top(px((THREAD_ROW_H - CTRL_SM_H) / 2.))
                .when(!show_actions, |d| {
                    d.opacity(0.).group_hover(row_id, |st| st.opacity(1.))
                })
                .child(trigger);
            if menu_open {
                actions = actions.child(self.render_thread_action_menu(
                    &entry,
                    display_title,
                    context,
                    is_settled,
                    archived,
                    pinned,
                    s,
                    cx,
                ));
            }
            row = row.child(actions);
        }
        row
    }

    #[allow(clippy::too_many_arguments)]
    fn render_thread_action_menu(
        &self,
        entry: &ChatListEntry,
        display_title: String,
        context: String,
        is_settled: bool,
        archived: bool,
        pinned: bool,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let chat_id = entry.info.id;
        let title = entry.title.clone();
        let loc = self.ui_locale;
        let now_ms = now_epoch_ms();
        let confirming_del = self
            .pending_delete_chat
            .is_some_and(|(id, at)| id == chat_id && at.elapsed() < DELETE_CONFIRM_TTL);
        let action = |id: &'static str, icon: ProIcon, label: &'static str| {
            menu_icon_item(
                SharedString::from(format!("thread-{id}-{chat_id}")),
                icon,
                label,
                s,
            )
        };
        let is_snoozed = self
            .prefs
            .snoozed_chats
            .get(&chat_id)
            .is_some_and(|&wake| wake > now_ms);
        let mut items = vec![menu_caption(context, s).into_any_element()];
        if !archived {
            items.push(
                action(
                    "pin",
                    ProIcon::Pin,
                    if pinned {
                        loc.pick("取消置顶", "Unpin")
                    } else {
                        loc.pick("置顶对话", "Pin")
                    },
                )
                .on_press(cx, move |wb, _, cx| {
                    wb.confirm_or_run(
                        format!("unpin-{chat_id}"),
                        pinned && wb.prefs.confirm_unpin,
                        loc.pick("再次点击确认取消置顶", "Click again to unpin"),
                        cx,
                        move |wb, cx| {
                            wb.close_menu(cx);
                            wb.toggle_pin_chat(chat_id, cx);
                        },
                    );
                })
                .into_any_element(),
            );
        }
        items.push(
            action("rename", ProIcon::NewChat, loc.pick("重命名", "Rename"))
                .on_press(cx, move |wb, win, cx| {
                    wb.open_rename_chat_dialog(chat_id, &title, win, cx)
                })
                .into_any_element(),
        );
        items.push(
            action(
                "snooze",
                ProIcon::Clock,
                if is_snoozed {
                    loc.pick("取消稍后提醒", "Wake thread")
                } else {
                    loc.pick("稍后提醒 · 2 小时", "Snooze for 2 hours")
                },
            )
            .on_press(cx, move |wb, _, cx| {
                wb.close_menu(cx);
                if wb.prefs.snoozed_chats.remove(&chat_id).is_some() {
                    wb.notify_toast(loc.pick("已取消稍后提醒", "Reminder cleared"), cx);
                } else {
                    wb.prefs
                        .snoozed_chats
                        .insert(chat_id, now_epoch_ms() + 2 * 3_600_000);
                    wb.notify_toast(loc.pick("已设为 2 小时后提醒", "Snoozed for 2 hours"), cx);
                }
                wb.save_prefs();
            })
            .into_any_element(),
        );
        if !archived {
            items.push(
                action(
                    "settle",
                    ProIcon::Check,
                    if is_settled {
                        loc.pick("移回活跃列表", "Move to active")
                    } else {
                        loc.pick("标记为已结项", "Mark settled")
                    },
                )
                .on_press(cx, move |wb, _, cx| {
                    wb.confirm_or_run(
                        format!("settle-{chat_id}"),
                        !is_settled && wb.prefs.confirm_archive,
                        loc.pick(
                            "再次点击确认将对话标记为已结项",
                            "Click again to mark settled",
                        ),
                        cx,
                        move |wb, cx| {
                            wb.close_menu(cx);
                            wb.toggle_settle_chat(chat_id, cx);
                        },
                    );
                })
                .into_any_element(),
            );
        }
        items.push(
            action(
                "archive",
                ProIcon::Inbox,
                if archived {
                    loc.pick("取消归档", "Unarchive")
                } else {
                    loc.pick("归档对话", "Archive")
                },
            )
            .on_press(cx, move |wb, _, cx| {
                wb.confirm_or_run(
                    format!("archive-thread-{chat_id}"),
                    !archived && wb.prefs.confirm_archive,
                    loc.pick("再次点击归档对话", "Click again to archive"),
                    cx,
                    move |wb, cx| {
                        wb.close_menu(cx);
                        wb.bridge
                            .send(crate::kar_bridge::KarCommand::UpdateSidebar {
                                chat_id,
                                action: crate::kar_bridge::SidebarAction::Archive(!archived),
                            });
                    },
                );
            })
            .into_any_element(),
        );
        items.push(menu_divider(s).into_any_element());
        items.push(
            action(
                "delete",
                ProIcon::Trash,
                if confirming_del {
                    loc.pick("确认删除对话", "Confirm deletion")
                } else {
                    loc.pick("删除对话", "Delete chat")
                },
            )
            .text_color(s.pro.red_ink)
            .on_press(cx, move |wb, _, cx| {
                if wb.prefs.confirm_delete && !wb.pending_delete_chat.is_some_and(|(id, at)| id == chat_id && at.elapsed() < DELETE_CONFIRM_TTL) {
                    wb.pending_delete_chat = Some((chat_id, std::time::Instant::now()));
                    cx.notify();
                } else {
                    wb.close_menu(cx);
                    wb.stage_deferred_chat_delete(chat_id, display_title.clone(), cx);
                }
            })
            .into_any_element(),
        );
        self.render_menu(
            Menu::ThreadActions(chat_id),
            MenuPlacement::BelowEnd,
            220.,
            items,
            s,
            cx,
        )
    }

    /// Example threads shown only in explicit demo mode.
    fn render_demo_showcase_cards(
        &self,
        nav_sensor: Option<&kui::components::pro_chrome::ProSelectionSensor>,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let show_companions = self.chat_list.is_empty();
        let mut col = div().flex().flex_col().gap(px(3.));

        if show_companions {
            // Companion 1: Approval badge card (`harbor-api`)
            col = col.child(demo_static_card(
                "demo-comp-approval",
                "harbor-api",
                loc.pick(
                    "为公开搜索接口增加限流保护",
                    "Rate limit the public search endpoint",
                ),
                Some("feat/search-rate-limit"),
                "codex",
                div()
                    .flex()
                    .items_center()
                    .gap(px(3.5))
                    .text_size(s.font_size(FS_META))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.pro.amber_ink)
                    .child(render_approval_hand(12., s.pro.amber_ink))
                    .child(loc.pick("待确认", "Approval"))
                    .into_any_element(),
                false,
                None,
                s,
                cx,
            ));
        }

        // Active selected Demo thread card
        col = col.child(demo_static_card(
            "nav-row-sample",
            "kar-studio",
            loc.pick(
                "重连时自动同步离线队列修改",
                "Sync offline edits on reconnect",
            ),
            Some("feat/offline-sync"),
            "claude",
            div()
                .font_features(tnum_features())
                .text_size(s.font_size(FS_META))
                .text_color(s.muted)
                .child("24m")
                .into_any_element(),
            nav_sensor.is_some(),
            nav_sensor,
            s,
            cx,
        ));

        if show_companions {
            col = col
                .child(demo_static_card(
                    "demo-comp-flaky",
                    "kar-studio",
                    loc.pick(
                        "修复结算流程偶发超时测试",
                        "Fix the flaky checkout test",
                    ),
                    Some("fix/flaky-checkout-test"),
                    "codex",
                    div()
                        .font_features(tnum_features())
                        .text_size(s.font_size(FS_META))
                        .text_color(s.muted)
                        .child("41m")
                        .into_any_element(),
                    false,
                    None,
                    s,
                    cx,
                ))
                .child(demo_static_card(
                    "demo-comp-done",
                    "fieldnotes",
                    loc.pick(
                        "重写快速上手指南与预算表说明",
                        "Rewrite the quickstart guide",
                    ),
                    None,
                    "opencode",
                    div()
                        .flex()
                        .items_center()
                        .gap(px(3.5))
                        .text_size(s.font_size(FS_META))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.pro.green_ink)
                        .child(render_circle_check_icon(12., s.pro.green))
                        .child(loc.pick("已完成", "Done"))
                        .into_any_element(),
                    false,
                    None,
                    s,
                    cx,
                ))
                .child(demo_static_card(
                    "demo-comp-node",
                    "harbor-api",
                    loc.pick("升级运行时依赖至最新 LTS", "Upgrade to Node 24"),
                    Some("chore/node-24"),
                    "claude",
                    div()
                        .font_features(tnum_features())
                        .text_size(s.font_size(FS_META))
                        .text_color(s.muted)
                        .child("2h")
                        .into_any_element(),
                    false,
                    None,
                    s,
                    cx,
                ));
        }

        col
    }

    fn set_sidebar_project_filter(
        &mut self,
        root: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.prefs.active_project_filter = root;
        self.save_prefs();
        self.open_menu = None;
        cx.notify();
    }

    fn sidebar_project_picker(
        &self,
        folders: &[SidebarFolder<'_>],
        parents: &HashMap<PathBuf, String>,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let filter_path = self.prefs.active_project_filter.as_deref();
        let active = folders.iter().position(|folder| Some(folder.root.as_path()) == filter_path);
        let scope = active.map(|i| folders[i].label(parents)).unwrap_or_else(|| {
            filter_path.and_then(|path| path.file_name()?.to_str())
                .map(project_display_name).unwrap_or(loc.pick("全部项目", "All projects")).to_owned()
        });
        let open = self.open_menu == Some(Menu::SidebarFilter);
        let scope_label = format!(
            "{} · {scope}",
            loc.pick("按项目筛选对话", "Filter chats by project")
        );
        let trigger = Tactile::ghost("side-project-scope", s)
            .max_w_full()
            .min_w_0()
            .h(px(CTRL_SM_H))
            .px(px(6.))
            .gap(px(4.))
            .radius(R_CTRL)
            .lit(open)
            .expanded(open)
            .ink(
                if filter_path.is_some() {
                    s.text
                } else {
                    s.muted
                },
                s.ink,
            )
            .aria(scope_label)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(s.font_size(FS_META))
                    .child(scope),
            )
            .child(
                div()
                    .flex_none()
                    .child(pro_icon(ProIcon::ChevronDown, 11., s.muted)),
            )
            .on_press(cx, |this, _, cx| this.toggle_menu(Menu::SidebarFilter, cx));

        let current = active.map_or(0, |index| index + 1);
        let last = folders.len() + 1;
        let navigation = window.use_keyed_state("side-project-options", cx, |_, _| {
            (None::<usize>, ScrollHandle::new())
        });
        if !open && navigation.read(cx).0.is_some() {
            navigation.update(cx, |state, _| state.0 = None);
        }
        let (cursor, scroll) = navigation.read(cx).clone();
        let all = menu_icon_item(
            "side-flt-all",
            ProIcon::List,
            loc.pick("全部项目", "All projects"),
            s,
        )
        .lit(cursor == Some(0))
        .when(filter_path.is_none(), |row| row.child(menu_check(s)))
        .on_press(cx, |this, _, cx| this.set_sidebar_project_filter(None, cx));
        let mut rows = div()
            .id("side-project-options-list")
            .max_h(px(
                (f32::from(window.viewport_size().height) - 32.).clamp(36., 348.)
            ))
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .child(all)
            .child(menu_divider(s));
        for (index, folder) in folders.iter().enumerate() {
            let root = folder.root.clone();
            rows = rows.child(
                menu_icon_item(("side-flt-proj", index), ProIcon::Folder, folder.label(parents), s)
                    .lit(cursor == Some(index + 1))
                    .when(index + 1 == current, |row| row.child(menu_check(s)))
                    .on_press(cx, move |this, _, cx| {
                        this.set_sidebar_project_filter(Some(root.clone()), cx)
                    }),
            );
        }
        rows = rows.child(menu_divider(s)).child(
            menu_icon_item(
                "side-add-project",
                ProIcon::FolderPlus,
                loc.pick("添加项目…", "Add project…"),
                s,
            )
            .lit(cursor == Some(last))
            .on_press(cx, |this, win, cx| {
                this.open_menu = None;
                this.add_project_folder(win, cx);
            }),
        );
        let menu = self.render_menu(
            Menu::SidebarFilter,
            MenuPlacement::BelowEnd,
            228.,
            vec![rows.into_any_element()],
            s,
            cx,
        );
        let paths: Vec<_> = folders.iter().map(|folder| folder.root.clone()).collect();
        menu_host(trigger, menu)
            .max_w(relative(0.66))
            .capture_key_down(cx.listener(move |this, ev: &KeyDownEvent, win, cx| {
                if this.open_menu != Some(Menu::SidebarFilter) || ev.keystroke.modifiers.modified()
                {
                    return;
                }
                let selected = cursor.unwrap_or(current).min(last);
                let next = match ev.keystroke.key.as_str() {
                    "escape" => {
                        this.close_menu(cx);
                        win.prevent_default();
                        cx.stop_propagation();
                        return;
                    }
                    "down" => (selected + 1) % (last + 1),
                    "up" => (selected + last) % (last + 1),
                    "home" => 0,
                    "end" => last,
                    "enter" | "space" => {
                        if selected == last {
                            this.open_menu = None;
                            this.add_project_folder(win, cx);
                        } else {
                            let root = selected
                                .checked_sub(1)
                                .map(|index| paths[index].clone());
                            this.set_sidebar_project_filter(root, cx);
                        }
                        win.prevent_default();
                        cx.stop_propagation();
                        return;
                    }
                    _ => return,
                };
                navigation.update(cx, |(cursor, scroll), _| {
                    *cursor = Some(next);
                    scroll.scroll_to_item(next + usize::from(next > 0) + usize::from(next == last));
                });
                win.prevent_default();
                cx.stop_propagation();
                cx.notify();
            }))
    }

    fn render_sidebar_more_menu(
        &self,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let rows = vec![
            menu_icon_item(
                "side-more-palette",
                ProIcon::Search,
                loc.pick("命令面板", "Command palette"),
                s,
            )
            .child(menu_hint("⌘K", s))
            .on_press(cx, |this, window, cx| {
                this.open_menu = None;
                this.toggle_command_palette(window, cx);
            })
            .into_any_element(),
            menu_icon_item(
                "side-more-archived",
                ProIcon::Inbox,
                loc.pick("已结项与归档…", "Settled & archived…"),
                s,
            )
            .on_press(cx, |this, _, cx| {
                this.open_menu = None;
                this.open_settings(SettingsTab::Archived, cx);
            })
            .into_any_element(),
            menu_divider(s).into_any_element(),
            menu_icon_item(
                "side-more-remote",
                ProIcon::Link,
                loc.pick("远端短码双向配对…", "Remote 6-digit pairing…"),
                s,
            )
            .on_press(cx, |this, _, cx| {
                this.open_settings(SettingsTab::Remote, cx);
            })
            .into_any_element(),
            menu_icon_item(
                "side-more-link",
                ProIcon::Globe,
                loc.pick("打开共享只读链接…", "Open shared feed link…"),
                s,
            )
            .on_press(cx, |this, window, cx| {
                this.open_shared_link_dialog(window, cx)
            })
            .into_any_element(),
            menu_divider(s).into_any_element(),
            menu_caption(loc.pick("演示与实验", "Demos & workshops"), s).into_any_element(),
            menu_icon_item(
                "side-more-sample-chat",
                ProIcon::Thread,
                loc.pick("示例对话", "Demo showcase thread"),
                s,
            )
            .on_press(cx, |this, window, cx| {
                this.open_menu = None;
                this.open_sample_chat(window, cx);
            })
            .into_any_element(),
            menu_icon_item(
                "side-more-coedit",
                ProIcon::GitBranch,
                loc.pick("双写协同演练", "Co-editing workshop"),
                s,
            )
            .on_press(cx, |this, window, cx| {
                this.enter_coedit_demo_workshop(window, cx)
            })
            .into_any_element(),
        ];
        self.render_menu(
            Menu::SidebarMore,
            MenuPlacement::BelowStart,
            236.,
            rows,
            s,
            cx,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn demo_static_card(
    id: &'static str,
    proj: &str,
    title: &str,
    branch: Option<&str>,
    runtime: &str,
    right_status: AnyElement,
    selected: bool,
    sensor: Option<&kui::components::pro_chrome::ProSelectionSensor>,
    s: &StudioPalette,
    cx: &App,
) -> Stateful<Div> {
    let project = branch.map_or_else(|| proj.to_owned(), |branch| format!("{proj} · {branch}"));
    let context = SharedString::from(format!("{title} · {project} · {}", runtime_display_name(runtime)));
    div()
        .id(id)
        .relative()
        .w_full()
        .h(px(THREAD_ROW_H))
        .px(px(9.))
        .rounded(px(R_CTRL))
        .when(!selected, |d| d.hover(|h| h.bg(s.sidebar_hover)))
        .when_some(sensor, |d, sn| d.child(pro_mark_selected(sn)))
        .flex()
        .items_center()
        .gap(px(10.))
        .child(thread_agent_mark(SharedString::from(format!("{id}-agent")), runtime, s, cx))
        .child(
            thread_title(SharedString::from(format!("{id}-title")), title.to_owned(), s)
                .tooltip(move |window, cx| Tooltip::new(context.clone()).build(window, cx)),
        )
        .child(right_status)
}

struct TitleOverflow {
    origin: Point<Pixels>,
    solid: Range<Pixels>,
    edge: Range<Pixels>,
    rtl: bool,
}

impl TitleOverflow {
    fn layout(
        title: &str,
        width: Pixels,
        bounds: Bounds<Pixels>,
        max_fade: Pixels,
        snap: impl FnOnce(Pixels) -> Pixels,
    ) -> Option<Self> {
        if width <= bounds.size.width {
            return None;
        }
        let rtl = unicode_bidi::get_base_direction(title) == unicode_bidi::Direction::Rtl;
        let fade = max_fade.min(bounds.size.width / 2.);
        let (x, solid, edge) = if rtl {
            let boundary = snap(bounds.left() + fade);
            (bounds.right() - width, boundary..bounds.right(), bounds.left()..boundary)
        } else {
            let boundary = snap(bounds.right() - fade);
            (bounds.left(), bounds.left()..boundary, boundary..bounds.right())
        };
        Some(Self { origin: point(x, bounds.top()), solid, edge, rtl })
    }

    fn opacity(&self, fraction_from_left: f32) -> f32 {
        if self.rtl { fraction_from_left } else { 1. - fraction_from_left }
    }
}

fn thread_title(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    s: &StudioPalette,
) -> Stateful<Div> {
    let title = title.into();
    let font_size = s.font_size(FS_UI);
    let ink = s.ink;
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(title.clone())
        .flex_1()
        .min_w_0()
        .h(s.line_height(LH_UI))
        .text_color(ink)
        .child(canvas(
            move |bounds, window, _| {
                if bounds.is_empty() || !bounds.intersects(&window.content_mask().bounds) {
                    return None;
                }
                let title = if title.contains(['\r', '\n']) {
                    title.replace(['\r', '\n'], " ").into()
                } else {
                    title
                };
                let run = window.text_style().to_run(title.len());
                Some(window.text_system().shape_line(title, font_size, &[run], None))
            },
            move |bounds, line, window, cx| {
                let Some(line) = line else { return };
                let height = bounds.size.height;
                let Some(layout) = TitleOverflow::layout(
                    line.text.as_ref(), line.width(), bounds, font_size * 1.6, |x| window.pixel_snap(x),
                ) else {
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        let _ = line.paint(bounds.origin, height, TextAlign::Left, None, window, cx);
                    });
                    return;
                };
                let origin = layout.origin;
                let edge = &layout.edge;
                let slice = |left, right| {
                    Bounds::from_corners(point(left, bounds.top()), point(right, bounds.bottom()))
                };
                let mask = ContentMask { bounds: slice(layout.solid.start, layout.solid.end) };
                window.with_content_mask(Some(mask), |window| {
                    let _ = line.paint(origin, height, TextAlign::Left, None, window, cx);
                });

                // Fade the glyphs themselves so hover and selection show through color emoji too.
                let baseline = origin + point(px(0.), (height + line.ascent - line.descent) / 2.);
                let mut edge_glyphs = Vec::new();
                for run in &line.runs {
                    let extent = window.text_system().bounding_box(run.font_id, font_size);
                    for glyph in &run.glyphs {
                        let origin = baseline + glyph.position;
                        // Include a pixel on either side for the native antialiasing fringe.
                        let left = origin.x + extent.left() - px(1.);
                        let right = origin.x + extent.right() + px(1.);
                        if right > edge.start && left < edge.end {
                            edge_glyphs.push((run.font_id, glyph, origin, left, right));
                        }
                    }
                }
                const BANDS: u32 = 16;
                for i in 0..BANDS {
                    let start_fraction = i as f32 / BANDS as f32;
                    let end_fraction = (i + 1) as f32 / BANDS as f32;
                    let at = |fraction| window.pixel_snap(edge.start + (edge.end - edge.start) * fraction);
                    let clip = slice(at(start_fraction), at(end_fraction));
                    if clip.is_empty() {
                        continue;
                    }
                    let midpoint = (start_fraction + end_fraction) / 2.;
                    let alpha = layout.opacity(midpoint);
                    div().overflow_hidden().opacity(alpha).interactivity().paint(
                        None, None, clip, None, window, cx, |_, window, _| {
                            for &(font, glyph, origin, left, right) in &edge_glyphs {
                                if right > clip.left() && left < clip.right() {
                                    let _ = if glyph.is_emoji {
                                        window.paint_emoji(origin, font, glyph.id, font_size)
                                    } else {
                                        window.paint_glyph(origin, font, glyph.id, font_size, ink)
                                    };
                                }
                            }
                        },
                    );
                }
            },
        ).size_full())
}

fn status_dot(color: Hsla) -> Div {
    div().size(px(6.)).flex_none().rounded_full().bg(color)
}

fn thread_agent_mark(
    id: impl Into<ElementId>,
    runtime: &str,
    s: &StudioPalette,
    cx: &App,
) -> Stateful<Div> {
    let name = SharedString::from(runtime_display_name(runtime));
    div()
        .id(id)
        .size(px(ICON))
        .flex_none()
        .child(render_runtime_mark(runtime, ICON, s, cx))
        .tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx))
}


#[cfg(test)]
mod tests {
    use super::TitleOverflow;
    use gpui::{Bounds, point, px, size};

    #[test]
    fn folders_with_the_same_name_show_only_the_distinct_parent_suffix() {
        use super::{SidebarFolder, folder_disambiguators};
        use std::path::Path;

        let dirs = [
            "/clients/a/src/app",
            "/clients/b/src/app",
            "/docs/app",
            "/docs/notes",
        ];
        let folders = dirs.map(|dir| {
            let path = Path::new(dir);
            SidebarFolder::new(
                dir.into(),
                path,
                path.file_name().unwrap().to_str().unwrap(),
            )
        });
        let labels = folder_disambiguators(&folders);
        assert_eq!(Path::new(&labels[Path::new(dirs[0])]), Path::new("a/src"));
        assert_eq!(Path::new(&labels[Path::new(dirs[1])]), Path::new("b/src"));
        assert_eq!(labels[Path::new(dirs[2])], "docs");
        assert!(!labels.contains_key(Path::new(dirs[3])));
    }

    #[test]
    fn project_scope_limits_search_and_settled_rows_without_hiding_empty_folders() {
        use super::{ChatListEntry, SidebarFolder, filter_sidebar_folders, folder_disambiguators};
        use std::{collections::BTreeSet, path::Path};
        use uuid::Uuid;

        let entry = |title: &str, workspace: &str| {
            let info = serde_json::from_value(serde_json::json!({
                "id": Uuid::new_v4(), "runtime": "codex", "protocol": "codex", "native_id": "session",
                "workspace": workspace, "created_ms": 1, "program": "/test/codex", "approvals": "host"
            })).unwrap();
            ChatListEntry::from_info_and_turns_with_title(info, &[], Some(title))
        };
        let a = "/clients/a/app";
        let b = "/clients/b/app";
        let empty = "/clients/empty";
        let active = entry("active", a);
        let old_a = entry("old A", a);
        let old_b = entry("old B", b);
        let settled = BTreeSet::from([old_a.info.id, old_b.info.id]);

        let visible = |scope: Option<&str>, query: &str| {
            let mut folders = [
                (a, vec![&active, &old_a]),
                (b, vec![&old_b]),
                (empty, vec![]),
            ].into_iter().map(|(root, entries)| {
                let path = Path::new(root);
                let mut folder = SidebarFolder::new(root.into(), path, path.file_name().unwrap().to_str().unwrap());
                folder.entries = entries;
                folder
            }).collect::<Vec<_>>();
            let parents = folder_disambiguators(&folders);
            let shelf = filter_sidebar_folders(&mut folders, scope.map(Path::new), query, &parents, &settled, None);
            let folders = folders.into_iter().map(|f| {
                (f.root, f.entries.into_iter().map(|e| e.info.id).collect::<Vec<_>>())
            }).collect::<Vec<_>>();
            (folders, shelf.into_iter().map(|e| e.info.id).collect::<Vec<_>>())
        };
        let (all, shelf) = visible(None, "");
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].1, [active.info.id]);
        assert!(all[1].1.is_empty() && all[2].1.is_empty());
        assert_eq!(shelf.len(), 2);
        let (single, shelf) = visible(Some(a), "");
        assert_eq!(single, [(a.into(), vec![active.info.id])]);
        assert_eq!(shelf, [old_a.info.id]);
        assert_eq!(visible(Some(empty), ""), (vec![(empty.into(), vec![])], vec![]));
        assert_eq!(visible(Some(a), "old"), (vec![(a.into(), vec![old_a.info.id])], vec![]));
        assert_eq!(visible(Some(b), "active"), (vec![], vec![]));
        assert_eq!(visible(Some(a), "app"), (vec![], vec![]));
        assert_eq!(visible(None, "app").0.len(), 2);
    }

    #[test]
    fn overflowing_thread_titles_preserve_the_logical_start_in_both_directions() {
        let bounds = Bounds::new(point(px(40.), px(12.)), size(px(120.), px(20.)));
        let layout = |title: &str, width| TitleOverflow::layout(title, px(width), bounds, px(20.), |x| x);

        // With the old left-anchored clip, these two titles showed the same suffix.
        for title in ["שלום פרטי השיחה סיום", "בדיקה פרטי השיחה סיום"] {
            let clip = layout(title, 270.).unwrap();
            assert_eq!(clip.origin.x + px(270.), bounds.right());
            assert_eq!(clip.solid, px(60.)..bounds.right());
            assert_eq!(clip.edge, bounds.left()..px(60.));
            assert_eq!((clip.opacity(0.), clip.opacity(1.)), (0., 1.));
        }

        let mixed = layout("Review בדיקה", 270.).unwrap();
        assert_eq!(mixed.origin.x, bounds.left());
        assert_eq!(mixed.solid, bounds.left()..px(140.));
        assert_eq!(mixed.edge, px(140.)..bounds.right());
        assert_eq!((mixed.opacity(0.), mixed.opacity(1.)), (1., 0.));
        assert!(layout("שלום", 120.).is_none());
    }
}
