//! Desktop windows share one Kar service lease. UI calls use its authenticated
//! in-process router. Native providers reach their chat-scoped MCP endpoint on loopback.
mod files;
pub(crate) mod forge;
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak, mpsc},
    thread,
};

use anyhow::{Context, Result, bail};
use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    http::{Method, Request, header},
};
use kar::{chat::ChatStore, workbench::Workbench};
use serde_json::Value;
use tokio::{runtime::Runtime, sync::oneshot};
use tower::ServiceExt;
use uuid::Uuid;

type Hosts = Mutex<HashMap<PathBuf, Weak<Host>>>;

#[derive(Clone)]
pub(crate) struct Engine {
    host: Arc<Host>,
    project: Uuid,
    root: PathBuf,
}

struct Host {
    tx: Option<mpsc::Sender<Job>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for Host {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

enum Job {
    Register(PathBuf, mpsc::Sender<Result<Uuid>>),
    Endpoint(mpsc::Sender<Result<String>>),
    Request {
        method: Method,
        path: String,
        body: Option<(&'static str, Vec<u8>)>,
        guest: Option<String>,
        reply: oneshot::Sender<Result<Bytes>>,
    },
}

struct Service {
    address: std::net::SocketAddr,
    router: Router,
    app: Workbench,
    token: String,
    runtime: Runtime,
}

impl Service {
    fn open(
        store: &ChatStore,
        roots: &BTreeSet<PathBuf>,
        listener: &std::net::TcpListener,
    ) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let _entered = runtime.enter();
        let roots: Vec<_> = roots.iter().cloned().collect();
        // Kar releases its process lease when the last worker finishes. A window can
        // reopen in the short interval after its predecessor closed its router.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let app = loop {
            match Workbench::open(store.clone(), &roots) {
                Err(kar::Error::Io(error))
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                result => break result?,
            }
        };
        let token = Uuid::new_v4().to_string();
        let listener = listener.try_clone()?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let router = app.router_at(token.clone(), address)?;
        let native = router.clone().layer(axum::middleware::from_fn(
            |request: axum::extract::Request, next: axum::middleware::Next| async move {
                let path = request.uri().path();
                if path == "/mcp"
                    || path.starts_with("/assets/")
                    || path.starts_with("/hooks/workflows/")
                {
                    next.run(request).await
                } else {
                    axum::response::IntoResponse::into_response(axum::http::StatusCode::NOT_FOUND)
                }
            },
        ));
        let listener = tokio::net::TcpListener::from_std(listener)?;
        runtime.spawn(async move {
            if let Err(error) = axum::serve(listener, native).await {
                log::warn!("Kar native endpoint closed: {error}");
            }
        });
        drop(_entered);
        Ok(Self {
            address,
            router,
            app,
            token,
            runtime,
        })
    }

    fn register(&self, root: &Path) -> Result<Uuid> {
        let name = root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Workspace");
        Ok(self.app.register(root, name)?.id)
    }

    async fn dispatch(
        router: Router,
        token: String,
        method: Method,
        path: String,
        body: Option<(&'static str, Vec<u8>)>,
    ) -> Result<Bytes> {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"));
        let request = match body {
            Some((kind, data)) => request
                .header(header::CONTENT_TYPE, kind)
                .body(Body::from(data))?,
            None => request.body(Body::empty())?,
        };
        let response = router.oneshot(request).await?;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 32 * 1024 * 1024).await?;
        if !status.is_success() {
            let json = serde_json::from_slice::<Value>(&bytes).unwrap_or_default();
            bail!(
                "Kar {status}: {}",
                json["detail"]
                    .as_str()
                    .or_else(|| json["error"].as_str())
                    .unwrap_or(status.canonical_reason().unwrap_or("request failed"))
            );
        }
        Ok(bytes)
    }

    fn ensure_idle(&self, store: &ChatStore) -> Result<()> {
        for project in self.app.projects() {
            let status_path = format!("/projects/{}/workflow-state", project.id);
            let workflow = self.runtime.block_on(Self::dispatch(
                self.router.clone(),
                self.token.clone(),
                Method::GET,
                status_path,
                None,
            ))?;
            let state: Value = serde_json::from_slice(&workflow)?;
            anyhow::ensure!(
                !state["runs"].as_array().into_iter().flatten().any(|r| {
                    matches!(
                        r["status"].as_str(),
                        Some("ready" | "running" | "waiting" | "asking")
                    )
                }),
                "请等待工作流结束，再添加此工作区"
            );
            for chat in self.app.chats(project.id, false)? {
                let ownership = store
                    .directory()
                    .join(format!("{}.kwork-writer", chat.info.id));
                if ownership.exists() {
                    let history = store.history(chat.info.id)?;
                    anyhow::ensure!(
                        !history
                            .turns
                            .last()
                            .is_some_and(|t| t.status == kar::chat::TurnStatus::Running),
                        "请等待交接会话的当前回复结束，再添加此工作区"
                    );
                    if chat.info.process_only {
                        let lease = std::fs::OpenOptions::new()
                            .read(true)
                            .write(true)
                            .open(ownership)?;
                        lease
                            .try_lock()
                            .context("请先关闭正在控制的临时智能体会话，再添加工作区")?;
                    }
                }
            }
            let path = format!("/projects/{}/terminals", project.id);
            let bytes = self.runtime.block_on(Self::dispatch(
                self.router.clone(),
                self.token.clone(),
                Method::GET,
                path,
                None,
            ))?;
            let terminals: Vec<Value> = serde_json::from_slice(&bytes)?;
            anyhow::ensure!(
                !terminals.iter().any(|term| {
                    matches!(
                        term.pointer("/status/state").and_then(Value::as_str),
                        Some("running" | "starting" | "closing")
                    )
                }),
                "请先关闭正在运行的工作区终端，再添加此工作区"
            );
        }
        Ok(())
    }
}

fn native_listener(directory: &Path) -> Result<std::net::TcpListener> {
    use std::net::{Ipv4Addr, TcpListener};
    const PORT_KEY: &str = "desktop-workbench-port";
    let previous = crate::client_store::desktop_preference(directory, PORT_KEY)
        .and_then(|s| s.trim().parse::<u16>().ok())
        .filter(|port| *port >= 1024);
    let listener =
        match previous.and_then(|port| TcpListener::bind((Ipv4Addr::LOCALHOST, port)).ok()) {
            Some(listener) => listener,
            None => TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?,
        };
    let port = listener.local_addr()?.port();
    if previous != Some(port) {
        crate::client_store::write_desktop_preference(directory, PORT_KEY, &port.to_string())?;
    }
    Ok(listener)
}

fn serve(store: ChatStore, mut roots: BTreeSet<PathBuf>, rx: mpsc::Receiver<Job>) {
    let mut service: Option<Service> = None;
    let mut requests = tokio::task::JoinSet::new();
    // Keep a published webhook URL valid when the set of authorized roots changes.
    let mut endpoint = None;
    for job in rx {
        match job {
            Job::Endpoint(reply) => {
                let _ = reply.send(
                    service
                        .as_ref()
                        .context("Workbench closed")
                        .map(|s| format!("http://{}", s.address)),
                );
            }
            Job::Register(root, reply) => {
                let result = (|| {
                    if !roots.iter().any(|allowed| root.starts_with(allowed)) {
                        if let Some(old) = &service {
                            old.runtime
                                .block_on(async { while requests.join_next().await.is_some() {} });
                            old.ensure_idle(&store)?;
                        }
                        roots.insert(root.clone());
                        service.take();
                    }
                    if service.is_none() {
                        if endpoint.is_none() {
                            endpoint = Some(native_listener(store.directory())?);
                        }
                        service = Some(Service::open(
                            &store,
                            &roots,
                            endpoint.as_ref().context("Missing native endpoint")?,
                        )?);
                    }
                    service
                        .as_ref()
                        .context("Workbench closed")?
                        .register(&root)
                })();
                let _ = reply.send(result);
            }
            Job::Request {
                method,
                path,
                body,
                guest,
                reply,
            } => {
                if let Some(active) = &service {
                    while requests.try_join_next().is_some() {}
                    let router = active.router.clone();
                    let token = guest.unwrap_or_else(|| active.token.clone());
                    requests.spawn_on(
                        async move {
                            let result = Service::dispatch(router, token, method, path, body).await;
                            let _ = reply.send(result);
                        },
                        active.runtime.handle(),
                    );
                } else {
                    let _ = reply.send(Err(anyhow::anyhow!("Workbench closed")));
                }
            }
        }
    }
}

impl Engine {
    pub(crate) fn open(store: ChatStore, workspace: &Path) -> Result<Self> {
        static HOSTS: OnceLock<Hosts> = OnceLock::new();
        let root = workspace.canonicalize()?;
        let directory = store.directory().canonicalize()?;
        anyhow::ensure!(
            root.is_dir() && !root.starts_with(&directory),
            "invalid workspace root"
        );
        let mut hosts = HOSTS
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        hosts.retain(|_, weak| weak.strong_count() > 0);
        let host = match hosts.get(&directory).and_then(Weak::upgrade) {
            Some(host) => host,
            None => {
                let roots = crate::studio_hub::ProjectRegistry::load_or_seed(&directory, &root)
                    .projects
                    .into_iter()
                    .filter_map(|p| p.root_path.canonicalize().ok())
                    .filter(|p| p.is_dir() && !p.starts_with(&directory))
                    .collect();
                let (tx, rx) = mpsc::channel();
                let worker = thread::Builder::new()
                    .name("kar-workbench".into())
                    .spawn(move || serve(store, roots, rx))?;
                let host = Arc::new(Host {
                    tx: Some(tx),
                    worker: Some(worker),
                });
                hosts.insert(directory, Arc::downgrade(&host));
                host
            }
        };
        drop(hosts);
        let (reply, rx) = mpsc::channel();
        host.tx
            .as_ref()
            .context("Workbench closed")?
            .send(Job::Register(root.clone(), reply))
            .map_err(|_| anyhow::anyhow!("Workbench closed"))?;
        let project = rx.recv()??;
        Ok(Self {
            host,
            project,
            root,
        })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn loopback_url(&self) -> Result<String> {
        let (reply, rx) = mpsc::channel();
        self.host
            .tx
            .as_ref()
            .context("Workbench closed")?
            .send(Job::Endpoint(reply))
            .map_err(|_| anyhow::anyhow!("Workbench closed"))?;
        rx.recv()?
    }
    pub(crate) fn project_id(&self) -> Uuid {
        self.project
    }

    pub(crate) async fn git_action(&self, action: Value) -> Result<Value> {
        self.request(
            Method::POST,
            &format!("/projects/{}/git", self.project),
            Some(action),
        )
        .await
    }

    pub(crate) async fn read_file(
        &self,
        file: &Path,
    ) -> Result<crate::studio_hub::WorkspaceFilePreview> {
        let path = file.strip_prefix(self.root()).unwrap_or(file);
        anyhow::ensure!(
            !path.is_absolute()
                && path.components().all(|p| matches!(
                    p,
                    std::path::Component::Normal(_) | std::path::Component::CurDir
                )),
            "文件不在工作区内"
        );
        let root = self.root.clone();
        let relative = path.to_path_buf();
        let Some(name) = path.to_str() else {
            return tokio::task::spawn_blocking(move || {
                crate::studio_hub::read_workspace_file_preview(&root, &relative)
            })
            .await?;
        };
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("path", name)
            .finish();
        let result = self
            .request(
                Method::GET,
                &format!("/projects/{}/file?{query}", self.project),
                None,
            )
            .await?;
        let text = result["text"].as_str().context("工作台未返回文件内容")?;
        let cap = text.floor_char_boundary(256 * 1024);
        let truncated = cap < text.len() || result["truncated"].as_bool().unwrap_or(false);
        let content = text[..cap].to_owned();
        Ok(tokio::task::spawn_blocking(move || {
            crate::studio_hub::workspace_preview(&root, relative, content, truncated)
        })
        .await?)
    }

    pub(crate) async fn search(
        &self,
        query: &str,
        titles: &HashMap<Uuid, String>,
    ) -> Result<Vec<crate::studio_hub::GlobalSearchHit>> {
        use crate::studio_hub::{GlobalSearchHit, truncate_chars};
        use kar::workbench::Hit;
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let query_param = form_urlencoded::Serializer::new(String::new())
            .append_pair("q", query)
            .finish();
        let mut hits = Vec::new();
        for kind in ["messages", "paths", "content"] {
            let url = format!(
                "/projects/{}/search?{query_param}&limit=16&kind={kind}",
                self.project
            );
            let entries: Vec<Hit> =
                serde_json::from_value(self.request(Method::GET, &url, None).await?)?;
            hits.extend(entries.into_iter().map(|hit| match hit {
                Hit::Path { path, .. } => GlobalSearchHit::FilePath { rel_path: path },
                Hit::Content {
                    path, line, text, ..
                } => GlobalSearchHit::FileContent {
                    rel_path: path,
                    line,
                    snippet: truncate_chars(text.trim(), 96),
                },
                Hit::Message {
                    chat, role, text, ..
                } => GlobalSearchHit::ChatMessage {
                    chat_id: chat,
                    chat_title: titles.get(&chat).cloned().unwrap_or_default(),
                    role_label: match role {
                        kar::chat::Role::User => "提问",
                        kar::chat::Role::Assistant => "回复",
                        _ => "消息",
                    },
                    snippet: crate::studio_hub::extract_match_line(&text, &query.to_lowercase()),
                },
            }));
        }
        Ok(hits)
    }

    pub(crate) async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let payload = body
            .map(|v| serde_json::to_vec(&v))
            .transpose()?
            .map(|v| ("application/json", v));
        response_json(&self.send(method, path.to_owned(), payload, None).await?)
    }

