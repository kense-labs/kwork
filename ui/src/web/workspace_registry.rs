use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisteredWorkspace {
    id: String,
    path: String,
    title: String,
    created_at: Option<u64>,
    #[serde(default)]
    session_ids: Vec<String>,
}

#[derive(Default)]
pub(super) struct WorkspaceRegistryUi {
    items: Option<Vec<RegisteredWorkspace>>,
    member_workspaces: HashMap<String, usize>,
    sequence: u64,
    pub(super) pending: bool,
    pub(super) open_after_start: Option<String>,
    pub(super) startup: Option<WorkspaceStartup>,
    generation: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkspaceStartup {
    WaitingForSessions,
    InitializingDefault,
}

impl WorkspaceRegistryUi {
    pub(super) fn reset_snapshot(&mut self) {
        self.items = None;
        self.member_workspaces.clear();
        self.sequence = 0;
        self.startup = None;
        self.generation += 1;
    }

    pub(super) fn available(&self) -> bool {
        self.items.is_some()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.items.as_ref().is_some_and(Vec::is_empty)
    }

    pub(super) fn paths(&self) -> Option<Vec<String>> {
        self.items.as_ref().map(|items| {
            items
                .iter()
                .map(|workspace| workspace.path.clone())
                .collect()
        })
    }

    fn workspace(&self, path: &str) -> Option<&RegisteredWorkspace> {
        self.items
            .as_ref()?
            .iter()
            .find(|workspace| workspace.path == path)
    }

    pub(super) fn contains(&self, path: &str) -> bool {
        self.workspace(path).is_some()
    }

    pub(super) fn id(&self, path: &str) -> Option<&str> {
        self.workspace(path).map(|workspace| workspace.id.as_str())
    }

    pub(super) fn session_path(&self, id: &str) -> Option<&str> {
        let index = *self.member_workspaces.get(id)?;
        Some(self.items.as_ref()?.get(index)?.path.as_str())
    }

    pub(super) fn title(&self, path: &str) -> Option<&str> {
        self.workspace(path)
            .map(|workspace| workspace.title.as_str())
    }

    pub(super) fn title_in_use(&self, id: &str, title: &str) -> bool {
        self.items.as_ref().is_some_and(|items| {
            items
                .iter()
                .any(|workspace| workspace.id != id && workspace.title == title)
        })
    }
}

impl DshApp {
    pub(super) fn finish_startup_session_list(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.workspace_registry.startup != Some(WorkspaceStartup::WaitingForSessions)
            || self.sessions_loading
            || !self.workspace_preferences_loaded
            || !self.connected()
        {
            return;
        }
        self.workspace_registry.startup = None;
        let saved = self
            .selected_session
            .as_ref()
            .or(self.scratch_session.as_ref());
        if let Some(mut session) = saved
            .filter(|id| !self.archived_sessions.contains(*id))
            .and_then(|id| self.sessions.iter().find(|session| &session.id == id))
            .cloned()
        {
            self.scratch_opening =
                self.scratch_session.as_ref() == Some(&session.id) && session.title.is_none();
            session.blank |= self.scratch_opening;
            self.resume(session, w, cx);
        } else if let Some(path) = self.default_session_workspace().map(str::to_owned) {
            self.new_session_at(path, None, w, cx);
        } else if self.sessions.is_empty() {
            self.initialize_default_workspace(w, cx);
        }
    }

