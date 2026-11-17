//! Measured official Markdown inside the existing transcript scroll/layout owner.
use super::*;
use gpui::{
    AvailableSpace, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId,
    Pixels,
};

#[wasm_bindgen(raw_module = "../trajectory-json.js")]
extern "C" {
    #[wasm_bindgen(js_name = measureChatMarkdown)]
    fn measure(key: &str, source: &str, width: f32, dark: bool) -> f32;
    #[wasm_bindgen(js_name = stageChatMarkdown)]
    fn stage(key: &str, bounds: JsValue, viewport: JsValue, hidden: bool);
}

#[derive(Clone, Copy, Default)]
pub(super) struct Viewport {
    pub hidden: bool,
    pub composer: Option<Bounds<Pixels>>,
}

pub(super) fn markdown(
    source: &str,
    key: &str,
    compact: bool,
    options: super::workspace::MarkdownOptions<'_>,
    cx: &Context<DshApp>,
) -> gpui::Div {
    let session = options.active_session.unwrap_or("");
    let mentions: Vec<_> = options
        .mentions
        .into_iter()
        .flat_map(|vocabulary| {
            vocabulary
                .files
                .iter()
                .map(|file| json!({"authored":file.authored,"resolved":file.resolved}))
        })
        .collect();
    let images: Vec<_> = crate::markdown::image_destinations(source)
        .into_iter()
        .filter_map(|authored| {
            let path = crate::media::absolute_image_path(&authored)?;
            let crate::media::LocalImageState::Ready(image) =
                crate::media::cached_local_image(&path)?
            else {
                return None;
            };
            Some(json!({"authored":authored,"path":path,"url":image.data_url}))
        })
        .collect();
    div().w_full().min_w_0().flex_none().child(Markdown {
        key: format!("{session}:{key}"),
        source: json!({"session":session,"text":source,"streaming":!options.settled,"compact":compact,"mentions":mentions,"images":images}).to_string(),
        dark: super::workspace::dark(cx),
        viewport: options.dom.unwrap_or_default(),
    })
}

struct Markdown {
    key: String,
    source: String,
    dark: bool,
    viewport: Viewport,
}

pub(super) fn interaction(
    session: &str,
    request: &str,
    width: f32,
    payload: &Value,
    hidden: bool,
    cx: &Context<DshApp>,
) -> gpui::Div {
    div()
        .w_full()
        .max_w(px(width))
        .min_w_0()
        .flex_none()
        .child(Markdown {
            key: format!("{session}:interaction:{request}"),
            source: json!({"session":session,"interaction":payload}).to_string(),
            dark: super::workspace::dark(cx),
            viewport: Viewport {
                hidden,
                composer: None,
            },
        })
}

pub(super) fn entry_surface(
    key: &str,
    kind: &str,
    payload: &Value,
    options: super::workspace::MarkdownOptions<'_>,
    cx: &Context<DshApp>,
) -> gpui::Div {
    let session = options.active_session.unwrap_or("");
    div().w_full().min_w_0().flex_none().child(Markdown {
        key: format!("{session}:{kind}:{key}"),
        source: json!({"session":session, kind:payload}).to_string(),
        dark: super::workspace::dark(cx),
        viewport: options.dom.unwrap_or_default(),
    })
}
impl IntoElement for Markdown {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Markdown {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        w: &mut Window,
        _: &mut App,
    ) -> (LayoutId, ()) {
        let key = self.key.clone();
        let source = self.source.clone();
        let dark = self.dark;
        (
            w.request_measured_layout(Default::default(), move |known, available, w, _| {
                let width = known
                    .width
                    .or(match available.width {
                        AvailableSpace::Definite(width) => Some(width),
                        _ => None,
                    })
                    .unwrap_or(w.viewport_size().width)
                    .max(px(1.));
                gpui::size(width, px(measure(&key, &source, width.into(), dark)))
            }),
            (),
        )
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        w: &mut Window,
        _: &mut App,
    ) {
        let rect = |bounds: Bounds<Pixels>| -> [f32; 4] {
            [
                bounds.left().into(),
                bounds.top().into(),
                bounds.right().into(),
                bounds.bottom().into(),
            ]
        };
        let mut viewport = rect(w.content_mask().bounds);
        if let Some(composer) = self.viewport.composer
            && bounds.left() < composer.right()
            && bounds.right() > composer.left()
        {
            viewport[3] = viewport[3].min(composer.top().into());
        }
        if let (Ok(bounds), Ok(viewport)) = (
            serde_wasm_bindgen::to_value(&rect(bounds)),
            serde_wasm_bindgen::to_value(&viewport),
        ) {
            stage(&self.key, bounds, viewport, self.viewport.hidden);
        }
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }
}

impl DshApp {
    pub(super) fn chat_markdown_action(
        &mut self,
        value: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(value) = serde_json::from_str::<Value>(value) else {
            return;
        };
        if value["session"].as_str() != self.active_session.as_deref() {
            return;
        }
        match value["action"].as_str() {
            Some("question-focus" | "question-hold" | "question-hide") => {
                self.question_composer_action(&value, w, cx);
                return;
            }
            Some("plan-preview") => {
                self.plan_review_preview_action(&value, w, cx);
                return;
            }
            _ => {}
        }
        if self.trajectory.active != super::trajectory::ConversationView::Chat {
            return;
        }
        match value["action"].as_str().unwrap_or("") {
            "open-plan-card" => {
                if let Some(call) = value["callId"].as_str() {
                    self.open_submitted_plan(call, w, cx);
                }
            }
            "file" => {
                if let Some(path) = value["path"].as_str() {
                    let line = value["line"]
                        .as_u64()
                        .map(|line| line.min(usize::MAX as u64) as usize);
                    self.open_document_link(path, line, w, cx);
                }
            }
            "file-menu" => {
                if let Some(path) = value["path"].as_str() {
                    let line = value["line"]
                        .as_u64()
                        .map(|line| line.min(usize::MAX as u64) as usize);
                    self.open_document_link_in(
                        path,
                        line,
                        Some(value["destination"] == "float"),
                        w,
                        cx,
                    );
                }
            }
            "url-menu" => {
                if let Some(url) = value["url"].as_str() {
                    if !self.right_panel_visible() {
                        self.toggle_right_panel(w, cx);
                    }
                    let pane = self.sidebar.surface.layout.active_dock_pane();
                    self.new_browser_tab(pane, Some(url.to_owned()), w, cx);
                }
            }
            "url" => {
                if let Some(url) = value["url"].as_str() {
                    self.open_link(url, w, cx);
                }
            }
            "external" => {
                if let Some(url) = value["url"].as_str() {
                    bridge::open_url(url);
                }
            }
            "scroll" => {
                if let Some(delta) = value["delta"].as_f64().filter(|delta| delta.is_finite()) {
                    let scroll = &self.conversation.scroll;
                    let y = (f32::from(scroll.offset().y) - delta as f32)
                        .clamp(-f32::from(scroll.max_offset().y), 0.);
                    scroll.set_offset(gpui::point(scroll.offset().x, px(y)));
                    self.transcript_scrolled(-delta as f32, cx);
                }
            }
            "focus" => {
                self.composer.read(cx).focus.clone().focus(w, cx);
            }
            _ => {}
        }
    }
}
