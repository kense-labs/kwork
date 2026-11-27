//! Composer commands use the active runtime's inventory and the local chat controller.
use super::{DraftLocation, KarWorkbench, QueuedPrompt, slash_actions::LocalSlash};
use crate::kar_bridge::KarCommand;
use crate::studio_hub::SlashCommandItem;
use crate::types::ChatAccessMode;
use crate::ui_primitives::UiLocale;
use crate::ui_primitives::{
    FS_META, FS_UI, HEADER_H, ICON_SM, R_CTRL, StudioPalette, frosted_panel,
};
use gpui::{prelude::*, *};
use kui::primitives::pro_icons::{ProIcon, pro_icon, pro_svg};

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
    PlanMode,
    Local(LocalSlash),
    Native(String),
}

struct Choice {
    name: String,
    label: String,
    description: String,
    icon: ProIcon,
    aliases: &'static [&'static str],
    action: Command,
}

impl Choice {
    fn accepts_token(&self, token: &str) -> bool {
        matches_command(token, &self.name, self.aliases)
    }

    fn search_rank(&self, token: &str) -> Option<u8> {
        if self.accepts_token(token) {
            return Some(0);
        }
        if self.name.to_lowercase().starts_with(token)
            || self.aliases.iter().any(|name| name.starts_with(token))
        {
            return Some(1);
        }
        let term = token.strip_prefix('/')?;
        self.label.to_lowercase().contains(term).then_some(2)
    }
}

fn matches_command(token: &str, name: &str, aliases: &[&str]) -> bool {
    name.eq_ignore_ascii_case(token) || aliases.iter().any(|name| name.eq_ignore_ascii_case(token))
}

fn is_desktop_command(token: &str) -> bool {
    token.eq_ignore_ascii_case("/goal") || token.eq_ignore_ascii_case("/plan")
        || LOCAL
            .iter()
            .any(|entry| matches_command(token, entry.name, entry.aliases))
}

struct LocalEntry {
    action: LocalSlash,
    name: &'static str,
    labels: (&'static str, &'static str),
    descriptions: (&'static str, &'static str),
    icon: ProIcon,
    aliases: &'static [&'static str],
}

impl LocalEntry {
    const fn new(
        action: LocalSlash,
        name: &'static str,
        labels: (&'static str, &'static str),
        descriptions: (&'static str, &'static str),
        icon: ProIcon,
        aliases: &'static [&'static str],
    ) -> Self {
        Self {
            action,
            name,
            labels,
            descriptions,
            icon,
            aliases,
        }
    }

    fn choice(&self, loc: UiLocale) -> Choice {
        Choice {
            name: self.name.into(),
            label: loc.pick(self.labels.0, self.labels.1).into(),
            description: loc.pick(self.descriptions.0, self.descriptions.1).into(),
            icon: self.icon,
            aliases: self.aliases,
            action: Command::Local(self.action),
        }
    }
}

