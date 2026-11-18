//! ACP stdio transport and Tauri commands for DeepSeek Harness.

use crate::embedded_script::{cached_module, write_module};
use crate::prompt_files::{PromptFiles, StagedFile};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, oneshot};

const EVENT_NAME: &str = "acp-event";
const SESSION_BRIDGE: &str = include_str!("../../services/acp-session-bridge.mjs");
const SESSION_READ_STATUS: &str = include_str!("../../services/session-read-status.mjs");
const ASK_USER_TOOL: &str = include_str!("../../services/vendor/dsh-tool-ask-user/index.mjs");
const AGENT_TERMINAL_BRIDGE: &str = include_str!("../../services/agent-terminal-bridge.mjs");
const TERMINAL_INSTALLER: &[u8] = include_bytes!("../../services/install-terminal-controller.mjs");
const SCHEDULE_RUNTIME: &[u8] = include_bytes!("../../services/vendor/dsh-schedule/index.mjs");
const CLIENT_PREFERENCES: &str = include_str!("../../services/client-preferences.mjs");
const CLIENT_THEME: &str = include_str!("../../services/vendor/dsh-ui-preferences/theme.mjs");
const CLIENT_PREFERENCE_LICENSE: &str =
    include_str!("../../services/vendor/dsh-ui-preferences/LICENSE");
const CLIENT_DEVELOPER: &str =
    include_str!("../../services/vendor/dsh-ui-preferences/developer.mjs");
const AGENT_PRESET_PROFILE: &str = include_str!("../../services/agent-preset-profile.mjs");
const AGENT_PRESET_PACKAGES: &str = include_str!("../../services/agent-preset-packages.mjs");

/// Owns the single ACP connection shared by all windows and sessions.
#[derive(Default)]
pub struct AcpService {
    connection: Mutex<Option<Arc<AcpConnection>>>,
}

pub(crate) struct ClientPreferenceWriter(Arc<AcpConnection>);

pub(crate) async fn client_preference_writer(
    service: &AcpService,
) -> Result<ClientPreferenceWriter, String> {
    Ok(ClientPreferenceWriter(connection(service).await?))
}

impl ClientPreferenceWriter {
    pub(crate) async fn set_developer(&self, enabled: bool) -> Result<(), String> {
        let snapshot = self
            .0
            .request("_dsh/gpui/client-preferences", json!({"operation":"read"}))
            .await?;
        let row = snapshot["namespaces"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["ns"] == "ui-settings"))
            .ok_or("代码工作视图配置尚未就绪。")?;
        if row["value"]["enabled"] == enabled {
            return Ok(());
        }
        self.0
            .request(
                "_dsh/gpui/client-preferences",
                json!({"operation":"set", "namespace":"ui-settings",
            "revision":row["revision"], "changes":{"enabled":enabled}}),
            )
            .await?;
        Ok(())
    }
}

struct AcpConnection {
    next_id: AtomicU64,
    writer: Mutex<ChildStdin>,
    child: Mutex<Child>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>,
    approvals: Mutex<HashMap<String, PendingApproval>>,
    stopping: AtomicBool,
    account_models: AtomicBool,
    session_bridge: bool,
    prompt_files: Option<PromptFiles>,
    // Kept alive for as long as the child may still be reading the module.
    _session_bridge_script: Option<tempfile::NamedTempFile>,
    _agent_terminal_script: Option<tempfile::NamedTempFile>,
}

struct PendingApproval {
    wire_id: Value,
    session: String,
    tool_call: Option<String>,
    options: Vec<String>,
}

// ACP request IDs can be reused by a new connection. UI handles remain unique.
static NEXT_APPROVAL: AtomicU64 = AtomicU64::new(1);
static NEXT_TOOL_RESULT_WATCH: AtomicU64 = AtomicU64::new(1);

impl AcpConnection {
    async fn spawn(
        service_path: &Path,
        workspace: &Path,
        app: AppHandle,
    ) -> Result<Arc<Self>, String> {
        let built = service_path.join("apps/cli/lib/bin.js");
        let source = service_path.join("apps/cli/src/bin.ts");
        let node = resolve_node();
        let mut command = Command::new(&node);
        command.current_dir(service_path);
        let package = service_path.join("node_modules/@deepseek-ai/dsh/lib");
        let (entry, profile) = if built.is_file() {
            (built, service_path.join("apps/cli/lib/profile-boot.js"))
        } else if package.join("bin.js").is_file() {
            (package.join("bin.js"), package.join("profile-boot.js"))
        } else if source.is_file() {
            command.args(["--import", "tsx/esm"]);
            (source, service_path.join("apps/cli/src/profile-boot.ts"))
        } else {
            return Err(format!(
                "未找到 dsh-acp 启动入口：{}。请先运行 pnpm install 和 pnpm run build。",
                service_path.display()
            ));
        };
        let session_bridge = profile.is_file();
        let prompt_files = session_bridge.then(PromptFiles::new).transpose()?;
        let mut bridge_script = None;
        let mut agent_terminal_script = None;
        if let Some(staging) = &prompt_files {
            let terminal = prepare_terminal_module(&node, service_path).await;
            let ask_tool =
                serde_json::to_string(ASK_USER_TOOL).map_err(|error| error.to_string())?;
            let terminal_script =
                write_module("agent-terminal-bridge", AGENT_TERMINAL_BRIDGE.as_bytes())?;
            let terminal_import = module_import_specifier(terminal_script.path())?;
            let bridge = format!(
                "{SESSION_READ_STATUS}\nconst bundledAskUser = {ask_tool};\n{SESSION_BRIDGE}"
            )
            .replace("'./agent-terminal-bridge.mjs'", &terminal_import);
            let script = write_module("acp-session-bridge", bridge.as_bytes())?;
            // With `--eval` the label "dsh-gpui-acp" occupied argv[1] and the
            // bridge read service/profile from process.argv.slice(2). A real
            // .mjs file takes that slot instead, so the same slice still yields
            // [service, profile, ...patchFiles] without any bridge changes.
            command
                .arg(script.path())
                .arg(service_path)
                .arg(profile)
                .env("KWORK_PROMPT_FILES", staging.path())
                .env("KWORK_WORKSPACE", workspace)
                .env(
                    "KWORK_WORKSPACE_PREFERENCES",
                    crate::preferences::preferences_path(&app)?,
                );
            if cfg!(target_os = "macos") {
                let modules = json!({"theme": CLIENT_THEME, "developer": CLIENT_DEVELOPER, "license": CLIENT_PREFERENCE_LICENSE});
                let source =
                    format!("const bundledPreferenceModules = {modules};\n{CLIENT_PREFERENCES}");
                command.env(
                    "KWORK_CLIENT_PREFERENCES_MODULE",
                    cached_module("client-preferences", source.as_bytes())?,
                );
            }
            if cfg!(any(target_os = "macos", windows)) {
                let packages =
                    cached_module("agent-preset-packages", AGENT_PRESET_PACKAGES.as_bytes())?;
                let assets = json!({
                    "lock": serde_json::from_str::<Value>(include_str!("../../services/vendor/dsh-agent-presets/packages.json"))
                        .map_err(|error| error.to_string())?,
                    "acp": include_str!("../../services/vendor/dsh-agent-presets/acp.mjs"),
                    "license": include_str!("../../services/vendor/dsh-agent-presets/LICENSE"),
                    "presets": {
                        "standard": include_str!("../../services/vendor/dsh-agent-presets/standard.patch.yml"),
                        "ptc": include_str!("../../services/vendor/dsh-agent-presets/ptc.patch.yml"),
                        "minimal": include_str!("../../services/vendor/dsh-agent-presets/minimal.patch.yml"),
                        "cordis": include_str!("../../services/vendor/dsh-agent-presets/cordis.patch.yml")
                    }
                });
                let source =
                    format!("const bundledPresetAssets = {assets};\n{AGENT_PRESET_PROFILE}")
                        .replace(
                            "'./agent-preset-packages.mjs'",
                            &module_import_specifier(&packages)?,
                        );
                command.env(
                    "KWORK_AGENT_PRESETS_MODULE",
                    cached_module("agent-preset-profile", source.as_bytes())?,
                );
            }
            match terminal {
                Ok((entry, runtime)) => {
                    command.env("KWORK_TERMINAL_MODULE", entry);
                    command.env("KWORK_TERMINAL_RUNTIME", runtime);
                }
                Err(error) => {
                    command.env_remove("KWORK_TERMINAL_MODULE");
                    command.env_remove("KWORK_TERMINAL_RUNTIME");
                    eprintln!("KWork Agent terminal: {error}");
                }
            }
            // Supply the desktop account's Documents path to the Host. The
            // registry decides whether first-use directory creation is eligible.
            if std::env::var_os("KWORK_DOCUMENTS_DIRECTORY").is_none()
                && let Ok(documents) = app.path().document_dir()
                && documents.is_absolute()
                && !(cfg!(target_os = "linux")
                    && app.path().home_dir().is_ok_and(|home| home == documents))
            {
                command.env("KWORK_DOCUMENTS_DIRECTORY", documents);
            }
            // The 574 KB scheduler bundle is immutable: content-address it once
            // instead of rewriting a private copy for every ACP connection.
            match cached_module("dsh-schedule", SCHEDULE_RUNTIME) {
                Ok(path) => {
                    command.env("KWORK_SCHEDULE_MODULE", path);
                }
                Err(error) => {
                    // The scheduler is optional; the ACP conversation still works.
                    command.env_remove("KWORK_SCHEDULE_MODULE");
                    eprintln!("dsh-gpui schedule runtime: {error}");
                }
            }
            bridge_script = Some(script);
            agent_terminal_script = Some(terminal_script);
        } else {
            command.arg(entry).args(["--profile", "acp"]);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            format!(
                "无法启动 dsh-acp：{error}。请确认 Node.js 24.9+ 已安装，也可通过 DSH_NODE 指定路径。"
            )
        })?;
        let stdin = child.stdin.take().ok_or("无法打开 dsh-acp 标准输入")?;
        let stdout = child.stdout.take().ok_or("无法打开 dsh-acp 标准输出")?;
        let stderr = child.stderr.take().ok_or("无法打开 dsh-acp 标准错误")?;
        let connection = Arc::new(Self {
            next_id: AtomicU64::new(1),
            writer: Mutex::new(stdin),
            child: Mutex::new(child),
            pending: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
            stopping: AtomicBool::new(false),
            account_models: AtomicBool::new(false),
            session_bridge,
            prompt_files,
            _session_bridge_script: bridge_script,
            _agent_terminal_script: agent_terminal_script,
        });

