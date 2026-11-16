//! ACP conversation state shared by the selected view and background sessions.

use super::*;

#[derive(Clone, Copy)]
pub(super) struct MarkdownCache<'a> {
    pub(super) highlights:
        &'a std::cell::RefCell<HashMap<String, super::document_code::MarkdownHighlight>>,
    pub(super) math: &'a std::cell::RefCell<HashMap<String, super::document_math::MathCacheEntry>>,
    pub(super) expanded_diffs: &'a std::cell::RefCell<std::collections::HashSet<String>>,
}

pub(super) fn highlight_markdown(
    cache: MarkdownCache<'_>,
    session: Option<&str>,
    key: &str,
    source: &str,
    language: &str,
) -> (
    Vec<Vec<super::document_code::CodeSpan>>,
    Option<HighlightRequest>,
) {
    let Some(session) = session.map(str::to_owned) else {
        return (Vec::new(), None);
    };
    let now = bridge::monotonic_ms();
    let (tokens, request) = {
        let mut cache = cache.highlights.borrow_mut();
        if cache.len() > 256 {
            cache.retain(|_, entry| {
                std::rc::Rc::strong_count(&entry.session) > 1 || now - entry.last_used_ms < 30_000.
            });
        }
        let entry = cache.entry(key.to_owned()).or_insert_with(|| {
            super::document_code::MarkdownHighlight {
                source: String::new(),
                language: String::new(),
                tokens: Vec::new(),
                session: std::rc::Rc::new(super::document_code::HighlightSession::new()),
                requested: false,
                retry_at_ms: None,
                last_used_ms: now,
            }
        });
        entry.last_used_ms = now;
        if entry.source == source
            && entry.language == language
            && (entry.requested || entry.retry_at_ms.is_some_and(|retry| now < retry))
        {
            return (entry.tokens.clone(), None);
        }
        let tokens = entry.tokens.clone();
        entry.source = source.to_owned();
        entry.language = language.to_owned();
        entry.requested = true;
        entry.retry_at_ms = None;
        let highlighter = entry.session.clone();
        (
            tokens,
            Some((highlighter.highlight(source, language), highlighter)),
        )
    };
    let Some((request, highlighter)) = request else {
        return (tokens, None);
    };
    (
        tokens,
        Some(HighlightRequest {
            request,
            highlighter,
            session,
            key: key.to_owned(),
            source: source.to_owned(),
            language: language.to_owned(),
        }),
    )
}

pub(super) struct HighlightRequest {
    request: js_sys::Promise,
    highlighter: std::rc::Rc<super::document_code::HighlightSession>,
    session: String,
    key: String,
    source: String,
    language: String,
}

pub(super) fn resolve_highlight(request: HighlightRequest) {
    super::with_host_after_render(move |cx| start_highlight(request, cx));
}

