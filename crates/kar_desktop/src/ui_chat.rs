//! Native conversation timeline, file review, decisions and writing controls.

use std::path::PathBuf;
use std::time::Duration;


use gpui::{prelude::*, *};
use gpui_kit::component::text::TextView;
use kar::{
    ApprovalMode, ChangeKind as ToolChangeKind, FieldKind, PendingRequest, PermissionDecision,
    PolicyOption, QuestionReply, RequestKind, TaskStatus,
    chat::{
        AttachmentKind, ChangeKind as CheckpointChangeKind, ChatTurn, Item, Role as KarRole,
        TurnStatus,
    },
    observe::Tool,
};
use kui::{
    components::{
        code_block::CodeBlock,
        code_suggestion::{Decision as PatchDecision, Hunk as PatchHunk, PatchReview},
        hold_to_confirm::{HoldEvent, HoldToConfirm},
        json_viewer::JsonViewer,
        pro_chrome::{
            pro_fold, pro_presence, pro_progress_ring, pro_spring_f32, pro_view_transition_fill,
        },
    },
    primitives::{
        editor::{EditEvent, Editor, InputMode},
        ops_surface::{arrive, swap_label},
        pro_icons::{ProIcon, pro_icon},
    },
};
use uuid::Uuid;

use crate::{
    kar_bridge::KarCommand,
    studio_hub::parse_unified_diff_hunks,
    types::{
        ChatAccessMode, DiffLine, FeedConnectionState, OfficeKind, ProviderReadiness, ProviderStatusInfo,
        SelectionTarget, ToolPhase, describe_failure, format_tokens_compact,
        parse_diff, runtime_display_name, strip_selection_envelope,
        tool_input_text, tool_output_text, tool_phase, tool_summary, unwrap_shell_command,
    },
    ui_motion::{
        MenuPlacement, Tactile, disclosure_body, disclosure_line, icon_button, menu_caption,
        menu_check, menu_divider, menu_hint, menu_host, menu_icon_item, menu_item, primary_button,
        pulse_beacon, soft_button, spinner, text_button,
    },
    ui_primitives::{
        CTRL_H, DETAILS_CARD_W, DotTone, FS_META, FS_READ, FS_TAG, FS_UI, HEADER_H, ICON,
        ICON_SM, ICON_XS, LH_META, LH_READ, LH_UI, R_CARD, R_CTRL, R_HERO, R_PILL, StudioPalette, UiLocale,
        THREAD_PAD, THREAD_W, clean_display_doc_name, macos_traffic_pad, mono_font,
        render_approval_hand, render_bg_layers_icon, render_doc_outline_icon,
        render_file_diff_icon, render_folder_git_icon, render_folder_icon, render_git_branch_icon,
        render_machine_icon, render_office_chip, render_panel_bottom_icon, render_panel_right_icon,
        render_project_monogram, render_runtime_mark, render_schedule_repeat_icon,
        render_skills_puzzle_icon, render_square_menu_icon, render_square_pen_icon, render_status,
        render_steer_merge_icon, tnum_features,
    },
    ui_settings::SettingsTab,
    workbench::{
        KarWorkbench, Menu, Notice, NoticeAction, NoticeTone, QueuedPrompt, WorkspaceLayoutMode,
    },
};

/// Most lines of a tool's output or a file change shown before it is cut short.
const DETAIL_MAX_LINES: usize = 24;

impl KarWorkbench {
    pub(crate) fn render_chat_pane(
        &mut self,
        pane_w: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let s = StudioPalette::get(cx);

        let view_only = !self.access_mode.is_writable()
            && !matches!(self.access_mode, ChatAccessMode::NoneSelected);
        let on_start_screen = self.projected.turns.is_empty()
            && self.active_briefs.is_empty()
            && self.queued_prompt.is_none()
            && !view_only;
        let is_working = self.turn_in_progress();
        self.working_shimmer
            .update(cx, |sh, cx| sh.set_active(is_working, cx));

        let show_review_rail = !on_start_screen
            && self.layout_mode == WorkspaceLayoutMode::ChatFocus
            && pane_w >= 720.
            && self
                .turn_review
                .as_ref()
                .is_some_and(|review| Some(review.key.chat) == self.current_chat_id());
        let review_width = if show_review_rail {
            (pane_w * (1. - self.split_ratio)).clamp(300., pane_w - 300.)
        } else { 0. };
        let show_details_rail = !show_review_rail
            && self.thread_details_open
            && self.layout_mode == WorkspaceLayoutMode::ChatFocus
            && pane_w >= 660.;
        let center_w = if show_review_rail {
            pane_w - review_width
        } else if show_details_rail {
            (pane_w - DETAILS_CARD_W - 12.).max(360.)
        } else {
            pane_w
        };

        let header = self.render_chat_header(
            pane_w,
            on_start_screen,
            show_details_rail,
            show_review_rail,
            &s,
            window,
            cx,
        );
        let share_bar = self.render_share_bar(&s, cx);
        let goal_bar = self.render_goal_bar(&s, cx);
        let notice = self.render_notice(&s, cx);

        let stage = if on_start_screen {
            self.render_start_stage(center_w, &s, window, cx)
                .into_any_element()
        } else {
            let thread = if self.projected.turns.is_empty()
                && self.active_briefs.is_empty()
                && self.queued_prompt.is_none()
            {
                self.render_waiting_for_content(&s)
            } else {
                let turns = self.projected.turns.clone();
                let mut column = chat_content_lane(center_w, window)
                    .pt(px(24.))
                    .pb(px(32.))
                    .flex()
                    .flex_col()
                    .gap(px(32.));
                for (turn_idx, turn) in turns.iter().enumerate() {
                    column = column.child(arrive(
                        self.render_turn(turn_idx, turn, !show_review_rail, &s, window, cx),
                        ElementId::Name(format!("turn-in-{}", turn.id).into()),
                        0,
                        6.,
                        cx,
                    ));
                }
                if let Some(briefs_el) = self.render_brief_cards_for_chat(&s, window, cx) {
                    column = column.child(briefs_el);
                }
                if let Some(queued) = self.queued_prompt.clone() {
                    column = column.child(arrive(
                        self.render_queued_turn(&queued, &s, window, cx),
                        "turn-in-queued",
                        0,
                        6.,
                        cx,
                    ));
                }
                if let Some(tasks_el) = self.render_active_tasks(&s, window, cx) {
                    column = column.child(tasks_el);
                }

                let show_followups = !is_working
                    && !view_only
                    && !self.projected.turns.is_empty()
                    && self.composer_editor.read(cx).value().trim().is_empty()
                    && self.projected.pending_requests.is_empty()
                    && self.live_prompt_suggestion.is_some();
                if show_followups {
                    let mut strip = div()
                        .pt(px(4.))
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(6.));
                    if let Some(ref sugg) = self.live_prompt_suggestion
                        && !sugg.text.trim().is_empty()
                    {
                        let prompt_txt = sugg.text.clone();
                        let label_txt: String = {
                            let chars: String = prompt_txt.chars().take(44).collect();
                            if prompt_txt.chars().count() > 44 {
                                format!("{chars}…")
                            } else {
                                chars
                            }
                        };
                        strip = strip.child(
                            Tactile::soft("followup-live-kar-sugg", &s)
                                .fills(s.pro.accent_soft, s.pro.accent_soft)
                                .h(px(26.))
                                .px(px(10.))
                                .radius(R_PILL)
                                .gap(px(5.))
                                .text_size(px(FS_TAG))
                                .font_weight(FontWeight::MEDIUM)
                                .ink(s.pro.accent_info, s.pro.accent_info)
                                .child(pro_icon(ProIcon::Sparkles, 12., s.pro.accent_info))
                                .child(label_txt)
                                .on_press(cx, move |this, window, cx| {
                                    this.live_prompt_suggestion = None;
                                    this.fill_starter_prompt(&prompt_txt, window, cx);
                                }),
                        );
                    }
                    column = column.child(strip);
                }

                column.into_any_element()
            };

            let scroll_remaining =
                f32::from(self.chat_scroll.max_offset().y) + f32::from(self.chat_scroll.offset().y);
            let show_jump_bottom = scroll_remaining > 120. && !self.projected.turns.is_empty();

            let dock = self.render_decision_dock(&s, cx);
            let foot = chat_content_lane(center_w, window)
                .relative()
                .pt(px(8.))
                .pb(px(20.))
                .flex_none()
                .when(show_jump_bottom, |d| {
                    d.child(
                        div()
                            .mb(px(4.))
                            .left_0()
                            .right_0()
                            .flex()
                            .justify_center()
                            .child(
                                Tactile::soft("chat-jump-bottom", &s)
                                    .fills(s.card, s.hover)
                                    .radius(R_PILL)
                                    .h(px(28.))
                                    .px(px(12.))
                                    .gap(px(5.))
                                    .border_1()
                                    .border_color(s.line)
                                    .text_size(px(FS_META))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child("跳至最新")
                                    .child(pro_icon(ProIcon::ChevronDown, ICON_XS, s.muted))
                                    .on_press(cx, |this, _, cx| {
                                        this.chat_scroll.scroll_to_bottom();
                                        cx.notify();
                                    }),
                            ),
                    )
                })
                .child(if view_only {
                    self.render_view_only_bar(&s, cx)
                } else {
                    self.render_composer(center_w, &s, window, cx)
                        .into_any_element()
                });

            let center_lane = div()
                .flex_1()
                .min_w_0()
                .h_full()
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
                        .child(thread),
                )
                .child(dock)
                .child(foot);

