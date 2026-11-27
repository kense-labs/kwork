//! Native conversation timeline, file review, decisions and writing controls.

mod context_usage;

mod activity;
mod composer_activity;
mod composer_controls;
mod message_time;
mod reply_stream;
mod reading_anchor;
mod turn_outline;
mod thread_details;
mod turn_rail;
pub(crate) mod turn_navigation;
pub(crate) mod turn_steps;
pub(crate) mod questions;

use std::path::{Path, PathBuf};
use std::time::Duration;


use gpui::{prelude::*, *};
use gpui_kit::component::tooltip::Tooltip;
use kar::{
    ApprovalMode, ChangeKind as ToolChangeKind, PendingRequest, PermissionDecision, PolicyOption,
    RequestKind, TaskStatus,
    chat::{
        AttachmentKind, ChangeKind as CheckpointChangeKind, ChatTurn, Item, Role as KarRole,
        TurnStatus,
    },
    observe::Tool,
};
use kui::{
    components::{
        ai_mascot::{AiMascot, MascotStatus},
        code_suggestion::{Decision as PatchDecision, Hunk as PatchHunk, PatchReview},
        hold_to_confirm::{HoldEvent, HoldToConfirm},
        morph_loader::MorphLoader,
        pro_chrome::{
            pro_fold, pro_presence, pro_progress_ring, pro_spring_f32, pro_view_transition_fill,
        },
    },
    primitives::{
        editor::{EditEvent, Editor, InputMode},
        glyph::Glyph,
        ops_surface::{Fold, arrive},
        pro_icons::{ProIcon, pro_icon},
    },
};
use uuid::Uuid;

use crate::{
    kar_bridge::KarCommand,
    studio_hub::parse_unified_diff_hunks,
    types::{
        ChatAccessMode, DiffLine, FeedConnectionState, OfficeKind, ProviderReadiness, ProviderStatusInfo,
        SelectionTarget, describe_failure, format_tokens_compact,
        parse_diff, runtime_display_name, strip_selection_envelope,
        unwrap_shell_command,
    },
    ui_motion::{
        copy_text_button, copy_with_hint,
        MenuPlacement, Tactile, close_button, disclosure_body, disclosure_line, icon_button,
        icon_button_sm, menu_caption,
        menu_check, menu_divider, menu_hint, menu_host, menu_icon_item, menu_item,
        pulse_beacon, soft_button, soft_button_sm, spinner, text_button, text_button_sm,
    },
    ui_primitives::{
        CTRL_H, CTRL_SM_H, DotTone, FS_CONTROL, FS_META, FS_READ, FS_TAG, FS_UI, HEADER_H, ICON,
        ICON_SM, ICON_XS, LH_META, LH_READ, LH_UI, R_CARD, R_CTRL, R_HERO, R_PILL, R_SM, R_XS, ROW_H, StudioPalette, UiLocale,
        THREAD_PAD, TITLEBAR_BUTTON_GAP, TITLEBAR_EDGE_INSET, card_box, clean_display_doc_name, frosted_card, macos_traffic_pad, mono_font,
        render_bg_layers_icon,
        render_file_diff_icon, render_folder_icon, render_git_branch_icon,
        render_office_chip,
        render_runtime_mark, render_schedule_repeat_icon,
        render_skills_puzzle_icon, render_square_menu_icon, render_status,
        render_steer_merge_icon, search_input_box, tnum_features,
    },
    ui_settings::SettingsTab,
    workbench::{
        KarWorkbench, Menu, Notice, NoticeAction, NoticeTone, QueuedPrompt, WorkspaceLayoutMode,
    },
};

/// Most lines of a tool's output or a file change shown before it is cut short.
const DETAIL_MAX_LINES: usize = 24;

impl KarWorkbench {
    pub(crate) fn pending_decisions(&self) -> impl Iterator<Item = &PendingRequest> {
        self.projected.pending_requests.iter().filter(|request| {
            self.access_mode.is_writable()
                && matches!(request.kind, RequestKind::Permission | RequestKind::Question)
        })
    }