        tokio::spawn(read_stdout(connection.clone(), stdout, app.clone()));
        tokio::spawn(read_stderr(stderr));
        emit(&app, "service", json!({"status": "running"}));
        Ok(connection)
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(error) = self.write(message).await {
            self.pending.lock().await.remove(&id);
            return Err(error);
        }
        receiver
            .await
            .map_err(|_| "dsh-acp 在请求完成前断开了连接".to_string())?
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        self.write(json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await
    }

    async fn respond(&self, id: Value, result: Value) -> Result<(), String> {
        self.write(json!({"jsonrpc": "2.0", "id": id, "result": result}))
            .await
    }

    async fn receive_approval(&self, app: &AppHandle, wire_id: Value, mut params: Value) {
        if !(wire_id.is_string() || wire_id.is_number()) {
            return;
        }
        let mut approvals = self.approvals.lock().await;
        if approvals.values().any(|pending| pending.wire_id == wire_id) {
            return;
        }
        let session = params["sessionId"].as_str().unwrap_or_default();
        let options: Vec<_> = params["options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|option| option["optionId"].as_str())
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .collect();
        if session.is_empty() || options.is_empty() {
            self.cancel_approval(wire_id).await;
            return;
        }
        let request_id = format!("approval-{}", NEXT_APPROVAL.fetch_add(1, Ordering::Relaxed));
        approvals.insert(
            request_id.clone(),
            PendingApproval {
                wire_id,
                session: session.to_owned(),
                tool_call: params["toolCall"]["toolCallId"].as_str().map(str::to_owned),
                options,
            },
        );
        params["requestId"] = json!(request_id);
        emit(app, "permission", params);
    }

    async fn cancel_approval(&self, wire_id: Value) {
        if let Err(error) = self
            .respond(wire_id, json!({"outcome": {"outcome": "cancelled"}}))
            .await
        {
            eprintln!("dsh-acp permission cancellation: {error}");
        }
    }

    async fn cancel_turn(&self, app: &AppHandle, session: &str) -> Result<(), String> {
        let mut approvals = self.approvals.lock().await;
        self.notify("session/cancel", json!({"sessionId": session}))
            .await?;
        self.withdraw_approvals(&mut approvals, app, session, None)
            .await;
        Ok(())
    }

    async fn withdraw_approvals(
        &self,
        approvals: &mut HashMap<String, PendingApproval>,
        app: &AppHandle,
        session: &str,
        tool_call: Option<&str>,
    ) {
        for (request_id, pending) in approvals.extract_if(|_, pending| {
            pending.session == session
                && tool_call.is_none_or(|id| pending.tool_call.as_deref() == Some(id))
        }) {
            self.cancel_approval(pending.wire_id).await;
            emit(
                app,
                "permission_closed",
                json!({"sessionId": session, "requestId": request_id}),
            );
        }
    }

    async fn write(&self, message: Value) -> Result<(), String> {
        let mut encoded = serde_json::to_vec(&message).map_err(|error| error.to_string())?;
        encoded.push(b'\n');
        let mut writer = self.writer.lock().await;
        writer
            .write_all(&encoded)
            .await
            .map_err(|error| format!("写入 dsh-acp 失败：{error}"))?;
        writer
            .flush()
            .await
            .map_err(|error| format!("刷新 dsh-acp 输入失败：{error}"))
    }

    async fn fail_pending(&self, reason: &str) {
        let pending = std::mem::take(&mut *self.pending.lock().await);
        for (_, sender) in pending {
            let _ = sender.send(Err(reason.to_string()));
        }
    }

    async fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
        let mut child = self.child.lock().await;
        if let Err(error) = child.kill().await
            && child.id().is_some()
        {
            eprintln!("dsh-acp shutdown: {error}");
        }
    }

    fn stop_now(&self) {
        self.stopping.store(true, Ordering::Release);
        if let Ok(mut child) = self.child.try_lock() {
            let _ = child.start_kill();
        }
    }
}

async fn read_stdout(
    connection: Arc<AcpConnection>,
    stdout: impl tokio::io::AsyncRead + Unpin,
    app: AppHandle,
) {
    let mut lines = BufReader::new(stdout).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) if line.trim().is_empty() => {}
            Ok(Some(line)) => match serde_json::from_str::<Value>(&line) {
                Ok(message) => route_message(&connection, &app, message).await,
                Err(error) => eprintln!("dsh-acp invalid JSON-RPC message: {error}"),
            },
            Ok(None) => break,
            Err(error) => {
                eprintln!("dsh-acp stdout: {error}");
                break;
            }
        }
    }
    connection.approvals.lock().await.clear();
    connection.fail_pending("dsh-acp 已断开连接").await;
    if !connection.stopping.load(Ordering::Acquire) {
        crate::account_platform::invalidate_grant(&app);
        emit(
            &app,
            "service",
            json!({"status": "exited", "detail": "dsh-acp 已断开连接"}),
        );
    }
}

fn service_digest(service: &Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(service.to_string_lossy().as_bytes()))[..16].to_owned()
}

fn module_import_specifier(path: &Path) -> Result<String, String> {
    let url = url::Url::from_file_path(path)
        .map_err(|()| format!("无法生成模块 URL：{}", path.display()))?;
    serde_json::to_string(url.as_str()).map_err(|error| error.to_string())
}

