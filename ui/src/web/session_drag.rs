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

    pub(super) fn bounds(&self, id: &str) -> Option<Bounds<Pixels>> {
        self.0.borrow().get(id).copied()
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
    pub(super) fn session_recency(&self, session: &SessionSummary) -> u64 {
        self.session_activity
            .get(&session.id)
            .and_then(|activity| activity.last_prompt_at_ms)
            .max(session.created_at_ms)
            .unwrap_or(0)
    }

    pub(super) fn workspace_account_key<'a>(&'a self, session: &'a SessionSummary) -> &'a str {
        let path = if self.workspace_registry.available() {
            self.workspace_registry
                .session_path(&session.id)
                .unwrap_or("")
        } else {
            session.cwd.as_str()
        };
        if self.workspace_group_hidden(path) {
            ""
        } else {
            path
        }
    }

    fn session_account_key<'a>(&'a self, session: &'a SessionSummary) -> &'a str {
        match self.workspace_group_by {
            WorkspaceGroupBy::Flat => FLAT_SESSION_ORDER_KEY,
            _ => self.workspace_account_key(session),
        }
    }

    fn account_keys(&self) -> Vec<String> {
        let mut keys = vec![FLAT_SESSION_ORDER_KEY.to_owned()];
        for session in &self.sessions {
            let key = self.workspace_account_key(session).to_owned();
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys
    }

    fn ordered_account_ids(&self, key: &str, manual: bool) -> Vec<String> {
        let members = self
            .sessions
            .iter()
            .filter(|session| {
                key == FLAT_SESSION_ORDER_KEY || self.workspace_account_key(session) == key
            })
            .cloned()
            .collect::<Vec<_>>();
        ordered_session_ids(
            &members,
            if manual {
                Some(
                    self.session_order_by_account
                        .get(key)
                        .map_or(&[], Vec::as_slice),
                )
            } else {
                None
            },
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
                (key == FLAT_SESSION_ORDER_KEY)
                    == (self.workspace_group_by == WorkspaceGroupBy::Flat)
            })
            .flat_map(|key| self.ordered_account_ids(&key, true))
            .enumerate()
            .map(|(index, id)| (id, index))
            .collect()
    }

    fn current_session_orders(&self, manual: bool) -> HashMap<String, Vec<String>> {
        self.account_keys()
            .into_iter()
            .map(|key| {
                let order = self.ordered_account_ids(&key, manual);
                (key, order)
            })
            .collect()
    }

    pub(super) fn set_workspace_group_by(&mut self, group: WorkspaceGroupBy) {
        if (self.workspace_group_by == WorkspaceGroupBy::Flat) != (group == WorkspaceGroupBy::Flat)
        {
            self.workspace_watermarks.clear();
        }
        self.workspace_group_by = group;
    }

    pub(super) fn set_workspace_order_by(&mut self, order: WorkspaceOrderBy) {
        if self.workspace_order_by == order {
            return;
        }
        self.session_order_by_account = if order == WorkspaceOrderBy::Manual {
            self.current_session_orders(false)
        } else {
            HashMap::new()
        };
        self.workspace_order_by = order;
    }

    pub(super) fn sync_session_pin_order(&mut self, id: &str) {
        let Some(account) = self
            .sessions
            .iter()
            .find(|session| session.id == id)
            .map(|session| self.workspace_account_key(session).to_owned())
        else {
            return;
        };
        for key in [account, FLAT_SESSION_ORDER_KEY.to_owned()] {
            let order = self
                .session_order_by_account
                .remove(&key)
                .unwrap_or_else(|| self.ordered_account_ids(&key, true));
            self.session_order_by_account
                .insert(key, move_session_to_front(order, id));
        }
    }

    pub(super) fn reserve_scratch_order(&mut self, id: &str) -> bool {
        if self.workspace_order_by != WorkspaceOrderBy::Manual {
            return false;
        }
        let Some(account) = self
            .sessions
            .iter()
            .find(|session| session.id == id && session.blank)
            .map(|session| self.workspace_account_key(session).to_owned())
        else {
            return false;
        };
        let mut changed = false;
        for key in [account, FLAT_SESSION_ORDER_KEY.to_owned()] {
            if self
                .session_order_by_account
                .get(&key)
                .and_then(|order| order.first())
                .is_some_and(|first| first == id)
            {
                continue;
            }
            let order = self
                .session_order_by_account
                .remove(&key)
                .unwrap_or_else(|| self.ordered_account_ids(&key, true));
            self.session_order_by_account
                .insert(key, move_session_to_front(order, id));
            changed = true;
        }
        changed
    }

    pub(super) fn session_is_pinned(&self, id: &str) -> bool {
        self.pinned_sessions
            .iter()
            .any(|session_id| session_id == id)
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
            account_key: self.session_account_key(session).to_owned(),
            session_id: session.id.clone(),
            pinned: self.session_is_pinned(&session.id),
            origin: event.position,
            dragging: false,
            over: None,
        });
    }

    fn session_drop_target(
        &self,
        mut over: SessionDrop,
        gesture: &SessionDragGesture,
    ) -> Option<SessionDrop> {
        let session = self.sessions.iter().find(|session| session.id == over.id)?;
        let pinned =
            !self.archived_sessions.contains(&session.id) && self.session_is_pinned(&session.id);
        if self.session_account_key(session) != gesture.account_key || pinned != gesture.pinned {
            return None;
        }
        if session.blank {
            over.half = DropHalf::After;
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
        let over = match geometry.hit(event.position) {
            Some(hit) => self.session_drop_target(hit, gesture),
            None => gesture.over.clone(),
        };
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
                    && self.session_is_pinned(&session.id),
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
        let over = match geometry.hit(position) {
            Some(hit) => self.session_drop_target(hit, &gesture),
            None => gesture.over.clone(),
        };
        if let Some(over) = over {
            let manual = self.workspace_order_by == WorkspaceOrderBy::Manual;
            let mut orders = self.current_session_orders(manual);
            let rows = self.rendered_session_rows(&gesture.account_key, geometry);
            if let Some(order) = orders.get(&gesture.account_key)
                && let Some(next) =
                    session_drag_order(order, &rows, &gesture.session_id, gesture.pinned, &over)
            {
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
