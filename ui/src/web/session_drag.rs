//! In-window Session dragging over browser-local, per-account orders.

use super::*;
use crate::right_dock::drag_threshold;
use crate::session_order::{
    DropHalf, FLAT_SESSION_ORDER_KEY, SessionDrop, SessionRow, move_session_to_front,
    ordered_session_ids, session_drag_order, smallest_row_at,
};
use gpui::{
    Bounds, DispatchPhase, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, canvas,
};
use std::{cell::RefCell, rc::Rc};

pub(super) struct SessionDragGesture {
    account_key: String,
    session_id: String,
    pinned: bool,
    origin: Point<Pixels>,
    dragging: bool,
    over: Option<SessionDrop>,
}

#[derive(Clone, Default)]
pub(super) struct SessionRowGeometry(Rc<RefCell<HashMap<String, Bounds<Pixels>>>>);

impl SessionRowGeometry {
    pub(super) fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    pub(super) fn measure(&self, id: String) -> gpui::Canvas<()> {
        let geometry = self.clone();
        canvas(
            move |bounds, _, _| {
                geometry.0.borrow_mut().insert(id.clone(), bounds);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
    }

    fn hit(&self, position: Point<Pixels>) -> Option<SessionDrop> {
        let geometry = self.0.borrow();
        let (id, bounds) = smallest_row_at(geometry.iter(), position)?;
        Some(SessionDrop {
            id: id.clone(),
            half: if position.y < bounds.center().y {
                DropHalf::Before
            } else {
                DropHalf::After
            },
        })
    }

    fn rendered_ids(&self) -> Vec<String> {
        let mut rows = self
            .0
            .borrow()
            .iter()
            .map(|(id, bounds)| (id.clone(), bounds.origin.y, bounds.origin.x))
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.2.cmp(&b.2)));
        rows.into_iter().map(|(id, _, _)| id).collect()
    }