async fn prepare_terminal_module(
    node: &std::ffi::OsStr,
    service: &Path,
) -> Result<(PathBuf, PathBuf), String> {
    let home = std::env::var_os("DSH_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".dsh")))
        .ok_or("无法确定 Harness 主目录，请设置 DSH_HOME")?;
    let target = home.join("profiles/acp/.kwork-runtime").join(format!(
        "terminal-controller-{}-0.2.2-rc.3",
        service_digest(service)
    ));
    let adapter = cached_module("agent-terminal-bridge", AGENT_TERMINAL_BRIDGE.as_bytes())?;
    let contents = String::from_utf8_lossy(TERMINAL_INSTALLER).replace(
        "'./agent-terminal-bridge.mjs'",
        &module_import_specifier(&adapter)?,
    ) + "\nconsole.log(await main());\n";
    let installer = write_module("install-terminal-controller-resolved", contents.as_bytes())?;
    let output = Command::new(node)
        .arg(installer.path())
        .arg(&target)
        .arg(service)
        .arg(home)
        .current_dir(service)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|error| format!("无法准备 Agent 终端模块：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Agent 终端模块准备失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let entry = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    let entry = PathBuf::from(entry.trim());
    if !entry.is_file() {
        return Err("Agent 终端模块入口不存在。".into());
    }
    // <target>/v3/package/lib/index.js -> <target>/v3, which owns node_modules.
    let runtime = entry
        .ancestors()
        .nth(3)
        .ok_or("Agent 终端运行时路径无效")?
        .to_path_buf();
    Ok((entry, runtime))
}
async fn route_message(connection: &Arc<AcpConnection>, app: &AppHandle, message: Value) {
    let method = message.get("method").and_then(Value::as_str);
    let id = message.get("id").cloned();
    match (method, id) {
        (Some("session/request_permission"), Some(id)) => {
            let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
            connection.receive_approval(app, id, params).await;
        }
        (Some(_), Some(id)) => {
            let response = json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "client method is not supported"}
            });
            if let Err(error) = connection.write(response).await {
                eprintln!("dsh-acp method rejection: {error}");
            }
        }
        (Some("session/update"), None) => {
            let mut params = message.get("params").cloned().unwrap_or_else(|| json!({}));
            let update = &params["update"];
            if update["sessionUpdate"] == "tool_call_update"
                && matches!(update["status"].as_str(), Some("completed" | "failed"))
                && let Some(session) = params["sessionId"].as_str()
                && let Some(tool_call) = update["toolCallId"].as_str()
            {
                let mut approvals = connection.approvals.lock().await;
                connection
                    .withdraw_approvals(&mut approvals, app, session, Some(tool_call))
                    .await;
            }
            if !connection.account_models.load(Ordering::Acquire)
                && let Some(options) = params.pointer_mut("/update/configOptions")
            {
                filter_account_models(options);
            }
            emit(
                app,
                "session_update",
                json!({
                    "sessionId": params.get("sessionId").cloned().unwrap_or(Value::Null),
                    "update": params.get("update").cloned().unwrap_or(Value::Null),
                }),
            );
        }
        (Some("_dsh/gpui/plugin-install-event"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            if let Some(mut params) = message.get("params").and_then(Value::as_object).cloned() {
                params.insert("kind".into(), Value::String("plugin_install".into()));
                if let Err(error) = app.emit_to("main", EVENT_NAME, params) {
                    eprintln!("dsh-gpui plugin install event emission: {error}");
                }
            }
        }
        (Some("_dsh/gpui/session-read-changed"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            if let Some(state) = message.get("params").filter(|value| value.is_object()) {
                emit(app, "session_read", state.clone());
            }
        }
        (Some("_dsh/gpui/workspace-registry-changed"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            if let Some(snapshot) = message.get("params").filter(|value| value.is_object()) {
                emit(app, "workspace_registry", snapshot.clone());
            }
        }
        (Some("_dsh/gpui/composer-preference-changed"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            emit(app, "composer_preference_changed", json!({}));
        }
        (Some("_dsh/gpui/client-preferences-changed"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            emit(app, "client_preferences_changed", json!({}));
        }
        (Some("_dsh/gpui/agent-presets-changed"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            emit(app, "agent_presets_changed", json!({}));
        }
        (Some("_dsh/gpui/model-catalog-changed"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            if let Err(error) =
                app.emit_to("main", EVENT_NAME, json!({"kind": "model_catalog_changed"}))
            {
                eprintln!("dsh-gpui model catalog event emission: {error}");
            }
        }
        (Some("_dsh/gpui/permission-default-changed"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            emit(app, "permission_default_changed", json!({}));
        }
        (Some("_dsh/gpui/session-log-preference-changed"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            emit(app, "session_log_preference_changed", json!({}));
        }
        (Some("_dsh/gpui/deepseek-account-changed"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            let params = &message["params"];
            connection
                .account_models
                .store(params["ready"] == true, Ordering::Release);
            emit(
                app,
                "deepseek_account_changed",
                json!({"reason": params["reason"]}),
            );
        }
        (Some("_dsh/gpui/account-platform-session-revoked"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            crate::account_platform::invalidate_grant(app);
        }
        (Some("_dsh/gpui/terminal-changed"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            if let Some(params) = message.get("params").filter(|value| value.is_object())
                && let Err(error) = app.emit_to(
                    "main",
                    EVENT_NAME,
                    json!({
                        "kind": "agent_terminals", "sessionId": params["sessionId"],
                        "terminals": params["terminals"],
                    }),
                )
            {
                eprintln!("dsh-gpui terminal roster event emission: {error}");
            }
        }
        (Some("_dsh/gpui/tool-result"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            if let Some(params) = message.get("params").filter(|value| value.is_object()) {
                let mut event = params.clone();
                event["kind"] = json!("tool_result");
                if let Err(error) = app.emit_to("main", EVENT_NAME, event) {
                    eprintln!("dsh-gpui tool result event emission: {error}");
                }
            }
        }
        (Some("_dsh/gpui/terminal-frame"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            if let Some(params) = message.get("params").filter(|value| value.is_object())
                && let Err(error) = app.emit_to("main", EVENT_NAME, params)
            {
                eprintln!("dsh-gpui terminal frame event emission: {error}");
            }
        }
        (Some("_dsh/gpui/schedule-changed"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            if let Err(error) = app.emit_to("main", EVENT_NAME, json!({"kind": "schedule_changed"}))
            {
                eprintln!("dsh-gpui schedule event emission: {error}");
            }
        }
        (Some("_dsh/gpui/schedule-session"), None)
            if connection.session_bridge && !connection.stopping.load(Ordering::Acquire) =>
        {
            if let Some(session) = message.get("params").filter(|value| value.is_object())
                && let Err(error) = app.emit_to(
                    "main",
                    EVENT_NAME,
                    json!({"kind": "schedule_session", "session": session}),
                )
            {
                eprintln!("dsh-gpui schedule session event emission: {error}");
            }
        }
        (Some(_), None) => {}
        (None, Some(id)) => {
            let Some(id) = id.as_u64() else {
                eprintln!("dsh-acp response has a non-numeric id");
                return;
            };
            let sender = connection.pending.lock().await.remove(&id);
            if let Some(sender) = sender {
                let _ = sender.send(decode_response(&message));
            }
        }
        (None, None) => eprintln!("dsh-acp message has neither method nor id"),
    }
}

async fn read_stderr(stderr: impl tokio::io::AsyncRead + Unpin) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        eprintln!("dsh-acp: {line}");
    }
}

fn decode_response(message: &Value) -> Result<Value, String> {
    if let Some(error) = message.get("error") {
        let code = error.get("code").and_then(Value::as_i64).unwrap_or(-1);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown JSON-RPC error");
        let detail = error.get("data").map(|data| {
            if let Some(text) = data.as_str() {
                format!("：{text}")
            } else {
                format!("：{data}")
            }
        });
        return Err(format!(
            "ACP {code}: {message}{}",
            detail.unwrap_or_default()
        ));
    }
    Ok(message.get("result").cloned().unwrap_or_else(|| json!({})))
}

fn emit(app: &AppHandle, kind: &str, payload: Value) {
    let mut event = json!({"kind": kind});
    if let (Some(target), Some(source)) = (event.as_object_mut(), payload.as_object()) {
        target.extend(source.clone());
    }
    if let Err(error) = app.emit(EVENT_NAME, event) {
        eprintln!("dsh-gpui event emission: {error}");
    }
}

/// Resolved paths and the ACP initialize result returned to the UI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartServiceReply {
    pub agent: Value,
    pub workspace: String,
    pub service_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_providers: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deepseek_account: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_archive: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_registry: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_reading: Option<Value>,
}

#[tauri::command]
pub async fn start_service(
    service_path: Option<String>,
    workspace: Option<String>,
    app: AppHandle,
    service: State<'_, AcpService>,
) -> Result<StartServiceReply, String> {
    let service_path = resolve_service_path(service_path.as_deref())?;
    let workspace = resolve_workspace(workspace.as_deref())?;
    let mut slot = service.connection.lock().await;
    crate::account_platform::invalidate_grant(&app);
    if let Some(previous) = slot.take() {
        previous.stop().await;
    }
    emit(&app, "service", json!({"status": "starting"}));
    let connection = AcpConnection::spawn(&service_path, &workspace, app.clone()).await?;
    let agent = connection
        .request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": {"name": "dsh-gpui", "version": env!("CARGO_PKG_VERSION")}
            }),
        )
        .await?;
    let model_providers = if connection.session_bridge {
        connection
            .request("_dsh/gpui/session/questions", json!({}))
            .await?;
        connection
            .request(
                "_dsh/gpui/providers/list",
                json!({"includeRetrySettings":true}),
            )
            .await
            .ok()
    } else {
        None
    };
    let deepseek_account = if connection.session_bridge {
        connection
            .request("_dsh/gpui/deepseek-account", json!({"action":"status"}))
            .await
            .ok()
    } else {
        None
    };
    connection.account_models.store(
        deepseek_account
            .as_ref()
            .is_some_and(|reply| reply["modelReady"] == true),
        Ordering::Release,
    );
    let session_reading = if connection.session_bridge {
        Some(
            connection
                .request("_dsh/gpui/session/read-state", json!({"action":"snapshot"}))
                .await?,
        )
    } else {
        None
    };
    let workspace_registry = if connection.session_bridge {
        connection
            .request(
                "_dsh/gpui/workspace-registry",
                json!({"operation": {"action": "snapshot"}}),
            )
            .await
            .inspect_err(|error| eprintln!("KWork workspace registry: {error}"))
            .ok()
    } else {
        None
    };
    let workspace_archive = if connection.session_bridge
        && workspace_registry
            .as_ref()
            .is_none_or(|snapshot| !snapshot["archived"].is_array())
    {
        Some(
            connection
                .request("_dsh/gpui/session/archive", json!({"action":"read"}))
                .await?,
        )
    } else {
        None
    };
    emit(
        &app,
        "service",
        json!({"status": "running", "workspace": workspace.display().to_string()}),
    );
    *slot = Some(connection);
    Ok(StartServiceReply {
        agent,
        workspace: workspace.display().to_string(),
        service_path: service_path.display().to_string(),
        model_providers,
        deepseek_account,
        workspace_archive,
        workspace_registry,
        session_reading,
    })
}

#[tauri::command]
pub(crate) async fn workspace_registry(
    webview: tauri::Webview,
    operation: Value,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("仅主窗口可管理工作区。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("当前服务无法管理工作区。".into());
    }
    connection
        .request(
            "_dsh/gpui/workspace-registry",
            json!({"operation": operation}),
        )
        .await
}

#[tauri::command]
pub(crate) async fn session_read_state(
    webview: tauri::Webview,
    session_id: Option<String>,
    action: Option<Value>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("session-status/unavailable".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("当前服务不支持会话阅读状态。".into());
    }
    connection
        .request(
            "_dsh/gpui/session/read-state",
            json!({"sessionId": session_id, "action": action}),
        )
        .await
}

#[tauri::command]
pub async fn stop_service(app: AppHandle, service: State<'_, AcpService>) -> Result<(), String> {
    crate::account_platform::invalidate_grant(&app);
    if let Some(connection) = service.connection.lock().await.take() {
        connection.stop().await;
    }
    Ok(())
}

#[tauri::command]
pub async fn new_session(
    webview: tauri::Webview,
    cwd: String,
    workspace_id: Option<String>,
    agent_preset: Option<String>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    let connection = connection(&service).await?;
    if workspace_id.is_some() && (webview.label() != "main" || !connection.session_bridge) {
        return Err("当前窗口或服务无法创建工作区会话。".into());
    }
    let mut request = json!({"cwd": cwd, "mcpServers": []});
    if let Some(preset) = agent_preset {
        if webview.label() != "main"
            || !connection.session_bridge
            || preset.is_empty()
            || preset.len() > 512
        {
            return Err("当前窗口或服务无法指定 Agent 预设。".into());
        }
        let roster = connection
            .request(
                "_dsh/gpui/agent-preset-settings",
                json!({"operation":"list"}),
            )
            .await?;
        if !roster["presets"].as_array().is_some_and(|presets| {
            presets
                .iter()
                .any(|option| option["id"] == preset && option["broken"].is_null())
        }) {
            return Err("所选 Agent 预设已不可用。".into());
        }
        request["_meta"] = json!({"kwork.agentPreset": preset});
    }
    let mut reply = connection.request("session/new", request).await?;
    if let Some(id) = reply["sessionId"].as_str().map(str::to_owned)
        && let Some(workspace_id) = workspace_id
    {
        match connection
            .request(
                "_dsh/gpui/workspace-registry",
                json!({"operation": {"action": "attach-session", "id": workspace_id, "sessionId": id}}),
            )
            .await
        {
            Ok(snapshot) => reply["workspaceRegistry"] = snapshot,
            Err(error) => reply["workspaceAttachError"] = Value::String(error),
        }
    }
    prepare_session_models(&connection, &mut reply);
    prepare_session_preset(&connection, &mut reply).await;
    Ok(reply)
}

#[tauri::command]
pub async fn list_sessions(
    cwd: Option<String>,
    cursor: Option<String>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    connection(&service)
        .await?
        .request("session/list", json!({"cwd": cwd, "cursor": cursor}))
        .await
}

#[tauri::command]
pub async fn search_session_history(
    query: Option<String>,
    generation: u64,
    webview: tauri::WebviewWindow,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("Session search is only available in the main window.".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Ok(json!({"items": [], "hasMore": false}));
    }
    connection
        .request(
            "_dsh/gpui/session/search",
            json!({"query": query, "generation": generation}),
        )
        .await
}

#[tauri::command]
pub async fn resume_session(
    session_id: String,
    cwd: String,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    let connection = connection(&service).await?;
    resume_session_reply(&connection, &session_id, &cwd).await
}

async fn resume_session_reply(
    connection: &AcpConnection,
    session_id: &str,
    cwd: &str,
) -> Result<Value, String> {
    let params = json!({"sessionId": session_id, "cwd": cwd, "mcpServers": []});
    let mut reply = if connection.session_bridge {
        connection
            .request("_dsh/gpui/session/open", params.clone())
            .await?
    } else {
        Value::Null
    };
    if reply.is_null() {
        reply = connection.request("session/resume", params).await?;
    }
    prepare_session_models(connection, &mut reply);
    prepare_session_preset(connection, &mut reply).await;
    Ok(reply)
}

async fn prepare_session_preset(connection: &AcpConnection, reply: &mut Value) {
    if !cfg!(any(target_os = "macos", windows)) || !connection.session_bridge {
        return;
    }
    if let Some(session) = reply["sessionId"].as_str()
        && let Ok(state) = connection
            .request(
                "_dsh/gpui/agent-preset-settings",
                json!({"operation": "session", "sessionId": session}),
            )
            .await
        && let Some(preset) = state["agentPreset"].as_str()
    {
        reply["agentPreset"] = json!(preset);
    }
}

fn model_address(value: &str) -> Option<[String; 2]> {
    serde_json::from_str(value).ok()
}

fn filter_account_models(config_options: &mut Value) {
    if let Some(options) = config_options.as_array_mut()
        && let Some(model) = options.iter_mut().find(|option| option["id"] == "model")
        && let Some(choices) = model["options"].as_array_mut()
    {
        choices.retain(|item| {
            item["group"] != "deepseek-account"
                && item["value"]
                    .as_str()
                    .and_then(model_address)
                    .is_none_or(|[provider, _]| provider != "deepseek-account")
        });
    }
}

fn prepare_session_models(connection: &AcpConnection, reply: &mut Value) {
    if !connection.account_models.load(Ordering::Acquire)
        && let Some(options) = reply.get_mut("configOptions")
    {
        filter_account_models(options);
    }
}

#[tauri::command]
pub(crate) async fn rename_session(
    webview: tauri::Webview,
    session_id: String,
    title: String,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" || session_id.is_empty() {
        return Err("session/not-found".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("当前服务不支持重命名会话。".into());
    }
    connection
        .request(
            "_dsh/gpui/session/rename",
            json!({"sessionId": session_id, "title": title}),
        )
        .await
}

#[tauri::command]
pub(crate) async fn fork_session(
    webview: tauri::Webview,
    session_id: String,
    at_seq: Option<u64>,
    open: bool,
    allow_migration: bool,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" || session_id.is_empty() {
        return Err("session/not-found".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("session/fork-unavailable".into());
    }
    let mut params = json!({"sessionId": session_id, "allowMigration": allow_migration});
    if let Some(seq) = at_seq {
        params["atSeq"] = seq.into();
    }
    let mut child = connection.request("_dsh/gpui/session/fork", params).await?;
    if open {
        let id = child["sessionId"]
            .as_str()
            .ok_or("分支会话缺少 Session ID")?;
        let cwd = child["cwd"].as_str().ok_or("分支会话缺少工作目录")?;
        let resumed = resume_session_reply(&connection, id, cwd).await?;
        child["configOptions"] = resumed["configOptions"].clone();
    }
    Ok(child)
}

#[tauri::command]
pub async fn close_session(
    session_id: String,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    connection(&service)
        .await?
        .request("session/close", json!({"sessionId": session_id}))
        .await
}

#[tauri::command]
pub async fn prompt(
    session_id: String,
    text: Option<String>,
    prompt: Option<Value>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    let blocks = match prompt {
        Some(Value::Array(items)) if !items.is_empty() => Value::Array(items),
        _ => {
            let text = text.unwrap_or_default();
            json!([{"type": "text", "text": text}])
        }
    };
    connection(&service)
        .await?
        .request(
            "session/prompt",
            json!({"sessionId": session_id, "prompt": blocks}),
        )
        .await
}

#[tauri::command]
pub(crate) async fn composer_preference(
    webview: tauri::Webview,
    mode: Option<String>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("conversation/unavailable".into());
    }
    if mode
        .as_deref()
        .is_some_and(|mode| !matches!(mode, "queue" | "steer"))
    {
        return Err("不支持的发送行为。".into());
    }
    if let Ok(connection) = connection(&service).await
        && connection.session_bridge
    {
        let state = connection
            .request("_dsh/gpui/composer-preference", json!({"mode": mode}))
            .await?;
        if state["available"].as_bool() == Some(true) {
            return Ok(state);
        }
    }
    crate::preferences::desktop_composer_preference(webview.app_handle(), mode).await
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum PromptFileOperation {
    Begin {
        name: String,
        size: u64,
    },
    Append {
        id: String,
        offset: u64,
        chunk: Vec<u8>,
    },
    Discard {
        id: String,
    },
}

#[tauri::command]
pub(crate) async fn stage_prompt_file(
    webview: tauri::Webview,
    session_id: String,
    operation: PromptFileOperation,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("附件上传不可用。".into());
    }
    let connection = connection(&service).await?;
    tokio::task::spawn_blocking(move || {
        let files = connection
            .prompt_files
            .as_ref()
            .ok_or("当前服务不支持上传普通文件。")?;
        match operation {
            PromptFileOperation::Begin { name, size } => files
                .begin(session_id, name, size)
                .map(|id| json!({"id": id})),
            PromptFileOperation::Append { id, offset, chunk } => files
                .append(&session_id, &id, offset, chunk)
                .map(|()| Value::Null),
            PromptFileOperation::Discard { id } => {
                files.discard(&session_id, &id).map(|()| Value::Null)
            }
        }
    })
    .await
    .map_err(|error| format!("附件暂存失败：{error}"))?
}

fn assemble_prompt_files(
    connection: &AcpConnection,
    session_id: &str,
    content: &mut Value,
) -> Result<Vec<StagedFile>, String> {
    if !content
        .as_array()
        .is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "file"))
    {
        return Ok(Vec::new());
    }
    connection
        .prompt_files
        .as_ref()
        .ok_or("当前服务不支持上传普通文件。")?
        .assemble(session_id, content)
}

/// Admit a composed message as a durable Inbox item.
/// `mode` is `queue` (next turn) or `steer` (next step of the running turn).
#[tauri::command]
pub async fn queue_prompt(
    session_id: String,
    mut prompt: Value,
    mode: String,
    request_id: Option<String>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    let connection = connection(&service).await?;
    let _files = assemble_prompt_files(&connection, &session_id, &mut prompt)?;
    connection
        .request(
            "_dsh/gpui/session/queue-prompt",
            json!({"sessionId": session_id, "prompt": prompt, "mode": mode, "requestId": request_id}),
        )
        .await
}

#[tauri::command]
pub async fn inbox_state(
    session_id: String,
    request_seq: Option<u64>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    connection(&service)
        .await?
        .request(
            "_dsh/gpui/session/inbox-state",
            json!({"sessionId": session_id, "requestSeq": request_seq}),
        )
        .await
}

#[tauri::command]
pub async fn inbox_action(
    session_id: String,
    item_id: String,
    action: String,
    text: Option<String>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    connection(&service)
        .await?
        .request(
            "_dsh/gpui/session/inbox-action",
            json!({"sessionId": session_id, "itemId": item_id, "action": action, "text": text}),
        )
        .await
}

/// Read the session's current goal (durable phase plus process-local activation).
#[tauri::command]
pub async fn goal_state(
    session_id: String,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    connection(&service)
        .await?
        .request(
            "_dsh/gpui/session/goal-state",
            json!({"sessionId": session_id}),
        )
        .await
}

/// Read or save root-session Team mode through the Host's agentTeams service.
#[tauri::command]
pub async fn team_state(
    webview: tauri::Webview,
    session_id: String,
    enabled: Option<bool>,
    request: Option<u64>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" || session_id.is_empty() {
        return Err("team/unavailable".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Ok(json!({"available": false}));
    }
    connection
        .request(
            "_dsh/gpui/session/team-state",
            json!({"sessionId": session_id, "enabled": enabled, "request": request}),
        )
        .await
}

/// Pause, resume, clear, or edit the current goal through the goal service.
#[tauri::command]
pub async fn goal_action(
    session_id: String,
    action: String,
    goal_id: String,
    revision: u64,
    objective: Option<String>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    connection(&service)
        .await?
        .request(
            "_dsh/gpui/session/goal-action",
            json!({
                "sessionId": session_id,
                "action": action,
                "goalId": goal_id,
                "revision": revision,
                "objective": objective,
            }),
        )
        .await
}

#[tauri::command]
pub async fn cancel_session(
    session_id: String,
    app: AppHandle,
    service: State<'_, AcpService>,
) -> Result<(), String> {
    connection(&service)
        .await?
        .cancel_turn(&app, &session_id)
        .await
}

#[tauri::command]
pub async fn set_config_option(
    session_id: String,
    config_id: String,
    value: String,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    let connection = connection(&service).await?;
    if config_id == "model"
        && !connection.account_models.load(Ordering::Acquire)
        && model_address(&value).is_some_and(|[provider, _]| provider == "deepseek-account")
    {
        return Err("请先登录 DeepSeek 账号。".into());
    }
    let mut reply = connection
        .request(
            "session/set_config_option",
            json!({"sessionId": session_id, "configId": config_id, "value": value}),
        )
        .await?;
    prepare_session_models(&connection, &mut reply);
    Ok(reply)
}

#[tauri::command]
pub(crate) async fn model_catalog(
    webview: tauri::Webview,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("model/catalog-unavailable".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Ok(json!({"available": false}));
    }
    connection
        .request("_dsh/gpui/model-catalog", json!({}))
        .await
}

#[tauri::command]
pub(crate) async fn deepseek_account(
    webview: tauri::Webview,
    mut request: Value,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" || !request.is_object() {
        return Err("当前窗口无法管理 DeepSeek 账号。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Ok(json!({"available":false}));
    }
    request["version"] = env!("CARGO_PKG_VERSION").into();
    let reply = connection
        .request("_dsh/gpui/deepseek-account", request)
        .await?;
    connection
        .account_models
        .store(reply["modelReady"] == true, Ordering::Release);
    Ok(reply)
}

#[tauri::command]
pub(crate) async fn account_notifications(
    webview: tauri::Webview,
    mut request: Value,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" || !request.is_object() {
        return Err("当前窗口无法读取账号通知。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Ok(Value::Null);
    }
    request["version"] = env!("CARGO_PKG_VERSION").into();
    connection
        .request("_dsh/gpui/account-notifications", request)
        .await
}

pub(crate) async fn private_platform_grant(
    service: &AcpService,
) -> Result<crate::account_platform::PlatformGrant, String> {
    let connection = connection(service).await?;
    let unavailable = || "当前 DeepSeek 账号无法使用平台页面。".to_string();
    if !connection.session_bridge {
        return Err(unavailable());
    }
    let reply = connection
        .request("_dsh/gpui/account-platform-session", json!({}))
        .await
        .map_err(|_| unavailable())?;
    serde_json::from_value(reply).map_err(|_| unavailable())
}

#[tauri::command]
pub async fn respond_permission(
    webview: tauri::Webview,
    session_id: String,
    request_id: String,
    option_id: String,
    service: State<'_, AcpService>,
) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("permission/unavailable".into());
    }
    let connection = connection(&service).await?;
    let mut approvals = connection.approvals.lock().await;
    let approval = approvals
        .get(&request_id)
        .filter(|approval| approval.session == session_id)
        .ok_or("permission/request-closed")?;
    if !approval.options.contains(&option_id) {
        return Err("permission/invalid-choice".into());
    }
    connection
        .respond(
            approval.wire_id.clone(),
            json!({"outcome": {"outcome": "selected", "optionId": option_id}}),
        )
        .await?;
    approvals.remove(&request_id);
    Ok(())
}

#[tauri::command]
pub(crate) async fn plan_mode(
    webview: tauri::Webview,
    session_id: String,
    active: Option<bool>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" || session_id.is_empty() {
        return Err("plan/unavailable".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("当前服务不支持计划模式。".into());
    }
    connection
        .request(
            "_dsh/gpui/session/plan-mode",
            json!({"sessionId": session_id, "active": active}),
        )
        .await
}

#[tauri::command]
pub(crate) async fn session_commands(
    webview: tauri::Webview,
    session_id: String,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" || session_id.is_empty() {
        return Err("command/unavailable".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("当前服务不支持读取会话指令目录。".into());
    }
    connection
        .request(
            "_dsh/gpui/session/commands",
            json!({"sessionId": session_id}),
        )
        .await
}

#[tauri::command]
pub(crate) async fn execute_session_command(
    webview: tauri::Webview,
    session_id: String,
    line: String,
    attachments: Option<Value>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" || session_id.is_empty() {
        return Err("command/unavailable".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("当前服务不支持执行会话指令。".into());
    }
    let mut attachments = attachments.unwrap_or_else(|| json!([]));
    let _files = assemble_prompt_files(&connection, &session_id, &mut attachments)?;
    connection
        .request(
            "_dsh/gpui/session/command-execute",
            json!({
                "sessionId": session_id,
                "line": line,
                "attachments": attachments,
            }),
        )
        .await
}

#[tauri::command]
pub(crate) async fn permission_mode(
    webview: tauri::Webview,
    session_id: String,
    preset: Option<String>,
    acknowledged: Option<bool>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" || session_id.is_empty() {
        return Err("permission/unavailable".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("当前服务不支持切换权限。".into());
    }
    connection
        .request(
            "_dsh/gpui/session/permission-mode",
            json!({"sessionId": session_id, "preset": preset, "acknowledged": acknowledged}),
        )
        .await
}

#[derive(serde::Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PlanDecision {
    Approve,
    Discuss,
}

#[tauri::command]
pub(crate) async fn respond_plan_review(
    webview: tauri::Webview,
    session_id: String,
    request_id: String,
    decision: PlanDecision,
    service: State<'_, AcpService>,
) -> Result<(), String> {
    if webview.label() != "main" || session_id.is_empty() || request_id.is_empty() {
        return Err("plan/review-closed".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("当前服务不支持计划审阅。".into());
    }
    connection
        .request(
            "_dsh/gpui/session/plan-review",
            json!({"sessionId": session_id, "requestId": request_id, "decision": decision}),
        )
        .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn question_state(
    webview: tauri::Webview,
    session_id: String,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" || session_id.is_empty() {
        return Err("question/unavailable".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Ok(json!([]));
    }
    connection
        .request(
            "_dsh/gpui/session/questions",
            json!({"sessionId": session_id}),
        )
        .await
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ModelProviderAction {
    List,
    Save,
    Remove,
    Discover,
    Authorize,
}

#[tauri::command]
pub(crate) async fn model_providers(
    webview: tauri::Webview,
    action: ModelProviderAction,
    draft: Option<Value>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("仅主窗口可管理模型服务。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("当前 dsh-acp 不支持编辑模型服务。".into());
    }
    let method = match action {
        ModelProviderAction::List => "_dsh/gpui/providers/list",
        ModelProviderAction::Save => "_dsh/gpui/providers/save",
        ModelProviderAction::Remove => "_dsh/gpui/providers/remove",
        ModelProviderAction::Discover => "_dsh/gpui/providers/discover",
        ModelProviderAction::Authorize => "_dsh/gpui/providers/authorization",
    };
    connection
        .request(method, draft.unwrap_or_else(|| json!({})))
        .await
}

#[tauri::command]
pub(crate) async fn models_settings(
    webview: tauri::Webview,
    request: Value,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("仅主窗口可管理模型设置。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("当前 dsh-acp 不支持模型设置。".into());
    }
    connection
        .request("_dsh/gpui/providers/remote", request)
        .await
}

#[derive(serde::Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum QuestionDecision {
    Answer,
    Cancel,
    Timeout,
}

#[derive(serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RuntimeSettingsPage {
    Shell,
    AgentLoop,
    Subagent,
}

#[tauri::command]
pub(crate) async fn read_file_image(
    webview: tauri::Webview,
    url: String,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("图片操作只能由主界面发起。".into());
    }
    connection(&service)
        .await?
        .request("_dsh/gpui/file-image", json!({"url":url}))
        .await
}

#[tauri::command]
pub(crate) async fn resolve_session_document(
    webview: tauri::Webview,
    session_id: String,
    path: String,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("文档预览必须由主界面打开。".into());
    }
    let connection = connection(&service).await?;
    let result = connection
        .request(
            "_dsh/gpui/session/resolve-document",
            json!({"sessionId":session_id,"path":path}),
        )
        .await?;
    if connection.stopping.load(Ordering::Acquire) {
        return Err("服务连接已变化，请重试。".into());
    }
    Ok(result)
}

#[tauri::command]
pub(crate) async fn settings_document(
    webview: tauri::Webview,
    operation: SettingsDocumentOperation,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("仅主窗口可打开配置文件。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge || !cfg!(target_os = "macos") {
        return match operation {
            SettingsDocumentOperation::Describe => Ok(json!({"available":false})),
            SettingsDocumentOperation::Open => Err("当前服务不支持打开配置文件。".into()),
        };
    }
    let reply = connection
        .request(
            "_dsh/gpui/settings-document",
            json!({"operation":operation}),
        )
        .await?;
    if connection.stopping.load(Ordering::Acquire) {
        return Err("服务连接已变化，请重试。".into());
    }
    match operation {
        SettingsDocumentOperation::Describe => Ok(json!({"available":reply["available"] == true})),
        SettingsDocumentOperation::Open => {
            let path = reply["path"]
                .as_str()
                .ok_or("无法定位配置文件。")?
                .to_owned();
            if !std::path::Path::new(&path).is_absolute() {
                return Err("配置文件路径无效。".into());
            }
            // Same text-editor intent as upstream openNativeTextFile on macOS.
            // No browser-supplied path or shell evaluation crosses this command.
            let status = tokio::process::Command::new("open")
                .args(["-t", &path])
                .kill_on_drop(true)
                .status()
                .await
                .map_err(|error| format!("无法打开配置文件：{error}"))?;
            if !status.success() {
                return Err(format!("无法打开配置文件：{status}"));
            }
            Ok(json!({"opened":true}))
        }
    }
}

#[derive(Clone, Copy, serde::Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SettingsDocumentOperation {
    Describe,
    Open,
}

