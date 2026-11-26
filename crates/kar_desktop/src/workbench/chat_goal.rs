use super::{DraftLocation, KarWorkbench, QueuedPrompt};
use crate::kar_bridge::{GoalAction, KarCommand};
use crate::types::ChatAccessMode;
use crate::ui_primitives::{FS_CONTROL, FS_META, R_CTRL, StudioPalette};
use gpui::{prelude::*, *};
use kar::GoalSnapshot;
use kui::components::input::Input;
use uuid::Uuid;

fn parse_goal(input: &str) -> Result<GoalAction, ()> {
    let input = input.trim();
    let (first, rest) = input.split_once(char::is_whitespace).unwrap_or((input, ""));
    let rest = rest.trim_start();
    let action = match first.to_ascii_lowercase().as_str() {
        "status" | "inspect" => GoalAction::Inspect,
        "pause" => GoalAction::Pause,
        "resume" => GoalAction::Resume,
        "complete" => GoalAction::Complete,
        "clear" => GoalAction::Clear,
        "set" => {
            return (!rest.is_empty())
                .then(|| GoalAction::Set(rest.into()))
                .ok_or(());
        }
        _ => return Ok(GoalAction::Set(input.into())),
    };
    if rest.is_empty() { Ok(action) } else { Err(()) }
}

impl KarWorkbench {
    fn current_goal(&self) -> Option<&GoalSnapshot> {
        let id = self.current_chat_id()?;
        (self.projected.chat_id == Some(id)).then_some(())?;
        self.projected.goal.as_ref()?.as_ref()
    }

    pub(crate) fn goal_status_label(&self, status: &str) -> String {
        let loc = self.ui_locale;
        match status {
            "active" => loc.pick("进行中", "Active"),
            "paused" => loc.pick("已暂停", "Paused"),
            "complete" => loc.pick("已完成", "Complete"),
            "blocked" => loc.pick("受阻", "Blocked"),
            "usageLimited" => loc.pick("已达用量限制", "Usage limit reached"),
            "budgetLimited" => loc.pick("已达预算", "Budget reached"),
            other => other,
        }
        .into()
    }

