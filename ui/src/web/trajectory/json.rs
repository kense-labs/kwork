//! Measured upstream JSON trees placed within the GPUI inspector's clip bounds.
use super::*;
use serde::Serialize;

#[wasm_bindgen(raw_module = "../trajectory-json.js")]
extern "C" {
    #[wasm_bindgen(js_name = beginTrajectoryJsonFrames)]
    pub(crate) fn begin_frames();
    #[wasm_bindgen(js_name = listenTrajectoryJson)]
    pub(crate) fn listen(callback: &js_sys::Function);
    #[wasm_bindgen(js_name = trajectoryJsonHeight)]
    fn height(key: &str) -> f32;
    #[wasm_bindgen(js_name = stageTrajectoryJson)]
    fn stage(key: &str, source: &str, layout: JsValue);
    #[wasm_bindgen(js_name = focusTrajectoryJson)]
    pub(super) fn focus(backward: bool) -> bool;
    #[wasm_bindgen(js_name = focusTrajectorySeparator)]
    pub(super) fn focus_separator() -> bool;
    #[wasm_bindgen(js_name = stageTrajectorySeparator)]
    fn stage_separator(bounds: JsValue, split: f32, width: f32, hidden: bool);
}

#[derive(Serialize)]
struct Layout {
    bounds: [f32; 4],
    viewport: [f32; 4],
    dark: bool,
    preview: bool,
    hidden: bool,
    kind: String,
}
pub(super) fn rect(bounds: gpui::Bounds<gpui::Pixels>) -> [f32; 4] {
    [
        bounds.left().into(),
        bounds.top().into(),
        bounds.right().into(),
        bounds.bottom().into(),
    ]
}
impl DshApp {
    pub(in crate::web) fn trajectory_controls_hidden(&self) -> bool {
        !self.workspace_tree_available()
            || self.session_downloads.visible()
            || self.session_feedback.visible()
            || self.shortcuts.open
            || self.sidebar_drawer_covers_window()
            || self.workspace_dialog.is_some()
            || self.session_rename.is_some()
            || self.confirm_permission.is_some()
            || self.drop_active
    }
    pub(super) fn trajectory_json(
        &self,
        record: &Record,
        source: &str,
        suffix: &str,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let key = format!(
            "{}:{}:{:?}:{suffix}",
            self.active_session.as_deref().unwrap_or(""),
            record.key,
            self.trajectory.detail_tab
        );
        let source = source.to_owned();
        let kind = suffix.to_owned();
        let preview = self.trajectory.detail_tab == DetailTab::Overview;
        let dark = super::super::workspace::dark(cx);
        let hidden = self.trajectory_controls_hidden();
        let composer = self.composer_seat_bounds.get();
        div()
            .relative()
            .w_full()
            .flex_none()
            .h(px(height(&key)))
            .child(
                canvas(
                    move |bounds, w, _| {
                        let mut viewport = rect(w.content_mask().bounds);
                        if let Some(composer) = composer
                            && bounds.left() < composer.right()
                            && bounds.right() > composer.left()
                        {
                            viewport[3] = viewport[3].min(composer.top().into());
                        }
                        if let Ok(layout) = serde_wasm_bindgen::to_value(&Layout {
                            bounds: rect(bounds),
                            viewport,
                            dark,
                            preview,
                            hidden,
                            kind: kind.clone(),
                        }) {
                            stage(&key, &source, layout);
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
    }
    pub(super) fn trajectory_resize_handle(&self, narrow: bool, width: f32) -> gpui::Canvas<()> {
        let hidden = narrow || self.trajectory_controls_hidden();
        let split = self.trajectory.split_width;
        canvas(
            move |bounds, _, _| {
                if let Ok(bounds) = serde_wasm_bindgen::to_value(&rect(bounds)) {
                    stage_separator(bounds, split, width, hidden);
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
    }
    pub(crate) fn trajectory_json_action(
        &mut self,
        action: &str,
        value: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action == "chat-markdown" {
            self.chat_markdown_action(value, w, cx);
            return;
        }
        if action == "chat-markdown-measure" {
            cx.notify();
            return;
        }
        if matches!(action, "accessible" | "accessible-tab") {
            self.trajectory_accessibility_action(action, value, w, cx);
            return;
        }
        if self.trajectory.active != ConversationView::Trajectory {
            return;
        }
        if action == "timeline" {
            self.trajectory_timeline_action(value, w, cx);
            return;
        }
        if self.trajectory.selected.is_none() {
            return;
        }
        match action {
            "program-open" => match value {
                "Input" => self.activate_trajectory_detail(DetailTab::Input, cx),
                "Output" => self.activate_trajectory_detail(DetailTab::Output, cx),
                _ => {}
            },
            "measure" => cx.notify(),
            "close" => self.close_trajectory_inspector(w, cx),
            "resize" => {
                if value == "reset" {
                    self.trajectory.details_width = None;
                    self.trajectory.tool_request_offset = None;
                } else if let Some(width) = value
                    .parse::<f32>()
                    .ok()
                    .filter(|width| width.is_finite())
                    .map(|width| {
                        width
                            .clamp(320., (self.trajectory.split_width - 280.).clamp(320., 720.))
                            .round()
                    })
                {
                    let split = self.trajectory.split_width;
                    let old_width = self
                        .trajectory
                        .details_width
                        .unwrap_or((split * 0.38).clamp(320., 440.))
                        .min((split - 280.).max(0.));
                    let old_offset = self
                        .trajectory
                        .tool_request_offset
                        .unwrap_or(split * 0.58 - (split * 0.36 - 56.).clamp(180., 480.));
                    self.trajectory.tool_request_offset =
                        Some(old_offset + (width - old_width) * 0.58);
                    self.trajectory.details_width = Some(width);
                }
                cx.notify();
            }
            "scroll" => {
                if let Ok(delta) = value.parse::<f32>()
                    && delta.is_finite()
                {
                    let scroll = &self.trajectory.inspector_scroll;
                    let y = (f32::from(scroll.offset().y) - delta)
                        .clamp(-f32::from(scroll.max_offset().y), 0.);
                    scroll.set_offset(gpui::point(px(0.), px(y)));
                    cx.notify();
                }
            }
            "tab-record" => {
                if let Some(key) = &self.trajectory.selected
                    && let Some(focus) = self.trajectory.record_focus.borrow().get(key)
                {
                    focus.focus(w, cx);
                }
            }
            "tab-header" => self.trajectory.close_focus.focus(w, cx),
            "tab-before" | "tab-after" => {
                if let Some(record) = self
                    .conversation
                    .trajectory
                    .records
                    .iter()
                    .chain(&self.conversation.trajectory.requests)
                    .find(|record| Some(&record.key) == self.trajectory.selected.as_ref())
                {
                    let request = self
                        .conversation
                        .trajectory
                        .requests
                        .iter()
                        .any(|item| item.key == record.key);
                    if let Some(tab) = record.detail_tabs(request).last()
                        && let Some(focus) = self.trajectory.detail_tab_focus.borrow().get(tab)
                    {
                        focus.focus(w, cx);
                    }
                }
                if action == "tab-after" {
                    w.focus_next(cx);
                }
            }
            _ => {}
        }
    }
}
