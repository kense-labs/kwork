//! Conversation handoff, side questions, and diagnostics.
use crate::ui_motion::selection::{Group, Item, SelectionStrip};
use crate::{
    kar_bridge::KarCommand,
    studio_hub::DiagLevel,
    types::{ChatAccessMode, runtime_display_name},
    ui_motion::{Tactile, icon_button, primary_button, soft_button},
    ui_primitives::{FS_META, FS_TITLE, FS_UI, R_MODAL, StudioPalette, render_runtime_mark},
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
                p.id != current && !(crate::aster::is_aster(&p.id) && crate::aster::is_aster(&current))
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
            && (!self.turn_in_progress() || self.hosted_writer.is_some());
        let choices = candidates
            .iter()
            .map(|provider| {
                let id = provider.id.clone();
                let label = provider.display_name();
                let mark = render_runtime_mark(&id, 16., s, cx);
                let selected = target.as_ref() == Some(&id);
                Item::new(id.clone(), label.clone(), selected, cx, move |wb, _, cx| {
                    wb.handoff_target_runtime = id.clone();
                    cx.notify();
                })
                .content(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .child(mark)
                        .child(label),
                )
            })
            .collect();
        let choices = SelectionStrip::new(
            "handoff-targets",
            loc.pick("接手智能体", "Receiving agent"),
            Group::Choices,
            choices,
            s,
        );
        let budgets = SelectionStrip::options(
            "handoff-budget",
            loc.pick("上下文预算", "Context budget"),
            Group::Choices,
            [16usize, 64, 128].map(|kb| (kb, format!("{kb} KB"))),
            self.handoff_budget_kb,
            s,
            cx,
            |wb, kb, _, cx| {
                wb.handoff_budget_kb = kb;
                cx.notify();
            },
        );

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
            loc.pick("在同一段对话中切换智能体。Kar 保留历史，并在下一条消息中附上交接摘要。", "Switch the agent in this thread. Kar preserves its history and attaches the handoff context to the next message.")
        };
        let fields = div()
            .flex()
            .flex_col()
            .gap(px(9.))
            .child(sheet_text(description, s))
            .child(sheet_text(loc.pick("接手智能体", "Receiving agent"), s))
            .when(!candidates.is_empty(), |d| d.child(choices))
            .child(sheet_text(
                loc.pick("摘要大小上限", "Summary size limit"),
                s,
            ))
            .child(budgets)
            .when(!self.turn_in_progress(),|fields| fields.child(self.handoff_note_input.clone()))
            .when_some(self.handoff_receipt.as_ref().filter(|(id,_)|Some(*id)==self.current_chat_id()),|fields,(_,receipt)| {
                let status = match receipt.status {
                    kar::chat::HandoffStatus::Ready=>loc.pick("已交接","Handed off"),
                    kar::chat::HandoffStatus::Failed=>loc.pick("交接失败","Handoff failed"),
                    kar::chat::HandoffStatus::Prepared=>loc.pick("正在交接","Handing off"),
                };
                let route=format!("{} → {}",runtime_display_name(&receipt.from.runtime),runtime_display_name(&receipt.request.runtime));
                fields.child(sheet_text(format!("{status} · {route}"),s))
                    .when_some(receipt.error.as_ref(),|fields,error|fields.child(sheet_text(error.clone(),s)))
            });
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
        let submit = primary_button("handoff-submit", if self.turn_in_progress() { loc.pick("回合结束后交接", "Hand off after this turn") } else { loc.pick("交接对话", "Hand off thread") }, s)
            .disabled(!eligible || target.is_none())
            .tip(format!(
                "{} {target_name}",
                loc.pick("接手方", "Receiving agent")
            ))
            .on_press(cx, move |this, _, cx| {
                let Some(id) = target.clone() else { return };
                let note = if this.turn_in_progress() { String::new() } else { this.handoff_note_input.read(cx).value(cx).trim().to_owned() };
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
                });
                cx.notify();
            });
        let card = crate::ui_primitives::frosted_panel(s)
            .w(px(568.))
            .max_w_full()
            .h(px(
                (f32::from(window.viewport_size().height) - 60.).clamp(1., 480.)
            ))
            .rounded(px(R_MODAL))
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
    pub(crate) fn supports_side_question(&self) -> bool {
        let Some(info)=self.projected.info.as_ref() else { return false; };
        matches!(self.access_mode,ChatAccessMode::WriteController { chat_id } if chat_id==info.id)
            && info.protocol==kar::Protocol::Claude
            && !self.projected.turns.is_empty()
            && self.side_question_capability.as_ref().is_some_and(|(chat,native)|*chat==info.id && native==&info.native_id)
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
        let description = loc.pick("Claude 会在当前对话的临时分支中回答，主对话保留原样。","Claude answers in a temporary fork of this conversation.");
        let mut body = div()
            .flex()
            .flex_col()
            .p(px(16.))
            .gap(px(10.))
            .child(sheet_text(description, s))
            .child(self.side_question_input.clone())
            .child(div().flex().justify_end().child(submit));
        if let Some((q, ans)) = &self.side_question_last_reply {
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
                        loc.pick("独立回复", "Private reply"),
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
        let card = crate::ui_primitives::frosted_panel(s)
            .w(px(500.))
            .max_w_full()
            .max_h(px(cap))
            .rounded(px(R_MODAL))
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
            self.side_question_open && self.supports_side_question(),
            30.,
            card.into_any_element(),
            |this, _, _| this.side_question_open = false,
            cx,
        )
    }

    pub fn submit_side_question(&mut self, cx: &mut Context<Self>) {
        if self.side_question_pending || !self.supports_side_question() { return; }
        let Some(chat_id)=self.current_chat_id() else {return};
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
        self.bridge.send(KarCommand::AskSideQuestion{request_id,chat_id,question:q});
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
