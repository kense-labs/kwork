//! Place the official Agent preset seat and read-only session label.
use super::*;
use gpui::canvas;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Layout<'a> {
    session: Option<&'a str>,
    hero: bool,
    developer_tools: bool,
    dark: bool,
    bounds: [f32; 4],
}

#[wasm_bindgen(raw_module = "../trajectory-json.js")]
extern "C" {
    #[wasm_bindgen(js_name = stageSessionPreset)]
    fn stage_preset(layout: JsValue);

    #[wasm_bindgen(js_name = presetSelectionPending)]
    pub(super) fn selection_pending(session: &str) -> bool;

    #[wasm_bindgen(js_name = stagedAgentPreset)]
    pub(super) fn staged_agent_preset() -> Option<String>;

    #[wasm_bindgen(js_name = stageCreatorAgentPreset)]
    fn stage_creator_preset();
}

impl DshApp {
    pub(super) fn agent_preset_anchor(&self, hero: bool, cx: &App) -> Option<gpui::Div> {
        if !self.connected()
            || !(self.conversation.has_agent_preset && self.conversation.acp_open
                || hero && self.active_session.is_none())
            || self.trajectory_controls_hidden()
        {
            return None;
        }
        let session = self.active_session.clone();
        let tools = self.developer_tools;
        let dark = super::workspace::dark(cx);
        Some(
            div()
                .relative()
                .w(px(if hero { 128. } else { 100. }))
                .min_w(px(54.))
                .h(px(28.))
                .child(
                    canvas(
                        move |bounds, _, _| {
                            let rect: [f32; 4] = [
                                bounds.left().into(),
                                bounds.top().into(),
                                bounds.right().into(),
                                bounds.bottom().into(),
                            ];
                            if let Ok(layout) = serde_wasm_bindgen::to_value(&Layout {
                                session: session.as_deref(),
                                hero,
                                developer_tools: tools,
                                dark,
                                bounds: rect,
                            }) {
                                stage_preset(layout);
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                ),
        )
    }

    pub(super) fn start_creator_preset(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if !self.connected() || self.creating_session {
            return;
        }
        stage_creator_preset();
        let Some(cwd) = self.new_session_workspace().map(str::to_owned) else {
            self.clear_main_session(w, cx);
            return;
        };
        self.begin_action(
            PendingAction::New {
                workspace_id: self.workspace_registry.id(&cwd).map(str::to_owned),
                cwd,
                draft: None,
                agent_preset: Some("cordis".into()),
            },
            cx,
        );
    }
}
