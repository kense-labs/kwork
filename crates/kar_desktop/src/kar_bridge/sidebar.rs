//! Cross-window thread metadata comes from Kar's authenticated project/thread store.
use super::*;
use axum::http::Method;
use serde_json::{Value, json};
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub enum SidebarAction {
    Pin(bool),
    Settle(bool),
    Archive(bool),
    Visit(u64),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SidebarMetadata {
    pub title: Option<String>,
    pub pinned: bool,
    pub settled: bool,
    pub archived: bool,
    pub visited_ms: Option<u64>,
}

#[derive(Deserialize)]
struct Row {
    project: Uuid,
    info: Id,
    archived: bool,
    metadata: Metadata,
}
#[derive(Deserialize)]
struct Id {
    id: Uuid,
}
#[derive(Deserialize)]
struct Metadata {
    title: Option<String>,
    pinned_ms: Option<u64>,
    settled_override: Option<String>,
    updated_ms: u64,
    visited_ms: Option<u64>,
}
#[derive(Deserialize)]
struct Rows {
    chats: Vec<Row>,
}

fn updates() -> &'static tokio::sync::broadcast::Sender<PathBuf> {
    static SIGNAL: OnceLock<tokio::sync::broadcast::Sender<PathBuf>> = OnceLock::new();
    SIGNAL.get_or_init(|| tokio::sync::broadcast::channel(64).0)
}

async fn rows(engine: &crate::engine::Engine) -> Result<Vec<Row>> {
    let load = |scope: &'static str| async move {
        let body = engine
            .request(
                Method::POST,
                "/chat-state/query",
                Some(json!({"query":scope})),
            )
            .await?;
        Ok::<_, anyhow::Error>(serde_json::from_value::<Rows>(body)?.chats)
    };
    let (mut active, archived) = tokio::try_join!(load("active"), load("archived"))?;
    active.extend(archived);
    Ok(active)
}

async fn post(
    engine: &crate::engine::Engine,
    project: Uuid,
    chat: Uuid,
    mut action: Value,
    command: Uuid,
) -> Result<()> {
    action["command_id"] = json!(command);
    engine
        .request(
            Method::POST,
            &format!("/projects/{project}/chats/{chat}/metadata"),
            Some(action),
        )
        .await?;
    Ok(())
}

fn projection(rows: Vec<Row>) -> BTreeMap<Uuid, SidebarMetadata> {
    rows.into_iter()
        .map(|r| {
            (
                r.info.id,
                SidebarMetadata {
                    title: r.metadata.title,
                    pinned: r.metadata.pinned_ms.is_some(),
                    settled: r.metadata.settled_override.as_deref() == Some("settled"),
                    archived: r.archived,
                    visited_ms: r.metadata.visited_ms,
                },
            )
        })
        .collect()
}

async fn migrate(engine: &crate::engine::Engine, dir: &Path, rows: &[Row]) -> Result<bool> {
    let titles = load_persisted_titles(dir);
    let pins = crate::studio_hub::ProjectRegistry::load_or_seed(dir, engine.root()).pinned_chats;
    let settled = crate::prefs::DesktopPreferences::load(dir).settled_chats;
    let mut changed = false;
    for row in rows {
        let id = row.info.id;
        if !titles.contains_key(&id) && !pins.contains(&id) && !settled.contains(&id) {
            continue;
        }
        let marker = format!("kar-metadata-migrated-{id}");
        if dir.join(&marker).is_file() {
            continue;
        }
        let mut actions = vec![];
        if row.metadata.title.is_none()
            && let Some(title) = titles.get(&id)
        {
            actions.push(("title", json!({"action":"rename","title":title})));
        }
        // Never revive an explicit unpin or unsettle performed by a newer client.
        if !row.archived && row.metadata.updated_ms == 0 {
            if pins.contains(&id) {
                actions.push(("pin", json!({"action":"pin"})));
            }
            if settled.contains(&id) {
                actions.push(("settle", json!({"action":"settle"})));
            }
        }
        for (kind, action) in actions {
            let command = Uuid::new_v5(&id, format!("kwork-legacy-metadata-{kind}").as_bytes());
            post(engine, row.project, id, action, command).await?;
            changed = true;
        }
        crate::client_store::write_desktop_preference(dir, &marker, "1")?;
    }
    Ok(changed)
}

