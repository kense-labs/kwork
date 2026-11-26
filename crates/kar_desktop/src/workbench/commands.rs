//! Composer commands use the active runtime's inventory and the local chat controller.
use super::{DraftLocation, KarWorkbench, QueuedPrompt};
use crate::kar_bridge::KarCommand;
use crate::studio_hub::SlashCommandItem;
use crate::types::ChatAccessMode;
use crate::ui_primitives::UiLocale;
use crate::ui_primitives::{FS_META, FS_UI, R_CARD, StudioPalette};
use gpui::{prelude::*, *};

#[derive(Default)]
pub(super) struct SlashPicker {
    browse: bool,
    dismissed: bool,
    index: usize,
    pub(super) creating_goal: bool,
    scroll: ScrollHandle,
}

impl SlashPicker {
    fn query<'a>(&self, text: &'a str) -> Option<&'a str> {
        if self.dismissed {
            return None;
        }
        if text.starts_with('/') && !text[1..].contains('/') && !text.contains(char::is_whitespace)
        {
            Some(text)
        } else {
            self.browse.then_some("/")
        }
    }

    pub(super) fn edited(&mut self) {
        self.browse = false;
        self.dismissed = false;
        self.index = 0;
        self.scroll.set_offset(point(px(0.), px(0.)));
    }

    pub(super) fn close(&mut self) {
        self.browse = false;
        self.dismissed = true;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Command {
    Goal,
    PlanDraft,
    Native(String),
}

struct Choice {
    name: String,
    description: String,
    action: Command,
    enabled: bool,
}

fn args<'a>(text: &'a str, command: &str) -> Option<&'a str> {
    let tail = text.strip_prefix(command)?;
    (tail.is_empty() || tail.starts_with(char::is_whitespace)).then(|| tail.trim_start())
}

fn command_choices(
    goal: bool,
    native: &[SlashCommandItem],
    known: bool,
    query: &str,
    loc: UiLocale,
) -> Vec<Choice> {
    let mut choices = Vec::new();
    if goal || (known && !native.iter().any(|c| c.name == "/goal")) {
        choices.push(Choice {
            name: "/goal".into(),
            description: if goal {
                loc.pick("设置对话目标", "Set a conversation goal")
            } else {
                loc.pick("当前会话不支持目标", "Goals unavailable in this session")
            }
            .into(),
            enabled: goal,
            action: Command::Goal,
        });
    }
    if known && !native.iter().any(|c| c.name == "/plan") {
        choices.push(Choice {
            name: "/plan".into(),
            description: loc.pick("起草计划请求", "Draft a planning request").into(),
            enabled: true,
            action: Command::PlanDraft,
        });
    }
    for cmd in native {
        if cmd.name == "/goal" && goal {
            continue;
        }
        choices.push(Choice {
            name: cmd.name.clone(),
            description: cmd.description.clone(),
            action: Command::Native(cmd.name.clone()),
            enabled: true,
        });
    }
    let prefix = query.to_ascii_lowercase();
    choices.retain(|c| c.name.to_ascii_lowercase().starts_with(&prefix));
    choices
}

impl KarWorkbench {
    pub(crate) fn can_set_chat_goal(&self) -> bool {
        match self.access_mode {
            ChatAccessMode::WriteController { chat_id } => {
                self.projected.chat_id == Some(chat_id) && self.projected.can_goals
            }
            ChatAccessMode::NoneSelected => self
                .providers
                .iter()
                .find(|p| p.id == self.selected_runtime)
                .is_some_and(|p| p.goals_supported),
            _ => false,
        }
    }

    fn slash_choices(&self, query: &str) -> Vec<Choice> {
        let goal = self.can_set_chat_goal();
        let inventory = if matches!(
            self.access_mode,
            ChatAccessMode::RemoteController { .. } | ChatAccessMode::DemoShowcase
        ) {
            Some(&[][..])
        } else {
            self.workspace_inventory.commands_for(
                &self.effective_workspace_dir(),
                self.active_or_selected_runtime_id(),
            )
        };
        command_choices(
            goal,
            inventory.unwrap_or_default(),
            inventory.is_some(),
            query,
            self.ui_locale,
        )
    }

