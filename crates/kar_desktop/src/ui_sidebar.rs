//! Calm, single-glance Kense navigation rail powered by tailored KUI micro-components:
//! `SearchField` (30px live filter), spring-gliding `pro_pill_indicator`, compact `DropdownMenu`,
//! and borderless 28px `ThemeSwitch`.

use gpui::{prelude::*, *};
use kar::chat::TurnStatus;
use kui::{
    components::pro_chrome::{
        pro_icon_btn, pro_mark_selected, pro_pill_indicator, pro_selection_sensor,
    },
    primitives::{
        control::activate,
        pro_icons::{ProIcon, pro_icon},
    },
};

use crate::{
    coedit::ScenarioKey,
    kar_bridge::KarCommand,
    types::ChatAccessMode,
    ui_primitives::{
        HEADER_H, MACOS_TRAFFIC_LIGHT_PAD, StudioPalette, clean_display_doc_name, render_demo_pill,
        render_kar_brand_lockup, render_office_chip, render_runtime_mark,
    },
    workbench::{KarWorkbench, WorkspaceLayoutMode, runtime_display_name},
};

impl KarWorkbench {
    pub(crate) fn render_sidebar(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let s = StudioPalette::get(cx);
        let active_chat_id = self.current_chat_id();
        let query = self.sidebar_filter.trim().to_lowercase();

        let selected_prov = self
            .providers
            .iter()
            .find(|pr| pr.id == self.selected_runtime)
            .cloned();

        // Shared spring-gliding selection pill across sidebar items
        let nav_sensor = pro_selection_sensor();
        let active_nav_key: SharedString = if self.coedit_panel_open {
            "nav-coedit-active".into()
        } else {
            match &self.access_mode {
                ChatAccessMode::DemoShowcase => "nav-demo-active".into(),
                ChatAccessMode::HttpReadOnly { .. } => "nav-http-active".into(),
                ChatAccessMode::NoneSelected => "nav-new-chat".into(),
                _ => active_chat_id
                    .map(|id| format!("chat-{id}").into())
                    .unwrap_or_else(|| "nav-new-chat".into()),
            }
        };

        // 1. Top 44px Window Chrome Bar (clears macOS traffic lights + draggable)
        let top_bar = div()
            .h(px(HEADER_H))
            .pl(px(if cfg!(target_os = "macos") {
                MACOS_TRAFFIC_LIGHT_PAD
            } else {
                14.
            }))
            .pr(px(8.))
            .window_control_area(WindowControlArea::Drag)
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .child(render_kar_brand_lockup(&s, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(pro_icon_btn(
                        "side-hdr-new-chat",
                        "新建对话 (⌘N)",
                        ProIcon::NewChat,
                        28.,
                        false,
                        cx,
                        |this, _, cx| {
                            this.coedit_panel_open = false;
                            this.create_new_real_chat(cx);
                        },
                    ))
                    .child(pro_icon_btn(
                        "side-collapse-btn",
                        "收起侧边栏 (⌘.)",
                        ProIcon::SidebarLeft,
                        28.,
                        false,
                        cx,
                        |this, _, cx| {
                            this.sidebar_open = false;
                            cx.notify();
                        },
                    )),
            );

        // 2. Compact 30px KUI SearchField Bar (live filter + ⌘K hint)
        let search_row = div()
            .px(px(10.))
            .pt(px(2.))
            .pb(px(8.))
            .flex_none()
            .child(self.sidebar_search.clone());

        // Contextual active special-mode pill row (shown ONLY when in Demo / HTTP / Co-Edit mode!)
        let active_special_row = if self.coedit_panel_open {
            Some(render_special_active_row(
                "nav-special-coedit",
                "人机双写演练",
                ProIcon::GitBranch,
                Some("演示"),
                &nav_sensor,
                &s,
            ))
        } else if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            Some(render_special_active_row(
                "nav-special-demo",
                "演示对话沙盒",
                ProIcon::Sparkles,
                Some("演示"),
                &nav_sensor,
                &s,
            ))
        } else if self.access_mode.is_http_read_only() {
            Some(render_special_active_row(
                "nav-special-http",
                "HTTP 只读订阅",
                ProIcon::Globe,
                None,
                &nav_sensor,
                &s,
            ))
        } else {
            None
        };

        // 3. Conversations Section (`对话`) — Clean Single-Line 30px Rows!
        let filtered_chats: Vec<_> = self
            .chat_list
            .iter()
            .filter(|e| {
                query.is_empty()
                    || e.title.to_lowercase().contains(&query)
                    || e.info.runtime.to_lowercase().contains(&query)
            })
            .cloned()
            .collect();

        let mut chats_group = div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(section_heading("对话", None, None, &s));

        if let Some(special) = active_special_row {
            chats_group = chats_group.child(special);
        }

        if self.chat_list.is_empty() {
            let is_new_sel = matches!(self.access_mode, ChatAccessMode::NoneSelected)
                && !self.coedit_panel_open;
            let hover_row = s.stage_hover;
            chats_group = chats_group.child(activate(
                div()
                    .id("side-empty-new-chat")
                    .role(Role::Button)
                    .tab_index(0)
                    .relative()
                    .h(px(30.))
                    .px(px(8.))
                    .rounded(px(7.5))
                    .when(is_new_sel, |d| d.child(pro_mark_selected(&nav_sensor)))
                    .when(!is_new_sel, |d| d.hover(move |st| st.bg(hover_row)))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .child(pro_icon(
                        ProIcon::Thread,
                        14.,
                        if is_new_sel { s.ink } else { s.muted },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.5))
                            .font_weight(if is_new_sel {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if is_new_sel { s.ink } else { s.text })
                            .child("新对话"),
                    ),
                cx,
                |this, _, cx| {
                    this.coedit_panel_open = false;
                    this.access_mode = ChatAccessMode::NoneSelected;
                    cx.notify();
                },
            ));
        } else if filtered_chats.is_empty() {
            chats_group = chats_group.child(
                div()
                    .h(px(28.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .text_color(s.faint)
                    .child("无匹配对话"),
            );
        } else {
            for entry in filtered_chats {
                let chat_id = entry.info.id;
                let selected = active_chat_id == Some(chat_id)
                    && !matches!(self.access_mode, ChatAccessMode::DemoShowcase)
                    && !self.access_mode.is_http_read_only()
                    && !self.coedit_panel_open;
                let is_running = entry.last_status == Some(TurnStatus::Running);
                let has_issue = matches!(
                    entry.last_status,
                    Some(TurnStatus::Failed | TurnStatus::Interrupted)
                );
                let hover_row = s.stage_hover;
                let hover_eye = s.stage_active;

                chats_group = chats_group.child(
                    div()
                        .relative()
                        .h(px(30.))
                        .pl(px(8.))
                        .pr(px(5.))
                        .rounded(px(7.5))
                        .group("chat-row")
                        .when(selected, |d| d.child(pro_mark_selected(&nav_sensor)))
                        .when(!selected, |d| d.hover(move |st| st.bg(hover_row)))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(activate(
                            div()
                                .id(SharedString::from(format!("chat-open-{chat_id}")))
                                .role(Role::Button)
                                .tab_index(0)
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .flex()
                                .items_center()
                                .gap(px(7.5))
                                .cursor_pointer()
                                .child(render_runtime_mark(&entry.info.runtime, 13.5, &s, cx))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(12.5))
                                        .font_weight(if selected {
                                            FontWeight::MEDIUM
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(if selected { s.ink } else { s.text })
                                        .child(entry.title),
                                )
                                .when(is_running, |d| {
                                    d.child(
                                        div()
                                            .size(px(6.))
                                            .rounded_full()
                                            .bg(s.pro.accent_info),
                                    )
                                })
                                .when(has_issue && !is_running, |d| {
                                    d.child(div().size(px(5.5)).rounded_full().bg(s.pro.amber))
                                }),
                            cx,
                            move |this, _, cx| {
                                this.coedit_panel_open = false;
                                this.bridge.send(KarCommand::SelectLocalChat {
                                    chat_id,
                                    prefer_read_only: false,
                                });
                                cx.notify();
                            },
                        ))
                        // Hover-revealed read-only subscriber trigger (keeps resting sidebar 100% clean!)
                        .child(
                            div()
                                .opacity(0.)
                                .group_hover("chat-row", |st| st.opacity(1.))
                                .child(activate(
                                    div()
                                        .id(SharedString::from(format!("chat-ro-{chat_id}")))
                                        .role(Role::Button)
                                        .aria_label("以只读订阅打开")
                                        .tab_index(0)
                                        .size(px(22.))
                                        .rounded(px(5.))
                                        .hover(move |st| st.bg(hover_eye))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .child(pro_icon(ProIcon::Eye, 12.5, s.muted)),
                                    cx,
                                    move |this, _, cx| {
                                        this.coedit_panel_open = false;
                                        this.bridge.send(KarCommand::SelectLocalChat {
                                            chat_id,
                                            prefer_read_only: true,
                                        });
                                        cx.notify();
                                    },
                                )),
                        ),
                );
            }
        }

        // 4. Kantor Documents Section (`文档` + compact 28x28 `+` DropdownMenu)
        let docs_plus = self.sidebar_docs_menu.clone().into_any_element();
        let mut docs_group = div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(section_heading(
                "文档",
                (!self.office_tabs.is_empty()).then_some(self.office_tabs.len()),
                Some(docs_plus),
                &s,
            ));

        let matching_tabs: Vec<(usize, _)> = self
            .office_tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                if query.is_empty() {
                    return true;
                }
                clean_display_doc_name(&t.doc_name(cx))
                    .to_lowercase()
                    .contains(&query)
            })
            .map(|(i, t)| (i, t.clone()))
            .collect();

        if self.office_tabs.is_empty() {
            let hover_open = s.stage_hover;
            docs_group = docs_group.child(activate(
                div()
                    .id("side-open-doc-dialog")
                    .role(Role::Button)
                    .tab_index(0)
                    .h(px(30.))
                    .px(px(8.))
                    .rounded(px(7.5))
                    .hover(move |st| st.bg(hover_open))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .child(pro_icon(ProIcon::Folder, 13.5, s.faint))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(s.muted)
                            .child("打开或新建文档…"),
                    ),
                cx,
                |this, _, cx| {
                    this.set_layout_mode(WorkspaceLayoutMode::Split, cx);
                },
            ));
        } else {
            for (idx, tab) in matching_tabs {
                let is_active = self.active_office_tab == Some(idx)
                    && self.layout_mode != WorkspaceLayoutMode::ChatFocus;
                let clean_name = clean_display_doc_name(&tab.doc_name(cx));
                let dirty = tab.is_dirty(cx);
                let is_demo_doc = tab.is_demo;
                let kind = tab.kind;
                let tab_id = tab.id;
                let hover_doc = s.stage_hover;

                docs_group = docs_group.child(activate(
                    div()
                        .id(SharedString::from(format!("side-doc-{tab_id}")))
                        .role(Role::Button)
                        .tab_index(0)
                        .h(px(30.))
                        .px(px(8.))
                        .rounded(px(7.5))
                        .bg(if is_active {
                            s.stage_active
                        } else {
                            transparent_black()
                        })
                        .hover(move |st| if is_active { st } else { st.bg(hover_doc) })
                        .flex()
                        .items_center()
                        .gap(px(7.5))
                        .cursor_pointer()
                        .child(render_office_chip(kind, true, &s))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.5))
                                .font_weight(if is_active {
                                    FontWeight::MEDIUM
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_color(if is_active { s.ink } else { s.text })
                                .truncate()
                                .child(clean_name),
                        )
                        .when(dirty, |d| {
                            d.child(div().size(px(5.5)).rounded_full().bg(s.pro.amber))
                        })
                        .when(is_demo_doc, |d| d.child(render_demo_pill("演示", &s))),
                    cx,
                    move |this, _, cx| {
                        if this.layout_mode == WorkspaceLayoutMode::ChatFocus {
                            this.set_layout_mode(WorkspaceLayoutMode::Split, cx);
                        }
                        this.active_office_tab = Some(idx);
                        if let Some(sc_str) =
                            this.office_tabs.get(idx).and_then(|t| t.demo_scenario_key)
                        {
                            this.coedit_panel_open = true;
                            let sc = match sc_str {
                                "excel_same_cell" => ScenarioKey::ExcelSameCellConflict,
                                "ppt_move_text" => ScenarioKey::PptMoveAndText,
                                _ => ScenarioKey::WordSameParagraph,
                            };
                            this.active_coedit_scenario = sc;
                            this.coedit_segmented
                                .update(cx, |seg, cx| seg.sync_value(sc.as_str(), cx));
                        }
                        cx.notify();
                    },
                ));
            }
        }

        // 5. Calm 44px Bottom Bar: `[Provider Mark · Codex •]` on left, `[Tools ⚙]` + `[Theme ☼/☾]` on right
        let prov_ok = selected_prov.as_ref().map(|pr| pr.is_ready()).unwrap_or(true);
        let prov_dot = if prov_ok { s.pro.green } else { s.pro.amber };
        let footer_hover = s.stage_hover;

        let bottom_bar = div()
            .h(px(44.))
            .px(px(10.))
            .border_t_1()
            .border_color(s.border_soft)
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(6.))
            .child(activate(
                div()
                    .id("side-footer-provider-btn")
                    .role(Role::Button)
                    .tab_index(0)
                    .h(px(28.))
                    .px(px(7.))
                    .rounded(px(7.))
                    .hover(move |st| st.bg(footer_hover))
                    .flex()
                    .items_center()
                    .gap(px(6.5))
                    .cursor_pointer()
                    .child(render_runtime_mark(&self.selected_runtime, 14., &s, cx))
                    .child(
                        div()
                            .max_w(px(96.))
                            .truncate()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child(runtime_display_name(&self.selected_runtime)),
                    )
                    .child(div().size(px(5.5)).rounded_full().bg(prov_dot)),
                cx,
                |this, _, cx| this.toggle_provider_sheet(cx),
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(self.sidebar_tools_menu.clone())
                    .child(self.theme_switch.clone()),
            );

        div()
            .size_full()
            .bg(s.stage)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(top_bar)
            .child(search_row)
            .child(
                div()
                    .id("kar-sidebar-scroll")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(10.))
                    .py(px(2.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(pro_pill_indicator(
                        "kar-sidebar-nav-pill",
                        active_nav_key,
                        nav_sensor,
                        7.5,
                        s.stage_active,
                        None,
                        false,
                    ))
                    .child(chats_group)
                    .child(docs_group),
            )
            .child(bottom_bar)
            .into_any_element()
    }
}

fn section_heading(
    title: &'static str,
    count: Option<usize>,
    trailing: Option<AnyElement>,
    s: &StudioPalette,
) -> Div {
    div()
        .h(px(26.))
        .pl(px(8.))
        .pr(px(2.))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.muted)
                        .child(title),
                )
                .when_some(count, |d, n| {
                    d.child(
                        div()
                            .text_size(px(10.5))
                            .text_color(s.faint)
                            .child(format!("{n}")),
                    )
                }),
        )
        .when_some(trailing, |d, el| d.child(el))
}

fn render_special_active_row(
    id: &'static str,
    label: &'static str,
    icon: ProIcon,
    demo_tag: Option<&'static str>,
    sensor: &kui::components::pro_chrome::ProSelectionSensor,
    s: &StudioPalette,
) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .h(px(30.))
        .px(px(8.))
        .rounded(px(7.5))
        .child(pro_mark_selected(sensor))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(6.))
        .child(
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(7.5))
                .child(pro_icon(icon, 13.5, s.ink))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.ink)
                        .child(label),
                ),
        )
        .when_some(demo_tag, |d, t| d.child(render_demo_pill(t, s)))
}