#[tauri::command]
pub(crate) async fn runtime_settings(
    webview: tauri::Webview,
    page: RuntimeSettingsPage,
    change: Option<Value>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("仅主窗口可编辑运行设置。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Ok(Value::Null);
    }
    connection
        .request(
            "_dsh/gpui/runtime-settings",
            json!({"page":page,"change":change}),
        )
        .await
}

#[tauri::command]
pub(crate) async fn web_settings(
    webview: tauri::Webview,
    change: Option<Value>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("仅主窗口可编辑网页搜索与获取设置。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Ok(json!({"snapshot": null, "applied": []}));
    }
    connection
        .request("_dsh/gpui/web-settings", json!({"change":change}))
        .await
}

#[tauri::command]
pub(crate) async fn permission_default(
    webview: tauri::Webview,
    request: Value,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("仅主窗口可修改默认权限。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Ok(json!({"view": {"writable": false, "namespaces": []}, "catalog": null}));
    }
    connection
        .request("_dsh/gpui/permission-default", request)
        .await
}

#[tauri::command]
pub(crate) async fn client_preferences(
    webview: tauri::Webview,
    request: Value,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("仅主窗口可访问界面偏好。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Ok(json!({"writable": false, "namespaces": []}));
    }
    connection
        .request("_dsh/gpui/client-preferences", request)
        .await
}

