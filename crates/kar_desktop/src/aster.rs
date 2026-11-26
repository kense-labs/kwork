//! Aster's native ACP executable. New chats use Kar's owner-managed local launch profile.
//! Kar records the immutable profile revision in each chat for future resumes.
use std::{
    ffi::OsStr,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use axum::http::Method;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::{
    engine::Engine,
    types::{ProviderAuthState, ProviderAvailability, ProviderModelOption, ProviderStatusInfo},
};

pub(crate) const ID: &str = "aster";
const PREFIX: &str = "acp-profile:kwork-aster:";
const VERSION: &str = "9.3.1";

pub(crate) fn is_aster(id: &str) -> bool {
    id == ID || id.starts_with(PREFIX)
}

pub(crate) fn runtime_artifact(path: &Path) -> bool {
    [
        ".aster/index",
        ".aster/sessions",
        ".aster/memory/durable_memory",
    ]
    .iter()
    .any(|dir| path.starts_with(dir))
}

fn filename() -> &'static str {
    if cfg!(windows) {
        "aster-acp.exe"
    } else {
        "aster-acp"
    }
}

fn managed_dir() -> Option<PathBuf> {
    std::env::home_dir().map(|home| home.join(".local/share/kwork/runtimes/aster").join(VERSION))
}

pub(crate) fn config_path() -> Option<PathBuf> {
    std::env::var_os("ASTER_CONFIG")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|p| p.join(".aster/config.acl")))
}

fn executable(path: &Path) -> bool {
    let Ok(meta) = path.metadata() else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    meta.is_file()
}

pub(crate) fn find_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ASTER_ACP_AGENT_BIN").map(PathBuf::from) {
        return path.is_absolute().then_some(path).filter(|p| executable(p));
    }
    let bundle = std::env::current_exe().ok().and_then(|p| {
        p.parent()?
            .parent()
            .map(|d| d.join("Resources").join(filename()))
    });
    let path = crate::kar_bridge::augmented_path_os();
    bundle
        .into_iter()
        .chain(
            std::env::split_paths(&path)
                .filter(|dir| dir.is_absolute())
                .map(|dir| dir.join(filename())),
        )
        .chain(managed_dir().map(|d| d.join(filename())))
        .find(|p| executable(p))
}

pub(crate) fn seed() -> ProviderStatusInfo {
    let availability = find_binary().map_or(ProviderAvailability::Missing, |command| {
        ProviderAvailability::Available {
            version: String::new(),
            command: Some(command),
        }
    });
    ProviderStatusInfo {
        workspace_commands: None,
        id: ID.into(),
        protocol: "acp".into(),
        availability,
        auth: ProviderAuthState::NotChecked,
        mcp_office_supported: false,
        steering_supported: false,
        goals_supported: false,
        can_install: release().is_some(),
        install_lines: vec![],
        models: vec![],
        quota_summary: None,
    }
}

pub(crate) async fn register(engine: &Engine, binary: &Path) -> Result<String> {
    let profile = engine
        .request(
            Method::PUT,
            "/acp-registry/profiles/kwork-aster",
            Some(json!({
                "source": "local", "name": "Aster", "command_path": binary, "command_args": []
            })),
        )
        .await?;
    Ok(profile["runtime"]
        .as_str()
        .filter(|id| id.starts_with(PREFIX))
        .context("Kar 未返回 Aster 启动配置")?
        .into())
}