    pub(super) fn pointer_capture(self, cx: &Context<DshApp>) -> gpui::Canvas<()> {
        let view = cx.weak_entity();
        canvas(
            |_, _, _| {},
            move |_, _, w, _| {
                let move_view = view.clone();
                let move_geometry = self.clone();
                w.on_mouse_event(move |event: &MouseMoveEvent, phase, w, cx| {
                    if phase == DispatchPhase::Capture
                        && let Some(view) = move_view.upgrade()
                        && view.read(cx).session_drag_gesture.is_some()
                    {
                        let event = event.clone();
                        let geometry = move_geometry.clone();
                        w.defer(cx, move |_, cx| {
                            view.update(cx, |this, cx| {
                                this.move_session_drag(&event, &geometry, cx)
                            });
                        });
                    }
                });
                w.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture
                        && event.button == MouseButton::Left
                        && let Some(view) = view.upgrade()
                        && view.read(cx).session_drag_gesture.is_some()
                    {
                        view.update(cx, |this, cx| {
                            this.release_session_drag(event.position, &self, cx)
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
    fn session_recency(&self, session: &SessionSummary) -> u64 {
        self.session_activity
            .get(&session.id)
            .and_then(|activity| activity.last_prompt_at_ms)
            .max(session.created_at_ms)
            .unwrap_or(0)
    }

    fn workspace_account_key(&self, session: &SessionSummary) -> String {
        if self.hidden_workspaces.contains(&session.cwd) {
            String::new()
        } else {
            session.cwd.clone()
        }
    }

    fn session_account_key(&self, session: &SessionSummary) -> String {
        match self.workspace_group_by {
            WorkspaceGroupBy::Flat => FLAT_SESSION_ORDER_KEY.to_owned(),
            _ => self.workspace_account_key(session),
        }
    }

    fn account_keys(&self) -> Vec<String> {
        let mut keys = vec![FLAT_SESSION_ORDER_KEY.to_owned()];
        for session in &self.sessions {
            let key = self.workspace_account_key(session);
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys
    }

    fn ordered_account_ids(&self, key: &str) -> Vec<String> {
        let members = self
            .sessions
            .iter()
            .filter(|session| self.session_account_key(session) == key)
            .cloned()
            .collect::<Vec<_>>();
        ordered_session_ids(
            &members,
            self.session_order_by_account.get(key).map(Vec::as_slice),
            self.active_session.as_deref(),
            &self.pinned_sessions,
            &self.archived_sessions,
            |session| self.session_recency(session),
        )
    }

    /// Global ranks only order members within their account; Workspace ordering is separate.
    pub(super) fn manual_session_ranks(&self) -> HashMap<String, usize> {
        self.account_keys()
            .into_iter()
            .filter(|key| {
                key != FLAT_SESSION_ORDER_KEY || self.workspace_group_by == WorkspaceGroupBy::Flat
            })
            .flat_map(|key| self.ordered_account_ids(&key))
            .enumerate()
            .map(|(index, id)| (id, index))
            .collect()
    }

    fn initial_session_orders(&self) -> HashMap<String, Vec<String>> {
        self.account_keys()
            .into_iter()
            .map(|key| {
                let order = self.ordered_account_ids(&key);
                (key, order)
            })
            .collect()
    }

    pub(super) fn set_workspace_group_by(&mut self, group: WorkspaceGroupBy) {
        if self.workspace_group_by == group {
            return;
        }
        self.workspace_group_by = group;
        if group == WorkspaceGroupBy::Flat && self.workspace_order_by == WorkspaceOrderBy::Manual {
            let visible = self
                .session_row_geometry
                .rendered_ids()
                .into_iter()
                .filter_map(|id| {
                    self.sessions
                        .iter()
                        .find(|session| session.id == id)
                        .cloned()
                })
                .filter(|session| !self.hidden_workspaces.contains(&session.cwd))
                .collect::<Vec<_>>();
            let mut order = self
                .full_workspace_order()
                .into_iter()
                .flat_map(|cwd| self.ordered_account_ids(&cwd))
                .collect::<Vec<_>>();
            if visible.is_empty() {
                order = self.ordered_account_ids(FLAT_SESSION_ORDER_KEY);
            } else {
                let visible_ids = visible
                    .iter()
                    .map(|session| session.id.as_str())
                    .collect::<std::collections::HashSet<_>>();
                order.retain(|id| !visible_ids.contains(id.as_str()));
                order.splice(0..0, visible.into_iter().map(|session| session.id));
            }
            self.session_order_by_account
                .insert(FLAT_SESSION_ORDER_KEY.to_owned(), order);
        }
    }

    pub(super) fn set_workspace_order_by(&mut self, order: WorkspaceOrderBy) {
        if self.workspace_order_by == order {
            return;
        }
        self.session_order_by_account = if order == WorkspaceOrderBy::Manual {
            self.initial_session_orders()
        } else {
            HashMap::new()
        };
        self.workspace_order_by = order;
    }

    pub(super) fn sync_session_pin_order(&mut self, id: &str) {
        if self.workspace_order_by != WorkspaceOrderBy::Manual || !self.pinned_sessions.contains(id)
        {
            return;
        }
        let Some(account) = self
            .sessions
            .iter()
            .find(|session| session.id == id)
            .map(|session| self.workspace_account_key(session))
        else {
            return;
        };
        let mut orders = self.initial_session_orders();
        for key in [account, FLAT_SESSION_ORDER_KEY.to_owned()] {
            if let Some(order) = orders.get_mut(&key) {
                *order = move_session_to_front(std::mem::take(order), id);
            }
        }
        self.session_order_by_account = orders;
    }

    pub(super) fn press_session_row(
        &mut self,
        session: &SessionSummary,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        if !self.search_editor.read(cx).value().trim().is_empty()
            || session.blank
            || self.archived_sessions.contains(&session.id)
            || super::session_context_menu::mac_control_click(event.modifiers)
        {
            return;
        }
        self.session_drag_gesture = Some(SessionDragGesture {
            account_key: self.session_account_key(session),
            session_id: session.id.clone(),
            pinned: self.pinned_sessions.contains(&session.id),
            origin: event.position,
            dragging: false,
            over: None,
        });
    }

    fn session_drop_at(
        &self,
        position: Point<Pixels>,
        geometry: &SessionRowGeometry,
        gesture: &SessionDragGesture,
    ) -> Option<SessionDrop> {
        let over = geometry.hit(position)?;
        let session = self.sessions.iter().find(|session| session.id == over.id)?;
        if session.blank
            || self.archived_sessions.contains(&session.id)
            || self.session_account_key(session) != gesture.account_key
            || self.pinned_sessions.contains(&session.id) != gesture.pinned
        {
            return None;
        }
        Some(over)
    }

    fn move_session_drag(
        &mut self,
        event: &MouseMoveEvent,
        geometry: &SessionRowGeometry,
        cx: &mut Context<Self>,
    ) {
        if !event.dragging() {
            self.cancel_session_drag(cx);
            return;
        }
        let Some(gesture) = self.session_drag_gesture.as_ref() else {
            return;
        };
        let delta = event.position - gesture.origin;
        let starting = !gesture.dragging;
        if starting && !drag_threshold(f32::from(delta.x), f32::from(delta.y)) {
            return;
        }
        let over = self.session_drop_at(event.position, geometry, gesture);
        let gesture = self
            .session_drag_gesture
            .as_mut()
            .expect("gesture still exists");
        gesture.dragging = true;
        if starting || gesture.over != over {
            gesture.over = over;
            cx.notify();
        }
        cx.stop_propagation();
    }

    fn rendered_session_rows(
        &self,
        account_key: &str,
        geometry: &SessionRowGeometry,
    ) -> Vec<SessionRow> {
        geometry
            .rendered_ids()
            .into_iter()
            .filter_map(|id| self.sessions.iter().find(|session| session.id == id))
            .filter(|session| self.session_account_key(session) == account_key)
            .map(|session| SessionRow {
                id: session.id.clone(),
                pinned: !self.archived_sessions.contains(&session.id)
                    && self.pinned_sessions.contains(&session.id),
                blank: session.blank,
                archived: self.archived_sessions.contains(&session.id),
            })
            .collect()
    }

    fn release_session_drag(
        &mut self,
        position: Point<Pixels>,
        geometry: &SessionRowGeometry,
        cx: &mut Context<Self>,
    ) {
        let Some(gesture) = self
            .session_drag_gesture
            .take()
            .filter(|gesture| gesture.dragging)
        else {
            return;
        };
        let over = self
            .session_drop_at(position, geometry, &gesture)
            .or_else(|| gesture.over.clone());
        if let Some(over) = over {
            let order = self.ordered_account_ids(&gesture.account_key);
            let rows = self.rendered_session_rows(&gesture.account_key, geometry);
            if let Some(next) =
                session_drag_order(&order, &rows, &gesture.session_id, gesture.pinned, &over)
            {
                let mut orders = self.session_order_by_account.clone();
                orders.insert(gesture.account_key, next);
                self.session_order_by_account = orders;
                self.workspace_order_by = WorkspaceOrderBy::Manual;
                self.save_workspace_preferences(cx);
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn cancel_session_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(gesture) = self.session_drag_gesture.take() else {
            return false;
        };
        if gesture.dragging {
            cx.notify();
        }
        true
    }

    pub(super) fn session_drop_half(&self, id: &str) -> Option<DropHalf> {
        let gesture = self.session_drag_gesture.as_ref()?;
        if !gesture.dragging || gesture.over.as_ref()?.id != id {
            return None;
        }
        Some(gesture.over.as_ref()?.half)
    }

    pub(super) fn is_dragged_session(&self, id: &str) -> bool {
        self.session_drag_gesture
            .as_ref()
            .is_some_and(|gesture| gesture.dragging && gesture.session_id == id)
    }
}