    pub(crate) fn render_chat_pane(
        &mut self,
        pane_w: f32,
        revealed_dock_width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, bool) {
        let s = StudioPalette::get(cx);

        let view_only = !self.access_mode.is_writable()
            && !matches!(self.access_mode, ChatAccessMode::NoneSelected);
        let mut decisions = self.pending_decisions();
        let decision = decisions.next().cloned();
        let pending_count = usize::from(decision.is_some()) + decisions.count();
        let question_cursor = decision.as_ref()
            .filter(|request| request.kind == RequestKind::Question)
            .map(|request| request.cursor.clone());
        let last_question = window.use_keyed_state("chat-presented-question", cx, |_, _| None);
        let reveal_question = last_question.update(cx, |previous, _| {
            let changed = *previous != question_cursor;
            *previous = question_cursor;
            changed && previous.is_some()
        });
        let activity = composer_activity::current(
            &self.access_mode,
            &self.projected,
            self.queued_prompt.is_some(),
            self.prompt_attempts.values(),
            &self.connection_state,
        );
        let on_start_screen = activity.is_none()
            && self.projected.turns.is_empty()
            && self.active_briefs.is_empty()
            && self.queued_prompt.is_none()
            && decision.is_none()
            && !view_only;
        let is_working = self.turn_in_progress();

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
        let center_w = pane_w - review_width;

        let thread_w = self.prefs.chat_width_mode.width_for_pane(center_w);
        let height_fits = window.use_keyed_state("thread-detail-height-fits", cx, |_, _| false);
        let margin = thread_details::margin_width(
            center_w, thread_w, f32::from(s.font_size(FS_UI)) / FS_UI, self.thread_details_margin_available,
        ).filter(|_| *height_fits.read(cx));
        self.set_thread_details_space(margin.is_some());
        let reveal = self.thread_details_reveal(window, cx);
        let header = self.render_chat_header(
            pane_w,
            revealed_dock_width,
            on_start_screen,
            &s,
            window,
            cx,
        );
        let matches = if self.chat_search_open {
            matching_turn_indices(&self.projected.turns, &self.chat_search_query.trim().to_lowercase())
        } else {
            Vec::new()
        };
        let active_match = (!matches.is_empty()).then(|| self.chat_search_match_ix % matches.len());
        let find_bar = self.render_chat_find_bar(thread_w, &matches, &s, window, cx);
        let share_bar = self.render_share_bar(thread_w, &s, cx);
        let goal_bar = self.render_goal_bar(thread_w, &s, cx);
        let notice = self.render_notice(thread_w, &s, cx);

        let active_match_turn_ix = active_match.map(|index| matches[index]);

        let stage = if on_start_screen {
            self.render_start_stage(center_w, &s, window, cx)
                .into_any_element()
        } else {
            let mut rows = Vec::new();
            if self.projected.turns.is_empty()
                && self.active_briefs.is_empty()
                && self.queued_prompt.is_none()
                && decision.is_none()
                && activity.is_none()
            {
                rows.push(self.render_waiting_for_content(&s));
            } else {
                for (turn_idx, turn) in self.projected.turns.iter().enumerate() {
                    let is_active_match = active_match_turn_ix == Some(turn_idx);
                    let turn_matched = matches.binary_search(&turn_idx).is_ok();
                    let rendered = self
                        .render_turn(turn_idx, turn, !show_review_rail, &s, window, cx)
                        .when(turn_matched, |d| {
                            d.relative()
                                .when(is_active_match, |d| {
                                    d.rounded(px(R_CARD)).bg(s.pro.accent_soft.opacity(0.32))
                                })
                                .child(div().absolute().left(px(-6.)).top_0().bottom_0().w(px(2.))
                                    .bg(if is_active_match { s.primary } else { s.primary.opacity(0.38) }))
                        });
                    rows.push(
                        arrive(
                            rendered,
                            ElementId::Name(format!("turn-in-{}", turn.id).into()),
                            0,
                            6.,
                            cx,
                        )
                        .into_any_element(),
                    );
                }
                if let Some(briefs_el) = self.render_brief_cards_for_chat(&s, window, cx) {
                    rows.push(briefs_el.into_any_element());
                }
                if let Some(queued) = self.queued_prompt.clone() {
                    rows.push(
                        arrive(self.render_queued_turn(&queued, &s, window, cx), "turn-in-queued", 0, 6., cx)
                            .into_any_element(),
                    );
                }
                if let Some(tasks_el) = self.render_active_tasks(&s, window, cx) {
                    rows.push(tasks_el.into_any_element());
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
                                .text_size(s.font_size(FS_TAG))
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
                    rows.push(strip.into_any_element());
                }
            }

            let scroll_remaining =
                f32::from(self.chat_scroll.max_offset().y) + f32::from(self.chat_scroll.offset().y);
            let show_jump_bottom = scroll_remaining > CTRL_H && !self.projected.turns.is_empty();

            let (question, approval) = if let Some(request) = decision {
                let permission = request.kind == RequestKind::Permission;
                let cursor = request.cursor.clone();
                let card = self.render_decision_card(request, pending_count, center_w < 460., &s, window, cx);
                let lane = chat_content_lane(center_w, thread_w, window);
                if permission {
                    // Keep the request reachable while the user reads earlier turns.
                    // Long command lines and policy lists scroll inside the approval area.
                    let panel = lane.id(SharedString::from(format!("approval-{}-{}", cursor.stream_id, cursor.after_seq)))
                        .role(Role::Region)
                        .aria_label(self.ui_locale.pick("待授权请求", "Pending approval"))
                        .flex()
                        .flex_col()
                        .min_h_0()
                        .max_h(relative(if center_w < 460. { 0.46 } else { 0.38 }))
                        .overflow_y_scroll()
                        .child(card);
                    (None, Some(panel))
                } else {
                    (Some(lane.flex_none().pb(px(8.)).child(card)), None)
                }
            } else {
                (None, None)
            };
            if reveal_question {
                self.chat_scroll.scroll_to_item(rows.len());
            }
            let activity_row = activity.map(|phase| {
                self.render_composer_activity(phase, &s, window, cx)
            });
            let foot = chat_content_lane(center_w, thread_w, window)
                .relative()
                .pt(px(8.))
                .pb(px(20.))
                .flex_none()
                .child(if view_only {
                    div()
                        .children(activity_row)
                        .when(self.recoverable_archived_plan(), |row| row.child(
                            Tactile::soft("resume-archived-proposal", &s)
                                .h(px(CTRL_H))
                                .px(px(12.))
                                .radius(R_CTRL)
                                .child(self.ui_locale.pick("在工作区继续这份计划", "Continue this plan in the workspace"))
                                .on_press(cx, |this, w, cx| this.recover_archived_plan(w, cx))))
                        .child(self.render_view_only_bar(&s, cx))
                        .into_any_element()
                } else {
                    let slash_popup = self.slash_popup(&s, window, cx);
                    self.render_composer(
                        center_w, activity_row, slash_popup, &s, window, cx,
                    )
                    .into_any_element()
                });

            // Each turn is a direct scroll child so its anchor follows live Markdown layout,
            // expanded tool details, window resizing and variable-height attachments.
            let rail_inset = turn_rail::leading_inset(self.sidebar_open);
            let rail = self.render_turn_navigation(center_w, rail_inset, &s, window, cx);
            let show_rail = rail.is_some();
            let gutter = chat_lane_gutter(center_w, window);
            let lane_margin = ((center_w - thread_w) / 2. - gutter).max(0.);
            let rail_padding = (rail_inset + turn_rail::CONTENT_CLEARANCE - lane_margin).max(gutter);
            // Markdown can gain a line after the final feed event has been delivered.
            // Read the previous layout immediately before the scroll view measures its new height.
            let tail = self.projected.turns.last().map(|turn| turn.id);
            let live_tail = window.use_keyed_state("recent-live-tail", cx, |_, _| None);
            let follow_layout = live_tail.update(cx, |recent, _| {
                if is_working {
                    *recent = tail.map(|id| (id, std::time::Instant::now()));
                }
                recent.is_some_and(|(id, at)| Some(id) == tail && at.elapsed() < Duration::from_secs(2))
            }) && pending_count == 0 && !self.chat_search_open && !self.chat_follow_paused;
            let geometry = (self.current_chat_id(), center_w);
            let previous = window.use_keyed_state("thread-reading-geometry", cx, |_, _| geometry);
            let (previous_chat, previous_width) =
                previous.update(cx, |last, _| std::mem::replace(last, geometry));
            let same_thread = previous_chat == geometry.0;
            let reflow = same_thread && (previous_width - center_w).abs() > 0.01;
            let anchor = (reflow && scroll_remaining > 1. && !reveal_question)
                .then(|| reading_anchor::ReadingAnchor::capture(&self.chat_scroll)).flatten();
            let handle = self.chat_scroll.clone();
            let count = rows.len();
            let scroll = div()
                .id("kar-timeline-scroll")
                .track_scroll(&self.chat_scroll)
                .size_full()
                .flex()
                .flex_col()
                .overflow_y_scroll()
                .on_scroll(cx.listener(|this, event: &ScrollEvent, window, cx| {
                    this.turn_navigation.cancel_jump();
                    let dy = event.delta.pixel_delta(window.line_height()).y;
                    if dy > px(0.) && this.chat_scroll.max_offset().y > px(1.) {
                        this.chat_follow_paused = true;
                    } else if dy < px(0.) && this.chat_follow_paused {
                        let view = cx.entity().downgrade();
                        window.on_next_frame(move |_, cx| {
                            let _ = view.update(cx, |this, cx| {
                                if this.chat_scroll.max_offset().y + this.chat_scroll.offset().y <= px(1.) {
                                    this.chat_follow_paused = false;
                                    cx.notify();
                                }
                            });
                        });
                    }
                }))
                .children(rows.into_iter().enumerate().map(|(index, row)| {
                    let row = chat_content_lane(center_w, thread_w, window)
                        .flex_none()
                        .pt(px(if index == 0 { 24. } else { 32. }))
                        .when(index + 1 == count, |d| d.pb(px(32.)))
                        .when(show_rail, |d| d.pl(px(rail_padding)))
                        .child(row).into_any_element();
                    match &anchor {
                        Some(anchor) => anchor.track(index, row),
                        None => row,
                    }
                }))
                .children(question);
            let center_lane = div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .when(follow_layout || reflow, |d| d.child(canvas(
                            move |viewport, window, _| {
                                if let Some(anchor) = anchor {
                                    anchor.restore(viewport, &handle, window);
                                } else if handle.max_offset().y + handle.offset().y <= px(1.) {
                                    handle.scroll_to_bottom();
                                }
                            },
                            |_, _, _, _| {},
                        ).absolute().inset_0()))
                        .child(scroll)
                        .child(timeline_top_fade(self.chat_scroll.clone(), crate::ui_primitives::material::reading_surface(&s)))
                        .children(rail)
                        .when(show_jump_bottom, |lane| lane.child(self.jump_to_latest(activity.is_some(), &s, window, cx))),
                )
                .children(approval)
                .child(foot);

            center_lane.into_any_element()
        };
        let stage = self.thread_body_with_details(stage, margin, height_fits, reveal, &s, cx);
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
        } else {
            stage
        };

        let pane = div()
            .id("kar-chat-drop-zone")
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.handle_dropped_paths(paths.paths(), window, cx);
            }))
            .child(header)
            .child(find_bar)
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
            .into_any_element();
        (pane, show_review_rail)
    }

    fn jump_to_latest(
        &self,
        generating: bool,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let palette = *s;
        let loader = generating.then(|| {
            window
                .use_keyed_state("jump-generating-indicator", cx, |_, cx| {
                    cx.new(|_| MorphLoader::new("jump-dots").with_size(20.).decorative(true))
                })
                .read(cx)
                .clone()
        });
        let label = self.ui_locale.pick("回到最新消息", "Jump to latest message");
        div()
            .absolute()
            .left(relative(0.5))
            .ml(px(-20.))
            .bottom(px(4.))
            .occlude()
            .child(
                Tactile::ghost("chat-jump-bottom", s)
                    .w(px(40.)).h(px(36.)).justify_center().radius(R_PILL)
                    .fills(transparent_black(), white().opacity(0.))
                    .aria(label)
                    .hover_glyph(move |reveal| {
                        let arrow = if loader.is_some() { reveal } else { 1. };
                        let layer = || {
                            div().absolute().inset_0().flex().items_center().justify_center()
                        };
                        div().relative().w(px(40. - 8. * arrow)).h(px(CTRL_H)).rounded_full()
                            .bg(palette.card).border_1().border_color(palette.line)
                            .when_some(loader.filter(|_| arrow < 1.), |d, loader| {
                                d.child(layer().opacity(1. - arrow).text_color(palette.muted).child(loader))
                            })
                            .child(layer().opacity(arrow).child(pro_icon(ProIcon::ArrowDown, 18., palette.ink)))
                            .into_any_element()
                    })
                    .on_press(cx, |this, window, cx| {
                        this.turn_navigation.cancel_jump();
                        this.chat_follow_paused = false;
                        this.chat_scroll.scroll_to_bottom();
                        let view = cx.entity().downgrade();
                        window.on_next_frame(move |_, cx| {
                            let _ = view.update(cx, |_, cx| cx.notify());
                        });
                        cx.notify();
                    }),
            )
    }

    fn render_chat_header(
        &self,
        pane_w: f32,
        revealed_dock_width: f32,
        on_start_screen: bool,
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
        let has_own_chat = matches!(
            self.access_mode,
            ChatAccessMode::WriteController { .. }
                | ChatAccessMode::LocalReadOnlySubscriber { .. }
                | ChatAccessMode::ArchivedReadOnly { .. }
        );

        let rail_hidden = (1. - self.sidebar_reveal).clamp(0., 1.);
        let base_pad = 20.0;
        let traffic_extra = macos_traffic_pad(window) - base_pad;
        let left_pad = base_pad + traffic_extra * rail_hidden;
        let collapsed_slot_w = (2. * CTRL_SM_H + TITLEBAR_BUTTON_GAP) * rail_hidden;
        // Leave room for the thread title and the panel controls when the sidebar tools move here.
        let title_lane = pane_w - left_pad - collapsed_slot_w;
        let compact_crumb = title_lane < if on_start_screen { 232. } else { 272. };

        let right_pad = TITLEBAR_EDGE_INSET + (CTRL_SM_H + TITLEBAR_BUTTON_GAP - revealed_dock_width).max(0.);
        let terminal = icon_button_sm("chat-toggle-bottom-terminal", ProIcon::PanelBottom,
                loc.pick("底部集成终端  ⇧⌘T", "Toggle bottom terminal  ⇧⌘T"), s)
            .expanded(self.bottom_terminal_open)
            .on_press(cx, |this, _, cx| this.toggle_bottom_terminal(cx));
        if self.thread_details_open && !self.thread_details_margin_available {
            let left = left_pad + (CTRL_SM_H + TITLEBAR_BUTTON_GAP) * rail_hidden;
            let controls_w = pane_w - left - right_pad - 2. * (CTRL_SM_H + TITLEBAR_BUTTON_GAP);
            return crate::ui_primitives::titlebar_row(window)
                .pl(px(left))
                .pr(px(right_pad))
                .gap(px(TITLEBAR_BUTTON_GAP))
                .child(self.details_titlebar_controls(controls_w, s, cx))
                .child(self.thread_details_toggle(s, cx))
                .child(terminal);
        }

        let ws = self.effective_workspace_dir();
        let proj_name = if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            "kar-studio".to_owned()
        } else {
            chat_id
                .and_then(|cid| self.project_registry.project_for_chat(&ws, cid))
                .map(|p| p.name.as_str())
                .or_else(|| ws.file_name().and_then(|name| name.to_str()))
                .map_or("Kwork", crate::studio_hub::project_display_name)
                .to_owned()
        };

        let new_in_project = loc.pick("在此项目中新建对话", "New thread in this project");
        let project_crumb = Tactile::ghost("chat-project-crumb", s)
            .h(px(CTRL_SM_H))
            .flex_none()
            .when(compact_crumb, |b| {
                b.w(px(24.)).flex_none().justify_center().child(render_folder_icon(16., s.muted))
            })
            .when(!compact_crumb, |b| b.px(px(4.)))
            .tip(new_in_project)
            .aria(format!("{new_in_project} · {proj_name}"))
            .when(!compact_crumb, |b| {
                b.child(
                    div()
                        .max_w(px((pane_w * 0.18).clamp(44., 160.)))
                        .truncate()
                        .text_size(s.font_size(FS_UI))
                        .text_color(s.muted)
                        .child(proj_name),
                )
            })
            .on_press(cx, move |this, win, cx| {
                this.select_project_and_new_chat(ws.clone(), win, cx)
            });

        let title_for_rename = title.clone();
        let title_el: AnyElement = if let Some(cid) = chat_id.filter(|_| has_own_chat) {
            Tactile::ghost("chat-header-rename", s)
                .h(px(CTRL_SM_H))
                .px(px(5.))
                .mx(px(-5.))
                .min_w_0()
                .tip(loc.pick("点击重命名对话", "Click to rename thread"))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(s.font_size(FS_UI))
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
                .text_size(s.font_size(FS_UI))
                .font_weight(FontWeight::MEDIUM)
                .text_color(s.ink)
                .child(title)
                .into_any_element()
        };

        crate::ui_primitives::titlebar_row(window)
            .pl(px(left_pad))
            .pr(px(right_pad))
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
                                .gap(px(TITLEBAR_BUTTON_GAP))
                                .child(div().size(px(CTRL_SM_H)).flex_none())
                                .child(
                                    icon_button_sm("chat-new", ProIcon::NewChat,
                                        loc.pick("新建对话  ⌘N", "New thread  ⌘N"), s)
                                        .on_press(cx, |this, window, cx| {
                                            this.start_new_chat(window, cx)
                                        }),
                                ),
                        )
                    })
                    .child(project_crumb)
                    .when(!compact_crumb, |d| {
                        d.child(div().text_size(s.font_size(FS_CONTROL)).text_color(s.faint).child("/"))
                    })
                    .child(title_el)
                    .when(!on_start_screen, |d| {
                        d.child(menu_host(
                            Tactile::ghost("chat-more", s)
                                .size(px(CTRL_SM_H))
                                .radius(crate::ui_primitives::R_SM)
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
                                .text_size(s.font_size(FS_TAG))
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
                    .when_some(
                        self.detected_dev_ports
                            .first()
                            .copied()
                            .filter(|_| self.prefs.auto_show_dev_preview_pill),
                        |d, port| {
                            d.child(
                                Tactile::soft("chat-localhost-preview-pill", s)
                                    .fills(s.pro.accent_soft, s.pro.accent_soft)
                                    .radius(R_CTRL)
                                    .h(px(CTRL_SM_H))
                                    .px(px(10.))
                                    .gap(px(5.))
                                    .font_features(tnum_features())
                                    .text_size(s.font_size(FS_META))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.pro.accent_info)
                                    .tip(loc.pick(
                                        "打开本地运行中的预览地址",
                                        "Open live localhost preview",
                                    ))
                                    .child(pro_icon(ProIcon::Globe, 13., s.pro.accent_info))
                                    .child(format!("localhost:{port}"))
                                    .on_press(cx, move |_, _, _| {
                                        let url = format!("http://localhost:{port}");
                                        let _ = std::process::Command::new("open")
                                            .arg(&url)
                                            .spawn();
                                    }),
                            )
                        },
                    )
                    .when(remote_active, |d| {
                        d.child(
                            Tactile::soft("chat-remote-live-pill", s)
                                .fills(s.pro.green_soft, s.pro.green_soft)
                                .radius(R_CTRL)
                                .h(px(CTRL_SM_H))
                                .px(px(9.))
                                .gap(px(5.))
                                .text_size(s.font_size(FS_META))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(s.pro.green_ink)
                                .child(pulse_beacon(s.pro.green, 5., true, window, cx))
                                .child(loc.pick("已配对", "Paired"))
                                .on_press(cx, |this, _, cx| {
                                    this.open_settings(SettingsTab::Remote, cx)
                                }),
                        )
                    })
                    .when(!on_start_screen || self.chat_search_open, |d| {
                        d.child(
                            icon_button_sm(
                                "chat-toggle-find",
                                ProIcon::Search,
                                loc.pick("对话内查找  ⌘F", "Find in thread  ⌘F"),
                                s,
                            )
                            .lit(self.chat_search_open)
                            .on_press(cx, |this, _, cx| {
                                this.chat_search_open = !this.chat_search_open;
                                cx.notify();
                            }),
                        )
                    })
                    .child(self.thread_details_toggle(s, cx))
                    .child(terminal),
            )
    }

    pub(crate) fn thread_details_toggle(&self, s: &StudioPalette, cx: &mut Context<Self>) -> Tactile {
        let label = self.ui_locale.pick("会话环境与 Git 详情卡", "Thread environment and Git details");
        Tactile::ghost("chat-toggle-details-card", s)
            .size(px(CTRL_SM_H))
            .justify_center()
            .lit(self.thread_details_open)
            .expanded(self.thread_details_open)
            .aria(label)
            .tip(label)
            .child(render_square_menu_icon(15.5, if self.thread_details_open { s.ink } else { s.muted }))
            .on_press(cx, |this, _, cx| this.toggle_thread_details(cx))
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
                Some(pro_icon(ProIcon::PageDoc, ICON, s.muted)),
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
        if self.supports_side_question() {
        rows.push(
            menu_icon_item("chat-more-side-q", ProIcon::Sparkles, "旁路快问（不入上下文）…", s)
                .on_press(cx, |this, _, cx| this.toggle_side_question_sheet(cx))
                .into_any_element(),
        );
        }

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
        self.projected
            .context_usage
            .as_ref()
            .and_then(context_usage::fraction)
    }

    fn usage_summary_line(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(ratio) = self.context_ratio() {
            let label = self.ui_locale.pick("上下文", "Context");
            parts.push(format!("{label} {:.0}%", ratio * 100.));
        } else if let Some(used) = self.projected.context_usage.as_ref().and_then(|u| u.tokens) {
            parts.push(format!(
                "{used} {}",
                self.ui_locale.pick("上下文词元", "context tokens")
            ));
        }
        if let Some(last) = self.projected.usage.last()
            && let Some(tot) = last
                .usage
                .with_children
                .as_ref()
                .or(last.usage.total.as_ref())
        {
            let total_io = tot.input.saturating_add(tot.output);
            if total_io > 0 {
                parts.push(format!(
                    "{} {}",
                    format_tokens_compact(total_io),
                    self.ui_locale.pick("词元", "tokens")
                ));
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

    fn step_chat_search(&mut self, previous: bool, cx: &mut Context<Self>) {
        let query = self.chat_search_query.trim().to_lowercase();
        let matches = matching_turn_indices(&self.projected.turns, &query);
        let count = matches.len();
        if count == 0 {
            return;
        }
        let current = self.chat_search_match_ix % count;
        self.chat_search_match_ix = if previous {
            current.checked_sub(1).unwrap_or(count - 1)
        } else {
            (current + 1) % count
        };
        self.chat_scroll.scroll_to_item(matches[self.chat_search_match_ix]);
        cx.notify();
    }

    fn render_chat_find_bar(
        &self,
        thread_w: f32,
        matches: &[usize],
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        const BAR_HEIGHT: f32 = 46.;
        #[derive(Clone)]
        struct ChatFindBarState {
            editor: Entity<Editor>,
            wired: bool,
            return_focus: Option<FocusHandle>,
            match_count: usize,
        }

        let loc = self.ui_locale;
        let open = self.chat_search_open;
        let state = window.use_keyed_state("kar-chat-find-state", cx, |win, cx| {
            let editor = cx.new(|cx| {
                let mut ed = Editor::new("对话内查找", InputMode::Search, win, cx);
                ed.set_placeholder(loc.pick("在当前对话记录中查找…", "Find in thread…"));
                ed.font_size = FS_UI;
                ed
            });
            ChatFindBarState {
                editor,
                wired: false,
                return_focus: None,
                match_count: 0,
            }
        });
        let cur_state = state.read(cx).clone();
        let editor = cur_state.editor;
        if !cur_state.wired {
            state.update(cx, |st, _| st.wired = true);
            cx.subscribe_in(
                &editor,
                window,
                |this, ed_ent, ev: &EditEvent, _win, cx| match ev {
                    EditEvent::Changed => {
                        this.chat_search_query = ed_ent.read(cx).value().to_owned();
                        this.chat_search_match_ix = 0;
                        let q_lc = this.chat_search_query.trim().to_lowercase();
                        if !q_lc.is_empty() {
                            let matches = matching_turn_indices(&this.projected.turns, &q_lc);
                            if let Some(&first_turn_ix) = matches.first() {
                                this.chat_scroll.scroll_to_item(first_turn_ix);
                            }
                        }
                        cx.notify();
                    }
                    EditEvent::Submitted => this.step_chat_search(false, cx),
                    EditEvent::Escape => {
                        this.chat_search_open = false;
                        cx.notify();
                    }
                    _ => {}
                },
            )
            .detach();
        }
        if open != cur_state.return_focus.is_some() {
            editor.update(cx, |ed, _| ed.disabled = !open);
            if open {
                let origin = window.focused(cx).unwrap_or_else(|| self.focus_handle.clone());
                state.update(cx, |st, _| st.return_focus = Some(origin));
                let focus = editor.read(cx).focus.clone();
                let view = cx.weak_entity();
                window.defer(cx, move |win, cx| {
                    if view.upgrade().is_some_and(|wb| wb.read(cx).chat_search_open) {
                        focus.focus(win, cx);
                    }
                });
            } else if let Some(origin) = state.update(cx, |st, _| st.return_focus.take()) {
                origin.focus(window, cx);
            }
        }
        let reveal = pro_spring_f32("kar-chat-find-reveal", f32::from(open), 0.16, window, cx)
            .clamp(0., 1.);
        if !open && reveal <= 0.001 {
            return div().h_0().into_any_element();
        }

        // Keep the counter and controls in place until their exit finishes.
        let match_count = if open { matches.len() } else { cur_state.match_count };
        if open && match_count != cur_state.match_count {
            state.update(cx, |st, _| st.match_count = match_count);
        }
        let cur_ix = if match_count == 0 {
            0
        } else {
            (self.chat_search_match_ix % match_count) + 1
        };
        let counter_label = if self.chat_search_query.trim().is_empty() {
            String::new()
        } else if match_count == 0 {
            loc.pick("无匹配", "0 matches").to_owned()
        } else {
            format!("{cur_ix} / {match_count}")
        };
        let focused = editor.read(cx).focus.is_focused(window);

        let bar = div()
            .w_full()
            .h(px(BAR_HEIGHT))
            .flex_none()
            .max_w(px(thread_w))
            .mx_auto()
            .px(px(THREAD_PAD))
            .pt(px(6.))
            .pb(px(2.))
            .child(
                card_box(s)
                    .h(px(38.))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        search_input_box("chat-find-input-box", editor.clone(), focused, true, s)
                            .capture_key_down(cx.listener(move |this, ev: &KeyDownEvent, win, cx| {
                                if ev.keystroke.key == "enter"
                                    && ev.keystroke.modifiers == Modifiers::shift()
                                    && !editor.read(cx).is_composing()
                                {
                                    this.step_chat_search(true, cx);
                                    win.prevent_default();
                                    cx.stop_propagation();
                                }
                            }))
                            .flex_1()
                            .min_w_0()
                            .border_0()
                            .bg(transparent_black()),
                    )
                    .when(!counter_label.is_empty(), |d| {
                        d.child(
                            div()
                                .flex_none()
                                .px(px(6.))
                                .text_size(s.font_size(FS_META))
                                .text_color(if match_count == 0 {
                                    s.pro.red_ink
                                } else {
                                    s.muted
                                })
                                .font_features(tnum_features())
                                .child(counter_label),
                        )
                    })
                    .child(
                        icon_button_sm(
                            "chat-find-prev",
                            ProIcon::ChevronUp,
                            loc.pick("上一处匹配  ⇧↵", "Previous match  ⇧↵"),
                            s,
                        )
                        .disabled(match_count == 0)
                        .when(!open && match_count > 0, |b| b.inert())
                        .on_press(cx, |this, _, cx| this.step_chat_search(true, cx)),
                    )
                    .child(
                        icon_button_sm(
                            "chat-find-next",
                            ProIcon::ChevronDown,
                            loc.pick("下一处匹配  ↵", "Next match  ↵"),
                            s,
                        )
                        .disabled(match_count == 0)
                        .when(!open && match_count > 0, |b| b.inert())
                        .on_press(cx, |this, _, cx| this.step_chat_search(false, cx)),
                    )
                    .child(
                        icon_button_sm(
                            "chat-find-close",
                            ProIcon::Close,
                            loc.pick("关闭查找  Esc", "Close find  Esc"),
                            s,
                        )
                        .when(!open, |b| b.inert())
                        .on_press(cx, |this, _, cx| {
                            this.chat_search_open = false;
                            cx.notify();
                        }),
                    ),
            )
            .into_any_element();

        div()
            .w_full()
            .h(px(BAR_HEIGHT * reveal))
            .flex_none()
            .overflow_hidden()
            .opacity(reveal)
            .child(bar)
            .into_any_element()
    }

    fn render_share_bar(
        &self,
        thread_w: f32,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let url = self.active_http_server_url.clone();
        let open = url.is_some();
        let bar = url.map(|url| {
            let to_copy = url.clone();
            div()
                .w_full()
                .max_w(px(thread_w))
                .mx_auto()
                .px(px(THREAD_PAD))
                .pt(px(6.))
                .pb(px(2.))
                .child(
                    div()
                        .h(px(ROW_H))
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
                                .text_size(s.font_size(FS_META))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(s.ink)
                                .child("正在共享"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(s.font_size(FS_META))
                                .text_color(s.muted)
                                .child(url),
                        )
                        .child(text_button_sm("share-copy", "复制链接", s).on_press(
                            cx,
                            move |this, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(to_copy.clone()));
                                this.notify_toast("链接已复制", cx);
                            },
                        ))
                        .child(
                            text_button_sm("share-stop", "停止", s)
                                .on_press(cx, |this, _, cx| this.stop_sharing(cx)),
                        ),
                )
                .into_any_element()
        });
        pro_fold("kar-share-bar", open, 42., 0., move || {
            bar.unwrap_or_else(|| div().into_any_element())
        })
    }

    fn render_goal_bar(
        &self,
        thread_w: f32,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::kar_bridge::GoalAction;
        let chat = self.current_chat_id().filter(|id| self.projected.chat_id == Some(*id));
        let goal = chat.and_then(|_| self.projected.goal.as_ref()?.as_ref());
        let loc = self.ui_locale;
        let bar = goal.map(|g| {
            let budget = g.token_budget.flatten();
            let owner = chat.filter(|_| self.can_set_chat_goal());
            let control = match g.status.as_str() {
                "active" => Some((GoalAction::Pause, ProIcon::Pause, loc.pick("暂停目标", "Pause goal"))),
                "paused" | "blocked" | "budgetLimited" | "usageLimited" => Some((GoalAction::Resume, ProIcon::ArrowRight, loc.pick("继续目标", "Resume goal"))),
                _ => None,
            };
            let mut row = div().w_full().min_w_0().h(px(ROW_H)).px(px(8.))
                .rounded(px(R_CTRL)).bg(s.soft).flex().items_center().gap(px(6.));
            if let Some((used, limit)) = g.tokens_used.zip(budget).filter(|(_, limit)| *limit > 0) {
                row = row.child(pro_progress_ring((used as f64 / limit as f64).min(1.) as f32, 14., 2., s.ink, s.line_strong));
            } else {
                row = row.child(pro_icon(ProIcon::Tasks, ICON_SM, s.muted));
            }
            row = row.child(div().flex_1().min_w_0().truncate()
                    .text_color(s.ink).text_size(s.font_size(FS_CONTROL)).child(g.objective.clone()))
                .child(div().flex_none().text_size(s.font_size(FS_META)).text_color(s.muted).child(self.goal_status_label(&g.status)));
            if thread_w >= 520. && let Some(seconds) = g.time_used_seconds.filter(|s| *s > 0) {
                row = row.child(div().flex_none().font_features(tnum_features()).text_size(s.font_size(FS_META)).text_color(s.muted)
                    .child(worked_duration(seconds.saturating_mul(1000), loc)));
            }
            if let Some(id) = owner {
                if let Some((action, icon, label)) = control {
                    row = row.child(icon_button_sm("goal-toggle", icon, label, s)
                        .disabled(self.goal_request_pending(id))
                        .on_press(cx, move |this, _, cx| { this.request_chat_goal(id, action.clone(), cx); }));
                }
                row = row.child(text_button_sm("goal-edit", loc.pick("管理", "Manage"), s)
                    .on_press(cx, move |this, window, cx| {
                        if this.current_chat_id() == Some(id) { this.open_goal_dialog(window, cx); }
                    }));
            }
            div().w_full().max_w(px(thread_w)).mx_auto().px(px(THREAD_PAD)).pt(px(6.)).pb(px(2.))
                .child(row).into_any_element()
        });
        let open = bar.is_some();
        pro_fold("kar-goal-bar", open, 42., 0., move || {
            bar.unwrap_or_else(|| div().into_any_element())
        })
    }

    fn render_notice(
        &self,
        thread_w: f32,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let block = self
            .notice
            .clone()
            .map(|notice| self.render_notice_block(thread_w, notice, s, cx));
        pro_fold("kar-notice", self.notice_open, 72., 0., move || {
            block.unwrap_or_else(|| div().into_any_element())
        })
    }

    fn render_notice_block(
        &self,
        thread_w: f32,
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
            .max_w(px(thread_w))
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
                                    .text_size(s.font_size(FS_UI))
                                    .line_height(s.line_height(LH_UI))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(title_ink)
                                    .child(notice.title),
                            )
                            .when(!notice.detail.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_size(s.font_size(FS_META))
                                        .line_height(s.line_height(LH_META))
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
        let thread_w = self.prefs.chat_width_mode.width_for_pane(pane_w);
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
        let project =
            crate::studio_hub::project_display_name(&project).replace(['\n', '\r', '\t'], " ");
        let content_w = (pane_w - 2. * chat_lane_gutter(pane_w, window)).clamp(1., thread_w);
        let title_size = f32::from(s.font_size((content_w / 24.).clamp(18., 22.)));
        let title_line = (title_size * 1.38).ceil();
        let short = f32::from(window.viewport_size().height) <= 544.;
        let can_switch = !self.chat_is_open();
        let full_title = match loc {
            crate::ui_primitives::UiLocale::Zh => format!("要在 {project} 中构建什么？"),
            crate::ui_primitives::UiLocale::En => format!("What should we build in {project}?"),
        };
        let run = window.text_style().to_run(full_title.len());
        let title_width = window
            .text_system()
            .shape_line(full_title.into(), px(title_size), &[run], None)
            .width();
        let inline = f32::from(title_width) + 32. <= content_w;
        let label_size = if inline { title_size } else { f32::from(s.font_size(FS_READ)) };
        let label_line = if inline { title_line } else { f32::from(s.line_height(LH_READ)) };
        let picker = Tactile::ghost("draft-project", s)
            .h(px(label_line + 8.))
            .max_w(px(content_w))
            .px(px(4.))
            .gap(px(4.))
            .text_size(px(label_size))
            .line_height(px(label_line))
            .font_weight(FontWeight::MEDIUM)
            .ink(s.ink, s.ink)
            .aria(format!(
                "{} · {project}",
                loc.pick("切换项目", "Switch project")
            ))
            .child(
                div()
                    .max_w(px((content_w - 38.).max(1.)))
                    .truncate()
                    .child(project),
            )
            .when(can_switch, |button| {
                button.child(pro_icon(ProIcon::ChevronDown, 12., s.muted))
            });
        let project_picker = if can_switch {
            menu_host(
                picker.on_press(cx, |this, _, cx| {
                    this.toggle_menu(Menu::ComposerProject, cx)
                }),
                self.render_composer_project_menu(s, window, cx),
            )
            .into_any_element()
        } else {
            picker.inert().into_any_element()
        };
        let heading = div()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(title_size))
            .line_height(px(title_line))
            .font_weight(FontWeight::NORMAL)
            .text_color(s.ink);
        let heading = if self.pinned_target.is_some() {
            heading.child(loc.pick("讨论选中内容", "Discuss selection"))
        } else if inline {
            heading
                .child(
                    div()
                        .mr(px(2.))
                        .child(loc.pick("要在", "What should we build in")),
                )
                .child(project_picker)
                .child(loc.pick("中构建什么？", "?"))
        } else {
            heading
                .flex_col()
                .gap(px(3.))
                .child(loc.pick("我们要构建什么？", "What should we build?"))
                .child(project_picker)
        };

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
        let slash_popup = self.slash_popup(s, window, cx);
        let browsing_commands = slash_popup.is_some();
        let planning = self.planning_here();
        let composer =
            self.render_composer(pane_w, None, slash_popup, s, window, cx);
        let content = chat_content_lane(pane_w, thread_w, window)
            .flex_none()
            .flex()
            .flex_col()
            .when(!planning, |col| {
                col.child(heading.when(browsing_commands, |title| title.invisible()))
            })
            .when_some(self.planning_topic(), |col, topic| {
                let visible_lines = if short { 2. } else { 6. };
                col.child(
                    div().w_full().flex().justify_end().child(
                        div().max_w(relative(0.86)).px(px(12.)).py(px(7.))
                            .rounded(px(R_CARD)).bg(s.hover).child(
                                div().id("planned-task-scroll").max_h(px(20. * visible_lines))
                                    .overflow_y_scroll().text_size(s.font_size(FS_UI)).line_height(s.line_height(20.))
                                    .text_color(s.ink).child(topic.to_owned())
                            )
                    )
                )
            })
            .when(!short && !planning, |col| {
                col.child(
                    div().when(browsing_commands, |hint| hint.invisible()).mt(px(4.)).flex().justify_center().child(
                        secondary
                            .h(px(CTRL_SM_H))
                            .px(px(8.))
                            .text_size(s.font_size(FS_CONTROL))
                            .ink(s.muted, s.ink),
                    ),
                )
            })
            .child(div().mt(px(if short { 10. } else { 16. })).child(composer))
            .when(self.start_dropzone_open, |col| {
                col.child(pro_fold(
                    "kar-start-dropzone-fold",
                    true,
                    184.,
                    0.,
                    move || div().w_full().pt(px(8.)).child(dropzone).into_any_element(),
                ))
            });

        div()
            .id("kar-start-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .min_h_full()
                    .py(px(if short { 10. } else { 16. }))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(div().flex_grow(0.72))
                    .child(content)
                    .child(div().flex_grow(1.)),
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
                    .text_size(s.font_size(FS_READ))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.ink)
                    .child(title),
            )
            .child(
                div()
                    .text_size(s.font_size(FS_UI))
                    .line_height(s.line_height(LH_UI))
                    .text_color(s.muted)
                    .child(detail.to_owned()),
            )
            .into_any_element()
    }

    // ── Two-Tier Composer (`AssistantHomeScreen` KUI architecture) ───────────

    fn render_view_only_bar(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let locale = self.ui_locale;
        let (icon, text) = match &self.access_mode {
            ChatAccessMode::HttpReadOnly { .. } => (
                ProIcon::Link,
                locale
                    .pick(
                        "正在通过共享链接实时查看对话",
                        "Viewing live updates through a shared link",
                    )
                    .to_owned(),
            ),
            ChatAccessMode::RemoteController { .. } => (
                ProIcon::Eye,
                locale
                    .pick(
                        "已作为观察者与主机配对，可实时查看对话",
                        "Paired as a viewer. Conversation updates appear live.",
                    )
                    .to_owned(),
            ),
            ChatAccessMode::ArchivedReadOnly { reason, .. } => (
                ProIcon::Clock,
                locale
                    .pick(reason.as_str(), "Viewing saved conversation history")
                    .to_owned(),
            ),
            _ => (
                ProIcon::Eye,
                locale
                    .pick(
                        "另一窗口正在控制此对话，此处实时同步",
                        "Another window controls this conversation. Updates appear live.",
                    )
                    .to_owned(),
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
            .text_size(s.font_size(FS_UI))
            .line_height(s.line_height(LH_UI))
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
                    soft_button("view-only-retry", locale.pick("重新连接", "Reconnect"), s)
                        .on_press(cx, move |this, window, cx| this.open_chat(chat_id, window, cx))
                } else {
                    soft_button("view-only-new-chat", locale.pick("开启新对话", "New thread"), s)
                        .on_press(cx, |this, window, cx| this.start_new_chat(window, cx))
                }
            )
            .into_any_element()
    }

    fn render_composer(
        &self,
        pane_w: f32,
        activity_row: Option<AnyElement>,
        slash_popup: Option<AnyElement>,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let desired_mode = match self.prefs.send_shortcut {
            crate::prefs::SendShortcutMode::Enter => InputMode::InlineMulti,
            crate::prefs::SendShortcutMode::ModEnter => InputMode::Multi,
        };
        if self.composer_editor.read(cx).mode != desired_mode {
            self.composer_editor
                .update(cx, |ed, _| ed.mode = desired_mode);
        }
        let mut visible_draft = self.composer_editor.read(cx).value().to_owned();
        let has_draft = !visible_draft.trim().is_empty() || !self.pending_attachments.is_empty();
        let dictation = self.composer_dictation();
        let suffix = crate::workbench::speech_suffix(&visible_draft, dictation.unwrap_or_default());
        self.composer_editor.update(cx, |editor, cx| editor.set_dictation_preview(&suffix, cx));
        visible_draft.push_str(&suffix);
        // The Git split can halve the writing width. Measure the same shaped text
        // that GPUI paints so wrapped CJK drafts remain visible while editing.
        let thread_w = self.prefs.chat_width_mode.width_for_pane(pane_w);
        let card_width = (pane_w - 2. * chat_lane_gutter(pane_w, window)).min(thread_w);
        let input_width = (card_width - 34.).max(60.);
        let lines = if visible_draft.is_empty() {
            2
        } else if visible_draft.len() > 4096 {
            10
        } else {
            let run = window.text_style().to_run(visible_draft.len());
            window
                .text_system()
                .shape_text(
                    visible_draft.clone().into(),
                    s.font_size(FS_READ),
                    &[run],
                    Some(px(input_width)),
                    Some(10),
                )
                .map(|rows| {
                    rows.iter()
                        .map(|r| r.wrap_boundaries().len() + 1)
                        .sum::<usize>()
                })
                .unwrap_or_else(|_| visible_draft.lines().count())
                .clamp(2, 10)
        };
        // Keep the transcript and pending requests reachable beside a long draft.
        // The editor scrolls internally once it takes a third of the chat height.
        let window_h = f32::from(window.viewport_size().height);
        let drawer_h = self.bottom_terminal_anim.get(window, cx).min(window_h);
        let editor_line = f32::from(s.font_size(FS_READ)) + 7.75;
        let max_field_h = ((window_h - drawer_h - HEADER_H) / 3.).max(2. * editor_line);
        let field_h = pro_spring_f32(
            "kar-composer-height",
            (lines as f32 * editor_line).min(max_field_h),
            0.16,
            window,
            cx,
        );

        let chat_open = self.chat_is_open();
        let turn_running = self.turn_in_progress();
        let can_send = has_draft && dictation.is_none() && !self.plan_input_blocked() && self.queued_prompt.is_none() && (!turn_running || chat_open);

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

        let attachments_bar = self.render_attachment_tray(s, window, cx);
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

        let runtime_id = if matches!(self.access_mode, ChatAccessMode::DemoShowcase) {
            "claude".to_owned()
        } else {
            self.effective_runtime()
        };
        let cur_approval = self.effective_chat_approvals();
        let is_full_auto = crate::prefs::is_approval_allow(cur_approval);
        let is_read_only = cur_approval == ApprovalMode::Deny;
        let (perm_icon, perm_icon_col) = if is_full_auto {
            (ProIcon::Bolt, s.pro.accent_info)
        } else if is_read_only {
            (ProIcon::Lock, s.pro.amber_ink)
        } else {
            (ProIcon::Shield, s.muted)
        };
        let perm_pill = menu_host(
            Tactile::ghost("cmp-options", s)
                .h(px(CTRL_H))
                .px(px(8.))
                .radius(R_CTRL)
                .gap(px(5.))
                .text_size(s.font_size(FS_CONTROL))
                .when(is_full_auto, |b| {
                    b.fills(s.pro.accent_soft.opacity(0.65), s.pro.accent_soft)
                        .ink(s.pro.accent_info, s.pro.accent_info)
                })
                .when(is_read_only, |b| {
                    b.fills(s.pro.amber_soft.opacity(0.65), s.pro.amber_soft)
                        .ink(s.pro.amber_ink, s.pro.amber_ink)
                })
                .when(!is_full_auto && !is_read_only, |b| b.ink(s.muted, s.ink))
                .lit(self.open_menu == Some(Menu::ComposerApproval))
                .aria(loc.pick("对话执行权限模式", "Conversation permission mode"))
                .when(self.open_menu != Some(Menu::ComposerApproval), |button| {
                    button.tip(if chat_open {
                        loc.pick("点击随时切换当前对话的权限模式", "Switch permission mode for this conversation")
                    } else {
                        loc.pick("设置新对话的执行权限模式", "Set permission mode for new conversations")
                    })
                })
                .child(pro_icon(perm_icon, 14.5, perm_icon_col))
                .when(card_width >= 380., |b| {
                    b.child(loc.approval_label(cur_approval))
                })
                .child(pro_icon(ProIcon::ChevronDown, 11., s.muted))
                .on_press(cx, |this, _, cx| {
                    this.toggle_menu(Menu::ComposerApproval, cx)
                }),
            self.render_approval_menu(start, s, cx),
        );

        let model = if chat_open {
            self.projected
                .info
                .as_ref()
                .and_then(|i| i.model.as_deref())
        } else {
            self.selected_model.as_deref()
        };
        let add_menu = match self.open_menu {
            Some(Menu::ComposerSkills) => self.render_skills_menu(end, s, cx),
            _ => self.render_add_menu(end, s, cx),
        };

        let ctx_ratio = self.context_ratio();
        let usage = self.projected.context_usage.as_ref();
        let ctx_label = ctx_ratio
            .map(|r| format!("{:.0}%", r * 100.))
            .unwrap_or_else(|| loc.pick("上下文", "Context").into());
        let mut context_rows = Vec::new();
        if self.open_menu == Some(Menu::ContextUsage)
            && let Some(info) = self.projected.info.as_ref().filter(|info| Some(info.id) == self.current_chat_id())
        {
            let default = loc.pick("运行时默认", "Runtime default");
            let mut metadata = div().px(px(8.)).py(px(6.)).flex().flex_col().gap(px(6.));
            for (label, value) in [
                (loc.pick("智能体", "Agent"), runtime_display_name(&info.runtime)),
                (loc.pick("模型", "Model"), info.model.as_deref().unwrap_or(default).to_owned()),
                (loc.pick("推理档位", "Reasoning"), info.settings.effort.as_deref().map(|level| effort_label(level, loc)).unwrap_or(default).to_owned()),
            ] {
                metadata = metadata.child(context_usage::detail_row(label, value, s));
            }
            context_rows.push(metadata.into_any_element());
            let id = info.id.to_string();
            context_rows.push(menu_icon_item("copy-session-id", ProIcon::Copy,
                loc.pick("复制会话 ID", "Copy chat ID"), s)
                .child(menu_hint(id[..8].to_owned(), s).font_features(tnum_features()))
                .on_press(cx, move |this, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(id.clone()));
                    this.close_menu(cx);
                    this.notify_toast(loc.pick("已复制会话 ID", "Chat ID copied"), cx);
                }).into_any_element());
            context_rows.push(menu_divider(s).into_any_element());
        }
        context_rows.push(
            context_usage::inspector(usage, self.projected.manual_compaction.as_ref(), loc, s)
                .into_any_element(),
        );
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
        let show_ctx = usage.is_some()
            || self.projected.can_compact
            || self.open_menu == Some(Menu::ContextUsage);
        let ctx_control = show_ctx.then(|| {
            menu_host(
                Tactile::ghost("cmp-context-inspector-btn", s)
                    .h(px(CTRL_H))
                    .px(px(7.))
                    .gap(px(5.))
                    .lit(self.open_menu == Some(Menu::ContextUsage))
                    .aria(loc.pick("上下文容量与压缩", "Context window usage"))
                    .when(self.open_menu != Some(Menu::ContextUsage), |button| {
                        button.tip(loc.pick("上下文容量与压缩", "Context window usage"))
                    })
                    .font_features(tnum_features())
                    .text_size(s.font_size(FS_CONTROL))
                    .ink(s.muted, s.ink)
                    .when_some(ctx_ratio, |row, r| {
                        row.child(pro_progress_ring(r, 13., 2., s.ink, s.line_strong))
                    })
                    .child(ctx_label)
                    .on_press(cx, |this, _, cx| this.toggle_menu(Menu::ContextUsage, cx)),
                self.render_menu(Menu::ContextUsage, start, 272., context_rows, s, cx),
            )
        });

        let can_switch_perm = self.access_mode.is_writable()
            || matches!(self.access_mode, ChatAccessMode::NoneSelected);
        let left = if self.active_voice_rec.is_some() {
            self.render_recording_controls(card_width, s, window, cx)
        } else if dictation.is_some() {
            div().flex_none().flex().items_center().gap(px(4.)).px(px(6.))
                .text_size(s.font_size(FS_META)).text_color(s.muted)
                .child(loc.pick("正在整理听写…", "Finishing dictation…"))
                .child(icon_button_sm("cmp-discard-transcription", ProIcon::Close,
                    loc.pick("放弃这次听写", "Discard dictation"), s)
                    .on_press(cx, |this, window, cx| {
                        this.discard_voice_dictation(cx);
                        this.composer_editor.read(cx).focus.clone().focus(window, cx);
                    }))
        } else {
            div()
                .flex_none()
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
                .child(
                    icon_button(
                        "cmp-voice-mic",
                        ProIcon::Mic,
                        loc.pick("语音听写  ⇧⌘V", "Dictate  ⇧⌘V"),
                        s,
                    )
                    .on_press(cx, |this, win, cx| {
                        this.toggle_voice_dictation(win, cx);
                        this.composer_editor.read(cx).focus.clone().focus(win, cx);
                    }),
                )
                .when(can_switch_perm, |d| d.child(perm_pill))
                .when_some(ctx_control, |d, c| d.child(c))
        };

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
            .min_w_0()
            .max_w_full()
            .h(px(CTRL_H))
            .px(px(8.))
            .radius(R_CTRL)
            .gap(px(6.))
            .text_size(s.font_size(FS_CONTROL))
            .child(render_runtime_mark(&runtime_id, 16., s, cx))
            .aria(display_agent.clone())
            .when(card_width >= 312., |b| b.child(
                div()
                    .min_w_0()
                    .max_w(px(240.))
                    .truncate()
                    .text_color(s.text)
                    .child(display_agent),
            ))
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
                if self.open_menu == Some(Menu::ComposerEffort) {
                    self.render_effort_menu(Menu::ComposerEffort, end, s, cx)
                } else {
                    self.render_agent_menu(s, window, cx)
                },
            )
            .flex_shrink(1.)
            .min_w_0()
            .into_any_element()
        };

        let action_disc = |id| {
            Tactile::ghost(id, s)
                .size(px(CTRL_H))
                .face_inset(1.5)
                .radius(R_PILL)
                .flex_none()
                .justify_center()
        };
        let primary_action: AnyElement = if turn_running && !has_draft {
            action_disc("cmp-stop")
                .fills(s.pro.red, s.pro.red_ink)
                .tip(loc.pick("停止当前执行", "Stop generation"))
                .child(div().size(px(9.)).rounded(px(R_XS)).bg(s.on_brand_action))
                .on_press(cx, |this, _, _| this.bridge.send(KarCommand::AbortTurn))
                .into_any_element()
        } else {
            action_disc("cmp-send")
                .fills(s.brand_action, s.brand_action)
                .lit_fill(s.brand_action)
                .lit(can_send)
                .disabled(!can_send)
                .tip(if turn_running {
                    match self.prefs.follow_up_mode {
                        crate::prefs::FollowUpMode::Queue => {
                            loc.pick("加入排队，待当前轮完成后自动发送", "Queue follow-up after active turn")
                        }
                        crate::prefs::FollowUpMode::Steer => {
                            loc.pick("实时汇入当前轮引导智能体", "Steer active turn immediately")
                        }
                    }
                } else {
                    match self.prefs.send_shortcut {
                        crate::prefs::SendShortcutMode::Enter => loc.pick("发送  ↵", "Send  ↵"),
                        crate::prefs::SendShortcutMode::ModEnter => loc.pick("发送  ⌘↵", "Send  ⌘↵"),
                    }
                })
                .child(pro_icon(ProIcon::ArrowUp, 15., s.on_brand_action))
                .on_press(cx, |this, window, cx| {
                    this.submit_composer_prompt(window, cx)
                })
                .into_any_element()
        };

        let right = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .justify_end()
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

        let editor_focus = self.composer_editor.read(cx).focus.clone();
        let editor_focused = editor_focus.is_focused(window);
        let write_card = crate::ui_primitives::frosted_composer(s)
            .when(editor_focused, |card| card.border_color(s.ink.opacity(0.24)))
            .w_full()
            .pt(px(16.))
            .pb(px(8.))
            .flex()
            .flex_col()
            .child(pinned_fold)
            .child(attachments_bar)
            .child(
                div()
                    .w_full()
                    .h(px(field_h))
                    .px(px(16.))
                    .cursor_text()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        editor_focus.focus(window, cx);
                    })
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
            .capture_key_down(cx.listener(|this, ev: &KeyDownEvent, win, cx| {
                this.navigate_slash_picker(ev, win, cx);
            }))
            .children(activity_row)
            .children(self.render_aster_mode(s, cx))
            .children(self.render_planning_panel(pane_w, window, s, cx))
            .child(
                div()
                    .relative()
                    .w_full()
                    .child(write_card)
                    .children(slash_popup),
            )
            .when(is_draft, |d| {
                let wt = self.new_chat_use_worktree;
                let local_label = loc.pick("当前工作目录", "Current checkout");
                let tree_label = loc.pick("独立分支副本", "Isolated worktree");
                let mode_label = if wt { tree_label } else { local_label };
                let mode_button = Tactile::ghost("cmp-worktree-mode", s)
                    .h(px(CTRL_SM_H))
                    .px(px(8.))
                    .radius(R_SM)
                    .gap(px(6.))
                    .text_size(s.font_size(FS_META))
                    .ink(s.muted, s.ink)
                    .aria(mode_label)
                    .child(render_folder_icon(13.5, s.muted))
                    .child(mode_label);
                let mode = if self.git_snapshot.is_repo || wt {
                    let choices = [local_label, tree_label]
                        .into_iter()
                        .enumerate()
                        .map(|(index, label)| {
                            let worktree = index == 1;
                            menu_item(("cmp-workspace-mode", index), None, label, s)
                                .when(worktree == wt, |b| b.child(menu_check(s)))
                                .on_press(cx, move |this, _, cx| {
                                    this.set_new_chat_worktree(worktree, cx);
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
                        self.render_menu(Menu::ComposerWorkspaceMode, start, 196., choices, s, cx),
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
                        .text_size(s.font_size(FS_META))
                        .child(mode)
                        .child(div().flex_1())
                        .when(self.git_snapshot.is_repo && pane_w >= 400., |row| {
                            let branch = self
                                .git_snapshot
                                .current_branch
                                .as_deref()
                                .unwrap_or("HEAD");
                            let trigger = Tactile::ghost("draft-branch", s)
                                .h(px(CTRL_SM_H))
                                .px(px(8.))
                                .radius(R_SM)
                                .gap(px(5.))
                                .text_size(s.font_size(FS_META))
                                .ink(s.muted, s.ink)
                                .child(render_git_branch_icon(13.5, s.muted))
                                .child(
                                    div()
                                        .max_w(px((pane_w * 0.24).clamp(60., 200.)))
                                        .truncate()
                                        .child(branch.to_owned()),
                                );
                            if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
                                row.child(menu_host(
                                    trigger
                                        .tip(loc.pick("切换分支", "Switch branch"))
                                        .lit(self.open_menu == Some(Menu::ComposerBranch))
                                        .on_press(cx, |this, _, cx| {
                                            this.toggle_menu(Menu::ComposerBranch, cx)
                                        }),
                                    self.render_git_branch_menu(
                                        Menu::ComposerBranch,
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
                        }),
                )
            })
    }

    fn render_composer_project_menu(
        &self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let open = self.open_menu == Some(Menu::ComposerProject);
        let cur_norm = self
            .config
            .workspace_dir
            .canonicalize()
            .unwrap_or_else(|_| self.config.workspace_dir.clone());
        let current = self.project_registry.projects.iter().position(|project| {
            project
                .root_path
                .canonicalize()
                .unwrap_or_else(|_| project.root_path.clone())
                == cur_norm
        });
        let count = self.project_registry.projects.len();
        let nav = window.use_keyed_state("home-project-navigation", cx, |_, cx| HomeProjectMenu {
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            cursor: 0,
            open: false,
            focus_on_mount: false,
        });
        if nav.read(cx).open != open {
            nav.update(cx, |state, cx| {
                state.open = open;
                if open {
                    state.focus_on_mount = true;
                    state.cursor = current.unwrap_or(0);
                    state.reveal(count);
                } else if state.focus.contains_focused(window, cx) {
                    self.composer_editor
                        .read(cx)
                        .focus
                        .clone()
                        .focus(window, cx);
                }
            });
        }
        let cursor = nav.read(cx).cursor.min(count);
        let caption = menu_caption(loc.pick("切换活跃项目", "Switch project"), s)
            .pt(px(4.))
            .line_height(s.line_height(LH_META));
        let mut rows = vec![caption.into_any_element()];

        for (ix, proj) in self.project_registry.projects.iter().enumerate() {
            rows.push(
                menu_item(
                    ("cmp-proj-item", ix),
                    Some(render_folder_icon(15., s.text)),
                    crate::studio_hub::project_display_name(&proj.name).to_owned(),
                    s,
                )
                .lit(ix == cursor)
                .when(Some(ix) == current, |r| r.child(menu_check(s)))
                .on_press(cx, move |this, win, cx| {
                    this.choose_home_project(ix, current, win, cx)
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
            .lit(cursor == count)
            .on_press(cx, move |this, win, cx| {
                this.choose_home_project(count, current, win, cx)
            })
            .into_any_element(),
        );

        let mounted = nav.clone();
        let forward = nav.clone();
        let backward = nav.clone();
        let list = div()
            .id("home-project-options")
            .key_context(HOME_PROJECT_KEY_CONTEXT)
            .track_focus(&nav.read(cx).focus)
            .track_scroll(&nav.read(cx).scroll)
            .max_h(px(
                (f32::from(window.viewport_size().height) - 32.).clamp(36., 348.)
            ))
            .overflow_y_scroll()
            .children(rows)
            .on_action(cx.listener(move |_, _: &NextHomeProject, _, cx| {
                forward.update(cx, |state, _| state.advance(1, count));
                cx.stop_propagation();
                cx.notify();
            }))
            .on_action(cx.listener(move |_, _: &PreviousHomeProject, _, cx| {
                backward.update(cx, |state, _| state.advance(count, count));
                cx.stop_propagation();
                cx.notify();
            }))
            .when(open, |list| {
                list.child(
                    canvas(
                        |_, _, _| (),
                        move |_, _, win, cx| {
                            if mounted.read(cx).focus_on_mount {
                                mounted.update(cx, |state, _| state.focus_on_mount = false);
                                win.defer(cx, move |win, cx| {
                                    let state = mounted.read(cx);
                                    if state.open {
                                        state.focus.clone().focus(win, cx);
                                    }
                                });
                            }
                        },
                    )
                    .absolute()
                    .size_0(),
                )
            })
            .capture_key_down(cx.listener(move |this, ev: &KeyDownEvent, win, cx| {
                if this.open_menu != Some(Menu::ComposerProject) {
                    return;
                }
                let key = ev.keystroke.key.as_str();
                let index = nav.read(cx).cursor.min(count);
                let next = match key {
                    "home" => 0,
                    "end" => count,
                    "enter" | "space" => {
                        this.choose_home_project(index, current, win, cx);
                        win.prevent_default();
                        cx.stop_propagation();
                        return;
                    }
                    _ => return,
                };
                nav.update(cx, |state, _| {
                    state.cursor = next;
                    state.reveal(count);
                });
                win.prevent_default();
                cx.stop_propagation();
                cx.notify();
            }));
        self.render_menu(
            Menu::ComposerProject,
            MenuPlacement::BelowStart,
            232.,
            vec![list.into_any_element()],
            s,
            cx,
        )
    }

    fn choose_home_project(
        &mut self,
        index: usize,
        current: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_menu = None;
        self.composer_editor
            .read(cx)
            .focus
            .clone()
            .focus(window, cx);
        if let Some(project) = self.project_registry.projects.get(index) {
            if Some(index) != current {
                self.select_project_and_new_chat(project.root_path.clone(), window, cx);
            }
        } else {
            self.pick_workspace(window, cx);
        }
        cx.notify();
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
                    .h(px(ROW_H))
                    .pl(px(10.))
                    .pr(px(6.))
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
                                    .px(px(6.))
                                    .py(px(1.5))
                                    .rounded(px(R_PILL))
                                    .bg(if finished {
                                        s.pro.green_soft
                                    } else {
                                        s.pro.accent_soft
                                    })
                                    .font_features(tnum_features())
                                    .text_size(s.font_size(FS_TAG))
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
                                    .text_size(s.font_size(FS_META))
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
                                soft_button_sm(
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
                                icon_button_sm(
                                    ("bg-cmd-dismiss", ix),
                                    ProIcon::Close,
                                    self.ui_locale.pick("收起后台命令条", "Dismiss background command"),
                                    s,
                                )
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

    fn render_queued_inputs_strip(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let can_steer = self.projected.can_steer;
        let loc = self.ui_locale;
        let (edit_label, edit_tip) = if self.access_mode.is_remote_paired() {
            (
                loc.pick("复制到草稿", "Copy to draft"),
                loc.pick(
                    "原消息保留在主机待发队列中",
                    "The original remains queued on the host",
                ),
            )
        } else {
            (
                loc.pick("编辑", "Edit"),
                loc.pick("取回到输入框继续修改", "Return to the composer to edit"),
            )
        };
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
            let can_withdraw =
                self.access_mode.is_writable() && item.status == kar::chat::InputStatus::Pending;
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
                                    .text_size(s.font_size(FS_TAG))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if can_steer {
                                        s.pro.accent_info
                                    } else {
                                        s.muted
                                    })
                                    .child(if self.access_mode.is_remote_paired() {
                                        if item.status == kar::chat::InputStatus::Sending {
                                            loc.pick("发送中", "Sending")
                                        } else {
                                            loc.pick("待发", "Queued")
                                        }
                                    } else if can_steer {
                                        loc.pick("平滑汇入", "Steering")
                                    } else {
                                        loc.pick("待发队列", "Queued")
                                    }),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(s.font_size(FS_META))
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
                                text_button(("edit-queued", ix), edit_label, s)
                                    .disabled(!can_withdraw)
                                    .tip(edit_tip)
                                    .on_press(cx, move |this, window, cx| {
                                        this.withdraw_or_edit_queued_input(
                                            input_id,
                                            true,
                                            window,
                                            cx,
                                        );
                                    }),
                            )
                            .child(
                                text_button(("withdraw-queued", ix), self.ui_locale.pick("撤回", "Withdraw"), s)
                                    .disabled(!can_withdraw)
                                    .on_press(cx, move |this, window, cx| {
                                        this.withdraw_or_edit_queued_input(
                                            input_id,
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
                                    .text_size(s.font_size(FS_UI))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(doc),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .whitespace_nowrap()
                                    .text_size(s.font_size(FS_UI))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(format!("· {place}")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(s.font_size(FS_META))
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
        let mut rows: Vec<AnyElement> = vec![
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
        if self.chat_is_open() {
            rows.push(
                menu_icon_item(
                    "cmp-current-permissions",
                    ProIcon::Shield,
                    loc.pick("本次对话权限", "Chat permissions"),
                    s,
                )
                .when_some(self.projected.info.as_ref(), |r, info| {
                    r.child(menu_hint(loc.approval_label(info.approvals), s))
                })
                .on_press(cx, |wb, _, cx| wb.toggle_menu(Menu::ComposerApproval, cx))
                .into_any_element(),
            );
        }
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
                .take(6)
                .enumerate()
            {
                let (zh_title, _) = crate::studio_hub::localized_skill_presentation(
                    &sk.name,
                    &sk.description,
                    self.ui_locale,
                );
                let token = format!("${} ", sk.name);
                let id_hint = format!("${}", sk.name);
                rows.push(
                    menu_item(
                        SharedString::from(format!("cmp-sk-{ix}")),
                        Some(render_skills_puzzle_icon(ICON_SM, s.muted)),
                        zh_title,
                        s,
                    )
                    .child(menu_hint(id_hint, s))
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
            menu_icon_item("cmp-open-slash", ProIcon::Bolt,
                self.ui_locale.pick("浏览对话指令…", "Browse chat commands…"), s)
                .on_press(cx, |this, win, cx| this.open_slash_picker(win, cx))
                .into_any_element(),
        );
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
        let loc = self.ui_locale;
        let open = self.chat_is_open();
        let active_mode = self.effective_chat_approvals();
        let mut rows: Vec<AnyElement> = vec![
            menu_caption(
                if open {
                    loc.pick("当前对话的执行权限（点击即刻切换）", "Permission policy for this chat")
                } else {
                    loc.pick("新对话默认执行权限", "Default permission policy")
                },
                s,
            )
            .into_any_element(),
        ];

        for (ix, candidate) in crate::prefs::all_approval_modes().into_iter().enumerate() {
            let selected = candidate == active_mode;
            let is_allow = crate::prefs::is_approval_allow(candidate);
            let is_deny = candidate == ApprovalMode::Deny;
            let (icon, icon_col, title, desc) = if is_allow {
                (
                    ProIcon::Bolt,
                    s.pro.accent_info,
                    loc.pick("全自动执行", "Auto-run"),
                    loc.pick(
                        "自动批准运行命令与修改文件，适合连续自治任务",
                        "Approve tool & command execution automatically",
                    ),
                )
            } else if is_deny {
                (
                    ProIcon::Lock,
                    s.pro.amber_ink,
                    loc.pick("仅只读保护", "Read-only"),
                    loc.pick(
                        "仅允许阅读与分析代码，自动拒绝高权限写入操作",
                        "Allow reading & analysis, decline write permissions",
                    ),
                )
            } else {
                (
                    ProIcon::Shield,
                    s.ink,
                    loc.pick("每次先问我（推荐）", "Ask first (Recommended)"),
                    loc.pick(
                        "涉及执行命令或敏感改动前，在对话中由你确认",
                        "Review and approve sensitive actions inline",
                    ),
                )
            };

            rows.push(
                Tactile::ghost(("approval-opt", ix), s)
                    .w_full()
                    .px(px(10.))
                    .py(px(7.))
                    .radius(R_SM)
                    .lit(selected)
                    .items_start()
                    .gap(px(9.))
                    .child(div().pt(px(2.)).flex_none().child(pro_icon(icon, ICON_SM, icon_col)))
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
                                    .justify_between()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(s.font_size(FS_CONTROL))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(s.ink)
                                            .child(title),
                                    )
                                    .when(selected, |d| d.child(menu_check(s))),
                            )
                            .child(
                                div()
                                    .text_size(s.font_size(FS_META))
                                    .line_height(s.line_height(LH_META))
                                    .text_color(s.muted)
                                    .child(desc),
                            ),
                    )
                    .on_press(cx, move |wb, _, cx| {
                        wb.set_active_or_new_chat_approvals(candidate, cx);
                        wb.close_menu(cx);
                    })
                    .into_any_element(),
            );
        }

        if open
            && let Some(native_mode) = self
                .projected
                .info
                .as_ref()
                .and_then(|i| i.settings.acp_mode.as_deref())
        {
            rows.push(menu_divider(s).into_any_element());
            rows.push(
                div()
                    .px(px(10.))
                    .py(px(4.))
                    .text_size(s.font_size(FS_TAG))
                    .text_color(s.muted)
                    .child(format!(
                        "{} · {native_mode}",
                        loc.pick("智能体协作模式", "Agent mode")
                    ))
                    .into_any_element(),
            );
        }
        rows.push(menu_divider(s).into_any_element());
        rows.push(
            menu_icon_item(
                "approval-new-chat-settings",
                ProIcon::Sliders,
                loc.pick("设置所有新对话的默认权限…", "Configure global defaults…"),
                s,
            )
            .on_press(cx, |wb, _, cx| wb.open_settings(SettingsTab::Agents, cx))
            .into_any_element(),
        );
        self.render_menu(Menu::ComposerApproval, placement, 296., rows, s, cx)
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
                        this.save_agent_choice(cx);
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
                } else if view.editor.read(cx).focus.is_focused(window) {
                    // Window-level Escape also closes popovers before the focused field sees the event.
                    self.composer_editor
                        .read(cx)
                        .focus
                        .clone()
                        .focus(window, cx);
                }
            });
        }
        let runtime = search.read(cx).provider.clone();
        let provider = self.providers.iter().find(|p| p.id == runtime);
        let options = picker_models(provider, &search.read(cx).query, loc);
        let cursor = search.read(cx).cursor.min(options.len().saturating_sub(1));
        let preview = options.get(cursor).map(|(model, _)| model);
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
                            view.query.clear();
                            let provider = this.providers.iter().find(|p| p.id == id);
                            view.cursor = picker_models(provider, "", this.ui_locale)
                                .iter()
                                .position(|(model, _)| {
                                    id == this.selected_runtime && model == &this.selected_model
                                })
                                .unwrap_or(0);
                            view.scroll.scroll_to_item(view.cursor);
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
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .px(px(6.))
            .py(px(MODEL_LIST_PAD));
        for (index, (model, label)) in options.iter().enumerate() {
            let chosen = runtime == self.selected_runtime && model == &self.selected_model;
            let model = model.clone();
            let runtime = runtime.clone();
            let search = search.clone();
            list = list.child(
                div()
                    .id(("cmp-model-opt", index))
                    .role(Role::ListBoxOption)
                    .aria_selected(chosen)
                    .aria_label(label.clone())
                    .w_full()
                    .h(px(MODEL_ROW_H))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .cursor_pointer()
                    .text_color(if cursor == index || chosen {
                        s.ink
                    } else {
                        s.text
                    })
                    .child(render_runtime_mark(&runtime, 15., s, cx))
                    .child(div().min_w_0().flex_1().truncate().child(label.clone()))
                    .when(chosen, |row| {
                        row.child(pro_icon(ProIcon::Check, 13., s.text))
                    })
                    .on_mouse_move(move |_, _, cx| {
                        if search.read(cx).cursor != index {
                            search.update(cx, |view, cx| {
                                view.cursor = index;
                                cx.notify();
                            });
                        }
                    })
                    .on_click(cx.listener(move |this, _, win, cx| {
                        let effort = (this.selected_runtime == runtime)
                            .then(|| this.reasoning_effort.clone())
                            .flatten();
                        this.apply_model_picker(&runtime, model.clone(), effort, win, cx);
                    })),
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
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .when(open && preview.is_some(), |d| {
                        d.child(model_cursor_band(cursor, &scroll, s.pro.pink, window, cx))
                    })
                    .child(list)
                    .child(model_scroll_fade(scroll, s.card)),
            );
        let model = preview.cloned().flatten();
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
        // Keep the list in place when neighboring models offer different reasoning controls.
        if provider.is_some_and(|p| {
            ready
                && crate::types::runtime_supports_effort(&p.id)
                && p.models.iter().any(|model| !model.effort_levels.is_empty())
        }) {
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
                        .text_size(s.font_size(FS_META))
                        .child(loc.pick("思考深度", "Reasoning")),
                );
            for (i, level) in preview
                .map(|_| None)
                .into_iter()
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
                        .disabled(levels.is_empty())
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
                div()
                    .pr(px(5.))
                    .text_size(s.font_size(FS_META))
                    .text_color(s.muted)
                    .child(loc.pick("↑↓ 浏览   ↵ 选择", "↑↓ Browse   ↵ Select")),
            );
        let s = *s;
        pro_presence(Menu::ComposerAgent.id(), open, 0.16, 0.12, move |amount| {
            let surface = crate::ui_primitives::frosted_panel(&s)
                .id("composer-model-picker")
                .role(Role::Dialog)
                .occlude()
                .w(px(width))
                .h(px(height))
                .overflow_hidden()
                .flex()
                .flex_col()
                .opacity(amount)
                .text_size(s.font_size(FS_CONTROL))
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
        let Some(provider) = self
            .providers
            .iter()
            .find(|p| p.id == runtime && p.is_ready())
        else {
            return;
        };
        if model
            .as_ref()
            .is_some_and(|id| !provider.models.iter().any(|m| &m.id == id))
        {
            return;
        }
        self.select_runtime_by_id(runtime, cx);
        self.selected_model = model;
        self.reasoning_effort = effort.filter(|level| self.offered_efforts().contains(level));
        self.save_agent_choice(cx);
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
        let mut asked = div().w_full().flex().flex_col().items_end().gap(px(8.));
        if let Some(target) = queued.target.clone() {
            asked = asked.child(self.render_target_reference("queued", target, s, cx));
        }
        asked.child(user_bubble("queued", &queued.text, None, self.ui_locale, s, window, cx))
    }

    fn render_composer_activity(
        &self,
        activity: composer_activity::Activity,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use composer_activity::Activity;
        let loc = self.ui_locale;
        let (pose, label) = match activity {
            Activity::Starting => {
                let agent = runtime_display_name(&self.selected_runtime);
                let label = match loc {
                    UiLocale::Zh => format!("正在启动 {agent}"),
                    UiLocale::En => format!("Starting {agent}"),
                };
                (MascotStatus::Listening, label)
            }
            Activity::Thinking => (MascotStatus::Thinking, loc.pick("正在思考", "Thinking").into()),
            Activity::Working => (MascotStatus::Thinking, loc.pick("正在处理", "Working").into()),
            Activity::Replying => (MascotStatus::Writing, loc.pick("正在回复", "Replying").into()),
        };
        // Keep the animated views as children so their frame notifications stay local.
        let mascot = window
            .use_keyed_state("composer-assistant-mascot", cx, |_, cx| {
                cx.new(|_| {
                    AiMascot::new(loc.pick("助手", "Assistant"))
                        .with_size(24.)
                        .with_status(pose)
                        .follow_pointer(false)
                })
            })
            .read(cx)
            .clone();
        if mascot.read(cx).status() != pose {
            mascot.update(cx, |view, cx| view.set_status(pose, cx));
        }
        let shimmer = activity::shimmer(
            "composer-activity-label".into(),
            label.clone().into(),
            FS_CONTROL,
            window,
            cx,
        );
        div()
            .id("composer-activity")
            .role(Role::Status)
            .aria_label(label)
            .h(px(24.))
            .pl(px(6.))
            .mb(px(6.))
            .flex()
            .items_center()
            .gap(px(7.))
            .text_size(s.font_size(FS_CONTROL))
            .text_color(s.muted)
            .child(mascot)
            .child(shimmer)
            .children(self.projected.turns.last().filter(|turn| {
                turn.status == TurnStatus::Running && turn.started_ms > 0 &&
                composer_activity::selected_projection(&self.access_mode, &self.projected)
            }).map(|turn| message_time::live_clock(turn.id, turn.started_ms, window, cx)))
            .into_any_element()
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
                    .text_size(s.font_size(FS_UI))
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
        let mut tasks = self.projected.tasks.iter().filter(|task| !task.status.is_terminal()).peekable();
        tasks.peek()?;
        let loc = self.ui_locale;
        let can_stop = self.projected.can_stop_task
            && matches!(self.access_mode, ChatAccessMode::WriteController { .. });
        let mut list = div().w_full().min_w_0().pl(px(4.)).flex().flex_col().items_start().gap(px(4.));
        for task in tasks {
            let id = task.id.clone();
            let group: SharedString = format!("live-task-{id}").into();
            let title = task.summary.as_deref().or(task.details.description.as_deref())
                .unwrap_or(loc.pick("后台任务", "Background task"));
            let label = match task.status {
                TaskStatus::Paused => format!("{} · {title}", loc.pick("已暂停", "Paused")),
                TaskStatus::Pending => format!("{} · {title}", loc.pick("等待中", "Pending")),
                _ => title.to_owned(),
            };
            let running = task.status == TaskStatus::Running;
            let mark = match task.status {
                TaskStatus::Paused => pro_icon(ProIcon::Pause, ICON_SM, s.muted),
                TaskStatus::Pending => pro_icon(ProIcon::Clock, ICON_SM, s.muted),
                _ => spinner(ICON_SM, s.muted, window, cx),
            };
            let name = if running {
                activity::shimmer(format!("{group}-shimmer").into(), label.into(), FS_UI, window, cx)
                    .into_any_element()
            } else {
                div().truncate().text_size(s.font_size(FS_UI)).text_color(s.muted).child(label)
                    .into_any_element()
            };
            let stop = loc.pick("停止", "Stop");
            let row = div().group(group.clone()).max_w_full().min_w_0().flex().items_center().gap(px(8.))
                .child(mark)
                .child(div().min_w_0().overflow_hidden().child(name))
                .when(can_stop, |row| row.child(
                    div().flex_none().when(!window.last_input_was_keyboard() && !window.last_input_was_touch(), |d| {
                        d.opacity(0.).group_hover(group.clone(), |style| style.opacity(1.))
                    }).child(Tactile::inline(SharedString::from(format!("{group}-stop")), s)
                        .aria(stop).h(px(CTRL_SM_H)).px(px(5.)).text_size(s.font_size(FS_META))
                        .ink(s.muted, s.ink).child(stop)
                        .on_press(cx, move |this, _, _| {
                            this.bridge.send(KarCommand::StopTask { task_id: id.clone() });
                        }))
                ));
            list = list.child(arrive(row, group, 0, 3., cx));
        }
        Some(list.into_any_element())
    }

    fn render_turn(
        &self,
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
                        .text_size(s.font_size(FS_META))
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
        let chat_id = self.projected.chat_id;
        for input in user_inputs(turn) {
            let capture =
                input.id.and_then(|id| self.client_reducer.turn_captures.get(&id)).filter(|c| {
                    chat_id.is_some() && c.chat_id == chat_id && Some(c.turn_id) == input.id
                });
            let clean = input
                .id
                .and_then(|id| self.prompt_attempts.get(&id))
                .filter(|attempt| chat_id.is_some() && attempt.chat_id == chat_id)
                .map(|attempt| attempt.raw_user_text.as_str())
                .unwrap_or_else(|| {
                    let target = capture
                        .and_then(|c| c.target_selection.as_ref())
                        .or_else(|| input.id.and_then(|id| self.turn_targets.get(&id)));
                    let attachments = if input.primary {
                        turn.attachments.as_slice()
                    } else {
                        input
                            .id
                            .and_then(|id| turn.steered_attachments.get(&id))
                            .map(Vec::as_slice)
                            .unwrap_or_default()
                    };
                    original_user_text(input.text, target, attachments)
                });
            if clean.trim().is_empty() {
                continue;
            }
            if first_ask.is_empty() {
                first_ask = clean.to_owned();
            }
            let sent_at = if input.primary {
                Some(turn.pi_input_timestamp_ms.filter(|ms| *ms != 0).unwrap_or(turn.started_ms))
            } else {
                capture.map(|c| c.captured_at_ms)
            };
            if input.primary
                && (capture.is_some_and(|c| c.approved_plan) || self.current_plan_was_approved(chat_id, turn_id))
                && let Some((_, proposal)) = clean.split_once("\n\n")
            {
                asked = asked.child(self.render_approved_plan_turn(turn_id, proposal, s, cx));
                continue;
            }
            asked = asked.child(user_bubble(
                &format!("{turn_id}-{}", input.index),
                clean,
                sent_at,
                self.ui_locale,
                s,
                window,
                cx,
            ));
        }

        // ── What the agent did (`Worked for Xs >` + Prose/Tables + Copy + Changed Files) ─
        let loc = self.ui_locale;
        let ended = turn.finished_ms.filter(|&end| !running && end >= turn.started_ms);
        let finish_time = ended.and_then(message_time::local);
        let duration = ended.filter(|_| turn.started_ms > 0).map(|end| {
            worked_duration(end - turn.started_ms, loc)
        });
        let mut parts = div()
            .group(action_group.clone())
            .w_full()
            .min_w_0()
            .pl(px(4.))
            .flex()
            .flex_col()
            .items_start()
            .gap(px(14.));
        let mut has_parts = false;
        let mut touched: Vec<(String, &'static str)> = Vec::new();

        let root = self.projected.info.as_ref().map(|info| info.native_id.as_str());
        let turn_steps = turn_steps::TurnSteps::new(turn, root);
        for entry in &turn.items {
            if let Item::Tool { activity, .. } = &entry.item {
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
        }
        let work_grp_key = (turn_id, 900_000);
        let work_grp_open = running || self.part_is_open(
            work_grp_key,
            turn.status != TurnStatus::Completed || !turn_steps.has_answer(),
        );
        let work_fold = Fold::get(
            SharedString::from(format!("turn-steps-body-{turn_id}")),
            work_grp_open,
            80.,
            0.20,
            window,
            cx,
        );
        let mut has_work = false;
        let mut work_rows = Vec::new();
        let mut reply_rows = Vec::new();

        let multiple_answer_parts = turn_steps.has_multiple_answers();
        let mut ix = 0;
        let mut branch_end = 0;
        let mut branch: Option<(Fold, Vec<AnyElement>, bool)> = None;
        loop {
            if ix == branch_end
                && let Some((fold, rows, is_step)) = branch.take()
                && fold.mounted()
            {
                let content = div()
                    .w_full()
                    .min_w_0()
                    .pl(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(rows);
                let lane = if is_step { &mut work_rows } else { &mut reply_rows };
                lane.push(fold.wrap(0., content).into_any_element());
            }
            let Some(entry) = turn.items.get(ix) else { break };
            let item = &entry.item;
            let source = &entry.source;
            let is_subagent = turn_steps.is_child(turn, ix);
            if (matches!(item, Item::Message { role: KarRole::User, .. }) && !is_subagent)
                || matches!(item, Item::Message { text, .. } if text.trim().is_empty())
            {
                ix += 1;
                continue;
            }
            let is_step = turn_steps.is_work(ix, running);
            if is_step {
                has_work = true;
                if !work_fold.mounted() {
                    ix += 1;
                    continue;
                }
            }
            let rows = if is_step { &mut work_rows } else { &mut reply_rows };
            if is_subagent && ix >= branch_end {
                branch_end = ix + turn.items[ix..].iter().take_while(|e| e.source == *source).count();
                let path = turn_steps.branch_path(turn, source);
                let (header, fold) = activity::branch_header(turn_id, ix, &path, loc, s, window, cx);
                rows.push(header);
                branch = Some((fold, Vec::new(), is_step));
            }
            let rows = if let Some((fold, children, _)) = branch.as_mut() {
                if !fold.mounted() {
                    ix = branch_end;
                    continue;
                }
                children
            } else {
                rows
            };
            match item {
                Item::Message { role: role @ (KarRole::User | KarRole::Reasoning), text, complete, .. } => {
                    let (icon, label) = match role {
                        KarRole::User => (ProIcon::Users, loc.pick("子任务指令", "Delegated task")),
                        _ => (ProIcon::Sparkles, loc.pick("思考过程", "Reasoning")),
                    };
                    let (label, body) = if *role == KarRole::Reasoning {
                        activity::reasoning_summary(text).unwrap_or((label, text.trim()))
                    } else {
                        (label, text.trim())
                    };
                    rows.push(self.render_step_text((turn_id, ix), body, icon, label, running && !*complete, s, window, cx)
                        .into_any_element());
                }
                Item::Message {
                    role: KarRole::Assistant,
                    text,
                    complete,
                    ..
                } => {
                    let msg_group: SharedString = format!("reply-actions-{turn_id}-{ix}").into();
                    let mut show_message_copy =
                        *complete && (is_step || running || multiple_answer_parts);
                    if show_message_copy && might_start_with_code(text) {
                        let shape = window.use_keyed_state(
                            SharedString::from(format!("reply-shape-{turn_id}-{ix}")),
                            cx,
                            |_, _| (text.clone(), is_single_code_message(text)),
                        );
                        if &shape.read(cx).0 != text {
                            shape.update(cx, |shape, _| {
                                *shape = (text.clone(), is_single_code_message(text))
                            });
                        }
                        show_message_copy = !shape.read(cx).1;
                    }
                    let message = div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(self.render_assistant_markdown_blocks(
                            turn_id,
                            ix,
                            text,
                            !*complete && running,
                            s,
                            cx,
                        ));
                    rows.push(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_start()
                            .group(msg_group.clone())
                            .child(message)
                            .when(show_message_copy, |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .ml(px(4.))
                                        .when(
                                            !window.last_input_was_keyboard()
                                                && !window.last_input_was_touch(),
                                            |d| {
                                                d.opacity(0.).group_hover(msg_group, |hover| {
                                                    hover.opacity(1.)
                                                })
                                            },
                                        )
                                        .child(copy_text_button(
                                            format!("message-copy-{turn_id}-{ix}"),
                                            loc.pick("复制这条消息", "Copy this message"),
                                            text.clone().into(),
                                            window,
                                            cx,
                                        )),
                                )
                            })
                            .into_any_element(),
                    );
                }
                Item::Tool { .. } => {
                    let source = &turn.items[ix].source;
                    let mut calls: Vec<(usize, &Tool)> = Vec::new();
                    let mut next_ix = ix;
                    while let Some(Item::Tool { activity, .. }) = turn
                        .items
                        .get(next_ix)
                        .filter(|item| &item.source == source)
                        .map(|item| &item.item)
                    {
                        calls.push((next_ix, activity));
                        next_ix += 1;
                    }
                    rows.push(
                        self.render_tool_run(turn_id, &calls, running, s, window, cx)
                            .into_any_element(),
                    );
                    ix = next_ix - 1;
                }
                Item::Retry { status } => {
                    let label = loc.pick("自动重试", "Automatic retry");
                    let note = match (status.attempt, status.max_attempts) {
                        (Some(a), Some(m)) => format!("{label} {a}/{m}"),
                        (Some(a), None) => format!("{label} {a}"),
                        _ => label.to_owned(),
                    };
                    let full = match status.reason.as_deref().filter(|r| !r.trim().is_empty()) {
                        Some(r) => format!("{note} · {r}"),
                        None => note,
                    };
                    rows.push(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(s.font_size(FS_META))
                            .text_color(s.muted)
                            .child(pro_icon(ProIcon::Refresh, ICON_XS, s.muted))
                            .child(div().min_w_0().truncate().child(full))
                            .into_any_element(),
                    );
                }
                Item::Compaction { report } => {
                    let label = match report.state.as_str() {
                        "started" => loc.pick("开始压缩上下文", "Started context compaction"),
                        "completed" => loc.pick("上下文已压缩", "Context compacted"),
                        "failed" => loc.pick("上下文压缩失败", "Context compaction failed"),
                        "aborted" => loc.pick("上下文压缩已停止", "Context compaction stopped"),
                        _ => loc.pick("上下文压缩", "Context compaction"),
                    };
                    rows.push(
                        div()
                            .w_full()
                            .min_h(px(26.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(s.font_size(FS_CONTROL))
                            .text_color(s.muted)
                            .child(pro_icon(ProIcon::Note, ICON_SM, s.muted))
                            .child(label)
                            .into_any_element(),
                    );
                }
                Item::Diff { diff, .. } => {
                    rows.push(
                        self.render_diff((turn_id, ix), diff, s, cx)
                            .into_any_element(),
                    );
                }
                Item::RequestError { error } => {
                    let is_unconfirmed = error.status == kar::RequestDeliveryStatus::Unconfirmed;
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
                    rows.push(
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
                                            .text_size(s.font_size(FS_META))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.ink)
                                            .child(badge),
                                    )
                                    .when(!error.reason.trim().is_empty(), |d| {
                                        d.child(
                                            div()
                                                .text_size(s.font_size(FS_META))
                                                .text_color(s.text)
                                                .child(error.reason.trim().to_owned()),
                                        )
                                    }),
                            )
                            .into_any_element(),
                    );
                }
            }
            ix += 1;
        }

        if has_work {
            has_parts = true;
            let mut process = div().w_full().min_w_0().flex().flex_col().items_start();
            if !running {
                let label = duration.unwrap_or_else(|| loc.pick("执行过程", "Activity").into());
                let toggle = Tactile::inline(SharedString::from(format!("worked-grp-{turn_id}")), s)
                    .aria(format!("{label} · {}", loc.pick("执行过程", "Activity")))
                    .expanded(work_grp_open)
                    .ml(px(-4.))
                    .px(px(4.))
                    .h(px(CTRL_SM_H))
                    .gap(px(5.))
                    .font_features(tnum_features())
                    .text_size(s.font_size(FS_UI))
                    .ink(s.muted, s.ink)
                    .child(label)
                    .child(activity::chevron(
                        format!("turn-chevron-{turn_id}").into(), work_grp_open, s.muted, window, cx,
                    ))
                    .on_press(cx, move |this, _, cx| {
                        this.part_open.insert(work_grp_key, !work_grp_open);
                        cx.notify();
                    });
                process = process.child(toggle);
            }
            if work_fold.mounted() {
                let content = div().w_full().min_w_0()
                    .when(!running, |d| d.pt(px(8.)))
                    .flex().flex_col().gap(px(10.)).children(work_rows);
                process = process.child(work_fold.wrap(0., content));
            }
            parts = parts.child(process);
        } else if let Some(label) = duration {
            has_parts = true;
            parts = parts.child(
                div()
                    .min_h(px(CTRL_SM_H))
                    .flex()
                    .items_center()
                    .font_features(tnum_features())
                    .text_size(s.font_size(FS_UI))
                    .text_color(s.muted)
                    .child(label),
            );
        }
        has_parts |= !reply_rows.is_empty();
        parts = parts.children(reply_rows);

        // ── Gather turn changed files ───────────────────────────────────────
        let checkpoint = turn.checkpoint.as_ref();
        let local = matches!(
            self.access_mode,
            ChatAccessMode::WriteController { .. }
                | ChatAccessMode::LocalReadOnlySubscriber { .. }
                | ChatAccessMode::ArchivedReadOnly { .. }
        );
        let has_after_tree = checkpoint.is_some_and(|cp| cp.after_tree.is_some());
        let aster_session = self.projected.info.as_ref().is_some_and(|info| crate::aster::is_aster(&info.runtime));
        let runtime_file = |path: &Path| aster_session && crate::aster::runtime_artifact(path);
        let has_checkpoint_changes = checkpoint.is_some_and(|cp| cp.files.iter().any(|c| !runtime_file(&c.path)));
        let reviewable = local && has_after_tree;
        let mut files = Vec::new();
        if let Some(cp) = checkpoint {
            for change in &cp.files {
                if runtime_file(&change.path) { continue; }
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
            if !runtime_file(&path) && !files.iter().any(|(seen, _, _)| seen == &path) {
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
                            .text_size(s.font_size(FS_UI))
                            .line_height(s.line_height(LH_UI))
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(copy.title),
                            )
                            .when(!copy.detail.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_size(s.font_size(FS_META))
                                        .line_height(s.line_height(LH_META))
                                        .text_color(s.text)
                                        .child(copy.detail),
                                )
                            })
                            .when_some(copy.hint, |d, hint| {
                                d.child(
                                    div()
                                        .text_size(s.font_size(FS_META))
                                        .text_color(s.muted)
                                        .child(hint),
                                )
                            }),
                    ),
            );
        }

        // ── Actions for the completed turn ──────────────────────────────────────
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

        let can_rewind_context = !running && idle && writable_real;
        let show_hold_rollback = (can_rollback_after && has_checkpoint_changes) || can_undo_turn;
        let show_extra_actions = show_hold_rollback || can_rewind_context;
        let extra_actions_key = (turn_id, usize::MAX);
        let extra_actions_open = self.part_is_open(extra_actions_key, false);
        let rewind_context_btn = current_cid
            .filter(|_| can_rewind_context && extra_actions_open)
            .map(|chat_id| {
                Tactile::soft(format!("rewind-ctx-{turn_id}"), s)
                    .h(px(CTRL_SM_H))
                    .px(px(10.))
                    .radius(R_CTRL)
                    .gap(px(5.))
                    .text_size(s.font_size(FS_META))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.ink)
                    .tip(loc.pick(
                        "仅撤回此轮之后的对话上下文，完整保留磁盘代码改动",
                        "Rewind conversation history only and keep all workspace file edits",
                    ))
                    .child(pro_icon(ProIcon::Undo, ICON_XS, s.muted))
                    .child(loc.pick("仅撤回对话", "Rewind context"))
                    .on_press(cx, move |this, _, cx| {
                        this.part_open.insert(extra_actions_key, false);
                        this.confirm_or_run(
                            format!("rewind-ctx-{turn_id}"),
                            true,
                            loc.pick(
                                "再次点击“仅撤回对话”以确认撤回对话记录（文件改动将完整保留）",
                                "Click again to rewind conversation history (workspace files stay intact)",
                            ),
                            cx,
                            move |wb, _| {
                                wb.bridge.send(KarCommand::RewindChat {
                                    chat_id,
                                    turn_id,
                                    restore_files: false,
                                });
                            },
                        );
                    })
                    .into_any_element()
            });
        let rollback_hold = current_cid.filter(|_| show_hold_rollback && extra_actions_open).map(|chat_id| {
            let before = can_undo_turn && !(can_rollback_after && has_checkpoint_changes);
            let tip = if before {
                loc.pick("按住约一秒，将文件恢复到本轮开始之前", "Hold for one second to restore files from before this turn")
            } else {
                loc.pick("按住约一秒，将文件恢复到本轮结束时", "Hold for one second to restore files to the end of this turn")
            };
            let label = if before { loc.pick("按住撤销改动", "Hold to undo") }
                        else { loc.pick("按住恢复文件", "Hold to restore") };
            let hold = self.rollback_hold_buttons.borrow_mut().entry((chat_id, turn_id, before)).or_insert_with(|| {
                let btn = cx.new(|cx| HoldToConfirm::new(format!("hold-rb-{chat_id}-{turn_id}"), label, window, cx)
                    .with_duration(Duration::from_millis(950))
                    .with_done_label(loc.pick("已确认", "Confirmed"))
                    .with_icon(Glyph::Undo)
                    .compact()
                    .danger());
                cx.subscribe(&btn, move |this, btn, event: &HoldEvent, cx| {
                    if *event == HoldEvent::Confirmed {
                        if matches!(this.access_mode, ChatAccessMode::WriteController { chat_id: id } if id == chat_id)
                            && !this.turn_in_progress() && this.part_is_open(extra_actions_key, false) {
                            this.bridge.send(KarCommand::RollbackToTurn { chat_id, turn_id, before });
                        }
                        btn.update(cx, |b, cx| b.reset(cx));
                    }
                }).detach();
                btn
            }).clone();
            div().id(SharedString::from(format!("turn-restore-tip-{turn_id}")))
                .aria_description(tip)
                .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
                .child(hold)
        });

        if finish_time.is_some() || copy_button.is_some() || stopped.is_some() || can_retry || can_fork || show_extra_actions
        {
            has_parts = true;
            parts = parts.child(
                div()
                    .w_full()
                    .min_w_0()
                    .min_h(px(CTRL_SM_H))
                    .mt(px(4.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(4.))
                    .when(
                        !window.last_input_was_keyboard()
                            && !window.last_input_was_touch()
                            && stopped.is_none()
                            && !can_retry
                            && !extra_actions_open,
                        |row| {
                            row.opacity(0.)
                                .group_hover(action_group.clone(), |hover| hover.opacity(1.))
                        },
                    )
                    .when_some(stopped, |row, note| {
                        row.child(
                            div()
                                .px(px(4.))
                                .text_size(s.font_size(FS_META))
                                .text_color(s.muted)
                                .child(note),
                        )
                    })
                    .when(can_retry, |row| {
                        row.child(
                            text_button_sm(format!("retry-{turn_id}"), loc.pick("重试", "Retry"), s)
                                .on_press(cx, move |this, _, cx| {
                                    this.retry_turn_with_same_uuid(turn_id, first_ask.clone(), cx)
                                }),
                        )
                    })
                    .when_some(copy_button, |row, button| {
                        row.child(copy_with_hint(
                            format!("reply-copy-tip-{turn_id}"),
                            loc.pick("复制本轮回复", "Copy entire response"),
                            button,
                        ))
                    })
                    .when_some(can_fork.then_some(current_cid).flatten(), |row, chat_id| {
                        row.child(
                            icon_button_sm(
                                format!("fork-{turn_id}"),
                                ProIcon::GitBranch,
                                loc.pick("从此处分叉新对话", "Fork thread from here"),
                                s,
                            )
                            .on_press(cx, move |this, _, _| {
                                this.bridge.send(KarCommand::ForkChatAt {
                                    chat_id,
                                    turn_id,
                                    model_override: None,
                                });
                            }),
                        )
                    })
                    .when(show_extra_actions, |row| {
                        row.child(
                            icon_button_sm(
                                format!("turn-actions-{turn_id}"),
                                ProIcon::MoreHorizontal,
                                loc.pick("更多本轮操作（撤回对话 / 恢复文件）", "More turn actions"),
                                s,
                            )
                            .lit(extra_actions_open)
                            .expanded(extra_actions_open)
                            .on_press(cx, move |this, _, cx| {
                                this.part_open
                                    .insert(extra_actions_key, !extra_actions_open);
                                cx.notify();
                            }),
                        )
                    })
                    .when_some(finish_time, |row, time| {
                        let agent = self.client_reducer.turn_captures.get(&turn_id)
                            .filter(|cap| cap.chat_id.is_some() && cap.chat_id == self.projected.chat_id)
                            .map(|cap| {
                                let name = runtime_display_name(&cap.runtime_id);
                                match cap.model.as_deref().filter(|model| !model.is_empty()) {
                                    Some(model) => format!("{name} · {model}"),
                                    None => name,
                                }
                            });
                        row.child(
                            message_time::label(
                                SharedString::from(format!("reply-time-{turn_id}")),
                                time,
                                s,
                                agent,
                            )
                            .mx(px(4.)),
                        )
                    })
                    .when_some(rewind_context_btn, |row, btn| row.child(btn))
                    .when_some(rollback_hold, |row, hold| row.child(hold)),
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
            .w_full()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(asked)
            .when(has_parts, |d| d.child(parts))
    }

    /// Keep the document intact so table, code and selection share one Markdown parse.
    pub(crate) fn render_assistant_markdown_blocks(
        &self,
        turn_id: Uuid,
        item_ix: usize,
        text: &str,
        streaming: bool,
        s: &StudioPalette,
        _cx: &mut Context<Self>,
    ) -> Div {
        div()
            .w_full()
            .min_w_0()
            .text_size(s.font_size(FS_READ))
            .line_height(s.line_height(LH_READ))
            .text_color(s.ink.opacity(0.94))
            .child(reply_stream::ReplyMarkdown {
                turn_id,
                item_ix,
                source: text.to_owned().into(),
                streaming,
                muted: s.muted,
                copy_label: self.ui_locale.pick("复制代码", "Copy code"),
            })
    }

    fn part_is_open(&self, key: (Uuid, usize), default_open: bool) -> bool {
        self.part_open.get(&key).copied().unwrap_or(default_open)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_step_text(
        &self,
        key: (Uuid, usize),
        text: &str,
        icon: ProIcon,
        label: &str,
        active: bool,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let default_open = active || text.chars().count() <= 360;
        let open = self.part_is_open(key, default_open);
        let body = div()
            .text_size(s.font_size(FS_CONTROL))
            .line_height(s.line_height(LH_UI))
            .text_color(s.muted)
            .child(text.to_owned())
            .into_any_element();
        let estimate = 28. + (text.chars().count() as f32 / 46.).ceil() * LH_UI;
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .items_start()
            .child(
                activity::activity_line(
                    SharedString::from(format!("think-{}-{}", key.0, key.1)),
                    icon,
                    label.to_owned(),
                    if active { activity::Phase::Running } else { activity::Phase::Ended },
                    open,
                    s,
                    window,
                    cx,
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

    fn render_diff(
        &self,
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
        let code_size = crate::ui_primitives::effective_code_font_size(cx);
        let line_height = (code_size * 1.5).round();

        let body_el: AnyElement = if open && !parsed_hunks.is_empty() {
            let cache_k = format!("turn-patch-{}-{}", key.0, key.1);
            let file_for_pr = file.clone();
            let pr = self
                .patch_review_cache
                .borrow_mut()
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
                .text_size(px(code_size))
                .line_height(px(line_height));
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
                        .font_features(tnum_features())
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
                (shown as f32 * line_height + 36.).max(120.),
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
                        .text_size(s.font_size(FS_UI))
                        .text_color(s.ink)
                        .child(name.clone()),
                )
                .child(
                    div()
                        .text_size(s.font_size(FS_META))
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
                            this.open_checkpoint_review(key.clone());
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

        frosted_card(s)
            .w_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                Tactile::ghost(
                    SharedString::from(format!("turn-files-fold-{turn}-button")),
                    s,
                )
                .h(px(ROW_H))
                .w_full()
                .px(px(14.))
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_features(tnum_features())
                        .text_size(s.font_size(FS_CONTROL))
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
                            .text_size(s.font_size(FS_UI))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child(name.clone()),
                    )
                    .child(div().text_size(s.font_size(FS_META)).text_color(s.muted).child(label))
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
                let code_size = crate::ui_primitives::effective_code_font_size(cx);
                let row_height = px((code_size * 1.6).round());
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
                                        .h(row_height)
                                        .line_height(row_height)
                                        .text_size(px(code_size))
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
                    .when(!in_rail, |d| d.h(row_height * count.min(10) as f32))
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
                        .text_size(s.font_size(FS_META))
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

    fn render_decision_card(
        &mut self,
        request: PendingRequest,
        total: usize,
        compact: bool,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let agent = runtime_display_name(&self.effective_runtime());
        let mono = mono_font(cx);
        let permission = request.kind == RequestKind::Permission;
        let heading = if permission {
            match loc {
                UiLocale::Zh => format!("{agent} 请求授权"),
                UiLocale::En => format!("{agent} requests approval"),
            }
        } else {
            format!("{} · {agent}", loc.pick("需要你的答复", "Input needed"))
        };

        let head = div()
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .text_size(s.font_size(FS_TAG))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(s.muted)
            .child(div().flex().items_center().gap(px(6.))
                .when(permission, |row| row.child(pro_icon(ProIcon::Shield, ICON_SM, s.pro.amber_ink)))
                .child(heading))
            .when(total > 1, |d| {
                d.child(format!(
                    "+{} {}",
                    total - 1,
                    loc.pick("项待处理", "pending")
                ))
            });

        let card = frosted_card(s)
            .w_full()
            .min_h_0()
            .p(px(if compact { 8. } else if permission { 12. } else { 16. }))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(head);

        if request.kind == RequestKind::Question {
            let content = self.render_question_fields(
                div().w_full().flex().flex_col().gap(px(8.)),
                request,
                s,
                window,
                cx,
            );
            return card.child(content);
        }

        let controls = request.controls();
        let reason = request.body["reason"]
            .as_str()
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(str::to_owned);
        let mut detail = div().w_full().flex().flex_col().gap(px(6.));
        if let Some(reason) = reason {
            detail = detail.child(decision_question(reason, s));
        }

        let mut facts = request_facts(&request, loc);
        if request.method == "item/permissions/requestApproval"
            && controls.resources.is_empty()
            && let Some(permissions) = request.body.get("permissions")
        {
            facts.push((loc.pick("权限", "Permissions"), permissions.to_string()));
        }
        for res in &controls.resources {
            let label = match res.id {
                kar::ResourceId::Read(_) => loc.pick("读取", "Read"),
                kar::ResourceId::Write(_) => loc.pick("写入", "Write"),
                kar::ResourceId::Network => loc.pick("网络", "Network"),
                kar::ResourceId::Entry(_) => loc.pick("文件权限", "File permission"),
            };
            facts.push((
                label,
                res.value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| res.value.to_string()),
            ));
        }
        if !facts.is_empty() {
            detail = detail.child(
                div()
                    .w_full()
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(R_CTRL))
                    .bg(s.soft)
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .children(facts.into_iter().map(|(label, value)| {
                        div()
                            .w_full()
                            .font_family(mono.clone())
                            .text_size(s.font_size(FS_META))
                            .text_color(s.muted)
                            .child(format!("{label}  {value}"))
                    })),
            );
        }

        let accepted = &controls.decisions;
        let first_of =
            |wanted: &[PermissionDecision]| wanted.iter().copied().find(|d| accepted.contains(d));
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

        let button_padding = if compact { 6. } else { 12. };
        let mut actions = div()
            .pt(px(2.))
            .w_full()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .justify_end()
            .gap(px(6.));
        for (i, choice) in accepted.iter().copied().enumerate() {
            if ![refuse, allow, allow_more].contains(&Some(choice)) {
                actions = actions.child(
                    text_button_sm(("decision-alternative", i), permission_label(choice, loc), s)
                        .h(px(CTRL_H))
                        .px(px(button_padding))
                        .on_press(cx, decide(choice)),
                );
            }
        }
        if let Some(decision) = refuse {
            actions = actions.child(
                Tactile::soft("decision-refuse", s)
                    .fills(s.pro.red_soft, s.pro.red.opacity(0.22))
                    .h(px(CTRL_H))
                    .px(px(button_padding))
                    .radius(R_CTRL)
                    .justify_center()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.pro.red_ink)
                    .child(permission_label(decision, loc))
                    .on_press(cx, decide(decision)),
            );
        }
        let mut policy_rows = div().w_full().flex().flex_wrap().gap(px(6.));
        for (pix, pol) in controls.policies.iter().enumerate() {
            let label = policy_option_label(pol, pix, loc);
            let choice = pol.choice;
            let req = request.clone();
            policy_rows = policy_rows.child(
                Tactile::soft(("decision-policy", pix), s)
                    .h(px(CTRL_H))
                    .px(px(button_padding))
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
                    .h(px(CTRL_H))
                    .px(px(button_padding))
                    .radius(R_CTRL)
                    .justify_center()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.ink)
                    .child(match decision {
                        PermissionDecision::AllowAlways => loc.pick("总是允许", "Allow always"),
                        _ => loc.pick("本次会话允许", "Allow session"),
                    })
                    .on_press(cx, decide(decision)),
            );
        }
        if let Some(decision) = allow {
            actions = actions.child(
                Tactile::primary("decision-allow", s)
                    .fills(s.primary, s.primary.opacity(0.9))
                    .h(px(CTRL_H))
                    .px(px(button_padding + 2.))
                    .radius(R_CTRL)
                    .justify_center()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(s.on_primary)
                    .child(permission_label(decision, loc))
                    .on_press(cx, decide(decision)),
            );
        }

        card.child(
            div().id("approval-details").min_h(px(24.)).overflow_y_scroll()
                .child(detail.when(!controls.policies.is_empty(), |d| d.child(policy_rows)))
        ).child(actions)
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

fn permission_label(decision: PermissionDecision, loc: UiLocale) -> &'static str {
    match decision {
        PermissionDecision::AllowOnce => loc.pick("允许一次", "Allow once"),
        PermissionDecision::AllowForTurn => loc.pick("本轮允许", "Allow for this turn"),
        PermissionDecision::AllowForSession => loc.pick("本次会话允许", "Allow for this session"),
        PermissionDecision::AllowAlways => loc.pick("总是允许", "Always allow"),
        PermissionDecision::Deny => loc.pick("拒绝", "Decline"),
        PermissionDecision::DenyAlways => loc.pick("总是拒绝", "Always decline"),
        PermissionDecision::Cancel => loc.pick("取消", "Cancel"),
    }
}

fn policy_option_label(pol: &PolicyOption, idx: usize, loc: UiLocale) -> String {
    pol.details["name"]
        .as_str()
        .or_else(|| pol.details["label"].as_str())
        .or_else(|| pol.details["title"].as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{} {}", loc.pick("策略", "Policy"), idx + 1))
}

fn timeline_top_fade(scroll: ScrollHandle, surface: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let opacity = (-f32::from(scroll.offset().y) / LH_READ).clamp(0., 1.);
            if opacity > 0. {
                // A feather in the reading surface's own color only softens the scrolling
                // ink. A color gradient also dithers the empty pixels across the whole pane.
                let height = bounds.size.height;
                let feather = Bounds::new(
                    point(bounds.left() - height, bounds.top() - height),
                    size(bounds.size.width + 2. * height, 1.5 * height),
                );
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    window.paint_drop_shadows(feather, Corners::default(), &[BoxShadow {
                        color: surface.opacity(opacity),
                        blur_radius: height / 6.,
                        offset: Point::default(),
                        spread_radius: px(0.),
                        inset: false,
                    }]);
                });
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .right_0()
    .h(px(LH_READ))
}

// The reference caps the content itself at 46rem, with the viewport gutters outside it.
fn chat_lane_gutter(width: f32, window: &Window) -> f32 {
    if f32::from(window.viewport_size().width) < 640. {
        12.
    } else {
        (12. + (width - 360.) * 0.12).clamp(12., 48.)
    }
}

fn chat_content_lane(width: f32, thread_w: f32, window: &Window) -> Div {
    let gutter = chat_lane_gutter(width, window);
    div()
        .w_full()
        .mx_auto()
        .max_w(px(thread_w + 2. * gutter))
        .px(px(gutter))
}

fn turn_matches_search(turn: &ChatTurn, query_lc: &str) -> bool {
    if query_lc.is_empty() {
        return false;
    }
    turn.items.iter().any(|entry| match &entry.item {
        Item::Message { text, .. } => text.to_lowercase().contains(query_lc),
        Item::Diff { diff, .. } => diff.to_lowercase().contains(query_lc),
        Item::Tool { activity, .. } => {
            activity
                .name
                .as_deref()
                .is_some_and(|n| n.to_lowercase().contains(query_lc))
                || activity
                    .tool_name
                    .as_deref()
                    .is_some_and(|n| n.to_lowercase().contains(query_lc))
        }
        Item::RequestError { error } => format!("{error:?}").to_lowercase().contains(query_lc),
        _ => false,
    })
}

fn matching_turn_indices(turns: &[ChatTurn], query_lc: &str) -> Vec<usize> {
    if query_lc.is_empty() {
        return Vec::new();
    }
    turns
        .iter()
        .enumerate()
        .filter_map(|(ix, turn)| turn_matches_search(turn, query_lc).then_some(ix))
        .collect()
}

struct UserInput<'a> {
    index: usize,
    text: &'a str,
    id: Option<Uuid>,
    primary: bool,
}

fn user_inputs(turn: &ChatTurn) -> impl Iterator<Item = UserInput<'_>> {
    let mut needs_primary = !turn.native_continuation;
    turn.items.iter().enumerate().filter_map(move |(index, entry)| {
        let Item::Message { role: KarRole::User, text, provider_id, .. } = &entry.item else {
            return None;
        };
        // Older imports can lack a source ID or carry the session ID from before a resume.
        // An actual child path or registered ACP child identifies a delegated prompt.
        if !entry.source.path.is_empty()
            || entry
                .source
                .session_id
                .as_ref()
                .is_some_and(|id| turn.child_sessions.contains_key(id))
        {
            return None;
        }
        let native_id = provider_id.as_deref().and_then(|id| Uuid::parse_str(id).ok());
        let steered = native_id.is_some_and(|id| {
            turn.steered_inputs.contains_key(&id) || turn.steered_attachments.contains_key(&id)
        });
        let primary = needs_primary && !steered;
        if primary {
            needs_primary = false;
        }
        let id = if primary { Some(turn.id) } else { native_id };
        Some(UserInput { index, text, id, primary })
    })
}

fn original_user_text<'a>(
    text: &'a str,
    target: Option<&SelectionTarget>,
    attachments: &[kar::chat::Attachment],
) -> &'a str {
    let mut text = text;
    if !attachments.is_empty()
        && let Some((body, footer)) = text.rsplit_once("\n\nAttached files (JSON):\n")
        && let Ok(references) = serde_json::from_str::<Vec<serde_json::Value>>(footer)
    {
        let files = attachments.iter().filter(|a| a.kind == AttachmentKind::File);
        let matches = !references.is_empty()
            && files.clone().count() == references.len()
            && files.zip(&references).all(|(file, reference)| {
                let suffix =
                    std::path::PathBuf::from(file.id.to_string()).join("content").join(&file.name);
                reference["name"].as_str() == Some(&file.name)
                    && reference["mime"].as_str() == Some(&file.mime)
                    && reference["path"]
                        .as_str()
                        .is_some_and(|path| std::path::Path::new(path).ends_with(suffix))
            });
        if matches {
            text = body;
        }
    }
    target
        .and_then(|target| text.strip_prefix(&target.compose_prompt("")))
        .unwrap_or_else(|| strip_selection_envelope(text))
}

fn user_bubble(
    key: &str,
    text: &str,
    sent_at: Option<u64>,
    loc: UiLocale,
    s: &StudioPalette,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let group = SharedString::from(format!("user-message-{key}"));
    let hovered = window.use_keyed_state(group.clone(), cx, |_, _| false);
    let focus =
        window.use_keyed_state(SharedString::from(format!("{group}-focus")), cx, |_, cx| {
            cx.focus_handle()
        });
    let focus = focus.read(cx).clone();
    let keyboard_focus = window.last_input_was_keyboard() && focus.contains_focused(window, cx);
    let text = SharedString::from(text.to_owned());
    let bubble = div()
        .min_w_0()
        .max_w_full()
        .px(px(14.))
        .py(px(7.))
        .rounded(px(12.))
        .bg(s.message_surface)
        .text_size(s.font_size(FS_READ))
        .line_height(s.line_height(LH_READ))
        .text_color(s.ink)
        .child(text.clone());
    let actions = div()
        .id(SharedString::from(format!("user-message-actions-{key}")))
        .track_focus(&focus)
        .h(px(CTRL_SM_H))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .when(!keyboard_focus && !window.last_input_was_touch() && !*hovered.read(cx), |d| d.opacity(0.))
        .when_some(sent_at.and_then(message_time::local), |row, time| {
            row.child(message_time::label(
                SharedString::from(format!("user-time-{key}")),
                time,
                s,
                None,
            ))
        })
        .child(copy_text_button(
            format!("user-copy-{key}"),
            loc.pick("复制我的消息", "Copy my message"),
            text,
            window,
            cx,
        ));
    div()
        .min_w_0()
        .max_w(relative(0.85))
        .id(group)
        .on_hover(move |inside, _, cx| {
            hovered.update(cx, |hovered, cx| {
                if *hovered != *inside {
                    *hovered = *inside;
                    cx.notify();
                }
            });
        })
        .flex()
        .flex_col()
        .items_end()
        .gap(px(6.))
        .child(bubble)
        .child(actions)
}

fn worked_duration(ms: u64, loc: UiLocale) -> String {
    let seconds = ms / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    match (loc, seconds) {
        (UiLocale::Zh, 0) => "用时不到 1秒".to_owned(),
        (UiLocale::En, 0) => "Under 1s".to_owned(),
        (UiLocale::Zh, 1..60) => format!("用时 {seconds}秒"),
        (UiLocale::En, 1..60) => format!("Took {seconds}s"),
        (UiLocale::Zh, 60..3600) => format!("用时 {minutes}分钟 {}秒", seconds % 60),
        (UiLocale::En, 60..3600) => format!("Took {minutes}m {}s", seconds % 60),
        (UiLocale::Zh, _) => format!("用时 {hours}小时 {}分钟 {}秒", minutes % 60, seconds % 60),
        (UiLocale::En, _) => format!("Took {hours}h {}m {}s", minutes % 60, seconds % 60),
    }
}

// Only code-shaped replies need the extra parse for the optional message action.
// The actual Markdown layout and code copy both use GPUI's complete GFM document.
fn might_start_with_code(text: &str) -> bool {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| {
            line.starts_with([' ', '\t']) || line.starts_with("```") || line.starts_with("~~~")
        })
}

fn is_single_code_message(text: &str) -> bool {
    use markdown::{ParseOptions, mdast::Node, to_mdast};
    matches!(to_mdast(text, &ParseOptions::gfm()), Ok(Node::Root(root)) if matches!(root.children.as_slice(), [Node::Code(_)]))
}

#[cfg(test)]
mod turn_display_tests {
    use super::{UiLocale, is_single_code_message, might_start_with_code, worked_duration};

    #[test]
    fn user_occurrences_keep_separate_identities_after_runtime_events() {
        use serde_json::json;
        let primary = uuid::Uuid::new_v4();
        let followup = uuid::Uuid::new_v4();
        let message = |session, role, id: Option<uuid::Uuid>| {
            json!({
                "source":{"session_id":session,"path":[]}, "kind":"message",
                "role":role, "text":"好", "provider_id":id, "complete":true
            })
        };
        let turn = serde_json::from_value(json!({
            "id":primary, "started_ms":10, "status":"completed",
            "child_sessions":{"worker":"root"}, "steered_inputs":{(followup.to_string()):"accepted"},
            "items":[message("root","reasoning",None), message("worker","user",None),
                     message("legacy-root","user",None), message("root","user",Some(followup))]
        })).unwrap();
        let inputs: Vec<_> = super::user_inputs(&turn)
            .map(|input| (input.index, input.id, input.text, input.primary))
            .collect();
        assert_eq!(inputs, [(2, Some(primary), "好", true), (3, Some(followup), "好", false)]);
    }

    #[test]
    fn only_the_recorded_attachment_footer_is_removed_from_user_text() {
        use serde_json::json;
        let id = uuid::Uuid::new_v4();
        let file = serde_json::from_value(json!({
            "id":id, "name":"a.txt", "mime":"text/plain", "bytes":1, "kind":"file", "sha256":"x"
        }))
        .unwrap();
        let original = "    preserve spaces\n\nAttached files (JSON):\n[]\n";
        let references = json!([{"name":"a.txt","mime":"text/plain","path":format!("/store/{id}/content/a.txt")}]);
        let with_files = format!("{original}\n\nAttached files (JSON):\n{references}");
        assert_eq!(super::original_user_text(&with_files, None, &[]), with_files);
        assert_eq!(super::original_user_text(&with_files, None, &[file]), original);
    }

    #[test]
    fn message_time_preserves_the_instant_and_skips_unknown_times() {
        let stamp = 1_763_963_000_000;
        let time = super::message_time::local(stamp).unwrap();
        let parsed = chrono::DateTime::parse_from_str(&time.full, "%Y-%m-%d %H:%M:%S %:z").unwrap();
        assert_eq!(parsed.timestamp_millis() as u64, stamp);
        assert_eq!(time.short, parsed.format("%H:%M").to_string());
        assert!(super::message_time::local(0).is_none());
        assert!(super::message_time::local(u64::MAX).is_none());
    }

    #[test]
    fn elapsed_time_uses_local_units_and_keeps_seconds() {
        assert_eq!(worked_duration(600, UiLocale::Zh), "用时不到 1秒");
        assert_eq!(worked_duration(853_900, UiLocale::Zh), "用时 14分钟 13秒");
        assert_eq!(worked_duration(3_602_000, UiLocale::En), "Took 1h 0m 2s");
    }

    #[test]
    fn single_code_messages_use_the_block_action_even_with_table_syntax() {
        for message in [
            "  ~~~markdown\n| 名 | 值 |\n| --- | --- |\n  ~~~",
            "````markdown\n```text\n| a | b |\n| --- | --- |\n```\n````",
            "\n    | a | b |\n    | --- | --- |\n",
        ] {
            assert!(might_start_with_code(message));
            assert!(is_single_code_message(message), "{message}");
        }
        assert!(!is_single_code_message(
            "~~~rs\nfn main() {}\n~~~\n\nMore context."
        ));
        assert!(!is_single_code_message("| a | b |\n| --- | --- |"));
    }
}

fn decision_question(text: String, s: &StudioPalette) -> Div {
    div()
        .text_size(s.font_size(FS_READ))
        .line_height(s.line_height(LH_READ))
        .font_weight(FontWeight::MEDIUM)
        .text_color(s.ink)
        .child(text)
}

fn request_facts(request: &PendingRequest, loc: UiLocale) -> Vec<(&'static str, String)> {
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
        facts.push((loc.pick("命令", "Command"), unwrap_shell_command(&command)));
    }
    if let Some(file) = text_at(&[
        &["input", "file_path"],
        &["input", "path"],
        &["path"],
        &["toolCall", "rawInput", "path"],
    ]) {
        facts.push((loc.pick("文件", "File"), file));
    }
    if let Some(dir) = text_at(&[&["cwd"], &["input", "cwd"]]) {
        facts.push((loc.pick("位置", "Directory"), dir));
    }
    if facts.is_empty()
        && let Some(what) = request.display_text()
    {
        facts.push((loc.pick("操作", "Action"), what));
    }
    facts
}

const HOME_PROJECT_KEY_CONTEXT: &str = "HomeProjectPicker";
actions!(home_project, [NextHomeProject, PreviousHomeProject]);

pub(crate) fn init_project_picker_keys(cx: &mut App) {
    let context = Some(HOME_PROJECT_KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("tab", NextHomeProject, context),
        KeyBinding::new("down", NextHomeProject, context),
        KeyBinding::new("shift-tab", PreviousHomeProject, context),
        KeyBinding::new("up", PreviousHomeProject, context),
    ]);
}

struct HomeProjectMenu {
    focus: FocusHandle,
    scroll: ScrollHandle,
    cursor: usize,
    open: bool,
    focus_on_mount: bool,
}

impl HomeProjectMenu {
    fn advance(&mut self, steps: usize, count: usize) {
        self.cursor = (self.cursor.min(count) + steps) % (count + 1);
        self.reveal(count);
    }

    fn reveal(&self, count: usize) {
        // The list starts with a caption and has a divider before the folder picker.
        self.scroll
            .scroll_to_item(self.cursor + 1 + usize::from(self.cursor == count));
    }
}

const MODEL_ROW_H: f32 = 32.;
const MODEL_LIST_PAD: f32 = 5.;

fn model_cursor_band(
    cursor: usize,
    scroll: &ScrollHandle,
    rose: Hsla,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let target = cursor as f32 * MODEL_ROW_H;
    let y = if kui::primitives::family::disabled_motion(cx) {
        target
    } else {
        pro_spring_f32("model-cursor-y", target, 0.24, window, cx)
    };
    let scroll = scroll.clone();
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let band = Bounds::new(
                bounds.origin + point(px(5.), px(MODEL_LIST_PAD + y) + scroll.offset().y),
                size((bounds.size.width - px(10.)).max(px(0.)), px(MODEL_ROW_H)),
            );
            let wash = linear_gradient(
                90.,
                linear_color_stop(rose.opacity(0.22), 0.),
                linear_color_stop(rose.opacity(0.), 1.),
            );
            window.paint_quad(fill(band, wash).corner_radii(px(6.)));
        },
    )
    .absolute()
    .inset_0()
}

fn model_scroll_fade(scroll: ScrollHandle, surface: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let remaining = f32::from(scroll.max_offset().y + scroll.offset().y);
            if remaining > 0. {
                window.paint_quad(fill(
                    bounds,
                    linear_gradient(
                        180.,
                        linear_color_stop(surface.opacity(0.), 0.),
                        linear_color_stop(surface.opacity((remaining / 12.).min(1.)), 1.),
                    ),
                ));
            }
        },
    )
    .absolute()
    .bottom_0()
    .left_0()
    .right_0()
    .h(px(12.))
}

/// Search state outlives repainting so keyboard focus and IME composition are retained.
struct ModelPickerSearch {
    editor: Entity<Editor>,
    provider: String,
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
            editor.font_size = FS_CONTROL;
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
                        let query = editor.read(cx).value().trim().to_lowercase();
                        if query != view.query {
                            view.query = query;
                            view.cursor = 0;
                            view.scroll.scroll_to_item(0);
                        }
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
