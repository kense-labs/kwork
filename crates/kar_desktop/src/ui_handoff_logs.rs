//! Cross-Runtime Session Handoff (`AgentHandoff`), Zero-Pollution Side Question (`旁路快问`),
//! and Multi-Source Diagnostics Explorer (`LogStream`).

use gpui::{prelude::*, *};
use kui::{
    components::{
        agent_handoff::{AgentHandoff, HandoffAuthor, HandoffMessage, HandoffSummary},
        log_stream::{LogEntry, LogLevel, LogSource, LogStream},
    },
    primitives::pro_icons::ProIcon,
};

use crate::{
    kar_bridge::KarCommand,
    studio_hub::DiagLevel,
    types::runtime_display_name,
    ui_motion::{Tactile, icon_button, primary_button, soft_button, text_button},
    ui_primitives::{
        FS_META, FS_TAG, FS_TITLE, LH_META, R_CARD, R_CTRL, R_PILL, StudioPalette,
        render_handoff_relay_icon, render_runtime_mark, render_side_question_icon, tnum_features,
    },
    workbench::KarWorkbench,
};

impl KarWorkbench {
    // ═══════════════════════════════════════════════════════════════════════
    // 1. Cross-Runtime Session Handoff Sheet (`AgentHandoff`)
    // ═══════════════════════════════════════════════════════════════════════

    pub(crate) fn ensure_handoff_widget(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cur_rt = self.active_or_selected_runtime_id().to_owned();
        let turns_len = self.projected.turns.len();
        let mut summary = HandoffSummary::new(format!(
            "当前会话包含 {turns_len} 轮对话记录，将打包上下文平滑移交至目标智能体"
        ));
        summary.fields = vec![
            ("当前智能体".into(), runtime_display_name(&cur_rt).into()),
            (
                "目标智能体".into(),
                runtime_display_name(&self.handoff_target_runtime).into(),
            ),
            (
                "上下文预算".into(),
                format!("{} KB", self.handoff_budget_kb).into(),
            ),
        ];

        if let Some(ref hw) = self.handoff_widget {
            hw.update(cx, |w, cx| w.set_summary(summary, cx));
        } else {
            let mut preview_msgs = Vec::new();
            for (idx, t) in self.projected.turns.iter().rev().take(3).rev().enumerate() {
                if let Some(kar::chat::Item::Message { text, .. }) =
                    t.items.first().map(|i| &i.item)
                {
                    preview_msgs.push(HandoffMessage::new(
                        format!("hm-u-{idx}"),
                        HandoffAuthor::Customer,
                        crate::studio_hub::truncate_chars(text.trim(), 120),
                    ));
                }
            }
            let target_disp = runtime_display_name(&self.handoff_target_runtime);
            self.handoff_widget = Some(cx.new(|cx| {
                let mut view = AgentHandoff::new(
                    "上下文便携打包预览",
                    "无缝跨供应商切换：无需从零复述前序结论与右侧文档状态",
                    summary,
                    window,
                    cx,
                )
                .with_assistant_model(runtime_display_name(&cur_rt))
                .estimate("即时移交 · 保留历史记录");
                view.set_messages(preview_msgs, cx);
                view.request_human(cx);
                let _ = view.connect(1, target_disp, cx);
                view.set_resolved(true, cx);
                view
            }));
        }
    }

    pub(crate) fn render_handoff_modal(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let panel = if self.handoff_sheet_open {
            self.ensure_handoff_widget(window, cx);
            self.render_handoff_panel(s, cx)
        } else {
            div().into_any_element()
        };
        let card = div()
            .w(px(600.))
            .max_w_full()
            .h(px((f32::from(window.viewport_size().height) - 78.).clamp(1., 700.)))
            .rounded(px(R_CARD))
            .bg(s.card)
            .border_1()
            .border_color(s.line)
            .shadow(s.popover_shadow())
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(panel)
            .into_any_element();
        self.render_modal(
            "kar-handoff-sheet",
            self.handoff_sheet_open,
            54.,
            card,
            |this, _, _| this.handoff_sheet_open = false,
            cx,
        )
    }