    pub(crate) fn open_goal_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_set_chat_goal() {
            return;
        }
        self.goal_dialog_owner = Some(DraftLocation::for_mode(
            &self.access_mode,
            &self.config.workspace_dir,
        ));
        self.open_menu = None;
        self.command_palette_open = false;
        self.provider_sheet_open = false;
        let existing = self
            .current_goal()
            .map(|g| g.objective.clone())
            .unwrap_or_default();
        self.goal_form_baseline.clone_from(&existing);
        self.goal_input
            .update(cx, |field, cx| field.set_value(existing, window, cx));
        self.goal_dialog
            .update(cx, |d, cx| d.set_open(true, window, cx));
        self.goal_input.read(cx).focus_handle(cx).focus(window, cx);
        if let ChatAccessMode::WriteController { chat_id } = self.access_mode
            && !self.goal_pending.contains(&chat_id)
        {
            self.request_chat_goal(chat_id, GoalAction::Inspect, cx);
        }
        cx.notify();
    }

    pub(super) fn goal_dialog_body(
        owner: &WeakEntity<Self>,
        field: Entity<Input>,
        cx: &App,
    ) -> AnyElement {
        let Some(entity) = owner.upgrade() else {
            return div().into_any_element();
        };
        let this = entity.read(cx);
        let loc = this.ui_locale;
        let s = StudioPalette::get(cx);
        if this.goal_dialog_owner.as_ref()
            != Some(&DraftLocation::for_mode(
                &this.access_mode,
                &this.config.workspace_dir,
            ))
        {
            return div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(field)
                .child(loc.pick(
                    "会话已切换，保存将保留到原草稿中。",
                    "Chat changed. Save to keep this objective in its original draft.",
                ))
                .into_any_element();
        }
        let chat_id = this.current_chat_id();
        let goal = this.current_goal();
        let busy = chat_id.is_some_and(|id| this.goal_pending.contains(&id));
        let state = match goal {
            Some(goal) => this.goal_status_label(&goal.status),
            None if chat_id.is_none() => loc
                .pick(
                    "保存后将在新会话中启用目标",
                    "Save to start a new chat with this goal",
                )
                .into(),
            None if busy => loc
                .pick("正在读取会话目标…", "Reading session goal…")
                .into(),
            None if this.projected.goal.is_some() => {
                loc.pick("当前会话没有目标", "This chat has no goal").into()
            }
            None => loc
                .pick("尚未读取到目标状态", "Goal status is unavailable")
                .into(),
        };
        let mut details = vec![state];
        if let Some(goal) = goal {
            if let Some(used) = goal.tokens_used {
                details.push(match goal.token_budget.flatten() {
                    Some(limit) => format!("{used} / {limit} tokens"),
                    None => format!("{used} tokens"),
                });
            } else if let Some(limit) = goal.token_budget.flatten() {
                details.push(format!("{} {limit} tokens", loc.pick("预算", "Budget")));
            }
            if let Some(seconds) = goal.time_used_seconds {
                details.push(format!("{} {}s", loc.pick("已用时", "Elapsed"), seconds));
            }
        }
        let mut body = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(px(FS_META))
                    .text_color(s.muted)
                    .child(details.join(" · ")),
            )
            .child(field);
        if let (Some(goal), Some(id)) = (goal, chat_id) {
            let mut controls = div().flex().flex_wrap().gap(px(4.));
            for (action, key, label) in [
                (GoalAction::Pause, "goal-pause", loc.pick("暂停", "Pause")),
                (
                    GoalAction::Resume,
                    "goal-resume",
                    loc.pick("继续", "Resume"),
                ),
                (
                    GoalAction::Complete,
                    "goal-complete",
                    loc.pick("标记完成", "Mark complete"),
                ),
                (
                    GoalAction::Clear,
                    "goal-clear",
                    loc.pick("清除目标", "Clear goal"),
                ),
            ] {
                if !action.available(&goal.status) {
                    continue;
                }
                let handle = owner.clone();
                controls = controls.child(
                    div()
                        .id(key)
                        .role(Role::Button)
                        .aria_label(label)
                        .tab_index(if busy { -1 } else { 0 })
                        .focus_visible(|d| d.bg(s.hover))
                        .px(px(8.))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .rounded(px(R_CTRL))
                        .text_size(px(FS_CONTROL))
                        .text_color(if busy { s.muted } else { s.ink })
                        .when(!busy, move |button| {
                            button.cursor_pointer().hover(|d| d.bg(s.hover)).on_click(
                                move |_, _, cx| {
                                    let _ = handle.update(cx, |this, cx| {
                                        this.request_chat_goal(id, action.clone(), cx);
                                    });
                                },
                            )
                        })
                        .child(label),
                );
            }
            body = body.child(controls);
        }
        body.into_any_element()
    }

    pub(super) fn request_chat_goal(
        &mut self,
        id: Uuid,
        action: GoalAction,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.can_set_chat_goal() || self.current_chat_id() != Some(id) {
            self.notify_warning(
                self.ui_locale
                    .pick("当前会话无法管理目标", "This chat cannot manage goals"),
                cx,
            );
            return false;
        }
        if !self.goal_pending.insert(id) {
            self.notify_warning(
                self.ui_locale.pick(
                    "正在等待目标操作完成，请稍后再试",
                    "A goal request is pending. Try again in a moment.",
                ),
                cx,
            );
            return false;
        }
        self.bridge.send(KarCommand::ManageGoal {
            chat_id: id,
            action,
        });
        self.goal_dialog.update(cx, |_, cx| cx.notify());
        cx.notify();
        true
    }

    pub(super) fn handle_goal_command(
        &mut self,
        argument: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match parse_goal(argument) {
            Ok(GoalAction::Set(objective)) => {
                self.submit_chat_goal(objective, window, cx);
            }
            Ok(GoalAction::Inspect) => {
                self.composer_editor
                    .update(cx, |ed, cx| ed.set_value("", window, cx));
                self.open_goal_dialog(window, cx);
            }
            Ok(action) => {
                let Some(id) = self.current_chat_id() else {
                    self.notify_warning(
                        self.ui_locale.pick(
                            "请先用 /goal set 设置会话目标",
                            "Set a chat goal with /goal set first",
                        ),
                        cx,
                    );
                    return;
                };
                if self.request_chat_goal(id, action, cx) {
                    self.composer_editor
                        .update(cx, |ed, cx| ed.set_value("", window, cx));
                }
            }
            Err(()) => self.notify_warning(
                self.ui_locale.pick(
                    "可用指令：/goal set <目标>、status、pause、resume、complete、clear",
                    "Use /goal set <objective>, status, pause, resume, complete or clear",
                ),
                cx,
            ),
        }
    }

    pub(super) fn sync_goal_form(
        &mut self,
        id: Uuid,
        goal: Option<&GoalSnapshot>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.goal_dialog.read(cx).is_open()
            || self.goal_dialog_owner != Some(DraftLocation::Chat(id))
        {
            return;
        }
        let text = goal.map(|g| g.objective.as_str()).unwrap_or("");
        if self.goal_input.read(cx).value(cx) == self.goal_form_baseline
            && self.goal_form_baseline != text
        {
            self.goal_input
                .update(cx, |field, cx| field.set_value(text, window, cx));
        }
        self.goal_form_baseline = text.to_owned();
        self.goal_dialog.update(cx, |_, cx| cx.notify());
    }

    pub(super) fn on_goal_result(
        &mut self,
        id: Uuid,
        action: GoalAction,
        result: Result<Option<GoalSnapshot>, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.goal_pending.remove(&id);
        let current = self.current_chat_id() == Some(id) && self.projected.chat_id == Some(id);
        match result {
            Ok(goal) if current => {
                self.sync_goal_form(id, goal.as_ref(), window, cx);
                self.projected.goal = Some(goal.clone());
                if action != GoalAction::Inspect {
                    let text = match goal {
                        Some(goal) => format!(
                            "{} · {}",
                            self.ui_locale.pick("目标已更新", "Goal updated"),
                            self.goal_status_label(&goal.status)
                        ),
                        None => self.ui_locale.pick("目标已清除", "Goal cleared").into(),
                    };
                    self.notify_toast(text, cx);
                }
            }
            Err(reason) => {
                if let GoalAction::Set(objective) = action {
                    self.prepend_composer_draft(
                        DraftLocation::Chat(id),
                        QueuedPrompt {
                            text: format!("/goal set {objective}"),
                            ..Default::default()
                        },
                        window,
                        cx,
                    );
                }
                if current {
                    self.notify_warning(
                        format!(
                            "{} · {reason}",
                            self.ui_locale.pick("目标操作失败", "Goal request failed")
                        ),
                        cx,
                    );
                }
            }
            _ => {}
        }
        self.goal_dialog.update(cx, |_, cx| cx.notify());
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{GoalAction, parse_goal};
    #[test]
    fn goal_commands_keep_literal_objectives_and_validate_actions() {
        assert_eq!(parse_goal("pause"), Ok(GoalAction::Pause));
        assert_eq!(parse_goal("status"), Ok(GoalAction::Inspect));
        assert_eq!(parse_goal("pause now"), Err(()));
        assert_eq!(parse_goal("set"), Err(()));
        assert_eq!(parse_goal("set pause"), Ok(GoalAction::Set("pause".into())));
        assert_eq!(
            parse_goal("ship safely"),
            Ok(GoalAction::Set("ship safely".into()))
        );
        assert!(GoalAction::Pause.available("active"));
        assert!(!GoalAction::Resume.available("complete"));
    }
}
