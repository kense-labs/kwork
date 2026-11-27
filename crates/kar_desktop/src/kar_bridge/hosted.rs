//! After a native handoff the Kar workbench owns the provider controller. Drive that
//! same controller through its public routes and keep only a desktop editing lease.
use super::*;
use axum::http::Method;
use kar::chat::{Handoff, HandoffStatus};
use serde_json::{Value, json};

#[derive(Clone)]
pub(super) struct Hosted {
    engine: crate::engine::Engine,
    project: Uuid,
    pub(super) chat: Uuid,
    _writer: Arc<std::fs::File>,
}

impl Hosted {
    pub(super) async fn new(state: &WorkerState, chat: Uuid) -> Result<Self> {
        let engine = state
            .engine
            .as_ref()
            .context("Kar 工作台服务不可用")?
            .clone();
        let project = sidebar::project(&engine, chat).await?;
        let writer = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(state.store_dir.join(format!("{chat}.kwork-writer")))?;
        writer.try_lock().context("另一个窗口正在控制这段对话")?;
        Ok(Self {
            engine,
            project,
            chat,
            _writer: Arc::new(writer),
        })
    }

    pub(super) async fn post(&self, suffix: &str, body: Value) -> Result<Value> {
        self.engine
            .request(
                Method::POST,
                &format!("/projects/{}/chats/{}/{}", self.project, self.chat, suffix),
                Some(body),
            )
            .await
    }

    pub(super) fn action(
        &self,
        suffix: String,
        body: Value,
        events: async_channel::Sender<KarBridgeEvent>,
    ) {
        let hosted = self.clone();
        tokio::spawn(async move {
            if let Err(e) = hosted.post(&suffix, body).await {
                let _ = events
                    .send(KarBridgeEvent::ToastMessage {
                        text: format!("工作台操作未完成：{e}"),
                        warning: true,
                    })
                    .await;
            }
        });
    }

    pub(super) async fn rollback(
        &self,
        turn: Uuid,
        before: bool,
        state: &WorkerState,
    ) -> Result<()> {
        let store = state.store.as_ref().context("对话存储不可用")?;
        let info = store.history(self.chat)?.chat;
        if crate::aster::is_aster(&info.runtime) {
            let preview: kar::chat::RollbackReport = serde_json::from_value(
                self.post(
                    "rollback",
                    json!({"turn":turn,"before":before,"dry_run":true}),
                )
                .await?,
            )?;
            anyhow::ensure!(
                !preview
                    .files
                    .iter()
                    .any(|f| crate::aster::runtime_artifact(&f.path)),
                "恢复会覆盖当前会话记录，请先检查文件差异"
            );
        }
        let report: kar::chat::RollbackReport = serde_json::from_value(
            self.post("rollback", json!({"turn":turn,"before":before}))
                .await?,
        )?;
        if !report.conflicts.is_empty() {
            anyhow::bail!("{} 个文件存在冲突", report.conflicts.len());
        }
        let files = report
            .files
            .into_iter()
            .map(|f| info.workspace.join(f.path))
            .collect();
        state.emit(KarBridgeEvent::WorkspaceFilesRestored { files });
        Ok(())
    }

    pub(super) async fn bind(self, state: &mut WorkerState) -> Result<()> {
        self.post("attach", json!({})).await?;
        let chat = self.chat;
        let store = state.store.clone().context("对话存储不可用")?;
        let history = store.history(chat)?;
        let queued = active_queued_inputs(Some(&store), chat);
        state.stop_feed();
        state.selected_chat_id = Some(chat);
        state.hosted_chat = Some(self);
        state.emit(KarBridgeEvent::AccessModeChanged(
            ChatAccessMode::WriteController { chat_id: chat },
        ));
        state.emit(KarBridgeEvent::HostedSession(chat));
        state.emit(KarBridgeEvent::ChatStateUpdated(state_from_history(
            history, None, queued,
        )));
        state.feed_cancel = Some(spawn_local_follower(
            store,
            state.store_dir.clone(),
            chat,
            None,
            state.evt_tx.clone(),
        ));
        Ok(())
    }

