//! An ephemeral, read-only Codex planning thread. The original Kar controller owns implementation.
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use kar::chat::ChatSession;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{ChildStdin, ChildStdout, Command},
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::timeout,
};
use uuid::Uuid;

use super::{KarBridgeEvent, WorkerState, augmented_path_os};

#[derive(Clone, Debug)]
pub struct PlanQuestion {
    pub id: String,
    pub prompt: String,
    pub options: Vec<PlanOption>,
}

#[derive(Clone, Debug)]
pub struct PlanOption {
    pub label: String,
    pub description: Option<String>,
}

type Answers = BTreeMap<String, String>;

pub(super) struct PlannerTask {
    chat: Uuid,
    request: Uuid,
    cancel: oneshot::Sender<()>,
    answers: mpsc::Sender<Answers>,
    task: JoinHandle<()>,
}

impl PlannerTask {
    pub(super) fn cancel(self) {
        let _ = self.cancel.send(());
    }
    pub(super) fn matches(&self, chat: Uuid, request: Uuid) -> bool {
        (self.chat, self.request) == (chat, request)
    }
    pub(super) fn answer(&self, answers: Answers) {
        let _ = self.answers.try_send(answers);
    }
}

pub(super) enum Source {
    CurrentChat,
    Workspace {
        workspace: PathBuf,
        program: Option<PathBuf>,
        model: Option<String>,
        effort: Option<String>,
    },
}

pub(super) fn start(
    state: &mut WorkerState,
    chat: Uuid,
    request: Uuid,
    topic: String,
    previous: Option<String>,
    source: Source,
) {
    let result = (|| -> Result<_> {
        if let Some(active) = state.planner.as_ref().filter(|p| !p.task.is_finished()) {
            if active.matches(chat, request) {
                return Ok(None);
            }
            bail!("已有计划正在生成");
        }
        ensure!(!topic.trim().is_empty(), "请输入计划主题");
        let launch = match source {
            Source::CurrentChat => {
                let session = state
                    .active_controller
                    .as_deref()
                    .filter(|s| s.info().id == chat && s.info().protocol == kar::Protocol::Codex)
                    .context("原生计划需要当前可写的 Codex 会话")?;
                Launch::new(session, &topic, previous.as_deref())?
            }
            Source::Workspace {
                workspace,
                program,
                model,
                effort,
            } => Launch::draft(
                workspace,
                program,
                model,
                effort,
                &topic,
                previous.as_deref(),
            )?,
        };
        Ok(Some(launch))
    })();
    let launch = match result {
        Ok(Some(launch)) => launch,
        Ok(None) => return,
        Err(error) => {
            state.emit(KarBridgeEvent::PlanGenerated {
                chat_id: chat,
                request_id: request,
                result: Err(error.to_string()),
            });
            return;
        }
    };
    let (cancel, cancelled) = oneshot::channel();
    let (answers, rx) = mpsc::channel(1);
    let events = state.evt_tx.clone();
    let task = tokio::spawn(async move {
        let result = launch.run(chat, request, &events, rx, cancelled).await;
        let _ = events
            .send(KarBridgeEvent::PlanGenerated {
                chat_id: chat,
                request_id: request,
                result: result.map_err(|e| format!("{e:#}")),
            })
            .await;
    });
    state.planner = Some(PlannerTask {
        chat,
        request,
        cancel,
        answers,
        task,
    });
}

struct Launch {
    program: PathBuf,
    cwd: PathBuf,
    original: Option<String>,
    prompt: String,
    model: Option<String>,
    effort: Option<String>,
    unset_env: std::collections::BTreeSet<String>,
}

impl Launch {
    fn new(session: &ChatSession, topic: &str, previous: Option<&str>) -> Result<Self> {
        let info = session.info();
        let history = session.history()?;
        // Let Codex fork the native history, including tool results, images and compaction.
        // A newly created thread has no persisted rollout until its first turn.
        let original = (!history.turns.is_empty()).then(|| info.native_id.clone());
        Ok(Self {
            program: info.program.clone(),
            cwd: info.workspace.clone(),
            original,
            prompt: Self::instructions(topic, previous),
            model: info.model.clone(),
            effort: info.settings.effort.clone(),
            unset_env: info.unset_env.clone(),
        })
    }