const LOCAL: &[LocalEntry] = &[
    LocalEntry::new(
        LocalSlash::Model,
        "/model",
        ("模型", "Model"),
        (
            "查看会话模型或选择新对话的模型",
            "View the session model or choose one for a new chat",
        ),
        ProIcon::Sliders,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Reasoning,
        "/reasoning",
        ("推理深度", "Reasoning effort"),
        ("查看或选择推理档位", "View or choose the reasoning effort"),
        ProIcon::Sparkles,
        &["/effort"],
    ),
    LocalEntry::new(
        LocalSlash::Permissions,
        "/permissions",
        ("审批权限", "Permissions"),
        (
            "查看会话权限或设置新对话默认值",
            "View session permissions or set defaults for new chats",
        ),
        ProIcon::Shield,
        &["/approval", "/approvals"],
    ),
    LocalEntry::new(
        LocalSlash::Status,
        "/status",
        ("会话状态", "Session status"),
        (
            "查看会话 ID、智能体与上下文用量",
            "View the chat ID, agent and context usage",
        ),
        ProIcon::Activity,
        &["/context"],
    ),
    LocalEntry::new(
        LocalSlash::New,
        "/new",
        ("新对话", "New chat"),
        ("在当前工作区开始一段对话", "Start a chat in this workspace"),
        ProIcon::NewChat,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Rename,
        "/rename",
        ("重命名", "Rename"),
        ("修改当前对话的名称", "Rename this chat"),
        ProIcon::Note,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Pin,
        "/pin",
        ("置顶对话", "Pin chat"),
        (
            "在侧栏置顶这段对话",
            "Keep this chat at the top of the sidebar",
        ),
        ProIcon::Pin,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Unpin,
        "/unpin",
        ("取消置顶", "Unpin chat"),
        (
            "将对话移回侧栏列表",
            "Return this chat to its usual position",
        ),
        ProIcon::Pin,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Fork,
        "/fork",
        ("创建分支", "Fork chat"),
        ("从已完成的回合创建对话分支", "Branch from a completed turn"),
        ProIcon::GitBranch,
        &["/branch"],
    ),
    LocalEntry::new(
        LocalSlash::Compact,
        "/compact",
        ("压缩上下文", "Compact context"),
        (
            "请求智能体整理当前会话的上下文",
            "Ask the agent to compact this chat’s context",
        ),
        ProIcon::Memory,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Stop,
        "/stop",
        ("停止执行", "Stop generation"),
        ("停止当前正在执行的回合", "Stop the turn that is running"),
        ProIcon::Close,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Review,
        "/review",
        ("审查改动", "Review changes"),
        (
            "查看最近一次文件改动的差异",
            "View the diff for the latest file change",
        ),
        ProIcon::Code,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Diff,
        "/diff",
        ("Git 变更", "Git changes"),
        (
            "查看工作区差异与提交历史",
            "Open workspace changes and commit history",
        ),
        ProIcon::GitBranch,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Attach,
        "/attach",
        ("添加附件", "Attach files"),
        (
            "选择要随消息发送的文件",
            "Choose files to attach to a message",
        ),
        ProIcon::Paperclip,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Files,
        "/files",
        ("工作区文件", "Workspace files"),
        ("浏览当前工作区的文件", "Browse the workspace file tree"),
        ProIcon::Folder,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Terminal,
        "/terminal",
        ("终端", "Terminal"),
        ("打开工作区终端", "Open the workspace terminal"),
        ProIcon::Terminal,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Search,
        "/search",
        ("搜索对话", "Search chats"),
        ("查找对话和消息", "Find conversations and messages"),
        ProIcon::Search,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Tasks,
        "/tasks",
        ("任务收件箱", "Task inbox"),
        ("打开任务与待办列表", "Open the task inbox"),
        ProIcon::Inbox,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Skills,
        "/skills",
        ("技能", "Skills"),
        (
            "查看工作区中的智能体技能",
            "Browse skills in this workspace",
        ),
        ProIcon::Tasks,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Handoff,
        "/handoff",
        ("交接对话", "Hand off chat"),
        (
            "将当前对话交接给其他智能体",
            "Hand this chat over to another agent",
        ),
        ProIcon::ExternalArrow,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::SideQuestion,
        "/side",
        ("旁路提问", "Side question"),
        (
            "针对这段对话另行提问",
            "Ask a side question about this chat",
        ),
        ProIcon::Thread,
        &["/side-question"],
    ),
    LocalEntry::new(
        LocalSlash::Share,
        "/share",
        ("共享设置", "Sharing settings"),
        (
            "查看本机共享链接与配对设置",
            "View sharing links and pairing settings for this device",
        ),
        ProIcon::Link,
        &[],
    ),
    LocalEntry::new(
        LocalSlash::Delete,
        "/delete",
        ("删除对话", "Delete chat"),
        ("确认后删除此对话", "Confirm before deleting this chat"),
        ProIcon::Trash,
        &[],
    ),
];

fn args<'a>(text: &'a str, command: &str) -> Option<&'a str> {
    let token = text.split_whitespace().next()?;
    token
        .eq_ignore_ascii_case(command)
        .then(|| text[token.len()..].trim_start())
}

