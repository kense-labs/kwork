//! KUI `AppShell`-aligned Kantor Office tab host & 3-way Human-AI Co-Editing Inspector.

use gpui::{prelude::*, *};
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
    coedit::{CoEditProposal, CoEditStatus, ScenarioKey, UserResolutionChoice},
    types::{OfficeKind, format_rev},
    ui_primitives::{
        DotTone, HEADER_H, clean_display_doc_name, render_demo_pill, render_office_chip,
    },
    workbench::KarWorkbench,
};

impl KarWorkbench {
    pub(crate) fn render_office_pane(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = ProPalette::get(cx);

        // Sliding tab indicator (`AppShell` `wtab-pill` exact pattern)
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
                7.5,
                p.chip,
                None,
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
                    .text_color(p.ink)
                    .child(pro_icon(ProIcon::PageDoc, 15., p.muted))
                    .child("Kantor Office 工作区"),
            );
        } else {
            for (idx, tab) in self.office_tabs.iter().enumerate() {
                let selected = self.active_office_tab == Some(idx);
                let clean_name = clean_display_doc_name(&tab.doc_name(cx));
                let dirty = tab.is_dirty(cx);
                let is_demo = tab.is_demo;
                let kind = tab.kind;
                let tid = tab.id;
                let hover_c = p.hover;

                tabs_row = tabs_row.child(
                    div()
                        .relative()
                        .h(px(30.))
                        .pl(px(8.))
                        .pr(px(5.))
                        .rounded(px(7.5))
                        .when(selected, |d| d.child(pro_mark_selected(&tab_sensor)))
                        .when(!selected, |d| d.hover(move |s| s.bg(hover_c)))
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
                                .gap(px(6.))
                                .cursor_pointer()
                                .child(render_office_chip(kind, true, &p))
                                .child(
                                    div()
                                        .max_w(px(148.))
                                        .truncate()
                                        .text_size(px(12.5))
                                        .font_weight(if selected {
                                            FontWeight::MEDIUM
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(if selected { p.ink } else { p.muted })
                                        .child(clean_name),
                                )
                                .when(dirty, |d| {
                                    d.child(div().size(px(5.5)).rounded_full().bg(p.amber))
                                })
                                .when(is_demo, |d| d.child(render_demo_pill("演示", &p))),
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
                                .hover(move |s| s.bg(hover_c))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .child(pro_icon(ProIcon::Close, 10.5, p.faint)),
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

        // Top 44px Header (exact baseline match with Sidebar & Chat headers)
        let header = div()
            .h(px(HEADER_H))
            .pl(px(10.))
            .pr(px(12.))
            .border_b_1()
            .border_color(p.line)
            .bg(p.panel)
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
                    .child(pro_icon_btn(
                        "office-open-path-btn",
                        "按本地路径打开文档",
                        ProIcon::Folder,
                        28.,
                        false,
                        cx,
                        |this, window, cx| {
                            this.open_path_dialog
                                .update(cx, |d, cx| d.set_open(true, window, cx));
                        },
                    ))
                    .child(self.office_menu.clone()),
            );

        let active_tab = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .cloned();

        let body = match active_tab {
            None => self.render_empty_office_launcher(&p, cx),
            Some(tab) => {
                let sel = tab.capture_selection(cx);
                let is_demo = tab.is_demo;
                let excerpt = if sel.excerpt.is_empty() {
                    "(空白)".to_owned()
                } else {
                    sel.excerpt.clone()
                };
                let rev_str = format_rev(sel.base_revision.unwrap_or(0));

                // Compact Single-Row Selection & Document Action Ribbon (38px)
                let ribbon = div()
                    .h(px(38.))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(p.line)
                    .bg(p.subtle)
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
                            .gap(px(7.))
                            .child(pro_icon(ProIcon::Pin, 12.5, p.muted))
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.ink)
                                    .child(sel.locator.target_label()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(p.faint)
                                    .child(rev_str),
                            )
                            .child(div().text_size(px(12.)).text_color(p.faint).child("·"))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(p.muted)
                                    .child(excerpt),
                            )
                            .when_some(self.office_notice.clone(), |d, note| {
                                d.child(
                                    div()
                                        .h(px(22.))
                                        .px(px(7.))
                                        .rounded_full()
                                        .bg(p.green_soft)
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .text_size(px(11.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.green_ink)
                                        .child(pro_icon(ProIcon::Check, 10.5, p.green_ink))
                                        .child(
                                            div()
                                                .max_w(px(160.))
                                                .truncate()
                                                .child(note),
                                        ),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .child(pro_btn(
                                "btn-ask-ai-selection",
                                "AI 改选区",
                                Some(ProIcon::Sparkles),
                                ProBtnStyle::Secondary,
                                true,
                                cx,
                                |this, window, cx| this.bind_active_selection_to_chat(window, cx),
                            ))
                            .child(pro_btn(
                                "btn-open-quick-edit",
                                "精确编辑",
                                Some(ProIcon::Sliders),
                                ProBtnStyle::Ghost,
                                true,
                                cx,
                                |this, window, cx| this.open_quick_edit_modal(window, cx),
                            ))
                            .when(!is_demo, |d| {
                                d.child(pro_btn(
                                    "btn-office-save",
                                    "保存",
                                    None,
                                    ProBtnStyle::Ghost,
                                    true,
                                    cx,
                                    |this, window, cx| {
                                        if let Some(i) = this.active_office_tab
                                            && let Some(t) = this.office_tabs.get(i).cloned()
                                        {
                                            let fname = clean_display_doc_name(&t.doc_name(cx));
                                            match t.save_in_place(window, cx) {
                                                Ok(_) => {
                                                    this.office_notice =
                                                        Some(format!("已保存 · {fname}"));
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
                                    "保存重开核验",
                                    Some(ProIcon::Refresh),
                                    ProBtnStyle::Secondary,
                                    true,
                                    cx,
                                    |this, window, cx| {
                                        this.save_close_and_reopen_active_tab(window, cx);
                                    },
                                ))
                            })
                            .when(is_demo, |d| {
                                d.child(pro_btn(
                                    "btn-toggle-coedit-panel",
                                    if self.coedit_panel_open {
                                        "收起裁决面板"
                                    } else {
                                        "展开裁决面板"
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
                            }),
                    );

                let _ = window;
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(ribbon)
                    .when(self.coedit_panel_open, |d| {
                        d.child(self.render_coedit_inspector(&p, cx))
                    })
                    .child(div().flex_1().min_h_0().overflow_hidden().child(tab.any_view()))
                    .into_any_element()
            }
        };

        div()
            .size_full()
            .bg(p.page)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
            .into_any_element()
    }

    fn render_empty_office_launcher(&self, p: &ProPalette, cx: &mut Context<Self>) -> AnyElement {
        let items: [(OfficeKind, &'static str, &'static str, &'static str); 3] = [
            (
                OfficeKind::Word,
                "sample_minutes.docx",
                "会议纪要样本 (.docx)",
                "原生段落排版、样式编辑与选区绑定",
            ),
            (
                OfficeKind::Excel,
                "sample_formulas.xlsx",
                "季度公式预算表 (.xlsx)",
                "单元格公式实时重算与跨表定位",
            ),
            (
                OfficeKind::Ppt,
                "sample_deck.pptx",
                "产品发布演示文稿 (.pptx)",
                "幻灯片对象坐标拖拽与文本框编辑",
            ),
        ];

        let hover_c = p.hover;
        let card_bg = p.card;
        let line_c = p.line;
        let line_soft = p.line_soft;

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
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.ink)
                                    .child("在同窗内打开原生 Kantor Office 文档"),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(20.))
                                    .text_color(p.muted)
                                    .child("直接加载测试副本进行编辑、保存副本与重开验证，或将任意选区一键关联至左侧 Kar 会话。"),
                            ),
                    )
                    .child(
                        div()
                            .rounded(px(11.))
                            .bg(card_bg)
                            .border_1()
                            .border_color(line_c)
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
                                            .px(px(14.))
                                            .py(px(11.))
                                            .when(idx > 0, |d| {
                                                d.border_t_1().border_color(line_soft)
                                            })
                                            .hover(move |s| s.bg(hover_c))
                                            .flex()
                                            .items_center()
                                            .gap(px(12.))
                                            .cursor_pointer()
                                            .child(render_office_chip(kind, false, p))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(1.))
                                                    .child(
                                                        div()
                                                            .text_size(px(13.))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(p.ink)
                                                            .child(title),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .text_color(p.muted)
                                                            .child(desc),
                                                    ),
                                            )
                                            .child(pro_icon(ProIcon::ChevronRight, 13., p.faint)),
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
        p: &ProPalette,
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

        // Segmented Scenario Selector with spring-gliding pill
        let sc_sensor = pro_selection_sensor();
        let sc_key: SharedString = sc.as_str().into();
        let scenarios: [(ScenarioKey, &'static str); 3] = [
            (ScenarioKey::WordSameParagraph, "Word 同段双写"),
            (ScenarioKey::ExcelSameCellConflict, "Excel 同格冲突"),
            (ScenarioKey::PptMoveAndText, "PPT 移位与删除"),
        ];
        let hover_c = p.hover;
        let segmented = div()
            .relative()
            .h(px(30.))
            .p(px(3.))
            .rounded(px(8.))
            .bg(p.tray)
            .border_1()
            .border_color(p.line)
            .flex()
            .items_center()
            .gap(px(2.))
            .child(pro_pill_indicator(
                "coedit-scenario-pill",
                sc_key,
                sc_sensor.clone(),
                6.,
                p.card,
                Some(p.line),
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
                        .when(!active, |d| d.hover(move |s| s.bg(hover_c)))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .font_weight(if active {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(if active { p.ink } else { p.muted })
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
                        .text_color(p.muted)
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
                        .text_color(p.muted)
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
                        .text_color(p.muted)
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
                p,
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
                p,
            ))
            .child(render_diff_card(
                third_title,
                &format!("Op {short_op}"),
                &third_val,
                third_tone,
                true,
                p,
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
                .rounded(px(9.))
                .bg(p.amber_soft)
                .border_1()
                .border_color(p.amber.opacity(0.32))
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
                        .text_color(p.amber_ink)
                        .child(pro_icon(ProIcon::AlertTriangle, 14., p.amber_ink))
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
                        .text_color(p.text)
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
                        .text_color(p.amber_ink)
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
                                        .bg(p.panel)
                                        .border_1()
                                        .border_color(p.line_strong)
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
                .rounded(px(9.))
                .bg(p.red_soft)
                .border_1()
                .border_color(p.red.opacity(0.28))
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
                        .text_color(p.red_ink)
                        .child(pro_icon(ProIcon::AlertTriangle, 14., p.red_ink))
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
                .rounded(px(8.))
                .bg(p.green_soft)
                .border_1()
                .border_color(p.green.opacity(0.26))
                .flex()
                .items_center()
                .gap(px(7.))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.green_ink)
                .child(pro_icon(ProIcon::Check, 13.5, p.green_ink))
                .child(div().flex_1().min_w_0().child(summary)),
        };

        div()
            .id("coedit-inspector-panel")
            .max_h(px(330.))
            .overflow_y_scroll()
            .px(px(14.))
            .py(px(12.))
            .border_b_1()
            .border_color(p.line_strong)
            .bg(p.panel)
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(10.))
            // Row 1: Title + Demo Pill + Scenario Switcher + Reset
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
                            .child(render_demo_pill("演示数据 · 隔离沙盒", p))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.ink)
                                    .child("人机双写协议裁决"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(p.muted)
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
            // Row 2: Simulation Triggers
            .child(sim_row)
            // Row 3: 3-Column Diff Cards
            .child(diff_grid)
            // Row 4: Decision / Reconfirmation Bar
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
    p: &ProPalette,
) -> Div {
    let (bg, border) = if emphasized {
        match tone {
            DotTone::Success => (p.green_soft, p.green.opacity(0.28)),
            DotTone::Warning => (p.amber_soft, p.amber.opacity(0.28)),
            DotTone::Danger => (p.red_soft, p.red.opacity(0.28)),
            _ => (p.card, p.line_strong),
        }
    } else {
        (p.subtle, p.line)
    };

    div()
        .flex_1()
        .min_w_0()
        .p(px(9.))
        .rounded(px(8.5))
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
                        .text_color(tone.text_color(p))
                        .child(
                            div()
                                .size(px(5.5))
                                .rounded_full()
                                .bg(tone.dot_color(p)),
                        )
                        .child(heading),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(p.faint)
                        .child(meta.to_owned()),
                ),
        )
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(17.5))
                .text_color(p.ink)
                .child(if value.is_empty() {
                    "(无内容 / 对象已移除)".to_owned()
                } else {
                    value.to_owned()
                }),
        )
}
