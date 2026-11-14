//! In-window Workspace dragging over a browser-local display order.

use super::*;
use crate::right_dock::drag_threshold;
use crate::session_order::DropHalf;
use crate::workspace_tree::{
    reconcile_workspace_order, workspace_drag_order, workspace_drop_target,
};
use gpui::{
    Bounds, DispatchPhase, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, canvas,
};
use std::{cell::RefCell, rc::Rc};

pub(super) struct WorkspaceDragGesture {
    cwd: String,
    origin: Point<Pixels>,
    dragging: bool,
    over: Option<WorkspaceDrop>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkspaceDrop {
    cwd: String,
    half: DropHalf,
}

#[derive(Clone, Default)]
pub(super) struct WorkspaceRowGeometry(Rc<RefCell<HashMap<String, Bounds<Pixels>>>>);

impl WorkspaceRowGeometry {
    pub(super) fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    pub(super) fn measure(&self, cwd: String) -> gpui::Canvas<()> {
        let geometry = self.clone();
        canvas(
            move |bounds, _, _| {
                geometry.0.borrow_mut().insert(cwd.clone(), bounds);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
    }

    fn hit(&self, position: Point<Pixels>) -> Option<String> {
        self.0
            .borrow()
            .iter()
            .filter(|(_, bounds)| bounds.contains(&position))
            .min_by(|(_, a), (_, b)| {
                let area = |bounds: &Bounds<Pixels>| {
                    f32::from(bounds.size.width) * f32::from(bounds.size.height)
                };
                area(a).total_cmp(&area(b))
            })
            .map(|(cwd, _)| cwd.clone())
    }

    fn bounds(&self, cwd: &str) -> Option<Bounds<Pixels>> {
        self.0.borrow().get(cwd).copied()
    }

    fn rendered_ids(&self) -> Vec<String> {
        let mut groups = self
            .0
            .borrow()
            .iter()
            .map(|(cwd, bounds)| (cwd.clone(), bounds.origin.y, bounds.origin.x))
            .collect::<Vec<_>>();
        groups.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.2.cmp(&b.2)));
        groups.into_iter().map(|(cwd, _, _)| cwd).collect()
    }

    pub(super) fn pointer_capture(self, cx: &Context<DshApp>) -> gpui::Canvas<()> {
        let view = cx.weak_entity();
        canvas(
            |_, _, _| {},
            move |_, _, w, _| {
                let move_view = view.clone();
                let move_geometry = self.clone();
                w.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture
                        && let Some(view) = move_view.upgrade()
                        && view.read(cx).workspace_drag_gesture.is_some()
                    {
                        view.update(cx, |this, cx| {
                            this.move_workspace_drag(event, &move_geometry, cx)
                        });
                    }
                });
                w.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture
                        && event.button == MouseButton::Left
                        && let Some(view) = view.upgrade()
                        && view.read(cx).workspace_drag_gesture.is_some()
                    {
                        view.update(cx, |this, cx| {
                            this.release_workspace_drag(event.position, &self, cx)
                        });
                    }
                });
            },
        )
        .absolute()
        .inset_0()
    }
}

impl DshApp {
    fn full_workspace_order(&self) -> Vec<String> {
        let mut known = Vec::new();
        for cwd in self
            .sessions
            .iter()
            .map(|session| &session.cwd)
            .chain(self.added_workspaces.iter())
        {
            if !cwd.is_empty() && !known.contains(cwd) {
                known.push(cwd.clone());
            }
        }
        reconcile_workspace_order(&self.workspace_order, &known)
    }

    pub(super) fn press_workspace_row(
        &mut self,
        cwd: &str,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        if cwd.is_empty()
            || !self.search_editor.read(cx).value().trim().is_empty()
            || super::session_context_menu::mac_control_click(event.modifiers)
        {
            return;
        }
        self.workspace_drag_gesture = Some(WorkspaceDragGesture {
            cwd: cwd.to_owned(),
            origin: event.position,
            dragging: false,
            over: None,
        });
    }

    fn workspace_drop_at(
        &self,
        position: Point<Pixels>,
        geometry: &WorkspaceRowGeometry,
        gesture: &WorkspaceDragGesture,
    ) -> Option<WorkspaceDrop> {
        let hit = geometry.hit(position)?;
        let order = self.full_workspace_order();
        let hierarchy = geometry.rendered_ids();
        let nested = self.workspace_group_by == WorkspaceGroupBy::WorkspaceTree;
        let cwd = workspace_drop_target(&order, &hierarchy, &gesture.cwd, &hit, nested)?;
        let bounds = geometry.bounds(&cwd)?;
        Some(WorkspaceDrop {
            cwd,
            half: if position.y < bounds.center().y {
                DropHalf::Before
            } else {
                DropHalf::After
            },
        })
    }

    fn move_workspace_drag(
        &mut self,
        event: &MouseMoveEvent,
        geometry: &WorkspaceRowGeometry,
        cx: &mut Context<Self>,
    ) {
        if !event.dragging() {
            self.cancel_workspace_drag(cx);
            return;
        }
        let Some(gesture) = self.workspace_drag_gesture.as_ref() else {
            return;
        };
        let delta = event.position - gesture.origin;
        let starting = !gesture.dragging;
        if starting && !drag_threshold(f32::from(delta.x), f32::from(delta.y)) {
            return;
        }
        let over = self.workspace_drop_at(event.position, geometry, gesture);
        let gesture = self
            .workspace_drag_gesture
            .as_mut()
            .expect("gesture still exists");
        gesture.dragging = true;
        if starting || gesture.over != over {
            gesture.over = over;
            cx.notify();
        }
        cx.stop_propagation();
    }

    fn release_workspace_drag(
        &mut self,
        position: Point<Pixels>,
        geometry: &WorkspaceRowGeometry,
        cx: &mut Context<Self>,
    ) {
        let Some(gesture) = self
            .workspace_drag_gesture
            .take()
            .filter(|gesture| gesture.dragging)
        else {
            return;
        };
        let over = self
            .workspace_drop_at(position, geometry, &gesture)
            .or(gesture.over);
        if let Some(over) = over {
            let order = self.full_workspace_order();
            let hierarchy = geometry.rendered_ids();
            let nested = self.workspace_group_by == WorkspaceGroupBy::WorkspaceTree;
            if let Some(next) = workspace_drag_order(
                &order,
                &hierarchy,
                &gesture.cwd,
                &over.cwd,
                over.half == DropHalf::After,
                nested,
            ) {
                self.workspace_order = next;
                self.save_workspace_preferences(cx);
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn cancel_workspace_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(gesture) = self.workspace_drag_gesture.take() else {
            return false;
        };
        if gesture.dragging {
            cx.notify();
        }
        true
    }

    pub(super) fn workspace_drop_half(&self, cwd: &str) -> Option<DropHalf> {
        let gesture = self.workspace_drag_gesture.as_ref()?;
        if !gesture.dragging || gesture.over.as_ref()?.cwd != cwd {
            return None;
        }
        Some(gesture.over.as_ref()?.half)
    }

    pub(super) fn is_dragged_workspace(&self, cwd: &str) -> bool {
        self.workspace_drag_gesture
            .as_ref()
            .is_some_and(|gesture| gesture.dragging && gesture.cwd == cwd)
    }
}
