//! Claude Desktop-style warm stone navigation rail with KUI spring-gliding selection pills,
//! `⌘K` CommandDesk launcher, Office canvas quick-links, and KUI `ThemeSwitch` footer card.

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
    types::{ChatAccessMode, OfficeKind},
    ui_primitives::{
        DotTone, HEADER_H, MACOS_TRAFFIC_LIGHT_PAD, StudioPalette, clean_display_doc_name,
        render_demo_pill, render_kar_brand_lockup, render_keycap, render_office_chip,
        render_runtime_mark,
    },
    workbench::{KarWorkbench, WorkspaceLayoutMode, runtime_display_name, status_short_text},
};

impl KarWorkbench {
    pub(crate) fn render_sidebar(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let s = StudioPalette::get(cx);
        let active_chat_id = self.current_chat_id();

        let selected_prov = self
            .providers
            .iter()
            .find(|pr| pr.id == self.selected_runtime)
            .cloned();
        let ready_providers = self.providers.iter().filter(|p| p.is_ready()).count();
        let total_providers = self.providers.len();

        // Shared-element spring-gliding pill sensor across mode & chat rows
        let nav_sensor = pro_selection_sensor();
        let active_nav_key: SharedString = if self.coedit_panel_open {
            "mode-coedit".into()
        } else {
            match &self.access_mode {
                ChatAccessMode::DemoShowcase => "mode-demo".into(),
                ChatAccessMode::HttpReadOnly { .. } => "mode-http".into(),
                _ => active_chat_id
                    .map(|id| format!("chat-{id}").into())
                    .unwrap_or_else(|| "none".into()),
            }
        };

        let is_http_active = self.access_mode.is_http_read_only();
        let is_demo_active = matches!(self.access_mode, ChatAccessMode::DemoShowcase);
        let is_coedit_active = self.coedit_panel_open;

        // 1. Top 48px Window Chrome Bar (clears macOS traffic lights + draggable)
        let top_bar = div()
            .h(px(HEADER_H))
            .pl(px(if cfg!(target_os = "macos") {
                MACOS_TRAFFIC_LIGHT_PAD
            } else {
                14.
            }))
            .pr(px(10.))
            .window_control_area(WindowControlArea::Drag)
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .child(render_kar_brand_lockup(&s, cx))
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
            ));

        // 2. Claude Desktop Signature `+ 新建对话` + `⌘K` Search Bar
        let hover_new = s.stage_hover;
        let accent_c = s.accent;
        let on_accent = s.on_primary;
        let search_bg = if s.dark {
            hsla(45. / 360., 0.10, 0.90, 0.045)
        } else {
            hsla(0., 0., 1., 0.55)
        };
        let search_hover = s.stage_hover;

        let quick_launch_bar = div()
            .px(px(10.))
            .pt(px(2.))
            .pb(px(8.))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(activate(
                div()
                    .id("side-new-real-chat")
                    .role(Role::Button)
                    .tab_index(0)
                    .h(px(34.))
                    .px(px(8.))
                    .rounded(px(9.))
                    .hover(move |st| st.bg(hover_new))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .cursor_pointer()
                    .child(
                        div()
                            .size(px(22.))
                            .rounded_full()
                            .bg(accent_c)
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(pro_icon(ProIcon::Plus, 13., on_accent)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child("新建对话"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(s.muted)
                            .child(runtime_display_name(&self.selected_runtime)),
                    ),
                cx,
                |this, _, cx| {
                    this.coedit_panel_open = false;
                    this.create_new_real_chat(cx);
                },
            ))
            .child(activate(
                div()
                    .id("side-open-cmd-palette")
                    .role(Role::Button)
                    .tab_index(0)
                    .h(px(32.))
                    .px(px(9.))
                    .rounded(px(8.5))
                    .bg(search_bg)
                    .border_1()
                    .border_color(s.border)
                    .hover(move |st| st.bg(search_hover))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .cursor_pointer()
                    .child(pro_icon(ProIcon::Search, 13.5, s.muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.5))
                            .text_color(s.muted)
                            .child("搜索会话、文档与命令…"),
                    )
                    .child(render_keycap("⌘K", &s)),
                cx,
                |this, window, cx| this.toggle_command_palette(window, cx),
            ));

        // 3. Modes & Diagnostics Section
        let prov_count_tag = if total_providers > 0 {
            Some(format!("{ready_providers} 已安装"))
        } else {
            None
        };
        let mode_group = div()
            .flex()
            .flex_col()
            .gap(px(1.5))
            .child(section_heading("工作台", None, &s))
            .child(render_mode_row(
                "nav-provider-health",
                "供应商健康诊断",
                ProIcon::Activity,
                self.provider_sheet_open,
                prov_count_tag.as_deref(),
                false,
                &nav_sensor,
                &s,
                cx,
                |this, _, cx| this.toggle_provider_sheet(cx),
            ))
            .child(render_mode_row(
                "nav-http-feed",
                "连接 HTTP 订阅…",
                ProIcon::Globe,
                is_http_active,
                None,
                false,
                &nav_sensor,
                &s,
                cx,
                |this, window, cx| {
                    this.http_dialog
                        .update(cx, |d, cx| d.set_open(true, window, cx));
                },
            ))
            .child(render_mode_row(
                "nav-demo-chat",
                "独立演示对话",
                ProIcon::Sparkles,
                is_demo_active,
                Some("样例"),
                true,
                &nav_sensor,
                &s,
                cx,
                |this, _, _| {
                    this.bridge.send(KarCommand::EnterDemoShowcase);
                },
            ))
            .child(render_mode_row(
                "nav-coedit-demo",
                "人机双写演练",
                ProIcon::GitBranch,
                is_coedit_active,
                Some("样例"),
                true,
                &nav_sensor,
                &s,
                cx,
                |this, window, cx| {
                    this.enter_coedit_demo_workshop(window, cx);
                },
            ));

        // 4. Recent Chats (`ChatStore` sessions)
        let mut chats_group = div().flex().flex_col().gap(px(1.5)).child(section_heading(
            "最近对话",
            Some(self.chat_list.len()),
            &s,
        ));

        if self.chat_list.is_empty() {
            chats_group = chats_group.child(
                div()
                    .mx(px(4.))
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(s.muted)
                    .child("暂无历史记录 — 在右侧输入即可自动创建真实持久化会话"),
            );
        } else {
            for entry in &self.chat_list {
                let chat_id = entry.info.id;
                let selected = active_chat_id == Some(chat_id)
                    && !matches!(self.access_mode, ChatAccessMode::DemoShowcase)
                    && !self.coedit_panel_open;
                let title = entry.title.clone();
                let meta = format!(
                    "{} · {} 回合 · {}",
                    runtime_display_name(&entry.info.runtime),
                    entry.turn_count,
                    status_short_text(entry.last_status)
                );
                let dot_tone = match entry.last_status {
                    Some(TurnStatus::Running) => DotTone::Info,
                    Some(TurnStatus::Completed) => DotTone::Success,
                    Some(TurnStatus::Interrupted) | Some(TurnStatus::Aborted) => DotTone::Warning,
                    Some(TurnStatus::Failed) => DotTone::Danger,
                    None => DotTone::Neutral,
                };
                let hover_row = s.stage_hover;

                chats_group = chats_group.child(
                    div()
                        .relative()
                        .px(px(8.))
                        .py(px(5.5))
                        .rounded(px(8.))
                        .group("chat-item")
                        .when(selected, |d| d.child(pro_mark_selected(&nav_sensor)))
                        .when(!selected, |d| d.hover(move |st| st.bg(hover_row)))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(activate(
                            div()
                                .id(SharedString::from(format!("chat-open-{chat_id}")))
                                .role(Role::Button)
                                .tab_index(0)
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .cursor_pointer()
                                .child(render_runtime_mark(&entry.info.runtime, 13.5, &s, cx))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap(px(1.))
                                        .child(
                                            div()
                                                .text_size(px(12.5))
                                                .font_weight(if selected {
                                                    FontWeight::MEDIUM
                                                } else {
                                                    FontWeight::NORMAL
                                                })
                                                .text_color(if selected { s.ink } else { s.text })
                                                .truncate()
                                                .child(title),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(4.5))
                                                .text_size(px(10.5))
                                                .text_color(s.muted)
                                                .child(
                                                    div()
                                                        .size(px(5.))
                                                        .rounded_full()
                                                        .bg(dot_tone.dot_color(&s)),
                                                )
                                                .child(div().min_w_0().truncate().child(meta)),
                                        ),
                                ),
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
                        .child(pro_icon_btn(
                            SharedString::from(format!("chat-ro-{chat_id}")),
                            "以只读订阅打开会话",
                            ProIcon::Eye,
                            24.,
                            false,
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
                );
            }
        }

        // 5. Kantor Office Artifacts Section (open tabs + 1-click sample launcher)
        let mut docs_group = div().flex().flex_col().gap(px(1.5)).child(section_heading(
            "Office 协作画布",
            (!self.office_tabs.is_empty()).then_some(self.office_tabs.len()),
            &s,
        ));

        if self.office_tabs.is_empty() {
            let quick_samples: [(OfficeKind, &'static str, &'static str); 3] = [
                (OfficeKind::Word, "sample_minutes.docx", "会议纪要样本.docx"),
                (
                    OfficeKind::Excel,
                    "sample_formulas.xlsx",
                    "季度预算公式表.xlsx",
                ),
                (OfficeKind::Ppt, "sample_deck.pptx", "产品发布文稿.pptx"),
            ];
            for (si, (kind, fname, label)) in quick_samples.into_iter().enumerate() {
                let hover_doc = s.stage_hover;
                docs_group = docs_group.child(activate(
                    div()
                        .id(SharedString::from(format!("side-quick-doc-{si}")))
                        .role(Role::Button)
                        .tab_index(0)
                        .h(px(30.))
                        .px(px(8.))
                        .rounded(px(7.5))
                        .hover(move |st| st.bg(hover_doc))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .cursor_pointer()
                        .child(render_office_chip(kind, true, &s))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.5))
                                .text_color(s.text)
                                .child(label),
                        )
                        .child(pro_icon(ProIcon::ChevronRight, 11.5, s.faint)),
                    cx,
                    move |this, window, cx| {
                        this.open_working_sample(fname, window, cx);
                    },
                ));
            }
        } else {
            for (idx, tab) in self.office_tabs.iter().enumerate() {
                let is_active = self.active_office_tab == Some(idx)
                    && self.layout_mode != WorkspaceLayoutMode::ChatFocus;
                let raw_name = tab.doc_name(cx);
                let clean_name = clean_display_doc_name(&raw_name);
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
                        .gap(px(8.))
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
                            d.child(div().size(px(6.)).rounded_full().bg(s.pro.amber))
                        })
                        .when(is_demo_doc, |d| d.child(render_demo_pill("演示", &s))),
                    cx,
                    move |this, _, cx| {
                        if this.layout_mode == WorkspaceLayoutMode::ChatFocus {
                            this.layout_mode = WorkspaceLayoutMode::Split;
                        }
                        this.active_office_tab = Some(idx);
                        if let Some(sc_str) =
                            this.office_tabs.get(idx).and_then(|t| t.demo_scenario_key)
                        {
                            this.coedit_panel_open = true;
                            this.active_coedit_scenario = match sc_str {
                                "excel_same_cell" => ScenarioKey::ExcelSameCellConflict,
                                "ppt_move_text" => ScenarioKey::PptMoveAndText,
                                _ => ScenarioKey::WordSameParagraph,
                            };
                        }
                        cx.notify();
                    },
                ));
            }
        }

        // 6. Claude Desktop Bottom Runtime & Appearance Footer Card
        let store_tail = self
            .config
            .chat_dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(|name| {
                if let Some(parent) = self
                    .config
                    .chat_dir
                    .parent()
                    .and_then(|pr| pr.file_name())
                    .and_then(|n| n.to_str())
                {
                    format!("{parent}/{name}")
                } else {
                    name.to_owned()
                }
            })
            .unwrap_or_else(|| self.config.chat_dir.display().to_string());

        let (prov_status_text, prov_ok) = selected_prov
            .as_ref()
            .map(|pr| pr.status_badge())
            .unwrap_or(("正在探测…", true));
        let prov_dot = if prov_ok {
            s.pro.green
        } else {
            s.pro.amber
        };
        let footer_bg = if s.dark {
            hsla(45. / 360., 0.08, 0.90, 0.045)
        } else {
            hsla(0., 0., 1., 0.52)
        };
        let footer_hover = s.stage_hover;

        let footer_card = div()
            .px(px(10.))
            .py(px(10.))
            .flex_none()
            .child(
                div()
                    .p(px(7.))
                    .rounded(px(11.))
                    .bg(footer_bg)
                    .border_1()
                    .border_color(s.border)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(activate(
                        div()
                            .id("side-footer-provider-btn")
                            .role(Role::Button)
                            .tab_index(0)
                            .flex_1()
                            .min_w_0()
                            .px(px(4.))
                            .py(px(2.))
                            .rounded(px(7.))
                            .hover(move |st| st.bg(footer_hover))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .cursor_pointer()
                            .child(
                                div()
                                    .relative()
                                    .size(px(28.))
                                    .rounded(px(7.5))
                                    .bg(s.subtle_bg)
                                    .border_1()
                                    .border_color(s.border)
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(render_runtime_mark(
                                        &self.selected_runtime,
                                        15.,
                                        &s,
                                        cx,
                                    ))
                                    .child(
                                        div()
                                            .absolute()
                                            .right(px(-2.))
                                            .bottom(px(-2.))
                                            .size(px(8.))
                                            .rounded_full()
                                            .bg(prov_dot)
                                            .border_1()
                                            .border_color(s.stage),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(12.5))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(s.ink)
                                                    .truncate()
                                                    .child(runtime_display_name(
                                                        &self.selected_runtime,
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(10.5))
                                                    .text_color(s.muted)
                                                    .child(prov_status_text),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.5))
                                            .text_color(s.faint)
                                            .truncate()
                                            .child(format!("ChatStore · {store_tail}")),
                                    ),
                            ),
                        cx,
                        |this, _, cx| this.toggle_provider_sheet(cx),
                    ))
                    .child(self.theme_switch.clone()),
            );

        div()
            .size_full()
            .bg(s.stage)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(top_bar)
            .child(quick_launch_bar)
            .child(
                div()
                    .id("kar-sidebar-scroll")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(10.))
                    .py(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(pro_pill_indicator(
                        "kar-sidebar-nav-pill",
                        active_nav_key,
                        nav_sensor,
                        8.0,
                        s.stage_active,
                        None,
                        false,
                    ))
                    .child(mode_group)
                    .child(chats_group)
                    .child(docs_group),
            )
            .child(footer_card)
            .into_any_element()
    }
}