fn start_highlight(request: HighlightRequest, cx: &mut Context<DshApp>) {
    let HighlightRequest {
        request,
        highlighter,
        session,
        key,
        source,
        language,
    } = request;
    cx.spawn(async move |this, cx| {
        let result = wasm_bindgen_futures::JsFuture::from(request).await;
        let _ = this.update(cx, |this, cx| {
            if this.active_session.as_deref() != Some(session.as_str()) {
                return;
            }
            let mut cache = this.conversation.markdown_highlights.borrow_mut();
            let Some(entry) = cache.get_mut(&key).filter(|entry| {
                entry.source == source
                    && entry.language == language
                    && std::rc::Rc::ptr_eq(&entry.session, &highlighter)
            }) else {
                return;
            };
            entry.requested = false;
            let retry = match result.and_then(|value| {
                serde_wasm_bindgen::from_value(value).map_err(|error| error.into())
            }) {
                Ok(tokens) => {
                    entry.tokens = tokens;
                    entry.retry_at_ms = None;
                    false
                }
                Err(_) => {
                    entry.retry_at_ms = Some(bridge::monotonic_ms() + 2000.);
                    true
                }
            };
            drop(cache);
            if retry {
                cx.spawn(async move |this, cx| {
                    gloo_timers::future::TimeoutFuture::new(2000).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
            cx.notify();
        });
    })
    .detach();
}

#[derive(Default)]
pub(super) struct Conversation {
    pub(super) acp_open: bool,
    next_local_id: u64,
    pub(super) draft: String,
    pub(super) draft_workspace: Option<String>,
    pub(super) error: Option<String>,
    pub(super) running: bool,
    pub(super) prompt_inflight: bool,
    pub(super) turn_started: Option<f64>,
    /// Monotonic ms of the last ACP content event. A live turn with no events
    /// for too long is ended by the turn clock instead of hanging forever.
    pub(super) last_event_ms: Option<f64>,
    pub(super) turn_summary: Option<TurnSummary>,
    pub(super) turn_usage_base: Option<u64>,
    pub(super) queued_prompts: Vec<QueuedPrompt>,
    pub(super) inbox_expanded: bool,
    pub(super) inbox_edit: Option<session_inbox::InboxEdit>,
    pub(super) inbox_action: Option<String>,
    pub(super) inbox_error: Option<String>,
    pub(super) inbox_submission: Option<session_inbox::InboxSubmission>,

    /// Current goal shown by the composer's GoalBar; None renders nothing.
    pub(super) goal: Option<crate::goal::GoalView>,
    pub(super) goal_sequence: u64,
    pub(super) goal_edit: Option<super::goal_bar::GoalEdit>,
    /// One goal mutation in flight; the strip disables its verbs meanwhile.
    pub(super) goal_busy: bool,
    /// Last rejected goal verb, shown inside the strip (official `actionError`).
    pub(super) goal_error: Option<String>,
    /// Durable goal revision already reconciled against history.
    ///
    /// Autonomous rounds can open and fail before the client has marked the
    /// conversation running; a missed `turn_end` would leave the strip stuck on
    /// "进行中的目标". After a goal revision changes, KWork re-reads history once
    /// while idle so the failure row (or the round's content) cannot be lost.
    pub(super) goal_synced_revision: Option<(String, u64)>,
    /// Direct children this session spawned (official subagent catalog).
    pub(super) subagents: Vec<crate::subagents::SubagentRow>,
    /// Whether the composer's subagent list is expanded.
    pub(super) subagents_expanded: bool,
    /// Root-only Team selection and shared tasks from the Cordis agentTeam projection.
    pub(super) team: Option<crate::subagents::TeamState>,
    /// One Team save in flight; no optimistic change of the saved selection.
    pub(super) team_pending: bool,
    /// Latest issued request; projection events may not overtake a pending save.
    pub(super) team_request: u64,
    /// A save failed, so its next reply is only an authoritative reread.
    pub(super) team_rereading: bool,
    /// Turn rail mark under the pointer (index into the rail items).
    pub(super) turn_rail_hover: Option<usize>,
    /// 待投递列表的本地单调序号；用于忽略比某一行更早请求的 Host 快照。
    pub(super) inbox_seq: u64,
    pub(super) wrapped_code: std::collections::HashSet<String>,
    pub(super) markdown_highlights:
        std::cell::RefCell<HashMap<String, super::document_code::MarkdownHighlight>>,
    pub(super) markdown_math:
        std::cell::RefCell<HashMap<String, super::document_math::MathCacheEntry>>,
    pub(super) turn_process: crate::history::turn_process::TurnProcess,
    pub(super) turn_disclosures: HashMap<u64, (u64, bool)>,
    pub(super) turn_tails: crate::history::turn_tails::TurnTails,
    pub(super) resumed_session: bool,
    pub(super) entries: Vec<ChatEntry>,
    pub(super) trajectory: crate::trajectory::State,
    pub(super) inherited_history: Option<crate::history::InheritedHistory>,
    pub(super) assistant_stream: crate::assistant_stream::AssistantStream,
    /// Tool name whose arguments are still streaming (`Some("")` when the name is
    /// not known yet); drives the official 准备X live label.
    pub(super) preparing_tool: Option<String>,
    /// Mutation path remembered for each in-flight tool call (official
    /// deliverables accumulator) and the turn's settled successful mutations.
    pub(super) tool_mutations: HashMap<String, String>,
    pub(super) pending_changed_files: Vec<String>,
    pub(super) usage: Option<(u64, u64)>,
    pub(super) approvals: std::collections::VecDeque<approval::ApprovalRequest>,
    pub(super) permission_mode: super::permission_picker::PermissionMode,
    pub(super) plan_reviews: Vec<plan::PlanReview>,
    pub(super) questions: Vec<question_panel::PendingQuestions>,
    pub(super) settled_questions: HashMap<String, Vec<crate::questions::RecordedAnswer>>,
    pub(super) queued_question_replies: HashMap<String, Vec<crate::questions::RecordedAnswer>>,
    pub(super) submitted_plans: HashMap<u64, Vec<crate::submitted_plan::SubmittedPlan>>,
    pub(super) plan_active: Option<bool>,
    pub(super) plan_mode_busy: bool,
    pub(super) command_busy: bool,
    pub(super) config_options: Vec<Value>,
    pub(super) command_model_pending: bool,
    pub(super) model_menu_pending: Option<(ComposerMenu, String)>,
    pub(super) scroll: ScrollHandle,
    pub(super) follow_tail: bool,
    pub(super) flow_motion: super::flow_motion::FlowMotion,
    pub(super) flow_scroll: super::flow_scroll::FlowScroll,
    pub(super) last_turn_settled: Option<f64>,
    pub(super) process_viewports: HashMap<String, super::process_scroll::ProcessViewport>,
    pub(super) expanded_entries: std::collections::HashSet<String>,
    pub(super) expanded_diffs: std::cell::RefCell<std::collections::HashSet<String>>,
    pub(super) decode_tokens: u64,
    pub(super) decode_ms: f64,
    pub(super) uncached_input_tokens: u64,
    pub(super) cache_read_tokens: u64,
    pub(super) cache_write_tokens: u64,
    pub(super) output_tokens: u64,
    pub(super) stats_turns: u64,
    pub(super) stats_steps: u64,
    pub(super) stats_llm_ms: f64,
    pub(super) stats_tool_ms: f64,
    pub(super) stats_ttft_ms: f64,
    pub(super) stats_ttft_steps: u64,
    pub(super) history_has_more: bool,
    pub(super) history_loading: bool,
    pub(super) history_request_id: u64,
    pub(super) history_prepend_pending: bool,
    pub(super) history_oldest_seq: Option<u64>,
    /// An older-page request failed; the edge row shows 重新加载.
    pub(super) history_error: Option<String>,
    /// This session has exposed an incomplete history window at least once, so
    /// the edge keeps showing 已到会话开头 after the last page arrives.
    pub(super) history_was_partial: bool,
    /// Top-level transcript rows drawn from the end of the conversation.
    ///
    /// The transcript has no virtualization; a few thousand rows would rebuild
    /// the whole element tree on every animation frame. The newest rows render
    /// first and 更早的记录 unfolds further rows on demand.
    pub(super) render_window: usize,
    pub(super) draft_attachments: Vec<DraftAttachment>,
    /// Delivery files announced during the running turn; they become the
    /// official turn-tail cards when the turn settles.
    pub(super) pending_deliverables: Vec<serde_json::Value>,
    /// Derived per-assistant file-mention vocabularies (rebuilt each transcript
    /// render from the turn's produced paths and deliveries).
    pub(super) mention_vocab:
        std::collections::HashMap<String, crate::mentions::FileMentionVocabulary>,
    /// Per-turn token usage read from the session log (official TurnUsagePanel).
    pub(super) turn_usage: std::collections::HashMap<u64, crate::state::TurnTokenUsage>,
}

impl Conversation {
    pub(super) fn fresh() -> Self {
        Self {
            follow_tail: true,
            ..Self::default()
        }
    }

    /// Official DeliverablesTail: delivery cards belong to the closing turn tail,
    /// not to the tool result's position, so they are held until the turn settles.
    /// Remember the mutation path of one tool call until its result settles.
    pub(super) fn track_tool_call(&mut self, call_id: &str, name: &str, arguments: &str) {
        if let Some(path) = crate::mentions::mutation_path(name, arguments) {
            self.tool_mutations.insert(call_id.to_owned(), path);
        }
    }

    /// Settle one tool call: a successful mutation joins the turn's changed files.
    pub(super) fn settle_tool_call(&mut self, call_id: &str, failed: bool) {
        let Some(path) = self.tool_mutations.remove(call_id) else {
            return;
        };
        if failed {
            return;
        }
        if !self.pending_changed_files.contains(&path) {
            self.pending_changed_files.push(path);
        }
    }

    pub(super) fn flush_pending_deliverables(&mut self) {
        if self.pending_deliverables.is_empty() {
            return;
        }
        let files = std::mem::take(&mut self.pending_deliverables);
        self.entries.push(crate::state::deliverable_entry(
            &files,
            Some(bridge::now_ms()),
        ));
    }

    /// Official history-edge state (labels live in `history::pagination`).
    pub(super) fn history_edge(&self) -> Option<crate::history::pagination::HistoryEdge> {
        crate::history::pagination::history_edge(
            self.history_has_more,
            self.history_loading,
            self.history_prepend_pending,
            self.history_error.is_some(),
            self.history_was_partial,
        )
    }

    pub(super) fn begin_turn(&mut self) {
        self.running = true;
        let now = bridge::monotonic_ms();
        self.turn_started = Some(now);
        self.last_event_ms = Some(now);
        self.turn_summary = None;
        self.preparing_tool = None;
        self.turn_usage_base = self.usage.map(|(used, _)| used);
    }

    pub(super) fn finish_turn(&mut self, end: TurnEnd) {
        let seconds = self
            .turn_started
            .map(|started| ((bridge::monotonic_ms() - started) / 1000.) as f32)
            .unwrap_or(0.);
        let used = self
            .usage
            .map(|(used, _)| used.saturating_sub(self.turn_usage_base.unwrap_or(0)))
            .filter(|used| *used > 0);
        if let Some(tokens) = used
            && seconds > 0.05
        {
            self.decode_tokens = self.decode_tokens.saturating_add(tokens);
            self.decode_ms += f64::from(seconds) * 1000.;
        }
        self.turn_started = None;
        self.last_event_ms = None;
        self.last_turn_settled = Some(bridge::monotonic_ms());
        self.turn_summary = Some(TurnSummary { end, seconds, used });
        self.running = false;
        self.preparing_tool = None;
        self.assistant_stream.clear();
        // Official order: the changed-files card precedes the delivered files.
        if !self.pending_changed_files.is_empty() {
            let paths = std::mem::take(&mut self.pending_changed_files);
            self.entries.push(crate::state::changed_files_entry(
                &paths,
                Some(bridge::now_ms()),
            ));
        }
        self.tool_mutations.clear();
        self.flush_pending_deliverables();
        self.approvals.clear();
    }

    pub(super) fn push_local(&mut self, role: EntryRole, text: String) {
        let key = format!("local-{}", self.next_local_id);
        self.next_local_id += 1;
        let mut entry = ChatEntry::local(role, key, text);
        entry.at_ms = Some(bridge::now_ms());
        self.entries.push(entry);
    }

    /// Append a locally built notice row (kept at transcript position like the
    /// official turn-positioned items) with a fresh local key.
    pub(super) fn push_turn_notice(&mut self, mut entry: ChatEntry) {
        entry.key = format!("local-{}", self.next_local_id);
        self.next_local_id += 1;
        entry.at_ms = Some(bridge::now_ms());
        self.entries.push(entry);
    }

    /// Turn number of one transcript row: every user-role row opens a turn, the
    /// same numbering the host writes into `turn/start`.
    pub(super) fn turn_number_for(&self, key: &str) -> Option<u64> {
        let mut turn = 0u64;
        for entry in &self.entries {
            if entry.role == EntryRole::User {
                turn = turn.saturating_add(1);
            }
            if entry.key == key {
                return (turn > 0).then_some(turn);
            }
        }
        None
    }

    /// Turn number of the most recent turn already on screen: one turn per
    /// user-role row, the same rule the turn rail and history replay use.
    pub(super) fn last_turn_number(&self) -> Option<u64> {
        self.entries
            .iter()
            .filter(|entry| entry.role == EntryRole::User)
            .count()
            .try_into()
            .ok()
            .filter(|count| *count > 0)
    }

    pub(super) fn pending_inbox_submission(
        &self,
        mode: crate::submit::SubmitMode,
    ) -> Option<&session_inbox::InboxSubmission> {
        self.inbox_submission.as_ref().filter(|send| {
            !send.admitted
                && send.mode == mode
                && !self
                    .queued_prompts
                    .iter()
                    .any(|row| row.request_id.as_deref() == Some(&send.id))
        })
    }

    /// A prompt may already have reached the transcript by the time ACP replies.
    pub(super) fn accept_inbox_row(&mut self, row: QueuedPrompt) {
        if self
            .queued_prompts
            .iter()
            .any(|existing| existing.item_id == row.item_id)
            || self.entries.iter().any(|entry| {
                entry.external_id.as_deref() == Some(&row.item_id) && entry.role == EntryRole::User
            })
        {
            return;
        }
        self.inbox_seq += 1;
        self.queued_prompts.push(row);
    }

    pub(super) fn inbox_snapshot_request(&mut self) -> u64 {
        self.inbox_seq += 1;
        self.inbox_seq
    }

    /// An older snapshot cannot undo a local mutation or a newer Host read.
    pub(super) fn apply_inbox_snapshot(&mut self, items: &[Value], issued_at: u64) {
        if issued_at != self.inbox_seq {
            return;
        }
        self.queued_prompts = items.iter().filter_map(QueuedPrompt::from_inbox).collect();
        if self.inbox_edit.as_ref().is_some_and(|edit| {
            !self.queued_prompts.iter().any(|row| {
                row.item_id == edit.item_id && row.mode == crate::submit::SubmitMode::Queue
            })
        }) {
            self.inbox_edit = None;
        }
        if !self
            .queued_prompts
            .iter()
            .any(|row| row.mode == crate::submit::SubmitMode::Queue)
            && self.inbox_submission.is_none()
        {
            self.inbox_expanded = false;
        }
    }

    /// The Host has claimed this row into the durable conversation log.
    pub(super) fn admit_inbox_row(&mut self, item_id: &str) -> bool {
        let count = self.queued_prompts.len();
        self.queued_prompts.retain(|row| row.item_id != item_id);
        let removed = self.queued_prompts.len() != count;
        if removed {
            self.inbox_seq += 1;
        }
        if self
            .inbox_edit
            .as_ref()
            .is_some_and(|edit| edit.item_id == item_id)
        {
            self.inbox_edit = None;
        }
        removed
    }

    pub(super) fn reconcile_assistant_stream(
        &mut self,
        message_id: Option<&str>,
        role: EntryRole,
        committed_key: &str,
    ) {
        for key in self.assistant_stream.reconcile(message_id, role) {
            if self.expanded_entries.remove(&key) {
                self.expanded_entries.insert(committed_key.to_owned());
            }
            if self.expanded_entries.remove(&format!("process-{key}")) {
                self.expanded_entries
                    .insert(format!("process-{committed_key}"));
            }
        }
    }

    pub(super) fn ingest_draft_items(&mut self, items: &[Value]) {
        for item in items {
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("未命名")
                .to_string();
            if id.is_empty() || name.is_empty() {
                continue;
            }
            let size = item.get("size").and_then(Value::as_u64).unwrap_or(0);
            let mime = item
                .get("mime")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let kind = match item["kind"].as_str() {
                Some("image") => AttachmentKind::Image,
                Some("file") => AttachmentKind::File,
                _ if mime.starts_with("image/") => AttachmentKind::Image,
                _ => AttachmentKind::File,
            };
            let preview = item
                .get("preview")
                .and_then(Value::as_str)
                .map(str::to_string);
            if self
                .draft_attachments
                .iter()
                .any(|existing| existing.id == id)
            {
                continue;
            }
            self.draft_attachments.push(DraftAttachment {
                id,
                name,
                size,
                kind,
                mime,
                preview,
            });
        }
    }

    pub(super) fn apply_permission(&mut self, event: &Value) {
        let Some(request) = approval::ApprovalRequest::from_event(event) else {
            return;
        };
        if self
            .approvals
            .iter()
            .any(|pending| pending.id == request.id)
        {
            return;
        }
        let tool_call = &request.tool_call;
        if let Some(call_id) = tool_call.get("toolCallId").and_then(Value::as_str)
            && let Some(entry) = self.entries.iter_mut().find(|entry| {
                entry.role == EntryRole::Tool && entry.external_id.as_deref() == Some(call_id)
            })
        {
            entry.status = "approval".to_string();
        }
        self.approvals.push_back(request);
    }
}
