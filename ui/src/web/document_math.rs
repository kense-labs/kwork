//! Local TeX typesetting and measured vector formulae for GPUI Markdown.

use super::*;
use serde::Deserialize;

#[wasm_bindgen(raw_module = "../document-math.js")]
extern "C" {
    #[wasm_bindgen(js_name = renderMath)]
    fn render_math(source: &str, display: bool, font_size: f32) -> JsValue;
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MathImage {
    svg: String,
    width: f32,
    height: f32,
    baseline_offset: f32,
}

pub(super) struct MathCacheEntry {
    source: String,
    display: bool,
    font_size: f32,
    last_used_ms: f64,
    image: Option<MathImage>,
}

pub(super) fn key(source: &str, index: usize) -> String {
    let mut fingerprint = 0xcbf29ce484222325u64;
    for byte in source.bytes() {
        fingerprint = (fingerprint ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    format!("math-{fingerprint:x}-{index}")
}

/// Settled TeX only: incomplete streaming source is never sent to the parser.
pub(super) fn typeset(
    cache: &std::cell::RefCell<HashMap<String, MathCacheEntry>>,
    key: &str,
    source: &str,
    display: bool,
    font_size: f32,
    settled: bool,
) -> Option<MathImage> {
    if !settled {
        return None;
    }
    let now = bridge::monotonic_ms();
    let mut cache = cache.borrow_mut();
    if cache.len() > 256 {
        cache.retain(|_, entry| now - entry.last_used_ms < 30_000.);
    }
    let entry = cache.entry(key.to_owned()).or_insert_with(|| {
        let value = render_math(source, display, font_size);
        MathCacheEntry {
            source: source.to_owned(),
            display,
            font_size,
            last_used_ms: now,
            image: serde_wasm_bindgen::from_value(value).ok(),
        }
    });
    if entry.source != source || entry.display != display || entry.font_size != font_size {
        entry.source = source.to_owned();
        entry.display = display;
        entry.font_size = font_size;
        entry.image = serde_wasm_bindgen::from_value(render_math(source, display, font_size)).ok();
    }
    entry.last_used_ms = now;
    entry.image.clone()
}

impl MathImage {
    pub(super) fn render(&self, color: gpui::Hsla) -> gpui::Svg {
        gpui::svg()
            .data(self.svg.as_bytes())
            .w(px(self.width))
            .h(px(self.height))
            .flex_none()
            .text_color(color)
    }

    pub(super) fn baseline(&self) -> f32 {
        self.height - self.baseline_offset
    }
}