#[tauri::command]
pub(crate) async fn agent_preset_settings(
    webview: tauri::Webview,
    request: Value,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("仅主窗口可访问 Agent 预设。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("Agent 预设服务尚未就绪，请重试。".into());
    }
    connection
        .request("_dsh/gpui/agent-preset-settings", request)
        .await
}

#[tauri::command]
pub(crate) async fn session_log_preference(
    webview: tauri::Webview,
    request: Value,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("仅主窗口可修改会话日志偏好。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Ok(json!({"available": false}));
    }
    connection
        .request("_dsh/gpui/session-log-preference", request)
        .await
}

#[tauri::command]
pub(crate) async fn plugin_inventory(
    webview: tauri::Webview,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("仅主窗口可读取插件清单。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("当前 dsh-acp 不支持插件清单。".into());
    }
    connection
        .request("_dsh/gpui/plugin-inventory", json!({}))
        .await
}

#[tauri::command]
pub(crate) async fn plugin_center(
    webview: tauri::Webview,
    operation: Option<PluginCenterOperation>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("仅主窗口可管理插件。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Ok(Value::Null);
    }
    connection
        .request("_dsh/gpui/plugin-center", json!({"operation": operation}))
        .await
}

#[derive(serde::Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum PluginCenterOperation {
    Bundle {
        name: String,
        enabled: bool,
    },
    Entry {
        name: String,
        row_id: String,
        enabled: bool,
    },
    Remove {
        name: String,
    },
}

