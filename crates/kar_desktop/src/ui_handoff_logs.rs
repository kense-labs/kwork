//! Conversation handoff, side questions, and diagnostics.
use crate::{
    kar_bridge::KarCommand,
    studio_hub::DiagLevel,
    types::{ChatAccessMode, runtime_display_name},
    ui_motion::{Tactile, icon_button, primary_button, soft_button},
    ui_primitives::{FS_META, FS_TITLE, FS_UI, R_CARD, StudioPalette, render_runtime_mark},
    workbench::KarWorkbench,
};
use gpui::{prelude::*, *};
use kui::{
    components::log_stream::{LogEntry, LogLevel, LogSource, LogStream},
    primitives::pro_icons::ProIcon,
};

fn sheet_header(title: &'static str, close: Tactile, s: &StudioPalette) -> Div {
    div()
        .flex_none()
        .px(px(16.))
        .h(px(46.))
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(s.line_soft)
        .child(
            div()
                .text_size(px(FS_TITLE))
                .font_weight(FontWeight::MEDIUM)
                .text_color(s.ink)
                .child(title),
        )
        .child(close)
}
fn sheet_text(text: impl Into<SharedString>, s: &StudioPalette) -> Div {
    div()
        .text_size(px(FS_META))
        .line_height(px(19.))
        .text_color(s.muted)
        .child(text.into())
}