// Aster publishes the active model and catalogue during ACP initialize. This probe never opens
// a chat, executes a prompt or imports another application's credentials.
async fn metadata(binary: &Path, workspace: &Path) -> Result<Value> {
    let mut child = tokio::process::Command::new(binary)
        .current_dir(workspace)
        .env_remove("KAR_WORKBENCH_TOKEN")
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let reply = async {
        let stdin = child.stdin.as_mut().context("ACP stdin")?;
        stdin.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":1,\"clientCapabilities\":{}}}\n").await?;
        stdin.flush().await?;
        let stdout = child.stdout.take().context("ACP stdout")?;
        let mut lines = BufReader::new(stdout.take(2 * 1024 * 1024)).lines();
        while let Some(line) = lines.next_line().await? {
            let reply: Value = serde_json::from_str(&line)?;
            if reply["id"] != 1 {
                continue;
            }
            let meta = &reply["result"]["_meta"];
            ensure!(meta["asterCode"] == true, "启动程序没有返回 Aster ACP 能力");
            return Ok(meta.clone());
        }
        bail!("Aster ACP 在初始化完成前退出")
    };
    let result = tokio::time::timeout(Duration::from_secs(8), reply).await;
    let _ = child.kill().await;
    result.context("Aster ACP 初始化超时")?
}

pub(crate) async fn probe(engine: Option<&Engine>, workspace: &Path) -> ProviderStatusInfo {
    let mut provider = seed();
    let Some(binary) = find_binary() else {
        return provider;
    };
    let result = async {
        let engine = engine.context("Kar 工作台服务不可用")?;
        let id = register(engine, &binary).await?;
        let meta = metadata(&binary, workspace).await?;
        Result::<_>::Ok((id, meta))
    }
    .await;
    match result {
        Ok((id, meta)) => {
            provider.id = id;
            provider.workspace_commands = crate::kar_bridge::session_commands::advertised_commands(
                &meta["availableCommands"],
                "Aster",
            )
            .map(|commands| (workspace.to_owned(), commands));
            let catalog = &meta["modelState"];
            provider.availability = ProviderAvailability::Available {
                version: meta["agentVersion"].as_str().unwrap_or_default().into(),
                command: Some(binary),
            };
            provider.auth = ProviderAuthState::Unknown {
                note: "模型认证由 Aster 配置管理".into(),
            };
            provider.models = catalog["availableModels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|row| {
                    let id = row["modelId"].as_str()?;
                    Some(ProviderModelOption {
                        id: id.into(),
                        label: row["name"].as_str().unwrap_or(id).into(),
                        is_default: catalog["currentModelId"] == id,
                        // Aster's effort-setting wire contract differs from Kar's generic ACP adapters.
                        effort_levels: vec![],
                    })
                })
                .collect();
        }
        Err(err) => {
            provider.availability = ProviderAvailability::Failed {
                reason: err.to_string(),
            }
        }
    }
    provider
}

// Digests published in the official aster-acp-v9.3.1 release SHA256SUMS.
fn release() -> Option<(&'static str, &'static str)> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some((
            "macos.tar.gz",
            "063e6a1ec61e4adeef7133b738ae0bf3e6860f356b544a1859c5c22265b5e6fc",
        )),
        ("linux", "aarch64") => Some((
            "linux-musl-arm64.tar.gz",
            "7cee185cf26f3df144551541916c389b1c72fd0c8bcafd33e55255f79a093657",
        )),
        ("linux", "x86_64") => Some((
            "linux-musl.tar.gz",
            "574573febaed7d10d74e8327495f77c3df4087422e169ccd2e7ddcf56106dbca",
        )),
        ("windows", "x86_64") => Some((
            "windows.zip",
            "1811b75b9ef9f484c049fbab20e281f11de401bfa01c2333a3967ed8889a4c8a",
        )),
        _ => None,
    }
}

