//! Native workspace chrome and thread navigation.

use std::time::Duration;

use gpui::{prelude::*, *};
use kar::chat::TurnStatus;
use kui::{
    components::pro_chrome::{
        pro_fold, pro_mark_selected, pro_pill_indicator, pro_progress_ring, pro_selection_sensor,
    },
    primitives::pro_icons::{ProIcon, pro_app_artwork, pro_icon},
};

use crate::{
    remote_link::now_epoch_ms,
    types::{
        ChatAccessMode, ChatListEntry, format_relative_age, runtime_display_name,
    },
    ui_motion::{
        MenuPlacement, MenuSize, Tactile, icon_button, menu_caption, menu_check, menu_divider, menu_hint,
        menu_host, menu_icon_item, menu_item, soft_button,
    },
    ui_primitives::{
        FS_META, FS_TAG, FS_UI, ICON, ICON_SM, R_CTRL, R_PILL, StudioPalette,
        clean_display_doc_name, macos_traffic_pad, render_approval_hand,
        render_circle_check_icon, render_folder_git_icon,
        render_folder_icon,
        sidebar_texture, render_office_chip, render_project_monogram,
        render_radar_link_icon, render_runtime_mark, render_sample_tag,
        render_schedule_repeat_icon, render_square_pen_icon,
        tnum_features,
    },
    ui_settings::SettingsTab,
    workbench::{KarWorkbench, MainStageView, Menu, RightPaneSurface, WorkspaceLayoutMode},
};

const DELETE_CONFIRM_TTL: Duration = Duration::from_secs(3);