#[derive(serde::Deserialize, Serialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum PluginInstallOperation {
    Registries,
    Versions {
        name: String,
    },
    Run {
        request_id: String,
        spec: String,
        registry: Option<String>,
    },
    Catalog {
        request_id: String,
        name: String,
        from: Option<String>,
        version: String,
        enabled: bool,
    },
    Update {
        request_id: String,
        name: String,
        from: String,
        version: String,
        registry: Option<String>,
    },
    Approve {
        request_id: String,
        previous_request_id: String,
    },
    Cancel {
        request_id: String,
    },
    Observe {
        request_id: String,
    },
}

#[tauri::command]
pub(crate) async fn plugin_install(
    webview: tauri::Webview,
    operation: PluginInstallOperation,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("仅主窗口可安装插件。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("当前 ACP 服务无法安装插件。".into());
    }
    connection
        .request("_dsh/gpui/plugin-install", json!({"operation":operation}))
        .await
}

#[derive(serde::Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub(crate) enum ScheduleOperation {
    Catalog,
    Create { request: Value },
    Edit { request: Value },
    Run { id: String },
    History { request: Value },
    ClearEnded,
}

#[tauri::command]
pub(crate) async fn schedule_tasks(
    webview: tauri::Webview,
    operation: ScheduleOperation,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("仅主窗口可管理自动化任务。".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Ok(json!({"available": false}));
    }
    connection
        .request("_dsh/gpui/schedule", json!({"operation": operation}))
        .await
}

