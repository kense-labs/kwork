//! Claude Artifacts-style Kantor Office Studio & 3-Way Human-AI Co-Editing Inspector
//! powered by KUI `SegmentedControl`, spring-gliding tabs, and real Kantor GPUI editors.

use gpui::{prelude::*, *};
use kui::{
    components::pro_chrome::{
        ProBtnStyle, pro_btn, pro_icon_btn, pro_mark_selected, pro_pill_indicator,
        pro_selection_sensor,
    },
    primitives::{
        control::activate,
        pro_icons::{ProIcon, pro_icon},
    },
};

use crate::{
    coedit::{CoEditProposal, CoEditStatus, ScenarioKey, UserResolutionChoice},
    types::{OfficeKind, format_rev},
    ui_primitives::{
        DotTone, HEADER_H, StudioPalette, clean_display_doc_name, render_claude_spark,
        render_demo_pill, render_office_chip,
    },
    workbench::{KarWorkbench, WorkspaceLayoutMode},
};

impl KarWorkbench {
    pub(crate) fn render_office_pane(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let s = StudioPalette::get(cx);

        // Sliding document tab pill (`pro_pill_indicator`)
        let tab_sensor = pro_selection_sensor();
        let active_tab_key: SharedString = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .map(|t| format!("otab-{}", t.id).into())
            .unwrap_or_else(|| "none".into());

        let mut tabs_row = div()
            .relative()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(pro_pill_indicator(
                "kar-office-tab-pill",
                active_tab_key,
                tab_sensor.clone(),
                8.0,
                s.card_bg,
                Some(s.border),
                false,
            ));

        if self.office_tabs.is_empty() {
            tabs_row = tabs_row.child(
                div()
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.ink)
                    .child(pro_icon(ProIcon::PageDoc, 15., s.accent))
                    .child("Kantor Office 协作画布"),
            );
        } else {
            for (idx, tab) in self.office_tabs.iter().enumerate() {
                let selected = self.active_office_tab == Some(idx);
                let clean_name = clean_display_doc_name(&tab.doc_name(cx));
                let dirty = tab.is_dirty(cx);
                let is_demo = tab.is_demo;
                let kind = tab.kind;
                let tid = tab.id;
                let hover_c = s.stage_hover;

                tabs_row = tabs_row.child(
                    div()
                        .relative()
                        .h(px(32.))
                        .pl(px(9.))
                        .pr(px(6.))
                        .rounded(px(8.))
                        .when(selected, |d| d.child(pro_mark_selected(&tab_sensor)))
                        .when(!selected, |d| d.hover(move |st| st.bg(hover_c)))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(activate(
                            div()
                                .id(SharedString::from(format!("office-tab-{tid}")))
                                .role(Role::Tab)
                                .tab_index(0)
                                .flex()
                                .items_center()
                                .gap(px(6.5))
                                .cursor_pointer()
                                .child(render_office_chip(kind, true, &s))
                                .child(
                                    div()
                                        .max_w(px(140.))
                                        .truncate()
                                        .text_size(px(12.5))
                                        .font_weight(if selected {
                                            FontWeight::MEDIUM
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(if selected { s.ink } else { s.muted })
                                        .child(clean_name),
                                )
                                .when(dirty, |d| {
                                    d.child(div().size(px(5.5)).rounded_full().bg(s.pro.amber))
                                })
                                .when(is_demo, |d| d.child(render_demo_pill("演示", &s))),
                            cx,
                            move |this, _, cx| {
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
                        ))
                        .child(activate(
                            div()
                                .id(SharedString::from(format!("office-tab-close-{tid}")))
                                .role(Role::Button)
                                .aria_label("关闭文档标签")
                                .tab_index(0)
                                .size(px(18.))
                                .rounded(px(4.5))
                                .hover(move |st| st.bg(hover_c))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .child(pro_icon(ProIcon::Close, 10.5, s.faint)),
                            cx,
                            move |this, _, cx| {
                                if idx < this.office_tabs.len() {
                                    this.office_tabs.remove(idx);
                                    this.active_office_tab =
                                        this.office_tabs.len().checked_sub(1);
                                    if !this.office_tabs.iter().any(|t| t.is_demo) {
                                        this.coedit_panel_open = false;
                                    }
                                    cx.notify();
                                }
                            },
                        )),
                );
            }
        }

        let active_tab = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .cloned();
        let has_tab = active_tab.is_some();
        let active_is_demo = active_tab.as_ref().is_some_and(|t| t.is_demo);

        // Unified 48px Claude Artifact Header Bar (tabs on left, crisp actions + expand/close on right)
        let accent_soft = s.accent_soft;
        let accent_ink = s.accent_ink;
        let hover_hdr = s.stage_hover;
        let header = div()
            .h(px(HEADER_H))
            .pl(px(10.))
            .pr(px(10.))
            .border_b_1()
            .border_color(s.border_soft)
            .bg(s.artifact_bg)
            .window_control_area(WindowControlArea::Drag)
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(tabs_row)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .when(has_tab, |d| {
                        d.child(activate(
                            div()
                                .id("btn-ask-ai-selection")
                                .role(Role::Button)
                                .tab_index(0)
                                .h(px(28.))
                                .px(px(10.))
                                .rounded(px(7.5))
                                .bg(accent_soft)
                                .border_1()
                                .border_color(s.accent.opacity(0.24))
                                .hover(|st| st.opacity(0.88))
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(accent_ink)
                                .cursor_pointer()
                                .child(render_claude_spark(12., s.accent))
                                .child("AI 改选区"),
                            cx,
                            |this, window, cx| this.bind_active_selection_to_chat(window, cx),
                        ))
                        .child(pro_icon_btn(
                            "btn-open-quick-edit",
                            "精确编辑选区段落 / 公式 / 坐标",
                            ProIcon::Sliders,
                            28.,
                            false,
                            cx,
                            |this, window, cx| this.open_quick_edit_modal(window, cx),
                        ))
                        .when(!active_is_demo, |el| {
                            el.child(activate(
                                div()
                                    .id("btn-office-save")
                                    .role(Role::Button)
                                    .tab_index(0)
                                    .h(px(28.))
                                    .px(px(8.))
                                    .rounded(px(7.))
                                    .hover(move |st| st.bg(hover_hdr))
                                    .flex()
                                    .items_center()
                                    .text_size(px(12.))
                                    .text_color(s.text)
                                    .cursor_pointer()
                                    .child("保存"),
                                cx,
                                |this, window, cx| {
                                    if let Some(i) = this.active_office_tab
                                        && let Some(t) = this.office_tabs.get(i).cloned()
                                    {
                                        let fname = clean_display_doc_name(&t.doc_name(cx));
                                        match t.save_in_place(window, cx) {
                                            Ok(_) => {
                                                let msg = format!("已保存 · {fname}");
                                                this.office_notice = Some(msg.clone());
                                                this.notify_toast(msg, cx);
                                            }
                                            Err(err) => {
                                                this.office_notice = Some(format!("{err:#}"));
                                            }
                                        }
                                        cx.notify();
                                    }
                                },
                            ))
                            .child(pro_btn(
                                "btn-office-roundtrip",
                                "重开核验",
                                Some(ProIcon::Refresh),
                                ProBtnStyle::Secondary,
                                true,
                                cx,
                                |this, window, cx| {
                                    this.save_close_and_reopen_active_tab(window, cx);
                                },
                            ))
                        })
                        .when(active_is_demo, |el| {
                            el.child(pro_btn(
                                "btn-toggle-coedit-panel",
                                if self.coedit_panel_open {
                                    "收起裁决"
                                } else {
                                    "展开裁决"
                                },
                                Some(ProIcon::GitBranch),
                                ProBtnStyle::Secondary,
                                true,
                                cx,
                                |this, _, cx| {
                                    this.coedit_panel_open = !this.coedit_panel_open;
                                    cx.notify();
                                },
                            ))
                        })
                    })
                    .child(self.office_menu.clone())
                    .child(pro_icon_btn(
                        "office-wide-toggle-btn",
                        if self.layout_mode == WorkspaceLayoutMode::CanvasWide {
                            "恢复标准分栏宽度"
                        } else {
                            "展开宽幅 Office 画布"
                        },
                        ProIcon::Expand,
                        28.,
                        self.layout_mode == WorkspaceLayoutMode::CanvasWide,
                        cx,
                        |this, _, cx| {
                            this.layout_mode =
                                if this.layout_mode == WorkspaceLayoutMode::CanvasWide {
                                    WorkspaceLayoutMode::Split
                                } else {
                                    WorkspaceLayoutMode::CanvasWide
                                };
                            cx.notify();
                        },
                    ))
                    .child(pro_icon_btn(
                        "office-close-pane-btn",
                        "收起 Office 协作画布",
                        ProIcon::Close,
                        28.,
                        false,
                        cx,
                        |this, _, cx| {
                            this.layout_mode = WorkspaceLayoutMode::ChatFocus;
                            cx.notify();
                        },
                    )),
            );

        let body = match active_tab {
            None => self.render_empty_office_launcher(&s, cx),
            Some(tab) => {
                let sel = tab.capture_selection(cx);
                let excerpt = if sel.excerpt.is_empty() {
                    "(空白)".to_owned()
                } else {
                    sel.excerpt.clone()
                };
                let rev_str = format_rev(sel.base_revision.unwrap_or(0));

                // Whisper-Quiet 28px Selection Breadcrumb Strip (replaces bulky second button bar!)
                let crumb_strip = div()
                    .h(px(29.))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(s.border_soft)
                    .bg(s.artifact_bg)
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(pro_icon(ProIcon::Pin, 11.5, s.accent))
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(sel.locator.target_label()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(s.faint)
                                    .child(rev_str),
                            )
                            .child(div().text_size(px(11.5)).text_color(s.faint).child("·"))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(s.muted)
                                    .child(excerpt),
                            ),
                    )
                    .when_some(self.office_notice.clone(), |d, note| {
                        d.child(
                            div()
                                .h(px(20.))
                                .px(px(7.))
                                .rounded_full()
                                .bg(s.pro.green_soft)
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .text_size(px(10.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(s.pro.green_ink)
                                .child(pro_icon(ProIcon::Check, 10., s.pro.green_ink))
                                .child(div().max_w(px(220.)).truncate().child(note)),
                        )
                    });

                let _ = window;
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(crumb_strip)
                    .when(self.coedit_panel_open, |d| {
                        d.child(self.render_coedit_inspector(&s, cx))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .child(tab.any_view()),
                    )
                    .into_any_element()
            }
        };

        div()
            .size_full()
            .bg(s.inset_bg)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
            .into_any_element()
    }

    fn render_empty_office_launcher(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let items: [(OfficeKind, &'static str, &'static str, &'static str); 3] = [
            (
                OfficeKind::Word,
                "sample_minutes.docx",
                "会议纪要样本 (.docx)",
                "原生段落排版、样式编辑、选区绑定与保存重开核验",
            ),
            (
                OfficeKind::Excel,
                "sample_formulas.xlsx",
                "季度公式预算表 (.xlsx)",
                "单元格值与 =SUM(...) 公式实时重算及跨表定位",
            ),
            (
                OfficeKind::Ppt,
                "sample_deck.pptx",
                "产品发布演示文稿 (.pptx)",
                "幻灯片对象坐标拖拽、文字编辑与原目标追踪",
            ),
        ];

        let hover_c = s.stage_hover;
        let card_bg = s.card_bg;
        let line_c = s.border;
        let line_soft = s.border_soft;

        div()
            .size_full()
            .p(px(28.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(480.))
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(render_claude_spark(18., s.accent))
                                    .child(
                                        div()
                                            .text_size(px(17.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.ink)
                                            .child("选择或新建 Kantor Office 协作文档"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(20.))
                                    .text_color(s.muted)
                                    .child("在同一原生 GPUI 窗口内打开测试副本，随时选中任意段落、单元格或幻灯片对象交由左侧 Kar 会话协同处理。"),
                            ),
                    )
                    .child(
                        div()
                            .rounded(px(14.))
                            .bg(card_bg)
                            .border_1()
                            .border_color(line_c)
                            .shadow(s.composer_shadows())
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .children(items.into_iter().enumerate().map(
                                |(idx, (kind, fname, title, desc))| {
                                    activate(
                                        div()
                                            .id(SharedString::from(format!("launch-doc-{idx}")))
                                            .role(Role::Button)
                                            .tab_index(0)
                                            .px(px(15.))
                                            .py(px(12.))
                                            .when(idx > 0, |d| {
                                                d.border_t_1().border_color(line_soft)
                                            })
                                            .hover(move |st| st.bg(hover_c))
                                            .flex()
                                            .items_center()
                                            .gap(px(12.))
                                            .cursor_pointer()
                                            .child(render_office_chip(kind, false, s))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(2.))
                                                    .child(
                                                        div()
                                                            .text_size(px(13.5))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(s.ink)
                                                            .child(title),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .text_color(s.muted)
                                                            .child(desc),
                                                    ),
                                            )
                                            .child(pro_icon(ProIcon::ChevronRight, 13.5, s.faint)),
                                        cx,
                                        move |this, window, cx| {
                                            this.open_working_sample(fname, window, cx);
                                        },
                                    )
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(pro_btn(
                                "empty-open-path",
                                "按本地路径打开…",
                                Some(ProIcon::Folder),
                                ProBtnStyle::Secondary,
                                false,
                                cx,
                                |this, window, cx| {
                                    this.open_path_dialog
                                        .update(cx, |d, cx| d.set_open(true, window, cx));
                                },
                            ))
                            .child(pro_btn(
                                "empty-open-coedit",
                                "启动人机双写演练 (样例)",
                                Some(ProIcon::GitBranch),
                                ProBtnStyle::Secondary,
                                false,
                                cx,
                                |this, window, cx| this.enter_coedit_demo_workshop(window, cx),
                            )),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn render_coedit_inspector(
        &mut self,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sc = self.active_coedit_scenario;
        if let Some(live) = self.read_live_coedit_state(sc, cx) {
            let _ = self.coedit_engine.sync_live_state(&live);
        }
        let Some(prop): Option<CoEditProposal> = self
            .coedit_engine
            .proposals()
            .iter()
            .find(|pr| pr.scenario == sc)
            .cloned()
        else {
            return div().into_any_element();
        };

        let op_id = prop.operation_id;
        let short_op = &op_id.to_string()[..8];
        let base_rev = format_rev(prop.base_revision);
        let latest_rev = format_rev(prop.latest_revision);

        // Crisp 3-Segment Scenario Switcher with KUI spring-gliding pill
        let sc_sensor = pro_selection_sensor();
        let sc_key: SharedString = sc.as_str().into();
        let scenarios: [(ScenarioKey, &'static str); 3] = [
            (ScenarioKey::WordSameParagraph, "Word 同段"),
            (ScenarioKey::ExcelSameCellConflict, "Excel 同格"),
            (ScenarioKey::PptMoveAndText, "PPT 移位/删除"),
        ];
        let seg_hover = s.stage_hover;
        let segmented = div()
            .relative()
            .h(px(30.))
            .p(px(3.))
            .rounded(px(8.5))
            .bg(s.tray_bg)
            .border_1()
            .border_color(s.border)
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(pro_pill_indicator(
                "coedit-scenario-pill",
                sc_key,
                sc_sensor.clone(),
                6.0,
                s.card_bg,
                Some(s.border),
                false,
            ))
            .children(scenarios.into_iter().enumerate().map(|(si, (key, label))| {
                let active = sc == key;
                activate(
                    div()
                        .id(SharedString::from(format!("coedit-seg-{si}")))
                        .role(Role::Tab)
                        .tab_index(0)
                        .relative()
                        .h(px(24.))
                        .px(px(10.))
                        .rounded(px(6.))
                        .when(active, |d| d.child(pro_mark_selected(&sc_sensor)))
                        .when(!active, |d| d.hover(move |st| st.bg(seg_hover)))
                        .flex()
                        .items_center()
                        .whitespace_nowrap()
                        .text_size(px(12.))
                        .font_weight(if active {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(if active { s.ink } else { s.muted })
                        .cursor_pointer()
                        .child(label),
                    cx,
                    move |this, window, cx| this.switch_coedit_scenario(key, window, cx),
                )
            }));

        // Compact Simulation Triggers Row
        let sim_row = match sc {
            ScenarioKey::WordSameParagraph => div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(s.muted)
                        .mr(px(2.))
                        .child("并发演练"),
                )
                .child(pro_btn(
                    "sim-word-date",
                    "人工改同段日期 → 2026-12-01",
                    Some(ProIcon::Play),
                    ProBtnStyle::Secondary,
                    true,
                    cx,
                    |this, window, cx| {
                        this.trigger_coedit_human_step("word_human_date", window, cx);
                    },
                ))
                .child(pro_btn(
                    "sim-word-mid",
                    "解决期再次修改段落 (触发重确认)",
                    None,
                    ProBtnStyle::Ghost,
                    true,
                    cx,
                    |this, window, cx| {
                        this.trigger_coedit_human_step("word_mid_conflict_edit", window, cx);
                    },
                )),
            ScenarioKey::ExcelSameCellConflict => div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(s.muted)
                        .mr(px(2.))
                        .child("并发演练"),
                )
                .child(pro_btn(
                    "sim-excel-b3",
                    "人工改 B3 → 135,000 (制造同格冲突)",
                    Some(ProIcon::Play),
                    ProBtnStyle::Secondary,
                    true,
                    cx,
                    |this, window, cx| {
                        this.trigger_coedit_human_step("excel_human_b3", window, cx);
                    },
                ))
                .child(pro_btn(
                    "sim-excel-mid",
                    "解决期再次改 B3 → 139,000 (触发重确认)",
                    None,
                    ProBtnStyle::Ghost,
                    true,
                    cx,
                    |this, window, cx| {
                        this.trigger_coedit_human_step("excel_mid_conflict_edit", window, cx);
                    },
                )),
            ScenarioKey::PptMoveAndText => div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(s.muted)
                        .mr(px(2.))
                        .child("并发演练"),
                )
                .child(pro_btn(
                    "sim-ppt-move",
                    "人工移位文本框 → (6.50, 4.80) cm",
                    Some(ProIcon::Play),
                    ProBtnStyle::Secondary,
                    true,
                    cx,
                    |this, window, cx| {
                        this.trigger_coedit_human_step("ppt_human_move", window, cx);
                    },
                ))
                .child(pro_btn(
                    "sim-ppt-mid",
                    "解决期再次移位 (触发重确认)",
                    None,
                    ProBtnStyle::Ghost,
                    true,
                    cx,
                    |this, window, cx| {
                        this.trigger_coedit_human_step("ppt_mid_conflict_move", window, cx);
                    },
                ))
                .child(pro_btn(
                    "sim-ppt-del",
                    "人工删除目标文本框 (目标冲突)",
                    Some(ProIcon::Trash),
                    ProBtnStyle::Danger,
                    true,
                    cx,
                    |this, window, cx| {
                        this.trigger_coedit_human_step("ppt_human_delete", window, cx);
                    },
                )),
        };

        // 3-Column Structured Diff Grid
        let is_deleted = prop.status == CoEditStatus::TargetDeletedConflict;
        let has_merged_preview = prop.merged_preview.is_some() && !is_deleted;
        let third_title = if has_merged_preview {
            "双项保留合并预览"
        } else {
            "AI 提议值"
        };
        let third_val = prop
            .merged_preview
            .clone()
            .filter(|_| !is_deleted)
            .unwrap_or_else(|| prop.proposed_value.clone());
        let third_tone = if is_deleted {
            DotTone::Danger
        } else if has_merged_preview {
            DotTone::Success
        } else {
            DotTone::Warning
        };

        let diff_grid = div()
            .w_full()
            .flex()
            .items_stretch()
            .gap(px(8.))
            .child(render_diff_card(
                "基准修订",
                &base_rev,
                &prop.base_value,
                DotTone::Neutral,
                false,
                s,
            ))
            .child(render_diff_card(
                "当前画布值 · 人工",
                &latest_rev,
                &prop.current_value,
                if is_deleted {
                    DotTone::Danger
                } else {
                    DotTone::Info
                },
                false,
                s,
            ))
            .child(render_diff_card(
                third_title,
                &format!("Op {short_op}"),
                &third_val,
                third_tone,
                true,
                s,
            ));

        // Decision & Stale-Revision Action Bar
        let status_summary = self
            .coedit_feedback
            .clone()
            .or_else(|| prop.conflict_reason.clone())
            .unwrap_or_else(|| {
                "选区与修订快照实时同步自下方原生 Kantor 编辑器（仅操作临时演示文档）。".into()
            });

        let decision_bar = match prop.status {
            CoEditStatus::StaleNeedsReconfirm {
                seen_revision,
                latest_revision: l_rev,
            } => div()
                .w_full()
                .px(px(12.))
                .py(px(9.))
                .rounded(px(10.))
                .bg(s.pro.amber_soft)
                .border_1()
                .border_color(s.pro.amber.opacity(0.32))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(12.))
                        .text_color(s.pro.amber_ink)
                        .child(pro_icon(ProIcon::AlertTriangle, 14., s.pro.amber_ink))
                        .child(
                            div().flex_1().min_w_0().child(format!(
                                "解决期间文档已产生新修订（{} → {}），请核对最新当前值后重新确认。",
                                format_rev(seen_revision),
                                format_rev(l_rev)
                            )),
                        ),
                )
                .child(pro_btn(
                    "btn-coedit-reconfirm",
                    format!("确认最新修订 {}", format_rev(l_rev)),
                    Some(ProIcon::Check),
                    ProBtnStyle::Primary,
                    true,
                    cx,
                    move |this, _, cx| {
                        let _ = this.coedit_engine.acknowledge_latest_revision(op_id, l_rev);
                        this.coedit_feedback = Some(format!(
                            "已确认最新修订 {}，现在可继续提交裁决选择。",
                            format_rev(l_rev)
                        ));
                        cx.notify();
                    },
                )),
            CoEditStatus::ReadyDualKeep => div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(12.))
                        .text_color(s.text)
                        .truncate()
                        .child(status_summary),
                )
                .child(pro_btn(
                    "btn-coedit-apply-dual",
                    "应用双项保留结果",
                    Some(ProIcon::Check),
                    ProBtnStyle::Primary,
                    true,
                    cx,
                    move |this, window, cx| {
                        this.submit_coedit_choice(
                            op_id,
                            UserResolutionChoice::ApplyDualKeep,
                            window,
                            cx,
                        );
                    },
                )),
            CoEditStatus::ConflictNeedsChoice => div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(s.pro.amber_ink)
                        .child(status_summary),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(pro_btn(
                                    "btn-coedit-keep",
                                    "保留当前值",
                                    None,
                                    ProBtnStyle::Secondary,
                                    true,
                                    cx,
                                    move |this, window, cx| {
                                        this.submit_coedit_choice(
                                            op_id,
                                            UserResolutionChoice::KeepCurrent,
                                            window,
                                            cx,
                                        );
                                    },
                                ))
                                .child(pro_btn(
                                    "btn-coedit-adopt",
                                    "采用 AI 提议",
                                    None,
                                    ProBtnStyle::Secondary,
                                    true,
                                    cx,
                                    move |this, window, cx| {
                                        this.submit_coedit_choice(
                                            op_id,
                                            UserResolutionChoice::AdoptProposal,
                                            window,
                                            cx,
                                        );
                                    },
                                )),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .w(px(156.))
                                        .h(px(28.))
                                        .px(px(8.))
                                        .rounded(px(7.))
                                        .bg(s.card_bg)
                                        .border_1()
                                        .border_color(s.border_strong)
                                        .flex()
                                        .items_center()
                                        .child(self.coedit_manual_editor.clone()),
                                )
                                .child(pro_btn(
                                    "btn-coedit-manual-submit",
                                    "手工合并",
                                    Some(ProIcon::Check),
                                    ProBtnStyle::Primary,
                                    true,
                                    cx,
                                    move |this, window, cx| {
                                        let val = this
                                            .coedit_manual_editor
                                            .read(cx)
                                            .value()
                                            .trim()
                                            .to_owned();
                                        this.submit_coedit_choice(
                                            op_id,
                                            UserResolutionChoice::ManualMerge { merged_value: val },
                                            window,
                                            cx,
                                        );
                                    },
                                )),
                        ),
                ),
            CoEditStatus::TargetDeletedConflict => div()
                .w_full()
                .px(px(12.))
                .py(px(8.))
                .rounded(px(10.))
                .bg(s.pro.red_soft)
                .border_1()
                .border_color(s.pro.red.opacity(0.28))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(12.))
                        .text_color(s.pro.red_ink)
                        .child(pro_icon(ProIcon::AlertTriangle, 14., s.pro.red_ink))
                        .child(div().flex_1().min_w_0().child(status_summary)),
                )
                .child(pro_btn(
                    "btn-coedit-dismiss-deleted",
                    "已知晓并丢弃提议",
                    None,
                    ProBtnStyle::Secondary,
                    true,
                    cx,
                    move |this, window, cx| {
                        this.submit_coedit_choice(
                            op_id,
                            UserResolutionChoice::DismissDeletedTarget,
                            window,
                            cx,
                        );
                    },
                )),
            CoEditStatus::Resolved { summary } => div()
                .w_full()
                .px(px(11.))
                .py(px(7.))
                .rounded(px(9.))
                .bg(s.pro.green_soft)
                .border_1()
                .border_color(s.pro.green.opacity(0.26))
                .flex()
                .items_center()
                .gap(px(7.))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(s.pro.green_ink)
                .child(pro_icon(ProIcon::Check, 13.5, s.pro.green_ink))
                .child(div().flex_1().min_w_0().child(summary)),
        };

        div()
            .id("coedit-inspector-panel")
            .max_h(px(345.))
            .overflow_y_scroll()
            .m(px(10.))
            .mb(px(4.))
            .p(px(12.))
            .rounded(px(13.))
            .border_1()
            .border_color(s.border_strong)
            .bg(s.card_bg)
            .shadow(s.composer_shadows())
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(10.))
            // Row 1: Title + Demo Pill + KUI SegmentedControl + Reset
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(render_demo_pill("演示数据 · 隔离沙盒", s))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(s.ink)
                                    .child("人机双写协议裁决"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(s.muted)
                                    .child(clean_display_doc_name(&prop.selection_desc)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(segmented)
                            .child(pro_icon_btn(
                                "coedit-reset-all",
                                "重置三组双写演练",
                                ProIcon::Refresh,
                                28.,
                                false,
                                cx,
                                |this, window, cx| {
                                    this.trigger_coedit_human_step("reset_all", window, cx);
                                },
                            )),
                    ),
            )
            .child(sim_row)
            .child(diff_grid)
            .child(decision_bar)
            .into_any_element()
    }
}

