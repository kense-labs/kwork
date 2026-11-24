//! Claude Desktop-inspired Conversational Stage + KUI `AssistantHomeScreen` Double-Deck Composer
//! + Real KUI Turn Components (`WorkBadge`, `ExplanationNote`, `InvocationInspector`, `PatchReview`, `CopyButton`).

use std::path::Path;

use gpui::{prelude::*, *};
use kar::{
    ApprovalMode, ChangeKind as ToolChangeKind,
    chat::{ChangeKind as CheckpointChangeKind, ChatTurn, Item, Role as KarRole, TurnStatus},
};
use kui::{
    components::pro_chrome::{
        ProBtnStyle, pro_btn, pro_fold, pro_icon_btn, pro_mark_selected, pro_pill_indicator,
        pro_presence, pro_selection_sensor, pro_view_transition_fill,
    },
    primitives::{
        control::activate,
        pro_icons::{ProIcon, pro_icon},
    },
};

use crate::{
    kar_bridge::KarCommand,
    types::{ChatAccessMode, FeedConnectionState, OfficeKind},
    ui_primitives::{
        DotTone, HEADER_H, MACOS_TRAFFIC_LIGHT_PAD, StudioPalette, clean_display_doc_name,
        render_approval_hand, render_claude_spark, render_demo_pill, render_keycap,
        render_office_chip, render_runtime_mark, render_status_pill, render_tray_folder,
    },
    workbench::{ComposerPopover, KarWorkbench, WorkspaceLayoutMode, runtime_display_name},
};