#[tauri::command]
pub(crate) async fn set_session_archived(
    webview: tauri::Webview,
    session_id: String,
    archived: bool,
    stop_activity: bool,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("仅主窗口可归档会话。".into());
    }
    connection(&service)
        .await?
        .request(
            "_dsh/gpui/session/archive",
            json!({
                "action": if archived { "archive" } else { "restore" },
                "sessionId": session_id,
                "stopActivity": stop_activity,
            }),
        )
        .await
}

#[tauri::command]
pub(crate) async fn respond_question(
    webview: tauri::Webview,
    session_id: String,
    request_id: String,
    decision: QuestionDecision,
    answers: Option<Vec<Value>>,
    service: State<'_, AcpService>,
) -> Result<(), String> {
    if webview.label() != "main" || session_id.is_empty() || request_id.is_empty() {
        return Err("question/unavailable".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("当前服务不支持问答。".into());
    }
    connection
        .request(
            "_dsh/gpui/session/question-decision",
            json!({"sessionId": session_id, "requestId": request_id, "decision": decision, "answers": answers}),
        )
        .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn record_session_feedback(
    webview: tauri::Webview,
    session_id: String,
    text: Option<String>,
    category: Option<FeedbackCategory>,
    service: State<'_, AcpService>,
) -> Result<(), String> {
    if webview.label() != "main" || session_id.is_empty() {
        return Err("session-not-found".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("feedback-unavailable".into());
    }
    let mut params = json!({"sessionId": session_id});
    if let Some(text) = text {
        params["text"] = text.into();
    }
    if let Some(category) = category {
        params["category"] = json!(category);
    }
    let reply = connection
        .request("_dsh/gpui/session/feedback", params)
        .await?;
    if reply["ok"] == true && reply["value"]["recorded"] == true {
        Ok(())
    } else {
        Err(reply["error"]["code"]
            .as_str()
            .unwrap_or("feedback-failed")
            .to_owned())
    }
}

#[derive(Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum FeedbackCategory {
    TaskResult,
    InstructionFollowing,
    ProductInteraction,
    ServiceStability,
    ResourceCost,
    SecurityPrivacyPermission,
    Other,
}

#[derive(Serialize, serde::Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum MessageFeedbackRequest {
    List,
    Put {
        message_id: String,
        rating: MessageRating,
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        category: Option<FeedbackCategory>,
        if_version: Option<String>,
    },
    Delete {
        message_id: String,
        if_version: String,
    },
}

#[derive(Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum MessageRating {
    Positive,
    Negative,
}

#[tauri::command]
pub(crate) async fn message_feedback(
    webview: tauri::Webview,
    session_id: String,
    request: MessageFeedbackRequest,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    if webview.label() != "main" || session_id.is_empty() {
        return Err("session-not-found".into());
    }
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Err("feedback-unavailable".into());
    }
    let method = match &request {
        MessageFeedbackRequest::List => "_dsh/gpui/message-feedback/list",
        MessageFeedbackRequest::Put { .. } => "_dsh/gpui/message-feedback/put",
        MessageFeedbackRequest::Delete { .. } => "_dsh/gpui/message-feedback/delete",
    };
    let mut params = serde_json::to_value(request).map_err(|error| error.to_string())?;
    if let Value::Object(params) = &mut params {
        params.remove("operation");
        params.insert("sessionId".into(), session_id.into());
    }
    connection.request(method, params).await
}

/// Capture the current ACP process for an export. A later service restart
/// cannot silently redirect the durability request to a different process.
pub(crate) struct SessionFlush(Option<Arc<AcpConnection>>);

impl SessionFlush {
    pub(crate) async fn new(service: &AcpService) -> Self {
        Self(
            service
                .connection
                .lock()
                .await
                .clone()
                .filter(|connection| connection.session_bridge),
        )
    }

    pub(crate) async fn flush(&self, id: &str) -> Result<(), String> {
        if let Some(connection) = &self.0 {
            connection
                .request("_dsh/gpui/session/flush", json!({"sessionId":id}))
                .await?;
        }
        Ok(())
    }
}

#[tauri::command]
pub(crate) async fn watch_tool_result(
    session_id: Option<String>,
    seq: Option<u64>,
    token: String,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    let revision = NEXT_TOOL_RESULT_WATCH.fetch_add(1, Ordering::Relaxed);
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Ok(json!({"active": false}));
    }
    connection
        .request(
            "_dsh/gpui/tool-result-watch",
            json!({
                "sessionId": session_id, "seq": seq, "token": token, "revision": revision,
            }),
        )
        .await
}

#[tauri::command]
pub(crate) async fn agent_terminal_watch(
    session_id: String,
    request: Option<u64>,
    active: Option<serde_json::Value>,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    let connection = connection(&service).await?;
    if !connection.session_bridge {
        return Ok(json!({"available": false}));
    }
    connection
        .request(
            "_dsh/gpui/terminal-watch",
            json!({"sessionId": session_id, "request": request, "active": active}),
        )
        .await
}

#[tauri::command]
pub(crate) async fn agent_terminal_attach(
    session_id: String,
    terminal_id: String,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    connection(&service)
        .await?
        .request(
            "_dsh/gpui/terminal-attach",
            json!({"sessionId": session_id, "terminalId": terminal_id}),
        )
        .await
}

#[tauri::command]
pub(crate) async fn agent_terminal_detach(
    session_id: String,
    terminal_id: String,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    connection(&service)
        .await?
        .request(
            "_dsh/gpui/terminal-detach",
            json!({"sessionId": session_id, "terminalId": terminal_id}),
        )
        .await
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum AgentTerminalInput {
    Write { data: String, typed: bool },
    Resize { cols: u16, rows: u16 },
}

#[tauri::command]
pub(crate) async fn agent_terminal_input(
    session_id: String,
    terminal_id: String,
    sequence: u64,
    input: AgentTerminalInput,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    let mut params = serde_json::to_value(input).map_err(|error| error.to_string())?;
    params["sessionId"] = session_id.into();
    params["terminalId"] = terminal_id.into();
    params["sequence"] = sequence.into();
    connection(&service)
        .await?
        .request("_dsh/gpui/terminal-input", params)
        .await
}

#[tauri::command]
pub(crate) async fn agent_terminal_rename(
    session_id: String,
    terminal_id: String,
    title: String,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    connection(&service)
        .await?
        .request(
            "_dsh/gpui/terminal-rename",
            json!({
                "sessionId": session_id, "terminalId": terminal_id, "title": title,
            }),
        )
        .await
}

#[tauri::command]
pub(crate) async fn agent_terminal_close(
    session_id: String,
    terminal_id: String,
    service: State<'_, AcpService>,
) -> Result<Value, String> {
    connection(&service)
        .await?
        .request(
            "_dsh/gpui/terminal-close",
            json!({"sessionId": session_id, "terminalId": terminal_id}),
        )
        .await
}

async fn connection(service: &AcpService) -> Result<Arc<AcpConnection>, String> {
    service
        .connection
        .lock()
        .await
        .clone()
        .ok_or_else(|| "dsh-acp 尚未连接".to_string())
}

fn resolve_workspace(value: Option<&str>) -> Result<PathBuf, String> {
    let path = match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => absolute_path(Path::new(value))?,
        None => default_workspace()?,
    };
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("工作区不可访问 {}：{error}", path.display()))?;
    if !canonical.is_dir() {
        return Err(format!("工作区不是目录：{}", canonical.display()));
    }
    Ok(canonical)
}