/// Fetch only the pinned release on explicit installation. Publish the executable atomically;
/// running and previously saved sessions can continue using their resolved binary.
pub(crate) fn install() -> Result<()> {
    let (platform, expected) = release().context("Aster 尚未提供适用于此平台的安装包")?;
    let dir = managed_dir().context("未找到用户目录")?;
    std::fs::create_dir_all(&dir)?;
    let url = format!(
        "https://github.com/aster-ai-lab/aster/releases/download/aster-acp-v{VERSION}/aster-acp-v{VERSION}-{platform}"
    );
    let mut body = ureq::get(&url)
        .call()?
        .into_body()
        .into_reader()
        .take(96 << 20);
    let mut archive = tempfile::tempfile()?;
    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buf = [0; 65536];
    loop {
        let n = body.read(&mut buf)?;
        if n == 0 {
            break;
        }
        digest.update(&buf[..n]);
        archive.write_all(&buf[..n])?;
    }
    let actual: String = digest
        .finish()
        .as_ref()
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect();
    ensure!(actual == expected, "Aster 安装包校验失败");
    use std::io::{Seek, copy};
    archive.rewind()?;
    let mut output = tempfile::NamedTempFile::new_in(&dir)?;
    if cfg!(windows) {
        let mut zip = zip::ZipArchive::new(archive)?;
        copy(&mut zip.by_name(filename())?, &mut output)?;
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
        let mut found = false;
        for entry in tar.entries()? {
            let mut file = entry?;
            if file.header().entry_type().is_file()
                && file.path()?.file_name() == Some(OsStr::new(filename()))
            {
                ensure!(!found, "Aster 安装包包含重复程序");
                copy(&mut file, &mut output)?;
                found = true;
            }
        }
        ensure!(found, "Aster 安装包缺少 ACP 程序");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        output
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o755))?;
    }
    output.as_file().sync_all()?;
    output.persist(dir.join(filename()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kar::{
        ApprovalMode, SessionOptions,
        chat::{ChatStore, Item, Role, TurnStatus},
    };
    use uuid::Uuid;

    /// Explicitly run against an installed, unmodified official executable. Only the external
    /// model endpoint is a deterministic loopback fixture; Kar and ACP use their real stores.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires installed Aster ACP"]
    async fn real_aster_create_plan_prompt_resume_goal() -> Result<()> {
        let binary = find_binary().context("install official aster-acp to run this test")?;
        let dir = tempfile::tempdir()?;
        let workspace = dir.path().join("workspace");
        std::fs::create_dir(&workspace)?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?;
        let entered = std::sync::Arc::new(tokio::sync::Notify::new());
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        #[derive(Default)]
        struct ModelProbe {
            paused: std::sync::atomic::AtomicBool,
            called: std::sync::atomic::AtomicBool,
            tool_reply: std::sync::Mutex<Option<String>>,
        }
        const TOOL: &str = "mcp__kar_workbench__orchestrator_capabilities";
        let probe = std::sync::Arc::new(ModelProbe::default());
        let model_probe = probe.clone();
        let pause = entered.clone();
        let resume = release.clone();
        let model = move |axum::Json(body): axum::Json<Value>| {
            let probe = model_probe.clone();
            let pause = pause.clone();
            let resume = resume.clone();
            async move {
                use std::sync::atomic::Ordering::Relaxed;
                let tools = body["tools"].as_array().into_iter().flatten();
                let can_call = tools.into_iter().any(|t| t["function"]["name"] == TOOL);
                let messages = body["messages"].to_string();
                if let Some(msg) = body["messages"].as_array().and_then(|msgs| {
                    msgs.iter().find(|msg| {
                        msg["role"] == "tool"
                            && msg["content"].to_string().contains("provider_session")
                    })
                }) {
                    *probe.tool_reply.lock().unwrap() = Some(msg["content"].to_string());
                }
                if messages.contains("DEFERRED_TEST_GATE") && !probe.paused.swap(true, Relaxed) {
                    pause.notify_one();
                    resume.notified().await;
                }
                let (delta, finish) = if can_call
                    && messages.contains("Confirm receiving the conversation.")
                    && !probe.called.swap(true, Relaxed)
                {
                    (
                        json!({"tool_calls":[{"index":0,"id":"inspect_kar_scope","type":"function","function":{"name":TOOL,"arguments":"{}"}}]}),
                        "tool_calls",
                    )
                } else {
                    (json!({"content":"KWORK_NATIVE_ASTER_REPLY"}), "stop")
                };
                let event = json!({"choices":[{"delta":delta}]});
                let end = json!({"choices":[{"delta":{},"finish_reason":finish}]});
                (
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    format!("data: {event}\n\ndata: {end}\n\ndata: [DONE]\n\n"),
                )
            }
        };
        let app = axum::Router::new().route("/v1/chat/completions", axum::routing::post(model));
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let config = dir.path().join("test.acl");
        std::fs::write(
            &config,
            format!(
                "default_model = \"mock/test\"\nproviders \"mock\" {{\n base_url = \"http://{port}/v1\"\n api_key = \"loopback\"\n models \"test\" {{\n }}\n}}\n"
            ),
        )?;
        let store = ChatStore::open(dir.path().join("chats"))?;
        let engine = Engine::open(store.clone(), &workspace)?;
        let registered = register(&engine, &binary).await?;
        ensure!(is_aster(&registered));
        // Persist the isolated provider HOME and ACL in the test profile itself. Reopening
        // through a fresh desktop worker must launch the same provider environment.
        #[cfg(unix)]
        let fixture_profile = |name: &'static str| {
            let engine = engine.clone();
            let args = json!([
                format!("ASTER_CONFIG={}", config.display()),
                format!("HOME={}", dir.path().display()),
                binary
            ]);
            async move {
                let result=engine.request(Method::PUT,&format!("/acp-registry/profiles/{name}"),Some(json!({
                    "source":"local","name":name,"command_path":"/usr/bin/env","command_args":args
                }))).await?;
                Ok::<String, anyhow::Error>(
                    result["runtime"]
                        .as_str()
                        .context("missing isolated profile")?
                        .to_owned(),
                )
            }
        };
        #[cfg(unix)]
        let pinned = fixture_profile("aster-source-fixture").await?;
        #[cfg(not(unix))]
        let pinned = registered;
        let configure = |opts: &mut SessionOptions| {
            opts.env.insert("HOME".into(), dir.path().into());
            opts.env
                .insert("ASTER_CONFIG".into(), config.clone().into());
        };
        let mut opts = SessionOptions::new(&workspace);
        configure(&mut opts);
        opts.approvals = ApprovalMode::Host;
        opts.model = Some("mock/test".into());
        opts.settings.acp_mode = Some("plan".into());
        let session = store.create(&pinned, None, opts).await?;
        let id = session.info().id;
        let native = session.info().native_id.clone();
        let sent = session
            .send(Uuid::new_v4(), "Reply with the agreed marker.")
            .await?;
        ensure!(
            sent.status == TurnStatus::Completed,
            "provider turn failed: {:?}",
            sent.failure
        );
        ensure!(sent.items.iter().any(|row| matches!(&row.item, Item::Message {role: Role::Assistant, text, ..} if text.contains("KWORK_NATIVE_ASTER_REPLY"))), "missing native model reply");
        session.close().await?;
        let resumed = store
            .resume_with(id, |opts| {
                configure(opts);
                opts.settings.acp_mode = Some("default".into());
            })
            .await?;
        ensure!(
            resumed.info().native_id == native,
            "native session changed across reopen"
        );
        let status = resumed.send(Uuid::new_v4(), "/goal status").await?;
        ensure!(
            status.status == TurnStatus::Completed,
            "Aster goal status failed: {:?}",
            status.failure
        );
        let output = status
            .items
            .iter()
            .filter_map(|row| match &row.item {
                Item::Message {
                    role: Role::Assistant,
                    text,
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        ensure!(
            !output.is_empty() && !output.contains("KWORK_NATIVE_ASTER_REPLY"),
            "goal command was sent to model: {output}"
        );
        resumed.close().await?;
        ensure!(
            store.history(id)?.turns.len() == 2,
            "conversation history was lost"
        );
        println!(
            "Aster official ACP: PLAN → streamed fixture-model reply → close → same native session DEFAULT → native /goal status: {output}"
        );
        use crate::kar_bridge::{
            KarBridgeEvent, KarBridgeHandle, KarCommand, SidebarAction, SidebarMetadata,
        };
        async fn observe(
            w: &KarBridgeHandle,
            id: Uuid,
            check: impl Fn(&SidebarMetadata) -> bool,
        ) -> Result<()> {
            tokio::time::timeout(Duration::from_secs(6), async {
                loop {
                    if let KarBridgeEvent::SidebarMetadata { rows, .. } = w.rx.recv().await? {
                        if rows.get(&id).is_some_and(&check) {
                            return Ok::<_, anyhow::Error>(());
                        }
                    }
                }
            })
            .await
            .context("timeout waiting for metadata from other window")??;
            Ok(())
        }
        let chats = store.directory().to_owned();
        std::fs::write(
            chats.join("kar_desktop_chat_titles.json"),
            serde_json::to_vec(&json!({"by_chat":{id.to_string():"Legacy title"}}))?,
        )?;
        let left = KarBridgeHandle::spawn(chats.clone(), workspace.clone());
        let right = KarBridgeHandle::spawn(chats.clone(), workspace.clone());
        observe(&left, id, |m| m.title.as_deref() == Some("Legacy title")).await?;
        observe(&right, id, |m| m.title.as_deref() == Some("Legacy title")).await?;
        left.send(KarCommand::RenameChat {
            chat_id: id,
            title: "Renamed on left".into(),
        });
        observe(&right, id, |m| {
            m.title.as_deref() == Some("Renamed on left")
        })
        .await?;
        left.send(KarCommand::UpdateSidebar {
            chat_id: id,
            action: SidebarAction::Pin(true),
        });
        observe(&right, id, |m| m.pinned).await?;
        left.send(KarCommand::UpdateSidebar {
            chat_id: id,
            action: SidebarAction::Settle(true),
        });
        observe(&right, id, |m| m.settled).await?;
        left.send(KarCommand::UpdateSidebar {
            chat_id: id,
            action: SidebarAction::Archive(true),
        });
        observe(&right, id, |m| m.archived).await?;
        right.send(KarCommand::SelectLocalChat {
            chat_id: id,
            prefer_read_only: false,
        });
        tokio::time::timeout(Duration::from_secs(6),async {
            loop {
                if matches!(right.rx.recv().await?,KarBridgeEvent::AccessModeChanged(crate::types::ChatAccessMode::ArchivedReadOnly{chat_id,..}) if chat_id == id) {return Ok::<_,anyhow::Error>(());}
            }
        }).await.context("archived chat did not open read-only")??;
        left.send(KarCommand::UpdateSidebar {
            chat_id: id,
            action: SidebarAction::Archive(false),
        });
        observe(&right, id, |m| {
            !m.archived && m.title.as_deref() == Some("Renamed on left")
        })
        .await?;
        #[cfg(unix)]
        {
            let target_runtime = fixture_profile("aster-target-fixture").await?;
            left.send(KarCommand::SelectLocalChat {
                chat_id: id,
                prefer_read_only: false,
            });
            tokio::time::timeout(Duration::from_secs(9),async {
                loop {
                    if let KarBridgeEvent::AccessModeChanged(mode) = left.rx.recv().await? {
                        ensure!(matches!(mode,crate::types::ChatAccessMode::WriteController{chat_id} if chat_id==id),"initial sender not writable: {mode:?}");
                        return Ok::<_,anyhow::Error>(());
                    }
                }
            }).await??;
            let side_request = Uuid::new_v4();
            left.send(KarCommand::AskSideQuestion {
                request_id: side_request,
                chat_id: id,
                question: "Unsupported provider test".into(),
            });
            tokio::time::timeout(Duration::from_secs(4), async {
                loop {
                    if let KarBridgeEvent::SideQuestionAnswered {
                        request_id, result, ..
                    } = left.rx.recv().await?
                    {
                        if request_id == side_request {
                            ensure!(
                                result.is_err(),
                                "unsupported provider produced a pretend answer"
                            );
                            return Ok::<_, anyhow::Error>(());
                        }
                    }
                }
            })
            .await??;
            ensure!(store.history(id)?.turns.len() == 2);
            left.send(KarCommand::HandoffChat {
                source_chat: Some(id),
                target_runtime: target_runtime.clone(),
                target_model: Some("mock/test".into()),
                user_note: String::new(),
                budget_bytes: 16384,
            });
            tokio::time::timeout(Duration::from_secs(15), async {
                loop {
                    match left.rx.recv().await? {
                        KarBridgeEvent::HandoffCompleted {
                            chat_id: Some(chat),
                            target_runtime: runtime,
                            ..
                        } if chat == id && runtime == target_runtime => {
                            return Ok::<_, anyhow::Error>(());
                        }
                        KarBridgeEvent::ToastMessage {
                            text,
                            warning: true,
                        } if text.contains("交接") => bail!("{text}"),
                        _ => (),
                    }
                }
            })
            .await
            .context("timed out waiting for handoff receipt")??;
            let hand = store
                .history(id)?
                .handoffs
                .pop()
                .context("missing durable handoff")?;
            ensure!(
                hand.status == kar::chat::HandoffStatus::Ready
                    && hand.from.runtime == pinned
                    && hand.target.is_some_and(|t| t.runtime == target_runtime)
            );
            left.send(KarCommand::CloseChat);
            left.send(KarCommand::SelectLocalChat {
                chat_id: id,
                prefer_read_only: false,
            });
            tokio::time::timeout(Duration::from_secs(9),async {
                loop {
                    if matches!(left.rx.recv().await?,KarBridgeEvent::HostedSession(chat) if chat==id) {return Ok::<_,anyhow::Error>(());}
                }
            }).await.context("target writer unavailable on reopen")??;
            right.send(KarCommand::SelectLocalChat {
                chat_id: id,
                prefer_read_only: false,
            });
            tokio::time::timeout(Duration::from_secs(9),async {
                loop {
                    if matches!(right.rx.recv().await?,KarBridgeEvent::AccessModeChanged(crate::types::ChatAccessMode::LocalReadOnlySubscriber{chat_id,..}) if chat_id==id) {return Ok::<_,anyhow::Error>(());}
                }
            }).await.context("second window incorrectly acquired writer")??;
            for prompt in ["Confirm receiving the conversation.", "/goal status"] {
                let req = Uuid::new_v4();
                left.send(KarCommand::SendPrompt {
                    request_id: req,
                    raw_user_text: prompt.into(),
                    payload: prompt.into(),
                    target: None,
                    attachments: vec![],
                });
                tokio::time::timeout(Duration::from_secs(14), async {
                    loop {
                        match left.rx.recv().await? {
                            KarBridgeEvent::PromptLifecycle { attempt }
                                if attempt.request_id == req && !attempt.in_flight =>
                            {
                                ensure!(
                                    attempt.last_error.is_none(),
                                    "hosted send failed: {:?}",
                                    attempt.last_error
                                );
                                return Ok::<_, anyhow::Error>(());
                            }
                            KarBridgeEvent::ChatStateUpdated(snapshot) => {
                                for request in snapshot.pending_requests {
                                    if request
                                        .body
                                        .to_string()
                                        .contains("orchestrator_capabilities")
                                    {
                                        left.send(KarCommand::DecidePermission {
                                            request,
                                            decision: kar::PermissionDecision::AllowOnce,
                                        });
                                    }
                                }
                            }
                            _ => (),
                        }
                    }
                })
                .await
                .context("hosted prompt did not complete")??;
            }
            let history = store.history(id)?;
            ensure!(
                history.turns.len() == 4
                    && history
                        .turns
                        .iter()
                        .all(|t| t.status == TurnStatus::Completed),
                "turns after handoff: {:?}",
                history
                    .turns
                    .iter()
                    .map(|t| (&t.status, &t.failure))
                    .collect::<Vec<_>>()
            );
            ensure!(history.handoffs[0].input.is_some() && history.chat.runtime == target_runtime);
            ensure!(history.turns[3].items.iter().any(|row| matches!(&row.item,Item::Message{role:Role::Assistant,text,..} if text.contains("goal") && !text.contains("KWORK_NATIVE_ASTER_REPLY"))),"native target command did not run");
            {
                let result = probe.tool_reply.lock().unwrap();
                let result = result
                    .as_deref()
                    .context("native MCP tool did not return its provider session grant")?;
                ensure!(
                    result.contains(&id.to_string()) && result.contains(&target_runtime),
                    "native MCP grant belongs to another chat/provider"
                );
                println!(
                    "Aster executed Kar orchestrator_capabilities. The returned provider grant was scoped to the same chat UUID and the receiving runtime."
                );
            }
            let gated = "DEFERRED_TEST_GATE";
            left.send(KarCommand::SendPrompt {
                request_id: Uuid::new_v4(),
                raw_user_text: gated.into(),
                payload: gated.into(),
                target: None,
                attachments: vec![],
            });
            tokio::time::timeout(Duration::from_secs(7), entered.notified())
                .await
                .context("provider did not enter gated turn")?;
            left.send(KarCommand::HandoffChat {
                source_chat: Some(id),
                target_runtime: pinned.clone(),
                target_model: Some("mock/test".into()),
                user_note: String::new(),
                budget_bytes: 16384,
            });
            tokio::time::timeout(Duration::from_secs(7), async {
                loop {
                    match left.rx.recv().await? {
                        KarBridgeEvent::ToastMessage {
                            text,
                            warning: false,
                        } if text.contains("已排队") => return Ok::<_, anyhow::Error>(()),
                        KarBridgeEvent::ToastMessage {
                            text,
                            warning: true,
                        } if text.contains("交接") => bail!("{text}"),
                        _ => (),
                    }
                }
            })
            .await
            .context("no prepared handoff while running")??;
            ensure!(
                store.history(id)?.chat.runtime == target_runtime,
                "handoff displaced active sender"
            );
            release.notify_one();
            tokio::time::timeout(Duration::from_secs(12), async {
                loop {
                    if let KarBridgeEvent::HandoffReceipt {
                        chat_id,
                        latest: Some(row),
                    } = left.rx.recv().await?
                    {
                        if chat_id == id && row.request.runtime == pinned {
                            ensure!(
                                row.status != kar::chat::HandoffStatus::Failed,
                                "{:?}",
                                row.error
                            );
                            if row.status == kar::chat::HandoffStatus::Ready {
                                return Ok::<_, anyhow::Error>(());
                            }
                        }
                    }
                }
            })
            .await
            .with_context(||{
                let history=store.history(id).unwrap();
                format!("deferred handoff did not settle: turns {:?}, receipts {:?}, waiting {:?}, tasks {}, notifications {}",history.turns.iter().map(|t|&t.status).collect::<Vec<_>>(),history.handoffs.iter().map(|h|(&h.status,&h.error)).collect::<Vec<_>>(),history.pending_requests.len(),history.tasks.len(),history.notifications.len())
            })??;
            ensure!(
                store.history(id)?.chat.runtime == pinned,
                "deferred target binding lost"
            );
            println!(
                "Deferred handoff remained queued while the original native provider was generating, then rebound the same chat after turn completion."
            );
            println!(
                "Same-chat Handoff Ready: source retained, target writer reopened, second window read-only, receipt claimed by first follow-up, subsequent native /goal status delivered."
            );
        }
        left.send(KarCommand::Shutdown);
        right.send(KarCommand::Shutdown);
        println!(
            "Two desktop windows: legacy title migration, rename, pin, settle, archive, read-only reopen, unarchive verified through the shared Kar store."
        );
        server.abort();
        Ok(())
    }
}
