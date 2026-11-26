//! Project-scoped file listings and clone receipts from the Kar workbench.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use axum::http::Method;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::json;

use super::Engine;
use crate::studio_hub::{WorkspaceTreeItem, should_skip_dir};

#[derive(Deserialize)]
#[serde(untagged)]
enum EntryPath {
    Text(PathBuf),
    Raw { base64: String },
}

impl EntryPath {
    fn into_path(self) -> Result<PathBuf> {
        match self {
            Self::Text(path) => Ok(path),
            Self::Raw { base64 } => {
                let bytes = STANDARD.decode(base64)?;
                #[cfg(unix)]
                {
                    use std::os::unix::ffi::OsStringExt;
                    Ok(std::ffi::OsString::from_vec(bytes).into())
                }
                #[cfg(not(unix))]
                Ok(String::from_utf8(bytes)?.into())
            }
        }
    }
}

#[derive(Deserialize)]
struct Entry {
    path: EntryPath,
    kind: String,
}

#[derive(Deserialize)]
struct Listing {
    entries: Vec<Entry>,
}

impl Engine {
    pub(crate) async fn file_tree(&self, show_hidden: bool) -> Result<Vec<WorkspaceTreeItem>> {
        let response = self
            .request(
                Method::GET,
                &format!("/projects/{}/entries", self.project_id()),
                None,
            )
            .await?;
        let listing: Listing = serde_json::from_value(response)?;
        let mut children = std::collections::HashMap::<PathBuf, Vec<WorkspaceTreeItem>>::new();
        for entry in listing.entries {
            let path = entry.path.into_path()?;
            let Some(parent) = path.parent() else {
                continue;
            };
            if path.components().count() > 4 {
                continue;
            }
            let name = path
                .file_name()
                .context("Invalid workspace entry")?
                .to_string_lossy();
            if should_skip_dir(&name) || (!show_hidden && name.starts_with('.')) {
                continue;
            }
            children
                .entry(parent.to_owned())
                .or_default()
                .push(WorkspaceTreeItem {
                    id: path.to_string_lossy().into_owned(),
                    name: name.into_owned(),
                    rel_path: path,
                    is_dir: entry.kind == "directory",
                    children: vec![],
                });
        }
        fn take_children(
            children: &mut std::collections::HashMap<PathBuf, Vec<WorkspaceTreeItem>>,
            path: &Path,
            budget: &mut usize,
        ) -> Vec<WorkspaceTreeItem> {
            let mut nodes = children.remove(path).unwrap_or_default();
            nodes.sort_by_cached_key(|n| {
                (!n.is_dir, n.name.starts_with('.'), n.name.to_lowercase())
            });
            let mut count = 0;
            for node in &mut nodes {
                if *budget == 0 {
                    break;
                }
                count += 1;
                *budget -= 1;
                if node.is_dir {
                    node.children = take_children(children, &node.rel_path, budget);
                }
            }
            nodes.truncate(count);
            nodes
        }
        Ok(take_children(&mut children, Path::new(""), &mut 220))
    }

    pub(crate) async fn clone_repository(&self, source: &str) -> Result<PathBuf> {
        let source = source.trim();
        let normalized;
        let source = if !source.contains("://")
            && !Path::new(source).is_absolute()
            && let Some((remote, path)) = source.split_once(':')
        {
            ensure!(
                remote.contains('@') && !remote.contains('/') && !path.is_empty(),
                "无效的 SSH 仓库地址"
            );
            normalized = format!("ssh://{remote}/{}", path.trim_start_matches('/'));
            &normalized
        } else {
            source
        };
        let parsed;
        let path = if Path::new(source).is_absolute() {
            Path::new(source)
        } else {
            parsed = url::Url::parse(source).context("无效的仓库地址")?;
            Path::new(parsed.path())
        };
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .trim_end_matches(".git");
        ensure!(
            !name.is_empty() && name != "." && name != "..",
            "无效的仓库目录名"
        );
        let dest = self.root().join(name);
        ensure!(!dest.exists(), "目标目录已存在 {}", dest.display());
        let result = self
            .request(
                Method::POST,
                "/projects/clone",
                Some(json!({"source":source,"directory":dest,"name":name})),
            )
            .await?;
        Ok(PathBuf::from(
            result["root"]
                .as_str()
                .context("Kar 未返回克隆项目的位置")?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kar::chat::ChatStore;

    #[tokio::test]
    async fn scoped_file_listing_and_clone_use_registered_projects() -> Result<()> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("source");
        let destination = root.path().join("clones");
        std::fs::create_dir_all(source.join("docs"))?;
        std::fs::create_dir(&destination)?;
        std::fs::write(source.join("docs/指南.md"), "Kwork")?;
        std::fs::write(source.join(".local"), "hidden")?;
        let store = ChatStore::open(root.path().join("chats"))?;
        let engine = Engine::open(store.clone(), &source)?;
        engine.git_action(json!({"op":"init"})).await?;
        let visible = engine.file_tree(false).await?;
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].children[0].rel_path, Path::new("docs/指南.md"));
        assert!(
            engine
                .file_tree(true)
                .await?
                .iter()
                .any(|n| n.name == ".local")
        );
        let other = Engine::open(store.clone(), &destination)?;
        let registered = other.clone_repository(source.to_str().unwrap()).await?;
        assert_eq!(registered, destination.canonicalize()?.join("source"));
        assert!(registered.join(".git").is_dir());
        assert!(
            other
                .clone_repository(source.to_str().unwrap())
                .await
                .is_err()
        );
        Ok(())
    }
}
