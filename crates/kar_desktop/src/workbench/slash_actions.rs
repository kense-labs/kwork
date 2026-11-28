//! Desktop actions available from the conversation composer.
use super::*;
use crate::turn_review::{FileReview, ReviewKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LocalSlash {
    New,
    Model,
    Reasoning,
    Permissions,
    Status,
    Rename,
    Pin,
    Unpin,
    Fork,
    Compact,
    Review,
    Handoff,
    SideQuestion,
    Attach,
    Browser,
    Files,
    Diff,
    Terminal,
    Skills,
    Stop,
    Search,
    Tasks,
    Share,
    Delete,
}

impl KarWorkbench {
    fn latest_review_key(&self) -> Option<ReviewKey> {
        let chat = self.current_chat_id()?;
        self.projected
            .turns
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, turn)| {
                let checkpoint = turn.checkpoint.as_ref()?;
                checkpoint.after_tree.as_ref()?;
                Some(ReviewKey {
                    chat,
                    turn: turn.id,
                    index,
                    path: checkpoint.files.first()?.path.clone(),
                })
            })
    }

    pub(super) fn slash_action_available(&self, action: LocalSlash) -> bool {
        use LocalSlash::*;
        let draft = matches!(self.access_mode, ChatAccessMode::NoneSelected);
        let current = self
            .current_chat_id()
            .is_some_and(|id| self.projected.chat_id == Some(id));
        let writer = current && matches!(self.access_mode, ChatAccessMode::WriteController { .. });
        let remote =
            current && self.access_mode.is_remote_paired() && self.access_mode.is_writable();
        let local = draft || writer;
        let pinned = || {
            self.current_chat_id()
                .is_some_and(|id| self.project_registry.is_pinned(id))
        };
        match action {
            New => writer || remote,
            Search | Tasks => draft || writer || remote,
            Model => {
                (draft && self.providers.iter().any(ProviderStatusInfo::is_ready))
                    || ((writer || remote) && self.current_chat_info().is_some())
            }
            Reasoning => {
                (draft && !self.offered_efforts().is_empty())
                    || ((writer || remote)
                        && self.current_chat_info().is_some_and(|info| {
                            info.settings.effort.is_some()
                                || self.providers.iter().any(|provider| {
                                    provider.id == info.runtime
                                        && !provider.effort_levels(info.model.as_deref()).is_empty()
                                })
                        }))
            }
            Permissions => local || remote,
            Status => writer || remote,
            Rename | Share => writer,
            Delete => writer && !self.turn_in_progress(),
            Pin => writer && !pinned(),
            Unpin => writer && pinned(),
            Fork => {
                writer
                    && self.projected.can_fork
                    && !self.turn_in_progress()
                    && self.projected.turns.last().is_some_and(|turn| {
                        turn.status != TurnStatus::Running
                            && turn
                                .checkpoint
                                .as_ref()
                                .is_some_and(|checkpoint| checkpoint.after_tree.is_some())
                    })
            }
            Compact => {
                writer
                    && self.projected.can_compact
                    && !self.turn_in_progress()
                    && !self.projected.turns.is_empty()
                    && !self
                        .projected
                        .manual_compaction
                        .as_ref()
                        .is_some_and(|report| {
                            matches!(
                                report.status,
                                kar::CompactionStatus::Dispatched | kar::CompactionStatus::Accepted
                            )
                        })
            }
            Review => writer && self.latest_review_key().is_some(),
            Handoff => {
                writer
                    && !self.turn_in_progress()
                    && !self.projected.turns.is_empty()
                    && self
                        .providers
                        .iter()
                        .any(|p| p.is_ready() && p.id != self.active_or_selected_runtime_id())
            }
            SideQuestion => {
                self.supports_side_question() && !self.side_question_pending
            }
            Stop => (writer || remote) && self.turn_in_progress(),
            Attach | Browser | Files | Terminal | Skills => local,
            Diff => {
                local
                    && self.git_snapshot.is_repo
                    && self
                        .git_snapshot
                        .repo_root
                        .as_ref()
                        .is_some_and(|root| self.effective_workspace_dir().starts_with(root))
            }
        }
    }

    pub(super) fn run_slash_action(
        &mut self,
        action: LocalSlash,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use LocalSlash::*;
        if !self.slash_action_available(action) {
            self.notify_warning(
                self.ui_locale.pick(
                    "当前会话已无法使用此指令",
                    "This command is no longer available",
                ),
                cx,
            );
            return;
        }
        let chat = self.current_chat_id();
        let draft = matches!(self.access_mode, ChatAccessMode::NoneSelected);
        match action {
            New => self.start_new_chat(window, cx),
            Model if draft => self.open_menu = Some(Menu::ComposerAgent),
            Reasoning if draft => self.open_menu = Some(Menu::ComposerEffort),
            Status | Model | Reasoning => self.open_menu = Some(Menu::ContextUsage),
            Permissions => self.open_menu = Some(Menu::ComposerApproval),
            Rename | Delete => {
                if let Some(id) = chat {
                    let title = self
                        .chat_list
                        .iter()
                        .find(|entry| entry.info.id == id)
                        .map(|entry| entry.title.clone())
                        .unwrap_or_default();
                    if action == Rename {
                        self.open_rename_chat_dialog(id, &title, window, cx);
                    } else {
                        let loc = self.ui_locale;
                        let confirm = window.prompt(
                            PromptLevel::Warning,
                            loc.pick("删除这个对话？", "Delete this chat?"),
                            Some(&title),
                            &[loc.pick("取消", "Cancel"), loc.pick("删除", "Delete")],
                            cx,
                        );
                        cx.spawn_in(window, async move |this, cx| {
                            if let Ok(1) = confirm.await {
                                let _ = this.update(cx, |this, cx| {
                                    if this.current_chat_id() == Some(id)
                                        && this.slash_action_available(Delete)
                                    {
                                        this.stage_deferred_chat_delete(id, title, cx);
                                    }
                                });
                            }
                        })
                        .detach();
                    }
                }
            }
            Pin | Unpin => {
                if let Some(id) = chat {
                    self.toggle_pin_chat(id, cx);
                }
            }
            Fork => {
                if let (Some(chat_id), Some(turn)) = (chat, self.projected.turns.last()) {
                    self.bridge.send(KarCommand::ForkChatAt {
                        chat_id,
                        turn_id: turn.id,
                        model_override: None,
                    });
                }
            }
            Compact => self.bridge.send(KarCommand::CompactChat),
            Review => {
                if let Some(key) = self.latest_review_key() {
                    self.jump_to_rail_turn(key.index, window, cx);
                    self.open_checkpoint_review(key);
                }
            }
            Handoff => self.toggle_handoff_sheet(cx),
            SideQuestion => self.toggle_side_question_sheet(cx),
            Attach => self.pick_attachments(window, cx),
            Browser => self.open_browser_dock_tab(cx),
            Files => self.open_files_dock_tab(cx),
            Diff => self.open_git_workbench_tab(cx),
            Terminal => self.open_terminal_dock_tab(cx),
            Skills => self.toggle_skills_sheet(cx),
            Stop => self.bridge.send(KarCommand::AbortTurn),
            Search => {
                self.set_sidebar_open(true, cx);
                self.sidebar_search
                    .update(cx, |editor, cx| editor.focus(window, cx));
            }
            Tasks => self.open_tasks_inbox_stage(cx),
            Share => self.open_settings(SettingsTab::Remote, cx),
        }
        cx.notify();
    }

    pub(crate) fn open_checkpoint_review(&mut self, key: ReviewKey) {
        self.turn_review = Some(FileReview {
            key: key.clone(),
            patch: None,
        });
        self.bridge.send(KarCommand::LoadTurnDiff(key));
    }
}