/// Harness starts a session from the workspace cwd, never from the process cwd.
/// A GUI launch on macOS inherits `/` as the working directory, which used to
/// make the `@` file browser list the entire filesystem root. Resolve a real
/// workspace instead: an explicit override, the enclosing KWork checkout of
/// this dev build, or the user's Documents/home directory.
fn default_workspace() -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Some(value) = std::env::var_os("KWORK_WORKSPACE") {
        candidates.push(PathBuf::from(value));
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        for _ in 0..10 {
            let Some(current) = dir else { break };
            if current.join("Cargo.toml").is_file() && current.join("ui").is_dir() {
                candidates.push(current.clone());
                break;
            }
            dir = current.parent().map(Path::to_path_buf);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(home.join("Documents"));
        candidates.push(home);
    }
    if let Ok(cwd) = std::env::current_dir()
        && cwd != Path::new("/")
    {
        candidates.push(cwd);
    }
    for candidate in candidates {
        if candidate.is_absolute() && candidate.is_dir() {
            return Ok(candidate);
        }
    }
    Err("无法确定工作区目录，请在设置中指定工作区".to_owned())
}

/// Resolve a Node.js binary for spawning dsh-acp and the credential helper.
/// GUI launches often have a minimal PATH, so probe known install locations.
pub(crate) fn resolve_node() -> std::ffi::OsString {
    if let Some(value) = std::env::var_os("DSH_NODE") {
        let path = PathBuf::from(&value);
        if path.is_file() || which_exists(&path) {
            return value;
        }
    }
    if which_exists(Path::new("node")) {
        return "node".into();
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    // Prefer a full Node install (fnm/homebrew) over the Harness runtime Node —
    // the latter can miss native addons built for a different ABI.
    let mut candidates = Vec::new();
    if let Ok(entries) = std::fs::read_dir(home.join(".local/share/fnm/node-versions")) {
        let mut versions: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
        versions.sort();
        versions.reverse();
        for version in versions {
            candidates.push(version.join("installation/bin/node"));
        }
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/node"),
        PathBuf::from("/usr/local/bin/node"),
        home.join(".dsh/dsh-runtimes/dsh-primary-runtime/dependencies/node/bin/node"),
    ]);
    #[cfg(windows)]
    {
        candidates.extend(windows_node_candidates());
    }
    for candidate in candidates {
        if candidate.is_file() {
            return candidate.into_os_string();
        }
    }
    // Fall back to bare name so the spawn error still mentions "node".
    "node".into()
}

/// Common Windows Node install locations. Also compiled on macOS so the
/// path handling here is always type-checked.
#[allow(dead_code)]
fn windows_node_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for key in [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "LOCALAPPDATA",
        "APPDATA",
    ] {
        if let Some(base) = std::env::var_os(key) {
            let base = PathBuf::from(base);
            candidates.push(base.join("nodejs").join("node.exe"));
            candidates.push(base.join("Programs").join("nodejs").join("node.exe"));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA")
        && let Ok(entries) = std::fs::read_dir(PathBuf::from(local).join("fnm_multishells"))
    {
        for entry in entries.flatten() {
            candidates.push(entry.path().join("node.exe"));
        }
    }
    if let Some(appdata) = std::env::var_os("APPDATA")
        && let Ok(entries) = std::fs::read_dir(PathBuf::from(appdata).join("nvm"))
    {
        for entry in entries.flatten() {
            candidates.push(entry.path().join("node.exe"));
        }
    }
    candidates
}

fn which_exists(path: &Path) -> bool {
    if path.components().count() == 1 {
        let Some(path_env) = std::env::var_os("PATH") else {
            return false;
        };
        for dir in std::env::split_paths(&path_env) {
            if dir.join(path).is_file() {
                return true;
            }
            if cfg!(windows) && path.extension().is_none() {
                let mut exe = path.to_path_buf();
                exe.set_extension("exe");
                if dir.join(exe).is_file() {
                    return true;
                }
            }
        }
        false
    } else {
        path.is_file()
    }
}

pub(crate) fn resolve_service_path(value: Option<&str>) -> Result<PathBuf, String> {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        let path = absolute_path(Path::new(value))?;
        return path
            .canonicalize()
            .map_err(|error| format!("dsh-acp 路径不可访问 {}：{error}", path.display()));
    }
    if let Some(path) = std::env::var_os("DSH_ACP_PATH") {
        let path = PathBuf::from(path);
        if path.is_dir() {
            return path
                .canonicalize()
                .map_err(|error| format!("dsh-acp 路径不可访问 {}：{error}", path.display()));
        }
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    #[cfg(target_os = "macos")]
    {
        let host = manifest.join("../services/macos-host");
        if host
            .join("node_modules/@deepseek-ai/dsh/lib/profile-boot.js")
            .is_file()
        {
            return host.canonicalize().map_err(|error| error.to_string());
        }
    }
    let candidates = [
        manifest.join("../../dsh-acp"),
        manifest.join("../dsh-acp"),
        PathBuf::from("./dsh-acp"),
        PathBuf::from("../dsh-acp"),
    ];
    candidates
        .iter()
        .find(|path| path.is_dir())
        .and_then(|path| path.canonicalize().ok())
        .ok_or_else(|| {
            "未找到 ./dsh-acp。请在设置中填写服务路径，或设置 DSH_ACP_PATH。".to_string()
        })
}

fn absolute_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path))
    }
}

impl AcpService {
    /// Best-effort synchronous cleanup during Tauri's terminal run event.
    pub fn shutdown_now(&self) {
        if let Ok(mut slot) = self.connection.try_lock()
            && let Some(connection) = slot.take()
        {
            connection.stop_now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MessageFeedbackRequest, decode_response, filter_account_models, module_import_specifier,
    };
    use serde_json::json;

    #[test]
    fn generated_esm_import_encodes_spaces_and_hashes_in_the_file_url() {
        let path = std::env::temp_dir()
            .join("KWork #1")
            .join("agent terminal.mjs");
        let specifier: String =
            serde_json::from_str(&module_import_specifier(&path).unwrap()).unwrap();
        assert!(specifier.starts_with("file://"));
        assert!(specifier.ends_with("/KWork%20%231/agent%20terminal.mjs"));
        assert_eq!(
            url::Url::parse(&specifier).unwrap().to_file_path().unwrap(),
            path
        );
    }
    #[test]
    fn signed_out_account_choices_are_hidden_and_session_selection_is_retained() {
        let mut options = json!([{"id":"model","currentValue":"[\"deepseek-account\",\"deepseek-v4-pro\"]",
            "options":[
                {"group":"deepseek-account","options":[{"value":"[\"deepseek-account\",\"deepseek-v4-pro\"]"}]},
                {"group":"deepseek-official","options":[
                    {"value":"[\"deepseek-official\",\"deepseek-v4-flash\"]"},
                    {"value":"[\"deepseek-official\",\"deepseek-v4-pro\"]"}
                ]},
                {"group":"custom","options":[{"value":"[\"custom\",\"private-model\"]"}]}
            ]
        }]);
        filter_account_models(&mut options);
        assert_eq!(options[0]["options"][0]["group"], "deepseek-official");
        assert_eq!(options[0]["options"][1]["group"], "custom");
        assert_eq!(
            options[0]["currentValue"],
            "[\"deepseek-account\",\"deepseek-v4-pro\"]"
        );
    }

    #[test]
    fn response_returns_the_result_without_transport_fields() {
        let response = json!({"jsonrpc": "2.0", "id": 4, "result": {"sessionId": "abc"}});
        assert_eq!(
            decode_response(&response).expect("valid response"),
            json!({"sessionId": "abc"})
        );
    }

    #[test]
    fn response_preserves_acp_error_detail() {
        let response = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "error": {"code": -32602, "message": "Invalid params", "data": "cwd must be absolute"}
        });
        let error = decode_response(&response).expect_err("invalid response");
        assert!(error.contains("-32602"));
        assert!(error.contains("cwd must be absolute"));
    }

    #[test]
    fn first_rating_sends_an_explicit_absence_precondition() {
        let request: MessageFeedbackRequest = serde_json::from_value(json!({
            "operation": "put", "messageId": "answer", "rating": "positive",
        }))
        .unwrap();
        let wire = serde_json::to_value(request).unwrap();
        assert_eq!(
            wire,
            json!({
                "operation": "put", "messageId": "answer",
                "rating": "positive", "ifVersion": null,
            })
        );
    }
}
