//! Native accessibility actions for controls painted by GPUI.
use super::*;
use serde::{Deserialize, Serialize};

#[wasm_bindgen(raw_module = "../trajectory-json.js")]
extern "C" {
    #[wasm_bindgen(js_name = stageTrajectoryAccessibility)]
    fn stage(key: &str, layout: JsValue);
}

#[derive(Serialize)]
struct Layout {
    bounds: [f32; 4],
    viewport: [f32; 4],
    action: String,
    value: String,
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expanded: Option<bool>,
    hidden: bool,
}

#[derive(Deserialize)]
struct Action {
    action: String,
    value: String,
    #[serde(default)]
    backward: bool,
}

impl DshApp {
    pub(super) fn trajectory_action_focus(
        &self,
        action: &str,
        value: &str,
        cx: &mut Context<Self>,
    ) -> gpui::FocusHandle {
        let key = if action == "record" {
            value.to_owned()
        } else {
            format!("a11y:{action}:{value}")
        };
        self.trajectory
            .record_focus
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone()
    }
    pub(super) fn trajectory_accessible(
        &self,
        action: &str,
        value: impl Into<String>,
        label: impl Into<String>,
        selected: Option<bool>,
        expanded: Option<bool>,
        clip_behind_inspector: bool,
    ) -> gpui::Canvas<()> {
        let action = action.to_owned();
        let value = value.into();
        let label = label.into();
        let key = format!(
            "{}:{action}:{value}",
            self.active_session.as_deref().unwrap_or("")
        );
        let hidden = self.trajectory_controls_hidden();
        let inspector_width = (self.trajectory.split_width * 0.92).min(420.);
        let dock_width = self.right_dock_frame.track;
        let composer = self.composer_seat_bounds.get();
        canvas(
            move |bounds, w, _| {
                let mut viewport = json::rect(w.content_mask().bounds);
                // The upstream narrow inspector is nonmodal. Keep the
                // exposed portion of the ledger reachable, like pointer input.
                if clip_behind_inspector {
                    viewport[2] = viewport[2]
                        .min(f32::from(w.viewport_size().width) - dock_width - inspector_width);
                }
                if let Some(composer) = composer
                    && bounds.left() < composer.right()
                    && bounds.right() > composer.left()
                {
                    viewport[3] = viewport[3].min(composer.top().into());
                }
                if let Ok(layout) = serde_wasm_bindgen::to_value(&Layout {
                    bounds: json::rect(bounds),
                    viewport,
                    action: action.clone(),
                    value: value.clone(),
                    label: label.clone(),
                    selected,
                    expanded,
                    hidden,
                }) {
                    stage(&key, layout);
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
    }

    pub(super) fn trajectory_accessibility_action(
        &mut self,
        event: &str,
        value: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.trajectory_controls_hidden() {
            return;
        }
        let Ok(action) = serde_json::from_str::<Action>(value) else {
            return;
        };
        if action.action == "view" {
            if event == "accessible-tab" {
                self.trajectory_action_focus("view", &action.value, cx)
                    .focus(w, cx);
                if action.backward {
                    w.focus_prev(cx);
                } else {
                    w.focus_next(cx);
                }
                return;
            }
            match action.value.as_str() {
                "chat" => self.open_conversation_view(ConversationView::Chat, w, cx),
                "trajectory" => self.open_conversation_view(ConversationView::Trajectory, w, cx),
                _ => {}
            }
            return;
        }
        if self.trajectory.active != ConversationView::Trajectory {
            return;
        }
        let tabs = self.trajectory.selected.as_ref().and_then(|key| {
            let request = self
                .conversation
                .trajectory
                .requests
                .iter()
                .any(|record| &record.key == key);
            self.conversation
                .trajectory
                .records
                .iter()
                .chain(&self.conversation.trajectory.requests)
                .find(|record| &record.key == key)
                .map(|record| record.detail_tabs(request))
        });
        let tab = tabs.as_ref().and_then(|tabs| {
            tabs.iter()
                .copied()
                .find(|tab| format!("{tab:?}") == action.value)
        });
        if event == "accessible-tab" {
            let focus = match action.action.as_str() {
                "record" => self
                    .trajectory
                    .record_focus
                    .borrow()
                    .get(&action.value)
                    .cloned(),
                "detail-tab" => {
                    tab.and_then(|tab| self.trajectory.detail_tab_focus.borrow().get(&tab).cloned())
                }
                "close" => Some(self.trajectory.close_focus.clone()),
                "fold-turn" | "fold-calls" | "fold-subcalls" => {
                    let domain = match action.action.as_str() {
                        "fold-turn" => FoldDomain::Turn,
                        "fold-calls" => FoldDomain::Calls,
                        _ => FoldDomain::Subcalls,
                    };
                    self.trajectory
                        .record_focus
                        .borrow()
                        .get(&format!("summary:{domain:?}:{}", action.value))
                        .cloned()
                }
                _ => Some(self.trajectory_action_focus(&action.action, &action.value, cx)),
            };
            if let Some(focus) = focus {
                focus.focus(w, cx);
            }
            if action.action == "detail-tab"
                && !action.backward
                && tab.is_some()
                && tab.as_ref() == tabs.as_ref().and_then(|tabs| tabs.last())
                && json::focus(false)
            {
                return;
            }
            if action.backward {
                w.focus_prev(cx);
            } else {
                w.focus_next(cx);
            }
            return;
        }
        match action.action.as_str() {
            "record"
                if self
                    .conversation
                    .trajectory
                    .records
                    .iter()
                    .chain(&self.conversation.trajectory.requests)
                    .any(|record| record.key == action.value) =>
            {
                self.select_trajectory_record(action.value, w, cx);
            }
            "fold-turn" => self.toggle_trajectory_fold(&action.value, FoldDomain::Turn, cx),
            "fold-calls" => self.toggle_trajectory_fold(&action.value, FoldDomain::Calls, cx),
            "fold-subcalls" => self.toggle_trajectory_fold(&action.value, FoldDomain::Subcalls, cx),
            "hierarchy" => self.open_trajectory_summary(&action.value, w, cx),
            "detail-tab" | "section" => {
                if let Some(tab) = tab {
                    if let Some(focus) = self.trajectory.detail_tab_focus.borrow().get(&tab) {
                        focus.focus(w, cx);
                    }
                    self.activate_trajectory_detail(tab, cx);
                }
            }
            "close" => self.close_trajectory_inspector(w, cx),
            "thinking" => {
                self.toggle_trajectory_thinking(cx);
            }
            "timestamp" => {
                self.trajectory.unix_timestamp = !self.trajectory.unix_timestamp;
                cx.notify();
            }
            _ => {}
        }
    }
}
