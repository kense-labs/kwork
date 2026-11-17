//! Official Markdown file/URL classification at the GPUI interaction boundary.

use super::*;

#[wasm_bindgen(raw_module = "../markdown-navigation.js")]
extern "C" {
    #[wasm_bindgen(js_name = markdownDestination)]
    fn destination(value: &str) -> JsValue;

    #[wasm_bindgen(js_name = markdownLinkIcon)]
    pub(super) fn link_icon(value: &str, file: bool) -> String;
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
pub(super) enum Destination {
    File { path: String, line: Option<usize> },
    Url { url: String },
}

pub(super) fn parse(value: &str) -> Option<Destination> {
    serde_wasm_bindgen::from_value(destination(value)).ok()
}