            center_lane.into_any_element()
        };
        let stage = if show_review_rail && let Some(review) = self.turn_review.as_ref() {
            div()
                .size_full()
                .flex()
                .child(div().flex_1().min_w_0().h_full().child(stage))
                .child(
                    div()
                        .w(px(review_width))
                        .h_full()
                        .flex_none()
                        .border_l_1()
                        .border_color(s.line_soft)
                        .child(self.render_file_review(review, true, &s, cx)),
                )
                .into_any_element()
        } else if show_details_rail {
            div()
                .size_full()
                .flex()
                .child(div().flex_1().min_w_0().h_full().child(stage))
                .child(
                    div()
                        .w(px(DETAILS_CARD_W + 12.))
                        .h_full()
                        .flex_none()
                        .pt(px(14.))
                        .pr(px(16.))
                        .pl(px(4.))
                        .child(self.render_thread_details_card(&s, window, cx)),
                )
                .into_any_element()
        } else {
            stage
        };

        div()
            .id("kar-chat-drop-zone")
            .size_full()
            .bg(s.surface)
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.handle_dropped_paths(paths.paths(), window, cx);
            }))
            .child(header)
            .child(share_bar)
            .child(goal_bar)
            .child(notice)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(pro_view_transition_fill(
                        "kar-chat-stage",
                        if on_start_screen { "start" } else { "thread" },
                        stage,
                    )),
            )
            .into_any_element()
    }

    fn render_chat_header(
        &self,
        pane_w: f32,
        on_start_screen: bool,
        show_details_rail: bool,
        show_review_rail: bool,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let chat_id = self.current_chat_id();
        let title: String = match &self.access_mode {
            ChatAccessMode::DemoShowcase => loc
                .pick(
                    "重连时自动同步离线队列修改",
                    "Sync offline edits on reconnect",
                )
                .into(),
            ChatAccessMode::HttpReadOnly { .. } => {
                loc.pick("共享的对话", "Shared live thread").into()
            }
            ChatAccessMode::RemoteController { host_name, .. } => {
                format!("{} {host_name}", loc.pick("远端：", "Remote:"))
            }
            _ => chat_id
                .and_then(|id| self.chat_list.iter().find(|e| e.info.id == id))
                .map(|e| {
                    if e.turn_count == 0 && e.title == "新对话" {
                        loc.pick("新对话", "New thread").to_owned()
                    } else {
                        e.title.clone()
                    }
                })
                .or_else(|| {
                    self.queued_prompt
                        .as_ref()
                        .map(|q| q.text.lines().next().unwrap_or_default().to_owned())
                })
                .unwrap_or_else(|| loc.pick("新对话", "New thread").into()),
        };
        let assoc_badge = chat_id.and_then(|id| self.client_reducer.association_for(&id).cloned());
        let remote_active = self.active_http_server_url.is_some()
            || self.remote_telemetry.listen_url.is_some()
            || matches!(self.access_mode, ChatAccessMode::RemoteController { .. });
        let status = self.chat_status();
        let pointer_controls = !window.last_input_was_keyboard() && !window.last_input_was_touch();
        let docs_shown = show_review_rail || self.layout_mode != WorkspaceLayoutMode::ChatFocus;
        let has_own_chat = matches!(
            self.access_mode,
            ChatAccessMode::WriteController { .. }
                | ChatAccessMode::LocalReadOnlySubscriber { .. }
                | ChatAccessMode::ArchivedReadOnly { .. }
        );

        let rail_hidden = (1. - self.sidebar_reveal).clamp(0., 1.);
        let base_pad = 20.0;
        let traffic_extra = (macos_traffic_pad(window) - base_pad).max(0.0);
        let left_pad = base_pad + traffic_extra * rail_hidden;
        let collapsed_slot_w = 60.0 * rail_hidden;

        let ws = self.effective_workspace_dir();
        let proj_name = if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            "kar-studio".to_owned()
        } else {
            chat_id
                .and_then(|cid| self.project_registry.project_for_chat(&ws, cid))
                .map(|p| p.name.clone())
                .or_else(|| ws.file_name().and_then(|n| n.to_str()).map(str::to_owned))
                .unwrap_or_else(|| "workspace".into())
        };

        let project_crumb = Tactile::ghost("chat-project-crumb", s)
            .h(px(28.))
            .min_w_0()
            .gap(px(6.))
            .tip(loc.pick("在此项目中新建对话", "New thread in this project"))
            .child(render_project_monogram(&proj_name, 16., s, cx))
            .child(
                div()
                    .max_w(px((pane_w * 0.18).clamp(44., 160.)))
                    .truncate()
                    .text_size(px(14.))
                    .text_color(s.muted)
                    .child(proj_name),
            )
            .on_press(cx, move |this, win, cx| {
                this.select_project_and_new_chat(ws.clone(), win, cx)
            });

        let title_for_rename = title.clone();
        let title_el: AnyElement = if let Some(cid) = chat_id.filter(|_| has_own_chat) {
            Tactile::ghost("chat-header-rename", s)
                .h(px(26.))
                .px(px(5.))
                .mx(px(-5.))
                .min_w_0()
                .tip(loc.pick("点击重命名对话", "Click to rename thread"))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.ink)
                        .child(title),
                )
                .on_press(cx, move |this, window, cx| {
                    this.open_rename_chat_dialog(cid, &title_for_rename, window, cx);
                })
                .into_any_element()
        } else {
            div()
                .min_w_0()
                .truncate()
                .text_size(px(13.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(s.ink)
                .child(title)
                .into_any_element()
        };

        div()
            .h(px(HEADER_H))
            .pl(px(left_pad))
            .pr(px(13.))
            .window_control_area(WindowControlArea::Drag)
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .child(
                div()
                    .group("chat-title-actions")
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .when(rail_hidden > 0.01, |d| {
                        d.child(
                            div()
                                .w(px(collapsed_slot_w))
                                .flex_none()
                                .overflow_hidden()
                                .opacity(rail_hidden)
                                .flex()
                                .items_center()
                                .gap(px(2.))
                                .child(
                                    icon_button(
                                        "chat-show-sidebar",
                                        ProIcon::SidebarLeft,
                                        loc.pick("展开侧边栏  ⌘.", "Expand sidebar  ⌘."),
                                        s,
                                    )
                                    .on_press(cx, |this, _, cx| this.set_sidebar_open(true, cx)),
                                )
                                .child(
                                    Tactile::ghost("chat-new", s)
                                        .size(px(CTRL_H))
                                        .justify_center()
                                        .tip(loc.pick("新建对话  ⌘N", "New thread  ⌘N"))
                                        .child(render_square_pen_icon(15., s.muted))
                                        .on_press(cx, |this, window, cx| {
                                            this.start_new_chat(window, cx)
                                        }),
                                ),
                        )
                    })
                    .child(project_crumb)
                    .child(div().text_size(px(13.)).text_color(s.faint).child("/"))
                    .child(title_el)
                    .when(!on_start_screen, |d| {
                        d.child(menu_host(
                            Tactile::ghost("chat-more", s)
                                .size(px(24.))
                                .radius(5.)
                                .justify_center()
                                .lit(self.open_menu == Some(Menu::ChatMore))
                                .tip(loc.pick("对话与接力选项", "Thread actions"))
                                .child(pro_icon(ProIcon::MoreHorizontal, 14., s.muted))
                                .on_press(cx, |this, _, cx| this.toggle_menu(Menu::ChatMore, cx)),
                            self.render_chat_more_menu(s, cx),
                        ).when(pointer_controls && self.open_menu != Some(Menu::ChatMore), |menu| {
                            menu.opacity(0.).group_hover("chat-title-actions", |show| show.opacity(1.))
                        }))
                    })
                    .when_some(assoc_badge, |d, assoc| {
                        d.child(
                            Tactile::soft("chat-assoc-pill", s)
                                .h(px(21.))
                                .px(px(7.))
                                .radius(R_PILL)
                                .gap(px(4.))
                                .text_size(px(FS_TAG))
                                .text_color(s.pro.accent_info)
                                .child(render_schedule_repeat_icon(11., s.pro.accent_info))
                                .child(assoc.label)
                                .on_press(cx, |this, _, cx| {
                                    this.open_settings(SettingsTab::Schedules, cx)
                                }),
                        )
                    })
                    .when_some(status, |d, (label, tone)| {
                        d.child(render_status(label, tone, s))
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .when_some(self.presence_stack.clone(), |d, ps| {
                        d.child(div().mr(px(4.)).child(ps))
                    })
                    .when(remote_active, |d| {
                        d.child(
                            Tactile::soft("chat-remote-live-pill", s)
                                .fills(s.pro.green_soft, s.pro.green_soft)
                                .radius(R_PILL)
                                .h(px(24.))
                                .px(px(8.))
                                .gap(px(5.))
                                .text_size(px(FS_TAG))
                                .text_color(s.pro.green_ink)
                                .child(pulse_beacon(s.pro.green, 5., true, window, cx))
                                .child(loc.pick("已配对", "Paired"))
                                .on_press(cx, |this, _, cx| {
                                    this.open_settings(SettingsTab::Remote, cx)
                                }),
                        )
                    })
                    // 1. `SquareMenu` (`[≡]`) -> Toggle `ThreadDetailsCard` (active threads)
                    .when(pane_w >= 660., |d| {
                        d.child(
                            Tactile::ghost("chat-toggle-details-card", s)
                                .size(px(28.))
                                .justify_center()
                                .lit(show_details_rail)
                                .tip(loc.pick("会话环境与 Git 详情卡", "Toggle thread details"))
                                .child(render_square_menu_icon(
                                    15.5,
                                    if show_details_rail { s.ink } else { s.muted },
                                ))
                                .on_press(cx, move |this, _, cx| {
                                    if show_review_rail {
                                        this.turn_review = None;
                                        this.thread_details_open = false;
                                    }
                                    this.toggle_thread_details(cx);
                                }),
                        )
                    })
                    // 2. `PanelBottom` (`[⬒]`) -> Toggle Bottom Resizable Terminal Drawer
                    .child(
                        Tactile::ghost("chat-toggle-bottom-terminal", s)
                            .size(px(28.))
                            .justify_center()
                            .lit(self.bottom_terminal_open)
                            .tip(loc.pick("底部集成终端  ⇧⌘T", "Toggle bottom terminal  ⇧⌘T"))
                            .child(render_panel_bottom_icon(
                                15.5,
                                if self.bottom_terminal_open {
                                    s.ink
                                } else {
                                    s.muted
                                },
                            ))
                            .on_press(cx, |this, _, cx| this.toggle_bottom_terminal(cx)),
                    )
                    // 3. `PanelRight` (`[◫]`) -> Toggle Right Split Panel (Diff / Files / Office)
                    .child(
                        Tactile::ghost("chat-toggle-docs", s)
                            .size(px(28.))
                            .justify_center()
                            .lit(docs_shown)
                            .tip(loc.pick(
                                "右侧面板（差异 / 文件 / 文档）  ⌘J",
                                "Toggle right panel  ⌘J",
                            ))
                            .child(render_panel_right_icon(
                                15.5,
                                if docs_shown { s.ink } else { s.muted },
                            ))
                            .on_press(cx, move |this, win, cx| {
                                if show_review_rail {
                                    this.close_file_review(win, cx);
                                } else {
                                    this.toggle_document_pane(cx);
                                }
                            }),
                    ),
            )
    }

    // ── T3 Code `ThreadDetailsCard` (`280px` Top-Right Floating Card) ────────

    fn render_thread_details_card(
        &self,
        s: &StudioPalette,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let chat_id = self.current_chat_id();
        let host_label = match &self.access_mode {
            ChatAccessMode::RemoteController { host_name, .. } => host_name.clone(),
            ChatAccessMode::DemoShowcase => "MacStudio".into(),
            _ => loc.pick("本机", "Local computer").into(),
        };

        let wt_branch = chat_id
            .and_then(|cid| self.project_registry.worktree_for(cid))
            .map(|wt| wt.branch.clone());
        let branch_name = if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            "feat/offline-sync".to_owned()
        } else {
            wt_branch
                .or_else(|| self.git_snapshot.current_branch.clone())
                .unwrap_or_else(|| "HEAD".into())
        };
        let (added_n, deleted_n) = if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            (71usize, 2usize)
        } else {
            self.git_snapshot.line_stats()
        };
        let dirty_count = self.git_snapshot.dirty_count();

        let open_in_row = menu_host(
            Tactile::ghost("details-open-in-btn", s)
                .w_full()
                .h(px(32.))
                .px(px(10.))
                .justify_between()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(9.))
                        .child(pro_icon(ProIcon::ExternalArrow, 14., s.muted))
                        .child(
                            div()
                                .text_size(px(FS_UI))
                                .text_color(s.text)
                                .child(loc.pick("在编辑器或画布打开", "Open in editor")),
                        ),
                )
                .child(pro_icon(ProIcon::ChevronDown, 11., s.muted))
                .lit(self.open_menu == Some(Menu::DetailsOpenIn))
                .on_press(cx, |this, _, cx| this.toggle_menu(Menu::DetailsOpenIn, cx)),
            self.render_details_open_in_menu(s, cx),
        );

        let scripts_row = menu_host(
            Tactile::ghost("details-scripts-btn", s)
                .w_full()
                .h(px(32.))
                .px(px(10.))
                .gap(px(9.))
                .child(pro_icon(ProIcon::Plus, 14., s.muted))
                .child(
                    div()
                        .text_size(px(FS_UI))
                        .text_color(s.text)
                        .child(loc.pick("运行项目脚本", "Add project script")),
                )
                .lit(self.open_menu == Some(Menu::DetailsScripts))
                .on_press(cx, |this, _, cx| this.toggle_menu(Menu::DetailsScripts, cx)),
            self.render_details_scripts_menu(s, cx),
        );

        let branch_row = menu_host(
            Tactile::ghost("details-branch-btn", s)
                .w_full()
                .h(px(32.))
                .px(px(10.))
                .justify_between()
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(9.))
                        .child(render_git_branch_icon(14., s.muted))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(FS_UI))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(s.ink)
                                .child(branch_name.clone()),
                        ),
                )
                .child(pro_icon(ProIcon::ChevronDown, 11., s.muted))
                .lit(self.open_menu == Some(Menu::GitBranch))
                .on_press(cx, |this, _, cx| this.toggle_menu(Menu::GitBranch, cx)),
            self.render_git_branch_menu(
                &self.git_snapshot,
                &branch_name,
                MenuPlacement::BelowEnd,
                s,
                cx,
            ),
        );

        let card = div()
            .w_full()
            .rounded(px(R_HERO))
            .bg(s.card)
            .border_1()
            .border_color(s.line)
            .p(px(6.))
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(
                div()
                    .w_full()
                    .h(px(32.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .child(render_machine_icon(14.5, s.muted))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(FS_UI))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(host_label),
                            ),
                    )
                    .child(render_folder_git_icon(14., s.muted)),
            )
            .child(open_in_row)
            .child(scripts_row)
            .child(div().my(px(4.)).mx(px(8.)).h(px(1.)).bg(s.line))
            .when(self.git_snapshot.is_repo, |d| {
                d.child(branch_row).child(
                    Tactile::ghost("details-open-changes", s)
                        .w_full()
                        .h(px(32.))
                        .px(px(10.))
                        .justify_between()
                        .tip(loc.pick("在右侧打开完整差异审查  ⇧⌘G", "Open diff panel  ⇧⌘G"))
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(render_file_diff_icon(14., s.muted))
                                .child(
                                    div()
                                        .text_size(px(FS_UI))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(s.ink)
                                        .child(loc.pick("变更", "Changes")),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(FS_META))
                                        .text_color(s.muted)
                                        .child(loc.pick("工作区", "Working tree")),
                                ),
                        )
                        .child(if added_n > 0 || deleted_n > 0 {
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .font_features(tnum_features())
                                .text_size(px(FS_META))
                                .font_weight(FontWeight::MEDIUM)
                                .child(
                                    div()
                                        .text_color(s.pro.green_ink)
                                        .child(format!("+{added_n}")),
                                )
                                .child(
                                    div()
                                        .text_color(s.pro.red_ink)
                                        .child(format!("-{deleted_n}")),
                                )
                                .into_any_element()
                        } else {
                            div()
                                .font_features(tnum_features())
                                .text_size(px(FS_META))
                                .text_color(s.muted)
                                .child(format!("{dirty_count}"))
                                .into_any_element()
                        })
                        .on_press(cx, |this, _, cx| this.open_git_workbench_tab(cx)),
                )
            })
            .into_any_element();

        pro_presence("kar-thread-details-card", true, 5., 0.985, move |_| card)
    }

    fn render_details_open_in_menu(
        &self,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let ws = self.effective_workspace_dir();
        let ws_copy = ws.display().to_string();
        let rows: Vec<AnyElement> = vec![
            menu_item(
                "det-open-files",
                Some(render_folder_icon(ICON, s.muted)),
                loc.pick("在右侧浏览项目文件树", "Browse workspace files"),
                s,
            )
            .child(menu_hint("⇧⌘E", s))
            .on_press(cx, |this, _, cx| {
                this.open_menu = None;
                this.open_files_dock_tab(cx);
            })
            .into_any_element(),
            menu_item(
                "det-open-office",
                Some(render_doc_outline_icon(OfficeKind::Word, ICON, s.muted)),
                loc.pick("打开本地 Office 文档…", "Open Office document…"),
                s,
            )
            .child(menu_hint("⌘O", s))
            .on_press(cx, |this, win, cx| {
                this.open_menu = None;
                this.pick_documents(win, cx);
            })
            .into_any_element(),
            menu_divider(s).into_any_element(),
            menu_icon_item(
                "det-copy-path",
                ProIcon::Copy,
                loc.pick("复制工作区目录路径", "Copy workspace path"),
                s,
            )
            .on_press(cx, move |this, _, cx| {
                this.open_menu = None;
                cx.write_to_clipboard(ClipboardItem::new_string(ws_copy.clone()));
                this.notify_toast(
                    loc.pick("工作区路径已复制", "Workspace path copied"),
                    cx,
                );
            })
            .into_any_element(),
        ];
        self.render_menu(Menu::DetailsOpenIn, MenuPlacement::BelowEnd, 224., rows, s, cx)
    }

    fn render_details_scripts_menu(
        &self,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let mut rows: Vec<AnyElement> = vec![
            menu_caption(
                loc.pick("在底部终端运行脚本", "Run in bottom terminal"),
                s,
            )
            .into_any_element(),
        ];
        for (ix, (label, cmd)) in [
            ("cargo check", "cargo check"),
            ("cargo test", "cargo test"),
            ("git status -sb", "git status -sb"),
            ("ls -la", "ls -la"),
        ]
        .into_iter()
        .enumerate()
        {
            rows.push(
                menu_icon_item(("det-script", ix), ProIcon::Terminal, label, s)
                    .on_press(cx, move |this, win, cx| {
                        this.open_menu = None;
                        this.set_bottom_terminal_open(true, cx);
                        this.run_terminal_command_str(cmd, win, cx);
                    })
                    .into_any_element(),
            );
        }
        self.render_menu(Menu::DetailsScripts, MenuPlacement::BelowEnd, 212., rows, s, cx)
    }

    fn render_chat_more_menu(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let sharing = self.active_http_server_url.is_some();
        let writable = matches!(self.access_mode, ChatAccessMode::WriteController { .. });
        let idle = !self.turn_in_progress();
        let can_compact =
            writable && idle && self.projected.can_compact && self.projected.turns.len() >= 2;
        let can_goals = writable && self.projected.can_goals;
        let chat_id = self.current_chat_id();
        let is_pinned = chat_id.is_some_and(|cid| self.project_registry.is_pinned(cid));
        let current_title = chat_id
            .and_then(|id| self.chat_list.iter().find(|e| e.info.id == id))
            .map(|e| e.title.clone())
            .unwrap_or_default();
        let del_title = if current_title.is_empty() {
            "当前对话".to_owned()
        } else {
            current_title.clone()
        };

        let mut rows: Vec<AnyElement> = Vec::new();
        if let Some(usage_summary) = self.usage_summary_line() {
            rows.push(menu_caption(usage_summary, s).into_any_element());
            rows.push(menu_divider(s).into_any_element());
        }

        if let Some(cid) = chat_id {
            let title_clone = current_title;
            rows.push(
                menu_icon_item(
                    "chat-more-pin",
                    ProIcon::Pin,
                    if is_pinned {
                        "取消置顶对话"
                    } else {
                        "置顶此对话"
                    },
                    s,
                )
                .on_press(cx, move |this, _, cx| {
                    this.open_menu = None;
                    this.toggle_pin_chat(cid, cx);
                })
                .into_any_element(),
            );
            rows.push(
                menu_icon_item("chat-more-rename", ProIcon::Note, "重命名对话…", s)
                    .on_press(cx, move |this, window, cx| {
                        this.open_rename_chat_dialog(cid, &title_clone, window, cx);
                    })
                    .into_any_element(),
            );
            rows.push(menu_divider(s).into_any_element());
        }

        rows.push(
            menu_icon_item("chat-more-handoff", ProIcon::ExternalArrow, "跨智能体接力切换…", s)
                .on_press(cx, |this, _, cx| this.toggle_handoff_sheet(cx))
                .into_any_element(),
        );
        rows.push(
            menu_icon_item("chat-more-side-q", ProIcon::Sparkles, "旁路快问（不入上下文）…", s)
                .on_press(cx, |this, _, cx| this.toggle_side_question_sheet(cx))
                .into_any_element(),
        );

        if self.projected.can_compact {
            rows.push(
                menu_icon_item("chat-more-compact", ProIcon::Memory, "整理上下文", s)
                    .disabled(!can_compact)
                    .on_press(cx, |this, _, cx| {
                        this.open_menu = None;
                        this.bridge.send(KarCommand::CompactChat);
                        cx.notify();
                    })
                    .into_any_element(),
            );
        }
        if can_goals {
            rows.push(
                menu_icon_item("chat-more-goal", ProIcon::Tasks, "设定对话目标…", s)
                    .on_press(cx, |this, window, cx| this.open_goal_dialog(window, cx))
                    .into_any_element(),
            );
        }
        rows.push(menu_divider(s).into_any_element());

        rows.push(
            menu_icon_item("chat-more-remote", ProIcon::Link, "远端短码双向配对…", s)
                .on_press(cx, |this, _, cx| {
                    this.open_settings(SettingsTab::Remote, cx);
                })
                .into_any_element(),
        );
        rows.push(if sharing {
            menu_icon_item("chat-more-unshare", ProIcon::Globe, "停止共享链接", s)
                .on_press(cx, |this, _, cx| this.stop_sharing(cx))
                .into_any_element()
        } else {
            menu_icon_item("chat-more-share", ProIcon::Globe, "生成只读共享链接", s)
                .on_press(cx, |this, _, cx| this.start_sharing(cx))
                .into_any_element()
        });
        rows.push(
            menu_icon_item("chat-more-settings", ProIcon::Sliders, "设置与控制中心", s)
                .child(menu_hint("⌘,", s))
                .on_press(cx, |this, _, cx| {
                    this.open_settings(SettingsTab::Agents, cx);
                })
                .into_any_element(),
        );

        if let Some(cid) = chat_id {
            rows.push(menu_divider(s).into_any_element());
            rows.push(
                menu_icon_item("chat-more-delete", ProIcon::Trash, "删除此对话", s)
                    .disabled(!idle)
                    .on_press(cx, move |this, _, cx| {
                        this.open_menu = None;
                        this.stage_deferred_chat_delete(cid, del_title.clone(), cx);
                    })
                    .into_any_element(),
            );
        }

        self.render_menu(Menu::ChatMore, MenuPlacement::BelowEnd, 232., rows, s, cx)
    }

    fn context_ratio(&self) -> Option<f32> {
        let ctx_u = self.projected.context_usage.as_ref()?;
        if let Some(pct) = ctx_u.percent {
            return Some((pct as f32 / 100.0).clamp(0.0, 1.0));
        }
        if let (Some(tok), Some(win)) = (ctx_u.tokens, ctx_u.context_window)
            && win > 0
        {
            return Some((tok as f32 / win as f32).clamp(0.0, 1.0));
        }
        None
    }

    fn usage_summary_line(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(ref ctx_u) = self.projected.context_usage {
            if let Some(pct) = ctx_u.percent {
                parts.push(format!("上下文 {:.0}%", pct.clamp(0.0, 100.0)));
            } else if let (Some(tok), Some(win)) = (ctx_u.tokens, ctx_u.context_window)
                && win > 0
            {
                parts.push(format!(
                    "上下文 {} / {}",
                    format_tokens_compact(tok as i64),
                    format_tokens_compact(win as i64)
                ));
            }
        }
        if let Some(last) = self.projected.usage.last()
            && let Some(tot) = last.usage.with_children.as_ref().or(last.usage.total.as_ref())
        {
            let total_io = tot.input.saturating_add(tot.output);
            if total_io > 0 {
                parts.push(format!("{} tokens", format_tokens_compact(total_io)));
            }
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }

    fn chat_status(&self) -> Option<(&'static str, DotTone)> {
        let loc = self.ui_locale;
        match &self.connection_state {
            FeedConnectionState::Reconnecting { .. } => {
                return Some((loc.pick("正在重连", "Reconnecting"), DotTone::Warning));
            }
            FeedConnectionState::Disconnected { .. } => {
                return Some((loc.pick("已断开", "Disconnected"), DotTone::Danger));
            }
            FeedConnectionState::Connecting { .. } => {
                return Some((loc.pick("正在连接", "Connecting"), DotTone::Info));
            }
            _ => {}
        }
        match &self.access_mode {
            ChatAccessMode::LocalReadOnlySubscriber { .. }
            | ChatAccessMode::HttpReadOnly { .. } => {
                Some((loc.pick("实时同步", "Live"), DotTone::Info))
            }
            ChatAccessMode::RemoteController { role, .. } => Some((
                if role.can_mutate() {
                    loc.pick("双向配对", "Paired")
                } else {
                    loc.pick("远端观察", "Read-only")
                },
                DotTone::Success,
            )),
            ChatAccessMode::ArchivedReadOnly { .. } => {
                Some((loc.pick("归档只读", "Archived"), DotTone::Neutral))
            }
            _ => None,
        }
    }

    fn render_share_bar(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let url = self.active_http_server_url.clone();
        let open = url.is_some();
        let bar = url.map(|url| {
            let to_copy = url.clone();
            div()
                .w_full()
                .max_w(px(THREAD_W))
                .mx_auto()
                .px(px(THREAD_PAD))
                .pt(px(6.))
                .pb(px(2.))
                .child(
                    div()
                        .h(px(34.))
                        .pl(px(12.))
                        .pr(px(4.))
                        .rounded(px(R_CTRL))
                        .bg(s.soft)
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().size(px(6.)).flex_none().rounded_full().bg(s.pro.green))
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(FS_META))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(s.ink)
                                .child("正在共享"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(FS_META))
                                .text_color(s.muted)
                                .child(url),
                        )
                        .child(text_button("share-copy", "复制链接", s).on_press(
                            cx,
                            move |this, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(to_copy.clone()));
                                this.notify_toast("链接已复制", cx);
                            },
                        ))
                        .child(
                            text_button("share-stop", "停止", s)
                                .on_press(cx, |this, _, cx| this.stop_sharing(cx)),
                        ),
                )
                .into_any_element()
        });
        pro_fold("kar-share-bar", open, 42., 0., move || {
            bar.unwrap_or_else(|| div().into_any_element())
        })
    }

    fn render_goal_bar(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let goal = self.projected.goal.as_ref().and_then(Option::as_ref).cloned();
        let writable = matches!(self.access_mode, ChatAccessMode::WriteController { .. });
        let open = goal.is_some();
        let bar = goal.map(|g| {
            div()
                .w_full()
                .max_w(px(THREAD_W))
                .mx_auto()
                .px(px(THREAD_PAD))
                .pt(px(6.))
                .pb(px(2.))
                .child(
                    div()
                        .h(px(34.))
                        .pl(px(12.))
                        .pr(px(4.))
                        .rounded(px(R_CTRL))
                        .bg(s.soft)
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(pro_icon(ProIcon::Tasks, ICON_SM, s.muted))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(FS_META))
                                .text_color(s.ink)
                                .child(format!("目标：{}", g.objective)),
                        )
                        .when(writable && self.projected.can_goals, |d| {
                            d.child(
                                text_button("goal-edit", "修改", s)
                                    .on_press(cx, |this, window, cx| {
                                        this.open_goal_dialog(window, cx)
                                    }),
                            )
                            .child(
                                icon_button("goal-clear", ProIcon::Close, "清除目标", s)
                                    .on_press(cx, |this, _, _| {
                                        this.bridge.send(KarCommand::ClearGoal)
                                    }),
                            )
                        }),
                )
                .into_any_element()
        });
        pro_fold("kar-goal-bar", open, 42., 0., move || {
            bar.unwrap_or_else(|| div().into_any_element())
        })
    }

    fn render_notice(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let block = self
            .notice
            .clone()
            .map(|notice| self.render_notice_block(notice, s, cx));
        pro_fold("kar-notice", self.notice_open, 72., 0., move || {
            block.unwrap_or_else(|| div().into_any_element())
        })
    }

    fn render_notice_block(
        &self,
        notice: Notice,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (fill, mark, title_ink) = match notice.tone {
            NoticeTone::Info => (s.soft, s.muted, s.ink),
            NoticeTone::Warning => (s.pro.amber_soft, s.pro.amber_ink, s.ink),
            NoticeTone::Error => (s.pro.red_soft, s.pro.red, s.ink),
        };
        div()
            .w_full()
            .max_w(px(THREAD_W))
            .mx_auto()
            .px(px(THREAD_PAD))
            .pt(px(8.))
            .pb(px(4.))
            .child(
                div()
                    .w_full()
                    .pl(px(12.))
                    .pr(px(4.))
                    .py(px(8.))
                    .rounded(px(R_CARD))
                    .bg(fill)
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(div().pt(px(3.)).flex_none().child(pro_icon(
                        if notice.tone == NoticeTone::Info {
                            ProIcon::Bell
                        } else {
                            ProIcon::AlertTriangle
                        },
                        ICON_SM,
                        mark,
                    )))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .py(px(2.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(FS_UI))
                                    .line_height(px(LH_UI))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(title_ink)
                                    .child(notice.title),
                            )
                            .when(!notice.detail.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_size(px(FS_META))
                                        .line_height(px(LH_META))
                                        .text_color(s.text)
                                        .child(notice.detail),
                                )
                            }),
                    )
                    .when(notice.action == NoticeAction::ShowAgents, |d| {
                        d.child(text_button("notice-agents", "查看智能体", s).on_press(
                            cx,
                            |this, _, cx| {
                                this.open_settings(SettingsTab::Agents, cx);
                            },
                        ))
                    })
                    .when(notice.action == NoticeAction::ChooseProject, |d| {
                        d.child(text_button("notice-project", "选择项目", s)
                            .on_press(cx, |this, window, cx| this.pick_workspace(window, cx)))
                    })
                    .child(
                        icon_button("notice-dismiss", ProIcon::Close, "知道了", s)
                            .on_press(cx, |this, _, cx| this.dismiss_notice(cx)),
                    ),
            )
            .into_any_element()
    }

    fn render_start_stage(
        &self,
        pane_w: f32,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let loc = self.ui_locale;
        let workspace = self.effective_workspace_dir();
        let project = self
            .project_registry
            .projects
            .iter()
            .find(|project| project.root_path == workspace)
            .map(|project| project.name.clone())
            .or_else(|| {
                workspace
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| workspace.display().to_string());
        let inline = f32::from(window.viewport_size().width) >= 1024.
            && pane_w > 480. + project.chars().count() as f32 * 16.;
        let short = f32::from(window.viewport_size().height) <= 544.;
        let title_size = if pane_w < 560. { 24. } else { 30. };
        let project_label = div()
            .min_w_0()
            .max_w(px((pane_w - 80.).clamp(100., 440.)))
            .truncate()
            .text_color(s.ink)
            .child(project);
        let project_trigger = Tactile::ghost("draft-project", s)
            .h(px(if inline { 38. } else { 26. }))
            .max_w_full()
            .text_size(px(if inline { title_size } else { 16. }))
            .font_weight(FontWeight::NORMAL)
            .tip(workspace.display().to_string())
            .child(project_label);
        let project_picker = if self.chat_is_open() {
            project_trigger.inert().into_any_element()
        } else {
            menu_host(
                project_trigger.on_press(cx, |this, _, cx| {
                    this.toggle_menu(Menu::ComposerProject, cx)
                }),
                self.render_composer_project_menu(MenuPlacement::BelowStart, s, cx),
            )
            .into_any_element()
        };
        let heading = div()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(if inline { 0. } else { 8. }))
            .when(!inline, |d| d.flex_col())
            .text_size(px(title_size))
            .line_height(px(if title_size == 24. { 32. } else { 36. }))
            .font_weight(FontWeight::NORMAL)
            .text_color(s.ink)
            .when(self.pinned_target.is_some(), |d| {
                d.child(loc.pick("讨论选中内容", "Discuss selection"))
            })
            .when(self.pinned_target.is_none(), |d| {
                d.when(inline, |row| {
                    row.child(
                        div()
                            .mr(px(7.))
                            .child(loc.pick("要在", "What should we build in")),
                    )
                })
                .when(!inline, |col| {
                    col.child(loc.pick("我们要构建什么？", "What should we build?"))
                })
                .child(project_picker)
                .when(inline, |row| {
                    row.child(
                        div()
                            .ml(px(if loc == crate::ui_primitives::UiLocale::Zh {
                                4.
                            } else {
                                0.
                            }))
                            .child(loc.pick("中构建什么？", "?")),
                    )
                })
            });

        let recent = self
            .chat_list
            .iter()
            .filter(|entry| entry.turn_count > 0 && entry.info.workspace == workspace)
            .filter(|entry| !self.settled_chats.contains(&entry.info.id))
            .max_by_key(|entry| entry.last_activity_ms);
        let secondary = if let Some(entry) = recent {
            let id = entry.info.id;
            Tactile::ghost("home-resume", s)
                .tip(entry.title.clone())
                .child(loc.pick("或继续上次的对话", "or resume the last thread"))
                .on_press(cx, move |this, win, cx| this.open_chat(id, win, cx))
        } else {
            Tactile::ghost("home-files", s)
                .child(loc.pick("或浏览工作区文件", "or browse workspace files"))
                .on_press(cx, |this, _, cx| this.open_files_dock_tab(cx))
        };
        let dropzone = self.file_dropzone.clone();
        let composer = self.render_composer(pane_w, s, window, cx);
        div()
            .id("kar-start-scroll")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .child(
                chat_content_lane(pane_w, window)
                    .relative()
                    .flex_none()
                    .mt_auto()
                    .when(!short, |d| d.mb_auto().top(px((32. - HEADER_H) / 2.)))
                    .when(short, |d| d.pb(px(12.)))
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom(relative(1.))
                            .px(px(12.))
                            .pb(px(16.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(heading)
                            .when(!short, |d| {
                                d.child(
                                    secondary
                                        .mt(px(8.))
                                        .h(px(24.))
                                        .px(px(4.))
                                        .text_size(px(14.))
                                        .ink(s.muted, s.ink),
                                )
                            }),
                    )
                    .child(composer)
                    .when(self.start_dropzone_open, |d| {
                        d.child(pro_fold(
                            "kar-start-dropzone-fold",
                            true,
                            184.,
                            0.,
                            move || div().w_full().pt(px(8.)).child(dropzone).into_any_element(),
                        ))
                    }),
            )
    }

    fn render_waiting_for_content(&self, s: &StudioPalette) -> AnyElement {
        let (title, detail) = match &self.access_mode {
            ChatAccessMode::HttpReadOnly { .. } => {
                ("正在等待新内容", "对话一有进展，就会实时出现在这里。")
            }
            ChatAccessMode::ArchivedReadOnly { reason, .. } => {
                ("这个历史对话暂无消息记录", reason.as_str())
            }
            _ => ("这个对话还没有内容", "它正在另一个窗口里进行，内容会同步到这里。"),
        };
        div()
            .w_full()
            .min_h(px(320.))
            .px(px(THREAD_PAD))
            .py(px(64.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(FS_READ))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.ink)
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(FS_UI))
                    .line_height(px(LH_UI))
                    .text_color(s.muted)
                    .child(detail.to_owned()),
            )
            .into_any_element()
    }

    // ── Two-Tier Composer (`AssistantHomeScreen` KUI architecture) ───────────

    fn render_view_only_bar(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let (icon, text) = match &self.access_mode {
            ChatAccessMode::HttpReadOnly { .. } => (
                ProIcon::Link,
                "这是共享链接的只读观察视图，内容实时同步".to_owned(),
            ),
            ChatAccessMode::ArchivedReadOnly { reason, .. } => (ProIcon::Clock, reason.clone()),
            _ => (
                ProIcon::Eye,
                "该对话正在另一个窗口中进行，当前窗口实时同步查看".to_owned(),
            ),
        };
        div()
            .w_full()
            .min_h(px(46.))
            .pl(px(16.))
            .pr(px(10.))
            .py(px(10.))
            .rounded(px(R_HERO))
            .bg(s.soft)
            .flex()
            .items_center()
            .justify_between()
            .gap(px(10.))
            .text_size(px(FS_UI))
            .line_height(px(LH_UI))
            .text_color(s.muted)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(pro_icon(icon, ICON_SM, s.muted))
                    .child(div().min_w_0().truncate().child(text)),
            )
            .child(
                if let ChatAccessMode::ArchivedReadOnly {chat_id,..} = self.access_mode
                    && self.projected.info.as_ref().is_some_and(|info| info.workspace.is_dir()) {
                    soft_button("view-only-retry", "重新连接", s)
                        .on_press(cx, move |this, window, cx| this.open_chat(chat_id, window, cx))
                } else {
                    soft_button("view-only-new-chat", "开启新对话", s)
                        .on_press(cx, |this, window, cx| this.start_new_chat(window, cx))
                }
            )
            .into_any_element()
    }

    fn render_composer(
        &self,
        pane_w: f32,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let draft = self.composer_editor.read(cx).value().to_owned();
        // The Git split can halve the writing width. Measure the same shaped text
        // that GPUI paints so wrapped CJK drafts remain visible while editing.
        let input_width = ((pane_w - 2. * chat_lane_gutter(pane_w, window)).min(THREAD_W) - 34.).max(60.);
        let lines = if draft.is_empty() {
            2
        } else if draft.len() > 4096 {
            10
        } else {
            let run = window.text_style().to_run(draft.len());
            window
                .text_system()
                .shape_text(
                    draft.clone().into(),
                    px(FS_READ),
                    &[run],
                    Some(px(input_width)),
                    Some(10),
                )
                .map(|rows| {
                    rows.iter()
                        .map(|r| r.wrap_boundaries().len() + 1)
                        .sum::<usize>()
                })
                .unwrap_or_else(|_| draft.lines().count())
                .clamp(2, 10)
        };
        let field_h = pro_spring_f32(
            "kar-composer-height",
            lines as f32 * 22.75,
            0.16,
            window,
            cx,
        );

        let chat_open = self.chat_is_open();
        let turn_running = self.turn_in_progress();
        let has_draft = !draft.trim().is_empty() || !self.pending_attachments.is_empty();
        let can_send = has_draft && self.queued_prompt.is_none() && (!turn_running || chat_open);

        let start = MenuPlacement::AboveStart;
        let end = MenuPlacement::AboveEnd;

        let pinned = self
            .pinned_target
            .clone()
            .map(|target| self.render_pinned_chip(target, s, cx));
        let pinned_fold = pro_fold(
            "composer-pinned-fold",
            pinned.is_some(),
            40.,
            0.,
            move || pinned.unwrap_or_else(|| div().into_any_element()),
        );

        let attachments_bar = (!self.pending_attachments.is_empty())
            .then(|| self.render_pending_attachments_strip(s, cx));
        let bg_cmds_bar = self
            .client_reducer
            .background_commands
            .values()
            .any(|cmd| self.projected.turns.iter().any(|t| t.id == cmd.turn_id))
            .then(|| self.render_background_commands_strip(s, cx));
        let queued_bar = (!self.projected.queued_inputs.is_empty())
            .then(|| self.render_queued_inputs_strip(s, cx));

        let loc = self.ui_locale;
        let compact_bar = pane_w < 600.;

        let approval = if chat_open {
            self.projected
                .info
                .as_ref()
                .map_or(self.approval_mode, |info| info.approvals)
        } else {
            self.approval_mode
        };
        let approval_label = match approval {
            ApprovalMode::Host => loc.pick("请求审批", "Request approval"),
            ApprovalMode::Deny => loc.pick("自动拒绝", "Deny requests"),
        };
        let runtime_id = if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            "claude".to_owned()
        } else {
            self.effective_runtime()
        };
        let settings_button = Tactile::ghost("cmp-options", s)
            .h(px(24.))
            .px(px(6.))
            .gap(px(5.))
            .lit(self.open_menu == Some(Menu::ComposerApproval))
            .tip(loc.pick("工具权限", "Tool permissions"))
            .ink(s.muted, s.ink)
            .child(render_approval_hand(13., s.muted))
            .when(pane_w >= 440., |b| b.child(approval_label))
            .on_press(cx, |this, _, cx| {
                this.toggle_menu(Menu::ComposerApproval, cx)
            });

        let model = if chat_open {
            self.projected
                .info
                .as_ref()
                .and_then(|i| i.model.as_deref())
        } else {
            self.selected_model.as_deref()
        };
        let add_menu = if self.open_menu == Some(Menu::ComposerSkills) {
            self.render_skills_menu(end, s, cx)
        } else {
            self.render_add_menu(end, s, cx)
        };

        let ctx_ratio = self.context_ratio();
        let usage = self.projected.context_usage.as_ref();
        let ctx_label = ctx_ratio
            .map(|r| format!("{:.0}%", r * 100.))
            .unwrap_or_else(|| loc.pick("上下文", "Context").into());
        let mut context_rows =
            vec![menu_caption(loc.pick("上下文用量", "Context window"), s).into_any_element()];
        let mut values = Vec::new();
        if let Some(tokens) = usage.and_then(|u| u.tokens) {
            values.push(format!("Used  {tokens} tokens"));
        }
        if let Some(limit) = usage.and_then(|u| u.context_window) {
            values.push(format!("Limit  {limit} tokens"));
        }
        if let Some(ratio) = ctx_ratio {
            values.push(format!("Usage  {:.0}%", ratio * 100.));
        }
        if values.is_empty() {
            values.push(
                loc.pick("智能体尚未上报上下文用量", "No context telemetry yet")
                    .into(),
            );
        }
        for text in values {
            context_rows.push(
                div()
                    .px(px(10.))
                    .py(px(4.))
                    .text_size(px(FS_META))
                    .text_color(s.text)
                    .child(text)
                    .into_any_element(),
            );
        }
        if self.projected.can_compact
            && !turn_running
            && matches!(self.access_mode, ChatAccessMode::WriteController { .. })
        {
            context_rows.push(menu_divider(s).into_any_element());
            context_rows.push(
                menu_icon_item(
                    "compact-context",
                    ProIcon::Sparkles,
                    loc.pick("压缩上下文", "Compact context"),
                    s,
                )
                .on_press(cx, |this, _, cx| {
                    this.close_menu(cx);
                    this.bridge.send(KarCommand::CompactChat);
                })
                .into_any_element(),
            );
        }
        let show_ctx = usage.is_some() || self.open_menu == Some(Menu::ContextUsage);
        let ctx_control = show_ctx.then(|| {
            menu_host(
                Tactile::ghost("cmp-context-inspector-btn", s)
                    .h(px(CTRL_H))
                    .px(px(7.))
                    .gap(px(5.))
                    .lit(self.open_menu == Some(Menu::ContextUsage))
                    .tip(loc.pick("上下文容量与压缩", "Context window usage"))
                    .font_features(tnum_features())
                    .text_size(px(FS_META))
                    .ink(s.muted, s.ink)
                    .when_some(ctx_ratio, |row, r| {
                        row.child(pro_progress_ring(r, 13., 2., s.ink, s.line_strong))
                    })
                    .child(ctx_label)
                    .on_press(cx, |this, _, cx| this.toggle_menu(Menu::ContextUsage, cx)),
                self.render_menu(Menu::ContextUsage, start, 224., context_rows, s, cx),
            )
        });

        let left = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                Tactile::ghost("cmp-add", s)
                    .size(px(CTRL_H))
                    .justify_center()
                    .tip(loc.pick("添加文件或图片附件", "Attach files or images"))
                    .child(pro_icon(ProIcon::Paperclip, 16., s.muted))
                    .on_press(cx, |this, win, cx| this.pick_attachments(win, cx)),
            )
            .when_some(ctx_control, |d, c| d.child(c));

        let effort_word = if chat_open {
            self.projected
                .info
                .as_ref()
                .and_then(|info| info.settings.effort.as_deref())
        } else {
            self.selected_effort()
        };
        let catalogue = self.providers.iter().find(|provider| provider.id == runtime_id);
        let display_agent = if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            "Claude Opus 4.6".to_owned()
        } else if let Some(id) = model {
            catalogue
                .and_then(|p| p.models.iter().find(|entry| entry.id == id))
                .map_or_else(|| id.to_owned(), |entry| entry.label.clone())
        } else {
            catalogue
                .filter(|_| !chat_open)
                .and_then(|p| p.models.iter().find(|entry| entry.is_default))
                .map_or_else(|| runtime_display_name(&runtime_id), |entry| entry.label.clone())
        };
        let agent_button = Tactile::ghost("cmp-agent", s)
            .flex_none()
            .h(px(CTRL_H))
            .px(px(8.))
            .radius(R_CTRL)
            .gap(px(5.5))
            .text_size(px(FS_UI))
            .child(render_runtime_mark(&runtime_id, 16., s, cx))
            .child(
                div()
                    .max_w(px((pane_w - 165.).clamp(58., 240.)))
                    .truncate()
                    .text_color(s.text)
                    .child(display_agent),
            )
            .when_some(effort_word.filter(|_| !compact_bar), |b, level| {
                b.child(div().text_color(s.muted).child(effort_label(level, loc).to_owned()))
            });

        let agent_control = if chat_open || matches!(self.access_mode, ChatAccessMode::DemoShowcase)
        {
            agent_button
                .inert()
                .tip(loc.pick("当前对话使用的智能体与推理档位", "Active model & effort"))
                .into_any_element()
        } else {
            menu_host(
                agent_button
                    .child(pro_icon(ProIcon::ChevronDown, 12., s.muted))
                    .lit(self.open_menu == Some(Menu::ComposerAgent))
                    .on_press(cx, |this, _, cx| this.toggle_menu(Menu::ComposerAgent, cx)),
                self.render_agent_menu(s, window, cx),
            )
            .into_any_element()
        };

        let primary_action: AnyElement = if turn_running && !has_draft {
            Tactile::primary("cmp-stop", s)
                .fills(s.pro.red, s.pro.red_ink)
                .radius(16.)
                .size(px(32.))
                .flex_none()
                .justify_center()
                .tip(loc.pick("停止当前执行", "Stop generation"))
                .child(div().size(px(9.)).rounded(px(2.)).bg(s.on_brand_action))
                .on_press(cx, |this, _, _| this.bridge.send(KarCommand::AbortTurn))
                .into_any_element()
        } else {
            Tactile::primary("cmp-send", s)
                .radius(16.)
                .fills(s.brand_action, s.brand_action)
                .lit_fill(s.brand_action)
                .lit(can_send)
                .disabled(!can_send)
                .size(px(32.))
                .flex_none()
                .justify_center()
                .tip(if turn_running {
                    loc.pick("平滑汇入补充指令", "Steer / queue follow-up")
                } else {
                    loc.pick("发送  ↵", "Send  ↵")
                })
                .child(pro_icon(ProIcon::ArrowUp, 15., s.on_brand_action))
                .on_press(cx, |this, window, cx| {
                    this.submit_composer_prompt(window, cx)
                })
                .into_any_element()
        };

        let right = div()
            .flex()
            .items_center()
            .gap(px(5.))
            .child(agent_control)
            .child(menu_host(
                icon_button(
                    "cmp-more",
                    ProIcon::MoreHorizontal,
                    loc.pick("更多输入选项", "More composer controls"),
                    s,
                )
                .lit(matches!(
                    self.open_menu,
                    Some(Menu::ComposerAdd | Menu::ComposerSkills)
                ))
                .on_press(cx, |this, _, cx| this.toggle_menu(Menu::ComposerAdd, cx)),
                add_menu,
            ))
            .child(primary_action);

        let write_card = div()
            .w_full()
            .rounded(px(16.))
            .bg(s.card)
            .border_1()
            .border_color(s.line_soft)
            .when(!s.dark, |d| {
                d.shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.05),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(2.),
                    spread_radius: px(0.),
                    inset: false,
                }])
            })
            .pt(px(16.))
            .pb(px(8.))
            .flex()
            .flex_col()
            .child(pinned_fold)
            .when_some(attachments_bar, |d, bar| {
                d.child(div().px(px(16.)).child(bar))
            })
            .child(
                div()
                    .w_full()
                    .h(px(field_h))
                    .px(px(16.))
                    .child(self.composer_editor.clone()),
            )
            .child(
                div()
                    .w_full()
                    .mt(px(8.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(6.))
                    .child(left)
                    .child(right),
            );

        let is_draft = !chat_open && !matches!(self.access_mode, ChatAccessMode::DemoShowcase);
        div()
            .relative()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .when_some(bg_cmds_bar, |d, b| d.child(b))
            .when_some(queued_bar, |d, q| d.child(q))
            .child(write_card)
            .when(is_draft, |d| {
                let wt = self.new_chat_use_worktree;
                let local_label = loc.pick("当前检出", "Current checkout");
                let tree_label = loc.pick("独立工作树", "Worktree");
                let ws = self.effective_workspace_dir();
                let mode_button = Tactile::ghost("cmp-worktree-mode", s)
                    .h(px(24.))
                    .px(px(6.))
                    .gap(px(6.))
                    .ink(s.muted, s.ink)
                    .tip(ws.display().to_string())
                    .child(render_folder_icon(13., s.muted))
                    .child(if wt { tree_label } else { local_label });
                let mode = if self.git_snapshot.is_repo || wt {
                    let choices = [local_label, tree_label]
                        .into_iter()
                        .enumerate()
                        .map(|(index, label)| {
                            let worktree = index == 1;
                            menu_item(("cmp-workspace-mode", index), None, label, s)
                                .when(worktree == wt, |b| b.child(menu_check(s)))
                                .on_press(cx, move |this, _, cx| {
                                    this.new_chat_use_worktree = worktree;
                                    this.close_menu(cx);
                                })
                                .into_any_element()
                        })
                        .collect();
                    menu_host(
                        mode_button
                            .lit(self.open_menu == Some(Menu::ComposerWorkspaceMode))
                            .on_press(cx, |this, _, cx| {
                                this.toggle_menu(Menu::ComposerWorkspaceMode, cx)
                            }),
                        self.render_menu(Menu::ComposerWorkspaceMode, start, 192., choices, s, cx),
                    )
                    .into_any_element()
                } else {
                    mode_button.inert().into_any_element()
                };
                d.child(
                    div()
                        .w_full()
                        .pt(px(8.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(12.))
                        .child(mode)
                        .child(div().flex_1())
                        .when(self.git_snapshot.is_repo && pane_w >= 400., |row| {
                            let branch = self
                                .git_snapshot
                                .current_branch
                                .as_deref()
                                .unwrap_or("HEAD");
                            let trigger = Tactile::ghost("draft-branch", s)
                                .h(px(24.))
                                .px(px(6.))
                                .gap(px(5.))
                                .ink(s.muted, s.ink)
                                .child(render_git_branch_icon(13., s.muted))
                                .child(
                                    div()
                                        .max_w(px((pane_w * 0.2).clamp(60., 180.)))
                                        .truncate()
                                        .child(branch.to_owned()),
                                );
                            if self.layout_mode == WorkspaceLayoutMode::ChatFocus
                                && !self.thread_details_open
                            {
                                row.child(menu_host(
                                    trigger
                                        .tip(loc.pick("切换分支", "Switch branch"))
                                        .lit(self.open_menu == Some(Menu::GitBranch))
                                        .on_press(cx, |this, _, cx| {
                                            this.toggle_menu(Menu::GitBranch, cx)
                                        }),
                                    self.render_git_branch_menu(
                                        &self.git_snapshot,
                                        branch,
                                        end,
                                        s,
                                        cx,
                                    ),
                                ))
                            } else {
                                row.child(
                                    trigger
                                        .tip(loc.pick(
                                            "在 Git 面板查看分支",
                                            "View branch in Git panel",
                                        ))
                                        .on_press(cx, |this, _, cx| {
                                            this.open_git_workbench_tab(cx)
                                        }),
                                )
                            }
                        })
                        .child(menu_host(
                            settings_button,
                            self.render_approval_menu(end, s, cx),
                        )),
                )
            })
    }

    fn render_composer_project_menu(
        &self,
        placement: MenuPlacement,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let cur_norm = self
            .config
            .workspace_dir
            .canonicalize()
            .unwrap_or_else(|_| self.config.workspace_dir.clone());
        let mut rows: Vec<AnyElement> =
            vec![menu_caption(loc.pick("切换活跃项目", "Switch project"), s).into_any_element()];

        for (ix, proj) in self.project_registry.projects.iter().enumerate() {
            let is_cur = proj
                .root_path
                .canonicalize()
                .unwrap_or_else(|_| proj.root_path.clone())
                == cur_norm;
            let root = proj.root_path.clone();
            rows.push(
                menu_item(
                    ("cmp-proj-item", ix),
                    Some(render_folder_icon(15., s.text)),
                    proj.name.clone(),
                    s,
                )
                .when(is_cur, |r| r.child(menu_check(s)))
                .on_press(cx, move |this, window, cx| {
                    this.open_menu = None;
                    if !is_cur {
                        this.select_project_and_new_chat(root.clone(), window, cx);
                    }
                })
                .into_any_element(),
            );
        }

        rows.push(menu_divider(s).into_any_element());
        rows.push(
            menu_icon_item(
                "cmp-proj-pick",
                ProIcon::Plus,
                loc.pick("添加并选择其他文件夹…", "Choose another folder…"),
                s,
            )
            .on_press(cx, |this, window, cx| this.pick_workspace(window, cx))
            .into_any_element(),
        );

        self.render_menu(Menu::ComposerProject, placement, 232., rows, s, cx)
    }

    fn render_background_commands_strip(
        &self,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now_ms = crate::remote_link::now_epoch_ms();
        let mut col = div()
            .w_full()
            .px(px(6.))
            .py(px(4.))
            .flex()
            .flex_col()
            .gap(px(4.));
        for (ix, bg) in self
            .client_reducer
            .background_commands
            .values()
            .filter(|cmd| self.projected.turns.iter().any(|t| t.id == cmd.turn_id))
            .enumerate()
        {
            let cid = bg.tool_id.clone();
            let cid_dismiss = bg.tool_id.clone();
            let elapsed_secs = now_ms.saturating_sub(bg.started_ms) / 1000;
            let follow_up = bg.follow_up_requested;
            let finished = bg.completed;
            let badge_txt = if finished {
                "后台已完成".to_owned()
            } else {
                format!("后台跟踪 · {elapsed_secs}s")
            };
            col = col.child(
                div()
                    .h(px(30.))
                    .pl(px(8.))
                    .pr(px(4.))
                    .rounded(px(R_CTRL))
                    .bg(s.card)
                    .border_1()
                    .border_color(s.line_soft)
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
                            .gap(px(6.))
                            .child(render_bg_layers_icon(
                                ICON_SM,
                                if finished {
                                    s.pro.green
                                } else {
                                    s.pro.accent_info
                                },
                            ))
                            .child(
                                div()
                                    .flex_none()
                                    .px(px(5.))
                                    .py(px(1.))
                                    .rounded(px(R_PILL))
                                    .bg(if finished {
                                        s.pro.green_soft
                                    } else {
                                        s.pro.accent_soft
                                    })
                                    .text_size(px(FS_TAG))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if finished {
                                        s.pro.green_ink
                                    } else {
                                        s.pro.accent_info
                                    })
                                    .child(badge_txt),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(mono_font(cx))
                                    .text_size(px(FS_META))
                                    .text_color(s.ink)
                                    .child(bg.command_summary.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                soft_button(
                                    ("bg-cmd-followup", ix),
                                    if follow_up {
                                        "结束汇报：开"
                                    } else {
                                        "结束汇报：关"
                                    },
                                    s,
                                )
                                .lit(follow_up)
                                .on_press(cx, move |this, _, cx| {
                                    if let Some(cmd) =
                                        this.client_reducer.background_commands.get_mut(&cid)
                                    {
                                        cmd.follow_up_requested = !cmd.follow_up_requested;
                                        cx.notify();
                                    }
                                }),
                            )
                            .child(
                                Tactile::ghost(("bg-cmd-dismiss", ix), s)
                                    .size(px(20.))
                                    .justify_center()
                                    .tip("收起后台命令条")
                                    .child(pro_icon(ProIcon::Close, 10., s.muted))
                                    .on_press(cx, move |this, _, cx| {
                                        this.client_reducer.background_commands.remove(&cid_dismiss);
                                        cx.notify();
                                    }),
                            ),
                    ),
            );
        }
        col.into_any_element()
    }

    fn render_pending_attachments_strip(
        &self,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut row = div().w_full().pb(px(4.)).flex().flex_wrap().items_center().gap(px(6.));
        for (ix, path) in self.pending_attachments.iter().enumerate() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "附件".into());
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let is_img = matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp");
            row = row.child(
                div()
                    .h(px(26.))
                    .pl(px(8.))
                    .pr(px(4.))
                    .rounded(px(R_CTRL))
                    .bg(s.soft)
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(pro_icon(
                        if is_img {
                            ProIcon::Sparkles
                        } else {
                            ProIcon::Paperclip
                        },
                        ICON_XS,
                        s.muted,
                    ))
                    .child(
                        div()
                            .max_w(px(160.))
                            .truncate()
                            .text_size(px(FS_META))
                            .text_color(s.ink)
                            .child(name),
                    )
                    .child(
                        Tactile::ghost(("cmp-att-rm", ix), s)
                            .size(px(18.))
                            .radius(4.)
                            .justify_center()
                            .tip("移除附件")
                            .child(pro_icon(ProIcon::Close, 10., s.muted))
                            .on_press(cx, move |this, _, cx| {
                                if ix < this.pending_attachments.len() {
                                    this.pending_attachments.remove(ix);
                                    cx.notify();
                                }
                            }),
                    ),
            );
        }
        row.into_any_element()
    }

    fn render_queued_inputs_strip(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let can_steer = self.projected.can_steer;
        let mut col = div()
            .w_full()
            .px(px(6.))
            .py(px(4.))
            .flex()
            .flex_col()
            .gap(px(4.));
        for (ix, item) in self.projected.queued_inputs.iter().enumerate() {
            let input_id = item.id;
            let raw_clean = strip_selection_envelope(&item.text).to_owned();
            let text = raw_clean.lines().next().unwrap_or("").to_owned();
            let raw_for_edit = raw_clean.clone();
            let raw_for_withdraw = raw_clean;
            col = col.child(
                div()
                    .h(px(30.))
                    .pl(px(8.))
                    .pr(px(4.))
                    .rounded(px(R_CTRL))
                    .bg(s.card)
                    .border_1()
                    .border_color(s.line_soft)
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
                            .gap(px(6.))
                            .child(if can_steer {
                                render_steer_merge_icon(ICON_XS, s.pro.accent_info)
                            } else {
                                pro_icon(ProIcon::Clock, ICON_XS, s.muted)
                            })
                            .child(
                                div()
                                    .flex_none()
                                    .px(px(6.))
                                    .py(px(1.))
                                    .rounded(px(R_PILL))
                                    .bg(if can_steer {
                                        s.pro.accent_soft
                                    } else {
                                        s.soft
                                    })
                                    .text_size(px(FS_TAG))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if can_steer {
                                        s.pro.accent_info
                                    } else {
                                        s.muted
                                    })
                                    .child(if can_steer {
                                        "平滑汇入"
                                    } else {
                                        "待发队列"
                                    }),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(FS_META))
                                    .text_color(s.ink)
                                    .child(text),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(
                                text_button(("edit-queued", ix), "编辑", s)
                                    .tip("取回到输入框继续修改")
                                    .on_press(cx, move |this, window, cx| {
                                        this.withdraw_or_edit_queued_input(
                                            input_id,
                                            raw_for_edit.clone(),
                                            true,
                                            window,
                                            cx,
                                        );
                                    }),
                            )
                            .child(
                                text_button(("withdraw-queued", ix), "撤回", s)
                                    .on_press(cx, move |this, window, cx| {
                                        this.withdraw_or_edit_queued_input(
                                            input_id,
                                            raw_for_withdraw.clone(),
                                            false,
                                            window,
                                            cx,
                                        );
                                    }),
                            ),
                    ),
            );
        }
        col.into_any_element()
    }

    fn render_pinned_chip(
        &self,
        target: SelectionTarget,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let doc = if target.is_code() {
            target.doc_name.clone()
        } else {
            clean_display_doc_name(&target.doc_name)
        };
        let place = target.locator.target_label();
        let excerpt = target.excerpt.replace('\n', " ");
        let lead_badge: AnyElement = if target.is_code() {
            div()
                .size(px(18.))
                .flex_none()
                .rounded(px(4.))
                .bg(s.pro.purple.opacity(0.14))
                .flex()
                .items_center()
                .justify_center()
                .child(pro_icon(ProIcon::Code, 11., s.pro.purple))
                .into_any_element()
        } else {
            render_office_chip(target.kind, s).into_any_element()
        };
        div()
            .w_full()
            .pb(px(4.))
            .child(
                div()
                    .w_full()
                    .h(px(32.))
                    .rounded(px(R_CTRL))
                    .bg(s.soft)
                    .flex()
                    .items_center()
                    .child(
                        Tactile::ghost("cmp-pinned", s)
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .px(px(8.))
                            .gap(px(8.))
                            .tip("点击在右侧面板定位此引用")
                            .child(lead_badge)
                            .child(
                                div()
                                    .flex_none()
                                    .max_w(px(168.))
                                    .truncate()
                                    .text_size(px(FS_UI))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(doc),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .whitespace_nowrap()
                                    .text_size(px(FS_UI))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(format!("· {place}")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(FS_META))
                                    .text_color(s.muted)
                                    .child(excerpt),
                            )
                            .on_press(cx, move |this, window, cx| {
                                this.jump_to_selection_target(&target, window, cx)
                            }),
                    )
                    .child(
                        icon_button("cmp-pinned-clear", ProIcon::Close, "移除引用", s)
                            .mx(px(2.))
                            .on_press(cx, |this, _, cx| {
                                this.pinned_target = None;
                                cx.notify();
                            }),
                    ),
            )
            .into_any_element()
    }

    fn render_add_menu(
        &self,
        placement: MenuPlacement,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let has_doc = self.active_office_tab.is_some();
        let loc = self.ui_locale;
        let rows: Vec<AnyElement> = vec![
            menu_icon_item(
                "cmp-add-selection",
                ProIcon::Pin,
                loc.pick("引用当前文档选区", "Quote document selection"),
                s,
            )
            .disabled(!has_doc)
            .on_press(cx, |this, window, cx| {
                this.bind_active_selection_to_chat(window, cx)
            })
            .into_any_element(),
            menu_item(
                "cmp-add-skills",
                Some(render_skills_puzzle_icon(ICON, s.muted)),
                loc.pick("插入技能或指令", "Insert skill or command"),
                s,
            )
            .on_press(cx, |this, _, cx| this.toggle_menu(Menu::ComposerSkills, cx))
            .into_any_element(),
            menu_divider(s).into_any_element(),
            menu_item(
                "cmp-add-open",
                Some(render_folder_icon(ICON, s.muted)),
                loc.pick("打开文档…", "Open document…"),
                s,
            )
            .child(menu_hint("⌘O", s))
            .on_press(cx, |this, window, cx| this.pick_documents(window, cx))
            .into_any_element(),
        ];
        self.render_menu(Menu::ComposerAdd, placement, 244., rows, s, cx)
    }

    fn render_skills_menu(
        &self,
        placement: MenuPlacement,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut rows: Vec<AnyElement> = Vec::new();
        if !self.workspace_inventory.skills.is_empty() {
            rows.push(menu_caption("工作区技能 ($skill)", s).into_any_element());
            for (ix, sk) in self
                .workspace_inventory
                .skills
                .iter()
                .filter(|sk| sk.enabled)
                .take(5)
                .enumerate()
            {
                let token = format!("${} ", sk.name);
                let label = format!("${}", sk.name);
                rows.push(
                    menu_item(
                        SharedString::from(format!("cmp-sk-{ix}")),
                        Some(render_skills_puzzle_icon(ICON_SM, s.muted)),
                        label,
                        s,
                    )
                    .on_press(cx, move |this, win, cx| {
                        this.open_menu = None;
                        this.inject_token_into_composer(&token, win, cx);
                    })
                    .into_any_element(),
                );
            }
            rows.push(menu_divider(s).into_any_element());
        }
        if !self.workspace_inventory.commands.is_empty() {
            rows.push(menu_caption("快捷斜杠指令", s).into_any_element());
            for (ix, cmd) in self.workspace_inventory.commands.iter().take(4).enumerate() {
                let token = format!("{} ", cmd.name);
                rows.push(
                    menu_icon_item(
                        SharedString::from(format!("cmp-cmd-{ix}")),
                        ProIcon::Bolt,
                        cmd.name.clone(),
                        s,
                    )
                    .on_press(cx, move |this, win, cx| {
                        this.open_menu = None;
                        this.inject_token_into_composer(&token, win, cx);
                    })
                    .into_any_element(),
                );
            }
            rows.push(menu_divider(s).into_any_element());
        }
        rows.push(
            menu_item(
                "cmp-open-skills-hub",
                Some(render_skills_puzzle_icon(ICON, s.muted)),
                "打开技能与 MCP 中心…",
                s,
            )
            .child(menu_hint("⇧⌘M", s))
            .on_press(cx, |this, _, cx| this.toggle_skills_sheet(cx))
            .into_any_element(),
        );
        self.render_menu(Menu::ComposerSkills, placement, 244., rows, s, cx)
    }

    fn render_approval_menu(
        &self,
        placement: MenuPlacement,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = self.approval_mode;
        let loc = self.ui_locale;
        let mut rows: Vec<AnyElement> = vec![
            menu_caption(
                loc.pick("智能体请求权限时", "When an agent requests permission"),
                s,
            )
            .into_any_element(),
        ];
        for (ix, (mode, title, detail)) in [
            (
                ApprovalMode::Host,
                loc.pick("需确认", "Ask for approval"),
                loc.pick(
                    "将智能体的权限请求交给你处理",
                    "Review permission requests in the conversation",
                ),
            ),
            (
                ApprovalMode::Deny,
                loc.pick("自动拒绝", "Deny automatically"),
                loc.pick(
                    "自动拒绝需要额外授权的操作",
                    "Decline operations that need additional permission",
                ),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let chosen = current == mode;
            rows.push(
                Tactile::ghost(("cmp-approval-option", ix), s)
                    .role(Role::ListBoxOption)
                    .w_full()
                    .flex_none()
                    .px(px(8.))
                    .py(px(6.))
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(FS_UI))
                                    .line_height(px(LH_UI))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(FS_META))
                                    .line_height(px(LH_META))
                                    .text_color(s.muted)
                                    .child(detail),
                            ),
                    )
                    .when(chosen, |row| row.child(menu_check(s)))
                    .on_press(cx, move |this, _, cx| {
                        this.approval_mode = mode;
                        this.open_menu = None;
                        cx.notify();
                    })
                    .into_any_element(),
            );
        }
        self.render_menu(Menu::ComposerApproval, placement, 268., rows, s, cx)
    }

    pub(crate) fn render_effort_menu(
        &self,
        menu: Menu,
        placement: MenuPlacement,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut rows = Vec::new();
        for (i, value) in std::iter::once(None)
            .chain(self.offered_efforts().iter().map(|v| Some(v.as_str())))
            .enumerate()
        {
            let chosen = value == self.selected_effort();
            let label = value
                .unwrap_or(self.ui_locale.pick("使用模型默认值", "Model default"))
                .to_owned();
            let value = value.map(str::to_owned);
            rows.push(
                menu_item(("effort-level", i), None, label, s)
                    .when(chosen, |d| d.child(menu_check(s)))
                    .on_press(cx, move |this, _, cx| {
                        this.reasoning_effort = value.clone();
                        this.open_menu = None;
                        cx.notify();
                    })
                    .into_any_element(),
            );
        }
        self.render_menu(menu, placement, 192., rows, s, cx)
    }

    fn render_agent_menu(
        &self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.open_menu == Some(Menu::ComposerAgent);
        let loc = self.ui_locale;
        let bench = cx.weak_entity();
        let search = window.use_keyed_state("composer-model-search", cx, |win, cx| {
            ModelPickerSearch::new(bench.clone(), win, cx)
        });
        if search.read(cx).open != open {
            search.update(cx, |view, cx| {
                view.open = open;
                if open {
                    view.provider = self.selected_runtime.clone();
                    view.model = self.selected_model.clone();
                    let provider = self.providers.iter().find(|p| p.id == view.provider);
                    view.editor.update(cx, |editor, cx| {
                        editor.set_placeholder(loc.pick("搜索模型…", "Search models…"));
                        editor.set_value("", window, cx);
                    });
                    view.query.clear();
                    view.cursor = picker_models(provider, "", loc)
                        .iter()
                        .position(|(model, _)| *model == self.selected_model)
                        .unwrap_or_default();
                    view.scroll.scroll_to_item(view.cursor);
                    view.focus_on_mount = true;
                }
            });
        }
        let runtime = search.read(cx).provider.clone();
        let model = search.read(cx).model.clone();
        let provider = self.providers.iter().find(|p| p.id == runtime);
        let options = picker_models(provider, &search.read(cx).query, loc);
        let cursor = search.read(cx).cursor;
        let scroll = search.read(cx).scroll.clone();
        let field = search.read(cx).editor.clone();
        let width = (f32::from(window.viewport_size().width) - 20.).min(554.);
        let top_margin = if cfg!(target_os = "macos") && width < 480. {
            44.
        } else {
            10.
        };
        let height = (f32::from(window.viewport_size().height) - top_margin - 10.).min(393.);
        let ready = provider.is_some_and(ProviderStatusInfo::is_ready);
        let mut installed: Vec<_> = self
            .providers
            .iter()
            .filter(|p| p.readiness() != ProviderReadiness::NotInstalled)
            .collect();
        installed.sort_by_key(|p| p.readiness());

        let mut rail = div()
            .id("picker-agents")
            .flex_none()
            .w(px(44.))
            .h_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .p(px(5.))
            .gap(px(4.))
            .border_r_1()
            .border_color(s.line_soft);
        for (i, p) in installed.into_iter().enumerate() {
            let selected = p.id == runtime;
            let name = p.display_name();
            let id = p.id.clone();
            let search = search.clone();
            rail = rail.child(
                Tactile::ghost(("cmp-agent-option", i), s)
                    .size(px(32.))
                    .flex_none()
                    .justify_center()
                    .lit(selected)
                    .tip(name.clone())
                    .aria(name)
                    .child(render_runtime_mark(&id, 18., s, cx))
                    .on_press(cx, move |this, win, cx| {
                        search.update(cx, |view, cx| {
                            view.provider.clone_from(&id);
                            view.model = if view.provider == this.selected_runtime {
                                this.selected_model.clone()
                            } else {
                                None
                            };
                            view.cursor = 0;
                            view.scroll.scroll_to_item(0);
                            view.editor.update(cx, |editor, cx| {
                                editor.set_value("", win, cx);
                                editor.focus(win, cx);
                            });
                            cx.notify();
                        });
                    }),
            );
        }
        let input = div()
            .h(px(43.))
            .flex_none()
            .mx(px(12.))
            .flex()
            .items_center()
            .gap(px(6.))
            .border_b_1()
            .border_color(s.line_soft)
            .child(pro_icon(ProIcon::Search, 15., s.muted))
            .child(div().flex_1().min_w_0().h(px(30.)).child(field));
        let mut list = div()
            .id("picker-models")
            .role(Role::ListBox)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .px(px(6.))
            .py(px(5.));
        for (index, (model, label)) in options.iter().enumerate() {
            let chosen = *model == search.read(cx).model;
            let model = model.clone();
            let search = search.clone();
            list = list.child(
                Tactile::ghost(("cmp-model-opt", index), s)
                    .role(Role::ListBoxOption)
                    .w_full()
                    .h(px(32.))
                    .gap(px(8.))
                    .px(px(8.))
                    .lit(cursor == index)
                    .lit_fill(s.hover)
                    .aria(label.clone())
                    .child(render_runtime_mark(&runtime, 15., s, cx))
                    .child(div().min_w_0().flex_1().truncate().child(label.clone()))
                    .when(chosen, |row| {
                        row.child(pro_icon(ProIcon::Check, 13., s.text))
                    })
                    .on_press(cx, move |_, win, cx| {
                        search.update(cx, |view, cx| {
                            view.model = model.clone();
                            view.cursor = index;
                            view.editor.read(cx).focus.clone().focus(win, cx);
                            cx.notify();
                        });
                    }),
            );
        }
        if options.is_empty() {
            let message = match provider.map(ProviderStatusInfo::readiness) {
                Some(ProviderReadiness::NeedsLogin) => {
                    loc.pick("此智能体需要登录。", "Sign in to use this agent.")
                }
                Some(ProviderReadiness::Broken) => {
                    loc.pick("此智能体无法启动。", "This agent could not start.")
                }
                _ if ready => loc.pick("没有匹配的模型", "No matching models"),
                _ if self.providers_probing => {
                    loc.pick("正在检查本地智能体…", "Checking local agents…")
                }
                _ => loc.pick("请选择已安装的智能体", "Select an installed agent"),
            };
            list = list.child(div().p(px(10.)).text_color(s.muted).child(message));
        }
        let model_list = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(input)
            .child(list);
        let levels = provider.map_or(&[][..], |p| p.effort_levels(model.as_deref()));
        let effort = self
            .selected_effort()
            .filter(|effort| {
                runtime == self.selected_runtime && levels.iter().any(|level| level == effort)
            })
            .map(str::to_owned);
        let mut body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(rail)
            .child(model_list);
        if ready && !levels.is_empty() {
            let mut detail = div()
                .id("picker-reasoning")
                .w(px(if width < 480. { 110. } else { 192. }))
                .h_full()
                .flex_none()
                .overflow_y_scroll()
                .px(px(8.))
                .bg(s.tray)
                .border_l_1()
                .border_color(s.line_soft)
                .child(
                    div()
                        .h(px(43.))
                        .flex()
                        .items_center()
                        .px(px(5.))
                        .text_color(s.muted)
                        .text_size(px(12.))
                        .child(loc.pick("思考深度", "Reasoning")),
                );
            for (i, level) in std::iter::once(None)
                .chain(levels.iter().map(|v| Some(v.as_str())))
                .enumerate()
            {
                let default_label = if width < 480. {
                    loc.pick("默认", "Default")
                } else {
                    loc.pick("模型默认", "Model default")
                };
                let label = level
                    .map_or(default_label, |value| effort_label(value, loc))
                    .to_owned();
                let chosen = level == effort.as_deref();
                let value = level.map(str::to_owned);
                let runtime = runtime.clone();
                let model = model.clone();
                detail = detail.child(
                    Tactile::ghost(("cmp-effort-opt", i), s)
                        .w_full()
                        .h(px(32.))
                        .gap(px(4.))
                        .px(px(5.))
                        .aria(label.clone())
                        .child(div().flex_1().min_w_0().truncate().child(label))
                        .when(chosen, |row| {
                            row.child(pro_icon(ProIcon::Check, 13., s.text))
                        })
                        .on_press(cx, move |this, win, cx| {
                            this.apply_model_picker(
                                &runtime,
                                model.clone(),
                                value.clone(),
                                win,
                                cx,
                            );
                        }),
                );
            }
            body = body.child(detail);
        }
        let footer = div()
            .flex_none()
            .h(px(44.))
            .px(px(8.))
            .flex()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(s.line_soft)
            .child(
                Tactile::ghost("cmp-agent-all", s)
                    .h(px(28.))
                    .px(px(7.))
                    .gap(px(6.))
                    .child(pro_icon(ProIcon::Settings, 14., s.muted))
                    .child(loc.pick("管理智能体", "Manage agents"))
                    .on_press(cx, |this, _, cx| this.toggle_provider_sheet(cx)),
            )
            .child(
                Tactile::soft("cmp-model-done", s)
                    .h(px(28.))
                    .px(px(10.))
                    .disabled(!ready)
                    .child(loc.pick("完成", "Done"))
                    .on_press(cx, move |this, win, cx| {
                        this.apply_model_picker(&runtime, model.clone(), effort.clone(), win, cx)
                    }),
            );
        let s = *s;
        pro_presence(Menu::ComposerAgent.id(), open, 0.16, 0.12, move |amount| {
            let surface = div()
                .id("composer-model-picker")
                .role(Role::Dialog)
                .occlude()
                .w(px(width))
                .h(px(height))
                .rounded(px(12.))
                .overflow_hidden()
                .border_1()
                .border_color(s.line)
                .bg(s.card)
                .shadow(s.popover_shadow())
                .flex()
                .flex_col()
                .opacity(amount)
                .text_size(px(13.))
                .text_color(s.text)
                .child(body)
                .child(footer)
                // The animated layer must mount the editor before transferring focus from its trigger.
                .when(open, |d| {
                    d.child(
                        canvas(
                            |_, _, _| (),
                            move |_, _, win, cx| {
                                if search.read(cx).focus_on_mount {
                                    search.update(cx, |v, _| v.focus_on_mount = false);
                                    win.defer(cx, move |win, cx| {
                                        let search = search.read(cx);
                                        if search.open {
                                            search.editor.read(cx).focus.clone().focus(win, cx);
                                        }
                                    });
                                }
                            },
                        )
                        .absolute()
                        .size_0(),
                    )
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down_out(move |_, _, cx| {
                    let _ = bench.update(cx, |this, cx| this.dismiss_menu(Menu::ComposerAgent, cx));
                });
            div()
                .absolute()
                .right_0()
                .top_0()
                .size_0()
                .when(open, |d| {
                    d.child(
                        deferred(
                            div()
                                .absolute()
                                .size_0()
                                .child(kantor_ui::overlay::shield()),
                        )
                        .with_priority(3),
                    )
                })
                .child(
                    deferred(
                        anchored()
                            .anchor(Anchor::BottomRight)
                            .offset(point(px(0.), px(-6. + 4. * (1. - amount))))
                            .snap_to_window_with_margin(Edges {
                                top: px(top_margin),
                                ..Edges::all(px(10.))
                            })
                            .child(surface),
                    )
                    .with_priority(3),
                )
                .into_any_element()
        })
    }

    fn apply_model_picker(
        &mut self,
        runtime: &str,
        model: Option<String>,
        effort: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_runtime_by_id(runtime, cx);
        self.selected_model = model;
        self.reasoning_effort = effort.filter(|level| self.offered_efforts().contains(level));
        self.close_model_picker(window, cx);
    }

    fn close_model_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(cx);
        self.composer_editor
            .read(cx)
            .focus
            .clone()
            .focus(window, cx);
    }

    // ── Thread ──────────────────────────────────────────────────────────────

    fn render_queued_turn(
        &self,
        queued: &QueuedPrompt,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let agent = runtime_display_name(&self.selected_runtime);
        let mut asked = div().w_full().flex().flex_col().items_end().gap(px(8.));
        if let Some(target) = queued.target.clone() {
            asked = asked.child(self.render_target_reference("queued", target, s, cx));
        }
        asked = asked.child(user_bubble(queued.text.clone(), s));
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(asked)
            .child(working_line(
                spinner(ICON_SM, s.muted, window, cx),
                format!("正在启动 {agent}"),
                "queued-status",
                s,
                window,
                cx,
            ))
    }

    fn render_target_reference(
        &self,
        key: &str,
        target: SelectionTarget,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Tactile {
        let doc = if target.is_code() {
            target.doc_name.clone()
        } else {
            clean_display_doc_name(&target.doc_name)
        };
        let place = target.locator.target_label();
        let lead_badge: AnyElement = if target.is_code() {
            div()
                .size(px(18.))
                .flex_none()
                .rounded(px(4.))
                .bg(s.pro.purple.opacity(0.14))
                .flex()
                .items_center()
                .justify_center()
                .child(pro_icon(ProIcon::Code, 11., s.pro.purple))
                .into_any_element()
        } else {
            render_office_chip(target.kind, s).into_any_element()
        };
        Tactile::soft(SharedString::from(format!("turn-target-{key}")), s)
            .max_w(relative(0.8))
            .h(px(CTRL_H))
            .px(px(8.))
            .gap(px(8.))
            .tip("在右侧面板中打开查看")
            .child(lead_badge)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(FS_UI))
                    .text_color(s.text)
                    .child(format!("{doc} · {place}")),
            )
            .on_press(cx, move |this, window, cx| {
                this.jump_to_selection_target(&target, window, cx)
            })
    }

    fn render_active_tasks(
        &self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let live: Vec<_> = self
            .projected
            .tasks
            .iter()
            .filter(|t| {
                matches!(
                    t.status,
                    TaskStatus::Pending | TaskStatus::Running | TaskStatus::Paused
                )
            })
            .cloned()
            .collect();
        if live.is_empty() {
            return None;
        }
        let can_stop = self.projected.can_stop_task
            && matches!(self.access_mode, ChatAccessMode::WriteController { .. });
        let mut box_el = div()
            .w_full()
            .px(px(12.))
            .py(px(8.))
            .rounded(px(R_CARD))
            .bg(s.soft)
            .flex()
            .flex_col()
            .gap(px(6.));
        for (ix, task) in live.into_iter().enumerate() {
            let label = task
                .summary
                .clone()
                .or_else(|| task.details.description.clone())
                .unwrap_or_else(|| format!("后台任务 {}", task.id));
            let tid = task.id.clone();
            box_el = box_el.child(
                div()
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
                            .gap(px(8.))
                            .child(spinner(ICON_XS, s.muted, window, cx))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(FS_META))
                                    .text_color(s.text)
                                    .child(label),
                            ),
                    )
                    .when(can_stop, |d| {
                        d.child(
                            text_button(("stop-task", ix), "停止", s).on_press(
                                cx,
                                move |this, _, _| {
                                    this.bridge.send(KarCommand::StopTask {
                                        task_id: tid.clone(),
                                    });
                                },
                            ),
                        )
                    }),
            );
        }
        Some(box_el.into_any_element())
    }

    fn render_turn(
        &mut self,
        turn_index: usize,
        turn: &ChatTurn,
        inline_review: bool,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let turn_id = turn.id;
        let action_group: SharedString = format!("turn-actions-hover-{turn_id}").into();
        let running = turn.status == TurnStatus::Running;

        // ── What was asked (target + attachments + user bubbles) ────────────
        let mut asked = div().w_full().flex().flex_col().items_end().gap(px(8.));
        if let Some(target) = self.turn_targets.get(&turn_id).cloned() {
            asked = asked.child(self.render_target_reference(&turn_id.to_string(), target, s, cx));
        }
        if !turn.attachments.is_empty() {
            let mut att_row = div()
                .max_w(relative(0.84))
                .flex()
                .flex_wrap()
                .justify_end()
                .gap(px(6.));
            for att in &turn.attachments {
                att_row = att_row.child(
                    div()
                        .h(px(24.))
                        .px(px(8.))
                        .rounded(px(R_CTRL))
                        .bg(s.soft)
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .text_size(px(FS_META))
                        .text_color(s.text)
                        .child(pro_icon(
                            if att.kind == AttachmentKind::Image {
                                ProIcon::Sparkles
                            } else {
                                ProIcon::Paperclip
                            },
                            ICON_XS,
                            s.muted,
                        ))
                        .child(att.name.clone()),
                );
            }
            asked = asked.child(att_row);
        }

        let mut first_ask = String::new();
        for item in &turn.items {
            if let Item::Message {
                role: KarRole::User,
                text,
                ..
            } = &item.item
            {
                let own = strip_selection_envelope(text);
                // Strip internal "Attached files (JSON):" footer if Kar appended it.
                let clean = own
                    .split("\n\nAttached files (JSON):\n")
                    .next()
                    .unwrap_or(own)
                    .trim()
                    .to_owned();
                if !clean.is_empty() {
                    if first_ask.is_empty() {
                        first_ask.clone_from(&clean);
                    }
                    asked = asked.child(user_bubble(clean, s));
                }
            }
        }

        // ── What the agent did (`Worked for Xs >` + Prose/Tables + Copy + Changed Files) ─
        let loc = self.ui_locale;
        let duration = turn.finished_ms.and_then(|end| end.checked_sub(turn.started_ms));
        let mut parts = div()
            .w_full()
            .min_w_0()
            .pl(px(4.))
            .flex()
            .flex_col()
            .items_start()
            .gap(px(14.));
        let mut has_parts = false;
        let mut touched: Vec<(String, &'static str)> = Vec::new();

        // Collect file changes from tools and check whether any tool failed or is still running
        let mut work_items_count = 0usize;
        let mut any_work_failed = false;
        for ti in &turn.items {
            match &ti.item {
                Item::Message {
                    role: KarRole::Reasoning,
                    text,
                    ..
                } if !text.trim().is_empty() => {
                    work_items_count += 1;
                }
                Item::Tool { activity, .. } => {
                    work_items_count += 1;
                    if tool_phase(activity) == ToolPhase::Failed {
                        any_work_failed = true;
                    }
                    if let Some(result) = &activity.result {
                        for change in &result.file_changes {
                            touched.push((
                                change.path.clone(),
                                match change.kind {
                                    ToolChangeKind::Added => "新增",
                                    ToolChangeKind::Deleted => "删除",
                                    ToolChangeKind::Modified | ToolChangeKind::Unknown => "修改",
                                },
                            ));
                        }
                    }
                }
                _ => {}
            }
        }

        // T3 Code `Worked for 2m 14s >` collapsible work-group header for finished turns
        let collapse_work_group = !running && !any_work_failed && work_items_count > 0;
        let work_grp_key = (turn_id, 900_000);
        let work_grp_open = self.part_is_open(work_grp_key, false);

        if collapse_work_group {
            has_parts = true;
            let worked_label = format_worked_header(duration, work_items_count, loc);
            let mut wg_body = div()
                .w_full()
                .pl(px(12.))
                .mt(px(4.))
                .border_l_1()
                .border_color(s.line)
                .flex()
                .flex_col()
                .gap(px(6.));
            if work_grp_open {
                let mut wix = 0;
                while wix < turn.items.len() {
                    let sub = !turn.items[wix].source.path.is_empty();
                    match &turn.items[wix].item {
                        Item::Message {
                            role: KarRole::Reasoning,
                            text,
                            ..
                        } if !text.trim().is_empty() => {
                            wg_body = wg_body.child(
                                div()
                                    .text_size(px(FS_META))
                                    .line_height(px(18.))
                                    .text_color(s.muted)
                                    .child(text.trim().to_owned()),
                            );
                        }
                        Item::Tool { .. } => {
                            let st = wix;
                            let mut calls: Vec<(usize, &Tool)> = Vec::new();
                            while let Some(Item::Tool { activity, .. }) =
                                turn.items.get(wix).map(|i| &i.item)
                            {
                                calls.push((wix, activity));
                                wix += 1;
                            }
                            wg_body = wg_body.child(self.render_tool_run(
                                turn_id, st, &calls, sub, s, window, cx,
                            ));
                            continue;
                        }
                        _ => {}
                    }
                    wix += 1;
                }
            }
            parts = parts.child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_start()
                    .child(
                        Tactile::ghost(
                            SharedString::from(format!("worked-grp-{turn_id}")),
                            s,
                        )
                        .h(px(26.))
                        .px(px(6.))
                        .ml(px(-6.))
                        .gap(px(5.))
                        .font_features(tnum_features())
                        .text_size(px(13.5))
                        .ink(s.muted, s.ink)
                        .child(worked_label)
                        .child(pro_icon(
                            if work_grp_open {
                                ProIcon::ChevronDown
                            } else {
                                ProIcon::ChevronRight
                            },
                            11.5,
                            s.muted,
                        ))
                        .on_press(cx, move |this, _, cx| {
                            this.part_open.insert(work_grp_key, !work_grp_open);
                            cx.notify();
                        }),
                    )
                    .child(pro_fold(
                        SharedString::from(format!("worked-grp-fold-{turn_id}")),
                        work_grp_open,
                        (work_items_count as f32 * 44. + 48.).clamp(72., 280.),
                        0.,
                        move || wg_body.into_any_element(),
                    )),
            );
        }

        let mut ix = 0;
        while ix < turn.items.len() {
            let is_subagent = !turn.items[ix].source.path.is_empty();
            match &turn.items[ix].item {
                Item::Message {
                    role: KarRole::User,
                    ..
                } => {}
                Item::Message {
                    role: KarRole::Reasoning,
                    text,
                    complete,
                    ..
                } => {
                    if !collapse_work_group && !text.trim().is_empty() {
                        has_parts = true;
                        parts = parts.child(self.render_reasoning(
                            (turn_id, ix),
                            text.trim(),
                            !*complete && running,
                            is_subagent,
                            s,
                            window,
                            cx,
                        ));
                    }
                }
                Item::Message {
                    role: KarRole::Assistant,
                    text,
                    complete,
                    ..
                } => {
                    if !text.trim().is_empty() {
                        has_parts = true;
                        parts = parts.child(
                            div()
                                .w_full()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .when(is_subagent, |d| {
                                    d.child(
                                        div()
                                            .text_size(px(FS_TAG))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.muted)
                                            .child(loc.pick("子智能体回复", "Subagent")),
                                    )
                                })
                                .child(self.render_assistant_markdown_blocks(
                                    turn_id,
                                    ix,
                                    text,
                                    !*complete && running,
                                    s,
                                    cx,
                                )),
                        );
                    }
                }
                Item::Tool { .. } => {
                    let start = ix;
                    let mut calls: Vec<(usize, &Tool)> = Vec::new();
                    while let Some(Item::Tool { activity, .. }) =
                        turn.items.get(ix).map(|item| &item.item)
                    {
                        calls.push((ix, activity));
                        ix += 1;
                    }
                    if !collapse_work_group {
                        has_parts = true;
                        parts = parts.child(self.render_tool_run(
                            turn_id,
                            start,
                            &calls,
                            is_subagent,
                            s,
                            window,
                            cx,
                        ));
                    }
                    continue;
                }
                Item::Retry { status } => {
                    has_parts = true;
                    let note = match (status.attempt, status.max_attempts) {
                        (Some(a), Some(m)) => format!("正在自动重试（第 {a}/{m} 次）"),
                        (Some(a), None) => format!("正在自动重试（第 {a} 次）"),
                        _ => "连接波动，正在自动重试".to_owned(),
                    };
                    let full = match status.reason.as_deref().filter(|r| !r.trim().is_empty()) {
                        Some(r) => format!("{note}：{r}"),
                        None => note,
                    };
                    parts = parts.child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(FS_META))
                            .text_color(s.muted)
                            .child(pro_icon(ProIcon::Refresh, ICON_XS, s.muted))
                            .child(div().min_w_0().truncate().child(full)),
                    );
                }
                Item::Compaction { .. } => {
                    has_parts = true;
                    parts = parts.child(
                        div()
                            .w_full()
                            .py(px(4.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(div().flex_1().h(px(1.)).bg(s.line))
                            .child(
                                div()
                                    .text_size(px(FS_META))
                                    .text_color(s.muted)
                                    .child("更早的对话已整理为摘要"),
                            )
                            .child(div().flex_1().h(px(1.)).bg(s.line)),
                    );
                }
                Item::Diff { diff, .. } => {
                    has_parts = true;
                    parts = parts.child(self.render_diff((turn_id, ix), diff, s, cx));
                }
                Item::RequestError { error } => {
                    has_parts = true;
                    let is_unconfirmed =
                        error.status == kar::RequestDeliveryStatus::Unconfirmed;
                    let badge = if is_unconfirmed {
                        "送达结果未确认 · 未自动重放"
                    } else {
                        "交互答复未送达"
                    };
                    let (bg, icon_color) = if is_unconfirmed {
                        (s.pro.amber_soft, s.pro.amber)
                    } else {
                        (s.pro.red_soft, s.pro.red)
                    };
                    parts = parts.child(
                        div()
                            .w_full()
                            .px(px(12.))
                            .py(px(8.))
                            .rounded(px(R_CARD))
                            .bg(bg)
                            .flex()
                            .items_start()
                            .gap(px(8.))
                            .child(div().pt(px(2.)).flex_none().child(pro_icon(
                                ProIcon::AlertTriangle,
                                ICON_SM,
                                icon_color,
                            )))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(FS_META))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.ink)
                                            .child(badge),
                                    )
                                    .when(!error.reason.trim().is_empty(), |d| {
                                        d.child(
                                            div()
                                                .text_size(px(FS_META))
                                                .text_color(s.text)
                                                .child(error.reason.trim().to_owned()),
                                        )
                                    }),
                            ),
                    );
                }
            }
            ix += 1;
        }

        // ── Gather turn changed files ───────────────────────────────────────
        let checkpoint = turn.checkpoint.as_ref();
        let local = matches!(
            self.access_mode,
            ChatAccessMode::WriteController { .. }
                | ChatAccessMode::LocalReadOnlySubscriber { .. }
                | ChatAccessMode::ArchivedReadOnly { .. }
        );
        let has_after_tree = checkpoint.is_some_and(|cp| cp.after_tree.is_some());
        let has_checkpoint_changes = checkpoint.is_some_and(|cp| !cp.files.is_empty());
        let reviewable = local && has_after_tree;
        let mut files = Vec::new();
        if let Some(cp) = checkpoint {
            for change in &cp.files {
                let verb = match change.kind {
                    CheckpointChangeKind::Added => "新增",
                    CheckpointChangeKind::Modified => "修改",
                    CheckpointChangeKind::Deleted => "删除",
                };
                files.push((change.path.clone(), verb, reviewable));
            }
        }
        for (path, verb) in touched {
            let path = PathBuf::from(path);
            if !files.iter().any(|(seen, _, _)| seen == &path) {
                files.push((path, verb, false));
            }
        }

        // ── What went wrong, in plain words ─────────────────────────────────
        if let Some(failure) = turn.failure.as_ref() {
            has_parts = true;
            let copy = describe_failure(failure);
            parts = parts.child(
                div()
                    .w_full()
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(R_CARD))
                    .bg(s.pro.red_soft)
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(div().pt(px(3.)).flex_none().child(pro_icon(
                        ProIcon::AlertTriangle,
                        ICON_SM,
                        s.pro.red,
                    )))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .text_size(px(FS_UI))
                            .line_height(px(LH_UI))
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(copy.title),
                            )
                            .when(!copy.detail.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_size(px(FS_META))
                                        .line_height(px(LH_META))
                                        .text_color(s.text)
                                        .child(copy.detail),
                                )
                            })
                            .when_some(copy.hint, |d, hint| {
                                d.child(
                                    div()
                                        .text_size(px(FS_META))
                                        .text_color(s.muted)
                                        .child(hint),
                                )
                            }),
                    ),
            );
        }

        // ── While it works: KUI `TextShimmer` + animated `AiSpark` ───────────
        if running {
            has_parts = true;
            let lead = self
                .turn_sparks
                .get(&turn_id)
                .cloned()
                .map(IntoElement::into_any_element)
                .unwrap_or_else(|| spinner(ICON_SM, s.muted, window, cx));
            let activity_txt = self.describe_activity(turn);
            self.working_shimmer
                .update(cx, |sh, cx| sh.set_text(activity_txt, cx));
            parts = parts.child(
                div()
                    .id(SharedString::from(format!("turn-status-{turn_id}")))
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(lead)
                    .child(self.working_shimmer.clone()),
            );
        }

        // ── Subtle right-aligned Copy button + left turn actions (`hero.webp`) ─
        let stopped = match turn.status {
            TurnStatus::Interrupted | TurnStatus::Aborted => Some(loc.pick("已停止", "Stopped")),
            TurnStatus::Failed if turn.failure.is_none() => Some(loc.pick("未完成", "Incomplete")),
            _ => None,
        };
        let writable_real = matches!(self.access_mode, ChatAccessMode::WriteController { .. })
            && !self.projected.is_demo;
        let idle = !self.turn_in_progress();
        let can_retry = matches!(
            turn.status,
            TurnStatus::Failed | TurnStatus::Interrupted | TurnStatus::Aborted
        ) && writable_real;
        let can_fork =
            !running && idle && writable_real && self.projected.can_fork && has_after_tree;
        let can_rollback_after = !running && idle && writable_real && has_after_tree;
        let can_undo_turn =
            !running && idle && writable_real && has_checkpoint_changes && turn_index == 0;
        let current_cid = self.current_chat_id();

        let copy_button = (!running)
            .then(|| self.copy_buttons.get(&turn_id).cloned())
            .flatten();

        let show_hold_rollback = (can_rollback_after && has_checkpoint_changes) || can_undo_turn;
        let extra_actions_key = (turn_id, usize::MAX);
        let extra_actions_open = self.part_is_open(extra_actions_key, false);
        let rollback_hold = if show_hold_rollback && extra_actions_open {
            let before_flag = can_undo_turn && !(can_rollback_after && has_checkpoint_changes);
            Some(
                self.rollback_hold_buttons
                    .entry(turn_id)
                    .or_insert_with(|| {
                        let btn = cx.new(|cx| {
                            HoldToConfirm::new(
                                SharedString::from(format!("hold-rb-{turn_id}")),
                                if before_flag {
                                    "按住撤销改动"
                                } else {
                                    "按住恢复本轮结束时的文件"
                                },
                                window,
                                cx,
                            )
                            .with_duration(Duration::from_millis(950))
                            .with_done_label("已回退")
                            .danger()
                        });
                        cx.subscribe(&btn, move |this, btn_ent, ev: &HoldEvent, cx| {
                            if *ev == HoldEvent::Confirmed {
                                this.bridge.send(KarCommand::RollbackToTurn {
                                    turn_id,
                                    before: before_flag,
                                });
                                btn_ent.update(cx, |b, cx| b.reset(cx));
                            }
                        })
                        .detach();
                        btn
                    })
                    .clone(),
            )
        } else {
            None
        };

        if copy_button.is_some()
            || stopped.is_some()
            || can_retry
            || can_fork
            || show_hold_rollback
        {
            has_parts = true;
            parts = parts.child(
                div()
                    .w_full()
                    .min_h(px(26.))
                    .when(!window.last_input_was_keyboard() && !window.last_input_was_touch() && stopped.is_none() && !can_retry && !extra_actions_open, |row| {
                        row.opacity(0.).group_hover(action_group.clone(), |hover| hover.opacity(1.))
                    })
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(2.))
                            .when_some(stopped, |d, note| {
                                d.child(
                                    div()
                                        .px(px(4.))
                                        .text_size(px(FS_META))
                                        .text_color(s.muted)
                                        .child(note),
                                )
                            })
                            .when(can_retry, |d| {
                                d.child(
                                    text_button(
                                        SharedString::from(format!("retry-{turn_id}")),
                                        loc.pick("重试", "Retry"),
                                        s,
                                    )
                                    .on_press(cx, move |this, _, cx| {
                                        this.retry_turn_with_same_uuid(
                                            turn_id,
                                            first_ask.clone(),
                                            cx,
                                        )
                                    }),
                                )
                            })
                            .when_some(can_fork.then_some(current_cid).flatten(), |d, cid| {
                                d.child(
                                    Tactile::ghost(
                                        SharedString::from(format!("fork-{turn_id}")),
                                        s,
                                    )
                                    .h(px(26.))
                                    .px(px(6.))
                                    .gap(px(4.))
                                    .text_size(px(FS_META))
                                    .ink(s.muted, s.ink)
                                    .tip(loc.pick("从此步状态分叉新对话", "Fork thread from here"))
                                    .child(pro_icon(ProIcon::GitBranch, ICON_XS, s.muted))
                                    .child(loc.pick("分叉", "Fork"))
                                    .on_press(cx, move |this, _, _| {
                                        this.bridge.send(KarCommand::ForkChatAt {
                                            chat_id: cid,
                                            turn_id,
                                            model_override: None,
                                        });
                                    }),
                                )
                            })
                            .when(show_hold_rollback, |d| {
                                d.child(
                                    icon_button(
                                        SharedString::from(format!("turn-actions-{turn_id}")),
                                        ProIcon::MoreHorizontal,
                                        loc.pick("更多本轮操作", "More turn actions"),
                                        s,
                                    )
                                    .lit(extra_actions_open)
                                    .on_press(cx, move |this, _, cx| {
                                        this.part_open
                                            .insert(extra_actions_key, !extra_actions_open);
                                        cx.notify();
                                    }),
                                )
                            })
                            .when_some(rollback_hold, |d, hold_btn| {
                                d.child(div().ml(px(4.)).child(hold_btn))
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .opacity(0.72)
                            .when_some(copy_button, |d, button| d.child(button)),
                    ),
            );
        }

        // ── T3 Code Turn Changed Files Card (`3 changed files  +71 -2  ⌄  [± Open diff]`) ─
        if !files.is_empty() {
            has_parts = true;
            parts = parts.child(self.render_changed_files(turn_id, turn_index, files, s, cx));
        }
        if inline_review && let Some(review) = self
            .turn_review
            .as_ref()
            .filter(|r| r.key.turn == turn_id && Some(r.key.chat) == self.current_chat_id())
        {
            parts = parts.child(self.render_file_review(review, false, s, cx));
        }

        div()
            .group(action_group)
            .w_full()
            .flex()
            .flex_col()
            .gap(px(46.5))
            .child(asked)
            .when(has_parts, |d| d.child(parts))
    }

    /// Render assistant Markdown with T3 Code's signature borderless horizontal-rule tables
    /// (`Check | Result`).
    fn render_assistant_markdown_blocks(
        &self,
        turn_id: Uuid,
        item_ix: usize,
        text: &str,
        streaming: bool,
        s: &StudioPalette,
        _cx: &mut Context<Self>,
    ) -> Div {
        let segments = parse_markdown_segments(text);
        let mut col = div().w_full().min_w_0().flex().flex_col().gap(px(14.));

        for (seg_ix, seg) in segments.into_iter().enumerate() {
            match seg {
                MdSegment::Prose(prose) => {
                    if prose.trim().is_empty() {
                        continue;
                    }
                    col = col.child(
                        div()
                            .w_full()
                            .min_w_0()
                            .text_size(px(14.))
                            .line_height(px(LH_READ))
                            .text_color(s.ink)
                            .child(
                                TextView::markdown(
                                    ElementId::Name(
                                        format!("reply-{turn_id}-{item_ix}-{seg_ix}").into(),
                                    ),
                                    prose,
                                )
                                .selectable(true)
                                .stream_fade(streaming),
                            ),
                    );
                }
                MdSegment::Table { headers, rows } => {
                    let mut tbl = div()
                        .w_full()
                        .my(px(4.))
                        .flex()
                        .flex_col();
                    // Header row (`border-b`)
                    let mut hrow = div()
                        .w_full()
                        .py(px(8.))
                        .border_b_1()
                        .border_color(s.line)
                        .flex()
                        .items_center()
                        .gap(px(16.));
                    for h in &headers {
                        hrow = hrow.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(s.ink)
                                .child(h.clone()),
                        );
                    }
                    tbl = tbl.child(hrow);
                    // Data rows (`border-b` subtle)
                    for r in rows {
                        let mut drow = div()
                            .w_full()
                            .py(px(9.5))
                            .border_b_1()
                            .border_color(s.line_soft)
                            .flex()
                            .items_center()
                            .gap(px(16.));
                        for col_ix in 0..headers.len() {
                            let cell = r.get(col_ix).cloned().unwrap_or_default();
                            drow = drow.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(13.5))
                                    .text_color(if col_ix == 0 { s.text } else { s.ink })
                                    .child(cell),
                            );
                        }
                        tbl = tbl.child(drow);
                    }
                    col = col.child(tbl);
                }
            }
        }
        col
    }

    fn describe_activity(&self, turn: &ChatTurn) -> String {
        if !self.projected.pending_requests.is_empty() {
            return "等待你的确认".into();
        }
        match turn.items.last().map(|item| &item.item) {
            Some(Item::Tool { activity, .. }) if !activity.finished => {
                let what: String = tool_summary(activity).chars().take(48).collect();
                format!("正在执行 {what}")
            }
            Some(Item::Message {
                role: KarRole::Assistant,
                complete: false,
                ..
            }) => "正在回复".into(),
            _ => "正在思考".into(),
        }
    }

    fn part_is_open(&self, key: (Uuid, usize), default_open: bool) -> bool {
        self.part_open.get(&key).copied().unwrap_or(default_open)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_reasoning(
        &self,
        key: (Uuid, usize),
        text: &str,
        live: bool,
        is_subagent: bool,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = self.part_is_open(key, live);
        let lead = if live {
            spinner(ICON_SM, s.muted, window, cx)
        } else {
            pro_icon(ProIcon::Sparkles, ICON_SM, s.muted)
        };
        let label = match (live, is_subagent) {
            (true, true) => "子智能体正在思考",
            (true, false) => "正在思考",
            (false, true) => "子智能体思考过程",
            (false, false) => "思考过程",
        };
        let body = div()
            .text_size(px(FS_UI))
            .line_height(px(LH_UI))
            .text_color(s.muted)
            .child(text.to_owned())
            .into_any_element();
        let estimate = 24. + (text.len() as f32 / 90.).ceil() * LH_UI;
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .items_start()
            .child(
                disclosure_line(
                    SharedString::from(format!("think-{}-{}", key.0, key.1)),
                    lead,
                    label,
                    open,
                    s,
                )
                .on_press(cx, move |this, _, cx| {
                    this.part_open.insert(key, !open);
                    cx.notify();
                }),
            )
            .child(disclosure_body(
                SharedString::from(format!("think-body-{}-{}", key.0, key.1)),
                open,
                estimate,
                body,
                s,
            ))
    }

    #[allow(clippy::too_many_arguments)]
    fn render_tool_run(
        &mut self,
        turn_id: Uuid,
        start_ix: usize,
        calls: &[(usize, &Tool)],
        is_subagent: bool,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = (turn_id, start_ix);
        let phases: Vec<ToolPhase> = calls.iter().map(|(_, tool)| tool_phase(tool)).collect();
        let any_failed = phases.contains(&ToolPhase::Failed);
        let running_call = calls
            .iter()
            .zip(&phases)
            .find(|(_, phase)| **phase == ToolPhase::Running)
            .map(|((_, tool), _)| *tool);
        let open = self.part_is_open(key, any_failed);

        let lead = if running_call.is_some() {
            spinner(ICON_SM, s.muted, window, cx)
        } else if any_failed {
            pro_icon(ProIcon::AlertTriangle, ICON_SM, s.pro.red)
        } else {
            pro_icon(ProIcon::Check, ICON_SM, s.muted)
        };
        let base_label = match (calls, running_call) {
            ([(_, only)], _) => tool_summary(only),
            (_, Some(current)) => tool_summary(current),
            (many, None) => format!("执行了 {} 步操作", many.len()),
        };
        let label = if is_subagent {
            format!("子智能体 · {base_label}")
        } else {
            base_label
        };

        let single = calls.len() == 1;
        let mut body = div().w_full().flex().flex_col().gap(px(2.));
        for ((item_ix, tool), phase) in calls.iter().zip(&phases) {
            let detail_key = (turn_id, *item_ix);
            if single {
                body = body.child(self.render_tool_detail(detail_key, tool, s, window, cx));
                continue;
            }
            let detail_open = self.tool_detail == Some(detail_key);
            let mark = match phase {
                ToolPhase::Running => spinner(ICON_XS, s.muted, window, cx),
                ToolPhase::Done => pro_icon(ProIcon::Check, ICON_XS, s.muted),
                ToolPhase::Failed => pro_icon(ProIcon::AlertTriangle, ICON_XS, s.pro.red),
                ToolPhase::Cancelled => pro_icon(ProIcon::Minus, ICON_XS, s.faint),
            };
            let detail =
                detail_open.then(|| self.render_tool_detail(detail_key, tool, s, window, cx));
            body = body
                .child(
                    Tactile::ghost(
                        SharedString::from(format!("tool-{turn_id}-{item_ix}")),
                        s,
                    )
                    .ml(px(-8.))
                    .max_w_full()
                    .h(px(CTRL_H))
                    .px(px(8.))
                    .gap(px(8.))
                    .text_size(px(FS_UI))
                    .ink(s.text, s.ink)
                    .lit(detail_open)
                    .child(mark)
                    .child(div().min_w_0().truncate().child(tool_summary(tool)))
                    .on_press(cx, move |this, _, cx| {
                        this.tool_detail = (!detail_open).then_some(detail_key);
                        cx.notify();
                    }),
                )
                .child(pro_fold(
                    SharedString::from(format!("tool-detail-{turn_id}-{item_ix}")),
                    detail_open,
                    128.,
                    0.,
                    move || {
                        div()
                            .pb(px(4.))
                            .when_some(detail, |d, detail| d.child(detail))
                            .into_any_element()
                    },
                ));
        }

        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .items_start()
            .child(
                disclosure_line(
                    SharedString::from(format!("tools-{turn_id}-{start_ix}")),
                    lead,
                    label,
                    open,
                    s,
                )
                .on_press(cx, move |this, _, cx| {
                    this.part_open.insert(key, !open);
                    cx.notify();
                }),
            )
            .child(disclosure_body(
                SharedString::from(format!("tools-body-{turn_id}-{start_ix}")),
                open,
                if single {
                    112.
                } else {
                    calls.len() as f32 * 30.
                },
                body.into_any_element(),
                s,
            ))
    }

    fn render_tool_detail(
        &mut self,
        key: (Uuid, usize),
        tool: &Tool,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mono = mono_font(cx);
        let input = tool_input_text(tool);
        let raw_output = tool_output_text(tool);
        let failed = tool_phase(tool) == ToolPhase::Failed;

        let input_el: Option<AnyElement> = input.map(|inp| {
            let trimmed = inp.trim();
            if (trimmed.starts_with('{') || trimmed.starts_with('['))
                && let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed)
                && (val.is_object() || val.is_array())
            {
                let cache_k = format!("tool-in-json-{}-{}", key.0, key.1);
                let jv = self
                    .tool_json_viewers
                    .entry(cache_k)
                    .or_insert_with(|| {
                        cx.new(|cx| JsonViewer::new("参数", val, window, cx).with_max_height(180.))
                    })
                    .clone();
                return div().w_full().child(jv).into_any_element();
            }
            let is_shell = matches!(tool.name.as_deref(), Some("Bash" | "shell"))
                || tool.native_type.as_deref() == Some("commandExecution");
            if is_shell && (trimmed.contains('\n') || trimmed.len() >= 56) {
                let cache_k = format!("tool-in-sh-{}-{}", key.0, key.1);
                let clean_cmd = unwrap_shell_command(trimmed).to_owned();
                let cb = self
                    .code_block_cache
                    .entry(cache_k)
                    .or_insert_with(|| cx.new(|cx| CodeBlock::new("bash", "bash", clean_cmd, cx)))
                    .clone();
                return div().w_full().child(cb).into_any_element();
            }
            div().text_color(s.ink).child(inp).into_any_element()
        });

        let output_el: Option<AnyElement> = raw_output.map(|out| {
            let trimmed = out.trim();
            if !failed
                && (trimmed.starts_with('{') || trimmed.starts_with('['))
                && let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed)
                && (val.is_object() || val.is_array())
            {
                let cache_k = format!("tool-out-json-{}-{}", key.0, key.1);
                let jv = self
                    .tool_json_viewers
                    .entry(cache_k)
                    .or_insert_with(|| {
                        cx.new(|cx| JsonViewer::new("输出", val, window, cx).with_max_height(200.))
                    })
                    .clone();
                return div().w_full().child(jv).into_any_element();
            }
            div()
                .text_color(if failed { s.pro.red_ink } else { s.muted })
                .child(clip_lines(&out, DETAIL_MAX_LINES))
                .into_any_element()
        });

        div()
            .id(SharedString::from(format!("tool-box-{}-{}", key.0, key.1)))
            .w_full()
            .overflow_x_scroll()
            .px(px(12.))
            .py(px(8.))
            .rounded(px(R_CTRL))
            .bg(s.soft)
            .flex()
            .flex_col()
            .gap(px(8.))
            .font_family(mono)
            .text_size(px(FS_META))
            .line_height(px(18.))
            .when_some(input_el, |d, el| d.child(el))
            .when_some(output_el, |d, el| d.child(el))
            .into_any_element()
    }

    fn render_diff(
        &mut self,
        key: (Uuid, usize),
        diff: &str,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Div {
        let view = parse_diff(diff);
        let open = self.part_is_open(key, key.1 >= 999_000);
        let file = if view.file.is_empty() {
            "工作区文件变更".to_owned()
        } else {
            view.file.clone()
        };
        let label = format!("修改了 {file}　+{} −{}", view.added, view.removed);

        let parsed_hunks = if open {
            parse_unified_diff_hunks(diff)
        } else {
            Vec::new()
        };
        let shown = view.lines.len().min(DETAIL_MAX_LINES * 2);

        let body_el: AnyElement = if open && !parsed_hunks.is_empty() {
            let cache_k = format!("turn-patch-{}-{}", key.0, key.1);
            let file_for_pr = file.clone();
            let pr = self
                .patch_review_cache
                .entry(cache_k)
                .or_insert_with(|| {
                    let hunks = parsed_hunks
                        .iter()
                        .map(|h| PatchHunk {
                            line: h.start_line,
                            before: h.before.clone(),
                            after: h.after.clone(),
                            decision: PatchDecision::Pending,
                        })
                        .collect();
                    cx.new(|cx| {
                        PatchReview::new(
                            format!("turn-pr-{}-{}", key.0, key.1),
                            file_for_pr,
                            hunks,
                            cx,
                        ).read_only(mono_font(cx))
                    })
                })
                .clone();
            div().w_full().py(px(2.)).child(pr).into_any_element()
        } else {
            let mono = mono_font(cx);
            let mut body = div()
                .id(SharedString::from(format!("diff-scroll-{}-{}", key.0, key.1)))
                .w_full()
                .py(px(4.))
                .rounded(px(R_CTRL))
                .bg(s.soft)
                .overflow_x_scroll()
                .flex()
                .flex_col()
                .font_family(mono)
                .text_size(px(FS_META))
                .line_height(px(18.));
            for line in view.lines.iter().take(shown) {
                let (sign, text, ink, wash) = match line {
                    DiffLine::Added(text) => ("+", text, s.pro.green_ink, s.pro.green_soft),
                    DiffLine::Removed(text) => ("−", text, s.pro.red_ink, s.pro.red_soft),
                    DiffLine::Context(text) => (" ", text, s.muted, transparent_black()),
                };
                body = body.child(
                    div()
                        .w_full()
                        .px(px(12.))
                        .bg(wash)
                        .flex()
                        .gap(px(8.))
                        .text_color(ink)
                        .child(div().w(px(8.)).flex_none().child(sign))
                        .child(div().flex_1().min_w_0().whitespace_nowrap().child(text.clone())),
                );
            }
            if view.lines.len() > shown {
                body = body.child(
                    div()
                        .px(px(12.))
                        .pt(px(4.))
                        .text_color(s.faint)
                        .child(format!("还有 {} 行", view.lines.len() - shown)),
                );
            }
            body.into_any_element()
        };

        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .items_start()
            .child(
                disclosure_line(
                    SharedString::from(format!("diff-{}-{}", key.0, key.1)),
                    pro_icon(ProIcon::Code, ICON_SM, s.muted),
                    label,
                    open,
                    s,
                )
                .on_press(cx, move |this, _, cx| {
                    this.part_open.insert(key, !open);
                    cx.notify();
                }),
            )
            .child(disclosure_body(
                SharedString::from(format!("diff-body-{}-{}", key.0, key.1)),
                open,
                (shown as f32 * 18. + 36.).max(120.),
                body_el,
                s,
            ))
    }

    fn render_changed_files(
        &self,
        turn: Uuid,
        index: usize,
        files: Vec<(PathBuf, &'static str, bool)>,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let workspace = self.effective_workspace_dir();
        let count = files.len();
        let default_open = !self.bottom_terminal_open
            && (matches!(self.access_mode, ChatAccessMode::DemoShowcase)
                || (count <= 4 && index + 1 == self.projected.turns.len()));
        let open = self
            .expanded_turn_changes
            .get(&turn)
            .copied()
            .unwrap_or(default_open);

        let count_label = match loc {
            crate::ui_primitives::UiLocale::En => format!(
                "{count} {}",
                if count == 1 {
                    "changed file"
                } else {
                    "changed files"
                }
            ),
            crate::ui_primitives::UiLocale::Zh => format!("{count} 个文件变更"),
        };

        let mut rows_col = div()
            .w_full()
            .px(px(10.))
            .pb(px(8.))
            .pt(px(2.))
            .border_t_1()
            .border_color(s.line_soft)
            .flex()
            .flex_col()
            .gap(px(2.));

        for (ix, (path, verb, reviewable)) in files.into_iter().enumerate() {
            let name = path.display().to_string();
            let office = OfficeKind::for_path(&path);
            let full = workspace.join(&path);
            let verb_localized = match verb {
                "新增" => loc.pick("新增", "Added"),
                "删除" => loc.pick("删除", "Deleted"),
                _ => loc.pick("修改", "Modified"),
            };

            let row_btn = Tactile::ghost(SharedString::from(format!("file-{turn}-{ix}")), s)
                .w_full()
                .h(px(36.))
                .px(px(8.))
                .gap(px(10.))
                .child(render_file_diff_icon(16., s.muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(FS_UI))
                        .text_color(s.ink)
                        .child(name.clone()),
                )
                .child(
                    div()
                        .text_size(px(FS_META))
                        .text_color(s.muted)
                        .child(verb_localized),
                );

            let row_btn = if reviewable && let Some(chat) = self.current_chat_id() {
                let key = crate::turn_review::ReviewKey {
                    chat,
                    turn,
                    index,
                    path,
                };
                let active = self.turn_review.as_ref().is_some_and(|r| r.key == key);
                row_btn
                    .lit(active)
                    .aria(format!("查看本轮差异 {name}"))
                    .on_press(cx, move |this, win, cx| {
                        if this.turn_review.as_ref().is_some_and(|r| r.key == key) {
                            this.close_file_review(win, cx);
                        } else {
                            this.turn_review = Some(crate::turn_review::FileReview {
                                key: key.clone(),
                                patch: None,
                            });
                            this.bridge.send(KarCommand::LoadTurnDiff(key.clone()));
                        }
                        cx.notify();
                    })
            } else if !matches!(
                self.access_mode,
                ChatAccessMode::HttpReadOnly { .. } | ChatAccessMode::RemoteController { .. }
            ) && full.is_file()
            {
                row_btn.on_press(cx, move |this, window, cx| {
                    if office.is_some() {
                        this.open_office_file_tab(full.clone(), false, None, window, cx);
                    } else {
                        this.open_workspace_code_tab(path.clone(), cx);
                    }
                })
            } else {
                row_btn.inert().tip(loc.pick(
                    "此文件没有可读取的本地差异",
                    "No local diff is available for this file",
                ))
            };
            rows_col = rows_col.child(row_btn);
        }

        div()
            .w_full()
            .rounded(px(R_CARD))
            .bg(s.card.opacity(if s.dark { 0.72 } else { 1.0 }))
            .border_1()
            .border_color(s.line)
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                Tactile::ghost(
                    SharedString::from(format!("turn-files-fold-{turn}-button")),
                    s,
                )
                .h(px(40.))
                .w_full()
                .px(px(14.))
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(FS_UI))
                        .font_weight(FontWeight::MEDIUM)
                        .child(count_label),
                )
                .child(pro_icon(
                    if open {
                        ProIcon::ChevronUp
                    } else {
                        ProIcon::ChevronDown
                    },
                    14.,
                    s.muted,
                ))
                .on_press(cx, move |this, _, cx| {
                    this.expanded_turn_changes.insert(turn, !open);
                    cx.notify();
                }),
            )
            .child(pro_fold(
                SharedString::from(format!("turn-files-fold-{turn}")),
                open,
                count as f32 * 38. + 12.,
                0.,
                move || rows_col.into_any_element(),
            ))
    }

    pub(crate) fn close_file_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.turn_review = None;
        if self.access_mode.is_writable() {
            self.composer_editor
                .update(cx, |ed, cx| ed.focus(window, cx));
        }
        cx.notify();
    }

    fn render_file_review(
        &self,
        review: &crate::turn_review::FileReview,
        in_rail: bool,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = &review.key;
        let loc = self.ui_locale;
        let name = key.path.display().to_string();
        let label = match loc {
            crate::ui_primitives::UiLocale::Zh => format!("第 {} 轮差异", key.index + 1),
            crate::ui_primitives::UiLocale::En => format!("Turn {} diff", key.index + 1),
        };
        let mut panel = div()
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .when(in_rail, |d| d.h_full())
            .when(!in_rail, |d| {
                d.rounded(px(12.)).border_1().border_color(s.line)
            })
            .bg(s.surface)
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .min_h(px(if in_rail { 44. } else { 32. }))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(s.line_soft)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(render_file_diff_icon(15., s.muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child(name.clone()),
                    )
                    .child(div().text_size(px(12.)).text_color(s.muted).child(label))
                    .child(
                        icon_button(
                            "close-turn-file-diff",
                            ProIcon::Close,
                            loc.pick("收起差异", "Close diff"),
                            s,
                        )
                        .on_press(cx, |this, win, cx| this.close_file_review(win, cx)),
                    ),
            );
        match &review.patch {
            Some(Ok(lines)) if !lines.is_empty() => {
                let lines = lines.clone();
                let count = lines.len();
                let palette = *s;
                let mono = mono_font(cx);
                panel = panel.child(
                    uniform_list(
                        SharedString::from(format!("review-lines-{}-{}", key.turn, name)),
                        count,
                        move |range, _, _| {
                            range
                                .map(|ix| {
                                    let line = &lines[ix];
                                    let (ink, bg) = match line.as_bytes().first() {
                                        Some(b'+') => {
                                            (palette.pro.green_ink, palette.pro.green_soft)
                                        }
                                        Some(b'-') => (palette.pro.red_ink, palette.pro.red_soft),
                                        Some(b'@') => (palette.muted, palette.soft),
                                        _ => (palette.text, palette.surface),
                                    };
                                    div()
                                        .px(px(9.))
                                        .h(px(21.))
                                        .line_height(px(21.))
                                        .text_size(px(FS_META))
                                        .font_family(mono.clone())
                                        .whitespace_nowrap()
                                        .bg(bg)
                                        .text_color(ink)
                                        .child(line.clone())
                                })
                                .collect()
                        },
                    )
                    .when(in_rail, |d| d.flex_1().min_h_0())
                    .when(!in_rail, |d| d.h(px(21. * count.min(10) as f32)))
                    .w_full()
                    .with_horizontal_sizing_behavior(
                        gpui::ListHorizontalSizingBehavior::Unconstrained,
                    ),
                );
            }
            result => {
                let detail = match result {
                    None => "正在读取本轮检查点…",
                    Some(Err(err)) => err.as_str(),
                    _ => "此文件没有可显示的文本行。",
                };
                panel = panel.child(
                    div()
                        .px(px(9.))
                        .py(px(6.))
                        .text_size(px(FS_META))
                        .text_color(s.muted)
                        .child(detail.to_owned()),
                );
            }
        }
        let path = key.path.clone();
        let full = self.effective_workspace_dir().join(&path);
        let intro = match loc {
            crate::ui_primitives::UiLocale::Zh => {
                format!("关于第 {} 轮对 {} 的改动，", key.index + 1, name)
            }
            crate::ui_primitives::UiLocale::En => {
                format!("About the change to {} in turn {}, ", name, key.index + 1)
            }
        };
        panel.child(
            div()
                .px(px(6.))
                .py(px(6.))
                .flex_none()
                .border_t_1()
                .border_color(s.line_soft)
                .flex()
                .items_center()
                .justify_between()
                .child(div().when(full.is_file(), |d| {
                    d.child(
                        text_button(
                            "turn-review-source",
                            loc.pick("打开当前文件", "Open current file"),
                            s,
                        )
                        .on_press(cx, move |this, win, cx| {
                            if OfficeKind::for_path(&full).is_some() {
                                this.open_office_file_tab(full.clone(), false, None, win, cx);
                            } else {
                                this.open_workspace_code_tab(path.clone(), cx);
                            }
                        }),
                    )
                }))
                .when(self.access_mode.is_writable(), |d| {
                    d.child(
                        soft_button(
                            "follow-up-turn-file",
                            loc.pick("追问此改动", "Ask about this change"),
                            s,
                        )
                        .aria(loc.pick("追问此改动", "Ask about this change"))
                        .on_press(cx, move |this, win, cx| {
                            this.turn_review = None;
                            this.composer_editor.update(cx, |ed, cx| {
                                let draft = ed.value();
                                if !draft.starts_with(&intro) {
                                    ed.set_value(format!("{intro}{draft}"), win, cx);
                                }
                                ed.focus(win, cx);
                            });
                            cx.notify();
                        }),
                    )
                }),
        )
    }

    // ── Decisions (Permissions + Full Structured QuestionForm) ──────────────

    fn render_decision_dock(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let waiting: Vec<PendingRequest> = if self.access_mode.is_writable() {
            self.projected
                .pending_requests
                .iter()
                .filter(|r| matches!(r.kind, RequestKind::Permission | RequestKind::Question))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let total = waiting.len();
        let card = waiting
            .into_iter()
            .next()
            .map(|request| self.render_decision_card(request, total, s, cx));
        let open = card.is_some();
        pro_fold("kar-decision-dock", open, 184., 0., move || {
            div()
                .w_full()
                .max_w(px(THREAD_W))
                .mx_auto()
                .px(px(THREAD_PAD))
                .pb(px(8.))
                .when_some(card, |d, card| d.child(card))
                .into_any_element()
        })
    }

    fn render_decision_card(
        &self,
        request: PendingRequest,
        total: usize,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let agent = runtime_display_name(&self.effective_runtime());
        let mono = mono_font(cx);

        let head = div()
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(10.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(s.muted)
            .child(match request.kind {
                RequestKind::Question => {
                    format!("{} · {agent}", loc.pick("需要你的答复", "INPUT NEEDED"))
                }
                _ => loc.pick("需要执行授权", "APPROVAL NEEDED").to_owned(),
            })
            .when(total > 1, |d| {
                d.child(format!(
                    "+{} {}",
                    total - 1,
                    loc.pick("项待处理", "pending")
                ))
            });

        let mut card = div()
            .w_full()
            .p(px(16.))
            .rounded(px(R_HERO))
            .bg(s.card)
            .border_1()
            .border_color(s.line_strong)
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(head);

        if request.kind == RequestKind::Question {
            return self.render_question_decision_body(card, request, s, cx);
        }

        let controls = request.controls();
        let reason = request.body["reason"]
            .as_str()
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(str::to_owned);
        card = card.child(decision_question(
            reason.unwrap_or_else(|| loc.pick("command", "command").to_owned()),
            s,
        ));

        let mut facts = request_facts(&request);
        for res in &controls.resources {
            if let Some(txt) = res.value.as_str().filter(|v| !v.is_empty()) {
                facts.push(("资源", txt.to_owned()));
            }
        }
        if !facts.is_empty() {
            card = card.child(
                div()
                    .w_full()
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(R_CTRL))
                    .bg(s.soft)
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .children(facts.into_iter().map(|(_, value)| {
                        div()
                            .w_full()
                            .font_family(mono.clone())
                            .text_size(px(12.5))
                            .text_color(s.muted)
                            .child(value)
                    })),
            );
        }

        let accepted = &controls.decisions;
        let first_of = |wanted: &[PermissionDecision]| {
            wanted.iter().copied().find(|d| accepted.contains(d))
        };
        let refuse = first_of(&[PermissionDecision::Deny, PermissionDecision::Cancel]);
        let allow = first_of(&[
            PermissionDecision::AllowOnce,
            PermissionDecision::AllowForTurn,
        ]);
        let allow_more = first_of(&[
            PermissionDecision::AllowForSession,
            PermissionDecision::AllowAlways,
        ]);
        let decide = |decision: PermissionDecision| {
            let req = request.clone();
            move |this: &mut Self, _: &mut Window, _: &mut Context<Self>| {
                this.bridge.send(KarCommand::DecidePermission {
                    request: req.clone(),
                    decision,
                });
            }
        };

        // KUI `AgentThreadScreen` crisp `30px` horizontal approval bar
        let mut actions = div()
            .pt(px(2.))
            .w_full()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_end()
            .gap(px(6.));
        if let Some(decision) = refuse {
            actions = actions.child(
                Tactile::soft("decision-refuse", s)
                    .fills(s.pro.red_soft, s.pro.red.opacity(0.22))
                    .h(px(30.))
                    .px(px(12.))
                    .radius(R_CTRL)
                    .justify_center()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.pro.red_ink)
                    .child(loc.pick("拒绝", "Decline"))
                    .on_press(cx, decide(decision)),
            );
        }
        for (pix, pol) in controls.policies.iter().take(3).enumerate() {
            let label = policy_option_label(pol, pix);
            let choice = pol.choice;
            let req = request.clone();
            actions = actions.child(
                Tactile::soft(("decision-policy", pix), s)
                    .h(px(30.))
                    .px(px(12.))
                    .radius(R_CTRL)
                    .justify_center()
                    .child(label)
                    .on_press(cx, move |this, _, _| {
                        this.bridge.send(KarCommand::SelectPolicy {
                            request: req.clone(),
                            choice,
                        });
                    }),
            );
        }
        if let Some(decision) = allow_more {
            actions = actions.child(
                Tactile::soft("decision-allow-more", s)
                    .h(px(30.))
                    .px(px(12.))
                    .radius(R_CTRL)
                    .justify_center()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.ink)
                    .child(match decision {
                        PermissionDecision::AllowAlways => {
                            loc.pick("总是允许", "Allow always")
                        }
                        _ => loc.pick("本次会话允许", "Allow session"),
                    })
                    .on_press(cx, decide(decision)),
            );
        }
        if let Some(decision) = allow {
            actions = actions.child(
                Tactile::primary("decision-allow", s)
                    .fills(s.primary, s.primary.opacity(0.9))
                    .h(px(30.))
                    .px(px(14.))
                    .radius(R_CTRL)
                    .justify_center()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(s.on_primary)
                    .child(loc.pick("允许一次", "Allow once"))
                    .on_press(cx, decide(decision)),
            );
        }

        card.child(actions)
    }

    fn render_question_decision_body(
        &self,
        mut card: Div,
        request: PendingRequest,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Div {
        let form = request.question_form().unwrap_or_default();
        let heading = form
            .message
            .clone()
            .or_else(|| form.fields.first().map(|f| f.label.clone()))
            .unwrap_or_else(|| "需要你做出决定".to_owned());
        card = card.child(decision_question(heading, s));

        if let Some(url) = form.url.clone().filter(|u| !u.trim().is_empty()) {
            let to_copy = url.clone();
            card = card.child(
                div()
                    .w_full()
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(R_CTRL))
                    .bg(s.soft)
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(FS_META))
                            .text_color(s.pro.accent_info)
                            .child(url),
                    )
                    .child(text_button("q-copy-url", "复制链接", s).on_press(
                        cx,
                        move |this, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(to_copy.clone()));
                            this.notify_toast("已复制认证或外部链接", cx);
                        },
                    )),
            );
        }

        // Multi-field QuestionForm support (`form.fields.len() > 1`)
        if form.fields.len() > 1 {
            let mut multi_box = div().w_full().flex().flex_col().gap(px(8.));
            for (f_ix, field) in form.fields.iter().enumerate() {
                let fid = field.id.clone();
                let selected_val = self.question_field_values.get(&fid).cloned();
                let mut row = div().w_full().flex().flex_col().gap(px(4.)).child(
                    div()
                        .text_size(px(FS_UI))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.ink)
                        .child(field.label.clone()),
                );
                if !field.options.is_empty() {
                    let mut opts = div().flex().flex_wrap().gap(px(6.));
                    for (o_ix, opt) in field.options.iter().enumerate() {
                        let fid_c = fid.clone();
                        let val_c = opt.value.clone();
                        let is_sel = selected_val.as_deref() == Some(val_c.as_str());
                        opts = opts.child(
                            soft_button(("q-mf-opt", f_ix * 32 + o_ix), opt.label.clone(), s)
                                .lit(is_sel)
                                .on_press(cx, move |this, _, cx| {
                                    this.question_field_values
                                        .insert(fid_c.clone(), val_c.clone());
                                    cx.notify();
                                }),
                        );
                    }
                    row = row.child(opts);
                }
                multi_box = multi_box.child(row);
            }
            let req = request.clone();
            let fields_clone = form.fields.clone();
            return card
                .child(multi_box)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(8.))
                        .child(self.render_skip_question_btn(request, s, cx))
                        .child(
                            primary_button("decision-mf-submit", "提交全部答复", s).on_press(
                                cx,
                                move |this, _, cx| {
                                    let mut values = serde_json::Map::new();
                                    for f in &fields_clone {
                                        if let Some(v) = this.question_field_values.get(&f.id) {
                                            values.insert(f.id.clone(), v.clone().into());
                                        } else if let Some(first) = f.options.first() {
                                            values.insert(f.id.clone(), first.value.clone().into());
                                        }
                                    }
                                    this.question_field_values.clear();
                                    this.bridge.send(KarCommand::AnswerQuestion {
                                        request: req.clone(),
                                        reply: QuestionReply::Submit { values },
                                    });
                                    cx.notify();
                                },
                            ),
                        ),
                );
        }

        if let Some(field) = form.fields.first().cloned() {
            if let Some(desc) = field
                .description
                .clone()
                .filter(|_| form.message.is_some() && !field.label.is_empty())
            {
                card = card.child(
                    div()
                        .text_size(px(FS_META))
                        .line_height(px(LH_META))
                        .text_color(s.muted)
                        .child(desc),
                );
            }

            match field.kind {
                FieldKind::Select if !field.options.is_empty() => {
                    let mut options = div().mx(px(-8.)).flex().flex_col().gap(px(2.));
                    for (ix, option) in field.options.iter().enumerate() {
                        let req = request.clone();
                        let fid = field.id.clone();
                        let val = option.value.clone();
                        options = options.child(
                            Tactile::ghost(("decision-option", ix), s)
                                .w_full()
                                .min_h(px(36.))
                                .px(px(8.))
                                .py(px(6.))
                                .gap(px(10.))
                                .child(
                                    div()
                                        .size(px(20.))
                                        .flex_none()
                                        .rounded(px(6.))
                                        .bg(s.soft)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(FS_META))
                                        .text_color(s.muted)
                                        .child((ix + 1).to_string()),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(px(FS_UI))
                                                .line_height(px(LH_UI))
                                                .text_color(s.ink)
                                                .child(option.label.clone()),
                                        )
                                        .when_some(option.description.clone(), |d, desc| {
                                            d.child(
                                                div()
                                                    .text_size(px(FS_META))
                                                    .line_height(px(LH_META))
                                                    .text_color(s.muted)
                                                    .child(desc),
                                            )
                                        }),
                                )
                                .on_press(cx, move |this, window, cx| {
                                    let mut values = serde_json::Map::new();
                                    values.insert(fid.clone(), val.clone().into());
                                    this.question_multi_selected.clear();
                                    this.question_answer_editor
                                        .update(cx, |ed, cx| ed.set_value("", window, cx));
                                    this.bridge.send(KarCommand::AnswerQuestion {
                                        request: req.clone(),
                                        reply: QuestionReply::Submit { values },
                                    });
                                }),
                        );
                    }
                    card = card.child(options);
                    if field.allow_other {
                        card = card.child(self.render_question_freeform_row(
                            request.clone(),
                            field.id.clone(),
                            field.kind,
                            s,
                            cx,
                        ));
                    }
                }
                FieldKind::MultiSelect if !field.options.is_empty() => {
                    let mut options = div().mx(px(-8.)).flex().flex_col().gap(px(2.));
                    for (ix, option) in field.options.iter().enumerate() {
                        let val = option.value.clone();
                        let checked = self.question_multi_selected.contains(&val);
                        options = options.child(
                            Tactile::ghost(("decision-multi", ix), s)
                                .lit(checked)
                                .w_full()
                                .min_h(px(34.))
                                .px(px(8.))
                                .py(px(5.))
                                .gap(px(10.))
                                .child(
                                    div()
                                        .size(px(18.))
                                        .flex_none()
                                        .rounded(px(5.))
                                        .bg(if checked { s.primary } else { s.soft })
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when(checked, |b| {
                                            b.child(pro_icon(ProIcon::Check, ICON_XS, s.on_primary))
                                        }),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(FS_UI))
                                        .text_color(s.ink)
                                        .child(option.label.clone()),
                                )
                                .on_press(cx, move |this, _, cx| {
                                    if !this.question_multi_selected.remove(&val) {
                                        this.question_multi_selected.insert(val.clone());
                                    }
                                    cx.notify();
                                }),
                        );
                    }
                    let req = request.clone();
                    let fid = field.id.clone();
                    card = card.child(options).child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .child(self.render_skip_question_btn(request, s, cx))
                            .child(primary_button("decision-multi-submit", "提交所选", s).on_press(
                                cx,
                                move |this, _, cx| {
                                    let arr: Vec<serde_json::Value> = this
                                        .question_multi_selected
                                        .iter()
                                        .cloned()
                                        .map(serde_json::Value::from)
                                        .collect();
                                    let mut values = serde_json::Map::new();
                                    values.insert(fid.clone(), serde_json::Value::Array(arr));
                                    this.question_multi_selected.clear();
                                    this.bridge.send(KarCommand::AnswerQuestion {
                                        request: req.clone(),
                                        reply: QuestionReply::Submit { values },
                                    });
                                    cx.notify();
                                },
                            )),
                    );
                    return card;
                }
                FieldKind::Boolean => {
                    let req_no = request.clone();
                    let req_yes = request.clone();
                    let fid_no = field.id.clone();
                    let fid_yes = field.id.clone();
                    return card.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .child(self.render_skip_question_btn(request, s, cx))
                            .child(soft_button("decision-bool-no", "否", s).on_press(
                                cx,
                                move |this, _, _| {
                                    let mut values = serde_json::Map::new();
                                    values.insert(fid_no.clone(), false.into());
                                    this.bridge.send(KarCommand::AnswerQuestion {
                                        request: req_no.clone(),
                                        reply: QuestionReply::Submit { values },
                                    });
                                },
                            ))
                            .child(primary_button("decision-bool-yes", "是", s).on_press(
                                cx,
                                move |this, _, _| {
                                    let mut values = serde_json::Map::new();
                                    values.insert(fid_yes.clone(), true.into());
                                    this.bridge.send(KarCommand::AnswerQuestion {
                                        request: req_yes.clone(),
                                        reply: QuestionReply::Submit { values },
                                    });
                                },
                            )),
                    );
                }
                _ => {
                    card = card.child(self.render_question_freeform_row(
                        request.clone(),
                        field.id.clone(),
                        field.kind,
                        s,
                        cx,
                    ));
                }
            }
        }

        card.child(
            div()
                .flex()
                .justify_end()
                .child(self.render_skip_question_btn(request, s, cx)),
        )
    }

    fn render_question_freeform_row(
        &self,
        request: PendingRequest,
        field_id: String,
        kind: FieldKind,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(32.))
                    .px(px(10.))
                    .rounded(px(R_CTRL))
                    .bg(s.soft)
                    .flex()
                    .items_center()
                    .child(self.question_answer_editor.clone()),
            )
            .child(primary_button("decision-text-submit", "提交", s).on_press(
                cx,
                move |this, window, cx| {
                    let raw = this.question_answer_editor.read(cx).value().trim().to_owned();
                    let json_val = match kind {
                        FieldKind::Integer => raw
                            .parse::<i64>()
                            .map(serde_json::Value::from)
                            .unwrap_or_else(|_| raw.clone().into()),
                        FieldKind::Number => raw
                            .parse::<f64>()
                            .map(serde_json::Value::from)
                            .unwrap_or_else(|_| raw.clone().into()),
                        _ => serde_json::Value::from(raw),
                    };
                    let mut values = serde_json::Map::new();
                    values.insert(field_id.clone(), json_val);
                    this.question_answer_editor
                        .update(cx, |ed, cx| ed.set_value("", window, cx));
                    this.bridge.send(KarCommand::AnswerQuestion {
                        request: request.clone(),
                        reply: QuestionReply::Submit { values },
                    });
                },
            ))
    }

    fn render_skip_question_btn(
        &self,
        request: PendingRequest,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Tactile {
        text_button("decision-skip", "跳过", s).on_press(cx, move |this, _, _| {
            this.bridge.send(KarCommand::AnswerQuestion {
                request: request.clone(),
                reply: QuestionReply::Decline,
            });
        })
    }

    pub(crate) fn handle_dropped_paths(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for path in paths {
            if OfficeKind::for_path(path).is_some() {
                self.open_office_file_tab(path.clone(), false, None, window, cx);
            } else if path.is_file() && !self.pending_attachments.contains(path) {
                self.pending_attachments.push(path.clone());
            }
        }
        cx.notify();
    }
}

