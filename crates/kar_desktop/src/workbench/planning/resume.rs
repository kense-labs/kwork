//! A workspace plan can be the last viewed conversation before a Kar chat exists.
use crate::client_store;
use crate::workbench::KarWorkbench;
use gpui::Context;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};
use uuid::Uuid;

const LAST_CHAT: &str = "last-chat";

#[derive(Debug, Deserialize, Serialize)]
struct WorkspaceSelection {
    workspace_plan: Uuid,
    previous_chat: Option<Uuid>,
}

impl WorkspaceSelection {
    fn read(dir: &Path) -> Option<Self> {
        serde_json::from_str(&client_store::desktop_preference(dir, LAST_CHAT)?).ok()
    }

    fn remember(dir: &Path, workspace_plan: Uuid) -> io::Result<()> {
        let previous_chat = match Self::read(dir) {
            Some(mark) if mark.workspace_plan == workspace_plan => return Ok(()),
            Some(mark) => mark.previous_chat,
            None => client_store::last_chat(dir),
        };
        let value = serde_json::to_string(&Self {
            workspace_plan,
            previous_chat,
        })?;
        client_store::write_desktop_preference(dir, LAST_CHAT, &value)
    }

    fn forget(dir: &Path, workspace_plan: Uuid) -> io::Result<()> {
        let Some(mark) = Self::read(dir).filter(|mark| mark.workspace_plan == workspace_plan)
        else {
            return Ok(());
        };
        if let Some(previous) = mark.previous_chat {
            client_store::remember_chat(dir, previous)
        } else {
            match fs::remove_file(dir.join(LAST_CHAT)) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                result => result,
            }
        }
    }
}

impl KarWorkbench {
    pub(in crate::workbench) fn remember_workspace_plan(&mut self, cx: &mut Context<Self>) {
        if let Err(error) =
            WorkspaceSelection::remember(&self.config.chat_dir, self.workspace_plan_id())
        {
            self.notify_warning(format!("无法保存规划的启动位置：{error}"), cx);
        }
    }

    pub(in crate::workbench) fn forget_workspace_plan(&mut self, cx: &mut Context<Self>) {
        if let Err(error) =
            WorkspaceSelection::forget(&self.config.chat_dir, self.workspace_plan_id())
        {
            self.notify_warning(format!("无法更新规划的启动位置：{error}"), cx);
        }
    }

    /// Called once, only for an ordinary launch and before a chat has been selected.
    pub(in crate::workbench) fn resume_chat_or_workspace_plan(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        let saved = match WorkspaceSelection::read(&self.config.chat_dir) {
            Some(mark) => {
                if mark.workspace_plan == self.workspace_plan_id()
                    && self.restore_workspace_plan(cx)
                {
                    return None;
                }
                mark.previous_chat
            }
            None => client_store::last_chat(&self.config.chat_dir),
        };
        let entry = if let Some(id) = saved {
            self.chat_list.iter().find(|entry| entry.info.id == id)
        } else {
            // A first launch after upgrading may have chat records without a selection.
            self.chat_list
                .iter()
                .find(|entry| entry.info.workspace == self.config.workspace_dir)
        };
        entry.map(|entry| entry.info.id)
    }
}

#[cfg(test)]
mod tests {
    use super::{WorkspaceSelection as Marker, client_store};
    use uuid::Uuid;

    #[test]
    fn a_real_chat_selection_supersedes_a_workspace_plan() {
        let dir = tempfile::tempdir().unwrap();
        let (first, second, draft) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        client_store::remember_chat(dir.path(), first).unwrap();
        Marker::remember(dir.path(), draft).unwrap();
        let mark = Marker::read(dir.path()).unwrap();
        assert_eq!(mark.workspace_plan, draft);
        assert_eq!(mark.previous_chat, Some(first));
        client_store::remember_chat(dir.path(), second).unwrap();
        Marker::forget(dir.path(), draft).unwrap();
        assert!(Marker::read(dir.path()).is_none());
        assert_eq!(client_store::last_chat(dir.path()), Some(second));
    }

    #[test]
    fn leaving_a_workspace_plan_restores_the_real_selection() {
        let dir = tempfile::tempdir().unwrap();
        let (chat, a, b) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        client_store::remember_chat(dir.path(), chat).unwrap();
        Marker::remember(dir.path(), a).unwrap();
        Marker::remember(dir.path(), b).unwrap();
        Marker::forget(dir.path(), a).unwrap();
        assert_eq!(Marker::read(dir.path()).unwrap().workspace_plan, b);
        Marker::forget(dir.path(), b).unwrap();
        assert_eq!(client_store::last_chat(dir.path()), Some(chat));

        let empty = tempfile::tempdir().unwrap();
        Marker::remember(empty.path(), a).unwrap();
        Marker::forget(empty.path(), a).unwrap();
        assert!(!empty.path().join("last-chat").exists());
    }
}