    fn instructions(topic: &str, previous: Option<&str>) -> String {
        let mut prompt = String::from(
            "Prepare a proposed implementation plan for the user's request. Use the user's language. Ask for clarification when needed. Do not implement.\n\n",
        );
        if let Some(previous) = previous {
            prompt.push_str("Previous proposed plan:\n");
            prompt.push_str(previous);
            prompt.push_str("\n\n");
        }
        prompt.push_str("Current request:\n");
        prompt.push_str(topic);
        prompt
    }

    fn draft(
        workspace: PathBuf,
        program: Option<PathBuf>,
        model: Option<String>,
        effort: Option<String>,
        topic: &str,
        previous: Option<&str>,
    ) -> Result<Self> {
        let cwd = workspace.canonicalize().context("无法打开计划工作区")?;
        ensure!(cwd.is_dir(), "计划工作区必须是文件夹");
        // Same precedence as ChatStore::create: the discovered executable overrides the runtime default.
        let program = match program {
            Some(path) => path,
            None => {
                kar::runtime("codex")
                    .context("找不到 Codex 运行时")?
                    .program
            }
        };
        let program = if program.is_relative() && program.components().count() > 1 {
            program
                .canonicalize()
                .context("找不到当前配置的 Codex 可执行程序")?
        } else {
            program
        };
        Ok(Self {
            cwd,
            program,
            model,
            effort,
            original: None,
            unset_env: Default::default(),
            prompt: Self::instructions(topic, previous),
        })
    }