fn policy_option_label(pol: &PolicyOption, idx: usize) -> String {
    pol.details["name"]
        .as_str()
        .or_else(|| pol.details["label"].as_str())
        .or_else(|| pol.details["title"].as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("允许策略 {}", idx + 1))
}

// The reference caps the content itself at 46rem, with the viewport gutters outside it.
fn chat_lane_gutter(width: f32, window: &Window) -> f32 {
    if f32::from(window.viewport_size().width) >= 640. && width >= 300. { 48. } else { 12. }
}

fn chat_content_lane(width: f32, window: &Window) -> Div {
    let gutter = chat_lane_gutter(width, window);
    div()
        .w_full()
        .mx_auto()
        .max_w(px(THREAD_W + 2. * gutter))
        .px(px(gutter))
}

fn user_bubble(text: String, s: &StudioPalette) -> Div {
    div()
        .max_w(relative(0.80))
        .min_w_0()
        .overflow_hidden()
        .px(px(11.))
        .pt(px(13.))
        .pb(px(11.))
        .rounded(px(16.))
        .bg(s.message_surface)
        .text_size(px(14.))
        .line_height(px(23.))
        .text_color(s.ink)
        .child(text)
}

fn format_worked_header(
    duration_ms: Option<u64>,
    step_count: usize,
    loc: crate::ui_primitives::UiLocale,
) -> String {
    if let Some(ms) = duration_ms.filter(|&m| m > 0) {
        let secs = ms / 1000;
        let human = if secs >= 60 {
            format!("{}m {}s", secs / 60, secs % 60)
        } else if ms >= 1000 {
            format!("{:.1}s", ms as f64 / 1000.0)
        } else {
            format!("{ms}ms")
        };
        match loc {
            crate::ui_primitives::UiLocale::En => format!("Worked for {human}"),
            crate::ui_primitives::UiLocale::Zh => format!("已执行 {human}"),
        }
    } else {
        match loc {
            crate::ui_primitives::UiLocale::En => format!("Worked ({step_count} steps)"),
            crate::ui_primitives::UiLocale::Zh => format!("已执行 {step_count} 步操作"),
        }
    }
}

