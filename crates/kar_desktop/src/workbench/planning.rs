//! A local Codex plan is reviewed in its owning chat. Only approval dispatches an implementation turn.
mod resume;
mod view;
use super::*;
use crate::kar_bridge::PlanQuestion;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Phase {
    Draft,
    Working,
    Ready,
    Interrupted,
    Saved,
    Submitted,
}

#[derive(Clone, Serialize, Deserialize)]
struct Plan {
    id: Uuid,
    topic: String,
    proposal: String,
    phase: Phase,
    error: Option<String>,
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            topic: String::new(),
            proposal: String::new(),
            phase: Phase::Draft,
            error: None,
        }
    }
}

struct Clarification {
    questions: Vec<PlanQuestion>,
    answers: BTreeMap<String, String>,
}
impl Clarification {
    fn next(&self) -> Option<&PlanQuestion> {
        self.questions
            .iter()
            .find(|q| !self.answers.contains_key(&q.id))
    }
}

#[derive(Default)]
pub(super) struct Planning {
    by_chat: BTreeMap<Uuid, Plan>,
    workspace_ids: std::cell::RefCell<BTreeMap<PathBuf, Uuid>>,
    approval_lock: Option<std::fs::File>,

    creating: Option<(Uuid, PathBuf, Uuid)>,
    questions: BTreeMap<Uuid, Clarification>,
    expanded: BTreeSet<Uuid>,
    progress: BTreeMap<Uuid, String>,
    dispatching: BTreeSet<Uuid>,
}

impl Planning {
    pub(super) fn observe_attempt(&mut self, attempt: &PendingPromptAttempt) {
        if !attempt.in_flight {
            self.dispatching.remove(&attempt.request_id);
        }
    }
}

impl KarWorkbench {
    pub(super) fn supports_planning(&self) -> bool {
        match self.access_mode {
            ChatAccessMode::NoneSelected => {
                self.selected_runtime == "codex"
                    && self
                        .providers
                        .iter()
                        .any(|p| p.id == "codex" && p.is_ready())
            }
            ChatAccessMode::WriteController { chat_id } => {
                self.projected.chat_id == Some(chat_id)
                    && self
                        .current_chat_info()
                        .is_some_and(|i| i.protocol == kar::Protocol::Codex)
                    && !self.turn_in_progress()
            }
            _ => false,
        }
    }

    fn workspace_plan_id(&self) -> Uuid {
        // Kar chat IDs are random v4 IDs. A workspace owns one durable v5 planning draft.
        let directory = &self.config.workspace_dir;
        *self
            .planning
            .workspace_ids
            .borrow_mut()
            .entry(directory.clone())
            .or_insert_with(|| {
                let path = directory
                    .canonicalize()
                    .or_else(|_| std::path::absolute(directory))
                    .unwrap_or_else(|_| directory.clone());
                Uuid::new_v5(&Uuid::NAMESPACE_URL, path.as_os_str().as_encoded_bytes())
            })
    }

    fn plan_owner(&self) -> Option<Uuid> {
        match self.access_mode {
            ChatAccessMode::NoneSelected => Some(self.workspace_plan_id()),
            _ => self.current_chat_id(),
        }
    }

    fn current_plan(&self) -> Option<&Plan> {
        self.plan_owner()
            .and_then(|id| self.planning.by_chat.get(&id))
    }

    pub(crate) fn planning_here(&self) -> bool {
        let editable = matches!(self.access_mode, ChatAccessMode::WriteController { .. })
            || matches!(self.access_mode, ChatAccessMode::NoneSelected)
                && self.selected_runtime == "codex";
        editable
            && self
                .current_plan()
                .is_some_and(|p| !matches!(p.phase, Phase::Saved | Phase::Submitted))
    }

    pub(super) fn restore_workspace_plan(&mut self, cx: &mut Context<Self>) -> bool {
        let owner = self.workspace_plan_id();
        self.load_saved_plan(owner, cx);
        if !self
            .planning
            .by_chat
            .get(&owner)
            .is_some_and(|p| !matches!(p.phase, Phase::Saved | Phase::Submitted))
        {
            return false;
        }
        self.selected_runtime = "codex".into();
        self.sync_plan_placeholder(cx);
        cx.notify();
        true
    }