pub(super) fn watch(
    engine: crate::engine::Engine,
    dir: PathBuf,
    events: async_channel::Sender<KarBridgeEvent>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut sub = updates().subscribe();
        let mut first = true;
        let mut last = None;
        loop {
            let result = async {
                let mut list = rows(&engine).await?;
                if first && migrate(&engine, &dir, &list).await? {
                    list = rows(&engine).await?;
                }
                Ok::<_, anyhow::Error>(projection(list))
            }
            .await;
            match result {
                Ok(meta) => {
                    first = false;
                    if last.as_ref() != Some(&meta) {
                        last = Some(meta.clone());
                        if events
                            .send(KarBridgeEvent::SidebarMetadata {
                                dir: dir.clone(),
                                rows: meta,
                            })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                Err(error) => log::warn!("thread metadata unavailable: {error}"),
            }
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(30)) => break,
                    change = sub.recv() => match change {
                        Ok(changed) if changed != dir => continue,
                        _ => break,
                    }
                }
            }
        }
    })
}

pub(super) async fn change(
    state: &WorkerState,
    chat: Uuid,
    rename: Option<String>,
    action: Option<SidebarAction>,
) -> Result<()> {
    let engine = state.engine.as_ref().context("Kar 工作台服务不可用")?;
    let list = rows(engine).await?;
    let row = list
        .into_iter()
        .find(|r| r.info.id == chat)
        .context("当前工作台未登记这段对话所属的项目")?;
    let project = row.project;
    match (rename, action) {
        (Some(title), None) => {
            post(
                engine,
                project,
                chat,
                json!({"action":"rename","title":title}),
                Uuid::new_v4(),
            )
            .await?
        }
        (None, Some(SidebarAction::Archive(archived))) => {
            engine
                .request(
                    Method::PUT,
                    &format!("/projects/{project}/chats/{chat}/archive"),
                    Some(json!({"archived":archived})),
                )
                .await?;
        }
        (None, Some(SidebarAction::Visit(visited_ms))) => {
            post(engine, project, chat, json!({"action":"visit","visited_ms":visited_ms,"opened":true}), Uuid::new_v4()).await?;
        }
        (None, Some(action)) => {
            let action = match action {
                SidebarAction::Pin(true) => "pin",
                SidebarAction::Pin(false) => "unpin",
                SidebarAction::Settle(true) => "settle",
                SidebarAction::Settle(false) => "unsettle",
                SidebarAction::Archive(_) | SidebarAction::Visit(_) => unreachable!(),
            };
            post(
                engine,
                project,
                chat,
                json!({"action":action}),
                Uuid::new_v4(),
            )
            .await?;
        }
        _ => anyhow::bail!("invalid thread action"),
    }
    let _ = updates().send(state.store_dir.clone());
    Ok(())
}

pub(super) async fn archived(engine: &crate::engine::Engine, chat: Uuid) -> Result<bool> {
    let reply = engine
        .request(
            Method::POST,
            "/chat-state/query",
            Some(json!({"query":"archived"})),
        )
        .await?;
    Ok(serde_json::from_value::<Rows>(reply)?
        .chats
        .iter()
        .any(|r| r.info.id == chat))
}

pub(super) async fn project(engine: &crate::engine::Engine, chat: Uuid) -> Result<Uuid> {
    rows(engine).await?.into_iter().find(|r|r.info.id==chat)
        .map(|r|r.project).context("未找到此对话所属的 Kar 项目")
}