enum MdSegment {
    Prose(String),
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

fn is_table_separator_line(line: &str) -> bool {
    let t = line.trim();
    if !t.contains('|') {
        return false;
    }
    let cells: Vec<&str> = t
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect();
    !cells.is_empty()
        && cells.iter().all(|c| {
            let core = c.trim_matches(':');
            core.len() >= 3 && core.chars().all(|ch| ch == '-')
        })
}

fn split_pipe_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().replace("**", "").replace('`', ""))
        .collect()
}

fn parse_markdown_segments(text: &str) -> Vec<MdSegment> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut prose_buf = Vec::new();
    let mut in_fence = false;
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            prose_buf.push(lines[i]);
            i += 1;
            continue;
        }
        if !in_fence
            && i + 1 < lines.len()
            && trimmed.contains('|')
            && is_table_separator_line(lines[i + 1])
        {
            if !prose_buf.is_empty() {
                out.push(MdSegment::Prose(prose_buf.join("\n")));
                prose_buf.clear();
            }
            let headers = split_pipe_row(lines[i]);
            i += 2;
            let mut rows = Vec::new();
            while i < lines.len() && lines[i].trim().contains('|') && !lines[i].trim().is_empty() {
                rows.push(split_pipe_row(lines[i]));
                i += 1;
            }
            out.push(MdSegment::Table { headers, rows });
            continue;
        }
        prose_buf.push(lines[i]);
        i += 1;
    }
    if !prose_buf.is_empty() {
        out.push(MdSegment::Prose(prose_buf.join("\n")));
    }
    out
}