impl KarWorkbench {
    pub(crate) fn workspace_toolbar(
        &self,
        title: &'static str,
        action: Option<AnyElement>,
        s: &StudioPalette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let left = 20. + (macos_traffic_pad(window) - 20.).max(0.) * (1. - self.sidebar_reveal);
        div()
            .w_full()
            .h(px(crate::ui_primitives::HEADER_H))
            .pl(px(left))
            .pr(px(12.))
            .flex_none()
            .window_control_area(WindowControlArea::Drag)
            .flex()
            .items_center()
            .gap(px(8.))
            .when(!self.sidebar_open, |row| {
                row.child(
                    icon_button(
                        "stage-show-sidebar",
                        ProIcon::SidebarLeft,
                        self.ui_locale.pick("展开侧边栏", "Show sidebar"),
                        s,
                    )
                    .on_press(cx, |this, window, cx| {
                        this.focus_handle.focus(window, cx);
                        this.set_sidebar_open(true, cx);
                    }),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(FS_UI))
                    .line_height(px(20.))
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
                                .text_size(px(FS_META))
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

        let nav_sensor = pro_selection_sensor();
        let active_nav_key: SharedString = match self.main_stage_view {
            MainStageView::TasksInbox => "nav-tasks-inbox".into(),
            MainStageView::OfficeGallery => "nav-office-gallery".into(),
            MainStageView::Conversation => {
                if on_start_screen {
                    "nav-new-chat".into()
                } else {
                    match &self.access_mode {
                        ChatAccessMode::DemoShowcase => "nav-sample".into(),
                        ChatAccessMode::HttpReadOnly { .. }
                        | ChatAccessMode::RemoteController { .. } => "nav-shared".into(),
                        ChatAccessMode::NoneSelected => "nav-new-chat".into(),
                        _ => active_chat_id
                            .map(|id| format!("chat-{id}").into())
                            .unwrap_or_else(|| "nav-new-chat".into()),
                    }
                }
            }
        };

        let toolbar_width = width - 8. - 1.; // Right inset and sidebar border.
        let traffic_pad = macos_traffic_pad(window);
        let left_inset = if cfg!(target_os = "macos") && !window.is_fullscreen() {
            (toolbar_width - 4. * 28.).clamp(traffic_pad, 91.)
        } else {
            12.
        };
        let button_width = ((toolbar_width - left_inset) / 4.).clamp(24., 28.);
        let chrome_ink = s.ink;
        let toolbar_button = |id, label, icon, size| {
            Tactile::ghost(id, &s)
                .w(px(button_width))
                .h(px(28.))
                .flex_none()
                .radius(6.)
                .justify_center()
                .fills(transparent_black(), chrome_ink.opacity(0.14))
                .tip(label)
                .child(pro_icon(icon, size, chrome_ink))
        };
        let top_bar = div()
            .relative()
            .h(px(crate::ui_primitives::HEADER_H))
            .pl(px(left_inset))
            .pr(px(8.))
            .window_control_area(WindowControlArea::Drag)
            .flex_none()
            .flex()
            .items_center()
            .child(
                toolbar_button(
                    "side-collapse",
                    loc.pick("收起侧边栏  ⌘.", "Hide sidebar  ⌘."),
                    ProIcon::SidebarLeft,
                    16.,
                )
                .on_press(cx, |this, window, cx| {
                    this.focus_handle.focus(window, cx);
                    this.set_sidebar_open(false, cx);
                }),
            )
            .child(
                Tactile::ghost("side-home", &s)
                    .w(px(button_width))
                    .flex_none()
                    .h(px(28.))
                    .radius(6.)
                    .justify_center()
                    .fills(transparent_black(), chrome_ink.opacity(0.14))
                    .aria(loc.pick("Kwrok 主页", "Kwrok home"))
                    .tip(loc.pick("主页", "Home"))
                    .when_some(pro_app_artwork("kense", 17., cx), |d, mark| d.child(mark))
                    .on_press(cx, |this, win, cx| this.start_new_chat(win, cx)),
            )
            .child(
                toolbar_button(
                    "side-back",
                    loc.pick("后退", "Go back"),
                    ProIcon::ArrowLeft,
                    14.,
                )
                .disabled(!self.can_navigate(false))
                .on_press(cx, |this, win, cx| this.navigate_history(false, win, cx)),
            )
            .child(
                toolbar_button(
                    "side-forward",
                    loc.pick("前进", "Go forward"),
                    ProIcon::ArrowRight,
                    14.,
                )
                .disabled(!self.can_navigate(true))
                .on_press(cx, |this, win, cx| this.navigate_history(true, win, cx)),
            );

        let projects = menu_host(
            Tactile::ghost("side-project-scope", &s)
                .size(px(28.))
                .radius(6.)
                .justify_center()
                .lit(
                    self.open_menu == Some(Menu::SidebarFilter)
                        || self.sidebar_project_filter.is_some(),
                )
                .tip(loc.pick("项目", "Projects"))
                .child(render_folder_icon(16., s.muted))
                .on_press(cx, |this, _, cx| this.toggle_menu(Menu::SidebarFilter, cx)),
            self.render_sidebar_filter_menu(&s, cx),
        );
        let search = div()
            .id("side-search-field")
            .flex_1()
            .min_w_0()
            .h(px(32.))
            .px(px(8.))
            .gap(px(8.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .hover(|st| st.bg(s.sidebar_hover))
            .on_click(cx.listener(|this, _, win, cx| {
                this.sidebar_search.update(cx, |ed, cx| ed.focus(win, cx))
            }))
            .child(pro_icon(ProIcon::Search, 16., s.muted))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(21.))
                    .child(self.sidebar_search.clone()),
            )
            .when(has_query, |d| {
                d.child(
                    Tactile::ghost("side-search-clear", &s)
                        .size(px(20.))
                        .radius(5.)
                        .justify_center()
                        .tip(loc.pick("清空搜索", "Clear search"))
                        .child(pro_icon(ProIcon::Close, 12., s.muted))
                        .on_press(cx, |this, win, cx| {
                            this.sidebar_search.update(cx, |ed, cx| {
                                ed.set_value("", win, cx);
                                ed.focus(win, cx);
                            });
                        }),
                )
            });
        let thread_header = div()
            .w_full()
            .p(px(8.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(search)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(projects)
                    .child(
                        Tactile::ghost("side-new-thread-btn", &s)
                            .size(px(28.))
                            .radius(6.)
                            .justify_center()
                            .tip(loc.pick("新建对话  ⌘N", "New thread  ⌘N"))
                            .child(render_square_pen_icon(16., s.muted))
                            .on_press(cx, |this, win, cx| this.start_new_chat(win, cx)),
                    ),
            );

        // Active project filter pill if user filtered to a single repo
        let active_proj_chip = self.sidebar_project_filter.clone().map(|proj_path| {
            let pname = proj_path
                .file_name()
                .and_then(|n| n.to_str())
                .map_or("Kwork", crate::studio_hub::project_display_name)
                .to_owned();
            div().px(px(10.)).pb(px(6.)).flex_none().child(
                div()
                    .h(px(24.))
                    .pl(px(7.))
                    .pr(px(4.))
                    .rounded(px(R_PILL))
                    .bg(s.soft)
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(6.))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .child(render_project_monogram(&pname, 14., &s, cx))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(FS_TAG))
                                    .text_color(s.ink)
                                    .child(pname),
                            ),
                    )
                    .child(
                        Tactile::ghost("side-clear-proj-filter", &s)
                            .size(px(18.))
                            .radius(9.)
                            .justify_center()
                            .child(pro_icon(ProIcon::Close, 10., s.muted))
                            .on_press(cx, |this, _, cx| {
                                this.sidebar_project_filter = None;
                                cx.notify();
                            }),
                    ),
            )
        });

