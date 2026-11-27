//! Window glass for navigation chrome, with a denser finish for text over the conversation.
use super::{R_CARD, StudioPalette};
use gpui::{prelude::*, *};
use std::sync::{Arc, OnceLock};

fn reduce_transparency() -> bool {
    static REDUCED: OnceLock<bool> = OnceLock::new();
    *REDUCED.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            objc2_app_kit::NSWorkspace::sharedWorkspace()
                .accessibilityDisplayShouldReduceTransparency()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    })
}

pub(crate) fn native_glass() -> bool {
    cfg!(target_os = "macos") && !reduce_transparency()
}

// GPUI Kit lays down its theme background at the root, beneath the workbench. Only
// that layer needs to clear for the window manager to composite the blurred desktop.
pub(crate) fn init_window_glass(cx: &mut App) {
    if native_glass() {
        gpui_kit::base::Root::register_plugin::<GlassBackdrop>(cx, |_, _| GlassBackdrop);
    }
}

struct GlassBackdrop;

impl gpui_kit::base::RootPlugin for GlassBackdrop {
    fn style(&self, root: &mut Stateful<Div>, _: &mut Window, _: &mut App) {
        root.style().background = Some(transparent_black().into());
    }
}

impl Render for GlassBackdrop {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

pub(crate) fn window_tint(s: &StudioPalette) -> Hsla {
    if !native_glass() {
        return s.stage;
    }
    if s.dark {
        s.stage.blend(white().opacity(0.055)).opacity(0.84)
    } else {
        s.stage.opacity(0.90)
    }
}

/// Tint the macOS window blur once underneath the transcript and its controls.
pub(crate) fn reading_surface(s: &StudioPalette) -> Hsla {
    if native_glass() {
        s.surface.opacity(if s.dark { 0.56 } else { 0.64 })
    } else {
        s.surface
    }
}

pub(crate) fn frosted_card(s: &StudioPalette) -> Div {
    frost(s, R_CARD, 0.063, 0.76).when(native_glass(), |card| card.child(frost_grain(R_CARD)))
}

pub(crate) fn frosted_composer(s: &StudioPalette) -> Div {
    frost(s, 16., 0.092, 0.87).when(native_glass(), |card| card.child(frost_grain(16.)))
}

pub(crate) fn frosted_panel(s: &StudioPalette) -> Div {
    // Menus can cover glyphs. Keep their coat dense enough to read over a transcript.
    frost(s, 14., 0.108, 0.98).shadow(s.popover_shadow())
}

fn frost(s: &StudioPalette, radius: f32, lift: f32, density: f32) -> Div {
    let tint = if s.dark {
        s.card.blend(s.ink.opacity(lift))
    } else {
        s.card.blend(white().opacity(0.40))
    };
    let opacity = if native_glass() { density } else { 1. };
    div()
        .rounded(px(radius))
        .bg(tint.opacity(opacity))
        .border_1()
        .border_color(s.ink.opacity(if s.dark { 0.11 } else { 0.10 }))
}

// A fixed, low-contrast grain gives even a plain backdrop a little texture.
// It sits inside the rim, underneath the controls, and never needs an animation frame.
fn frost_grain(radius: f32) -> impl IntoElement {
    static GRAIN: OnceLock<Arc<RenderImage>> = OnceLock::new();
    let grain = GRAIN.get_or_init(|| {
        let mut seed = 0x41c6_ce57_u32;
        let pixels = image::RgbaImage::from_fn(512, 384, |_, _| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let grey = (seed >> 24) as u8;
            image::Rgba([grey, grey, grey, 255])
        });
        Arc::new(RenderImage::new([image::Frame::new(pixels)]))
    });
    let texture = canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let _ = window.paint_image(
                bounds,
                bounds,
                px(radius - 1.).into(),
                grain.clone(),
                0,
                false,
            );
        },
    );
    div()
        .absolute()
        .inset(px(1.))
        .overflow_hidden()
        .opacity(0.026)
        .child(texture.absolute().inset_0())
}
