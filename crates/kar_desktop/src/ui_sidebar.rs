//! KUI `AppShell`-aligned sidebar navigation rail with spring-gliding selection pills.

use gpui::{prelude::*, *};
use kar::chat::TurnStatus;
use kui::{
    components::pro_chrome::{
        ProBtnStyle, ProPalette, pro_btn, pro_icon_btn, pro_mark_selected, pro_pill_indicator,
        pro_selection_sensor,
    },
    primitives::{
        control::activate,
        pro_icons::{ProIcon, pro_icon},
    },
};

use crate::{
    apply_unified_theme,
    kar_bridge::KarCommand,
    types::ChatAccessMode,
    ui_primitives::{
        DotTone, HEADER_H, clean_display_doc_name, render_demo_pill, render_kar_brand_lockup,
        render_office_chip, render_runtime_mark, render_theme_glyph,
    },
    workbench::{KarWorkbench, status_short_text},
};

impl KarWorkbench {
    pub(crate) fn render_sidebar(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = ProPalette::get(cx);
        let active_chat_id = self.current_chat_id();

        let selected_prov = self
            .providers
            .iter()
            .find(|pr| pr.id == self.selected_runtime)
            .cloned();

        // Shared-element sliding pill sensor across mode & chat rows
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

        // Mode rows (`AppShell` 30px nav item geometry)
        let mode_group = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(section_heading("工作台入口", None, &p))
            .child(render_mode_row(
                "nav-http-feed",
                "连接 HTTP 订阅…",
                ProIcon::Globe,
                is_http_active,
                None,
                &nav_sensor,
                &p,
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
                &nav_sensor,
                &p,
                cx,
                |this, _, _| {
                    this.bridge.send(KarCommand::EnterDemoShowcase);
                },
            ))
            .child(render_mode_row(
                "nav-coedit-demo",
                "Office 人机双写演练",
                ProIcon::GitBranch,
                is_coedit_active,
                Some("样例"),
                &nav_sensor,
                &p,
                cx,
                |this, window, cx| {
                    this.enter_coedit_demo_workshop(window, cx);
                },
            ));

        // Persistent ChatStore sessions group
        let mut chats_group = div().flex().flex_col().gap(px(2.)).child(section_heading(
            "本地会话",
            Some(self.chat_list.len()),
            &p,
        ));

        if self.chat_list.is_empty() {
            chats_group = chats_group.child(
                div()
                    .px(px(8.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .bg(p.subtle)
                    .border_1()
                    .border_color(p.line_soft)
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(p.muted)
                    .child("暂无本地会话，点击上方「新建真实对话」即可创建。"),
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
                    entry.info.runtime,
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

                chats_group = chats_group.child(
                    div()
                        .relative()
                        .px(px(7.))
                        .py(px(5.))
                        .rounded(px(8.))
                        .when(selected, |d| d.child(pro_mark_selected(&nav_sensor)))
                        .when(!selected, |d| d.hover(move |s| s.bg(p.hover)))
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
                                .child(
                                    div()
                                        .size(px(6.))
                                        .rounded_full()
                                        .flex_none()
                                        .bg(dot_tone.dot_color(&p)),
                                )
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
                                                .text_color(if selected { p.ink } else { p.text })
                                                .truncate()
                                                .child(title),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(p.muted)
                                                .truncate()
                                                .child(meta),
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

        // Open Office tabs group
        let mut docs_group = div().flex().flex_col().gap(px(2.)).child(section_heading(
            "打开的文档",
            Some(self.office_tabs.len()),
            &p,
        ));
        if self.office_tabs.is_empty() {
            docs_group = docs_group.child(
                div()
                    .px(px(8.))
                    .py(px(6.))
                    .text_size(px(12.))
                    .text_color(p.faint)
                    .child("右侧可打开 .docx / .xlsx / .pptx"),
            );
        } else {
            for (idx, tab) in self.office_tabs.iter().enumerate() {
                let is_active = self.active_office_tab == Some(idx);
                let raw_name = tab.doc_name(cx);
                let clean_name = clean_display_doc_name(&raw_name);
                let dirty = tab.is_dirty(cx);
                let is_demo_doc = tab.is_demo;
                let kind = tab.kind;
                let tab_id = tab.id;

                docs_group = docs_group.child(activate(
                    div()
                        .id(SharedString::from(format!("side-doc-{tab_id}")))
                        .role(Role::Button)
                        .tab_index(0)
                        .h(px(29.))
                        .px(px(7.))
                        .rounded(px(7.))
                        .bg(if is_active {
                            p.select
                        } else {
                            transparent_black()
                        })
                        .hover(move |s| if is_active { s } else { s.bg(p.hover) })
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .cursor_pointer()
                        .child(render_office_chip(kind, true, &p))
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
                                .text_color(if is_active { p.ink } else { p.text })
                                .truncate()
                                .child(clean_name),
                        )
                        .when(dirty, |d| {
                            d.child(div().size(px(6.)).rounded_full().bg(p.amber))
                        })
                        .when(is_demo_doc, |d| d.child(render_demo_pill("演示", &p))),
                    cx,
                    move |this, _, cx| {
                        this.active_office_tab = Some(idx);
                        if let Some(sc_str) =
                            this.office_tabs.get(idx).and_then(|t| t.demo_scenario_key)
                        {
                            this.coedit_panel_open = true;
                            this.active_coedit_scenario = match sc_str {
                                "excel_same_cell" => crate::coedit::ScenarioKey::ExcelSameCellConflict,
                                "ppt_move_text" => crate::coedit::ScenarioKey::PptMoveAndText,
                                _ => crate::coedit::ScenarioKey::WordSameParagraph,
                            };
                        }
                        cx.notify();
                    },
                ));
            }
        }

        let store_tail = self
            .config
            .chat_dir
            .file_name()
            .and_then(|s| s.to_str())
            .map(|name| {
                if let Some(parent) = self
                    .config
                    .chat_dir
                    .parent()
                    .and_then(|pr| pr.file_name())
                    .and_then(|s| s.to_str())
                {
                    format!("{parent}/{name}")
                } else {
                    name.to_owned()
                }
            })
            .unwrap_or_else(|| self.config.chat_dir.display().to_string());

        let rt_id = self.selected_runtime.clone();
        let hover_hdr = p.hover;
        let is_dark = p.dark;
        let icon_fg = p.muted;

        div()
            .size_full()
            .bg(p.side_tint)
            .flex()
            .flex_col()
            .overflow_hidden()
            // 1. Unified 44px Top Header (exact 28x28 optical pair on right)
            .child(
                div()
                    .h(px(HEADER_H))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(p.line)
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(render_kar_brand_lockup(&p, cx))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(activate(
                                div()
                                    .id("side-theme-btn")
                                    .role(Role::Button)
                                    .aria_label("切换浅色或深色主题")
                                    .tab_index(0)
                                    .size(px(28.))
                                    .rounded(px(7.))
                                    .hover(move |s| s.bg(hover_hdr))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .child(render_theme_glyph(is_dark, icon_fg)),
                                cx,
                                |this, _, cx| {
                                    this.theme_mode = this.theme_mode.toggle();
                                    apply_unified_theme(this.theme_mode, cx);
                                    cx.notify();
                                },
                            ))
                            .child(pro_icon_btn(
                                "side-head-new-chat",
                                "新建真实对话",
                                ProIcon::NewChat,
                                28.,
                                false,
                                cx,
                                |this, _, cx| this.create_new_real_chat(cx),
                            )),
                    ),
            )
            // 2. Compact Runtime Row + New Chat Action
            .child(
                div()
                    .px(px(10.))
                    .py(px(9.))
                    .border_b_1()
                    .border_color(p.line_soft)
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(6.))
                            .child(self.runtime_menu.clone())
                            .when_some(selected_prov, |el, prov| {
                                let (badge_txt, ok) = prov.status_badge();
                                let dot = if ok { DotTone::Success } else { DotTone::Warning };
                                el.child(
                                    div()
                                        .min_w_0()
                                        .flex()
                                        .items_center()
                                        .gap(px(5.))
                                        .text_size(px(11.))
                                        .child(render_runtime_mark(&rt_id, 12.5, &p, cx))
                                        .child(
                                            div()
                                                .size(px(5.5))
                                                .rounded_full()
                                                .flex_none()
                                                .bg(dot.dot_color(&p)),
                                        )
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .text_color(if ok { p.muted } else { p.amber_ink })
                                                .child(badge_txt),
                                        ),
                                )
                            }),
                    )
                    .child(pro_btn(
                        "btn-new-real-chat",
                        "新建真实对话",
                        Some(ProIcon::Plus),
                        ProBtnStyle::Secondary,
                        false,
                        cx,
                        |this, _, cx| this.create_new_real_chat(cx),
                    )),
            )
            // 3. Scrollable Navigation & Session Rail with Sliding Pill Indicator
            .child(
                div()
                    .id("sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(8.))
                    .py(px(10.))
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(pro_pill_indicator(
                        "kar-side-nav-pill",
                        active_nav_key,
                        nav_sensor,
                        8.,
                        p.select,
                        None,
                        false,
                    ))
                    .child(mode_group)
                    .child(chats_group)
                    .child(docs_group),
            )
            // 4. Subtle Store Footer
            .child(
                div()
                    .h(px(32.))
                    .px(px(12.))
                    .border_t_1()
                    .border_color(p.line_soft)
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.))
                    .text_color(p.faint)
                    .child(pro_icon(ProIcon::Database, 12., p.faint))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(format!("ChatStore · {store_tail}")),
                    ),
            )
            .into_any_element()
    }
}