    pub(crate) fn plan_input_blocked(&self) -> bool {
        self.planning_here()
            && (self.planning.creating.as_ref().is_some_and(|(_, path, _)| {
                matches!(self.access_mode, ChatAccessMode::NoneSelected)
                    && path == &self.config.workspace_dir
            }) || self.current_plan().is_some_and(|p| {
                p.phase == Phase::Working && !self.planning.questions.contains_key(&p.id)
            }))
    }

    fn plan_file(&self, chat: Uuid) -> PathBuf {
        self.config
            .chat_dir
            .join("desktop-plans")
            .join(format!("{chat}.json"))
    }

    pub(super) fn load_saved_plan(&mut self, chat: Uuid, cx: &mut Context<Self>) {
        if self.planning.by_chat.contains_key(&chat) {
            return;
        }
        let bytes = match std::fs::read(self.plan_file(chat)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                self.notify_warning(
                    format!(
                        "{} · {e}",
                        self.ui_locale.pick("计划读取失败", "Could not load plan")
                    ),
                    cx,
                );
                return;
            }
        };
        match serde_json::from_slice::<Plan>(&bytes) {
            Ok(mut p) => {
                if p.phase == Phase::Working {
                    // The temporary planning thread belonged to a previous process.
                    p.phase = Phase::Interrupted;
                    p.error = None;
                }
                if p.phase == Phase::Ready {
                    self.planning.expanded.insert(chat);
                }
                self.planning.by_chat.insert(chat, p);
            }
            Err(e) => self.notify_warning(
                format!(
                    "{} · {e}",
                    self.ui_locale.pick("计划读取失败", "Could not load plan")
                ),
                cx,
            ),
        }
    }

    fn save_plan(&mut self, chat: Uuid, plan: Plan, cx: &mut Context<Self>) -> bool {
        let file = self.plan_file(chat);
        let save = || -> std::io::Result<()> {
            std::fs::create_dir_all(file.parent().expect("plan directory"))?;
            kantor_core::fsx::write_atomic(&file, &serde_json::to_vec(&plan)?)
        };
        if let Err(e) = save() {
            self.notify_warning(
                format!(
                    "{} · {e}",
                    self.ui_locale.pick("计划保存失败", "Could not save plan")
                ),
                cx,
            );
            return false;
        }
        self.planning.by_chat.insert(chat, plan);
        cx.notify();
        true
    }

    pub(super) fn sync_plan_placeholder(&self, cx: &mut Context<Self>) {
        let loc = self.ui_locale;
        let label = if !self.planning_here() {
            self.composer_placeholder_for_mode(&self.access_mode)
        } else if self
            .current_plan()
            .is_some_and(|p| self.planning.questions.contains_key(&p.id))
        {
            loc.pick("输入你的回答…", "Type your answer…")
        } else if self.current_plan().is_some_and(|p| !p.proposal.is_empty()) {
            loc.pick("想调整哪里？", "What should change in the plan?")
        } else {
            loc.pick("描述你想规划的任务…", "What would you like to plan?")
        };
        self.composer_editor.update(cx, |ed, cx| {
            ed.set_placeholder(label);
            cx.notify();
        });
    }

    pub(super) fn enter_planning(
        &mut self,
        topic: &str,
        submit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.supports_planning() {
            return;
        }
        let Some(owner) = self.plan_owner() else {
            return;
        };
        self.load_saved_plan(owner, cx);
        let mut plan = self.current_plan().cloned().unwrap_or_default();
        if plan.phase == Phase::Submitted {
            plan = Plan::default();
        }
        if plan.phase == Phase::Saved {
            plan.phase = if plan.proposal.is_empty() {
                Phase::Draft
            } else {
                Phase::Ready
            };
        }
        if !self.save_plan(owner, plan, cx) {
            return;
        }
        if matches!(self.access_mode, ChatAccessMode::NoneSelected) {
            self.remember_workspace_plan(cx);
        }
        self.composer_editor.update(cx, |ed, cx| {
            ed.set_value(topic.to_owned(), window, cx);
            ed.focus(window, cx);
        });
        self.sync_plan_placeholder(cx);
        if submit && !topic.trim().is_empty() {
            self.submit_planning_input(topic, window, cx);
        }
        cx.notify();
    }

    pub(super) fn submit_planning_input(
        &mut self,
        typed: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.planning_here() {
            return false;
        }
        if self.plan_input_blocked() || typed.trim().is_empty() {
            return true;
        }
        if !self.pending_attachments.is_empty() || self.pinned_target.is_some() {
            self.notify_warning(self.ui_locale.pick("请在规划任务中写出工作区文件名，或移除当前附件和文档选区。","For planning, name the workspace files in your request and remove attachments or document selections."),cx);
            return true;
        }
        let Some(owner) = self.plan_owner() else {
            return true;
        };
        let accepted = if let Some(p) = self.current_plan()
            && self.planning.questions.contains_key(&p.id)
        {
            self.answer_plan_question(owner, p.id, typed, window, cx);
            true
        } else {
            self.generate_plan(owner, typed, cx)
        };
        if accepted {
            self.composer_editor
                .update(cx, |ed, cx| ed.set_value("", window, cx));
        }
        self.sync_plan_placeholder(cx);
        true
    }

    pub(super) fn plan_create_failed(
        &mut self,
        id: Uuid,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.planning.creating.as_ref().is_some_and(|p| p.0 == id) {
            self.planning.creating = None;
            self.planning.approval_lock = None;
            self.sync_plan_placeholder(cx);
            cx.notify();
        }
    }

    pub(super) fn plan_chat_created(
        &mut self,
        chat: Uuid,
        id: Uuid,
        _topic: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((pending, _, owner)) = self.planning.creating.as_ref() else {
            return;
        };
        if *pending != id {
            return;
        }
        let owner = *owner;
        self.planning.creating = None;
        let _approval_lock = self.planning.approval_lock.take();
        let Some(plan) = self
            .planning
            .by_chat
            .get(&owner)
            .filter(|p| p.id == id && p.phase == Phase::Ready)
            .cloned()
        else {
            return;
        };
        if !self.save_plan(chat, plan.clone(), cx) {
            return;
        }
        if matches!(self.access_mode,ChatAccessMode::WriteController{chat_id} if chat_id==chat) {
            self.implement_plan(chat, id, cx);
        }
        let mut transferred = plan;
        transferred.phase = Phase::Submitted;
        self.save_plan(owner, transferred, cx);
        self.planning.approval_lock = None;
        self.sync_plan_placeholder(cx);
    }

    fn generate_plan(&mut self, chat: Uuid, input: &str, cx: &mut Context<Self>) -> bool {
        let draft = matches!(self.access_mode, ChatAccessMode::NoneSelected)
            && self.workspace_plan_id() == chat;
        if self.turn_in_progress()
            || (!draft
                && !matches!(self.access_mode,ChatAccessMode::WriteController{chat_id} if chat_id==chat))
        {
            return false;
        }
        let last = self.planning.by_chat.get(&chat);
        let previous = last
            .filter(|p| !p.proposal.is_empty())
            .map(|p| p.proposal.clone());
        let original = last.map(|p| p.topic.as_str()).unwrap_or("");
        let topic = if previous.is_some() && !original.is_empty() && original != input {
            format!(
                "{original}\n\n{}\n{input}",
                self.ui_locale.pick("修改要求", "Requested revision")
            )
        } else {
            input.to_owned()
        };
        let id = Uuid::new_v4();
        let p = Plan {
            id,
            topic: topic.clone(),
            proposal: previous.clone().unwrap_or_default(),
            phase: Phase::Working,
            error: None,
        };
        if !self.save_plan(chat, p, cx) {
            return false;
        }
        if draft {
            let program = self
                .providers
                .iter()
                .find(|p| p.id == "codex")
                .and_then(|p| match &p.availability {
                    ProviderAvailability::Available { command, .. } => command.clone(),
                    _ => None,
                });
            self.bridge.send(KarCommand::GenerateDraftPlan {
                draft_id: chat,
                request_id: id,
                topic,
                previous,
                workspace: self.config.workspace_dir.clone(),
                program,
                model: self.selected_model.clone(),
                effort: self.selected_effort().map(str::to_owned),
            });
        } else {
            self.bridge.send(KarCommand::GeneratePlan {
                chat_id: chat,
                request_id: id,
                topic,
                previous,
            });
        }
        true
    }

    pub(super) fn plan_progress(
        &mut self,
        chat: Uuid,
        id: Uuid,
        label: String,
        cx: &mut Context<Self>,
    ) {
        if self.plan_is_working(chat, id) && !label.trim().is_empty() {
            self.planning.progress.insert(id, label);
            cx.notify();
        }
    }

    pub(super) fn receive_plan_questions(
        &mut self,
        chat: Uuid,
        id: Uuid,
        questions: Vec<PlanQuestion>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.plan_is_working(chat, id) || questions.is_empty() {
            return;
        }
        self.planning.questions.insert(
            id,
            Clarification {
                questions,
                answers: BTreeMap::new(),
            },
        );
        if self.plan_owner() == Some(chat) {
            self.sync_plan_placeholder(cx);
        }
        cx.notify();
    }

    fn plan_is_working(&self, chat: Uuid, id: Uuid) -> bool {
        self.planning
            .by_chat
            .get(&chat)
            .is_some_and(|p| p.id == id && p.phase == Phase::Working)
    }

    fn answer_plan_question(
        &mut self,
        chat: Uuid,
        id: Uuid,
        answer: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.plan_is_working(chat, id) || answer.trim().is_empty() {
            return;
        }
        let Some(pending) = self.planning.questions.get_mut(&id) else {
            return;
        };
        let Some(question) = pending.next() else {
            return;
        };
        pending
            .answers
            .insert(question.id.clone(), answer.trim().to_owned());
        if pending.next().is_none() {
            let pending = self.planning.questions.remove(&id).unwrap();
            self.bridge.send(KarCommand::AnswerPlan {
                chat_id: chat,
                request_id: id,
                answers: pending.answers,
            });
        }
        self.sync_plan_placeholder(cx);
        self.composer_editor
            .update(cx, |ed, cx| ed.focus(window, cx));
        cx.notify();
    }

    pub(super) fn finish_planning(
        &mut self,
        chat: Uuid,
        id: Uuid,
        result: Result<String, String>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.plan_is_working(chat, id) {
            return;
        }
        self.planning.questions.remove(&id);
        self.planning.progress.remove(&id);
        let mut plan = self.planning.by_chat[&chat].clone();
        match result {
            Ok(body) if !body.trim().is_empty() => {
                plan.proposal = body;
                plan.phase = Phase::Ready;
                plan.error = None;
            }
            other => {
                plan.phase = Phase::Interrupted;
                plan.error = other.err();
            }
        }
        if self.plan_owner() == Some(chat) {
            self.planning.expanded.insert(chat);
        }
        self.save_plan(chat, plan, cx);
        if self.plan_owner() == Some(chat) {
            self.sync_plan_placeholder(cx);
        }
    }

    fn exit_plan(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(chat) = self.plan_owner()
            && let Some(mut plan) = self.current_plan().cloned()
        {
            if plan.phase == Phase::Working {
                self.bridge.send(KarCommand::CancelPlan {
                    chat_id: chat,
                    request_id: plan.id,
                });
                self.planning.questions.remove(&plan.id);
                self.planning.progress.remove(&plan.id);
            }
            plan.phase = Phase::Saved;
            self.save_plan(chat, plan, cx);
        }
        if matches!(self.access_mode, ChatAccessMode::NoneSelected) {
            self.forget_workspace_plan(cx);
        }
        self.sync_plan_placeholder(cx);
        self.composer_editor
            .update(cx, |ed, cx| ed.focus(window, cx));
        cx.notify();
    }

    pub(crate) fn recoverable_archived_plan(&self) -> bool {
        matches!(self.access_mode, ChatAccessMode::ArchivedReadOnly { .. })
            && self.projected.turns.is_empty()
            && self
                .current_plan()
                .is_some_and(|p| p.phase == Phase::Ready && !p.proposal.is_empty())
    }

    pub(crate) fn recover_archived_plan(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.recoverable_archived_plan() {
            return;
        }
        let plan = self.current_plan().unwrap().clone();
        let workspace = self.effective_workspace_dir();
        self.select_project_and_new_chat(workspace, window, cx);
        let owner = self.workspace_plan_id();
        self.load_saved_plan(owner, cx);
        if self.current_plan().is_some_and(|saved| {
            saved.id != plan.id && !matches!(saved.phase, Phase::Saved | Phase::Submitted)
        }) {
            self.notify_warning(self.ui_locale.pick("此工作区已有一份待处理的计划。请先处理它，再恢复另一份。","This workspace already has an active plan. Finish it before restoring another."),cx);
            return;
        }
        self.selected_runtime = "codex".into();
        if self.save_plan(owner, plan, cx) {
            self.planning.expanded.insert(owner);
            self.remember_workspace_plan(cx);
            self.sync_plan_placeholder(cx);
        }
    }

    pub(crate) fn planning_topic(&self) -> Option<&str> {
        self.current_plan()
            .filter(|_| self.planning_here())
            .map(|p| p.topic.as_str())
            .filter(|s| !s.is_empty())
    }

    pub(super) fn reconcile_plan_submission(&mut self, cx: &mut Context<Self>) {
        let Some(chat) = self
            .current_chat_id()
            .filter(|id| self.projected.chat_id == Some(*id))
        else {
            return;
        };
        if !matches!(self.access_mode, ChatAccessMode::WriteController { .. }) {
            return;
        }
        let Some(mut plan) = self
            .current_plan()
            .filter(|p| p.phase == Phase::Submitted)
            .cloned()
        else {
            return;
        };
        if self.projected.turns.iter().any(|t| t.id == plan.id)
            || self
                .prompt_attempts
                .get(&plan.id)
                .is_some_and(|a| a.last_error.is_none())
            || self.planning.dispatching.contains(&plan.id)
        {
            return;
        }
        plan.phase = Phase::Ready;
        self.save_plan(chat, plan, cx);
    }

    fn implement_plan(&mut self, chat: Uuid, id: Uuid, cx: &mut Context<Self>) {
        if self.turn_in_progress()
            || self.planning.creating.is_some()
            || self.plan_owner() != Some(chat)
        {
            return;
        }
        let Some(mut plan) = self
            .current_plan()
            .filter(|p| p.id == id && p.phase == Phase::Ready && !p.proposal.trim().is_empty())
            .cloned()
        else {
            return;
        };
        if matches!(self.access_mode, ChatAccessMode::NoneSelected) {
            // A workspace draft can be open in two windows before a Kar writer exists.
            let path = self.plan_file(chat);
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path.with_extension("approval-lock"));
            let lock = match lock {
                Ok(file) if file.try_lock().is_ok() => file,
                _ => {
                    self.notify_warning(
                        self.ui_locale.pick(
                            "另一窗口正在确认这份计划，或草稿目录不可写。",
                            "Another window is approving this plan, or its folder is read-only.",
                        ),
                        cx,
                    );
                    return;
                }
            };
            let saved = std::fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Plan>(&bytes).ok());
            if !saved.is_some_and(|p| p.id == id && p.phase == Phase::Ready) {
                self.planning.by_chat.remove(&chat);
                self.load_saved_plan(chat, cx);
                self.notify_warning(
                    self.ui_locale.pick(
                        "这份计划已更新，请查看最新版本。",
                        "This plan has changed. Review the latest version.",
                    ),
                    cx,
                );
                return;
            }
            if self.create_chat_for_composer(None, Some((id, plan.topic.clone())), cx) {
                self.planning.approval_lock = Some(lock);
                self.planning.creating = Some((id, self.config.workspace_dir.clone(), chat));
                cx.notify();
            }
            return;
        }
        if !matches!(self.access_mode,ChatAccessMode::WriteController{chat_id} if chat_id==chat) {
            return;
        }
        let text = format!(
            "{}\n\n{}",
            self.ui_locale.pick(
                "请实施以下已确认的计划。",
                "Implement the following approved plan."
            ),
            plan.proposal
        );
        plan.phase = Phase::Submitted;
        if self.save_plan(chat, plan, cx) {
            self.sync_plan_placeholder(cx);
            self.planning.dispatching.insert(id);
            self.dispatch_prompt_id(id, text, None, Vec::new(), cx);
        }
    }
}