fn render_diff_card(
    heading: &'static str,
    meta: &str,
    value: &str,
    tone: DotTone,
    emphasized: bool,
    s: &StudioPalette,
) -> Div {
    let (bg, border) = if emphasized {
        match tone {
            DotTone::Success => (s.pro.green_soft, s.pro.green.opacity(0.28)),
            DotTone::Warning => (s.pro.amber_soft, s.pro.amber.opacity(0.28)),
            DotTone::Danger => (s.pro.red_soft, s.pro.red.opacity(0.28)),
            _ => (s.subtle_bg, s.border_strong),
        }
    } else {
        (s.subtle_bg, s.border)
    };

    div()
        .flex_1()
        .min_w_0()
        .p(px(9.5))
        .rounded(px(9.5))
        .bg(bg)
        .border_1()
        .border_color(border)
        .flex()
        .flex_col()
        .gap(px(5.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(4.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(tone.text_color(s))
                        .child(
                            div()
                                .size(px(5.5))
                                .rounded_full()
                                .bg(tone.dot_color(s)),
                        )
                        .child(heading),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(s.faint)
                        .child(meta.to_owned()),
                ),
        )
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(17.5))
                .text_color(s.ink)
                .child(if value.is_empty() {
                    "(无内容 / 对象已移除)".to_owned()
                } else {
                    value.to_owned()
                }),
        )
}