    async fn run(
        self,
        chat: Uuid,
        request: Uuid,
        events: &async_channel::Sender<KarBridgeEvent>,
        mut answers: mpsc::Receiver<Answers>,
        mut cancelled: oneshot::Receiver<()>,
    ) -> Result<String> {
        let mut command = Command::new(&self.program);
        command
            .arg("app-server")
            .current_dir(&self.cwd)
            .env("PATH", augmented_path_os());
        for key in &self.unset_env {
            command.env_remove(key);
        }
        let mut child = command
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| {
                format!(
                    "无法启动当前配置的 Codex 可执行程序（{}）",
                    self.program.display()
                )
            })?;
        let mut wire = Wire {
            input: child.stdin.take().context("无法连接计划进程")?,
            lines: BufReader::new(child.stdout.take().context("无法读取计划输出")?).lines(),
            next: 0,
            buffered: VecDeque::new(),
        };
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Err(anyhow!("已取消计划")),
            result = self.conversation(&mut wire, chat, request, events, &mut answers) => result,
            status = child.wait() => Err(anyhow!("Codex 计划进程提前退出（{}）", status?)),
        };
        drop(wire);
        let _ = child.kill().await;
        result
    }

    async fn conversation(
        self,
        wire: &mut Wire,
        chat: Uuid,
        request: Uuid,
        events: &async_channel::Sender<KarBridgeEvent>,
        answers: &mut mpsc::Receiver<Answers>,
    ) -> Result<String> {
        let (thread, settings) = timeout(Duration::from_secs(45), self.open(wire))
            .await
            .context("Codex 计划初始化超时")??;
        let mut input = self.prompt;
        loop {
            let params = json!({"threadId":thread,"input":[{"type":"text","text":input,"text_elements":[]}],
                    "sandboxPolicy":{"type":"readOnly","networkAccess":false},"approvalPolicy":"never","collaborationMode":settings});
            let opened = timeout(
                Duration::from_secs(45),
                wire.call("turn/start", params, true),
            )
            .await
            .context("Codex 启动计划回合超时")??;
            let turn = string(&opened["turn"], "id")?;
            let mut plan = None;
            let mut message = None;
            let mut previous_progress = None;
            loop {
                let packet = timeout(Duration::from_secs(600), wire.next_packet())
                    .await
                    .context("等待 Codex 计划输出超时")??;
                let method = packet["method"].as_str().unwrap_or("");
                let params = &packet["params"];
                if let Some(id) = packet.get("id") {
                    if method == "item/tool/requestUserInput"
                        && params["threadId"] == thread
                        && params["turnId"] == turn
                    {
                        let questions = read_questions(&params["questions"])?;
                        let reply = ask(chat, request, questions, events, answers).await?;
                        let reply: BTreeMap<_, _> = reply
                            .into_iter()
                            .map(|(id, text)| (id, json!({"answers":[text]})))
                            .collect();
                        wire.send(json!({"id":id,"result":{"answers":reply}}))
                            .await?;
                    } else {
                        wire.deny(id).await?;
                    }
                    continue;
                }
                if params["threadId"] != thread {
                    continue;
                }
                if method == "item/plan/delta"
                    && params["turnId"] == turn
                    && let (Some(item), Some(delta)) =
                        (params["itemId"].as_str(), params["delta"].as_str())
                    && !delta.is_empty()
                {
                    events
                        .send(KarBridgeEvent::PlanDelta {
                            chat_id: chat,
                            request_id: request,
                            item: item.to_owned(),
                            delta: delta.to_owned(),
                        })
                        .await?;
                }
                if params["turnId"] == turn
                    && let Some(label) = progress_label(method, params)
                    && previous_progress.as_ref() != Some(&label)
                {
                    previous_progress = Some(label.clone());
                    events
                        .send(KarBridgeEvent::PlanProgress {
                            chat_id: chat,
                            request_id: request,
                            label,
                        })
                        .await?;
                }
                if method == "item/completed" && params["turnId"] == turn {
                    let item = &params["item"];
                    if item["type"] == "plan" {
                        plan = item["text"]
                            .as_str()
                            .filter(|s| !s.trim().is_empty())
                            .map(str::to_owned);
                    } else if item["type"] == "agentMessage"
                        && (item["phase"] == "final_answer" || item["phase"].is_null())
                    {
                        message = item["text"]
                            .as_str()
                            .filter(|s| !s.trim().is_empty())
                            .map(str::to_owned);
                    }
                }
                if method == "turn/completed" && params["turn"]["id"] == turn {
                    ensure!(
                        params["turn"]["status"] == "completed",
                        "Codex 未完成计划回合"
                    );
                    if let Some(plan) = plan {
                        return Ok(plan);
                    }
                    let prompt = message.context("Codex 未返回可实施的原生计划")?;
                    let key = format!("feedback-{turn}");
                    let question = PlanQuestion {
                        id: key.clone(),
                        prompt,
                        options: Vec::new(),
                    };
                    let reply = ask(chat, request, vec![question], events, answers).await?;
                    input = reply[&key].clone();
                    break;
                }
            }
        }
    }

    async fn open(&self, wire: &mut Wire) -> Result<(String, Value)> {
        wire.call("initialize",json!({"clientInfo":{"name":"kwrok_planner","title":"Kwrok","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}),false).await?;
        wire.send(json!({"method":"initialized"})).await?;
        let config = wire
            .call(
                "config/read",
                json!({"cwd":self.cwd,"includeLayers":false}),
                false,
            )
            .await?;
        let config = config["config"]
            .as_object()
            .context("无法核对工作区的 MCP 配置")?;
        let mut servers = serde_json::Map::new();
        if let Some(known) = config.get("mcp_servers").filter(|v| !v.is_null()) {
            for name in known.as_object().context("不支持的 MCP 配置")?.keys() {
                servers.insert(name.clone(), json!({"enabled":false}));
            }
        }
        // Codex Desktop can inject this service independently of the workspace configuration.
        servers.insert("node_repl".to_string(), json!({"enabled":false}));
        let features: BTreeMap<_, _> = [
            "apps",
            "plugins",
            "hooks",
            "browser_use",
            "browser_use_external",
            "in_app_browser",
            "computer_use",
            "in_app_local_automation",
            "image_generation",
            "multi_agent",
            "multi_agent_v2",
            "goals",
        ]
        .into_iter()
        .map(|k| (k, false))
        .collect();
        let mut start = json!({"cwd":self.cwd,"sandbox":"read-only","approvalPolicy":"never","ephemeral":true,"config":{"features":features,"mcp_servers":servers,"web_search":"disabled"}});
        if let Some(model) = &self.model {
            start["model"] = model.clone().into();
        }
        let method = if let Some(original) = &self.original {
            start["threadId"] = json!(original);
            // Omit the response's turn list; the fork still inherits all native history.
            start["excludeTurns"] = true.into();
            "thread/fork"
        } else {
            start["dynamicTools"] = json!([]);
            "thread/start"
        };
        let opened = wire.call(method, start, false).await?;
        let thread = string(&opened["thread"], "id")?;
        ensure!(
            self.original.as_deref() != Some(thread)
                && opened["thread"]["ephemeral"] == true
                && opened["approvalPolicy"] == "never"
                && opened["sandbox"]["type"] == "readOnly"
                && opened["sandbox"]["networkAccess"] == false,
            "运行时未确认独立只读计划环境"
        );
        let mut cursor = None::<String>;
        let mut cursors = std::collections::BTreeSet::new();
        loop {
            let inventory = wire
                .call(
                    "mcpServerStatus/list",
                    json!({"threadId":thread, "detail":"full", "limit":100, "cursor":cursor}),
                    false,
                )
                .await?;
            ensure!(
                inventory["data"].as_array().is_some_and(|items| items
                    .iter()
                    .all(|item| item["runtimeStatus"] == "disabled")),
                "计划环境仍有启用的 MCP 服务"
            );
            let Some(next) = inventory["nextCursor"].as_str() else {
                break;
            };
            ensure!(
                cursors.insert(next.to_owned()),
                "Codex 返回了重复的 MCP 目录游标"
            );
            cursor = Some(next.to_owned());
        }
        let model = string(&opened, "model")?;
        let effort = self
            .effort
            .as_deref()
            .or_else(|| opened["reasoningEffort"].as_str());
        let settings = json!({"mode":"plan","settings":{"model":model,"reasoning_effort":effort,"developer_instructions":null}});
        let available = wire
            .call("collaborationMode/list", json!({}), false)
            .await?;
        ensure!(
            available["data"]
                .as_array()
                .is_some_and(|list| list.iter().any(|mode| mode["mode"] == "plan")),
            "当前 Codex 不支持原生计划模式"
        );
        Ok((thread.to_owned(), settings))
    }
}

/// Only public action titles and the active update-plan step belong in the status line.
fn progress_label(method: &str, params: &Value) -> Option<String> {
    let item = &params["item"];
    let label = match method {
        "turn/plan/updated" => params["plan"]
            .as_array()?
            .iter()
            .find(|step| step["status"] == "inProgress")?["step"]
            .as_str()?
            .to_owned(),
        "item/started" => match item["type"].as_str()? {
            "commandExecution" => {
                let action = item["commandActions"]
                    .as_array()
                    .and_then(|list| list.first());
                let command = action
                    .and_then(|a| a["command"].as_str())
                    .or_else(|| item["command"].as_str())?;
                // Never expose shell arguments, inline scripts, environment values or tool output.
                let command = crate::types::unwrap_shell_command(command);
                let executable = command.split_whitespace().next()?.trim_matches(['\'', '"']);
                let executable = std::path::Path::new(executable).file_name()?.to_str()?;
                if !executable
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
                {
                    return None;
                }
                let file = action
                    .and_then(|a| a["path"].as_str().or_else(|| a["name"].as_str()))
                    .and_then(|s| std::path::Path::new(s).file_name())
                    .and_then(|s| s.to_str());
                file.map_or_else(
                    || executable.to_owned(),
                    |name| format!("{executable} · {name}"),
                )
            }
            "fileChange" => {
                let file = item["changes"].as_array()?.first()?;
                let name = std::path::Path::new(file["path"].as_str()?)
                    .file_name()?
                    .to_str()?;
                let operation = match file["kind"]["type"].as_str()? {
                    "add" => "+",
                    "delete" => "−",
                    "update" => "Δ",
                    _ => return None,
                };
                format!("{operation} {name}")
            }
            "mcpToolCall" | "dynamicToolCall" => item["tool"].as_str()?.to_owned(),
            _ => return None,
        },
        _ => return None,
    };
    let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
    (!label.is_empty()).then(|| crate::studio_hub::truncate_chars(&label, 120))
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Codex 计划协议缺少 {key}"))
}

fn read_questions(value: &Value) -> Result<Vec<PlanQuestion>> {
    let mut questions = Vec::new();
    for q in value.as_array().context("Codex 返回了无效的问题")? {
        ensure!(q["isSecret"] != true, "计划器不接收密码或其他机密答案");
        let id = string(q, "id")?.to_owned();
        ensure!(
            !questions.iter().any(|known: &PlanQuestion| known.id == id),
            "Codex 返回了重复的问题"
        );
        questions.push(PlanQuestion {
            id,
            prompt: string(q, "question")?.into(),
            options: q["options"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|o| {
                    Some(PlanOption {
                        label: o["label"].as_str()?.to_owned(),
                        description: o["description"]
                            .as_str()
                            .filter(|s| !s.trim().is_empty())
                            .map(str::to_owned),
                    })
                })
                .collect(),
        });
    }
    ensure!(!questions.is_empty(), "Codex 返回了空问题");
    Ok(questions)
}

async fn ask(
    chat: Uuid,
    request: Uuid,
    questions: Vec<PlanQuestion>,
    events: &async_channel::Sender<KarBridgeEvent>,
    answers: &mut mpsc::Receiver<Answers>,
) -> Result<Answers> {
    let ids: Vec<_> = questions.iter().map(|q| q.id.clone()).collect();
    events
        .send(KarBridgeEvent::PlanQuestions {
            chat_id: chat,
            request_id: request,
            questions,
        })
        .await?;
    while let Some(reply) = answers.recv().await {
        if reply.len() == ids.len()
            && ids
                .iter()
                .all(|id| reply.get(id).is_some_and(|s| !s.trim().is_empty()))
        {
            return Ok(reply);
        }
    }
    bail!("计划已结束")
}

struct Wire {
    input: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    next: u64,
    buffered: VecDeque<Value>,
}

impl Wire {
    async fn send(&mut self, packet: Value) -> Result<()> {
        let mut data = serde_json::to_vec(&packet)?;
        data.push(b'\n');
        self.input.write_all(&data).await?;
        self.input.flush().await?;
        Ok(())
    }
    async fn read(&mut self) -> Result<Value> {
        let line = self
            .lines
            .next_line()
            .await?
            .context("Codex 计划进程已退出")?;
        Ok(serde_json::from_str(&line).context("Codex 返回了无效的计划协议数据")?)
    }
    async fn next_packet(&mut self) -> Result<Value> {
        match self.buffered.pop_front() {
            Some(v) => Ok(v),
            None => self.read().await,
        }
    }
    async fn deny(&mut self, id: &Value) -> Result<()> {
        self.send(json!({"id":id,"error":{"code":-32601,"message":"Unavailable in the read-only planner"}})).await
    }
    async fn call(&mut self, method: &str, params: Value, keep_events: bool) -> Result<Value> {
        self.next += 1;
        let id = self.next;
        self.send(json!({"id":id,"method":method,"params":params}))
            .await?;
        loop {
            let packet = self.read().await?;
            if packet.get("method").is_none() && packet["id"] == id {
                if let Some(error) = packet.get("error") {
                    bail!(
                        "{method}: {}",
                        error["message"].as_str().unwrap_or("Codex 拒绝了请求")
                    );
                }
                return packet
                    .get("result")
                    .cloned()
                    .context("Codex 未返回请求结果");
            }
            if keep_events {
                ensure!(self.buffered.len() < 1024, "启动回合前收到过多计划事件");
                self.buffered.push_back(packet);
            } else if let Some(id) = packet.get("id") {
                self.deny(id).await?;
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn plan_progress_only_displays_an_active_public_step_or_started_tool() {
        let plan = json!({"plan":[{"step":"finished","status":"completed"},
            {"step":"Inspect the tests","status":"inProgress"},{"step":"later","status":"pending"}],
            "explanation":"other context"});
        assert_eq!(
            progress_label("turn/plan/updated", &plan).as_deref(),
            Some("Inspect the tests")
        );
        assert!(
            progress_label(
                "turn/plan/updated",
                &json!({"plan":[{"step":"later","status":"pending"}]})
            )
            .is_none()
        );
        let shell = json!({"item":{"type":"commandExecution","command":"cat /project/README.md","commandActions":[{"type":"read","command":"cat /project/README.md","path":"/project/README.md"}],"aggregatedOutput":"contents"}});
        assert_eq!(
            progress_label("item/started", &shell).as_deref(),
            Some("cat · README.md")
        );
        for kind in ["plan", "reasoning", "agentMessage", "functionCallOutput"] {
            assert!(
                progress_label(
                    "item/started",
                    &json!({"item":{"type":kind,"text":"hidden","summary":"hidden"}})
                )
                .is_none()
            );
        }
        assert!(progress_label("item/completed", &shell).is_none());
    }

    #[tokio::test]
    async fn private_planner_answers_native_questions_and_requires_a_completed_plan_item() {
        let root = tempfile::tempdir().unwrap();
        let peer = root.path().join("codex");
        // Exercise the actual bidirectional stdio transport, including a notification
        // that beats the turn/start reply and a final message that asks for clarification.
        std::fs::write(&peer, r#"#!/bin/sh
set -eu
take() { IFS= read -r packet; case "$packet" in *"$1"*) ;; *) exit 3;; esac; }
reply() { printf '%s\n' "$1"; }
take initialize
reply '{"id":1,"result":{}}'
take initialized
take config/read
reply '{"id":2,"result":{"config":{"mcp_servers":{"external":{}}}}}'
take thread/fork
case "$packet" in *'"threadId":"real"'*) ;; *) exit 8;; esac
case "$packet" in *'"ephemeral":true'*) ;; *) exit 9;; esac
case "$packet" in *'"external":{"enabled":false}'*) ;; *) exit 4;; esac
reply '{"id":3,"result":{"thread":{"id":"private","ephemeral":true},"model":"native-default","sandbox":{"type":"readOnly","networkAccess":false},"approvalPolicy":"never"}}'
take mcpServerStatus/list
reply '{"id":4,"result":{"data":[{"runtimeStatus":"disabled"}]}}'
take collaborationMode/list
reply '{"id":5,"result":{"data":[{"mode":"plan"}]}}'
take turn/start
case "$packet" in *'"collaborationMode":{"mode":"plan"'*) ;; *) exit 5;; esac
reply '{"method":"item/completed","params":{"threadId":"private","turnId":"one","item":{"type":"plan","text":"obsolete foreign plan"}}}'
reply '{"id":6,"result":{"turn":{"id":"two"}}}'
reply '{"method":"turn/plan/updated","params":{"threadId":"another","turnId":"two","plan":[{"step":"not ours","status":"inProgress"}]}}'
reply '{"method":"turn/plan/updated","params":{"threadId":"private","turnId":"two","plan":[{"step":"Check the index","status":"inProgress"}]}}'
reply '{"method":"turn/plan/updated","params":{"threadId":"private","turnId":"two","plan":[{"step":"Check the index","status":"inProgress"}]}}'
reply '{"method":"item/completed","params":{"threadId":"private","turnId":"two","item":{"type":"reasoning","text":"private"}}}'
reply '{"method":"item/tool/requestUserInput","id":45,"params":{"threadId":"private","turnId":"two","questions":[{"id":"scope","question":"Scope?","options":[{"label":"Local","description":"Only this project"}]}]}}'
take '"scope":{"answers":["Local"]}'
reply '{"method":"item/completed","params":{"threadId":"private","turnId":"two","item":{"type":"agentMessage","phase":"final_answer","text":"Which file?"}}}'
reply '{"method":"turn/completed","params":{"threadId":"private","turn":{"id":"two","status":"completed"}}}'
take turn/start
case "$packet" in *'README'*) ;; *) exit 6;; esac
reply '{"id":7,"result":{"turn":{"id":"three"}}}'
reply '{"method":"turn/plan/updated","params":{"threadId":"private","turnId":"three","plan":[{"step":"internal progress"}]}}'
reply '{"method":"item/plan/delta","params":{"threadId":"foreign","turnId":"three","itemId":"proposal","delta":"Should never appear"}}'
reply '{"method":"item/plan/delta","params":{"threadId":"private","turnId":"two","itemId":"proposal","delta":"Obsolete turn"}}'
reply '{"method":"item/plan/delta","params":{"threadId":"private","turnId":"three","itemId":"proposal","delta":"Review README"}}'
reply '{"method":"item/completed","params":{"threadId":"private","turnId":"three","item":{"type":"plan","text":"Review README headings"}}}'
reply '{"method":"turn/completed","params":{"threadId":"private","turn":{"id":"three","status":"completed"}}}'
IFS= read -r packet || true
"#).unwrap();
        std::fs::set_permissions(&peer, std::fs::Permissions::from_mode(0o700)).unwrap();
        let launch = Launch {
            program: peer,
            cwd: root.path().into(),
            original: Some("real".into()),
            model: None,
            effort: None,
            unset_env: Default::default(),
            prompt: "Plan updates to README".into(),
        };
        let (events, rx) = async_channel::unbounded();
        let (answers, answer_rx) = mpsc::channel(1);
        let (_cancel, cancel_rx) = oneshot::channel();
        let chat = Uuid::new_v4();
        let request = Uuid::new_v4();
        let task = tokio::spawn(async move {
            launch
                .run(chat, request, &events, answer_rx, cancel_rx)
                .await
        });
        timeout(Duration::from_secs(8), async {
            let KarBridgeEvent::PlanProgress {
                chat_id,
                request_id,
                label,
            } = rx.recv().await.unwrap()
            else {
                panic!("expected public progress")
            };
            assert_eq!(
                (chat_id, request_id, label.as_str()),
                (chat, request, "Check the index")
            );
            for (prompt, response) in [("Scope?", "Local"), ("Which file?", "README")] {
                let KarBridgeEvent::PlanQuestions {
                    chat_id,
                    request_id,
                    questions,
                } = rx.recv().await.unwrap()
                else {
                    panic!("expected questions")
                };
                assert_eq!((chat_id, request_id), (chat, request));
                assert_eq!(questions.len(), 1);
                assert_eq!(questions[0].prompt, prompt);
                if prompt == "Scope?" {
                    assert_eq!(
                        questions[0].options[0].description.as_deref(),
                        Some("Only this project")
                    );
                }
                answers
                    .send(BTreeMap::from([(questions[0].id.clone(), response.into())]))
                    .await
                    .unwrap();
            }
            let KarBridgeEvent::PlanDelta {
                chat_id,
                request_id,
                item,
                delta,
            } = rx.recv().await.unwrap()
            else {
                panic!("expected this turn's streamed proposal")
            };
            assert_eq!(
                (chat_id, request_id, item.as_str(), delta.as_str()),
                (chat, request, "proposal", "Review README")
            );
            assert_eq!(task.await.unwrap().unwrap(), "Review README headings");
        })
        .await
        .unwrap();
    }
}