        let undo_banner = self.sidebar_open.then(|| self.render_delete_undo(&s, cx)).flatten();

        // ── 4. Filter Active & Settled Threads ──────────────────────────────
        let deferred_del_id = self.deferred_delete_chat.as_ref().map(|(id, _, _)| *id);
        let proj_filter_norm = self
            .sidebar_project_filter
            .as_ref()
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()));

        let matches_filters = |e: &ChatListEntry| -> bool {
            if deferred_del_id == Some(e.info.id) {
                return false;
            }
            if let Some(ref pf) = proj_filter_norm {
                let ws_norm = e
                    .info
                    .workspace
                    .canonicalize()
                    .unwrap_or_else(|_| e.info.workspace.clone());
                if &ws_norm != pf {
                    return false;
                }
            }
            if query.is_empty() {
                return true;
            }
            let proj_match = self
                .project_registry
                .project_for_chat(&e.info.workspace, e.info.id)
                .is_some_and(|p| {
                    crate::studio_hub::project_display_name(&p.name)
                        .to_lowercase()
                        .contains(&query)
                })
                || e.info
                    .workspace
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.to_lowercase().contains(&query));
            proj_match
                || e.title.to_lowercase().contains(&query)
                || runtime_display_name(&e.info.runtime)
                    .to_lowercase()
                    .contains(&query)
        };

        let mut active_entries: Vec<ChatListEntry> = Vec::new();
        let mut settled_entries: Vec<ChatListEntry> = Vec::new();
        for entry in &self.chat_list {
            if !matches_filters(entry) {
                continue;
            }
            if self.settled_chats.contains(&entry.info.id) {
                settled_entries.push(entry.clone());
            } else {
                active_entries.push(entry.clone());
            }
        }
        // Pinned threads float smoothly to the top of the active feed
        active_entries.sort_by_key(|e| {
            (
                !self.project_registry.is_pinned(e.info.id),
                std::cmp::Reverse(e.last_activity_ms.unwrap_or(0)),
            )
        });

        let inbox_attention_count = self.projected.pending_requests.len();
        let mut list_col =
            div()
                .relative()
                .flex()
                .flex_col()
                .gap(px(1.))
                .child(pro_pill_indicator(
                    "kar-sidebar-nav-pill",
                    active_nav_key,
                    nav_sensor.clone(),
                    R_CTRL,
                    s.sidebar_selected,
                    Some(s.line_soft),
                    false,
                ));

        // ── Special active Demo / Shared / Remote thread cards ──────────────
        if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            list_col = list_col
                .child(
                    div()
                        .px(px(6.))
                        .pt(px(4.))
                        .pb(px(2.))
                        .text_size(px(FS_TAG))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.faint)
                        .child(loc.pick("最近对话", "Recent chats")),
                )
                .child(self.render_demo_showcase_cards(in_conv.then_some(&nav_sensor), &s, cx));
        } else if let ChatAccessMode::RemoteController { ref host_name, .. } = self.access_mode {
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
                                    .child(render_radar_link_icon(13., s.pro.green))
                                    .child(
                                        div()
                                            .text_size(px(FS_META))
                                            .text_color(s.muted)
                                            .child("remote"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(FS_TAG))
                                    .text_color(s.pro.green_ink)
                                    .child("live"),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .truncate()
                            .child(format!("{} {host_name}", loc.pick("远端主机：", "Remote:"))),
                    ),
            );
        } else if self.access_mode.is_http_read_only() {
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
                    .child(pro_icon(ProIcon::Link, ICON_SM, s.text))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child(loc.pick("共享的实时对话", "Shared live thread")),
                    ),
            );
        }

        for entry in &active_entries {
            list_col = list_col.child(self.render_sidebar_thread_card(
                entry.clone(),
                "act",
                active_chat_id,
                now_ms,
                false,
                &nav_sensor,
                &s,
                cx,
            ));
        }

        if active_entries.is_empty() && !matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            let message = if has_query {
                loc.pick("没有匹配的对话", "No matching threads")
            } else if !self.chat_list.is_empty() && self.sidebar_project_filter.is_some() {
                loc.pick("此项目还没有活跃对话", "No active threads in this project")
            } else if !settled_entries.is_empty() {
                loc.pick("没有活跃对话", "No active threads")
            } else {
                loc.pick("还没有对话", "No threads yet")
            };
            list_col = list_col.child(
                div()
                    .w_full()
                    .pt(px(24.))
                    .pb(px(24.))
                    .text_center()
                    .text_size(px(FS_META))
                    .line_height(px(16.))
                    .text_color(s.muted.opacity(0.68))
                    .child(message),
            );
        }

        // ── Open Kantor Office Documents shelf (shown when ≥ 1 doc is open) ─
        let doc_matches = |tab: &crate::office_host::OfficeTab| {
            clean_display_doc_name(&tab.doc_name(cx))
                .to_lowercase()
                .contains(&query)
        };
        let has_docs = self.office_tabs.iter().any(doc_matches);
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
                        .text_size(px(FS_TAG))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.faint)
                        .child(loc.pick("打开的文档", "OPEN DOCS")),
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
                docs_col = docs_col.child(
                    div()
                        .relative()
                        .w_full()
                        .h(px(34.))
                        .group(grp.clone())
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
                                .pr(px(24.))
                                .gap(px(7.))
                                .child(render_office_chip(tab.kind, &s))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(FS_META))
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
                                .on_press(cx, move |this, _, cx| this.activate_office_tab(idx, cx)),
                        )
                        .child(
                            div()
                                .absolute()
                                .right(px(4.))
                                .opacity(0.)
                                .group_hover(grp, |st| st.opacity(1.))
                                .child(
                                    Tactile::ghost(
                                        SharedString::from(format!("side-doc-close-{tab_id}")),
                                        &s,
                                    )
                                    .size(px(19.))
                                    .radius(4.)
                                    .justify_center()
                                    .child(pro_icon(ProIcon::Close, 10.5, s.muted))
                                    .on_press(cx, move |this, win, cx| {
                                        this.close_office_tab(tab_id, win, cx)
                                    }),
                                ),
                        ),
                );
            }
            list_col = list_col.child(docs_col);
        }

        // ── 5. Bottom `Settled  N >` Shelf (ONLY rendered when settled_count > 0!) ─
        let settled_count = settled_entries.len();
        let settled_shelf = (settled_count > 0 || !active_entries.is_empty()).then(|| {
            let settled_open = self.settled_shelf_open && settled_count > 0;
            let shelf_height = (settled_count as f32 * 80. + 4.)
                .min(f32::from(window.viewport_size().height) * 0.4);
            let mut settled_body = div().px(px(8.)).pb(px(6.)).flex().flex_col().gap(px(2.));
            for entry in settled_entries {
                settled_body = settled_body.child(self.render_sidebar_thread_card(
                    entry,
                    "set",
                    active_chat_id,
                    now_ms,
                    true,
                    &nav_sensor,
                    &s,
                    cx,
                ));
            }
            div()
                .flex_none()
                .px(px(6.))
                .pt(px(2.))
                .pb(px(4.))
                .child(
                    Tactile::ghost("side-settled-toggle", &s)
                        .w_full()
                        .h(px(34.))
                        .px(px(8.))
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(FS_META))
                                .text_color(s.muted)
                                .child(loc.pick("已结项", "Settled")),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .font_features(tnum_features())
                                .text_size(px(FS_META))
                                .text_color(s.muted)
                                .child(format!("{settled_count}"))
                                .child(pro_icon(
                                    if settled_open {
                                        ProIcon::ChevronDown
                                    } else {
                                        ProIcon::ChevronRight
                                    },
                                    11.,
                                    s.faint,
                                )),
                        )
                        .when(settled_count == 0, |btn| btn.inert())
                        .on_press(cx, |this, _, cx| {
                            this.settled_shelf_open = !this.settled_shelf_open;
                            cx.notify();
                        }),
                )
                .child(pro_fold(
                    "kar-settled-fold",
                    settled_open,
                    shelf_height,
                    0.,
                    move || {
                        div()
                            .id("settled-threads-scroll")
                            .h(px(shelf_height))
                            .overflow_y_scroll()
                            .child(settled_body)
                            .into_any_element()
                    },
                ))
        });

        let more_open = self.open_menu == Some(Menu::SidebarMore);
        let footer_button = |id, icon, label| {
            Tactile::ghost(id, &s)
                .size(px(32.))
                .radius(6.)
                .justify_center()
                .aria(label)
                .when(!more_open, |button| button.tip(label))
                .child(pro_icon(icon, 16., s.muted))
        };
        let more_btn = menu_host(
            footer_button(
                "side-more",
                ProIcon::MoreHorizontal,
                loc.pick("更多操作", "More actions"),
            )
            .lit(more_open)
            .on_press(cx, |this, _, cx| this.toggle_menu(Menu::SidebarMore, cx)),
            self.render_sidebar_more_menu(&s, window, cx),
        );
        let footer = div()
            .w_full()
            .flex_none()
            .px(px(8.))
            .pt(px(4.))
            .pb(px(8.))
            .flex()
            .items_center()
            .gap(px(if width < 204. { 3. } else { 4. }))
            .when(in_conv, |row| {
                row.child(
                    footer_button(
                        "side-footer-settings",
                        ProIcon::Settings,
                        loc.pick("设置  ⌘,", "Settings  ⌘,"),
                    )
                    .lit(self.settings_open)
                    .on_press(cx, |this, _, cx| {
                        this.toggle_settings(SettingsTab::General, cx)
                    }),
                )
                .child(
                    footer_button(
                        "side-nav-git-changes",
                        ProIcon::GitBranch,
                        loc.pick("Git 变更", "Git changes"),
                    )
                    .lit(
                        in_conv
                            && docs_shown
                            && self.right_surface == RightPaneSurface::GitWorkbench,
                    )
                    .on_press(cx, |this, _, cx| this.open_git_workbench_tab(cx)),
                )
                .child(
                    footer_button(
                        "side-nav-office-docs",
                        ProIcon::PageDoc,
                        loc.pick("文档", "Documents"),
                    )
                    .lit(self.main_stage_view == MainStageView::OfficeGallery)
                    .on_press(cx, |this, _, cx| this.open_office_gallery_stage(cx)),
                )
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .child(
                            footer_button(
                                "side-nav-tasks-inbox",
                                ProIcon::Inbox,
                                loc.pick("收件箱", "Inbox"),
                            )
                            .lit(self.main_stage_view == MainStageView::TasksInbox)
                            .on_press(cx, |this, _, cx| this.open_tasks_inbox_stage(cx)),
                        )
                        .when(inbox_attention_count > 0, |row| {
                            row.child(
                                div()
                                    .absolute()
                                    .right(px(5.))
                                    .top(px(5.))
                                    .size(px(5.))
                                    .rounded_full()
                                    .bg(s.pro.amber),
                            )
                        }),
                )
                .child(div().flex_1())
            })
            .when(!in_conv, |row| {
                row.child(
                    Tactile::ghost("side-return", &s)
                        .h(px(32.))
                        .flex_1()
                        .px(px(8.))
                        .gap(px(8.))
                        .tip(loc.pick("返回上个界面", "Go back"))
                        .child(pro_icon(ProIcon::ArrowLeft, 16., s.muted))
                        .child(
                            div()
                                .text_size(px(FS_UI))
                                .text_color(s.muted)
                                .child(loc.pick("返回", "Back")),
                        )
                        .on_press(cx, |this, win, cx| {
                            if this.can_navigate(false) {
                                this.navigate_history(false, win, cx);
                            } else {
                                this.start_new_chat(win, cx);
                            }
                        }),
                )
            })
            .child(more_btn);

        let body_scroll = div()
            .id("kar-sidebar-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(8.))
            .pb(px(8.))
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
            .child(thread_header)
            .when_some(active_proj_chip, |d, chip| d.child(chip))
            .when_some(undo_banner, |d, b| d.child(b))
            .child(body_scroll)
            .when_some(settled_shelf, |d, sh| d.child(sh))
            .child(footer)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_sidebar_thread_card(
        &self,
        entry: ChatListEntry,
        prefix: &'static str,
        active_chat_id: Option<uuid::Uuid>,
        now_ms: u64,
        is_settled: bool,
        nav_sensor: &kui::components::pro_chrome::ProSelectionSensor,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let chat_id = entry.info.id;
        let selected = active_chat_id == Some(chat_id)
            && matches!(
                self.access_mode,
                ChatAccessMode::WriteController { .. }
                    | ChatAccessMode::LocalReadOnlySubscriber { .. }
                    | ChatAccessMode::ArchivedReadOnly { .. }
            );
        let running = entry.last_status == Some(TurnStatus::Running);
        let needs_approval = selected && !self.projected.pending_requests.is_empty();
        let pinned = self.project_registry.is_pinned(chat_id);
        let confirming_del = self
            .pending_delete_chat
            .is_some_and(|(id, at)| id == chat_id && at.elapsed() < DELETE_CONFIRM_TTL);
        let has_assoc = self.client_reducer.association_for(&chat_id).is_some();

        let proj_name = self
            .project_registry
            .project_for_chat(&entry.info.workspace, chat_id)
            .map(|p| p.name.as_str())
            .or_else(|| entry.info.workspace.file_name()?.to_str())
            .map_or("Kwork", crate::studio_hub::project_display_name)
            .to_owned();

        let display_title = if entry.turn_count == 0 && entry.title == "新对话" {
            loc.pick("新对话", "New thread").to_owned()
        } else {
            entry.title.clone()
        };
        let age_txt = format_relative_age(entry.last_activity_ms, now_ms);
        let title_for_rename = entry.title.clone();
        let title_for_del = display_title.clone();
        let grp = SharedString::from(format!("side-card-grp-{prefix}-{chat_id}"));

        // Status / Age badge on top-right (fades out on hover to reveal quick actions)
        let status_right: AnyElement = if needs_approval {
            div()
                .flex()
                .items_center()
                .gap(px(3.5))
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(s.pro.amber_ink)
                .child(render_approval_hand(12., s.pro.amber_ink))
                .child(loc.pick("待确认", "Approval"))
                .into_any_element()
        } else if running {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(s.pro.accent_info)
                .child(status_dot(s.pro.accent_info))
                .child(loc.pick("运行中", "Working"))
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .font_features(tnum_features())
                .text_size(px(11.5))
                .text_color(s.muted)
                .when(pinned, |d| d.child(pro_icon(ProIcon::Pin, 10., s.muted)))
                .when(has_assoc, |d| {
                    d.child(render_schedule_repeat_icon(10.5, s.pro.accent_info))
                })
                .child(age_txt)
                .into_any_element()
        };

        let in_conv = self.main_stage_view == MainStageView::Conversation;
        div()
            .relative()
            .w_full()
            .group(grp.clone())
            .rounded(px(R_CTRL))
            .when(in_conv && selected && !is_settled, |d| {
                d.child(pro_mark_selected(nav_sensor))
            })
            .child(
                Tactile::ghost(
                    SharedString::from(format!("chat-card-{prefix}-{chat_id}")),
                    s,
                )
                .lit(in_conv && selected && is_settled)
                .lit_fill(s.sidebar_selected)
                .fills(transparent_black(), s.sidebar_hover)
                .w_full()
                .h(px(78.))
                .px(px(10.))
                .py(px(8.))
                .radius(R_CTRL)
                .flex_col()
                .items_start()
                .child(
                    div()
                        .w_full()
                        .h(px(20.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(render_project_monogram(&proj_name, 16., s, cx))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(FS_META))
                                .text_color(s.muted)
                                .child(proj_name),
                        )
                        .child(
                            div()
                                .flex_none()
                                .when(!running, |d| d.group_hover(grp.clone(), |h| h.opacity(0.)))
                                .child(status_right),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .mt(px(4.))
                        .h(px(20.))
                        .truncate()
                        .text_size(px(FS_UI))
                        .line_height(px(20.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.ink)
                        .child(display_title),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(16.))
                        .mt(px(2.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(if entry.info.worktree_id.is_some() {
                            pro_icon(ProIcon::GitBranch, 14., s.muted)
                        } else {
                            crate::ui_primitives::render_machine_icon(14., s.muted)
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(FS_META))
                                .line_height(px(16.))
                                .text_color(s.muted)
                                .child(if entry.info.worktree_id.is_some() {
                                    loc.pick("工作树", "Worktree")
                                } else {
                                    loc.pick("本机", "This computer")
                                }),
                        )
                        .child(render_runtime_mark(&entry.info.runtime, 14., s, cx)),
                )
                .on_press(cx, move |this, window, cx| {
                    this.pending_delete_chat = None;
                    this.open_chat(chat_id, window, cx);
                }),
            )
            // Hover quick actions on top-right (`✓ Settle`, `📌 Pin`, `✎ Rename`, `× Delete`)
            .when(!running, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(8.))
                        .right(px(6.))
                        .px(px(2.))
                        .rounded(px(5.))
                        .bg(if selected {
                            s.stage.blend(s.sidebar_selected)
                        } else {
                            s.stage
                        })
                        .flex()
                        .items_center()
                        .gap(px(1.))
                        .when(!confirming_del, |st| {
                            st.opacity(0.).group_hover(grp, |h| h.opacity(1.))
                        })
                        .when(!confirming_del, |st| {
                            st.child(
                                Tactile::ghost(
                                    SharedString::from(format!("chat-settle-{prefix}-{chat_id}")),
                                    s,
                                )
                                .size(px(19.))
                                .radius(4.)
                                .justify_center()
                                .tip(if is_settled {
                                    loc.pick("移回活跃列表", "Unsettle")
                                } else {
                                    loc.pick("标记为已结项 (Settle)", "Mark settled")
                                })
                                .child(render_circle_check_icon(
                                    11.5,
                                    if is_settled { s.pro.green } else { s.muted },
                                ))
                                .on_press(
                                    cx,
                                    move |this, _, cx| {
                                        this.toggle_settle_chat(chat_id, cx);
                                    },
                                ),
                            )
                            .child(
                                Tactile::ghost(
                                    SharedString::from(format!("chat-pin-{prefix}-{chat_id}")),
                                    s,
                                )
                                .size(px(19.))
                                .radius(4.)
                                .justify_center()
                                .tip(if pinned {
                                    loc.pick("取消置顶", "Unpin")
                                } else {
                                    loc.pick("置顶对话", "Pin")
                                })
                                .child(pro_icon(
                                    ProIcon::Pin,
                                    10.5,
                                    if pinned { s.ink } else { s.muted },
                                ))
                                .on_press(
                                    cx,
                                    move |this, _, cx| {
                                        this.toggle_pin_chat(chat_id, cx);
                                    },
                                ),
                            )
                            .child(
                                Tactile::ghost(
                                    SharedString::from(format!("chat-ren-{prefix}-{chat_id}")),
                                    s,
                                )
                                .size(px(19.))
                                .radius(4.)
                                .justify_center()
                                .tip(loc.pick("重命名对话", "Rename"))
                                .child(pro_icon(ProIcon::Note, 10.5, s.muted))
                                .on_press(
                                    cx,
                                    move |this, window, cx| {
                                        this.open_rename_chat_dialog(
                                            chat_id,
                                            &title_for_rename,
                                            window,
                                            cx,
                                        );
                                    },
                                ),
                            )
                        })
                        .child(if confirming_del {
                            Tactile::soft(
                                SharedString::from(format!("chat-del-confirm-{prefix}-{chat_id}")),
                                s,
                            )
                            .fills(s.pro.red_soft, s.pro.red.opacity(0.18))
                            .h(px(20.))
                            .px(px(6.))
                            .radius(5.)
                            .text_size(px(FS_TAG))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.pro.red_ink)
                            .child(loc.pick("删除", "Delete"))
                            .on_press(cx, move |this, _, cx| {
                                this.stage_deferred_chat_delete(chat_id, title_for_del.clone(), cx);
                            })
                            .into_any_element()
                        } else {
                            Tactile::ghost(
                                SharedString::from(format!("chat-del-{prefix}-{chat_id}")),
                                s,
                            )
                            .size(px(19.))
                            .radius(4.)
                            .justify_center()
                            .tip(loc.pick("删除对话", "Delete"))
                            .child(pro_icon(ProIcon::Close, 10.5, s.muted))
                            .on_press(cx, move |this, _, cx| {
                                this.pending_delete_chat =
                                    Some((chat_id, std::time::Instant::now()));
                                cx.notify();
                            })
                            .into_any_element()
                        }),
                )
            })
    }

    /// Rich 3-line T3 Code showcase cards rendered only in explicit `--demo-chat` mode.
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
                    .text_size(px(11.5))
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
                .text_size(px(11.5))
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
                        .text_size(px(11.5))
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
                        .text_size(px(11.5))
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
                        .text_size(px(11.5))
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

    fn render_sidebar_filter_menu(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let loc = self.ui_locale;
        let cur_filter = self
            .sidebar_project_filter
            .as_ref()
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()));
        let mut rows: Vec<AnyElement> = vec![
            menu_caption(loc.pick("按项目筛选", "Filter by project"), s).into_any_element(),
            menu_item(
                "side-flt-all",
                Some(render_folder_icon(ICON, s.muted)),
                loc.pick("全部项目对话", "All projects"),
                s,
            )
            .when(cur_filter.is_none(), |r| r.child(menu_check(s)))
            .on_press(cx, |this, _, cx| {
                this.sidebar_project_filter = None;
                this.open_menu = None;
                cx.notify();
            })
            .into_any_element(),
            menu_divider(s).into_any_element(),
        ];

        for (ix, proj) in self.project_registry.projects.iter().enumerate() {
            let p_norm = proj
                .root_path
                .canonicalize()
                .unwrap_or_else(|_| proj.root_path.clone());
            let is_sel = cur_filter.as_ref() == Some(&p_norm);
            let root = proj.root_path.clone();
            rows.push(
                menu_item(
                    ("side-flt-proj", ix),
                    Some(render_project_monogram(crate::studio_hub::project_display_name(&proj.name), 16., s, cx).into_any_element()),
                    crate::studio_hub::project_display_name(&proj.name).to_owned(),
                    s,
                )
                .when(is_sel, |r| r.child(menu_check(s)))
                .on_press(cx, move |this, _, cx| {
                    this.sidebar_project_filter = Some(root.clone());
                    this.open_menu = None;
                    cx.notify();
                })
                .into_any_element(),
            );
        }

        rows.push(menu_divider(s).into_any_element());
        rows.push(
            menu_icon_item(
                "side-add-project",
                ProIcon::Plus,
                loc.pick("添加项目…", "Add project…"),
                s,
            )
            .on_press(cx, |this, win, cx| {
                this.open_menu = None;
                this.add_project_folder(win, cx);
            })
            .into_any_element(),
        );
        self.render_menu(
            Menu::SidebarFilter,
            MenuPlacement::BelowEnd,
            220.,
            rows,
            s,
            cx,
        )
    }

    fn render_sidebar_more_menu(
        &self,
        s: &StudioPalette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let mut rows = Vec::new();
        if self.main_stage_view != MainStageView::Conversation {
            rows.extend([
                menu_icon_item(
                    "side-more-inbox",
                    ProIcon::Inbox,
                    loc.pick("收件箱", "Inbox"),
                    s,
                )
                .on_press(cx, |this, _, cx| this.open_tasks_inbox_stage(cx))
                .into_any_element(),
                menu_icon_item(
                    "side-more-docs",
                    ProIcon::PageDoc,
                    loc.pick("文档", "Documents"),
                    s,
                )
                .on_press(cx, |this, _, cx| this.open_office_gallery_stage(cx))
                .into_any_element(),
                menu_icon_item(
                    "side-more-git",
                    ProIcon::GitBranch,
                    loc.pick("Git 变更", "Git changes"),
                    s,
                )
                .on_press(cx, |this, _, cx| this.open_git_workbench_tab(cx))
                .into_any_element(),
                menu_divider(s).into_any_element(),
            ]);
        }
        rows.extend([
            menu_icon_item(
                "side-more-language",
                ProIcon::Globe,
                loc.pick("Switch to English", "切换为中文"),
                s,
            )
            .on_press(cx, |this, win, cx| {
                this.open_menu = None;
                this.toggle_ui_locale(win, cx);
            })
            .into_any_element(),
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
            .on_press(cx, |this, _, cx| {
                this.open_menu = None;
                this.open_sample_chat(cx);
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
        ]);
        self.render_menu(
            Menu::SidebarMore,
            MenuPlacement::AboveEnd,
            MenuSize {
                width: 236.,
                // 40px above the bottom edge for the trigger, 6px gap, 10px top margin.
                max_height: (f32::from(window.viewport_size().height) - 56.).max(36.),
            },
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
    div()
        .id(id)
        .relative()
        .w_full()
        .px(px(10.))
        .py(px(8.))
        .rounded(px(R_CTRL))
        .when(!selected, |d| d.hover(|h| h.bg(s.sidebar_hover)))
        .when_some(sensor, |d, sn| d.child(pro_mark_selected(sn)))
        .flex()
        .flex_col()
        .gap(px(3.5))
        .child(
            div()
                .w_full()
                .h(px(17.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(6.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(render_project_monogram(proj, 15.5, s, cx))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(FS_META))
                                .text_color(s.muted)
                                .child(proj.to_owned()),
                        ),
                )
                .child(right_status),
        )
        .child(
            div()
                .w_full()
                .truncate()
                .text_size(px(13.5))
                .line_height(px(18.5))
                .font_weight(if selected {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if selected { s.ink } else { s.text })
                .child(title.to_owned()),
        )
        .when(branch.is_some(), |d| {
            let br = branch.unwrap_or_default().to_owned();
            d.child(
                div()
                    .w_full()
                    .h(px(16.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(4.5))
                            .child(render_folder_git_icon(12., s.faint))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(s.muted)
                                    .child(br),
                            ),
                    )
                    .child(
                        div()
                            .opacity(if selected { 0.88 } else { 0.62 })
                            .child(render_runtime_mark(runtime, 13.5, s, cx)),
                    ),
            )
        })
        .when(branch.is_none(), |d| {
            d.child(
                div()
                    .w_full()
                    .h(px(16.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(
                        div()
                            .opacity(0.62)
                            .child(render_runtime_mark(runtime, 13.5, s, cx)),
                    ),
            )
        })
}

fn status_dot(color: Hsla) -> Div {
    div().size(px(6.)).flex_none().rounded_full().bg(color)
}