fn section_heading(title: &'static str, count: Option<usize>, s: &StudioPalette) -> Div {
    div()
        .h(px(22.))
        .px(px(8.))
        .flex()
        .items_center()
        .justify_between()
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
        })
}

#[allow(clippy::too_many_arguments)]
fn render_mode_row(
    id: &'static str,
    label: &'static str,
    icon: ProIcon,
    selected: bool,
    tag: Option<&str>,
    is_demo_tag: bool,
    sensor: &kui::components::pro_chrome::ProSelectionSensor,
    s: &StudioPalette,
    cx: &mut Context<KarWorkbench>,
    on_press: impl Fn(&mut KarWorkbench, &mut Window, &mut Context<KarWorkbench>) + Clone + 'static,
) -> Stateful<Div> {
    let hover_c = s.stage_hover;
    activate(
        div()
            .id(id)
            .role(Role::Button)
            .tab_index(0)
            .relative()
            .h(px(30.))
            .px(px(8.))
            .rounded(px(8.))
            .when(selected, |d| d.child(pro_mark_selected(sensor)))
            .when(!selected, |d| d.hover(move |st| st.bg(hover_c)))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .cursor_pointer()
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(pro_icon(
                        icon,
                        14.5,
                        if selected { s.ink } else { s.muted },
                    ))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.5))
                            .font_weight(if selected {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if selected { s.ink } else { s.text })
                            .child(label),
                    ),
            )
            .when_some(tag.map(str::to_owned), |d, t| {
                if is_demo_tag {
                    d.child(render_demo_pill(t, s))
                } else {
                    d.child(
                        div()
                            .h(px(19.))
                            .px(px(6.))
                            .rounded_full()
                            .bg(s.pro.green_soft)
                            .flex()
                            .items_center()
                            .text_size(px(10.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.pro.green_ink)
                            .child(t),
                    )
                }
            }),
        cx,
        on_press,
    )
}