    pub(crate) fn open_slash_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reveal_chat_composer(window, cx);
        if self.layout_mode == super::WorkspaceLayoutMode::CanvasWide {
            self.set_layout_mode(super::WorkspaceLayoutMode::Split, cx);
        }
        self.slash.edited();
        self.slash.browse = true;
        self.bridge.send(KarCommand::ProbeWorkspaceInventory {
            workspace: self.effective_workspace_dir(),
            runtime_id: self.active_or_selected_runtime_id().into(),
        });
        cx.notify();
    }

    fn choose_slash(&mut self, cmd: Command, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.composer_editor.read(cx).value().to_owned();
        let existing = if self.slash.browse { &draft } else { "" };
        self.slash.close();
        match cmd {
            Command::Goal => {
                if existing.is_empty() {
                    self.composer_editor
                        .update(cx, |ed, cx| ed.set_value("", window, cx));
                }
                self.open_goal_dialog(window, cx);
            }
            Command::PlanDraft => self.fill_plan_request(existing, window, cx),
            Command::Native(name) => {
                self.composer_editor.update(cx, |ed, cx| {
                    ed.set_value(format!("{name} {existing}"), window, cx);
                    ed.focus(window, cx);
                });
            }
        }
        cx.notify();
    }

    fn fill_plan_request(&mut self, topic: &str, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = self.ui_locale.pick(
            "请先给出实施计划，等我确认后再执行。\n任务：",
            "Propose an implementation plan and wait for my confirmation before executing.\nTask: ",
        );
        self.composer_editor.update(cx, |ed, cx| {
            ed.set_value(format!("{prompt}{topic}"), window, cx);
            ed.focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn confirm_chat_goal(&mut self, win: &mut Window, cx: &mut Context<Self>) {
        let Some(owner) = self.goal_dialog_owner.clone() else {
            return;
        };
        let objective = self.goal_input.read(cx).value(cx).trim().to_owned();
        if DraftLocation::for_mode(&self.access_mode, &self.config.workspace_dir) != owner {
            if !objective.is_empty() {
                self.prepend_composer_draft(
                    owner,
                    QueuedPrompt {
                        text: format!("/goal {objective}"),
                        ..Default::default()
                    },
                    win,
                    cx,
                );
                self.notify_warning(
                    self.ui_locale.pick(
                        "对话已切换，目标已留在原草稿中。",
                        "Conversation changed. Your objective is in the original draft.",
                    ),
                    cx,
                );
            }
            self.goal_dialog_owner = None;
            return;
        }
        if !objective.is_empty() && self.submit_chat_goal(objective, win, cx) {
            self.goal_dialog_owner = None;
            return;
        }
        // The shared dialog closes before emitting Confirmed. Keep the same input and owner on preflight failure.
        self.goal_dialog
            .update(cx, |dialog, cx| dialog.set_open(true, win, cx));
        self.goal_input.read(cx).focus_handle(cx).focus(win, cx);
    }

    pub(super) fn submit_chat_goal(
        &mut self,
        objective: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.can_set_chat_goal() || self.queued_prompt.is_some() || self.slash.creating_goal {
            self.notify_warning(
                self.ui_locale.pick(
                    "当前会话无法设置目标",
                    "Goals are unavailable in this session",
                ),
                cx,
            );
            return false;
        }
        if matches!(self.access_mode, ChatAccessMode::NoneSelected) {
            if !self.create_real_chat_with_goal(Some(objective), cx) {
                return false;
            }
        } else if let ChatAccessMode::WriteController { chat_id } = self.access_mode {
            self.bridge.send(KarCommand::SetGoal { chat_id, objective });
        }
        if args(self.composer_editor.read(cx).value().trim(), "/goal").is_some() {
            self.composer_editor
                .update(cx, |ed, cx| ed.set_value("", window, cx));
        }
        cx.notify();
        true
    }

    pub(super) fn submit_slash_command(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !text.starts_with('/') {
            return false;
        }
        let token = text.split_whitespace().next().unwrap_or_default();
        let choice = self
            .slash_choices(token)
            .into_iter()
            .find(|row| row.name == token);
        match choice.map(|c| c.action) {
            Some(Command::Goal) => {
                let objective = args(text, "/goal").unwrap_or_default();
                if !self.can_set_chat_goal() {
                    self.notify_warning(
                        self.ui_locale.pick(
                            "当前会话不支持目标",
                            "Goals are unavailable in this session",
                        ),
                        cx,
                    );
                } else if objective.is_empty() {
                    self.choose_slash(Command::Goal, window, cx);
                } else {
                    self.submit_chat_goal(objective.into(), window, cx);
                }
                true
            }
            Some(Command::PlanDraft) => {
                self.fill_plan_request(args(text, "/plan").unwrap_or_default(), window, cx);
                true
            }
            _ if text == "/" => {
                self.open_slash_picker(window, cx);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn navigate_slash_picker(
        &mut self,
        ev: &KeyDownEvent,
        win: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = self.composer_editor.read(cx);
        let mods = ev.keystroke.modifiers;
        if !editor.focus_handle(cx).is_focused(win)
            || editor.is_composing()
            || mods.shift
            || mods.platform
            || mods.control
            || mods.alt
        {
            return;
        }
        let Some(query) = self.slash.query(editor.value()) else {
            return;
        };
        let choices = self.slash_choices(query);
        if choices.is_empty() {
            if query == "/" && ev.keystroke.key == "escape" {
                self.slash.close();
                win.prevent_default();
                cx.stop_propagation();
                cx.notify();
            }
            return;
        }
        let index = self.slash.index.min(choices.len() - 1);
        match ev.keystroke.key.as_str() {
            "down" => self.slash.index = (index + 1) % choices.len(),
            "up" => self.slash.index = (index + choices.len() - 1) % choices.len(),
            "escape" => self.slash.close(),
            "enter" | "tab" => {
                if choices[index].enabled {
                    self.choose_slash(choices[index].action.clone(), win, cx);
                } else {
                    self.notify_warning(choices[index].description.clone(), cx);
                }
            }
            _ => return,
        }
        self.slash.scroll.scroll_to_item(self.slash.index);
        win.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn slash_popup(
        &self,
        width: f32,
        s: &StudioPalette,
        win: &Window,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let editor = self.composer_editor.read(cx);
        let below =
            self.projected.turns.is_empty() && f32::from(win.viewport_size().height) >= 530.;
        if !editor.focus_handle(cx).is_focused(win) || self.open_menu.is_some() {
            return None;
        }
        let query = self.slash.query(editor.value())?;
        let choices = self.slash_choices(query);
        if choices.is_empty() && query != "/" {
            return None;
        }
        let index = self.slash.index.min(choices.len().saturating_sub(1));
        let mut menu = div()
            .id("composer-slash-picker")
            .role(Role::ListBox)
            .aria_label(self.ui_locale.pick("对话指令", "Chat commands"))
            .w(px((width - 12.).clamp(120., 350.)))
            .p(px(4.))
            .max_h(px(
                (f32::from(win.viewport_size().height) * 0.38).clamp(70., 230.)
            ))
            .overflow_y_scroll()
            .track_scroll(&self.slash.scroll)
            .occlude()
            .flex()
            .flex_col()
            .rounded(px(R_CARD))
            .bg(s.card)
            .border_1()
            .border_color(s.line)
            .shadow(s.popover_shadow());
        if choices.is_empty() {
            menu = menu.child(
                div()
                    .px(px(9.))
                    .py(px(7.))
                    .text_size(px(FS_META))
                    .text_color(s.muted)
                    .child(self.ui_locale.pick(
                        "智能体尚未提供指令列表，可直接输入原生指令。",
                        "The agent hasn’t reported its commands. You can still type one.",
                    )),
            );
        }
        for (i, choice) in choices.into_iter().enumerate() {
            let enabled = choice.enabled;
            menu = menu.child(
                div()
                    .id(("slash-choice", i))
                    .role(Role::ListBoxOption)
                    .aria_label(format!("{} · {}", choice.name, choice.description))
                    .aria_selected(i == index)
                    .flex_none()
                    .flex()
                    .w_full()
                    .items_center()
                    .h(px(34.))
                    .px(px(9.))
                    .gap(px(12.))
                    .rounded(px(5.))
                    .when(i == index, |d| d.bg(s.soft))
                    .when(enabled, |d| d.cursor_pointer().hover(|st| st.bg(s.hover)))
                    .child(
                        div()
                            .text_size(px(FS_UI))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if enabled { s.ink } else { s.muted })
                            .child(choice.name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(FS_META))
                            .text_color(s.muted)
                            .child(choice.description),
                    )
                    .on_mouse_down(MouseButton::Left, move |_, win, cx| {
                        win.prevent_default();
                        cx.stop_propagation();
                    })
                    .when(enabled, |d| {
                        d.on_click(cx.listener(move |this, _, win, cx| {
                            this.choose_slash(choice.action.clone(), win, cx)
                        }))
                    }),
            );
        }
        Some(
            div()
                .absolute()
                .left(px(3.))
                .map(|d| {
                    if below {
                        d.top(relative(1.))
                    } else {
                        d.top_0()
                    }
                })
                .size_0()
                .child(
                    deferred(
                        anchored()
                            .anchor(if below {
                                Anchor::TopLeft
                            } else {
                                Anchor::BottomLeft
                            })
                            .offset(point(px(0.), px(if below { 5. } else { -5. })))
                            .snap_to_window_with_margin(Edges::all(px(8.)))
                            .child(menu),
                    )
                    .with_priority(3),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, SlashCommandItem, SlashPicker, UiLocale, args, command_choices};

    #[test]
    fn advertised_commands_keep_native_semantics() {
        let native = ["/goal", "/plan"].map(|name| SlashCommandItem {
            name: name.into(),
            description: "From provider".into(),
            hint: None,
            source: "runtime".into(),
        });
        let actions = |goal, inventory: &[SlashCommandItem]| {
            command_choices(goal, inventory, true, "/", UiLocale::En)
                .into_iter()
                .map(|c| c.action)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            actions(false, &native),
            vec![
                Command::Native("/goal".into()),
                Command::Native("/plan".into())
            ]
        );
        assert_eq!(
            actions(true, &native),
            vec![Command::Goal, Command::Native("/plan".into())]
        );
        assert_eq!(actions(true, &[]), vec![Command::Goal, Command::PlanDraft]);
        assert!(command_choices(false, &[], false, "/plan", UiLocale::En).is_empty());
        assert!(command_choices(false, &[], false, "/goal", UiLocale::En).is_empty());
        assert!(!command_choices(false, &[], true, "/goal", UiLocale::En)[0].enabled);
    }

    #[test]
    fn command_boundaries_preserve_paths_and_multiline_arguments() {
        let picker = SlashPicker::default();
        assert_eq!(picker.query("/pl"), Some("/pl"));
        assert_eq!(picker.query("/tmp/plan"), None);
        assert_eq!(picker.query("note /plan"), None);
        assert_eq!(args("/goal/path", "/goal"), None);
        assert_eq!(args("/planner", "/plan"), None);
        assert_eq!(
            args(
                "/plan  path with spaces
line 2",
                "/plan"
            ),
            Some(
                "path with spaces
line 2"
            )
        );
    }
}