    pub(crate) async fn guest_snapshot(&self, share: Uuid, token: &str) -> Result<Value> {
        response_json(
            &self
                .send(
                    Method::GET,
                    format!("/guest/{share}/snapshot"),
                    None,
                    Some(token.to_owned()),
                )
                .await?,
        )
    }

    pub(crate) async fn post_audio(&self, path: &str, audio: Vec<u8>) -> Result<Value> {
        response_json(
            &self
                .send(
                    Method::POST,
                    path.into(),
                    Some(("application/octet-stream", audio)),
                    None,
                )
                .await?,
        )
    }

    pub(crate) async fn read_binary(&self, path: String) -> Result<Bytes> {
        self.send(Method::GET, path, None, None).await
    }

    async fn send(
        &self,
        method: Method,
        path: String,
        body: Option<(&'static str, Vec<u8>)>,
        guest: Option<String>,
    ) -> Result<Bytes> {
        let (reply, rx) = oneshot::channel();
        self.host
            .tx
            .as_ref()
            .context("Workbench closed")?
            .send(Job::Request {
                method,
                path,
                body,
                guest,
                reply,
            })
            .map_err(|_| anyhow::anyhow!("Workbench closed"))?;
        rx.await?
    }
}

fn response_json(bytes: &[u8]) -> Result<Value> {
    if bytes.is_empty() {
        Ok(Value::Null)
    } else {
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio_hub::GlobalSearchHit;

    #[tokio::test]
    async fn windows_share_a_service_and_project_data_survives_reopen() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("workspace");
        let second = temp.path().join("other");
        std::fs::create_dir_all(&workspace)?;
        std::fs::create_dir_all(&second)?;
        std::fs::write(workspace.join("notes.txt"), "产品 & 计划\n")?;
        let store = ChatStore::open(temp.path().join("chats"))?;
        let engine = Engine::open(store.clone(), &workspace)?;
        let peer = Engine::open(store.clone(), &workspace)?;
        assert!(Arc::ptr_eq(&engine.host, &peer.host));
        let id = engine.project_id();
        let origin = engine.loopback_url()?;
        let hits = peer.search("产品 & 计划", &HashMap::new()).await?;
        assert!(
            matches!(&hits[..], [GlobalSearchHit::FileContent { rel_path, line: 1, .. }] if rel_path == Path::new("notes.txt"))
        );
        engine.git_action(serde_json::json!({"op":"init"})).await?;
        peer.git_action(serde_json::json!({"op":"stage", "paths":["notes.txt"]}))
            .await?;
        assert!(
            peer.git_action(serde_json::json!({"op":"switch_branch","branch":"missing"}))
                .await
                .is_err()
        );
        let other = Engine::open(store.clone(), &second)?;
        assert_ne!(id, other.project_id());
        assert!(
            other
                .search("产品 & 计划", &HashMap::new())
                .await?
                .is_empty()
        );
        assert_eq!(
            engine
                .request(Method::GET, &format!("/projects/{id}/git"), None)
                .await?["files"][0]["index"],
            "A"
        );
        drop((other, peer, engine));
        let restored = Engine::open(store, &workspace)?;
        assert_eq!(id, restored.project_id());
        assert_eq!(origin, restored.loopback_url()?);
        Ok(())
    }

    #[test]
    fn interactive_terminal_writes_once_through_kar() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let ws = temp.path().join("workspace");
        std::fs::create_dir(&ws)?;
        let term = crate::pty_terminal::InteractivePtySession::spawn(
            &temp.path().join("store"),
            &ws,
            24,
            80,
        )?;
        term.send_line("printf '%s\\n' one >> proof.txt")?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !ws.join("proof.txt").is_file() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(
            std::fs::read_to_string(ws.join("proof.txt"))
                .with_context(|| term.snapshot(false).plain_tail)?,
            "one\n"
        );
        let sibling = temp.path().join("sibling");
        std::fs::create_dir(&sibling)?;
        let store = ChatStore::open(temp.path().join("store"))?;
        let busy = Engine::open(store.clone(), &sibling);
        assert!(
            busy.err()
                .is_some_and(|error| error.to_string().contains("终端"))
        );
        assert!(term.is_alive());
        term.terminate();
        while term.is_alive() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!term.is_alive());
        Engine::open(store, &sibling)?;
        Ok(())
    }
}