fn command_choices(
    goal: bool,
    native: &[SlashCommandItem],
    planning: bool,
    query: &str,
    loc: UiLocale,
    available: impl Fn(LocalSlash) -> bool,
) -> Vec<Choice> {
    let mut choices = Vec::new();
    let advertised = |name: &str| native.iter().any(|cmd| cmd.name.eq_ignore_ascii_case(name));
    if goal {
        choices.push(Choice {
            name: "/goal".into(),
            label: loc.pick("目标", "Goal").into(),
            description: loc.pick("查看、编辑和管理会话目标", "View, edit and manage the session goal").into(),
            action: Command::Goal,
            icon: ProIcon::Tasks,
            aliases: &[],
        });
    }
    if planning && !advertised("/plan") {
        choices.push(Choice {
            name: "/plan".into(),
            label: loc.pick("计划", "Plan").into(),
            description: loc.pick("先在只读模式规划，确认后实施", "Plan in read-only mode, then choose whether to implement").into(),
            action: Command::PlanMode,
            icon: ProIcon::Tasks,
            aliases: &[],
        });
    }
    for entry in LOCAL {
        if available(entry.action) && !advertised(entry.name) {
            choices.push(entry.choice(loc));
        }
    }
    for cmd in native {
        if choices
            .iter()
            .any(|c| c.name.eq_ignore_ascii_case(&cmd.name))
        {
            continue;
        }
        let hint = cmd.hint.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let loc_desc = if cmd.description.is_empty() {
            String::new()
        } else {
            crate::studio_hub::localized_command_description(&cmd.name, &cmd.description, loc)
        };
        let description = match (hint, loc_desc.is_empty()) {
            (Some(hint), false) => format!("{hint} · {loc_desc}"),
            (Some(hint), true) => hint.to_owned(),
            (None, _) => loc_desc,
        };
        choices.push(Choice {
            name: cmd.name.clone(),
            label: cmd.name.clone(),
            description,
            action: Command::Native(cmd.name.clone()),
            icon: ProIcon::Terminal,
            aliases: &[],
        });
    }
    if query != "/" {
        // An unavailable /pin must never complete to “Unpin chat” on Enter.
        if is_desktop_command(query) && !choices.iter().any(|choice| choice.accepts_token(query)) {
            return Vec::new();
        }
        let token = query.to_lowercase();
        let mut matches: Vec<_> = choices
            .into_iter()
            .filter_map(|choice| choice.search_rank(&token).map(|rank| (rank, choice)))
            .collect();
        // Exact spellings win over search aliases, including native commands named like one.
        matches.sort_by_key(|(rank, choice)| (*rank, !choice.name.eq_ignore_ascii_case(&token)));
        return matches.into_iter().map(|(_, choice)| choice).collect();
    }
    let frequent = [
        "/model",
        "/reasoning",
        "/permissions",
        "/new",
        "/status",
        "/goal",
        "/plan",
    ];
    choices.sort_by_key(|choice| {
        frequent
            .iter()
            .position(|name| choice.name.eq_ignore_ascii_case(name))
            .unwrap_or(frequent.len())
    });
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
        let goal =
            self.can_set_chat_goal() && !self.slash.creating_goal && self.queued_prompt.is_none();
        let session = self.session_commands.as_ref().filter(|(id, _)| {
            self.projected.chat_id == Some(*id)
                && matches!(self.access_mode, ChatAccessMode::WriteController { chat_id } if chat_id == *id)
        });
        let native = match (&self.access_mode, session) {
            (_, Some((_, commands))) => commands.as_slice(),
            (ChatAccessMode::NoneSelected | ChatAccessMode::WriteController { .. }, None)
                if (self.hosted_writer.is_some() || matches!(self.access_mode,ChatAccessMode::NoneSelected))
                    && crate::aster::is_aster(self.active_or_selected_runtime_id()) => {
                let workspace = self.effective_workspace_dir();
                self.providers.iter().find(|p| p.id == crate::aster::ID)
                    .and_then(|p| p.workspace_commands.as_ref())
                    .filter(|(owner, _)| *owner == workspace)
                    .map_or(&[][..], |(_, commands)| commands.as_slice())
            }
            (ChatAccessMode::WriteController { .. } | ChatAccessMode::NoneSelected, None) =>
                self.workspace_inventory.advertised_commands_for(&self.effective_workspace_dir(), self.active_or_selected_runtime_id()),
            _ => &[],
        };
        let aster_plan = self.supports_aster_planning();
        let mut choices = command_choices(goal, native, self.supports_planning() || aster_plan, query, self.ui_locale, |action| {
            self.slash_action_available(action)
        });
        if aster_plan && let Some(plan) = choices.iter_mut().find(|choice| choice.action == Command::PlanMode) {
            plan.description = self.ui_locale.pick("切换 Aster 原生计划模式", "Switch to Aster's native plan mode").into();
        }
        choices
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
        // A provider update or a chat switch can invalidate the row between paint and click.
        if !self.slash_choices("/").iter().any(|row| row.action == cmd) {
            self.slash.close();
            self.warn_unavailable_slash(cx);
            return;
        }
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
            Command::PlanMode => self.enter_planning(existing, false, window, cx),
            Command::Local(action) => {
                if existing.is_empty() {
                    self.composer_editor
                        .update(cx, |ed, cx| ed.set_value("", window, cx));
                }
                self.run_slash_action(action, window, cx);
            }
            Command::Native(name) => {
                self.composer_editor.update(cx, |ed, cx| {
                    ed.set_value(format!("{name} {existing}"), window, cx);
                    ed.focus(window, cx);
                });
            }
        }
        cx.notify();
    }

    fn warn_unavailable_slash(&mut self, cx: &mut Context<Self>) {
        self.notify_warning(
            self.ui_locale.pick(
                "当前会话无法执行此命令。输入 / 可查看可用命令。",
                "This command is unavailable in this chat. Type / to see available commands.",
            ),
            cx,
        );
    }


    pub(super) fn confirm_chat_goal(&mut self, win: &mut Window, cx: &mut Context<Self>) {
        let Some(owner) = self.goal_dialog_owner.clone() else {
            return;
        };
        let objective = self.goal_input.read(cx).value(cx).trim().to_owned();
        let budget = self.goal_budget_input.read(cx).value(cx);
        let same_owner = DraftLocation::for_mode(&self.access_mode, &self.config.workspace_dir) == owner;
        let token_budget = if budget.trim().is_empty() || (same_owner && !self.goal_supports_token_budget()) { None } else {
            match super::chat_goal::parse_budget(&budget) {
                Ok(tokens) => Some(tokens),
                Err(()) => {
                    self.notify_warning(self.ui_locale.pick("Token 预算须为正整数（最大 9007199254740991）。", "Enter a positive token budget up to 9007199254740991."), cx);
                    self.goal_dialog.update(cx, |dialog, cx| dialog.set_open(true, win, cx));
                    self.goal_budget_input.read(cx).focus_handle(cx).focus(win, cx);
                    return;
                }
            }
        };
        let edit = crate::kar_bridge::GoalEdit {objective, token_budget};
        if !same_owner {
            if !edit.objective.is_empty() {
                self.prepend_composer_draft(
                    owner,
                    QueuedPrompt {
                        text: edit.command(),
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
        if !edit.objective.is_empty() && self.submit_chat_goal(edit, win, cx) {
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
        objective: crate::kar_bridge::GoalEdit,
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
            if !self.request_chat_goal(chat_id, crate::kar_bridge::GoalAction::Set(objective), cx) {
                return false;
            }
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
            .find(|row| row.accepts_token(token));
        match choice.map(|c| c.action) {
            Some(Command::Goal) => {
                let objective = args(text, "/goal").unwrap_or_default();
                if objective.is_empty() {
                    self.choose_slash(Command::Goal, window, cx);
                } else {
                    self.handle_goal_command(objective, window, cx);
                }
                true
            }
            Some(Command::PlanMode) => {
                self.enter_planning(args(text, "/plan").unwrap_or_default(), true, window, cx);
                true
            }
            Some(cmd @ Command::Local(_)) => {
                if text[token.len()..].trim().is_empty() {
                    self.choose_slash(cmd, window, cx);
                } else {
                    self.notify_warning(
                        self.ui_locale.pick(
                            "此命令不接收参数，请仅输入命令后再执行。",
                            "This command takes no arguments. Run it on its own.",
                        ),
                        cx,
                    );
                }
                true
            }
            _ if text == "/" => {
                self.open_slash_picker(window, cx);
                true
            }
            None if is_desktop_command(token) => {
                self.warn_unavailable_slash(cx);
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
            if ev.keystroke.key == "escape" {
                self.slash.close();
                win.prevent_default();
                cx.stop_propagation();
                cx.notify();
            }
            return;
        }
        let index = self.slash.index.min(choices.len() - 1);
        match ev.keystroke.key.as_str() {
            "down" | "up" => {
                let n = choices.len();
                let down = ev.keystroke.key == "down";
                self.slash.index = if down {
                    (index + 1) % n
                } else {
                    (index + n - 1) % n
                };
            }
            "escape" => self.slash.close(),
            "enter" | "tab" => {
                self.choose_slash(choices[index].action.clone(), win, cx);
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
        s: &StudioPalette,
        win: &Window,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let editor = self.composer_editor.read(cx);
        if !editor.focus_handle(cx).is_focused(win) || self.open_menu.is_some() {
            return None;
        }
        let query = self.slash.query(editor.value())?;
        let choices = self.slash_choices(query);
        if choices.is_empty() && query != "/" {
            return None;
        }
        let selected = self.slash.index.min(choices.len().saturating_sub(1));
        let mut menu = frosted_panel(s)
            .id("composer-slash-picker")
            .role(Role::ListBox)
            .aria_label(self.ui_locale.pick("对话指令", "Chat commands"))
            .p(px(4.))
            .overflow_y_scroll()
            .track_scroll(&self.slash.scroll)
            .occlude()
            .flex()
            .flex_col();
        if choices.is_empty() {
            menu = menu.child(
                div()
                    .px(px(9.))
                    .py(px(7.))
                    .text_size(s.font_size(FS_META))
                    .text_color(s.muted)
                    .child(self.ui_locale.pick(
                        "智能体尚未提供指令列表，可直接输入原生指令。",
                        "The agent hasn’t reported its commands. You can still type one.",
                    )),
            );
        }
        for (i, choice) in choices.into_iter().enumerate() {
            let label = choice.label;
            let alias = (label != choice.name).then(|| choice.name.clone());
            let icon = if choice.action == Command::Goal {
                pro_svg(
                    "0 0 24 24",
                    r#"<g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M20.6 10.1a9 9 0 1 1-6.7-6.7M16.8 11a5 5 0 1 1-3.8-3.8M12 12l8-8M17 4h3v3"/></g>"#,
                    ICON_SM,
                    s.ink,
                )
            } else {
                pro_icon(choice.icon, ICON_SM, s.ink)
            };
            let spoken = format!("{} · {}", choice.name, choice.description);
            menu = menu.child(
                div()
                    .id(("slash-choice", i))
                    .role(Role::ListBoxOption)
                    .aria_label(spoken)
                    .aria_selected(i == selected)
                    .flex_none()
                    .flex()
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .h(px(crate::ui_primitives::CTRL_H))
                    .px(px(10.))
                    .gap(px(8.))
                    .rounded(px(R_CTRL))
                    .when(i == selected, |d| d.bg(s.hover))
                    .hover(|d| d.bg(s.hover))
                    .cursor_pointer()
                    .child(icon)
                    .child(
                        div()
                            .flex_none()
                            .max_w(relative(0.44))
                            .truncate()
                            .text_size(s.font_size(FS_UI))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(s.font_size(FS_META))
                            .text_color(s.muted)
                            .child(choice.description),
                    )
                    .when_some(alias, |row, alias| {
                        row.child(
                            div()
                                .flex_none()
                                .text_size(s.font_size(FS_META))
                                .text_color(s.muted)
                                .child(alias),
                        )
                    })
                    .on_mouse_down(MouseButton::Left, |_, win, cx| {
                        // Preserve the editor focus so pointer selection works, including with an IME.
                        win.prevent_default();
                        cx.stop_propagation();
                    })
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.slash.index != i {
                            this.slash.index = i;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, win, cx| {
                        this.choose_slash(choice.action.clone(), win, cx);
                        cx.stop_propagation();
                    })),
            );
        }

        // Size against the actual writing card after layout. This remains accurate
        // in the centered start page, in a split pane and above a growing draft.
        Some(
            deferred(
                canvas(
                    move |anchor, win, cx| {
                        let bottom = anchor.top() - px(4.);
                        let max_height = (bottom - px(HEADER_H + 4.)).max(px(0.)).min(px(360.));
                        let mut menu = menu
                            .w(anchor.size.width)
                            .max_h(max_height)
                            .into_any_element();
                        let space = size(
                            AvailableSpace::Definite(anchor.size.width),
                            AvailableSpace::MaxContent,
                        );
                        let dimensions = menu.layout_as_root(space, win, cx);
                        menu.prepaint_at(point(anchor.left(), bottom - dimensions.height), win, cx);
                        menu
                    },
                    |_, mut menu, win, cx| menu.paint(win, cx),
                )
                .absolute()
                .left_0()
                .right_0()
                .top_0()
                .h(px(0.)),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Command, LocalSlash, SlashCommandItem, SlashPicker, UiLocale, args, command_choices,
        is_desktop_command,
    };

    #[test]
    fn advertised_commands_keep_native_semantics() {
        let mut native =
            ["/goal", "/plan", "/model", "/MODEL", "/context"].map(|name| SlashCommandItem {
                name: name.into(),
                description: "From provider".into(),
                hint: None,
                source: "runtime".into(),
            });
        native[2].hint = Some("<name>".into());
        let choices =
            |goal, query| command_choices(goal, &native, true, query, UiLocale::En, |_| true);
        assert_eq!(
            choices(false, "/goal")[0].action,
            Command::Native("/goal".into())
        );
        assert_eq!(choices(true, "/goal")[0].action, Command::Goal);
        assert_eq!(
            choices(true, "/plan")[0].action,
            Command::Native("/plan".into())
        );
        let model = choices(false, "/model");
        assert_eq!(model.len(), 1);
        assert_eq!(model[0].action, Command::Native("/model".into()));
        assert_eq!(model[0].description, "<name> · From provider");
        assert_eq!(
            choices(false, "/context")[0].action,
            Command::Native("/context".into())
        );
        assert!(command_choices(false, &[], false, "/goal", UiLocale::En, |_| false).is_empty());
        assert!(command_choices(false, &[], false, "/plan", UiLocale::En, |_| false).is_empty());
        let partial = command_choices(false, &native, false, "/plan", UiLocale::En, |_| false);
        assert_eq!(partial[0].action, Command::Native("/plan".into()));
        assert_eq!(
            command_choices(true, &[], true, "/plan", UiLocale::En, |_| false)[0].action,
            Command::PlanMode
        );
    }

    #[test]
    fn slash_catalog_uses_action_availability_and_localized_search() {
        let available = |a| {
            matches!(
                a,
                LocalSlash::Model | LocalSlash::Reasoning | LocalSlash::Pin
            )
        };
        let choices = |query| command_choices(false, &[], true, query, UiLocale::Zh, available);
        assert_eq!(choices("/置顶")[0].action, Command::Local(LocalSlash::Pin));
        assert_eq!(
            choices("/EFFORT")[0].action,
            Command::Local(LocalSlash::Reasoning)
        );
        assert!(choices("/compact").is_empty());
        assert!(
            command_choices(false, &[], true, "/pin", UiLocale::En, |a| a
                == LocalSlash::Unpin)
            .is_empty()
        );
        let names = choices("/")
            .into_iter()
            .map(|row| row.name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["/model", "/reasoning", "/plan", "/pin"]);
        let pending = command_choices(false, &[], false, "/model", UiLocale::Zh, available);
        assert_eq!(pending[0].action, Command::Local(LocalSlash::Model));
    }

    #[test]
    fn command_boundaries_preserve_paths_and_multiline_arguments() {
        let picker = SlashPicker::default();
        assert_eq!(picker.query("/pl"), Some("/pl"));
        assert_eq!(picker.query("/tmp/plan"), None);
        assert_eq!(picker.query("note /plan"), None);
        assert_eq!(args("/goal/path", "/goal"), None);
        assert_eq!(args("/planner", "/plan"), None);
        assert!(is_desktop_command("/PIN"));
        assert!(is_desktop_command("/context"));
        assert!(is_desktop_command("/goal"));
        assert!(!is_desktop_command("/tmp/plan"));
        assert!(!is_desktop_command("/my-provider-command"));
        // Only run a plan mode that the provider or this desktop supports.
        assert!(is_desktop_command("/plan"));
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
