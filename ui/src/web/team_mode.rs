//! Root-session Agent Team settings, disable confirmation, and shared task board.

use super::permission_dialog::{dialog_action, dialog_header_with_warning};
use super::*;
use crate::subagents::TeamState;
use serde::Serialize;

const DISABLE_TITLE: &str = "确认关闭智能体团队协作？";
const DISABLE_DESCRIPTION: [&str; 2] = [
    "对话过程中关闭智能体团队协作，将停止共享任务工具和团队协调指令。",
    "关闭后可能影响缓存复用、增加后续 token 用量与开销，并影响当前任务的协作效果。",
];
const DISABLE_CONFIRM: &str = "确认关闭";

pub(super) struct TeamDisableRequest {
    session_id: String,
    close: gpui::FocusHandle,
    cancel: gpui::FocusHandle,
    confirm: gpui::FocusHandle,
    return_focus: Option<gpui::FocusHandle>,
    error: Option<String>,
}

#[derive(PartialEq, Eq, Serialize)]
pub(super) struct AccessibleTeamDisable {
    session_id: String,
    title: &'static str,
    description: [&'static str; 2],
    confirm_label: &'static str,
    pending: bool,
    error: Option<String>,
}

impl DshApp {
    pub(super) fn request_team_state(&mut self, session: &str) {
        let Some(conversation) = self.conversation_mut(session) else {
            return;
        };
        if conversation.team_pending {
            return;
        }
        conversation.team_request += 1;
        let request = conversation.team_request;
        invoke_with_reply(
            "team_state",
            "team_state",
            json!({"sessionId": session, "request": request}),
        );
    }

    pub(super) fn finish_team_state(
        &mut self,
        owner: Option<&str>,
        result: Result<Value, String>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = owner else { return };
        let request = result
            .as_ref()
            .ok()
            .and_then(|reply| reply["request"].as_u64());
        let failed_read = result.is_err();
        let (previous, current, rereading) = {
            let Some(conversation) = self.conversation_mut(session) else {
                return;
            };
            if request != Some(conversation.team_request) || conversation.team_pending {
                return;
            }
            let previous = conversation.team.clone();
            let rereading = std::mem::take(&mut conversation.team_rereading);
            if let Ok(reply) = &result {
                if reply["available"].as_bool() == Some(true) {
                    match serde_json::from_value::<TeamState>(reply["team"].clone()) {
                        Ok(mut team) if reply["team"].is_object() => {
                            team.expanded_task = conversation
                                .team
                                .as_ref()
                                .and_then(|previous| previous.expanded_task.clone())
                                .filter(|expanded| {
                                    team.tasks.iter().any(|task| task.id == *expanded)
                                });
                            conversation.team = Some(team)
                        }
                        _ => conversation.team = None,
                    }
                } else {
                    conversation.team = None;
                }
            }
            (previous, conversation.team.clone(), rereading)
        };
        if failed_read && rereading {
            self.push_toast(
                "无法确认智能体团队协作设置已保存，请重试：无法重新读取 Host 状态。",
                cx,
            );
        }
        if self.active_session.as_deref() == Some(session) {
            if self.composer_menu == Some(ComposerMenu::Team) && current.is_none() {
                self.back_model_menu(ComposerMenu::Team, w, cx);
            }
            if current != previous {
                cx.notify();
            }
        }
    }

    pub(super) fn choose_team_mode(
        &mut self,
        enabled: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.conversation.team_pending
            || self.subagent_view.is_some()
            || self.team_disable.is_some()
        {
            return;
        }
        let Some(team) = self.conversation.team.clone() else {
            return;
        };
        if team.enabled == enabled || team.has_used_model.is_none() {
            return;
        }
        if !enabled && crate::subagents::disable_needs_confirmation(&team) == Some(true) {
            let Some(session) = self.active_session.clone() else {
                return;
            };
            self.composer_menu = None;
            self.model_menu.active = None;
            let cancel = cx.focus_handle();
            cancel.focus(w, cx);
            self.team_disable = Some(TeamDisableRequest {
                session_id: session,
                close: cx.focus_handle(),
                cancel,
                confirm: cx.focus_handle(),
                return_focus: Some(self.model_menu.trigger.clone()),
                error: None,
            });
            cx.notify();
            return;
        }
        let Some(session) = self.active_session.clone() else {
            return;
        };
        self.save_team_mode(session, enabled, w, cx);
    }