    fn initialize_default_workspace(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        self.workspace_registry.startup = Some(WorkspaceStartup::InitializingDefault);
        let generation = self.workspace_registry.generation;
        cx.spawn_in(w, async move |this, cx| {
            let result = bridge::invoke(
                "workspace_registry",
                json!({"operation": {"action": "initialize-default"}}),
            )
            .await;
            let _ = this.update_in(cx, |app, w, cx| {
                if generation != app.workspace_registry.generation || !app.connected() {
                    return;
                }
                let open =
                    app.workspace_registry.startup == Some(WorkspaceStartup::InitializingDefault);
                app.workspace_registry.startup = None;
                match result {
                    Ok(snapshot) => {
                        app.apply_workspace_registry(&snapshot);
                        if open
                            && let (Some(path), Some(id)) =
                                (snapshot["path"].as_str(), snapshot["workspaceId"].as_str())
                            && app.workspace_registry.id(path) == Some(id)
                        {
                            app.new_session_at(path.to_owned(), None, w, cx);
                        }
                    }
                    Err(_) if open => {
                        app.push_toast("无法创建默认工作区，请通过“选择工作区”选择文件夹", cx)
                    }
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn workspace_group_hidden(&self, path: &str) -> bool {
        !self.workspace_registry.available() && self.hidden_workspaces.contains(path)
    }

    pub(super) fn apply_session_workspace(&mut self, reply: &Value, cx: &mut Context<Self>) {
        self.apply_workspace_registry(&reply["workspaceRegistry"]);
        if let Some(error) = reply["workspaceAttachError"].as_str() {
            self.push_toast(format!("会话已创建，加入工作区失败：{error}"), cx);
        }
    }

    pub(super) fn apply_workspace_registry(&mut self, snapshot: &Value) -> bool {
        let Some(sequence) = snapshot["sequence"].as_u64() else {
            return false;
        };
        if snapshot["available"] != true {
            return false;
        }
        if sequence <= self.workspace_registry.sequence {
            return true;
        }
        let Ok(items) =
            serde_json::from_value::<Vec<RegisteredWorkspace>>(snapshot["items"].clone())
        else {
            return false;
        };
        self.workspace_registry.member_workspaces = items
            .iter()
            .enumerate()
            .flat_map(|(index, workspace)| {
                workspace
                    .session_ids
                    .iter()
                    .cloned()
                    .map(move |id| (id, index))
            })
            .collect();
        self.workspace_registry.items = Some(items);
        self.workspace_registry.sequence = sequence;
        if self.workspace_context_menu.as_ref().is_some_and(|menu| {
            menu.id.is_none() || self.workspace_registry.id(&menu.path) != menu.id.as_deref()
        }) {
            self.workspace_context_menu = None;
        }
        self.apply_session_archival(&snapshot["archived"]);
        if let Ok(pinned) = serde_json::from_value::<Vec<String>>(snapshot["pinned"].clone()) {
            self.pinned_sessions = pinned;
        }
        true
    }

    pub(super) fn default_session_workspace(&self) -> Option<&str> {
        let workspaces = self.workspace_registry.items.as_ref()?;
        if let Some(path) = self
            .active_session
            .as_deref()
            .and_then(|id| self.workspace_registry.session_path(id))
        {
            return Some(path);
        }
        let mut recent = HashMap::<&str, u64>::new();
        for session in &self.sessions {
            if let Some(path) = self.workspace_registry.session_path(&session.id) {
                let time = self.session_recency(session);
                let latest = recent.entry(path).or_default();
                *latest = (*latest).max(time);
            }
        }
        workspaces
            .iter()
            .rev()
            .max_by_key(|workspace| {
                recent
                    .get(workspace.path.as_str())
                    .copied()
                    .or(workspace.created_at)
                    .unwrap_or(0)
            })
            .map(|workspace| workspace.path.as_str())
    }

    pub(super) fn set_registered_session_pin(
        &mut self,
        id: &str,
        pinned: bool,
        cx: &mut Context<Self>,
    ) {
        if self.workspace_registry.pending || self.service_starting {
            return;
        }
        if !self.connected() {
            self.push_toast("连接服务后可修改置顶", cx);
            return;
        }
        let id = id.to_owned();
        let action = if pinned { "pin" } else { "unpin" };
        let operation = json!({"operation": {"action": action, "sessionId": id}});
        self.workspace_registry.pending = true;
        cx.spawn(async move |this, cx| {
            let result = bridge::invoke("workspace_registry", operation).await;
            let _ = this.update(cx, |app, cx| {
                app.workspace_registry.pending = false;
                match result {
                    Ok(snapshot) => {
                        app.apply_workspace_registry(&snapshot);
                        if pinned && app.session_is_pinned(&id) {
                            app.sync_session_pin_order(&id);
                        }
                        app.save_workspace_preferences(cx);
                    }
                    Err(error) => app.push_toast(format!("无法修改置顶：{error}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn move_registered_workspace(
        &mut self,
        source: &str,
        before: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        if self.workspace_registry.pending || !self.connected() {
            return;
        }
        let Some(workspace) = self.workspace_registry.workspace(source) else {
            return;
        };
        let before_id = match before {
            None => None,
            Some(path) => match self.workspace_registry.workspace(path) {
                Some(anchor) => Some(anchor.id.as_str()),
                None => return,
            },
        };
        let operation =
            json!({"operation": {"action": "move", "id": workspace.id, "beforeId": before_id}});
        self.workspace_registry.pending = true;
        cx.spawn(async move |this, cx| {
            let result = bridge::invoke("workspace_registry", operation).await;
            let _ = this.update(cx, |app, cx| {
                app.workspace_registry.pending = false;
                match result {
                    Ok(snapshot) => {
                        app.apply_workspace_registry(&snapshot);
                    }
                    Err(error) => app.push_toast(format!("工作区排序失败：{error}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn open_picked_workspace(
        &mut self,
        path: String,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace_registry.startup = None;
        if self.workspace_registry.available() {
            self.register_workspace(path, true, w, cx);
        } else {
            self.show_registered_workspace(path, true, w, cx);
        }
    }

    pub(super) fn restore_workspace_group(
        &mut self,
        path: String,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace_registry.available() && !self.workspace_registry.contains(&path) {
            self.register_workspace(path, false, w, cx);
        } else {
            self.show_registered_workspace(path, false, w, cx);
        }
    }

    fn register_workspace(
        &mut self,
        requested: String,
        open: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace_registry.pending {
            return;
        }
        if !self.connected() {
            self.push_toast("连接服务后可注册工作区", cx);
            return;
        }
        self.workspace_registry.pending = true;
        cx.spawn_in(w, async move |this, cx| {
            let result = bridge::invoke(
                "workspace_registry",
                json!({"operation": {"action": "create", "path": requested}}),
            )
            .await;
            let _ = this.update_in(cx, |app, w, cx| {
                app.workspace_registry.pending = false;
                match result {
                    Ok(snapshot) => {
                        app.apply_workspace_registry(&snapshot);
                        if let Some(path) = snapshot["path"]
                            .as_str()
                            .filter(|path| app.workspace_registry.contains(path))
                        {
                            app.hidden_workspaces.remove(&requested);
                            app.show_registered_workspace(path.to_owned(), open, w, cx);
                        } else {
                            app.push_toast("服务未返回已注册的工作区", cx);
                        }
                    }
                    Err(error) => app.push_toast(format!("工作区注册失败：{error}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn show_registered_workspace(
        &mut self,
        path: String,
        open: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hidden_workspaces.remove(&path);
        if open {
            self.collapsed_workspaces.remove(&path);
            if !self.added_workspaces.contains(&path) {
                self.added_workspaces.push(path.clone());
            }
        }
        self.save_workspace_preferences(cx);
        if open {
            self.new_session_at(path, None, w, cx);
        } else {
            self.push_toast("已恢复工作区分组", cx);
        }
        cx.notify();
    }
}