    fn render_handoff_panel(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let current_rt = self.active_or_selected_runtime_id().to_owned();
        let candidates = ["claude", "codex", "cursor", "antigravity", "opencode", "grok", "kimi"];

        let header = div()
            .pl(px(20.))
            .pr(px(14.))
            .pt(px(16.))
            .pb(px(12.))
            .border_b_1()
            .border_color(s.line_soft)
            .flex()
            .items_start()
            .justify_between()
            .gap(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .child(
                        div()
                            .size(px(34.))
                            .flex_none()
                            .rounded(px(9.))
                            .bg(s.pro.accent_soft)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(render_handoff_relay_icon(18., s.pro.accent_info)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(FS_TITLE))
                                    .line_height(px(22.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child("交接对话"),
                            )
                            .child(
                                div()
                                    .text_size(px(FS_META))
                                    .line_height(px(LH_META))
                                    .text_color(s.muted)
                                    .child("例如先用 Codex 完成代码重构，再一键将上下文移交 Claude Code 或 Antigravity 撰写架构复盘文档。"),
                            ),
                    ),
            )
            .child(
                icon_button("kar-handoff-close", ProIcon::Close, "关闭  Esc", s).on_press(
                    cx,
                    |this, _, cx| {
                        this.handoff_sheet_open = false;
                        cx.notify();
                    },
                ),
            );

        // Target runtime cards
        let mut rt_grid = div().w_full().flex().flex_wrap().gap(px(8.));
        for (ix, &rt_id) in candidates.iter().enumerate() {
            let selected = self.handoff_target_runtime == rt_id;
            let is_current = current_rt == rt_id;
            let ready = self
                .providers
                .iter()
                .find(|p| p.id == rt_id)
                .is_some_and(|p| p.is_ready());
            let rt_owned = rt_id.to_owned();
            rt_grid = rt_grid.child(
                Tactile::ghost(("handoff-rt-opt", ix), s)
                    .lit(selected)
                    .h(px(36.))
                    .px(px(10.))
                    .radius(R_CTRL)
                    .gap(px(7.))
                    .border_1()
                    .border_color(if selected {
                        s.pro.accent_info
                    } else {
                        s.line_soft
                    })
                    .child(render_runtime_mark(rt_id, 16., s, cx))
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .font_weight(if selected {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(s.ink)
                            .child(runtime_display_name(rt_id)),
                    )
                    .when(is_current, |b| {
                        b.child(
                            div()
                                .text_size(px(FS_TAG))
                                .text_color(s.muted)
                                .child("当前"),
                        )
                    })
                    .when(ready && !is_current, |b| {
                        b.child(div().size(px(6.)).rounded_full().bg(s.pro.green))
                    })
                    .on_press(cx, move |this, _, cx| {
                        this.handoff_target_runtime = rt_owned.clone();
                        cx.notify();
                    }),
            );
        }

        // Context byte budget pills (`16 KB` / `64 KB` / `128 KB`)
        let mut budget_row = div().flex().items_center().gap(px(6.));
        for &kb in &[16usize, 64, 128] {
            let active = self.handoff_budget_kb == kb;
            budget_row = budget_row.child(
                Tactile::ghost(("handoff-budget", kb), s)
                    .lit(active)
                    .h(px(26.))
                    .px(px(9.))
                    .radius(R_PILL)
                    .font_features(tnum_features())
                    .text_size(px(FS_TAG))
                    .text_color(if active { s.ink } else { s.muted })
                    .child(format!("{kb} KB 上下文窗口"))
                    .on_press(cx, move |this, _, cx| {
                        this.handoff_budget_kb = kb;
                        cx.notify();
                    }),
            );
        }

        let mut body = div()
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child("1. 选择接手会话的目标智能体"),
                    )
                    .child(rt_grid),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child("2. 历史摘要携带体积上限"),
                    )
                    .child(budget_row),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child("3. 接力首条交办说明（可选）"),
                    )
                    .child(self.handoff_note_input.clone()),
            );

        if let Some(ref hw) = self.handoff_widget {
            body = body.child(div().w_full().child(hw.clone()));
        }

        let footer = div()
            .w_full()
            .px(px(20.))
            .py(px(12.))
            .border_t_1()
            .border_color(s.line_soft)
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(FS_TAG))
                    .text_color(s.muted)
                    .child("接力将在新会话首轮注入便携摘要，原会话历史完整保留"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        text_button("handoff-cancel-btn", "取消", s).on_press(
                            cx,
                            |this, _, cx| {
                                this.handoff_sheet_open = false;
                                cx.notify();
                            },
                        ),
                    )
                    .child(
                        primary_button(
                            "handoff-confirm-btn",
                            format!(
                                "立即接力至 {}",
                                runtime_display_name(&self.handoff_target_runtime)
                            ),
                            s,
                        )
                        .h(px(34.))
                        .px(px(16.))
                        .on_press(cx, |this, win, cx| {
                            let note =
                                this.handoff_note_input.read(cx).value(cx).trim().to_owned();
                            this.handoff_note_input
                                .update(cx, |inp, cx| inp.set_value("", win, cx));
                            this.handoff_sheet_open = false;
                            this.bridge.send(KarCommand::HandoffChat {
                                target_runtime: this.handoff_target_runtime.clone(),
                                target_model: None,
                                user_note: note,
                                budget_bytes: this.handoff_budget_kb * 1024,
                                workspace: this.config.workspace_dir.clone(),
                            });
                            this.notify_toast(
                                format!(
                                    "正在打包会话上下文并移交至 {}…",
                                    runtime_display_name(&this.handoff_target_runtime)
                                ),
                                cx,
                            );
                            cx.notify();
                        }),
                    ),
            );

        div()
            .size_full()
            .max_h(px(700.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header.flex_none())
            .child(
                div()
                    .id("kar-handoff-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
            .child(footer)
            .into_any_element()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 2. Zero-Pollution Ephemeral Side Question Modal (`旁路快问`)
    // ═══════════════════════════════════════════════════════════════════════

    pub(crate) fn render_side_question_modal(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let panel = if self.side_question_open {
            self.render_side_question_panel(s, cx)
        } else {
            div().into_any_element()
        };
        let card = div()
            .w(px(500.))
            .max_w_full()
            .max_h(px((f32::from(window.viewport_size().height) - 92.).clamp(1., 560.)))
            .rounded(px(R_CARD))
            .bg(s.card)
            .border_1()
            .border_color(s.line)
            .shadow(s.popover_shadow())
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(panel)
            .into_any_element();
        self.render_modal(
            "kar-side-question-sheet",
            self.side_question_open,
            68.,
            card,
            |this, _, _| this.side_question_open = false,
            cx,
        )
    }

    fn render_side_question_panel(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let header = div()
            .pl(px(18.))
            .pr(px(12.))
            .pt(px(14.))
            .pb(px(10.))
            .border_b_1()
            .border_color(s.line_soft)
            .flex()
            .items_start()
            .justify_between()
            .gap(px(10.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .child(
                        div()
                            .size(px(32.))
                            .flex_none()
                            .rounded(px(8.))
                            .bg(s.pro.accent_soft)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(render_side_question_icon(17., s.pro.accent_info)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(FS_TITLE - 1.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.ink)
                                            .child("旁路快问（Side Question）"),
                                    )
                                    .child(
                                        div()
                                            .px(px(6.))
                                            .py(px(1.))
                                            .rounded(px(R_PILL))
                                            .bg(s.pro.green_soft)
                                            .text_size(px(FS_TAG))
                                            .text_color(s.pro.green_ink)
                                            .child("零主上下文污染"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(FS_META))
                                    .line_height(px(LH_META))
                                    .text_color(s.muted)
                                    .child("在临时只读分叉中快速核对概念、API 或历史细节，绝不写入主会话历史或触发工具副作用。"),
                            ),
                    ),
            )
            .child(
                icon_button("kar-side-q-close", ProIcon::Close, "关闭  Esc", s).on_press(
                    cx,
                    |this, _, cx| {
                        this.side_question_open = false;
                        cx.notify();
                    },
                ),
            );

        let mut body = div().p(px(18.)).flex().flex_col().gap(px(12.));
        body = body.child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().min_w_0().child(self.side_question_input.clone()))
                .child(
                    primary_button(
                        "side-q-submit",
                        if self.side_question_pending {
                            "查询中…"
                        } else {
                            "快问"
                        },
                        s,
                    )
                    .disabled(self.side_question_pending)
                    .h(px(36.))
                    .px(px(14.))
                    .on_press(cx, |this, _, cx| {
                        this.submit_side_question(cx);
                    }),
                ),
        );

        if let Some((ref q, ref ans, via_fork)) = self.side_question_last_reply {
            let q_for_promote = q.clone();
            body = body.child(
                div()
                    .w_full()
                    .p(px(12.))
                    .rounded(px(R_CARD))
                    .bg(s.surface)
                    .border_1()
                    .border_color(s.line_soft)
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(FS_TAG))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if via_fork {
                                        s.pro.green_ink
                                    } else {
                                        s.pro.accent_info
                                    })
                                    .child(if via_fork {
                                        "Claude 原生无工具分叉回复"
                                    } else {
                                        "本地会话只读线索摘要"
                                    }),
                            )
                            .child(
                                soft_button("side-q-promote", "转入主对话深究", s)
                                    .on_press(cx, move |this, win, cx| {
                                        this.side_question_open = false;
                                        this.composer_editor.update(cx, |ed, cx| {
                                            ed.set_value(q_for_promote.clone(), win, cx)
                                        });
                                        let focus = this.composer_editor.read(cx).focus_handle(cx);
                                        win.focus(&focus, cx);
                                        cx.notify();
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .line_height(px(LH_META))
                            .text_color(s.ink)
                            .child(ans.clone()),
                    ),
            );
        }

        div()
            .w_full()
            .flex()
            .flex_col()
            .child(header)
            .child(body)
            .into_any_element()
    }

    pub fn submit_side_question(&mut self, cx: &mut Context<Self>) {
        let q = self.side_question_input.read(cx).value(cx).trim().to_owned();
        if q.is_empty() {
            self.notify_warning("请输入你想快速核实的问题", cx);
            return;
        }
        self.side_question_pending = true;
        let rt_id = self.active_or_selected_runtime_id().to_owned();
        let native_id = self.projected.info.as_ref().map(|i| i.native_id.clone());
        let mut excerpt_lines = Vec::new();
        for t in self.projected.turns.iter().rev().take(3).rev() {
            for it in &t.items {
                if let kar::chat::Item::Message { role, text, .. } = &it.item {
                    let prefix = match role {
                        kar::chat::Role::User => "用户",
                        kar::chat::Role::Assistant => "智能体",
                        _ => "消息",
                    };
                    excerpt_lines.push(format!(
                        "- {prefix}: {}",
                        crate::studio_hub::truncate_chars(text.trim(), 160)
                    ));
                }
            }
        }
        self.bridge.send(KarCommand::AskSideQuestion {
            runtime_id: rt_id,
            native_session_id: native_id,
            workspace: self.config.workspace_dir.clone(),
            question: q,
            context_excerpt: excerpt_lines.join("\n"),
        });
        cx.notify();
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 3. Multi-Source Diagnostics Explorer Modal (`LogStream`)
    // ═══════════════════════════════════════════════════════════════════════

    pub(crate) fn sync_log_stream_widget(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let records = self.diag_logs.records();
        let mut source_counts: std::collections::BTreeMap<String, (usize, u64)> =
            std::collections::BTreeMap::new();
        let entries: Vec<LogEntry> = records
            .into_iter()
            .map(|r| {
                let sc = source_counts.entry(r.source.clone()).or_insert((0, 0));
                sc.0 += 1;
                sc.1 += r.message.len() as u64;
                let lvl = match r.level {
                    DiagLevel::Debug => LogLevel::Debug,
                    DiagLevel::Info => LogLevel::Info,
                    DiagLevel::Warn => LogLevel::Warn,
                    DiagLevel::Error => LogLevel::Error,
                };
                let mut e = LogEntry {
                    id: r.id.into(),
                    timestamp: r.timestamp.into(),
                    level: lvl,
                    source: r.source.into(),
                    tag: r.tag.map(Into::into),
                    message: r.message.into(),
                    fields: Vec::new(),
                };
                for (k, v) in r.fields {
                    e = e.with_field(k, v);
                }
                e
            })
            .collect();

        let sources: Vec<LogSource> = source_counts
            .into_iter()
            .map(|(src, (cnt, bytes))| {
                let layer = match src.as_str() {
                    "git" => "kar::workbench_git",
                    "kar_office" => "In-Process MCP",
                    "inventory" => "kar::inventory",
                    "handoff" => "kar::chat::handoff",
                    "remote_link" => "HMAC Link",
                    _ => "GPUI Host",
                };
                LogSource::new(src.clone(), src, layer)
                    .with_count(cnt)
                    .with_bytes(bytes)
            })
            .collect();

        if let Some(ref ls) = self.log_stream_widget {
            ls.update(cx, |stream, cx| {
                stream.set_sources(sources, cx);
                stream.set_entries(entries, cx);
            });
        } else {
            self.log_stream_widget = Some(cx.new(|cx| {
                LogStream::new(
                    "kar-studio-log-stream",
                    "工作台全链路结构化事件流",
                    entries,
                    window,
                    cx,
                )
                .with_sources(sources)
            }));
        }
    }

    pub(crate) fn render_logs_panel(&self) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(14.))
                    .when_some(self.log_stream_widget.clone(), |d, ls| d.child(ls)),
            )
            .into_any_element()
    }
}