    fn save_team_mode(
        &mut self,
        session: String,
        enabled: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(conversation) = self.conversation_mut(&session) else {
            return;
        };
        if conversation.team_pending {
            return;
        }
        conversation.team_request += 1;
        let request = conversation.team_request;
        conversation.team_pending = true;
        if let Some(confirm) = self
            .team_disable
            .as_mut()
            .filter(|request| request.session_id == session)
        {
            confirm.error = None;
        }
        cx.spawn_in(w, async move |this, cx| {
            let result = bridge::invoke(
                "team_state",
                json!({"sessionId": session, "enabled": enabled, "request": request}),
            )
            .await;
            let result = match result {
                Ok(reply)
                    if reply["available"].as_bool() == Some(true) && reply["team"].is_object() =>
                {
                    Ok(reply)
                }
                Ok(_) => Err("当前会话不支持智能体团队协作。".to_owned()),
                Err(error) => Err(error),
            };
            let _ = this.update_in(cx, |app, w, cx| {
                app.finish_team_save(Some(&session), result, w, cx)
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn finish_team_save(
        &mut self,
        owner: Option<&str>,
        result: Result<Value, String>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = owner else { return };
        let Some(conversation) = self.conversation_mut(session) else {
            return;
        };
        conversation.team_pending = false;
        match result {
            Ok(reply) => {
                if let Some(conversation) = self.conversation_mut(session) {
                    conversation.team_rereading = true;
                }
                self.finish_team_state(owner, Ok(reply), w, cx);
                if self.team_disable_matches(session) {
                    self.close_team_disable(w, cx);
                }
            }
            Err(error) => {
                let message = format!("无法确认智能体团队协作设置已保存，请重试：{error}");
                if let Some(request) = self
                    .team_disable
                    .as_mut()
                    .filter(|request| request.session_id == session)
                {
                    request.error = Some(message);
                } else {
                    self.push_toast(message, cx);
                }
                if let Some(conversation) = self.conversation_mut(session) {
                    conversation.team_rereading = true;
                }
                self.request_team_state(session);
            }
        }
        cx.notify();
    }

    fn team_disable_matches(&self, session: &str) -> bool {
        self.team_disable
            .as_ref()
            .is_some_and(|request| request.session_id == session)
    }

    fn team_disable_pending(&self, request: &TeamDisableRequest) -> bool {
        self.session_conversation(&request.session_id)
            .is_some_and(|conversation| conversation.team_pending)
    }

    pub(super) fn accessible_team_disable(&self) -> Option<AccessibleTeamDisable> {
        let request = self.team_disable.as_ref()?;
        Some(AccessibleTeamDisable {
            session_id: request.session_id.clone(),
            title: DISABLE_TITLE,
            description: DISABLE_DESCRIPTION,
            confirm_label: DISABLE_CONFIRM,
            pending: self.team_disable_pending(request),
            error: request.error.clone(),
        })
    }

    pub(super) fn act_on_team_disable(
        &mut self,
        session: &str,
        confirm: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.team_disable_matches(session) {
            return;
        }
        if confirm {
            self.save_team_mode(session.to_owned(), false, w, cx);
        } else {
            self.close_team_disable(w, cx);
        }
    }

    pub(super) fn close_team_disable(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self
            .team_disable
            .as_ref()
            .is_some_and(|request| self.team_disable_pending(request))
        {
            return;
        }
        if let Some(request) = self.team_disable.take()
            && self.active_session.as_deref() == Some(request.session_id.as_str())
            && let Some(focus) = request.return_focus
        {
            focus.focus(w, cx);
        }
        cx.notify();
    }

    fn traverse_team_disable(&self, event: &gpui::KeyDownEvent, w: &mut Window, cx: &mut App) {
        let Some(request) = self.team_disable.as_ref() else {
            return;
        };
        if self.team_disable_pending(request) {
            request.close.focus(w, cx);
            return;
        }
        let buttons = [&request.close, &request.cancel, &request.confirm];
        let target = if let Some(index) = buttons.iter().position(|focus| focus.is_focused(w)) {
            buttons[(index
                + if event.keystroke.modifiers.shift {
                    2
                } else {
                    1
                })
                % buttons.len()]
        } else {
            &request.cancel
        };
        target.focus(w, cx);
    }

    pub(super) fn render_team_pane(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let p = dsh(cx);
        let enabled = self
            .conversation
            .team
            .as_ref()
            .is_some_and(|team| team.enabled);
        let available = self
            .conversation
            .team
            .as_ref()
            .is_some_and(|team| team.has_used_model.is_some());
        let busy = self.conversation.team_pending;
        row()
            .id("team-mode-switch")
            .role(gpui::Role::Switch)
            .aria_label("开启智能体团队协作")
            .aria_toggled(enabled.into())
            .tab_stop(available && !busy)
            .when(self.model_menu.active == Some(0), |row| {
                row.track_focus(&self.model_menu.row).bg(p.hover)
            })
            .w_full()
            .flex_none()
            .px(px(12.))
            .py(px(10.))
            .gap(px(16.))
            .rounded(px(12.))
            .items_start()
            .when(!available || busy, |row| row.opacity(0.5))
            .when(available && !busy, |row| {
                row.cursor_pointer().on_click(cx.listener(|this, _, w, cx| {
                    let enabled = this.conversation.team.as_ref().is_some_and(|team| team.enabled);
                    this.choose_team_mode(!enabled, w, cx);
                }))
            })
            .child(
                column()
                    .flex_1()
                    .min_w_0()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .child("开启智能体团队协作"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(p.label_caption)
                            .child("通过共享任务板协调多个智能体的分工与进度，复杂任务表现更佳，token 开销也相应更高。"),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    .w(px(36.))
                    .h(px(20.))
                    .mt(px(2.))
                    .rounded_full()
                    .bg(if enabled { p.button_primary_fill } else { p.border_l3 })
                    .child(
                        div()
                            .absolute()
                            .left(px(if enabled { 18. } else { 2. }))
                            .top(px(2.))
                            .size(px(16.))
                            .rounded_full()
                            .bg(if enabled { p.label_primary_foreground } else { p.switch_thumb }),
                    ),
            )
    }

    pub(super) fn render_team_board(
        &self,
        team: &TeamState,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if !team.enabled {
            return div().into_any_element();
        }
        let p = dsh(cx);
        let mut board = column()
            .id("agent-team-board")
            .role(gpui::Role::Region)
            .aria_label("任务板")
            .w_full()
            .px(px(8.))
            .py(px(6.))
            .gap(px(4.))
            .rounded(px(12.))
            .bg(p.bg_layer_1);
        if !team.tasks.is_empty() {
            board = board.child(
                row()
                    .w_full()
                    .h(px(22.))
                    .gap(px(6.))
                    .child(icon(Glyph::Card, 14., p.faint))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .child("共享任务"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_right()
                            .text_size(px(11.))
                            .line_height(px(18.))
                            .text_color(p.faint)
                            .child(crate::subagents::progress_label(team)),
                    ),
            );
        }
        if let Some(failure) = &team.failure {
            board = board.child(
                row()
                    .id("agent-team-failure")
                    .role(gpui::Role::Alert)
                    .gap(px(6.))
                    .child(icon(Glyph::Alert, 13., p.danger))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(p.danger)
                            .child(format!("任务记录无效：{failure}")),
                    ),
            );
        }
        for task in &team.tasks {
            let id = task.id.clone();
            let expanded = team.expanded_task.as_deref() == Some(&id);
            let blocked = task.status == "pending" && !task.ready;
            let status = if blocked {
                "被依赖阻塞"
            } else {
                task.status_label()
            };
            board = board.child(
                column()
                    .w_full()
                    .child(
                        row()
                            .id(gpui::ElementId::Name(format!("team-task-{id}").into()))
                            .role(gpui::Role::Button)
                            .aria_label(format!("{}，{status}", task.subject))
                            .aria_expanded(expanded)
                            .tab_index(0)
                            .w_full()
                            .h(px(26.))
                            .gap(px(7.))
                            .rounded(px(7.))
                            .cursor_pointer()
                            .hover(|style| style.bg(p.hover))
                            .child(div().size(px(7.)).ml(px(3.)).flex_none().rounded_full().bg(
                                match task.status.as_str() {
                                    "completed" => p.success,
                                    "in_progress" => p.state_business_primary,
                                    _ if blocked => p.state_warn_primary,
                                    _ => p.label_caption,
                                },
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .line_height(px(18.))
                                    .child(task.subject.clone()),
                            )
                            .child(icon(
                                if expanded { Glyph::Up } else { Glyph::Down },
                                12.,
                                p.faint,
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let Some(team) = this.conversation.team.as_mut() else {
                                    return;
                                };
                                team.expanded_task = if team.expanded_task.as_deref() == Some(&id) {
                                    None
                                } else {
                                    Some(id.clone())
                                };
                                cx.notify();
                            })),
                    )
                    .when(expanded, |element| {
                        element.child(
                            column()
                                .pl(px(17.))
                                .pr(px(8.))
                                .pb(px(6.))
                                .gap(px(4.))
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .when(!task.description.is_empty(), |details| {
                                    details.child(task.description.clone())
                                })
                                .child(
                                    crate::web::row()
                                        .gap(px(4.))
                                        .text_color(p.faint)
                                        .child("负责人：")
                                        .child(task.owner().to_owned()),
                                ),
                        )
                    }),
            );
        }
        board.into_any_element()
    }

    pub(super) fn render_team_disable(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Stateful<gpui::Div>> {
        let request = self.team_disable.as_ref()?;
        let busy = self.team_disable_pending(request);
        let session = request.session_id.clone();
        Some(
            row()
                .id("team-disable-overlay")
                .absolute()
                .inset_0()
                .p(px(24.))
                .justify_center()
                .items_center()
                .bg(dsh(cx).bg_mask_1)
                .occlude()
                .on_click(cx.listener(|app, _, w, cx| app.close_team_disable(w, cx)))
                .child(
                    column()
                        .id("team-disable-dialog")
                        .role(gpui::Role::Dialog)
                        .aria_label(DISABLE_TITLE)
                        .tab_group()
                        .w(px(440.))
                        .max_w_full()
                        .max_h_full()
                        .min_h_0()
                        .gap(px(20.))
                        .pb(px(24.))
                        .rounded(px(24.))
                        .bg(dsh(cx).bg_layer_2)
                        .shadow(elevation_prominent())
                        .occlude()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .capture_key_down(cx.listener(|app, event: &gpui::KeyDownEvent, w, cx| {
                            let stroke = &event.keystroke;
                            match stroke.key.as_str() {
                                "escape" => app.close_team_disable(w, cx),
                                "tab"
                                    if !(stroke.modifiers.control
                                        || stroke.modifiers.alt
                                        || stroke.modifiers.platform) =>
                                {
                                    app.traverse_team_disable(event, w, cx);
                                }
                                _ => return,
                            }
                            w.prevent_default();
                            cx.stop_propagation();
                        }))
                        .child(
                            column()
                                .id("team-disable-content")
                                .min_h_0()
                                .overflow_y_scroll()
                                .child(dialog_header_with_warning(
                                    DISABLE_TITLE,
                                    "team-disable-close",
                                    Some(&request.close),
                                    true,
                                    cx,
                                    |app, w, cx| app.close_team_disable(w, cx),
                                ))
                                .child(
                                    column()
                                        .px(px(24.))
                                        .gap(px(12.))
                                        .text_size(px(14.))
                                        .line_height(px(22.))
                                        .text_color(dsh(cx).secondary)
                                        .children(DISABLE_DESCRIPTION)
                                        .children(request.error.as_ref().map(|error| {
                                            div().text_color(dsh(cx).danger).child(error.clone())
                                        })),
                                ),
                        )
                        .child(
                            row()
                                .flex_none()
                                .px(px(24.))
                                .gap(px(8.))
                                .justify_end()
                                .child(
                                    dialog_action(
                                        "team-disable-cancel",
                                        "取消",
                                        false,
                                        busy,
                                        72.,
                                        cx,
                                        |app, w, cx| app.close_team_disable(w, cx),
                                    )
                                    .track_focus(&request.cancel),
                                )
                                .child(
                                    dialog_action(
                                        "team-disable-confirm",
                                        DISABLE_CONFIRM,
                                        true,
                                        busy,
                                        96.,
                                        cx,
                                        move |app, w, cx| {
                                            app.act_on_team_disable(&session, true, w, cx)
                                        },
                                    )
                                    .track_focus(&request.confirm),
                                ),
                        ),
                ),
        )
    }
}