impl KarWorkbench {
    pub(crate) fn render_handoff_modal(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let current = self.active_or_selected_runtime_id().to_owned();
        let candidates: Vec<_> = self
            .providers
            .iter()
            .filter(|p| {
                p.id != current
                    && (p.is_ready() || matches!(self.access_mode, ChatAccessMode::DemoShowcase))
            })
            .collect();
        let target = candidates
            .iter()
            .find(|p| p.id == self.handoff_target_runtime)
            .or_else(|| candidates.first())
            .map(|p| p.id.clone());
        let target_name = target
            .as_deref()
            .map(runtime_display_name)
            .unwrap_or_default();
        let eligible = matches!(
            self.access_mode,
            ChatAccessMode::WriteController { .. } | ChatAccessMode::DemoShowcase
        ) && !self.projected.turns.is_empty()
            && !self.turn_in_progress();
        let mut choices = div().flex().flex_wrap().gap(px(6.));
        for (idx, provider) in candidates.iter().enumerate() {
            let id = provider.id.clone();
            choices = choices.child(
                soft_button(("handoff-target", idx), provider.display_name(), s)
                    .lit(target.as_deref() == Some(id.as_str()))
                    .child(render_runtime_mark(&id, 16., s, cx))
                    .on_press(cx, move |this, _, cx| {
                        this.handoff_target_runtime = id.clone();
                        cx.notify();
                    }),
            );
        }
        let mut budgets = div().flex().flex_wrap().gap(px(6.));
        for kb in [16usize, 64, 128] {
            budgets = budgets.child(
                soft_button(("handoff-budget", kb), format!("{kb} KB"), s)
                    .lit(self.handoff_budget_kb == kb)
                    .on_press(cx, move |this, _, cx| {
                        this.handoff_budget_kb = kb;
                        cx.notify();
                    }),
            );
        }
        let close = icon_button(
            "handoff-close",
            ProIcon::Close,
            loc.pick("关闭", "Close"),
            s,
        )
        .on_press(cx, |this, _, cx| {
            this.handoff_sheet_open = false;
            cx.notify();
        });
        let description = if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            loc.pick("演示模式会模拟交接。", "Demo mode simulates the handoff.")
        } else {
            loc.pick("将最近对话的摘要和交接说明发给另一个智能体。原对话会保留。", "Send a summary of this thread and your note to another agent. Your original thread stays available.")
        };
        let fields = div()
            .flex()
            .flex_col()
            .gap(px(9.))
            .child(sheet_text(description, s))
            .child(sheet_text(loc.pick("接手智能体", "Receiving agent"), s))
            .child(choices)
            .child(sheet_text(
                loc.pick("摘要大小上限", "Summary size limit"),
                s,
            ))
            .child(budgets)
            .child(self.handoff_note_input.clone());
        let mut body = div()
            .id("handoff-content")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(16.))
            .child(fields);
        if !eligible || target.is_none() {
            let explanation = if !eligible {
                loc.pick(
                    "打开有已完成回合的本地可写对话后即可交接。",
                    "Open a writable local conversation with a completed turn to hand it off.",
                )
            } else {
                loc.pick(
                    "需要另一个可用的智能体。可在设置中查看智能体状态。",
                    "Another agent must be available. Check Providers in Settings.",
                )
            };
            body = body.child(div().pt(px(12.)).child(sheet_text(explanation, s)));
        }
        let submit = primary_button("handoff-submit", loc.pick("交接对话", "Hand off thread"), s)
            .disabled(!eligible || target.is_none())
            .tip(format!(
                "{} {target_name}",
                loc.pick("接手方", "Receiving agent")
            ))
            .on_press(cx, move |this, _, cx| {
                let Some(id) = target.clone() else { return };
                let note = this.handoff_note_input.read(cx).value(cx).trim().to_owned();
                if note.len() > this.handoff_budget_kb * 1024 - 512 {
                    this.notify_warning(
                        this.ui_locale.pick(
                            "交接说明超出摘要预算，请缩短说明或调大上限。",
                            "Shorten the note or increase the summary limit.",
                        ),
                        cx,
                    );
                    return;
                }
                this.handoff_sheet_open = false;
                this.bridge.send(KarCommand::HandoffChat {
                    source_chat: this.current_chat_id(),
                    target_runtime: id,
                    target_model: None,
                    user_note: note,
                    budget_bytes: this.handoff_budget_kb * 1024,
                    workspace: this.effective_workspace_dir(),
                });
                cx.notify();
            });
        let card = div()
            .w(px(568.))
            .max_w_full()
            .h(px(
                (f32::from(window.viewport_size().height) - 60.).clamp(1., 480.)
            ))
            .rounded(px(R_CARD))
            .bg(s.card)
            .border_1()
            .border_color(s.line)
            .shadow(s.popover_shadow())
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(sheet_header(
                loc.pick("交接对话", "Hand off thread"),
                close,
                s,
            ))
            .child(body)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .justify_end()
                    .px(px(16.))
                    .py(px(9.))
                    .border_t_1()
                    .border_color(s.line_soft)
                    .child(submit),
            );
        self.render_modal(
            "kar-handoff-sheet",
            self.handoff_sheet_open,
            30.,
            card.into_any_element(),
            |this, _, _| this.handoff_sheet_open = false,
            cx,
        )
    }
    pub(crate) fn render_side_question_modal(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let close = icon_button("side-q-close", ProIcon::Close, loc.pick("关闭", "Close"), s)
            .on_press(cx, |this, _, cx| {
                this.side_question_open = false;
                cx.notify();
            });
        let submit_label = if self.side_question_pending {
            loc.pick("查询中…", "Checking…")
        } else {
            loc.pick("查询", "Ask")
        };
        let submit = primary_button("side-q-submit", submit_label, s)
            .disabled(self.side_question_pending)
            .on_press(cx, |this, _, cx| this.submit_side_question(cx));
        let description = loc.pick("Claude 可在独立分支回答；其他智能体显示本对话的最近摘录。",
            "Claude can answer in a separate session. Other agents show recent excerpts from this conversation.");
        let mut body = div()
            .flex()
            .flex_col()
            .p(px(16.))
            .gap(px(10.))
            .child(sheet_text(description, s))
            .child(self.side_question_input.clone())
            .child(div().flex().justify_end().child(submit));
        if let Some((q, ans, fork)) = &self.side_question_last_reply {
            let q = q.clone();
            body = body.child(
                div()
                    .border_t_1()
                    .border_color(s.line_soft)
                    .pt(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(sheet_text(
                        if *fork {
                            loc.pick("独立回复", "Private reply")
                        } else {
                            loc.pick("本地对话摘录", "Local conversation excerpts")
                        },
                        s,
                    ))
                    .child(
                        div()
                            .text_size(px(FS_UI))
                            .text_color(s.ink)
                            .child(ans.clone()),
                    )
                    .when(
                        self.access_mode.is_writable()
                            || matches!(self.access_mode, ChatAccessMode::NoneSelected),
                        |row| {
                            row.child(
                                div().flex().justify_end().child(
                                    soft_button(
                                        "side-q-promote",
                                        loc.pick("添加到对话草稿", "Add to thread draft"),
                                        s,
                                    )
                                    .on_press(
                                        cx,
                                        move |this, win, cx| {
                                            this.reveal_chat_composer(win, cx);
                                            this.composer_editor.update(cx, |ed, cx| {
                                                let prior = ed.value();
                                                let next = if prior.is_empty() {
                                                    q.clone()
                                                } else {
                                                    format!("{prior}\n{q}")
                                                };
                                                ed.set_value(next, win, cx);
                                                ed.focus(win, cx);
                                            });
                                            cx.notify();
                                        },
                                    ),
                                ),
                            )
                        },
                    ),
            );
        }
        let cap = (f32::from(window.viewport_size().height) - 60.).clamp(1., 500.);
        let card = div()
            .w(px(500.))
            .max_w_full()
            .max_h(px(cap))
            .rounded(px(R_CARD))
            .bg(s.card)
            .border_1()
            .border_color(s.line)
            .shadow(s.popover_shadow())
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(sheet_header(
                loc.pick("旁路快问", "Side question"),
                close,
                s,
            ))
            .child(
                div()
                    .id("side-question-scroll")
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            );
        self.render_modal(
            "kar-side-question-sheet",
            self.side_question_open,
            30.,
            card.into_any_element(),
            |this, _, _| this.side_question_open = false,
            cx,
        )
    }

    pub fn submit_side_question(&mut self, cx: &mut Context<Self>) {
        if self.side_question_pending {
            return;
        }
        let q = self
            .side_question_input
            .read(cx)
            .value(cx)
            .trim()
            .to_owned();
        if q.is_empty() {
            self.notify_warning("请输入你想快速核实的问题", cx);
            return;
        }
        self.side_question_pending = true;
        self.side_question_last_reply = None;
        let request_id = uuid::Uuid::new_v4();
        self.side_question_request = Some(request_id);
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
            request_id,
            runtime_id: rt_id,
            native_session_id: native_id,
            workspace: self.effective_workspace_dir(),
            question: q,
            context_excerpt: excerpt_lines.join("\n"),
        });
        cx.notify();
    }

    pub(crate) fn sync_log_stream_widget(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let revision = self.diag_logs.revision();
        if self.log_stream_revision == Some(revision) {
            return;
        }
        self.log_stream_revision = Some(revision);
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
                    self.ui_locale.pick("工作台事件", "Workbench events"),
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
