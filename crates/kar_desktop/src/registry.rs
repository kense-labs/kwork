//! ACP registry actions run through Kar, which owns the launch receipts and profile revisions.
use anyhow::{Context, Result};
use axum::http::Method;
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
};

use crate::{
    engine::Engine,
    kar_bridge::{KarBridgeEvent, augmented_path_os},
    prefs::CustomAcpAgent,
    types::{ModelCatalogState, ProviderAuthState, ProviderAvailability, ProviderStatusInfo},
};

#[derive(Clone, Debug, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
}
#[derive(Clone, Debug)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub runtime: String,
    pub profile: bool,
    pub version: String,
}
#[derive(Debug)]
pub enum Action {
    List,
    Search(String),
    Prepare(String),
    Register(CustomAcpAgent),
    Remove(Installed),
}

static NAMES: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();
pub(crate) fn name(runtime: &str) -> Option<String> {
    NAMES.get()?.read().ok()?.get(runtime).cloned()
}

pub(crate) fn merge(providers: &mut Vec<ProviderStatusInfo>, installed: &[Installed]) {
    providers.retain(|p| {
        if crate::aster::is_aster(&p.id) || !(p.id.starts_with("acp:") || p.id.starts_with("acp-profile:")) {
            return true;
        }
        installed.iter().any(|item| item.runtime == p.id
            && matches!(&p.availability, ProviderAvailability::Available { version, .. } if *version == item.version))
    });
    for item in installed {
        if providers.iter().any(|p| p.id == item.runtime) { continue; }
        providers.push(ProviderStatusInfo {
            id: item.runtime.clone(),
            protocol: "acp".into(),
            workspace_commands: None,
            availability: ProviderAvailability::Available {
                version: item.version.clone(),
                command: None,
            },
            auth: ProviderAuthState::NotChecked,
            mcp_office_supported: false,
            steering_supported: false,
            goals_supported: false,
            can_install: false,
            install_lines: vec![],
            models: vec![],
            model_catalog: ModelCatalogState::Loading,
            catalog_workspace: None,
            quota_summary: None,
        });
    }
}

fn program(command: &str) -> Result<PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() {
        return Ok(path.canonicalize()?);
    }
    anyhow::ensure!(
        path.components().count() == 1,
        "ACP 程序请使用绝对路径或 PATH 中的命令名"
    );
    std::env::split_paths(&augmented_path_os())
        .map(|dir| dir.join(path))
        .find(|p| p.is_file())
        .with_context(|| format!("未找到 ACP 程序 {command}"))?
        .canonicalize()
        .map_err(Into::into)
}

async fn installed(engine: &Engine) -> Result<Vec<Installed>> {
    let response = engine
        .request(Method::GET, "/acp-registry/instances", None)
        .await?;
    let mut entries = vec![];
    for (key, profile) in [("instances", false), ("profiles", true)] {
        for row in response[key].as_array().context("ACP 清单格式无效")? {
            let runtime = row["runtime"].as_str().context("ACP 条目缺少运行时")?;
            if crate::aster::is_aster(runtime) {
                continue;
            }
            entries.push(Installed {
                id: row[if profile { "id" } else { "agent_id" }]
                    .as_str()
                    .context("ACP 条目缺少标识")?
                    .into(),
                name: row["name"].as_str().unwrap_or(runtime).into(),
                runtime: runtime.into(),
                profile,
                version: row["version"].as_str().unwrap_or_default().into(),
            });
        }
    }
    let mut names = NAMES
        .get_or_init(Default::default)
        .write()
        .unwrap_or_else(|e| e.into_inner());
    for entry in &entries {
        names.insert(entry.runtime.clone(), entry.name.clone());
    }
    Ok(entries)
}

pub(crate) async fn run(
    engine: Engine,
    action: Action,
    events: async_channel::Sender<KarBridgeEvent>,
) -> Result<()> {
    match action {
        Action::List => {}
        Action::Search(query) => {
            let query = form_urlencoded::Serializer::new(String::new())
                .append_pair("query", &query)
                .finish();
            let value = engine
                .request(
                    Method::GET,
                    &format!("/acp-registry/search?{query}&limit=30"),
                    None,
                )
                .await?;
            let rows = serde_json::from_value(value["agents"].clone())?;
            let _ = events.send(KarBridgeEvent::RegistryCatalog(rows)).await;
            return Ok(());
        }
        Action::Prepare(id) => {
            if !crate::node_runtime::has_npm_in_path(&augmented_path_os()) {
                let _ = events
                    .send(KarBridgeEvent::ToastMessage {
                        text: format!("正在准备运行环境并安装插件 {id}…"),
                        warning: false,
                    })
                    .await;
                let _ = tokio::task::spawn_blocking(|| {
                    crate::node_runtime::ensure_node_and_npm(&augmented_path_os())
                })
                .await;
            }
            engine
                .request(
                    Method::POST,
                    "/acp-registry/prepare",
                    Some(json!({"agent_id":id})),
                )
                .await
                .map_err(|e| anyhow::anyhow!("{}", crate::node_runtime::humanize_agent_error(&e.to_string())))?;
            let _ = events
                .send(KarBridgeEvent::ToastMessage {
                    text: format!("{id} 已准备就绪"),
                    warning: false,
                })
                .await;
        }
        Action::Register(agent) => {
            let path = program(&agent.command)?;
            let body = json!({"name":agent.name, "source":"local", "command_path":path, "command_args":agent.args});
            engine
                .request(
                    Method::PUT,
                    &format!("/acp-registry/profiles/{}", agent.id),
                    Some(body),
                )
                .await?;
            let _ = events
                .send(KarBridgeEvent::LegacyAcpRegistered(agent.id))
                .await;
        }
        Action::Remove(item) => {
            let route = if item.profile {
                format!("/acp-registry/profiles/{}", item.id)
            } else {
                format!("/acp-registry/instances/{}", item.runtime)
            };
            engine.request(Method::DELETE, &route, None).await?;
        }
    }
    let _ = events
        .send(KarBridgeEvent::RegistryInstalled(installed(&engine).await?))
        .await;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use kar::chat::ChatStore;

    #[tokio::test]
    async fn local_acp_profile_is_registered_selectable_and_removable() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let workspace = dir.path().join("workspace");
        std::fs::create_dir(&workspace)?;
        let store = ChatStore::open(dir.path().join("chats"))?;
        let engine = Engine::open(store.clone(), &workspace)?;
        let (events, _updates) = async_channel::unbounded();
        run(
            engine.clone(),
            Action::Register(CustomAcpAgent {
                id: "qa-echo".into(),
                name: "Local ACP test".into(),
                command: "/bin/cat".into(),
                args: vec![],
                enabled: true,
            }),
            events.clone(),
        )
        .await?;
        drop(engine);
        let restored = Engine::open(store, &workspace)?;
        let profiles = installed(&restored).await?;
        let item = profiles
            .iter()
            .find(|p| p.id == "qa-echo")
            .context("profile was not restored")?;
        let mut choices = vec![];
        merge(&mut choices, &profiles);
        assert!(choices.iter().any(|p| p.id == item.runtime));
        assert_eq!(name(&item.runtime).as_deref(), Some("Local ACP test"));
        run(restored.clone(), Action::Remove(item.clone()), events).await?;
        assert!(installed(&restored).await?.is_empty());
        Ok(())
    }
}