fn working_line(
    lead: AnyElement,
    status: String,
    key: impl Into<ElementId>,
    s: &StudioPalette,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    div()
        .h(px(24.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(FS_UI))
        .text_color(s.muted)
        .child(lead)
        .child(swap_label(key, status, window, cx))
}

fn decision_question(text: String, s: &StudioPalette) -> Div {
    div()
        .text_size(px(FS_READ))
        .line_height(px(LH_READ))
        .font_weight(FontWeight::MEDIUM)
        .text_color(s.ink)
        .child(text)
}

fn clip_lines(text: &str, max: usize) -> String {
    let total = text.lines().count();
    if total <= max {
        return text.to_owned();
    }
    let mut clipped: String = text.lines().take(max).collect::<Vec<_>>().join("\n");
    clipped.push_str(&format!("\n… 还有 {} 行", total - max));
    clipped
}

fn request_facts(request: &PendingRequest) -> Vec<(&'static str, String)> {
    let body = &request.body;
    let text_at = |paths: &[&[&str]]| -> Option<String> {
        paths.iter().find_map(|path| {
            let mut node = body;
            for key in *path {
                node = &node[*key];
            }
            match node {
                serde_json::Value::String(v) if !v.trim().is_empty() => Some(v.trim().to_owned()),
                serde_json::Value::Array(parts) if !parts.is_empty() => Some(
                    parts
                        .iter()
                        .filter_map(|p| p.as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                _ => None,
            }
        })
    };
    let mut facts = Vec::new();
    if let Some(command) = text_at(&[
        &["command"],
        &["input", "command"],
        &["toolCall", "rawInput", "command"],
    ]) {
        facts.push(("命令", unwrap_shell_command(&command)));
    }
    if let Some(file) = text_at(&[
        &["input", "file_path"],
        &["input", "path"],
        &["path"],
        &["toolCall", "rawInput", "path"],
    ]) {
        facts.push(("文件", file));
    }
    if let Some(dir) = text_at(&[&["cwd"], &["input", "cwd"]]) {
        facts.push(("位置", dir));
    }
    if facts.is_empty()
        && let Some(what) = request.display_text()
    {
        facts.push(("操作", what));
    }
    facts
}

/// Search state outlives repainting so keyboard focus and IME composition are retained.
struct ModelPickerSearch {
    editor: Entity<Editor>,
    provider: String,
    model: Option<String>,
    query: String,
    cursor: usize,
    open: bool,
    focus_on_mount: bool,
    scroll: ScrollHandle,
    _events: Subscription,
}

impl ModelPickerSearch {
    fn new(bench: WeakEntity<KarWorkbench>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| {
            let mut editor = Editor::new("Search models", InputMode::Combobox, window, cx);
            editor.font_size = 13.;
            editor
        });
        let events = cx.subscribe_in(
            &editor,
            window,
            move |view: &mut Self, editor, event, win, cx| {
                if !view.open {
                    return;
                }
                let Some(bench) = bench.upgrade() else {
                    return;
                };
                match event {
                    EditEvent::Changed => {
                        view.query = editor.read(cx).value().trim().to_lowercase();
                        view.cursor = 0;
                        view.scroll.scroll_to_item(0);
                    }
                    EditEvent::Navigate(_) | EditEvent::Submitted => {
                        let host = bench.read(cx);
                        let provider = host.providers.iter().find(|p| p.id == view.provider);
                        let options = picker_models(provider, &view.query, host.ui_locale);
                        if options.is_empty() {
                            return;
                        }
                        if let EditEvent::Navigate(down) = event {
                            let last = options.len() - 1;
                            view.cursor = if *down {
                                (view.cursor + 1).min(last)
                            } else {
                                view.cursor.saturating_sub(1).min(last)
                            };
                            view.scroll.scroll_to_item(view.cursor);
                        } else if let Some((model, _)) = options.get(view.cursor) {
                            let effort = (view.provider == host.selected_runtime)
                                .then(|| host.reasoning_effort.clone())
                                .flatten();
                            bench.update(cx, |host, cx| {
                                host.apply_model_picker(
                                    &view.provider,
                                    model.clone(),
                                    effort,
                                    win,
                                    cx,
                                )
                            });
                        }
                    }
                    EditEvent::Escape => {
                        bench.update(cx, |host, cx| host.close_model_picker(win, cx))
                    }
                    _ => return,
                }
                cx.notify();
            },
        );
        Self {
            editor,
            provider: String::new(),
            model: None,
            query: String::new(),
            cursor: 0,
            open: false,
            focus_on_mount: false,
            scroll: ScrollHandle::new(),
            _events: events,
        }
    }
}

fn picker_models(
    provider: Option<&ProviderStatusInfo>,
    query: &str,
    loc: UiLocale,
) -> Vec<(Option<String>, String)> {
    let Some(p) = provider.filter(|p| p.is_ready()) else {
        return Vec::new();
    };
    let default = p
        .models
        .iter()
        .find(|model| model.is_default)
        .map(|model| {
            format!(
                "{} · {}",
                loc.pick("智能体默认", "Agent default"),
                model.label
            )
        })
        .unwrap_or_else(|| loc.pick("智能体默认模型", "Agent default model").to_owned());
    // A model-name query should select that exact model, including the current catalogue default.
    std::iter::once((None, default))
        .filter(|_| {
            query.is_empty()
                || loc
                    .pick("智能体默认模型", "Agent default model")
                    .to_lowercase()
                    .contains(query)
        })
        .chain(
            p.models
                .iter()
                .map(|m| (Some(m.id.clone()), m.label.clone())),
        )
        .filter(|(id, label)| {
            label.to_lowercase().contains(query)
                || id
                    .as_ref()
                    .is_some_and(|id| id.to_lowercase().contains(query))
        })
        .collect()
}

fn effort_label(value: &str, loc: UiLocale) -> &str {
    match value {
        "minimal" => loc.pick("最低", "Minimal"),
        "low" => loc.pick("低", "Low"),
        "medium" => loc.pick("中", "Medium"),
        "high" => loc.pick("高", "High"),
        "xhigh" => loc.pick("超高", "Extra High"),
        "max" => loc.pick("最大", "Max"),
        "ultra" => loc.pick("极高", "Ultra"),
        value => value,
    }
}
