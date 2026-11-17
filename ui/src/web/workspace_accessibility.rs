//! Publish the rendered sidebar rows to the webview's accessibility tree.

use super::session_context_menu::{SessionMenuAnchor, SessionMenuOrigin};
use super::*;
use serde::{Deserialize, Serialize};

#[wasm_bindgen(module = "/workspace-accessibility.js")]
extern "C" {
    fn update_workspace_tree(rows: JsValue, surface: JsValue, search: JsValue);
    pub(super) fn listen_workspace_tree(callback: &js_sys::Function);
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum RowAction {
    ToggleGroup {
        path: String,
        descendants: Vec<String>,
    },
    OpenSession(SessionSummary),
    SetWatermark {
        path: String,
        limit: Option<usize>,
    },
}

#[derive(PartialEq, Eq, Serialize)]
pub(super) struct AccessibleRow {
    pub(super) key: String,
    pub(super) label: String,
    pub(super) depth: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) expanded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) selected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) description: Option<String>,
    pub(super) menu: bool,
    pub(super) unarchive: bool,
    #[serde(skip)]
    pub(super) action: RowAction,
}

#[derive(PartialEq, Eq, Serialize)]
pub(super) struct AccessibleMenuItem {
    pub(super) key: String,
    pub(super) label: String,
    pub(super) disabled: bool,
    pub(super) submenu: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(super) enum AccessibleTarget {
    Session { id: String },
    Workspace { path: String, id: Option<String> },
}

#[derive(PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(super) enum AccessibleSurface {
    Menu {
        target: AccessibleTarget,
        label: String,
        submenu: bool,
        items: Vec<AccessibleMenuItem>,
    },
    Rename {
        target: AccessibleTarget,
        draft: String,
        pending: bool,
        blocked: bool,
        error: Option<String>,
    },
    Archive {
        target: AccessibleTarget,
        description: String,
        activity: Vec<String>,
        pending: bool,
        error: Option<String>,
    },
    RemoveWorkspace {
        target: AccessibleTarget,
        description: String,
        pending: bool,
        blocked: bool,
        error: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
enum SurfaceAction {
    MenuItem {
        target: AccessibleTarget,
        key: String,
    },
    MenuFocus {
        target: AccessibleTarget,
        key: String,
    },
    MenuDismiss {
        target: AccessibleTarget,
    },
    RenameDraft {
        target: AccessibleTarget,
        value: String,
    },
    RenameConfirm {
        target: AccessibleTarget,
        value: String,
    },
    RenameCancel {
        target: AccessibleTarget,
    },
    ArchiveConfirm {
        target: AccessibleTarget,
    },
    ArchiveCancel {
        target: AccessibleTarget,
    },
    RemoveWorkspaceConfirm {
        target: AccessibleTarget,
    },
    RemoveWorkspaceCancel {
        target: AccessibleTarget,
    },
}

#[derive(Default)]
pub(super) struct WorkspaceAccessibility {
    pub(super) rendering: Vec<AccessibleRow>,
    pub(super) search_status: Option<&'static str>,
    visible: Vec<AccessibleRow>,
    surface: Option<AccessibleSurface>,
    search: Option<AccessibleSearch>,
    focused: String,
}

#[derive(PartialEq, Eq, Serialize)]
pub(super) struct AccessibleSearch {
    pub(super) draft: Option<String>,
    pub(super) results: bool,
    pub(super) status: Option<&'static str>,
}

impl WorkspaceAccessibility {
    pub(super) fn publish(
        &mut self,
        surface: Option<AccessibleSurface>,
        search: Option<AccessibleSearch>,
    ) {
        if self.rendering != self.visible || self.surface != surface || self.search != search {
            std::mem::swap(&mut self.rendering, &mut self.visible);
            self.surface = surface;
            self.search = search;
            if let (Ok(rows), Ok(surface), Ok(search)) = (
                serde_wasm_bindgen::to_value(&self.visible),
                serde_wasm_bindgen::to_value(&self.surface),
                serde_wasm_bindgen::to_value(&self.search),
            ) {
                update_workspace_tree(rows, surface, search);
            }
        }
        self.rendering.clear();
        self.search_status = None;
    }

    pub(super) fn focused(&self, key: &str) -> bool {
        self.focused == key
    }
}

impl DshApp {
    pub(super) fn accessible_workspace_surface(&self, cx: &App) -> Option<AccessibleSurface> {
        if let Some(rename) = &self.session_rename {
            Some(AccessibleSurface::Rename {
                target: AccessibleTarget::Session {
                    id: rename.id.clone(),
                },
                draft: self.session_rename_editor.read(cx).value().to_owned(),
                pending: rename.pending,
                blocked: self
                    .session_rename_editor
                    .read(cx)
                    .value()
                    .trim()
                    .is_empty(),
                error: rename.error.clone(),
            })
        } else if let Some(dialog) = self.accessible_workspace_dialog(cx) {
            Some(dialog)
        } else if let Some(archive) = self.accessible_archive_confirmation() {
            Some(archive)
        } else {
            self.accessible_workspace_menu()
                .or_else(|| self.accessible_session_menu())
        }
    }

    pub(super) fn workspace_tree_available(&self) -> bool {
        !self.show_welcome
            && !self.show_settings
            && !self.account_launcher_modal()
            && !self.desktop_onboarding_visible()
            && !self.shortcuts.open
            && self.workspace_dialog.is_none()
            && self.session_rename.is_none()
            && self.confirm_permission.is_none()
            && self.team_disable.is_none()
            && self.image_viewer.is_none()
            && self.file_open_error.is_none()
            && self.stats_dialog.is_none()
            && self.turn_usage_dialog.is_none()
            && !self.session_downloads.visible()
            && !self.session_feedback.visible()
            && !self.archive_confirmation_open()
    }

    pub(super) fn handle_workspace_tree_action(
        &mut self,
        event: &str,
        key: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event == "surface" {
            if let Ok(action) = serde_json::from_str::<SurfaceAction>(key) {
                self.handle_workspace_surface_action(action, w, cx);
            }
            return;
        }
        let visible = self.workspace_tree_available() && !self.rail_active(w);
        if event.starts_with("search-")
            && visible
            && self.accessible_workspace_surface(cx).is_none()
        {
            match event {
                "search-open" => {
                    self.open_workspace_search(w, cx);
                    return;
                }
                "search-close" => {
                    self.close_workspace_search(false, w, cx);
                    return;
                }
                "search-draft" if self.search_expanded => {
                    if self.search_editor.read(cx).value() != key {
                        replace(&self.search_editor, key, w, cx);
                        self.update_workspace_search(w, cx);
                    }
                    return;
                }
                _ => {}
            }
        }
        let row = self
            .workspace_accessibility
            .visible
            .iter()
            .find(|row| row.key == key);
        if event == "focus" {
            let focused = if visible && row.is_some() { key } else { "" };
            if self.workspace_accessibility.focused != focused {
                if let Some(row) = row.filter(|_| visible) {
                    let bounds = match &row.action {
                        RowAction::ToggleGroup { path, .. } => {
                            self.workspace_row_geometry.bounds(path).map(|mut bounds| {
                                bounds.size.height = px(32.);
                                bounds
                            })
                        }
                        RowAction::OpenSession(session) => {
                            self.session_row_geometry.bounds(&session.id)
                        }
                        RowAction::SetWatermark { .. } => None,
                    };
                    if let Some(bounds) = bounds {
                        self.scroll_sidebar_row_into_view(bounds);
                    }
                }
                self.workspace_accessibility.focused = focused.to_owned();
                cx.notify();
            }
            return;
        }
        if !visible {
            return;
        }
        if event == "unarchive" {
            if let Some(RowAction::OpenSession(session)) =
                row.filter(|row| row.unarchive).map(|row| &row.action)
            {
                self.set_session_archived(session.id.clone(), false, false, w, cx);
            }
            return;
        }
        if event == "open-menu" {
            let viewport = self.sidebar_session_scroll().bounds();
            match row.filter(|row| row.menu).map(|row| row.action.clone()) {
                Some(RowAction::OpenSession(session)) => {
                    if let Some(mut anchor) = self.session_row_geometry.bounds(&session.id) {
                        anchor.origin.y = anchor.origin.y.clamp(viewport.top(), viewport.bottom());
                        self.open_session_context(
                            &session.id,
                            SessionMenuOrigin::Row,
                            SessionMenuAnchor::Button(anchor),
                            self.session_row_focus.get(&session.id).cloned(),
                            w,
                            cx,
                        );
                    }
                }
                Some(RowAction::ToggleGroup { path, .. }) => {
                    if let Some(bounds) = self.workspace_row_geometry.bounds(&path) {
                        let y = bounds.top().clamp(viewport.top(), viewport.bottom());
                        self.open_workspace_context(&path, y, w, cx);
                    }
                }
                _ => {}
            }
            return;
        }
        if event != "activate" {
            return;
        }
        match row.map(|row| row.action.clone()) {
            Some(RowAction::ToggleGroup { path, descendants }) => {
                self.toggle_workspace_group(&path, &descendants, cx);
            }
            Some(RowAction::OpenSession(session)) => self.open_workspace_session(session, w, cx),
            Some(RowAction::SetWatermark { path, limit }) => {
                self.set_workspace_watermark(&path, limit, cx)
            }
            None => {}
        }
    }

    fn handle_workspace_surface_action(
        &mut self,
        action: SurfaceAction,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let confirm = matches!(
            &action,
            SurfaceAction::RenameConfirm { .. } | SurfaceAction::RemoveWorkspaceConfirm { .. }
        );
        match action {
            SurfaceAction::MenuItem { target, key } => {
                self.handle_accessible_menu(&target, &key, true, w, cx)
            }
            SurfaceAction::MenuFocus { target, key } => {
                self.handle_accessible_menu(&target, &key, false, w, cx)
            }
            SurfaceAction::MenuDismiss { target } => match target {
                AccessibleTarget::Session { id } if self.session_context_for(&id).is_some() => {
                    self.escape_header_menu(w, cx);
                }
                AccessibleTarget::Workspace { path, id }
                    if self.workspace_menu_matches(&path, id.as_deref()) =>
                {
                    self.dismiss_workspace_context(true, w, cx);
                }
                _ => {}
            },
            SurfaceAction::RenameDraft { target, value }
            | SurfaceAction::RenameConfirm { target, value } => {
                if let Some(editor) = self.accessible_rename_editor(&target) {
                    if editor.read(cx).value() != value {
                        replace(&editor, value, w, cx);
                    }
                    if confirm {
                        match target {
                            AccessibleTarget::Session { .. } => self.confirm_session_rename(cx),
                            AccessibleTarget::Workspace { .. } => {
                                self.confirm_workspace_rename(w, cx)
                            }
                        }
                    }
                    cx.notify();
                }
            }
            SurfaceAction::RenameCancel { target }
                if self.accessible_rename_editor(&target).is_some() =>
            {
                self.close_workspace_dialog(w, cx);
                cx.notify();
            }
            SurfaceAction::ArchiveConfirm {
                target: AccessibleTarget::Session { id },
            } => self.act_on_accessible_archive_confirmation(&id, true, w, cx),
            SurfaceAction::ArchiveCancel {
                target: AccessibleTarget::Session { id },
            } => self.act_on_accessible_archive_confirmation(&id, false, w, cx),
            SurfaceAction::RemoveWorkspaceConfirm { target }
            | SurfaceAction::RemoveWorkspaceCancel { target }
                if self.workspace_dialog_matches(&target, WorkspaceDialogAction::Remove) =>
            {
                if confirm {
                    self.confirm_workspace_remove(w, cx);
                } else {
                    self.close_workspace_dialog(w, cx);
                }
                cx.notify();
            }
            _ => {}
        }
    }

    fn handle_accessible_menu(
        &mut self,
        target: &AccessibleTarget,
        key: &str,
        activate: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match target {
            AccessibleTarget::Session { id } => {
                self.handle_accessible_session_menu(id, key, activate, w, cx)
            }
            AccessibleTarget::Workspace { path, id }
                if self.workspace_menu_matches(path, id.as_deref()) =>
            {
                self.handle_accessible_group_menu(key, activate, w, cx)
            }
            _ => {}
        }
    }

    fn accessible_rename_editor(&self, target: &AccessibleTarget) -> Option<gpui::Entity<Editor>> {
        match target {
            AccessibleTarget::Session { id }
                if self
                    .session_rename
                    .as_ref()
                    .is_some_and(|rename| rename.id == *id && !rename.pending) =>
            {
                Some(self.session_rename_editor.clone())
            }
            AccessibleTarget::Workspace { .. }
                if self.workspace_dialog_matches(target, WorkspaceDialogAction::Rename) =>
            {
                Some(self.workspace_rename_editor.clone())
            }
            _ => None,
        }
    }

    fn workspace_dialog_matches(
        &self,
        target: &AccessibleTarget,
        action: WorkspaceDialogAction,
    ) -> bool {
        let AccessibleTarget::Workspace { path, id } = target else {
            return false;
        };
        self.workspace_dialog.as_ref().is_some_and(|dialog| {
            !dialog.pending && dialog.action == action && dialog.path == *path && dialog.id == *id
        })
    }
}
