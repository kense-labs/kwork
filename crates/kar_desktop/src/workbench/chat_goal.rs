use super::{DraftLocation, KarWorkbench, QueuedPrompt};
use crate::kar_bridge::{GoalAction, GoalEdit, KarCommand};
use crate::types::ChatAccessMode;
use crate::ui_primitives::{FS_CONTROL, FS_META, R_CTRL, StudioPalette};
use gpui::{prelude::*, *};
use kar::GoalSnapshot;
use kui::components::input::Input;
use uuid::Uuid;

pub(super) fn parse_budget(text: &str) -> Result<u64, ()> {
    text.trim()
        .parse::<u64>()
        .ok()
        .filter(|tokens| (1..=9_007_199_254_740_991).contains(tokens))
        .ok_or(())
}

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
        "budget" => return parse_budget(rest).map(GoalAction::Budget),
        "--budget" => {
            let (tokens, objective) = rest.split_once(char::is_whitespace).ok_or(())?;
            let objective = objective.trim_start();
            if objective.is_empty() {
                return Err(());
            }
            return Ok(GoalAction::Set(GoalEdit {
                objective: objective.into(),
                token_budget: Some(parse_budget(tokens)?),
            }));
        }
        "set" => {
            return (!rest.is_empty())
                .then(|| {
                    GoalAction::Set(GoalEdit {
                        objective: rest.into(),
                        token_budget: None,
                    })
                })
                .ok_or(());
        }
        _ if !input.is_empty() => {
            return Ok(GoalAction::Set(GoalEdit {
                objective: input.into(),
                token_budget: None,
            }));
        }
        _ => return Err(()),
    };
    if rest.is_empty() { Ok(action) } else { Err(()) }
}

#[derive(Debug, PartialEq, Eq)]
enum GoalCommandError {
    Syntax,
    BudgetUnavailable,
}

fn goal_command(argument: &str, runtime: &str) -> Result<GoalAction, GoalCommandError> {
    let first = argument.split_whitespace().next().unwrap_or_default();
    if runtime != "codex"
        && (first.eq_ignore_ascii_case("budget") || first.eq_ignore_ascii_case("--budget"))
    {
        return Err(GoalCommandError::BudgetUnavailable);
    }
    parse_goal(argument).map_err(|()| GoalCommandError::Syntax)
}

impl KarWorkbench {
    pub(super) fn goal_supports_token_budget(&self) -> bool {
        self.active_or_selected_runtime_id() == "codex"
    }

    fn update_goal_budget_hint(&self, existing: Option<u64>, cx: &mut Context<Self>) {
        let loc = self.ui_locale;
        let text = match existing {
            Some(tokens) => format!(
                "{} {tokens} tokens",
                loc.pick(
                    "留空保留现有预算",
                    "Leave blank to keep the current budget of"
                )
            ),
            None => loc
                .pick("可选，输入 token 数", "Optional token limit")
                .to_owned(),
        };
        self.goal_budget_input
            .read(cx)
            .editor()
            .update(cx, |editor, cx| {
                editor.set_placeholder(text);
                cx.notify();
            });
    }

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
        self.update_goal_budget_hint(
            self.current_goal()
                .and_then(|goal| goal.token_budget.flatten()),
            cx,
        );
        self.goal_input
            .update(cx, |field, cx| field.set_value(existing, window, cx));
        self.goal_budget_input
            .update(cx, |field, cx| field.set_value("", window, cx));
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
            .child(field)
            .when(this.goal_supports_token_budget(), |body| {
                body.child(this.goal_budget_input.clone())
            });
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

    pub(crate) fn goal_request_pending(&self, id: Uuid) -> bool {
        self.goal_pending.contains(&id)
    }

    pub(crate) fn request_chat_goal(
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
        match goal_command(argument,self.active_or_selected_runtime_id()) {
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
            Err(GoalCommandError::BudgetUnavailable) => self.notify_warning(
                self.ui_locale.pick("DSH 目标按轮次计限。用 /goal set <目标> 设置目标。","DSH limits goals by rounds. Use /goal set <objective> to set a goal."),cx
            ),
            Err(GoalCommandError::Syntax) if !self.goal_supports_token_budget() => self.notify_warning(
                self.ui_locale.pick("用 /goal set <目标> 设置目标，或用 status、pause、resume、complete、clear 管理目标。","Use /goal set <objective>, or manage a goal with status, pause, resume, complete or clear."),cx
            ),
            Err(GoalCommandError::Syntax) => self.notify_warning(
                self.ui_locale.pick(
                    "用 /goal --budget <正整数> <目标> 设置预算和目标，或用 status、budget、pause、resume、complete、clear 管理目标",
                    "Use /goal --budget <tokens> <objective>, status, budget, pause, resume, complete or clear",
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
        self.update_goal_budget_hint(goal.and_then(|g| g.token_budget.flatten()), cx);
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
                let draft = match action {
                    GoalAction::Set(edit) => Some(edit.command()),
                    GoalAction::Budget(tokens) => Some(format!("/goal budget {tokens}")),
                    _ => None,
                };
                if let Some(command) = draft {
                    self.prepend_composer_draft(
                        DraftLocation::Chat(id),
                        QueuedPrompt {
                            text: command,
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
    use super::{GoalCommandError, goal_command};
    use super::{GoalAction, GoalEdit, parse_goal};
    #[test]
    fn explicit_budget_syntax_requires_codex_while_goals_work_on_dsh() {
        for command in [
            "budget 6800",
            "--budget 6800 ship",
            "BUDGET",
            " --Budget invalid ship",
        ] {
            assert_eq!(
                goal_command(command, "dsh"),
                Err(GoalCommandError::BudgetUnavailable)
            );
        }
        assert_eq!(
            goal_command("budget 6800", "codex"),
            Ok(GoalAction::Budget(6800))
        );
        assert_eq!(
            goal_command("set budget review", "dsh"),
            parse_goal("set budget review").map_err(|_| GoalCommandError::Syntax)
        );
    }

    #[test]
    fn goal_commands_keep_literal_objectives_and_validate_actions() {
        assert_eq!(parse_goal("pause"), Ok(GoalAction::Pause));
        assert_eq!(parse_goal("status"), Ok(GoalAction::Inspect));
        assert_eq!(parse_goal("pause now"), Err(()));
        assert_eq!(parse_goal("set"), Err(()));
        assert_eq!(
            parse_goal("set pause"),
            Ok(GoalAction::Set(GoalEdit {
                objective: "pause".into(),
                token_budget: None
            }))
        );
        assert_eq!(
            parse_goal("ship safely"),
            Ok(GoalAction::Set(GoalEdit {
                objective: "ship safely".into(),
                token_budget: None
            }))
        );
        let edit = GoalEdit {
            objective: "a multi-word objective".into(),
            token_budget: Some(6800),
        };
        assert_eq!(
            parse_goal(edit.command().strip_prefix("/goal ").unwrap()),
            Ok(GoalAction::Set(edit))
        );
        assert_eq!(parse_goal("budget 6800"), Ok(GoalAction::Budget(6800)));
        for invalid in [
            "budget -1",
            "budget 0",
            "budget 9007199254740992",
            "--budget 1",
            "--budget 0 task",
        ] {
            assert_eq!(parse_goal(invalid), Err(()));
        }
        assert!(GoalAction::Pause.available("active"));
        assert!(!GoalAction::Resume.available("complete"));
    }
}
