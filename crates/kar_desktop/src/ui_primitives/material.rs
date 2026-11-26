//! Window glass for navigation chrome, with a denser finish for text over the conversation.
use super::StudioPalette;
use gpui::{prelude::*, *};
use std::sync::OnceLock;

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

pub(crate) fn frosted_panel(s: &StudioPalette) -> Div {
    surface(s, true)
}

pub(crate) fn frosted_composer(s: &StudioPalette) -> Div {
    surface(s, false)
}

fn surface(s: &StudioPalette, floating: bool) -> Div {
    let top = if s.dark {
        s.card.blend(white().opacity(0.14))
    } else {
        s.card.blend(white().opacity(0.50))
    };
    let bottom = if s.dark {
        s.card.blend(white().opacity(0.09))
    } else {
        s.card.blend(s.ink.opacity(0.032))
    };
    // Keep glyphs behind a popup from showing through as a second line of text.
    let density = if reduce_transparency() {
        1.
    } else if floating {
        0.98
    } else {
        0.95
    };
    let (alpha, travel, blur, spread) = match (floating, s.dark) {
        (true, true) => (0.34, 6., 20., -4.),
        (true, false) => (0.105, 6., 20., -4.),
        (false, true) => (0.18, 1., 3., 0.),
        (false, false) => (0.04, 1., 3., 0.),
    };
    let mut shadows = vec![BoxShadow {
        color: black().opacity(alpha),
        offset: point(px(0.), px(travel)),
        blur_radius: px(blur),
        spread_radius: px(spread),
        inset: false,
    }];
    shadows.push(BoxShadow {
        color: white().opacity(if s.dark { 0.065 } else { 0.68 }),
        offset: point(px(0.), px(1.)),
        blur_radius: px(0.),
        spread_radius: px(0.),
        inset: true,
    });
    div()
        .rounded(px(if floating { 14. } else { 16. }))
        .bg(linear_gradient(
            170.,
            linear_color_stop(top.opacity(density), 0.),
            linear_color_stop(bottom.opacity(density), 1.),
        ))
        .border_1()
        .border_color(s.ink.opacity(if s.dark { 0.14 } else { 0.13 }))
        .shadow(shadows)
}