    pub(super) fn send(
        &self,
        mut attempt: PendingPromptAttempt,
        store: ChatStore,
        events: async_channel::Sender<KarBridgeEvent>,
    ) {
        let hosted = self.clone();
        tokio::spawn(async move {
            let result = async {
                let ids = persist_attachment_paths(&store, hosted.chat, &attempt.attachments)?;
                let body = json!({"id": attempt.request_id, "text": attempt.sent_payload, "attachments": ids});
                let value = hosted.post("messages", body).await?;
                Ok::<_, anyhow::Error>(serde_json::from_value::<ChatTurn>(value)?)
            }.await;
            sidebar_journals::signal_turn(store.directory(), hosted.chat);
            attempt.in_flight = false;
            match result {
                Ok(turn) => {
                    let _ = events.send(KarBridgeEvent::ChatTurnActivity {
                        chat_id: hosted.chat,
                        status: turn.status,
                        activity_ms: turn.finished_ms.unwrap_or(turn.started_ms),
                    }).await;
                }
                Err(error) => attempt.last_error = Some(error.to_string()),
            }
            let _ = events
                .send(KarBridgeEvent::PromptLifecycle { attempt })
                .await;
        });
    }
}

pub(super) async fn transfer(
    state: &mut WorkerState,
    chat: Uuid,
    mut runtime: String,
    model: Option<String>,
    note: String,
    budget: usize,
) -> Result<()> {
    let already_hosted = state.hosted_chat.as_ref().is_some_and(|h| h.chat == chat);
    let direct = state
        .active_controller
        .as_ref()
        .is_some_and(|s| s.info().id == chat);
    anyhow::ensure!(
        state.selected_chat_id == Some(chat) && (already_hosted || direct),
        "请先在控制该对话的窗口交接"
    );
    let store = state.store.clone().context("对话存储不可用")?;
    let history = store.history(chat)?;
    let busy = history
        .turns
        .last()
        .is_some_and(|t| t.status == TurnStatus::Running);
    anyhow::ensure!(!busy || already_hosted, "请在当前回合结束后交接");
    anyhow::ensure!(
        !busy || note.trim().is_empty(),
        "请在交接完成后给接手方发送说明"
    );
    let hosted = match state.hosted_chat.clone() {
        Some(h) if h.chat == chat => h,
        _ => Hosted::new(state, chat).await?,
    };
    if runtime == crate::aster::ID {
        let binary = crate::aster::find_binary().context("内置智能体运行环境不可用")?;
        runtime = crate::aster::register(&hosted.engine, &binary).await?;
    }
    let command = Uuid::new_v4();
    if direct {
        state.stop_feed();
        state.close_controller(false).await;
    }
    let result = hosted.post("handoffs",json!({"id":command,"runtime":runtime,"model":model,"budget_bytes":budget,"when":if busy {"next_turn"} else {"immediate"}})).await;
    let receipt = match result.and_then(|value| Ok(serde_json::from_value::<Handoff>(value)?)) {
        Ok(r) => r,
        Err(e) => {
            if direct {
                let _ = state
                    .cmd_tx
                    .send(KarCommand::SelectLocalChat {
                        chat_id: chat,
                        prefer_read_only: false,
                    })
                    .await;
            }
            return Err(e);
        }
    };
    match receipt.status {
        HandoffStatus::Ready => {
            // Bind the target preserved by Kar; there is no second chat or duplicate prompt.
            if let Err(e) = hosted.bind(state).await {
                let _ = state
                    .cmd_tx
                    .send(KarCommand::SelectLocalChat {
                        chat_id: chat,
                        prefer_read_only: false,
                    })
                    .await;
                anyhow::bail!("Kar 已交接，正在重新连接接手方：{e}");
            }
            state.emit(KarBridgeEvent::HandoffCompleted {
                chat_id: Some(chat),
                target_runtime: receipt
                    .target
                    .as_ref()
                    .context("接手方缺失")?
                    .runtime
                    .clone(),
                summary_message: receipt.context,
            });
            if !note.trim().is_empty() {
                state.send_prompt(Uuid::new_v4(), note.clone(), note, None, vec![]);
            }
            state.refresh_chat_list();
        }
        HandoffStatus::Failed => {
            if direct {
                let _ = state
                    .cmd_tx
                    .send(KarCommand::SelectLocalChat {
                        chat_id: chat,
                        prefer_read_only: false,
                    })
                    .await;
            }
            anyhow::bail!("{}", receipt.error.unwrap_or_else(|| "Kar 交接失败".into()));
        }
        HandoffStatus::Prepared => {
            state.emit(KarBridgeEvent::ToastMessage {
                text: "已排队，当前回合结束后交接。".into(),
                warning: false,
            });
            // The durable Kar handoff worker changes the same chat's journal after the active turn.
            // Its existing feed follows the change even when this window is closed.
        }
    }
    Ok(())
}
