//! Kense Monochrome Conversational Stage powered by tailored KUI micro-components:
//! `AiSpark`, `GlassSegmented`, `Alert`, `WorkBadge`, `ExplanationNote`, `InvocationInspector`,
//! `PatchReview`, `CopyButton`, and KUI `AssistantHomeScreen`'s Double-Deck Composer.

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
        pro_icons::{ProIcon, pro_app_artwork, pro_icon},
    },
};

use crate::{
    kar_bridge::KarCommand,
    types::{ChatAccessMode, FeedConnectionState, OfficeKind},
    ui_primitives::{
        DotTone, HEADER_H, MACOS_TRAFFIC_LIGHT_PAD, StudioPalette, clean_display_doc_name,
        render_approval_hand, render_demo_pill, render_doc_outline_icon, render_office_chip,
        render_runtime_mark, render_status_pill, render_tray_folder,
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

        let active_id = self.current_chat_id();
        let header_title: String = match &self.access_mode {
            ChatAccessMode::DemoShowcase => "演示对话沙盒".into(),
            ChatAccessMode::HttpReadOnly { .. } => "HTTP 只读副本".into(),
            _ => active_id
                .and_then(|cid| self.chat_list.iter().find(|e| e.info.id == cid))
                .map(|e| e.title.clone())
                .unwrap_or_else(|| "新对话".into()),
        };

        // Contextual status pill — shown ONLY when state is non-default (zero resting clutter!)
        let contextual_pill = self.compute_contextual_header_pill();

        // Clear macOS traffic lights when sidebar is collapsed
        let left_header_pad = if !self.sidebar_open && cfg!(target_os = "macos") {
            MACOS_TRAFFIC_LIGHT_PAD - 4.
        } else {
            14.
        };

        // 1. Unified 44px Header Bar (every control on a strict 28px height grid)
        let header = div()
            .h(px(HEADER_H))
            .pl(px(left_header_pad))
            .pr(px(12.))
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
                        .when_some(pro_app_artwork("kense", 17., cx), |el, mark| el.child(mark))
                    })
                    .child(
                        div()
                            .min_w_0()
                            .max_w(px(340.))
                            .truncate()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child(header_title),
                    )
                    .when_some(contextual_pill, |d, (lbl, tone)| {
                        d.child(render_status_pill(lbl, tone, &s))
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(self.layout_segmented.clone())
                    .child(self.chat_actions_menu.clone()),
            );

        // Optional local HTTP/SSE publisher strip (only visible while actively serving SSE)
        let http_host_bar = self.active_http_server_url.clone().map(|srv_url| {
            div()
                .px(px(14.))
                .h(px(34.))
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
                        .child(div().size(px(5.5)).rounded_full().bg(s.pro.green))
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
                            "第二窗口观察",
                            Some(ProIcon::ExternalArrow),
                            ProBtnStyle::Secondary,
                            true,
                            cx,
                            |this, _, cx| this.open_subscriber_second_window(cx),
                        ))
                        .child(pro_btn(
                            "btn-stop-srv-inline",
                            "停止广播",
                            None,
                            ProBtnStyle::Ghost,
                            true,
                            cx,
                            |this, _, _| this.bridge.send(KarCommand::StopHttpServer),
                        )),
                )
        });

        // Contextual KUI Inline Alert (full-width inside the 680px column)
        let alert_banner = self.show_alert.then(|| {
            div()
                .w_full()
                .max_w(px(680.))
                .mx_auto()
                .px(px(22.))
                .pt(px(12.))
                .flex_none()
                .child(self.status_alert.clone())
        });

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
                    .max_w(px(680.))
                    .mx_auto()
                    .px(px(22.))
                    .pt(px(20.))
                    .pb(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(26.));
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
                .max_w(px(680.))
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

    /// Restrained, High-Signal Centered Start Screen:
    /// 1. Living KUI `AiSpark` + Clean Single-Line Heading
    /// 2. Monochrome KUI Double-Deck Composer (`max_w(640px)`)
    /// 3. ONE Single Row of 4 Tactile Quick-Launch Capsules (with spring hover indicator)
    fn render_centered_start_stage(
        &self,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let is_demo = matches!(self.access_mode, ChatAccessMode::DemoShowcase);
        let has_pinned = self.pinned_target.is_some();

        let greeting = if is_demo {
            "演示对话沙盒已就绪"
        } else if has_pinned {
            "已引用文档选区，想怎样修改或分析？"
        } else {
            "今天想推进什么？"
        };

        // 4 clean, non-redundant capsules (opens Word / Excel / PPT coworking or 3-way Co-Edit!)
        let chip_sensor = pro_selection_sensor();
        let hovered_key: SharedString = self
            .hovered_starter_chip
            .map(|i| format!("chip-{i}").into())
            .unwrap_or_else(|| "none".into());
        let hovered_idx = self.hovered_starter_chip;

        let quick_actions: [(Option<OfficeKind>, &'static str, &'static str); 4] = [
            (Some(OfficeKind::Word), "会议纪要 .docx", "sample_minutes.docx"),
            (Some(OfficeKind::Excel), "预算公式 .xlsx", "sample_formulas.xlsx"),
            (Some(OfficeKind::Ppt), "产品路演 .pptx", "sample_deck.pptx"),
            (None, "双写冲突演练", "__COEDIT__"),
        ];

        let chip_border = s.border;
        let ink_c = s.ink;
        let text_c = s.text;
        let muted_c = s.muted;

        let capsules_row = div()
            .mt(px(16.))
            .relative()
            .w_full()
            .flex()
            .flex_wrap()
            .justify_center()
            .gap(px(8.))
            .child(pro_pill_indicator(
                "kar-start-chip-hover-pill",
                hovered_key,
                chip_sensor.clone(),
                999.,
                s.subtle_bg,
                None,
                false,
            ))
            .children(
                quick_actions
                    .into_iter()
                    .enumerate()
                    .map(|(ci, (kind_opt, label, target))| {
                        let is_hov = hovered_idx == Some(ci);
                        let sensor_ref = chip_sensor.clone();
                        let icon_el = match kind_opt {
                            Some(k) => render_doc_outline_icon(k, 14., muted_c),
                            None => pro_icon(ProIcon::GitBranch, 13.5, muted_c),
                        };
                        activate(
                            div()
                                .id(SharedString::from(format!("start-capsule-{ci}")))
                                .role(Role::Button)
                                .tab_index(0)
                                .relative()
                                .h(px(30.))
                                .px(px(12.))
                                .rounded_full()
                                .border_1()
                                .border_color(chip_border)
                                .when(is_hov, |d| d.child(pro_mark_selected(&sensor_ref)))
                                .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                                    let next = (*inside).then_some(ci);
                                    if this.hovered_starter_chip != next {
                                        this.hovered_starter_chip = next;
                                        cx.notify();
                                    }
                                }))
                                .flex()
                                .items_center()
                                .gap(px(6.5))
                                .text_size(px(12.5))
                                .text_color(if is_hov { ink_c } else { text_c })
                                .cursor_pointer()
                                .child(icon_el)
                                .child(label),
                            cx,
                            move |this, window, cx| {
                                if target == "__COEDIT__" {
                                    this.enter_coedit_demo_workshop(window, cx);
                                } else {
                                    this.open_working_sample(target, window, cx);
                                }
                            },
                        )
                    }),
            );

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
                    .max_w(px(640.))
                    .px(px(22.))
                    .py(px(36.))
                    .flex()
                    .flex_col()
                    .items_center()
                    // 1. Living KUI AiSpark + Crisp Monochrome Heading
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(10.))
                            .child(self.home_spark.clone())
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .line_height(px(28.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(greeting),
                            )
                            .when(is_demo, |d| d.child(render_demo_pill("演示数据", s))),
                    )
                    // 2. Monochrome KUI Double-Deck Composer
                    .child(
                        div()
                            .mt(px(22.))
                            .w_full()
                            .child(self.render_double_deck_composer(s, cx)),
                    )
                    // 3. Single Row of 4 Tactile Capsules
                    .child(capsules_row),
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
                "当前连接提供 GET /snapshot 与 GET /events 只读视图；新回合与工具事件到达时将实时渲染。",
            )
        } else {
            (
                "只读观察模式",
                "另一窗口持有本会话写入锁；本窗口实时同步全部回合、工具执行与文件变更。",
            )
        };
        div()
            .w_full()
            .min_h(px(320.))
            .max_w(px(520.))
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
                    .size(px(38.))
                    .rounded_full()
                    .bg(s.subtle_bg)
                    .border_1()
                    .border_color(s.border)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(pro_icon(ProIcon::Eye, 16., s.muted)),
            )
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.ink)
                    .child(title),
            )
            .child(
                div()
                    .max_w(px(420.))
                    .text_size(px(12.5))
                    .line_height(px(19.))
                    .text_color(s.muted)
                    .child(sub),
            )
            .into_any_element()
    }

    /// KUI `AssistantHomeScreen` Double-Deck Monochrome Composer (`28px` harmonic control bar).
    fn render_double_deck_composer(&self, s: &StudioPalette, cx: &mut Context<Self>) -> Div {
        let draft = self.composer_editor.read(cx).value().to_owned();
        let line_count = draft
            .split('\n')
            .count()
            .max((draft.chars().count() / 54) + 1)
            .clamp(2, 6);
        let field_h = (line_count as f32) * 21.0;

        let can_type = self.access_mode.is_writable()
            || matches!(self.access_mode, ChatAccessMode::NoneSelected);
        let is_running = self
            .projected
            .turns
            .last()
            .is_some_and(|t| t.status == TurnStatus::Running)
            || self.prompt_attempts.values().any(|a| a.in_flight);
        let can_send = can_type && !is_running && !draft.trim().is_empty();

        // 1. Folded Selection Target Attachment Strip (`pro_fold`)
        let has_pinned = self.pinned_target.is_some();
        let pinned_snapshot = self.pinned_target.clone();
        let s_copy = *s;
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
                let hover_c = s_copy.stage_hover;
                div()
                    .w_full()
                    .h(px(32.))
                    .pl(px(8.))
                    .pr(px(5.))
                    .mb(px(4.))
                    .rounded(px(8.5))
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
                            .gap(px(6.5))
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
                                    .h(px(22.))
                                    .px(px(6.5))
                                    .rounded(px(5.5))
                                    .hover(move |st| st.bg(hover_c))
                                    .flex()
                                    .items_center()
                                    .gap(px(3.))
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s_copy.ink)
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
                                    .child(pro_icon(ProIcon::ExternalArrow, 10.5, s_copy.muted)),
                            )
                            .child(
                                div()
                                    .id("cmp-clear-target")
                                    .size(px(22.))
                                    .rounded(px(5.5))
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
                                    .child(pro_icon(ProIcon::Close, 10.5, s_copy.muted)),
                            ),
                    )
                    .into_any_element()
            },
        );

        // 2. Composer 28px Control Bar
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
            .child(
                div()
                    .id("cmp-plus-btn")
                    .size(px(28.))
                    .rounded(px(7.))
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
                    .child(pro_icon(ProIcon::Plus, 15., s.text)),
            )
            .when(has_office_tab && can_type, |d| {
                d.child(activate(
                    div()
                        .id("cmp-bind-selection")
                        .role(Role::Button)
                        .tab_index(0)
                        .h(px(28.))
                        .px(px(8.5))
                        .rounded(px(7.))
                        .bg(s.subtle_bg)
                        .border_1()
                        .border_color(s.border)
                        .hover(move |st| st.bg(hover_pill))
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.ink)
                        .cursor_pointer()
                        .child(pro_icon(ProIcon::Pin, 12., s.muted))
                        .child("引用选区"),
                    cx,
                    |this, window, cx| this.bind_active_selection_to_chat(window, cx),
                ))
            })
            .when(can_type, |d| {
                d.child(
                    div()
                        .id("cmp-approval-btn")
                        .h(px(28.))
                        .px(px(8.))
                        .rounded(px(7.))
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
                        .child(approval_label),
                )
            });

        let submit_control = if is_running && can_type {
            activate(
                div()
                    .id("cmp-stop-turn")
                    .role(Role::Button)
                    .aria_label("中止当前回合")
                    .tab_index(0)
                    .size(px(28.))
                    .rounded(px(8.))
                    .bg(s.stage_active)
                    .border_1()
                    .border_color(s.border_strong)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(div().size(px(8.5)).rounded(px(2.)).bg(s.ink)),
                cx,
                |this, _, _| this.bridge.send(KarCommand::AbortTurn),
            )
            .into_any_element()
        } else {
            let send_bg = if can_send {
                s.primary_bg
            } else if s.dark {
                hsla(0., 0., 1., 0.08)
            } else {
                hsla(0., 0., 0., 0.06)
            };
            let send_hover = s.accent_hover;
            activate(
                div()
                    .id("cmp-send-turn")
                    .role(Role::Button)
                    .aria_label("发送消息")
                    .tab_index(0)
                    .size(px(28.))
                    .rounded(px(8.))
                    .bg(send_bg)
                    .when(can_send, move |d| {
                        d.hover(move |st| st.bg(send_hover)).cursor_pointer()
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(pro_icon(
                        ProIcon::ArrowUp,
                        15.,
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
            .gap(px(5.))
            .child(
                div()
                    .id("cmp-runtime-picker-btn")
                    .h(px(28.))
                    .pl(px(8.))
                    .pr(px(6.5))
                    .rounded(px(7.))
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
                    .gap(px(5.5))
                    .text_size(px(12.5))
                    .text_color(s.ink)
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
            .child(submit_control);

        let add_pop = self.render_add_context_popover(s, cx);
        let approval_pop = self.render_approval_popover(s, cx);
        let runtime_pop = self.render_runtime_popover(s, cx);

        // Upper Deck (`write_card` — KUI `AssistantHomeScreen` geometry)
        let write_card = div()
            .relative()
            .w_full()
            .rounded(px(12.))
            .bg(s.card_bg)
            .border_1()
            .border_color(if can_type { s.border_strong } else { s.border })
            .shadow(s.composer_shadows())
            .pt(px(10.))
            .px(px(10.))
            .pb(px(8.))
            .flex()
            .flex_col()
            .child(pinned_fold)
            .child(
                div()
                    .w_full()
                    .h(px(field_h))
                    .px(px(4.))
                    .child(self.composer_editor.clone()),
            )
            .child(
                div()
                    .w_full()
                    .mt(px(8.))
                    .h(px(28.))
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

        // Lower Deck (`28px` Whisper-Quiet Context Tray)
        let ws_folder = self
            .config
            .workspace_dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| "workspace".into());

        let active_doc_label: Option<String> = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .map(|t| clean_display_doc_name(&t.doc_name(cx)));

        let mode_note: Option<&'static str> = if !can_type {
            Some("只读已锁定")
        } else if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            Some("演示沙盒")
        } else {
            None
        };

        let tray_footer = div()
            .h(px(28.))
            .px(px(8.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.5))
                    .text_color(s.muted)
                    .child(render_tray_folder(13.5, s.muted))
                    .child(div().min_w_0().truncate().child(ws_folder))
                    .when_some(mode_note, |d, note| {
                        d.child(div().text_color(s.faint).child("·"))
                            .child(div().text_color(s.text).child(note))
                    }),
            )
            .child(activate(
                div()
                    .id("cmp-tray-office-toggle")
                    .role(Role::Button)
                    .tab_index(0)
                    .h(px(22.))
                    .px(px(7.))
                    .rounded_full()
                    .hover(move |st| st.bg(hover_pill))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_size(px(11.5))
                    .text_color(s.muted)
                    .cursor_pointer()
                    .child(pro_icon(ProIcon::PageDoc, 12., s.muted))
                    .child(match active_doc_label {
                        Some(doc) => doc,
                        None => "Office 画布".into(),
                    }),
                cx,
                |this, _, cx| {
                    let next = if this.layout_mode == WorkspaceLayoutMode::ChatFocus {
                        WorkspaceLayoutMode::Split
                    } else {
                        WorkspaceLayoutMode::ChatFocus
                    };
                    this.set_layout_mode(next, cx);
                },
            ));

        div()
            .w_full()
            .rounded(px(15.))
            .bg(s.tray_bg)
            .border_1()
            .border_color(s.border)
            .p(px(4.))
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
                (
                    "open_xlsx",
                    "打开 Excel 预算公式表 (.xlsx)",
                    ProIcon::UsageChart,
                ),
                ("open_pptx", "打开 PPT 产品发布文稿 (.pptx)", ProIcon::Sparkles),
                ("open_coedit", "人机双写协议演练 (演示数据)", ProIcon::GitBranch),
            ];
            div()
                .id("cmp-add-pop-surface")
                .absolute()
                .left(px(8.))
                .bottom(px(42. + 6. * amt))
                .opacity(amt)
                .w(px(248.))
                .p(px(5.))
                .rounded(px(10.))
                .bg(card_bg)
                .border_1()
                .border_color(border_c)
                .shadow(shadows.clone())
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .flex()
                .flex_col()
                .gap(px(1.5))
                .children(items.into_iter().enumerate().filter_map(|(ix, (act, label, ic))| {
                    if act == "bind_sel" && !has_tab {
                        return None;
                    }
                    let weak_c = weak.clone();
                    Some(
                        div()
                            .id(SharedString::from(format!("cmp-add-item-{ix}")))
                            .h(px(30.))
                            .px(px(9.))
                            .rounded(px(7.))
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
                            .child(pro_icon(ic, 13.5, muted_c))
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
                .left(px(40.))
                .bottom(px(42. + 6. * amt))
                .opacity(amt)
                .w(px(268.))
                .p(px(5.))
                .rounded(px(10.))
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
                        .px(px(9.))
                        .py(px(6.5))
                        .rounded(px(7.))
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
                                .gap(px(1.5))
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
                            d.child(div().pt(px(2.)).child(pro_icon(ProIcon::Check, 13., ink_c)))
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
                let mark = render_runtime_mark(id, 13.5, &s_copy, cx);
                (id.to_owned(), runtime_display_name(id), badge, ok, mark)
            })
            .collect();

        pro_presence("cmp-runtime-popover", open, 0.16, 0.12, move |amt| {
            let mut list = div()
                .id("cmp-runtime-pop-surface")
                .absolute()
                .right(px(8.))
                .bottom(px(42. + 6. * amt))
                .opacity(amt)
                .w(px(256.))
                .p(px(5.))
                .rounded(px(10.))
                .bg(s_copy.card_bg)
                .border_1()
                .border_color(s_copy.border_strong)
                .shadow(shadows.clone())
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .flex()
                .flex_col()
                .gap(px(1.5));

            for (ix, (id, disp, badge, ok, mark)) in rows_data.into_iter().enumerate() {
                let is_sel = selected_rt == id;
                let id_owned = id;
                let weak_c = weak.clone();
                let hover_c = s_copy.stage_hover;
                let dot_c = if ok { s_copy.pro.green } else { s_copy.pro.amber };
                list = list.child(
                    div()
                        .id(SharedString::from(format!("cmp-rt-opt-{ix}")))
                        .h(px(30.))
                        .px(px(8.5))
                        .rounded(px(7.))
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
                                .child(mark)
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font_weight(if is_sel {
                                            FontWeight::MEDIUM
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(s_copy.ink)
                                        .child(disp),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .child(div().size(px(5.5)).rounded_full().bg(dot_c))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(s_copy.muted)
                                        .child(badge),
                                )
                                .when(is_sel, |d| {
                                    d.child(pro_icon(ProIcon::Check, 12., s_copy.ink))
                                }),
                        ),
                );
            }

            let hover_diag = s_copy.stage_hover;
            list.child(
                div()
                    .mt(px(3.))
                    .pt(px(3.))
                    .border_t_1()
                    .border_color(s_copy.border_soft)
                    .child(
                        div()
                            .id("cmp-rt-open-health")
                            .h(px(28.))
                            .px(px(8.5))
                            .rounded(px(6.5))
                            .hover(move |st| st.bg(hover_diag))
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(11.5))
                            .text_color(s_copy.text)
                            .cursor_pointer()
                            .on_click(move |_, _, cx| {
                                if let Some(e) = weak_diag.upgrade() {
                                    e.update(cx, |this, cx| {
                                        this.toggle_provider_sheet(cx);
                                    });
                                }
                            })
                            .child("查看完整健康度面板…")
                            .child(pro_icon(ProIcon::ExternalArrow, 11., s_copy.muted)),
                    ),
            )
            .into_any_element()
        })
    }

    /// Return a header status pill ONLY when in a non-default state (read-only, HTTP, reconnecting, demo, or active run).
    fn compute_contextual_header_pill(&self) -> Option<(String, DotTone)> {
        match &self.connection_state {
            FeedConnectionState::Reconnecting { attempt, .. } => {
                return Some((format!("重连中 #{attempt}"), DotTone::Warning));
            }
            FeedConnectionState::Disconnected { .. } => {
                return Some(("连接已断开".into(), DotTone::Danger));
            }
            FeedConnectionState::Connecting { .. } => {
                return Some(("连接中…".into(), DotTone::Info));
            }
            FeedConnectionState::Connected {
                cursor,
                duplicates_ignored,
                ..
            } if self.access_mode.is_http_read_only() => {
                let cur = short_cursor_label(cursor.as_deref().unwrap_or("-"));
                let lbl = if *duplicates_ignored > 0 {
                    format!("只读流 · 游标 {cur} · 去重 {duplicates_ignored}")
                } else {
                    format!("只读流 · 游标 {cur}")
                };
                return Some((lbl, DotTone::Info));
            }
            _ => {}
        }

        match &self.access_mode {
            ChatAccessMode::LocalReadOnlySubscriber { .. } => {
                return Some(("只读观察".into(), DotTone::Warning));
            }
            ChatAccessMode::HttpReadOnly { .. } => {
                return Some(("HTTP 只读".into(), DotTone::Info));
            }
            ChatAccessMode::DemoShowcase => {
                return Some(("演示数据".into(), DotTone::Warning));
            }
            _ => {}
        }

        let in_flight_send = self.prompt_attempts.values().any(|a| a.in_flight);
        if let Some(last) = self.projected.turns.last() {
            if last.status == TurnStatus::Running || in_flight_send {
                let any_tool = last
                    .items
                    .iter()
                    .any(|it| matches!(&it.item, Item::Tool { activity, .. } if !activity.finished));
                return Some(if any_tool {
                    ("工具运行中".into(), DotTone::Info)
                } else {
                    ("生成中".into(), DotTone::Accent)
                });
            }
            if matches!(
                last.status,
                TurnStatus::Failed | TurnStatus::Interrupted | TurnStatus::Aborted
            ) {
                return Some(("已中断".into(), DotTone::Warning));
            }
        } else if in_flight_send {
            return Some(("发送中".into(), DotTone::Accent));
        }

        None
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

        // Selection Target Reference Pill (persists across tab switches)
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
                    .max_w(px(400.))
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(10.))
                    .bg(s.card_bg)
                    .border_1()
                    .border_color(s.border)
                    .hover(move |st| st.bg(hover_c).border_color(border_hi))
                    .flex()
                    .flex_col()
                    .gap(px(2.5))
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
                                    .gap(px(6.))
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
                                    .text_color(s.muted)
                                    .child("定位")
                                    .child(pro_icon(ProIcon::ExternalArrow, 10.5, s.muted)),
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

        let mut user_blocks = div().w_full().flex().flex_col().items_end().gap(px(6.));
        if let Some(chip) = target_ref {
            user_blocks = user_blocks.child(chip);
        }

        let mut agent_blocks = div()
            .w_full()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(10.));
        let mut has_agent_items = false;
        let mut tool_file_changes: Vec<(String, &'static str)> = Vec::new();

        let rt_id = self
            .projected
            .info
            .as_ref()
            .map(|i| i.runtime.as_str())
            .unwrap_or(&self.selected_runtime);
        let agent_name = if self.projected.is_demo {
            "Kar 演示"
        } else {
            runtime_display_name(rt_id)
        };

        // Assistant Author Row: KUI `AiSpark` (14.5px monochrome) + Provider Name
        let spark_el = self.turn_sparks.get(&turn_id).cloned();
        agent_blocks = agent_blocks.child(
            div()
                .h(px(22.))
                .flex()
                .items_center()
                .gap(px(7.))
                .when_some(spark_el, |d, sp| d.child(sp))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.ink)
                        .child(agent_name),
                )
                .when(self.projected.is_demo, |d| d.child(render_demo_pill("演示数据", s))),
        );

        // Live KUI `WorkBadge` while running (left-aligned!)
        if turn.status == TurnStatus::Running
            && let Some(work_badge) = self.turn_badges.get(&turn_id)
        {
            has_agent_items = true;
            agent_blocks = agent_blocks.child(div().w_full().child(work_badge.clone()));
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
                            .px(px(14.))
                            .py(px(9.))
                            .rounded(px(14.))
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
                            .text_size(px(14.))
                            .line_height(px(22.5))
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

        // Recorded Workspace & Office File Change Chips
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
            let mut chips = div().pt(px(2.)).flex().flex_wrap().items_center().gap(px(6.));
            for (rel_path, kind_label) in all_files {
                let full = ws_root.join(&rel_path);
                let office_kind = OfficeKind::for_path(Path::new(&rel_path));
                let can_open = office_kind.is_some() && full.is_file();
                let tag_color = match kind_label {
                    "新增" => s.pro.green_ink,
                    "删除" => s.pro.red_ink,
                    _ => s.muted,
                };
                let hover_c = s.stage_hover;
                let pill = div()
                    .id(SharedString::from(format!("fc-{turn_id}-{rel_path}")))
                    .h(px(28.))
                    .pl(px(8.))
                    .pr(px(9.))
                    .rounded(px(7.5))
                    .bg(s.card_bg)
                    .border_1()
                    .border_color(s.border)
                    .flex()
                    .items_center()
                    .gap(px(6.5))
                    .text_size(px(12.))
                    .when_some(office_kind, |d, ok| d.child(render_office_chip(ok, true, s)))
                    .when(office_kind.is_none(), |d| {
                        d.child(pro_icon(ProIcon::PageDoc, 12.5, s.muted))
                    })
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child(rel_path.clone()),
                    )
                    .child(div().text_size(px(11.)).text_color(tag_color).child(kind_label))
                    .when(can_open, |d| {
                        d.child(pro_icon(ProIcon::ExternalArrow, 11., s.muted))
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
                    .py(px(8.5))
                    .rounded(px(9.))
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

        // Razor-Sharp 26px Turn Footer: 26x26 KUI `CopyButton` + Subtle Meta + Same-UUID Retry
        let copy_btn = self.copy_buttons.get(&turn_id).cloned();
        let status_note = match turn.status {
            TurnStatus::Interrupted => Some("已中断"),
            TurnStatus::Aborted => Some("已中止"),
            TurnStatus::Failed => Some("执行失败"),
            _ => None,
        };

        let footer_actions = div()
            .h(px(26.))
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .when_some(copy_btn, |d, cb| d.child(cb))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(s.faint)
                            .child(format!("回合 {turn_number:02} · {short_uuid}")),
                    )
                    .when_some(status_note, |d, st| {
                        d.child(
                            div()
                                .text_size(px(11.))
                                .text_color(s.pro.amber_ink)
                                .child(format!("· {st}")),
                        )
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
            .gap(px(14.))
            .child(user_blocks)
            .when(has_agent_items || turn.status == TurnStatus::Running, |d| {
                d.child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(agent_blocks)
                        .child(footer_actions),
                )
            })
    }

    fn render_permission_dock(&self, s: &StudioPalette, cx: &mut Context<Self>) -> Div {
        let mut stack = div()
            .w_full()
            .max_w(px(680.))
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
                    .p(px(11.))
                    .rounded(px(12.))
                    .bg(s.card_bg)
                    .border_1()
                    .border_color(s.border_strong)
                    .shadow(s.composer_shadows())
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
                            .gap(px(9.))
                            .child(
                                div()
                                    .size(px(28.))
                                    .rounded(px(7.))
                                    .bg(s.pro.amber_soft)
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(pro_icon(ProIcon::Shield, 14., s.pro.amber_ink)),
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
                                            .text_color(s.ink)
                                            .child(format!("工具执行需要确认 · {}", req.method)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
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
    if raw.len() > 18 {
        format!("{}…", &raw[..18])
    } else {
        raw.to_owned()
    }
}
