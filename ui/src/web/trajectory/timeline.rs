//! Measured seat and actions for the official React timeline.
use super::*;
use serde::Serialize;
use std::collections::HashSet;

#[wasm_bindgen(raw_module = "../trajectory-json.js")]
extern "C" {
    #[wasm_bindgen(js_name = trajectoryTimelineHeight)]
    fn height() -> f32;
    #[wasm_bindgen(js_name = trajectoryTimelineFocus)]
    fn focus_indexes(source: &str) -> String;
    #[wasm_bindgen(js_name = stageTrajectoryTimeline)]
    fn stage(source: &str, layout: JsValue);
    #[wasm_bindgen(js_name = trajectoryTimelineHistoryStarted)]
    fn history_started(id: &str, request: f64, loading: bool);
    #[wasm_bindgen(js_name = focusTrajectorySearch)]
    pub(super) fn focus_search();
}

#[derive(Serialize)]
struct Layout {
    bounds: [f32; 4],
    viewport: [f32; 4],
    dark: bool,
    hidden: bool,
    focused: bool,
}

impl DshApp {
    pub(super) fn trajectory_timeline_source(&self, query: &str) -> String {
        let state = &self.conversation.trajectory;
        let selected = state
            .records
            .iter()
            .find(|record| Some(&record.key) == self.trajectory.selected.as_ref());
        let matches = (!query.trim().is_empty()).then(|| {
            state
                .records
                .iter()
                .filter(|record| crate::trajectory::matches(record, query))
                .map(|record| record.index)
                .collect::<Vec<_>>()
        });
        let mode = match (self.trajectory.actual_duration, self.trajectory.actual_time) {
            (false, false) => "sequence",
            (true, false) => "duration",
            (false, true) => "time",
            (true, true) => "actual",
        };
        json!({
            "session": self.active_session,
            "turns": crate::trajectory::timeline::turns(state),
            "mode": mode,
            "query": query,
            "folds": self.trajectory_fold_states(),
            "range": self.trajectory.range,
            "selectedIndex": selected.map(|record| record.index),
            "matches": matches,
            "hasEarlier": self.conversation.history_has_more,
            "historyLoading": self.conversation.history_loading,
            "historyRequest": self.conversation.history_request_id,
            "historyError": self.conversation.history_error.is_some(),
        })
        .to_string()
    }

    pub(super) fn trajectory_timeline_focus(&mut self, source: &str) {
        let indexes: Option<HashSet<usize>> =
            serde_json::from_str(&focus_indexes(source)).unwrap_or(None);
        if indexes != self.trajectory.range_indexes && indexes.is_some() {
            self.trajectory.focus_range = true;
        }
        self.trajectory.range_indexes = indexes;
    }

    pub(super) fn trajectory_timeline(
        &self,
        source: String,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let dark = super::super::workspace::dark(cx);
        let hidden = self.trajectory_controls_hidden();
        let focus = self.trajectory.timeline_focus.clone();
        div()
            .id("trajectory-timeline-seat")
            .relative()
            .w_full()
            .flex_none()
            .h(px(height()))
            .track_focus(&self.trajectory.timeline_focus)
            .child(
                canvas(
                    move |bounds, w, _| {
                        if let Ok(layout) = serde_wasm_bindgen::to_value(&Layout {
                            bounds: json::rect(bounds),
                            viewport: json::rect(w.content_mask().bounds),
                            dark,
                            hidden,
                            focused: focus.is_focused(w),
                        }) {
                            stage(&source, layout);
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
    }

    pub(super) fn trajectory_timeline_action(
        &mut self,
        value: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(data) = serde_json::from_str::<Value>(value) else {
            return;
        };
        if data["session"].as_str() != self.active_session.as_deref() {
            return;
        }
        match data["action"].as_str().unwrap_or("") {
            "query" => {
                let Some(query) = data["value"].as_str() else {
                    return;
                };
                self.trajectory.search = query.to_owned();
                self.trajectory.follow = false;
                self.trajectory.prepend_anchor = None;
                self.trajectory
                    .scroll
                    .set_offset(gpui::point(px(0.), px(0.)));
            }
            "duration" | "time" => {
                let Some(value) = data["value"].as_bool() else {
                    return;
                };
                if data["action"] == "duration" {
                    self.trajectory.actual_duration = value;
                } else {
                    self.trajectory.actual_time = value;
                }
                self.clear_trajectory_viewport(cx);
            }
            "fold" => {
                let Some(domain) = data["value"].as_str().and_then(FoldDomain::parse) else {
                    return;
                };
                self.toggle_all_trajectory_folds(domain, cx);
            }
            "range" => {
                self.trajectory.range = data["value"]["start"]
                    .as_f64()
                    .zip(data["value"]["end"].as_f64())
                    .filter(|(a, b)| a.is_finite() && b.is_finite())
                    .map(|(a, b)| (a.min(b), a.max(b)));
                self.trajectory.focus_range = self.trajectory.range.is_some();
            }
            "select" | "focus" => {
                let record = data["value"].as_u64().and_then(|index| {
                    self.conversation
                        .trajectory
                        .records
                        .iter()
                        .find(|record| record.index == index as usize)
                });
                if let Some(record) = record {
                    let key = record.key.clone();
                    self.trajectory.follow = false;
                    self.trajectory.focused_record = Some(key.clone());
                    if data["action"] == "select" {
                        self.trajectory.range = None;
                        self.trajectory.range_indexes = None;
                        self.select_trajectory_record(key, w, cx);
                    }
                }
            }
            "load" => {
                if let Some(id) = data["value"].as_str() {
                    self.request_older_history(cx);
                    history_started(
                        id,
                        self.conversation.history_request_id as f64,
                        self.conversation.history_loading,
                    );
                }
            }
            "tab" => {
                self.trajectory.timeline_focus.focus(w, cx);
                if data["value"] == true {
                    w.focus_prev(cx);
                } else {
                    w.focus_next(cx);
                }
            }
            "measure" => {}
            _ => return,
        }
        cx.notify();
    }
}