fn section_heading(title: &'static str, count: Option<usize>, p: &ProPalette) -> Div {
    div()
        .px(px(6.))
        .mb(px(2.))
        .flex()
        .items_center()
        .justify_between()
        .text_size(px(11.5))
        .text_color(p.faint)
        .child(title)
        .when_some(count, |d, n| d.child(n.to_string()))
}

#[allow(clippy::too_many_arguments)]
fn render_mode_row(
    id: &'static str,
    label: &'static str,
    icon: ProIcon,
    active: bool,
    demo_tag: Option<&'static str>,
    sensor: &kui::components::pro_chrome::ProSelectionSensor,
    p: &ProPalette,
    cx: &Context<KarWorkbench>,
    on_press: impl Fn(&mut KarWorkbench, &mut Window, &mut Context<KarWorkbench>) + Clone + 'static,
) -> Stateful<Div> {
    let icon_c = if active { p.ink } else { p.muted };
    let text_c = if active { p.ink } else { p.text };
    let hover_c = p.hover;
    let focus_c = p.focus;

    activate(
        div()
            .id(id)
            .role(Role::Button)
            .tab_index(0)
            .relative()
            .h(px(30.))
            .px(px(7.))
            .rounded(px(8.))
            .when(active, |d| d.child(pro_mark_selected(sensor)))
            .when(!active, |d| d.hover(move |s| s.bg(hover_c)))
            .focus_visible(move |s| s.border_1().border_color(focus_c))
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_pointer()
            .child(pro_icon(icon, 15., icon_c))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.5))
                    .font_weight(if active {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(text_c)
                    .child(label),
            )
            .when_some(demo_tag, |d, tag| d.child(render_demo_pill(tag, p))),
        cx,
        on_press,
    )
}
