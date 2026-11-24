//! Editorial Kar Chat Workbench built on KUI `agent_chat` / `agent_thread` patterns & components.

use std::path::Path;

use gpui::{prelude::*, *};
use kar::{
    ChangeKind as ToolChangeKind,
    chat::{ChangeKind as CheckpointChangeKind, ChatTurn, Item, Role as KarRole, TurnStatus},
};
use kui::{
    components::pro_chrome::{ProBtnStyle, ProPalette, pro_btn, pro_fold},
    primitives::{
        control::activate,
        pro_icons::{ProIcon, pro_icon},
    },
};

use crate::{
    kar_bridge::KarCommand,
    types::{ChatAccessMode, FeedConnectionState, OfficeKind},
    ui_primitives::{
        DotTone, HEADER_H, clean_display_doc_name, render_demo_pill, render_keycap,
        render_office_chip, render_runtime_mark, render_status_pill,
    },
    workbench::{KarWorkbench, runtime_display_name},
};

impl KarWorkbench {
    pub(crate) fn render_chat_pane(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = ProPalette::get(cx);

        let rt_id = self
            .projected
            .info
            .as_ref()
            .map(|i| i.runtime.as_str())
            .unwrap_or(&self.selected_runtime);

        let (crumb_main, crumb_sub) = match &self.access_mode {
            ChatAccessMode::DemoShowcase => ("样例对话演示".to_owned(), Some("隔离内存样例".to_owned())),
            ChatAccessMode::HttpReadOnly { url, .. } => ("HTTP 只读观察".to_owned(), Some(url.clone())),
            _ => match &self.projected.info {
                Some(info) => {
                    let ws_name = info
                        .workspace
                        .file_name()
                        .and_then(|s| s.to_str())
                        .map(str::to_owned)
                        .unwrap_or_else(|| info.workspace.display().to_string());
                    (
                        format!("{} · {}", runtime_display_name(&info.runtime), &info.id.to_string()[..8]),
                        Some(ws_name),
                    )
                }
                None => {
                    let ws_name = self
                        .config
                        .workspace_dir
                        .file_name()
                        .and_then(|s| s.to_str())
                        .map(str::to_owned)
                        .unwrap_or_else(|| self.config.workspace_dir.display().to_string());
                    ("新建对话".to_owned(), Some(ws_name))
                }
            },
        };

        let (mode_label, mode_tone) = self.compute_mode_pill();
        let (phase_label, phase_tone) = self.compute_phase_pill();

        // Top 44px Header (exact baseline match with Sidebar & Office headers)
        let header = div()
            .h(px(HEADER_H))
            .px(px(16.))
            .border_b_1()
            .border_color(p.line)
            .bg(p.panel)
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
                    .gap(px(8.))
                    .child(render_runtime_mark(rt_id, 15., &p, cx))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.ink)
                            .child(crumb_main),
                    )
                    .when_some(crumb_sub, |d, sub| {
                        d.child(div().text_size(px(12.5)).text_color(p.faint).child("/"))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.5))
                                    .text_color(p.muted)
                                    .child(sub),
                            )
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(render_status_pill(mode_label, mode_tone, &p))
                    .child(render_status_pill(phase_label, phase_tone, &p))
                    .child(self.chat_actions_menu.clone()),
            );

        // Optional local HTTP/SSE publisher strip
        let http_host_bar = self.active_http_server_url.clone().map(|srv_url| {
            div()
                .px(px(16.))
                .h(px(36.))
                .border_b_1()
                .border_color(p.line_soft)
                .bg(p.subtle)
                .flex_none()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .text_size(px(12.))
                        .text_color(p.text)
                        .child(div().size(px(6.)).rounded_full().bg(p.green))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .child(format!("只读 SSE 端点运行中 · {srv_url}")),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(pro_btn(
                            "btn-spawn-sub-win",
                            "新窗口观察",
                            Some(ProIcon::ExternalArrow),
                            ProBtnStyle::Secondary,
                            true,
                            cx,
                            |this, _, cx| this.open_subscriber_second_window(cx),
                        ))
                        .child(pro_btn(
                            "btn-stop-srv-inline",
                            "停止服务",
                            None,
                            ProBtnStyle::Ghost,
                            true,
                            cx,
                            |this, _, _| this.bridge.send(KarCommand::StopHttpServer),
                        )),
                )
        });

        // Quiet HTTP Replica Read-Only Capsule (`agent_thread.rs` capsule banner style)
        let replica_capsule = match (&self.access_mode, &self.connection_state) {
            (
                ChatAccessMode::HttpReadOnly { .. },
                FeedConnectionState::Connected {
                    cursor,
                    duplicates_ignored,
                    gaps_recovered,
                },
            ) => {
                let cur = short_cursor_label(cursor.as_deref().unwrap_or("-"));
                Some(
                    div()
                        .w_full()
                        .pt(px(10.))
                        .px(px(18.))
                        .flex_none()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .max_w_full()
                                .h(px(30.))
                                .px(px(12.))
                                .rounded_full()
                                .bg(p.chip)
                                .border_1()
                                .border_color(p.line)
                                .overflow_hidden()
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .text_size(px(11.5))
                                .text_color(p.text)
                                .child(pro_icon(ProIcon::Lock, 12., p.muted))
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .child(format!(
                                            "HTTP 只读副本 (GET /snapshot + /events) · 游标 {cur} · 去重 {duplicates_ignored} · 补缺 {gaps_recovered}"
                                        )),
                                ),
                        ),
                )
            }
            _ => None,
        };

        // Contextual KUI Alert (provider diagnostics, disconnects, etc.)
        let alert_banner = self.show_alert.then(|| {
            div()
                .w_full()
                .max_w(px(740.))
                .mx_auto()
                .px(px(22.))
                .pt(px(12.))
                .flex_none()
                .child(self.status_alert.clone())
        });

        // Main scrollable thread body
        let timeline_content = if self.projected.turns.is_empty() {
            self.render_empty_chat_hero(&p, cx)
        } else {
            let turns_snapshot = self.projected.turns.clone();
            let mut stream = div()
                .w_full()
                .max_w(px(740.))
                .mx_auto()
                .px(px(24.))
                .pt(px(20.))
                .pb(px(28.))
                .flex()
                .flex_col()
                .gap(px(28.));
            for (idx, turn) in turns_snapshot.iter().enumerate() {
                stream = stream.child(self.render_turn_editorial(idx + 1, turn, &p, cx));
            }
            stream.into_any_element()
        };

        // Host Permission Approval Dock (when ApprovalMode::Host awaits user decision)
        let approval_dock = (!self.projected.pending_requests.is_empty()
            && self.access_mode.is_writable())
        .then(|| self.render_permission_dock(&p, cx));

        // Unified `agent_chat.rs` Composer Card
        let composer = self.render_composer_card(&p, cx);

        div()
            .size_full()
            .bg(p.panel)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .when_some(http_host_bar, |d, bar| d.child(bar))
            .when_some(replica_capsule, |d, cap| d.child(cap))
            .when_some(alert_banner, |d, al| d.child(al))
            .child(
                div()
                    .id("kar-timeline-scroll")
                    .track_scroll(&self.chat_scroll)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(timeline_content),
            )
            .when_some(approval_dock, |d, dock| d.child(dock))
            .child(composer)
            .into_any_element()
    }

    fn compute_mode_pill(&self) -> (String, DotTone) {
        match &self.access_mode {
            ChatAccessMode::NoneSelected => ("未选会话".into(), DotTone::Neutral),
            ChatAccessMode::WriteController { chat_id } => (
                format!("写入控制器 · {}", &chat_id.to_string()[..8]),
                DotTone::Success,
            ),
            ChatAccessMode::LocalReadOnlySubscriber { chat_id, .. } => (
                format!("只读订阅 · {}", &chat_id.to_string()[..8]),
                DotTone::Warning,
            ),
            ChatAccessMode::HttpReadOnly { .. } => ("HTTP 只读".into(), DotTone::Info),
            ChatAccessMode::DemoShowcase => ("演示数据".into(), DotTone::Warning),
        }
    }

    fn compute_phase_pill(&self) -> (String, DotTone) {
        let in_flight_send = self.prompt_attempts.values().any(|a| a.in_flight);
        match &self.connection_state {
            FeedConnectionState::Reconnecting { attempt, .. } => {
                (format!("重连中 #{attempt}"), DotTone::Warning)
            }
            FeedConnectionState::Disconnected { .. } => ("已断开".into(), DotTone::Danger),
            FeedConnectionState::Connecting { .. } => ("连接中".into(), DotTone::Info),
            _ => {
                if self.projected.turns.is_empty() && !in_flight_send {
                    ("空会话".into(), DotTone::Neutral)
                } else if let Some(last) = self.projected.turns.last() {
                    let any_tool_running = last.items.iter().any(|it| {
                        matches!(&it.item, Item::Tool { activity, .. } if !activity.finished)
                    });
                    if any_tool_running && last.status == TurnStatus::Running {
                        ("工具运行中".into(), DotTone::Info)
                    } else if last.status == TurnStatus::Running || in_flight_send {
                        ("生成中".into(), DotTone::Info)
                    } else {
                        match last.status {
                            TurnStatus::Completed => ("已完成".into(), DotTone::Success),
                            TurnStatus::Interrupted => ("已中断".into(), DotTone::Warning),
                            TurnStatus::Failed => ("失败".into(), DotTone::Danger),
                            TurnStatus::Aborted => ("已中止".into(), DotTone::Warning),
                            TurnStatus::Running => ("运行中".into(), DotTone::Info),
                        }
                    }
                } else if in_flight_send {
                    ("发送中".into(), DotTone::Info)
                } else {
                    ("就绪".into(), DotTone::Neutral)
                }
            }
        }
    }

    fn render_empty_chat_hero(&self, p: &ProPalette, cx: &mut Context<Self>) -> AnyElement {
        let writable = self.access_mode.is_writable();
        let is_http_ro = self.access_mode.is_http_read_only();
        let is_demo = matches!(self.access_mode, ChatAccessMode::DemoShowcase);
        let rt_name = runtime_display_name(&self.selected_runtime);

        let title = if is_http_ro {
            "正在观察远程 HTTP 会话流"
        } else if is_demo {
            "独立演示对话已就绪"
        } else if writable {
            "新会话已就绪"
        } else {
            "开启对话或关联右侧文档"
        };

        let subtitle = if is_http_ro {
            "当前连接仅提供 GET /snapshot 与 GET /events 只读事件流，新回合到达时将实时渲染。"
        } else if writable {
            "直接在下方输入任务，或在右侧 Kantor 编辑器选中段落、单元格、幻灯片对象后点击「AI 改选区」。"
        } else {
            "使用真实 Kar ChatStore 管理持久化会话，在同一原生 GPUI 窗口内联动 Word、Excel 与 PPT 编辑。"
        };

        let starters = [
            (
                "审阅当前文档选区",
                "提炼关键结论并给出可直接替换的润色版本",
                "请审阅当前绑定的文档内容，提炼关键行动项并给出更精炼的修改建议。",
                ProIcon::PageDoc,
            ),
            (
                "检查工作区与文件变更",
                "列出目录文件并核对最近一次编辑状态",
                "请检查当前工作区目录中的文件列表，并用简短中文总结当前内容。",
                ProIcon::Terminal,
            ),
            (
                "核验表格公式与指标",
                "结合右侧预算表单元格分析口径与汇总",
                "请核对当前表格选区的公式逻辑，检查合计行是否覆盖全部明细项。",
                ProIcon::UsageChart,
            ),
        ];

        div()
            .w_full()
            .min_h(px(380.))
            .max_w(px(660.))
            .mx_auto()
            .px(px(24.))
            .py(px(36.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .text_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(44.))
                            .rounded(px(11.))
                            .bg(p.subtle)
                            .border_1()
                            .border_color(p.line)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(pro_icon(ProIcon::Thread, 20., p.muted)),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .text_size(px(17.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.ink)
                            .child(title),
                    )
                    .child(
                        div()
                            .max_w(px(440.))
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .text_color(p.muted)
                            .child(subtitle),
                    ),
            )
            .when(!writable && !is_http_ro, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(pro_btn(
                            "hero-create-real-chat",
                            format!("用 {rt_name} 新建对话"),
                            Some(ProIcon::Plus),
                            ProBtnStyle::Primary,
                            false,
                            cx,
                            |this, _, cx| this.create_new_real_chat(cx),
                        ))
                        .child(pro_btn(
                            "hero-enter-demo-chat",
                            "查看独立演示对话",
                            Some(ProIcon::Sparkles),
                            ProBtnStyle::Secondary,
                            false,
                            cx,
                            |this, _, _| this.bridge.send(KarCommand::EnterDemoShowcase),
                        )),
                )
            })
            .when(writable, |el| {
                let hover_c = p.hover;
                let border_c = p.line;
                let border_hi = p.line_strong;
                let card_bg = p.card;
                let ink_c = p.ink;
                let muted_c = p.muted;
                el.child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .children(starters.into_iter().enumerate().map(
                            |(si, (s_title, s_desc, s_prompt, ic))| {
                                activate(
                                    div()
                                        .id(SharedString::from(format!("starter-card-{si}")))
                                        .role(Role::Button)
                                        .tab_index(0)
                                        .px(px(14.))
                                        .py(px(11.))
                                        .rounded(px(10.))
                                        .bg(card_bg)
                                        .border_1()
                                        .border_color(border_c)
                                        .hover(move |s| s.bg(hover_c).border_color(border_hi))
                                        .flex()
                                        .items_center()
                                        .gap(px(12.))
                                        .cursor_pointer()
                                        .child(
                                            div()
                                                .size(px(30.))
                                                .rounded(px(7.))
                                                .bg(p.subtle)
                                                .flex_none()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(pro_icon(ic, 14., muted_c)),
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
                                                        .text_size(px(13.))
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .text_color(ink_c)
                                                        .child(s_title),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .text_color(muted_c)
                                                        .truncate()
                                                        .child(s_desc),
                                                ),
                                        )
                                        .child(pro_icon(ProIcon::ArrowRight, 13., p.faint)),
                                    cx,
                                    move |this, window, cx| {
                                        this.fill_starter_prompt(s_prompt, window, cx);
                                    },
                                )
                            },
                        )),
                )
            })
            .into_any_element()
    }

    fn render_turn_editorial(
        &self,
        turn_number: usize,
        turn: &ChatTurn,
        p: &ProPalette,
        cx: &mut Context<Self>,
    ) -> Div {
        let turn_id = turn.id;
        let short_uuid = turn_id.to_string()[..8].to_owned();
        let (st_text, st_dot) = match turn.status {
            TurnStatus::Running => ("运行中", DotTone::Info),
            TurnStatus::Completed => ("已完成", DotTone::Success),
            TurnStatus::Interrupted => ("已中断", DotTone::Warning),
            TurnStatus::Failed => ("失败", DotTone::Danger),
            TurnStatus::Aborted => ("已中止", DotTone::Warning),
        };

        let first_user_msg = turn
            .items
            .iter()
            .find_map(|i| match &i.item {
                Item::Message {
                    role: KarRole::User,
                    text,
                    ..
                } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default();

        let can_retry = matches!(
            turn.status,
            TurnStatus::Failed | TurnStatus::Interrupted | TurnStatus::Aborted
        ) && matches!(self.access_mode, ChatAccessMode::WriteController { .. });
        let retry_text = first_user_msg.clone();

        // Bound Office target reference pill (persists across tab switches!)
        let target_ref = self.turn_targets.get(&turn_id).cloned().map(|tgt| {
            let clean_doc = clean_display_doc_name(&tgt.doc_name);
            let loc = tgt.locator.target_label();
            let kind = tgt.kind;
            let hover_c = p.chip_hover;
            let tgt_jump = tgt;
            activate(
                div()
                    .id(SharedString::from(format!("turn-tgt-{turn_id}")))
                    .role(Role::Button)
                    .tab_index(0)
                    .h(px(26.))
                    .pl(px(6.))
                    .pr(px(9.))
                    .rounded(px(7.))
                    .bg(p.subtle)
                    .border_1()
                    .border_color(p.line)
                    .hover(move |s| s.bg(hover_c))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .child(render_office_chip(kind, false, p))
                    .child(
                        div()
                            .max_w(px(320.))
                            .truncate()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.ink)
                            .child(format!("{clean_doc} · {loc}")),
                    )
                    .child(pro_icon(ProIcon::ExternalArrow, 12., p.muted)),
                cx,
                move |this, window, cx| {
                    this.jump_to_selection_target(&tgt_jump, window, cx);
                },
            )
        });

        // Separate user bubble(s) and assistant response parts cleanly
        let mut user_blocks = div().w_full().flex().flex_col().items_end().gap(px(6.));
        if let Some(chip) = target_ref {
            user_blocks = user_blocks.child(chip);
        }

        let mut agent_blocks = div()
            .w_full()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(12.));
        let mut has_agent_items = false;
        let mut tool_file_changes: Vec<(String, &'static str)> = Vec::new();

        let rt_id = self
            .projected
            .info
            .as_ref()
            .map(|i| i.runtime.as_str())
            .unwrap_or(&self.selected_runtime);
        let agent_name = if self.projected.is_demo {
            "Kar 演示助手"
        } else {
            runtime_display_name(rt_id)
        };

        for (item_ix, tl_item) in turn.items.iter().enumerate() {
            match &tl_item.item {
                Item::Message {
                    role: KarRole::User,
                    text,
                    ..
                } => {
                    user_blocks = user_blocks.child(
                        div()
                            .max_w(relative(0.84))
                            .px(px(14.))
                            .py(px(9.))
                            .rounded(px(15.))
                            .bg(p.chip)
                            .text_size(px(14.))
                            .line_height(px(21.5))
                            .text_color(p.ink)
                            .child(text.clone()),
                    );
                }
                Item::Message {
                    role: KarRole::Reasoning,
                    text,
                    ..
                } => {
                    if !text.trim().is_empty() {
                        has_agent_items = true;
                        if let Some(note_entity) = self.reasoning_blocks.get(&(turn_id, item_ix)) {
                            agent_blocks =
                                agent_blocks.child(div().w_full().child(note_entity.clone()));
                        }
                    }
                }
                Item::Message {
                    role: KarRole::Assistant,
                    text,
                    complete,
                    ..
                } => {
                    has_agent_items = true;
                    agent_blocks = agent_blocks.child(
                        div()
                            .w_full()
                            .text_size(px(14.))
                            .line_height(px(22.4))
                            .text_color(p.ink)
                            .child(if *complete {
                                text.clone()
                            } else {
                                format!("{text} ▍")
                            }),
                    );
                }
                Item::Tool { id, activity } => {
                    has_agent_items = true;
                    if let Some(res) = &activity.result {
                        for fc in &res.file_changes {
                            let kind_tag = match fc.kind {
                                ToolChangeKind::Added => "A",
                                ToolChangeKind::Modified => "M",
                                ToolChangeKind::Deleted => "D",
                                ToolChangeKind::Unknown => "U",
                            };
                            tool_file_changes.push((fc.path.clone(), kind_tag));
                        }
                    }
                    if let Some(inspector) = self.tool_inspectors.get(&(turn_id, id.clone())) {
                        agent_blocks = agent_blocks.child(div().w_full().child(inspector.clone()));
                    }
                }
                Item::Compaction { report } => {
                    has_agent_items = true;
                    agent_blocks = agent_blocks.child(
                        div()
                            .h(px(24.))
                            .px(px(9.))
                            .rounded_full()
                            .bg(p.subtle)
                            .border_1()
                            .border_color(p.line)
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(11.5))
                            .text_color(p.muted)
                            .child(pro_icon(ProIcon::Sparkles, 12., p.faint))
                            .child(format!("上下文压缩检查点 · {:?}", report.state)),
                    );
                }
                Item::Diff { .. } => {
                    has_agent_items = true;
                    if let Some(pr) = self.diff_reviews.get(&(turn_id, item_ix)) {
                        agent_blocks = agent_blocks.child(div().w_full().child(pr.clone()));
                    }
                }
            }
        }

        // Merge checkpoint file changes + tool-reported file changes into crisp Artifact Chips
        let mut all_files: Vec<(String, &'static str)> = Vec::new();
        if let Some(cp) = &turn.checkpoint {
            for fc in &cp.files {
                let kind_tag = match fc.kind {
                    CheckpointChangeKind::Added => "A",
                    CheckpointChangeKind::Modified => "M",
                    CheckpointChangeKind::Deleted => "D",
                };
                all_files.push((fc.path.display().to_string(), kind_tag));
            }
        }
        for tf in tool_file_changes {
            if !all_files.iter().any(|(path, _)| path == &tf.0) {
                all_files.push(tf);
            }
        }

        let ws_root = self
            .projected
            .info
            .as_ref()
            .map(|i| i.workspace.clone())
            .unwrap_or_else(|| self.config.workspace_dir.clone());

        if !all_files.is_empty() {
            has_agent_items = true;
            let mut chips = div().flex().flex_wrap().items_center().gap(px(6.));
            chips = chips.child(
                div()
                    .text_size(px(11.5))
                    .text_color(p.muted)
                    .mr(px(2.))
                    .child("文件变更"),
            );
            for (rel_path, kind_code) in all_files {
                let full = ws_root.join(&rel_path);
                let office_kind = OfficeKind::for_path(Path::new(&rel_path));
                let can_open = office_kind.is_some() && full.is_file();
                let tag_color = match kind_code {
                    "A" => p.green_ink,
                    "D" => p.red_ink,
                    _ => p.amber_ink,
                };
                let hover_c = p.chip_hover;
                let pill = div()
                    .id(SharedString::from(format!("fc-{turn_id}-{rel_path}")))
                    .h(px(26.))
                    .pl(px(8.))
                    .pr(px(9.))
                    .rounded(px(6.5))
                    .bg(p.subtle)
                    .border_1()
                    .border_color(p.line)
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(tag_color)
                            .child(kind_code),
                    )
                    .child(
                        div()
                            .text_color(p.ink)
                            .child(rel_path.clone()),
                    )
                    .when(can_open, |d| {
                        d.child(pro_icon(ProIcon::ExternalArrow, 11.5, p.muted))
                    });

                if can_open {
                    chips = chips.child(activate(
                        pill.hover(move |s| s.bg(hover_c)).cursor_pointer(),
                        cx,
                        move |this, window, cx| {
                            this.open_office_file_tab(full.clone(), false, None, window, cx);
                        },
                    ));
                } else {
                    chips = chips.child(pill);
                }
            }
            agent_blocks = agent_blocks.child(chips);
        }

        if let Some(fail) = turn.failure.as_ref() {
            has_agent_items = true;
            agent_blocks = agent_blocks.child(
                div()
                    .w_full()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .bg(p.red_soft)
                    .border_1()
                    .border_color(p.red.opacity(0.24))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.5))
                    .text_color(p.red_ink)
                    .child(pro_icon(ProIcon::AlertTriangle, 14., p.red))
                    .child(div().flex_1().min_w_0().child(format!("{fail:?}"))),
            );
        }

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(14.))
            // Quiet Turn Eyebrow Metadata
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(11.5))
                            .text_color(p.faint)
                            .child(format!("回合 {:02} · {short_uuid}", turn_number))
                            .when(self.projected.is_demo, |d| {
                                d.child(render_demo_pill("演示数据", p))
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .text_size(px(11.5))
                                    .text_color(st_dot.text_color(p))
                                    .child(
                                        div()
                                            .size(px(5.5))
                                            .rounded_full()
                                            .bg(st_dot.dot_color(p)),
                                    )
                                    .child(st_text),
                            )
                            .when(can_retry, |d| {
                                d.child(pro_btn(
                                    SharedString::from(format!("retry-{turn_id}")),
                                    format!("重试 · {short_uuid}"),
                                    Some(ProIcon::Refresh),
                                    ProBtnStyle::Secondary,
                                    true,
                                    cx,
                                    move |this, _, cx| {
                                        this.retry_turn_with_same_uuid(
                                            turn_id,
                                            retry_text.clone(),
                                            cx,
                                        );
                                    },
                                ))
                            }),
                    ),
            )
            .child(user_blocks)
            .when(has_agent_items || turn.status == TurnStatus::Running, |d| {
                d.child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(
                            div()
                                .h(px(22.))
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .child(render_runtime_mark(rt_id, 14., p, cx))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.ink)
                                        .child(agent_name),
                                ),
                        )
                        .child(agent_blocks),
                )
            })
    }

    fn render_permission_dock(&self, p: &ProPalette, cx: &mut Context<Self>) -> Div {
        let mut stack = div()
            .w_full()
            .max_w(px(740.))
            .mx_auto()
            .px(px(20.))
            .pb(px(8.))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(6.));

        for (rix, req) in self.projected.pending_requests.iter().enumerate() {
            let req_allow = req.clone();
            let req_deny = req.clone();
            let allow_choice = req
                .controls()
                .decisions
                .first()
                .copied()
                .unwrap_or(kar::PermissionDecision::AllowOnce);

            stack = stack.child(
                div()
                    .p(px(12.))
                    .rounded(px(12.))
                    .bg(p.card)
                    .border_1()
                    .border_color(p.line_strong)
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .size(px(28.))
                                    .rounded(px(7.))
                                    .bg(p.amber_soft)
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(pro_icon(ProIcon::Shield, 14., p.amber_ink)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.ink)
                                            .child(format!("工具执行需要确认 · {}", req.method)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(p.muted)
                                            .truncate()
                                            .child(req.body.to_string()),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(pro_btn(
                                SharedString::from(format!("perm-deny-{rix}")),
                                "拒绝",
                                None,
                                ProBtnStyle::Ghost,
                                true,
                                cx,
                                move |this, _, _| {
                                    this.bridge.send(KarCommand::DecidePermission {
                                        request: req_deny.clone(),
                                        decision: kar::PermissionDecision::Deny,
                                    });
                                },
                            ))
                            .child(pro_btn(
                                SharedString::from(format!("perm-allow-{rix}")),
                                "允许执行",
                                Some(ProIcon::Check),
                                ProBtnStyle::Primary,
                                true,
                                cx,
                                move |this, _, _| {
                                    this.bridge.send(KarCommand::DecidePermission {
                                        request: req_allow.clone(),
                                        decision: allow_choice,
                                    });
                                },
                            )),
                    ),
            );
        }
        stack
    }

    fn render_composer_card(&self, p: &ProPalette, cx: &mut Context<Self>) -> Div {
        let draft = self.composer_editor.read(cx).value().to_owned();
        let line_count = draft
            .split('\n')
            .count()
            .max((draft.chars().count() / 56) + 1)
            .clamp(2, 6);
        let field_h = (line_count as f32) * 21.;

        let writable = self.access_mode.is_writable();
        let is_running = self
            .projected
            .turns
            .last()
            .is_some_and(|t| t.status == TurnStatus::Running)
            || self.prompt_attempts.values().any(|a| a.in_flight);
        let can_send = writable && !is_running && !draft.trim().is_empty();

        // Pinned Office Selection Target chip folded cleanly inside top of composer (`pro_fold`)
        let has_pinned = self.pinned_target.is_some();
        let pinned_snapshot = self.pinned_target.clone();
        let p_copy = *p;
        let weak_jump = cx.weak_entity();
        let weak_rm = cx.weak_entity();

        let pinned_fold = pro_fold(
            "composer-pinned-target-fold",
            has_pinned,
            38.,
            0.,
            move || {
                let Some(tgt) = pinned_snapshot else {
                    return div().into_any_element();
                };
                let clean_doc = clean_display_doc_name(&tgt.doc_name);
                let loc = tgt.locator.target_label();
                let excerpt = if tgt.excerpt.is_empty() {
                    "(空内容)".to_owned()
                } else {
                    tgt.excerpt.clone()
                };
                let tgt_for_jump = tgt.clone();
                let hover_c = p_copy.hover;
                div()
                    .w_full()
                    .h(px(32.))
                    .pl(px(8.))
                    .pr(px(6.))
                    .rounded(px(9.))
                    .bg(p_copy.subtle)
                    .border_1()
                    .border_color(p_copy.line)
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .child(render_office_chip(tgt.kind, false, &p_copy))
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p_copy.ink)
                                    .child(format!("{clean_doc} · {loc}")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(p_copy.muted)
                                    .child(excerpt),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(
                                div()
                                    .id("cmp-jump-target")
                                    .h(px(22.))
                                    .px(px(6.))
                                    .rounded(px(5.))
                                    .hover(move |s| s.bg(hover_c))
                                    .flex()
                                    .items_center()
                                    .gap(px(3.))
                                    .text_size(px(11.5))
                                    .text_color(p_copy.text)
                                    .cursor_pointer()
                                    .on_click(move |_, window, cx| {
                                        if let Some(e) = weak_jump.upgrade() {
                                            let t = tgt_for_jump.clone();
                                            e.update(cx, |this, cx| {
                                                this.jump_to_selection_target(&t, window, cx);
                                            });
                                        }
                                    })
                                    .child("定位")
                                    .child(pro_icon(ProIcon::ExternalArrow, 11., p_copy.muted)),
                            )
                            .child(
                                div()
                                    .id("cmp-clear-target")
                                    .size(px(22.))
                                    .rounded(px(5.))
                                    .hover(move |s| s.bg(hover_c))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .on_click(move |_, _, cx| {
                                        if let Some(e) = weak_rm.upgrade() {
                                            e.update(cx, |this, cx| {
                                                this.pinned_target = None;
                                                cx.notify();
                                            });
                                        }
                                    })
                                    .child(pro_icon(ProIcon::Close, 11., p_copy.muted)),
                            ),
                    )
                    .into_any_element()
            },
        );

        let has_office_tab = self.active_office_tab.is_some();
        let left_tools = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .when(has_office_tab && writable, |d| {
                d.child(pro_btn(
                    "cmp-bind-selection",
                    "引用当前文档选区",
                    Some(ProIcon::Sparkles),
                    ProBtnStyle::Ghost,
                    true,
                    cx,
                    |this, window, cx| this.bind_active_selection_to_chat(window, cx),
                ))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .px(px(6.))
                    .text_size(px(11.5))
                    .text_color(p.muted)
                    .when(!writable, |d| {
                        d.child(pro_icon(ProIcon::Lock, 12., p.muted))
                            .child("只读观察模式")
                    })
                    .when(writable, |d| {
                        d.child(if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
                            "演示沙盒 · 不写入真实存储"
                        } else {
                            "幂等 Request UUID"
                        })
                    }),
            );

        let submit_control = if is_running && writable {
            activate(
                div()
                    .id("cmp-stop-turn")
                    .role(Role::Button)
                    .aria_label("中止当前回合")
                    .tab_index(0)
                    .size(px(30.))
                    .rounded(px(8.))
                    .bg(p.chip_hover)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(div().size(px(9.)).rounded(px(2.)).bg(p.ink)),
                cx,
                |this, _, _| this.bridge.send(KarCommand::AbortTurn),
            )
            .into_any_element()
        } else {
            let focus_c = p.focus;
            activate(
                div()
                    .id("cmp-send-turn")
                    .role(Role::Button)
                    .aria_label("发送消息")
                    .tab_index(0)
                    .size(px(30.))
                    .rounded(px(8.))
                    .bg(if can_send { p.primary } else { p.chip })
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(can_send, |d| d.cursor_pointer())
                    .focus_visible(move |s| s.border_1().border_color(focus_c))
                    .child(pro_icon(
                        ProIcon::ArrowUp,
                        16.,
                        if can_send { p.on_primary } else { p.faint },
                    )),
                cx,
                |this, window, cx| this.submit_composer_prompt(window, cx),
            )
            .into_any_element()
        };

        let right_tools = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .when(writable, |d| d.child(render_keycap("⌘↵", p)))
            .child(submit_control);

        let card_bg: Hsla = if p.dark { rgb(0x1c1c1e).into() } else { p.card };

        div()
            .w_full()
            .max_w(px(740.))
            .mx_auto()
            .px(px(20.))
            .pt(px(6.))
            .pb(px(16.))
            .flex_none()
            .child(
                div()
                    .w_full()
                    .rounded(px(15.))
                    .bg(card_bg)
                    .border_1()
                    .border_color(if writable { p.line_strong } else { p.line })
                    .p(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(pinned_fold)
                    .child(
                        div()
                            .w_full()
                            .h(px(field_h))
                            .px(px(4.))
                            .py(px(2.))
                            .child(self.composer_editor.clone()),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(left_tools)
                            .child(right_tools),
                    ),
            )
    }
}

fn short_cursor_label(raw: &str) -> String {
    if let Some((prefix, seq)) = raw.split_once(':')
        && prefix.len() > 10
    {
        return format!("{}…:{seq}", &prefix[..8]);
    }
    if raw.len() > 20 {
        format!("{}…", &raw[..16])
    } else {
        raw.to_owned()
    }
}