impl KarWorkbench {
    pub(crate) fn render_chat_pane(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let s = StudioPalette::get(cx);

        let rt_id = self
            .projected
            .info
            .as_ref()
            .map(|i| i.runtime.as_str())
            .unwrap_or(&self.selected_runtime);

        let (crumb_main, crumb_sub) = match &self.access_mode {
            ChatAccessMode::DemoShowcase => {
                ("样例对话沙盒".to_owned(), Some("隔离内存样例".to_owned()))
            }
            ChatAccessMode::HttpReadOnly { url, .. } => {
                ("HTTP 只读副本".to_owned(), Some(url.clone()))
            }
            _ => match &self.projected.info {
                Some(info) => {
                    let ws_name = info
                        .workspace
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(str::to_owned)
                        .unwrap_or_else(|| info.workspace.display().to_string());
                    (
                        format!(
                            "{} · {}",
                            runtime_display_name(&info.runtime),
                            &info.id.to_string()[..8]
                        ),
                        Some(ws_name),
                    )
                }
                None => {
                    let ws_name = self
                        .config
                        .workspace_dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(str::to_owned)
                        .unwrap_or_else(|| self.config.workspace_dir.display().to_string());
                    ("新对话".to_owned(), Some(ws_name))
                }
            },
        };

        let (mode_label, mode_tone) = self.compute_mode_pill();
        let (phase_label, phase_tone) = self.compute_phase_pill();

        // Claude Desktop 3-Segment Workspace Mode Switcher (`对话` | `协作` | `画布`)
        let mode_sensor = pro_selection_sensor();
        let active_layout_key: SharedString = match self.layout_mode {
            WorkspaceLayoutMode::ChatFocus => "layout-chat".into(),
            WorkspaceLayoutMode::Split => "layout-split".into(),
            WorkspaceLayoutMode::CanvasWide => "layout-wide".into(),
        };
        let layouts: [(WorkspaceLayoutMode, &'static str); 3] = [
            (WorkspaceLayoutMode::ChatFocus, "对话"),
            (WorkspaceLayoutMode::Split, "协作"),
            (WorkspaceLayoutMode::CanvasWide, "画布"),
        ];
        let seg_hover = s.stage_hover;
        let layout_switcher = div()
            .relative()
            .h(px(30.))
            .p(px(3.))
            .rounded(px(9.))
            .bg(s.tray_bg)
            .border_1()
            .border_color(s.border)
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(pro_pill_indicator(
                "kar-layout-mode-pill",
                active_layout_key,
                mode_sensor.clone(),
                6.5,
                s.card_bg,
                Some(s.border),
                false,
            ))
            .children(layouts.into_iter().enumerate().map(|(li, (mode, label))| {
                let active = self.layout_mode == mode;
                activate(
                    div()
                        .id(SharedString::from(format!("hdr-layout-{li}")))
                        .role(Role::Tab)
                        .tab_index(0)
                        .relative()
                        .h(px(24.))
                        .px(px(10.))
                        .rounded(px(6.5))
                        .when(active, |d| d.child(pro_mark_selected(&mode_sensor)))
                        .when(!active, |d| d.hover(move |st| st.bg(seg_hover)))
                        .flex()
                        .items_center()
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
                    move |this, _, cx| {
                        this.layout_mode = mode;
                        cx.notify();
                    },
                )
            }));

        // Clear macOS traffic lights when sidebar is collapsed!
        let left_header_pad = if !self.sidebar_open && cfg!(target_os = "macos") {
            MACOS_TRAFFIC_LIGHT_PAD - 4.
        } else {
            14.
        };

        let header = div()
            .h(px(HEADER_H))
            .pl(px(left_header_pad))
            .pr(px(14.))
            .border_b_1()
            .border_color(s.border_soft)
            .window_control_area(WindowControlArea::Drag)
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
                    .when(!self.sidebar_open, |d| {
                        d.child(pro_icon_btn(
                            "chat-expand-sidebar",
                            "展开侧边栏 (⌘.)",
                            ProIcon::SidebarLeft,
                            28.,
                            false,
                            cx,
                            |this, _, cx| {
                                this.sidebar_open = true;
                                cx.notify();
                            },
                        ))
                    })
                    .child(layout_switcher)
                    .child(
                        div()
                            .min_w_0()
                            .pl(px(4.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(render_runtime_mark(rt_id, 14., &s, cx))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(crumb_main),
                            )
                            .when_some(
                                crumb_sub.filter(|_| {
                                    self.layout_mode == WorkspaceLayoutMode::ChatFocus
                                }),
                                |d, sub| {
                                    d.child(div().text_size(px(12.)).text_color(s.faint).child("/"))
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .text_size(px(12.))
                                                .text_color(s.muted)
                                                .child(sub),
                                        )
                                },
                            ),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(render_status_pill(mode_label, mode_tone, &s))
                    .child(render_status_pill(phase_label, phase_tone, &s))
                    .child(self.chat_actions_menu.clone())
                    .child(pro_icon_btn(
                        "hdr-toggle-office-pane",
                        if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
                            "打开右侧 Kantor Office 协作画布"
                        } else {
                            "收起右侧 Office 画布"
                        },
                        ProIcon::SidebarRight,
                        28.,
                        self.layout_mode != WorkspaceLayoutMode::ChatFocus,
                        cx,
                        |this, _, cx| {
                            this.layout_mode = if this.layout_mode == WorkspaceLayoutMode::ChatFocus
                            {
                                WorkspaceLayoutMode::Split
                            } else {
                                WorkspaceLayoutMode::ChatFocus
                            };
                            cx.notify();
                        },
                    )),
            );

        // Optional local HTTP/SSE publisher strip
        let http_host_bar = self.active_http_server_url.clone().map(|srv_url| {
            div()
                .px(px(16.))
                .h(px(36.))
                .border_b_1()
                .border_color(s.border_soft)
                .bg(s.subtle_bg)
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
                        .text_color(s.text)
                        .child(div().size(px(6.)).rounded_full().bg(s.pro.green))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .child(format!("只读 SSE 广播运行中 · {srv_url}")),
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

        // Quiet HTTP Replica Read-Only Capsule
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
                                .bg(s.subtle_bg)
                                .border_1()
                                .border_color(s.border)
                                .overflow_hidden()
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .text_size(px(11.5))
                                .text_color(s.text)
                                .child(pro_icon(ProIcon::Lock, 12., s.muted))
                                .child(
                                    div().min_w_0().truncate().child(format!(
                                        "HTTP 只读副本 (GET /snapshot + /events) · 游标 {cur} · 去重 {duplicates_ignored} · 补缺 {gaps_recovered}"
                                    )),
                                ),
                        ),
                )
            }
            _ => None,
        };

        // Contextual KUI Alert (provider diagnostics, disconnects, read-only lock, etc.)
        let alert_banner = self.show_alert.then(|| {
            div()
                .w_full()
                .max_w(px(720.))
                .mx_auto()
                .px(px(22.))
                .pt(px(12.))
                .flex_none()
                .child(self.status_alert.clone())
        });

        // Claude Desktop Signature Experience:
        // - Empty writable/idle state -> Vertically centered Greeting + Double-Deck Composer + Starter Pills!
        // - Active thread / HTTP stream -> Editorial turn timeline + Bottom-anchored Double-Deck Composer!
        let use_centered_start = self.projected.turns.is_empty()
            && !self.access_mode.is_http_read_only()
            && !matches!(
                self.access_mode,
                ChatAccessMode::LocalReadOnlySubscriber { .. }
            );

        let stage_body = if use_centered_start {
            self.render_centered_start_stage(&s, cx).into_any_element()
        } else {
            let timeline_el = if self.projected.turns.is_empty() {
                self.render_readonly_waiting_state(&s, cx)
            } else {
                let turns_snapshot = self.projected.turns.clone();
                let mut stream = div()
                    .w_full()
                    .max_w(px(720.))
                    .mx_auto()
                    .px(px(24.))
                    .pt(px(20.))
                    .pb(px(28.))
                    .flex()
                    .flex_col()
                    .gap(px(28.));
                for (idx, turn) in turns_snapshot.iter().enumerate() {
                    stream = stream.child(self.render_turn_editorial(idx + 1, turn, &s, cx));
                }
                stream.into_any_element()
            };

            let approval_dock = (!self.projected.pending_requests.is_empty()
                && self.access_mode.is_writable())
            .then(|| self.render_permission_dock(&s, cx));

            let bottom_composer = div()
                .w_full()
                .max_w(px(720.))
                .mx_auto()
                .px(px(20.))
                .pt(px(4.))
                .pb(px(14.))
                .flex_none()
                .child(self.render_double_deck_composer(&s, cx));

            div()
                .size_full()
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(
                    div()
                        .id("kar-timeline-scroll")
                        .track_scroll(&self.chat_scroll)
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(timeline_el),
                )
                .when_some(approval_dock, |d, dock| d.child(dock))
                .child(bottom_composer)
                .into_any_element()
        };

        div()
            .size_full()
            .bg(s.inset_bg)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .when_some(http_host_bar, |d, bar| d.child(bar))
            .when_some(replica_capsule, |d, cap| d.child(cap))
            .when_some(alert_banner, |d, al| d.child(al))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(pro_view_transition_fill(
                        "kar-chat-stage-vtx",
                        if use_centered_start {
                            "start"
                        } else {
                            "thread"
                        },
                        stage_body,
                    )),
            )
            .into_any_element()
    }

    /// Claude Desktop + KUI `AssistantHomeScreen` Centered Start Screen:
    /// Starburst Spark + Editorial Greeting + Double-Deck Composer + Starter Pills + Office Artifact Cards!
    fn render_centered_start_stage(
        &self,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let is_demo = matches!(self.access_mode, ChatAccessMode::DemoShowcase);
        let has_pinned = self.pinned_target.is_some();

        let greeting = if is_demo {
            "独立演示工作台已就绪"
        } else if has_pinned {
            "已关联文档选区，想怎样润色或分析它？"
        } else {
            "下午好，今天想推进哪份文档或代码？"
        };

        let sub_greeting = if is_demo {
            "当前运行于内存隔离沙盒（演示数据），输入任意指令即可体验工具流与文件变更卡片。"
        } else {
            "直接输入任务创建真实持久化 Kar 会话，或与右侧原生 Kantor Word / Excel / PPT 画布实时联动。"
        };

        let starter_pills: [(&'static str, &'static str, ProIcon); 5] = [
            (
                "审阅文档选区",
                "请审阅当前绑定的文档内容，提炼核心行动项并给出可直接替换的精炼版本。",
                ProIcon::PageDoc,
            ),
            (
                "核验预算公式",
                "请核对当前表格选区的公式逻辑，检查合计行是否覆盖全部明细项并说明口径。",
                ProIcon::UsageChart,
            ),
            (
                "润色路演文稿",
                "请结合当前幻灯片标题与要点，给出更有说服力的路演金句与排版建议。",
                ProIcon::Sparkles,
            ),
            (
                "检查工作区变更",
                "请检查当前工作区目录中的文件列表，并用简短中文总结当前文件状态。",
                ProIcon::Terminal,
            ),
            (
                "双写冲突演练",
                "__OPEN_COEDIT__",
                ProIcon::GitBranch,
            ),
        ];

        let chip_bg = s.card_bg;
        let chip_hover = s.stage_hover;
        let chip_border = s.border;
        let chip_border_hi = s.border_strong;
        let ink_c = s.ink;
        let accent_c = s.accent;
        let muted_c = s.muted;

        let pills_row = div()
            .mt(px(16.))
            .w_full()
            .flex()
            .flex_wrap()
            .justify_center()
            .gap(px(8.))
            .children(
                starter_pills
                    .into_iter()
                    .enumerate()
                    .map(|(pi, (label, prompt, icon))| {
                        let is_coedit = prompt == "__OPEN_COEDIT__";
                        activate(
                            div()
                                .id(SharedString::from(format!("start-pill-{pi}")))
                                .role(Role::Button)
                                .tab_index(0)
                                .h(px(32.))
                                .px(px(12.))
                                .rounded_full()
                                .bg(chip_bg)
                                .border_1()
                                .border_color(chip_border)
                                .hover(move |st| st.bg(chip_hover).border_color(chip_border_hi))
                                .flex()
                                .items_center()
                                .gap(px(6.5))
                                .text_size(px(12.5))
                                .text_color(ink_c)
                                .cursor_pointer()
                                .child(pro_icon(
                                    icon,
                                    13.5,
                                    if is_coedit { accent_c } else { muted_c },
                                ))
                                .child(label),
                            cx,
                            move |this, window, cx| {
                                if is_coedit {
                                    this.enter_coedit_demo_workshop(window, cx);
                                } else {
                                    this.fill_starter_prompt(prompt, window, cx);
                                }
                            },
                        )
                    }),
            );

        // When in single-column ChatFocus mode, show a sleek 3-card Office Cowork Shelf right underneath!
        let show_cowork_cards = self.layout_mode == WorkspaceLayoutMode::ChatFocus;
        let cowork_shelf = show_cowork_cards.then(|| {
            let cards: [(OfficeKind, &'static str, &'static str, &'static str); 3] = [
                (
                    OfficeKind::Word,
                    "sample_minutes.docx",
                    "会议纪要.docx",
                    "段落选区修改与保存重开",
                ),
                (
                    OfficeKind::Excel,
                    "sample_formulas.xlsx",
                    "季度预算表.xlsx",
                    "SUM 公式实时重算验证",
                ),
                (
                    OfficeKind::Ppt,
                    "sample_deck.pptx",
                    "产品路演.pptx",
                    "形状文字与坐标精准调整",
                ),
            ];
            let c_bg = s.card_bg;
            let c_hover = s.stage_hover;
            let c_border = s.border;
            let c_border_hi = s.border_strong;
            div()
                .mt(px(26.))
                .w_full()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .px(px(4.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(s.muted)
                                .child("在右侧开启原生 Kantor Office 协作画布"),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(s.faint)
                                .child("点击即分栏打开测试副本"),
                        ),
                )
                .child(
                    div().w_full().flex().gap(px(10.)).children(
                        cards
                            .into_iter()
                            .enumerate()
                            .map(|(ci, (kind, fname, title, desc))| {
                                activate(
                                    div()
                                        .id(SharedString::from(format!("hero-doc-card-{ci}")))
                                        .role(Role::Button)
                                        .tab_index(0)
                                        .flex_1()
                                        .min_w_0()
                                        .p(px(12.))
                                        .rounded(px(12.))
                                        .bg(c_bg)
                                        .border_1()
                                        .border_color(c_border)
                                        .hover(move |st| st.bg(c_hover).border_color(c_border_hi))
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.))
                                        .cursor_pointer()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .child(render_office_chip(kind, false, s))
                                                .child(pro_icon(
                                                    ProIcon::ExternalArrow,
                                                    12.,
                                                    s.faint,
                                                )),
                                        )
                                        .child(
                                            div()
                                                .pt(px(2.))
                                                .text_size(px(13.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(s.ink)
                                                .truncate()
                                                .child(title),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.5))
                                                .text_color(s.muted)
                                                .truncate()
                                                .child(desc),
                                        ),
                                    cx,
                                    move |this, window, cx| {
                                        this.open_working_sample(fname, window, cx);
                                    },
                                )
                            }),
                    ),
                )
        });

        div()
            .id("kar-centered-start-scroll")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .my_auto()
                    .w_full()
                    .max_w(px(688.))
                    .px(px(24.))
                    .py(px(36.))
                    .flex()
                    .flex_col()
                    .items_center()
                    // 1. Claude-style Starburst Spark + Editorial Greeting
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .text_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(render_claude_spark(26., s.accent))
                                    .when(is_demo, |d| d.child(render_demo_pill("演示数据", s))),
                            )
                            .child(
                                div()
                                    .text_size(px(25.))
                                    .line_height(px(32.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(greeting),
                            )
                            .child(
                                div()
                                    .max_w(px(520.))
                                    .text_size(px(13.))
                                    .line_height(px(20.))
                                    .text_color(s.muted)
                                    .child(sub_greeting),
                            ),
                    )
                    // 2. Centered Double-Deck Composer Tray
                    .child(
                        div()
                            .mt(px(24.))
                            .w_full()
                            .child(self.render_double_deck_composer(s, cx)),
                    )
                    // 3. Starter Prompt Capsules
                    .child(pills_row)
                    // 4. Optional Cowork Office Quick-Launch Shelf
                    .when_some(cowork_shelf, |d, shelf| d.child(shelf)),
            )
    }

    fn render_readonly_waiting_state(
        &self,
        s: &StudioPalette,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_http = self.access_mode.is_http_read_only();
        let (title, sub) = if is_http {
            (
                "正在监听远程 HTTP/SSE 会话流",
                "当前连接仅提供 GET /snapshot 与 GET /events 只读视图；新回合与工具事件到达时将实时渲染。",
            )
        } else {
            (
                "只读观察模式（另一客户端持有写入控制器）",
                "本窗口自动订阅同一持久化会话状态，所有工具执行与回复将实时同步更新。",
            )
        };
        div()
            .w_full()
            .min_h(px(320.))
            .max_w(px(560.))
            .mx_auto()
            .px(px(24.))
            .py(px(48.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .text_center()
            .gap(px(10.))
            .child(
                div()
                    .size(px(42.))
                    .rounded_full()
                    .bg(s.subtle_bg)
                    .border_1()
                    .border_color(s.border)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(pro_icon(ProIcon::Eye, 18., s.muted)),
            )
            .child(
                div()
                    .text_size(px(16.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.ink)
                    .child(title),
            )
            .child(
                div()
                    .max_w(px(440.))
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .text_color(s.muted)
                    .child(sub),
            )
            .into_any_element()
    }

    /// Signature Double-Deck Composer Tray (`AssistantHomeScreen` + Claude Desktop):
    /// Upper Deck = Elevated `write_card` with `pro_fold` attachment pill, multiline KUI `Editor`,
    /// `+` Quick Attach popover, Approval Policy popover, inline Runtime Picker popover, and Terracotta Send button.
    /// Lower Deck = Context tray showing workspace folder & open Office documents pill.
    fn render_double_deck_composer(&self, s: &StudioPalette, cx: &mut Context<Self>) -> Div {
        let draft = self.composer_editor.read(cx).value().to_owned();
        let line_count = draft
            .split('\n')
            .count()
            .max((draft.chars().count() / 54) + 1)
            .clamp(2, 6);
        let field_h = (line_count as f32) * 21.5;

        let can_type = self.access_mode.is_writable()
            || matches!(self.access_mode, ChatAccessMode::NoneSelected);
        let is_running = self
            .projected
            .turns
            .last()
            .is_some_and(|t| t.status == TurnStatus::Running)
            || self.prompt_attempts.values().any(|a| a.in_flight);
        let can_send = can_type && !is_running && !draft.trim().is_empty();

        // 1. Folded Selection Target Attachment Strip inside Upper Deck (`pro_fold`)
        let has_pinned = self.pinned_target.is_some();
        let pinned_snapshot = self.pinned_target.clone();
        let s_copy = *s;
        let weak_jump = cx.weak_entity();
        let weak_rm = cx.weak_entity();

        let pinned_fold = pro_fold(
            "composer-pinned-target-fold",
            has_pinned,
            40.,
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
                let hover_c = s_copy.stage_hover;
                div()
                    .w_full()
                    .h(px(34.))
                    .pl(px(8.))
                    .pr(px(6.))
                    .rounded(px(10.))
                    .bg(s_copy.subtle_bg)
                    .border_1()
                    .border_color(s_copy.border)
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
                            .child(render_office_chip(tgt.kind, false, &s_copy))
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s_copy.ink)
                                    .child(format!("{clean_doc} · {loc}")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(s_copy.muted)
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
                                    .h(px(24.))
                                    .px(px(7.))
                                    .rounded(px(6.))
                                    .hover(move |st| st.bg(hover_c))
                                    .flex()
                                    .items_center()
                                    .gap(px(3.))
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s_copy.accent_ink)
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
                                    .child(pro_icon(ProIcon::ExternalArrow, 11., s_copy.accent_ink)),
                            )
                            .child(
                                div()
                                    .id("cmp-clear-target")
                                    .size(px(24.))
                                    .rounded(px(6.))
                                    .hover(move |st| st.bg(hover_c))
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
                                    .child(pro_icon(ProIcon::Close, 11., s_copy.muted)),
                            ),
                    )
                    .into_any_element()
            },
        );

        // 2. Composer Action Bar Controls + Spring-Animated Popovers (`pro_presence`)
        let has_office_tab = self.active_office_tab.is_some();
        let hover_pill = s.stage_hover;
        let approval_label = match self.approval_mode {
            ApprovalMode::Host => "请求审批",
            ApprovalMode::Deny => "只读拒绝",
        };

        let left_controls = div()
            .flex()
            .items_center()
            .gap(px(4.))
            // `+` Add / Open Document Popover Trigger
            .child(
                div()
                    .id("cmp-plus-btn")
                    .size(px(30.))
                    .rounded(px(8.))
                    .bg(if self.composer_popover == Some(ComposerPopover::AddContext) {
                        s.stage_active
                    } else {
                        transparent_black()
                    })
                    .hover(move |st| st.bg(hover_pill))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.composer_popover =
                            if this.composer_popover == Some(ComposerPopover::AddContext) {
                                None
                            } else {
                                Some(ComposerPopover::AddContext)
                            };
                        cx.notify();
                    }))
                    .child(pro_icon(ProIcon::Plus, 16., s.text)),
            )
            // `引用选区` Quick Action when an Office tab is open
            .when(has_office_tab && can_type, |d| {
                let accent_soft = s.accent_soft;
                let accent_ink = s.accent_ink;
                d.child(activate(
                    div()
                        .id("cmp-bind-selection")
                        .role(Role::Button)
                        .tab_index(0)
                        .h(px(28.))
                        .px(px(9.))
                        .rounded(px(8.))
                        .bg(accent_soft)
                        .hover(move |st| st.opacity(0.88))
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(accent_ink)
                        .cursor_pointer()
                        .child(render_claude_spark(12., s.accent))
                        .child("引用选区"),
                    cx,
                    |this, window, cx| this.bind_active_selection_to_chat(window, cx),
                ))
            })
            // Approval Policy Trigger (`请求审批` / `只读拒绝`)
            .when(can_type, |d| {
                d.child(
                    div()
                        .id("cmp-approval-btn")
                        .h(px(28.))
                        .px(px(8.))
                        .rounded(px(8.))
                        .bg(
                            if self.composer_popover == Some(ComposerPopover::ApprovalPolicy) {
                                s.stage_active
                            } else {
                                transparent_black()
                            },
                        )
                        .hover(move |st| st.bg(hover_pill))
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .text_size(px(12.))
                        .text_color(s.muted)
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.composer_popover = if this.composer_popover
                                == Some(ComposerPopover::ApprovalPolicy)
                            {
                                None
                            } else {
                                Some(ComposerPopover::ApprovalPolicy)
                            };
                            cx.notify();
                        }))
                        .child(render_approval_hand(13.5, s.muted))
                        .child(approval_label)
                        .child(pro_icon(ProIcon::ChevronDown, 11., s.faint)),
                )
            });

        // Inline Runtime / Model Picker + Send/Stop Button
        let submit_control = if is_running && can_type {
            activate(
                div()
                    .id("cmp-stop-turn")
                    .role(Role::Button)
                    .aria_label("中止当前回合")
                    .tab_index(0)
                    .size(px(32.))
                    .rounded(px(10.))
                    .bg(s.stage_active)
                    .border_1()
                    .border_color(s.border_strong)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(div().size(px(9.5)).rounded(px(2.5)).bg(s.ink)),
                cx,
                |this, _, _| this.bridge.send(KarCommand::AbortTurn),
            )
            .into_any_element()
        } else {
            let send_bg = if can_send { s.accent } else { s.tray_bg };
            let send_hover = s.accent_hover;
            activate(
                div()
                    .id("cmp-send-turn")
                    .role(Role::Button)
                    .aria_label("发送消息")
                    .tab_index(0)
                    .size(px(32.))
                    .rounded(px(10.))
                    .bg(send_bg)
                    .when(can_send, move |d| {
                        d.hover(move |st| st.bg(send_hover)).cursor_pointer()
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(pro_icon(
                        ProIcon::ArrowUp,
                        16.,
                        if can_send { s.on_primary } else { s.faint },
                    )),
                cx,
                |this, window, cx| this.submit_composer_prompt(window, cx),
            )
            .into_any_element()
        };

        let right_controls = div()
            .flex()
            .items_center()
            .gap(px(6.))
            // Claude Desktop-style Inline Runtime/Model Selector Pill
            .child(
                div()
                    .id("cmp-runtime-picker-btn")
                    .h(px(28.))
                    .px(px(8.5))
                    .rounded(px(8.))
                    .bg(
                        if self.composer_popover == Some(ComposerPopover::RuntimePicker) {
                            s.stage_active
                        } else {
                            transparent_black()
                        },
                    )
                    .hover(move |st| st.bg(hover_pill))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.text)
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.composer_popover =
                            if this.composer_popover == Some(ComposerPopover::RuntimePicker) {
                                None
                            } else {
                                Some(ComposerPopover::RuntimePicker)
                            };
                        cx.notify();
                    }))
                    .child(render_runtime_mark(&self.selected_runtime, 13.5, s, cx))
                    .child(runtime_display_name(&self.selected_runtime))
                    .child(pro_icon(ProIcon::ChevronDown, 11.5, s.muted)),
            )
            .when(can_type, |d| d.child(render_keycap("⌘↵", s)))
            .child(submit_control);

        // Floating Popover Surfaces anchored cleanly above the composer (`pro_presence`)
        let add_pop = self.render_add_context_popover(s, cx);
        let approval_pop = self.render_approval_popover(s, cx);
        let runtime_pop = self.render_runtime_popover(s, cx);

        // Upper Deck (`write_card`)
        let write_card = div()
            .relative()
            .w_full()
            .rounded(px(16.))
            .bg(s.card_bg)
            .border_1()
            .border_color(if can_type { s.border_strong } else { s.border })
            .shadow(s.composer_shadows())
            .p(px(10.))
            .flex()
            .flex_col()
            .gap(px(6.))
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
                    .pt(px(2.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(left_controls)
                    .child(right_controls),
            )
            .child(add_pop)
            .child(approval_pop)
            .child(runtime_pop);

        // Lower Deck (`32px` Context Tray Row inside outer tray)
        let ws_folder = self
            .config
            .workspace_dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| "workspace".into());
        let open_doc_count = self.office_tabs.len();
        let tray_note = if !can_type {
            "只读观察模式 · 输入已锁定"
        } else if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            "演示沙盒 · 不写入真实存储"
        } else {
            "幂等 UUID 重试保护"
        };

        let tray_footer = div()
            .h(px(30.))
            .px(px(10.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(11.5))
                    .text_color(s.muted)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .text_color(s.text)
                            .child(render_tray_folder(13.5, s.accent))
                            .child(ws_folder),
                    )
                    .child(div().text_color(s.faint).child("·"))
                    .child(div().min_w_0().truncate().child(tray_note)),
            )
            .child(activate(
                div()
                    .id("cmp-tray-office-toggle")
                    .role(Role::Button)
                    .tab_index(0)
                    .h(px(22.))
                    .px(px(7.))
                    .rounded(px(6.))
                    .hover(move |st| st.bg(hover_pill))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_size(px(11.5))
                    .text_color(s.text)
                    .cursor_pointer()
                    .child(pro_icon(ProIcon::PageDoc, 12., s.muted))
                    .child(if open_doc_count > 0 {
                        format!("已开 {open_doc_count} 份 Office 文档")
                    } else {
                        "打开 Office 画布".into()
                    }),
                cx,
                |this, _, cx| {
                    this.layout_mode = if this.layout_mode == WorkspaceLayoutMode::ChatFocus {
                        WorkspaceLayoutMode::Split
                    } else {
                        WorkspaceLayoutMode::ChatFocus
                    };
                    cx.notify();
                },
            ));

        div()
            .w_full()
            .rounded(px(20.))
            .bg(s.tray_bg)
            .border_1()
            .border_color(s.border)
            .p(px(5.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(write_card)
            .child(tray_footer)
    }

    fn render_add_context_popover(
        &self,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.composer_popover == Some(ComposerPopover::AddContext);
        let card_bg = s.card_bg;
        let border_c = s.border_strong;
        let hover_c = s.stage_hover;
        let ink_c = s.ink;
        let muted_c = s.muted;
        let shadows = s.popover_shadows();
        let has_tab = self.active_office_tab.is_some();
        let weak = cx.weak_entity();

        pro_presence("cmp-add-popover", open, 0.16, 0.12, move |amt| {
            let items: Vec<(&'static str, &'static str, ProIcon)> = vec![
                ("bind_sel", "关联当前 Office 文档选区", ProIcon::Pin),
                ("open_docx", "打开 Word 会议纪要样本 (.docx)", ProIcon::PageDoc),
                ("open_xlsx", "打开 Excel 预算公式表 (.xlsx)", ProIcon::UsageChart),
                ("open_pptx", "打开 PPT 产品发布文稿 (.pptx)", ProIcon::Sparkles),
                ("open_coedit", "启动人机双写协议演练 (样例)", ProIcon::GitBranch),
            ];
            div()
                .id("cmp-add-pop-surface")
                .absolute()
                .left(px(8.))
                .bottom(px(44. + 6. * amt))
                .opacity(amt)
                .w(px(252.))
                .p(px(5.))
                .rounded(px(12.))
                .bg(card_bg)
                .border_1()
                .border_color(border_c)
                .shadow(shadows.clone())
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .flex()
                .flex_col()
                .gap(px(2.))
                .children(items.into_iter().enumerate().filter_map(|(ix, (act, label, ic))| {
                    if act == "bind_sel" && !has_tab {
                        return None;
                    }
                    let weak_c = weak.clone();
                    Some(
                        div()
                            .id(SharedString::from(format!("cmp-add-item-{ix}")))
                            .h(px(32.))
                            .px(px(9.))
                            .rounded(px(8.))
                            .hover(move |st| st.bg(hover_c))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(12.5))
                            .text_color(ink_c)
                            .cursor_pointer()
                            .on_click(move |_, window, cx| {
                                if let Some(e) = weak_c.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.composer_popover = None;
                                        match act {
                                            "bind_sel" => {
                                                this.bind_active_selection_to_chat(window, cx)
                                            }
                                            "open_docx" => this.open_working_sample(
                                                "sample_minutes.docx",
                                                window,
                                                cx,
                                            ),
                                            "open_xlsx" => this.open_working_sample(
                                                "sample_formulas.xlsx",
                                                window,
                                                cx,
                                            ),
                                            "open_pptx" => this.open_working_sample(
                                                "sample_deck.pptx",
                                                window,
                                                cx,
                                            ),
                                            "open_coedit" => {
                                                this.enter_coedit_demo_workshop(window, cx)
                                            }
                                            _ => {}
                                        }
                                        cx.notify();
                                    });
                                }
                            })
                            .child(pro_icon(ic, 14., muted_c))
                            .child(label),
                    )
                }))
                .into_any_element()
        })
    }

    fn render_approval_popover(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let open = self.composer_popover == Some(ComposerPopover::ApprovalPolicy);
        let current = self.approval_mode;
        let card_bg = s.card_bg;
        let border_c = s.border_strong;
        let hover_c = s.stage_hover;
        let ink_c = s.ink;
        let muted_c = s.muted;
        let accent_c = s.accent;
        let shadows = s.popover_shadows();
        let weak = cx.weak_entity();

        pro_presence("cmp-approval-popover", open, 0.16, 0.12, move |amt| {
            let modes = [
                (
                    ApprovalMode::Host,
                    "请求交互审批 (Host)",
                    "工具执行前在对话底部弹出允许/拒绝确认栏",
                ),
                (
                    ApprovalMode::Deny,
                    "只读安全拒绝 (Deny)",
                    "自动拒绝需要额外宿主授权的敏感操作",
                ),
            ];
            div()
                .id("cmp-approval-pop-surface")
                .absolute()
                .left(px(42.))
                .bottom(px(44. + 6. * amt))
                .opacity(amt)
                .w(px(276.))
                .p(px(6.))
                .rounded(px(12.))
                .bg(card_bg)
                .border_1()
                .border_color(border_c)
                .shadow(shadows.clone())
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .flex()
                .flex_col()
                .gap(px(2.))
                .children(modes.into_iter().enumerate().map(|(ix, (m, title, desc))| {
                    let selected = current == m;
                    let weak_c = weak.clone();
                    div()
                        .id(SharedString::from(format!("cmp-appr-opt-{ix}")))
                        .px(px(10.))
                        .py(px(7.))
                        .rounded(px(8.))
                        .hover(move |st| st.bg(hover_c))
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap(px(8.))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            if let Some(e) = weak_c.upgrade() {
                                e.update(cx, |this, cx| {
                                    this.approval_mode = m;
                                    this.composer_popover = None;
                                    cx.notify();
                                });
                            }
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(ink_c)
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .line_height(px(15.5))
                                        .text_color(muted_c)
                                        .child(desc),
                                ),
                        )
                        .when(selected, |d| {
                            d.child(
                                div()
                                    .pt(px(2.))
                                    .child(pro_icon(ProIcon::Check, 13.5, accent_c)),
                            )
                        })
                }))
                .into_any_element()
        })
    }

    fn render_runtime_popover(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let open = self.composer_popover == Some(ComposerPopover::RuntimePicker);
        let selected_rt = self.selected_runtime.clone();
        let providers_snapshot = self.providers.clone();
        let s_copy = *s;
        let shadows = s.popover_shadows();
        let weak = cx.weak_entity();
        let weak_diag = cx.weak_entity();

        // Pre-render brand marks on `&App` before closure!
        let runtimes = [
            "codex", "claude", "grok", "opencode", "kimi", "pi", "qwen", "cursor",
        ];
        let rows_data: Vec<(String, &'static str, &'static str, bool, AnyElement)> = runtimes
            .into_iter()
            .map(|id| {
                let (badge, ok) = providers_snapshot
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.status_badge())
                    .unwrap_or(("待探测", false));
                let mark = render_runtime_mark(id, 14., &s_copy, cx);
                (id.to_owned(), runtime_display_name(id), badge, ok, mark)
            })
            .collect();

        pro_presence("cmp-runtime-popover", open, 0.16, 0.12, move |amt| {
            let mut list = div()
                .id("cmp-runtime-pop-surface")
                .absolute()
                .right(px(8.))
                .bottom(px(44. + 6. * amt))
                .opacity(amt)
                .w(px(268.))
                .p(px(6.))
                .rounded(px(12.))
                .bg(s_copy.card_bg)
                .border_1()
                .border_color(s_copy.border_strong)
                .shadow(shadows.clone())
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .flex()
                .flex_col()
                .gap(px(2.));

            for (ix, (id, disp, badge, ok, _)) in rows_data.iter().enumerate() {
                let is_sel = &selected_rt == id;
                let id_owned = id.clone();
                let weak_c = weak.clone();
                let hover_c = s_copy.stage_hover;
                let dot_c = if *ok { s_copy.pro.green } else { s_copy.pro.amber };
                list = list.child(
                    div()
                        .id(SharedString::from(format!("cmp-rt-opt-{ix}")))
                        .h(px(32.))
                        .px(px(9.))
                        .rounded(px(8.))
                        .bg(if is_sel {
                            s_copy.stage_active
                        } else {
                            transparent_black()
                        })
                        .hover(move |st| st.bg(hover_c))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            if let Some(e) = weak_c.upgrade() {
                                let pick = id_owned.clone();
                                e.update(cx, |this, cx| {
                                    this.select_runtime_by_id(&pick, cx);
                                });
                            }
                        })
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .child(div().size(px(6.)).rounded_full().bg(dot_c))
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font_weight(if is_sel {
                                            FontWeight::MEDIUM
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(s_copy.ink)
                                        .child(*disp),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(s_copy.muted)
                                        .child(*badge),
                                )
                                .when(is_sel, |d| {
                                    d.child(pro_icon(ProIcon::Check, 12.5, s_copy.accent))
                                }),
                        ),
                );
            }

            let hover_diag = s_copy.stage_hover;
            list.child(
                div()
                    .mt(px(3.))
                    .pt(px(4.))
                    .border_t_1()
                    .border_color(s_copy.border_soft)
                    .child(
                        div()
                            .id("cmp-rt-open-health")
                            .h(px(28.))
                            .px(px(9.))
                            .rounded(px(7.))
                            .hover(move |st| st.bg(hover_diag))
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(11.5))
                            .text_color(s_copy.accent_ink)
                            .cursor_pointer()
                            .on_click(move |_, _, cx| {
                                if let Some(e) = weak_diag.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.toggle_provider_sheet(cx);
                                    });
                                }
                            })
                            .child("查看完整供应商健康面板…")
                            .child(pro_icon(ProIcon::ExternalArrow, 11.5, s_copy.accent_ink)),
                    ),
            )
            .into_any_element()
        })
    }

    fn compute_mode_pill(&self) -> (String, DotTone) {
        match &self.access_mode {
            ChatAccessMode::NoneSelected => ("新会话".into(), DotTone::Accent),
            ChatAccessMode::WriteController { .. } => ("写入控制".into(), DotTone::Success),
            ChatAccessMode::LocalReadOnlySubscriber { .. } => ("只读订阅".into(), DotTone::Warning),
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
                    ("空闲".into(), DotTone::Neutral)
                } else if let Some(last) = self.projected.turns.last() {
                    let any_tool_running = last.items.iter().any(|it| {
                        matches!(&it.item, Item::Tool { activity, .. } if !activity.finished)
                    });
                    if any_tool_running && last.status == TurnStatus::Running {
                        ("工具运行中".into(), DotTone::Info)
                    } else if last.status == TurnStatus::Running || in_flight_send {
                        ("生成中".into(), DotTone::Accent)
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
                    ("发送中".into(), DotTone::Accent)
                } else {
                    ("就绪".into(), DotTone::Neutral)
                }
            }
        }
    }

    fn render_turn_editorial(
        &self,
        turn_number: usize,
        turn: &ChatTurn,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Div {
        let turn_id = turn.id;
        let short_uuid = turn_id.to_string()[..8].to_owned();
        let (st_text, st_dot) = match turn.status {
            TurnStatus::Running => ("运行中", DotTone::Accent),
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
        let retry_text = first_user_msg;

        // Claude Artifact-style Selection Target Reference Card (persists across tab switches!)
        let target_ref = self.turn_targets.get(&turn_id).cloned().map(|tgt| {
            let clean_doc = clean_display_doc_name(&tgt.doc_name);
            let loc = tgt.locator.target_label();
            let excerpt = tgt.excerpt.clone();
            let kind = tgt.kind;
            let hover_c = s.stage_hover;
            let border_hi = s.border_strong;
            let tgt_jump = tgt;
            activate(
                div()
                    .id(SharedString::from(format!("turn-tgt-{turn_id}")))
                    .role(Role::Button)
                    .tab_index(0)
                    .max_w(px(420.))
                    .px(px(11.))
                    .py(px(8.))
                    .rounded(px(12.))
                    .bg(s.card_bg)
                    .border_1()
                    .border_color(s.border)
                    .hover(move |st| st.bg(hover_c).border_color(border_hi))
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .cursor_pointer()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.5))
                                    .child(render_office_chip(kind, false, s))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.ink)
                                            .child(format!("{clean_doc} · {loc}")),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(px(3.))
                                    .text_size(px(11.))
                                    .text_color(s.accent_ink)
                                    .child("定位原目标")
                                    .child(pro_icon(ProIcon::ExternalArrow, 11., s.accent_ink)),
                            ),
                    )
                    .when(!excerpt.is_empty(), |d| {
                        d.child(
                            div()
                                .text_size(px(11.5))
                                .text_color(s.muted)
                                .truncate()
                                .child(excerpt),
                        )
                    }),
                cx,
                move |this, window, cx| {
                    this.jump_to_selection_target(&tgt_jump, window, cx);
                },
            )
        });

        // Separate user bubble(s) and assistant response blocks cleanly
        let mut user_blocks = div().w_full().flex().flex_col().items_end().gap(px(7.));
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

        // Mount KUI `WorkBadge` while the turn is actively executing; when settled,
        // render a clean left-aligned Claude-style author eyebrow so prose & tool cards align on one vertical axis!
        if turn.status == TurnStatus::Running {
            if let Some(work_badge) = self.turn_badges.get(&turn_id) {
                has_agent_items = true;
                agent_blocks = agent_blocks.child(div().w_full().child(work_badge.clone()));
            }
        } else {
            agent_blocks = agent_blocks.child(
                div()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .child(render_runtime_mark(rt_id, 14.5, s, cx))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child(agent_name),
                    ),
            );
        }

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
                            .px(px(15.))
                            .py(px(10.))
                            .rounded(px(18.))
                            .bg(s.user_bubble)
                            .text_size(px(14.))
                            .line_height(px(22.))
                            .text_color(s.ink)
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
                            .text_size(px(14.5))
                            .line_height(px(23.5))
                            .text_color(s.ink)
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
                                ToolChangeKind::Added => "新增",
                                ToolChangeKind::Modified => "修改",
                                ToolChangeKind::Deleted => "删除",
                                ToolChangeKind::Unknown => "变更",
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
                            .bg(s.subtle_bg)
                            .border_1()
                            .border_color(s.border)
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(11.5))
                            .text_color(s.muted)
                            .child(pro_icon(ProIcon::Sparkles, 12., s.faint))
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

        // Claude-style File & Artifact Cards
        let mut all_files: Vec<(String, &'static str)> = Vec::new();
        if let Some(cp) = &turn.checkpoint {
            for fc in &cp.files {
                let kind_tag = match fc.kind {
                    CheckpointChangeKind::Added => "新增",
                    CheckpointChangeKind::Modified => "修改",
                    CheckpointChangeKind::Deleted => "删除",
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
            let mut chips = div().pt(px(2.)).flex().flex_wrap().items_center().gap(px(8.));
            for (rel_path, kind_label) in all_files {
                let full = ws_root.join(&rel_path);
                let office_kind = OfficeKind::for_path(Path::new(&rel_path));
                let can_open = office_kind.is_some() && full.is_file();
                let tag_color = match kind_label {
                    "新增" => s.pro.green_ink,
                    "删除" => s.pro.red_ink,
                    _ => s.accent_ink,
                };
                let hover_c = s.stage_hover;
                let pill = div()
                    .id(SharedString::from(format!("fc-{turn_id}-{rel_path}")))
                    .h(px(30.))
                    .pl(px(9.))
                    .pr(px(10.))
                    .rounded(px(8.5))
                    .bg(s.card_bg)
                    .border_1()
                    .border_color(s.border)
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(px(12.))
                    .when_some(office_kind, |d, ok| d.child(render_office_chip(ok, true, s)))
                    .when(office_kind.is_none(), |d| {
                        d.child(pro_icon(ProIcon::PageDoc, 13., s.muted))
                    })
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child(rel_path.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(tag_color)
                            .child(kind_label),
                    )
                    .when(can_open, |d| {
                        d.child(pro_icon(ProIcon::ExternalArrow, 11.5, s.muted))
                    });

                if can_open {
                    chips = chips.child(activate(
                        pill.hover(move |st| st.bg(hover_c)).cursor_pointer(),
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
                    .py(px(9.))
                    .rounded(px(10.))
                    .bg(s.pro.red_soft)
                    .border_1()
                    .border_color(s.pro.red.opacity(0.24))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.5))
                    .text_color(s.pro.red_ink)
                    .child(pro_icon(ProIcon::AlertTriangle, 14., s.pro.red))
                    .child(div().flex_1().min_w_0().child(format!("{fail:?}"))),
            );
        }

        // Turn Footer Action Bar: KUI `CopyButton` + Turn Metadata + UUID Retry
        let copy_btn = self.copy_buttons.get(&turn_id).cloned();
        let footer_actions = div()
            .pt(px(2.))
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when_some(copy_btn, |d, cb| d.child(cb))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .text_size(px(11.5))
                            .text_color(s.muted)
                            .child(
                                div()
                                    .size(px(5.5))
                                    .rounded_full()
                                    .bg(st_dot.dot_color(s)),
                            )
                            .child(format!(
                                "{agent_name} · 回合 {:02} ({short_uuid}) · {st_text}",
                                turn_number
                            )),
                    )
                    .when(self.projected.is_demo, |d| {
                        d.child(render_demo_pill("演示数据", s))
                    }),
            )
            .when(can_retry, |d| {
                d.child(pro_btn(
                    SharedString::from(format!("retry-{turn_id}")),
                    format!("原 UUID 重试 · {short_uuid}"),
                    Some(ProIcon::Refresh),
                    ProBtnStyle::Secondary,
                    true,
                    cx,
                    move |this, _, cx| {
                        this.retry_turn_with_same_uuid(turn_id, retry_text.clone(), cx);
                    },
                ))
            });

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(user_blocks)
            .when(has_agent_items || turn.status == TurnStatus::Running, |d| {
                d.child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(agent_blocks)
                        .child(footer_actions),
                )
            })
    }

    fn render_permission_dock(&self, s: &StudioPalette, cx: &mut Context<Self>) -> Div {
        let mut stack = div()
            .w_full()
            .max_w(px(720.))
            .mx_auto()
            .px(px(20.))
            .pb(px(6.))
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
                    .rounded(px(14.))
                    .bg(s.card_bg)
                    .border_1()
                    .border_color(s.border_strong)
                    .shadow(s.composer_shadows())
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
                                    .size(px(30.))
                                    .rounded(px(8.))
                                    .bg(s.pro.amber_soft)
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(pro_icon(ProIcon::Shield, 15., s.pro.amber_ink)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.ink)
                                            .child(format!("工具执行需要确认 · {}", req.method)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(s.muted)
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
}

fn short_cursor_label(raw: &str) -> String {
    if raw.len() > 22 {
        format!("{}…", &raw[..22])
    } else {
        raw.to_owned()
    }
}
