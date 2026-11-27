//! The first-login flight. The camera moves through a fixed field, so advancing
//! the clock, skipping frames, and resizing all yield the same view of the scene.

use gpui::{prelude::*, *};
use std::{f32::consts::TAU, sync::Arc, time::Duration};

pub(crate) const FILM_DURATION: Duration = Duration::from_millis(5_500);
const TUNNEL_EXIT: f32 = 3.65;

// Use the same publisher artwork as the runtime catalog. All marks fly in a
// light environment, including when the surrounding workbench uses a dark theme.
const MARKS: &[(&[u8], u32)] = &[
    (
        include_bytes!("../../../assets/icons/agents/claude.svg"),
        0xff865b,
    ),
    (
        include_bytes!("../../../assets/icons/agents/gemini.svg"),
        0x5794ff,
    ),
    (
        include_bytes!("../../../assets/icons/agents/codex.svg"),
        0x44c7ad,
    ),
    (
        include_bytes!("../../../assets/icons/agents/kimi.svg"),
        0x438cff,
    ),
    (
        include_bytes!("../../../assets/icons/agents/antigravity.svg"),
        0xe087ce,
    ),
    (
        include_bytes!("../../../assets/icons/agents/cursor.svg"),
        0x8e98cf,
    ),
    (
        include_bytes!("../../../assets/icons/agents/mistral-vibe.svg"),
        0xffbb41,
    ),
    (
        include_bytes!("../../../assets/icons/agents/grok.svg"),
        0x68b8d3,
    ),
];

struct FlightArtwork {
    backdrop: Arc<RenderImage>,
    marks: Vec<Option<Arc<RenderImage>>>,
}

impl Asset for FlightArtwork {
    type Source = DevicePixels;
    type Output = Arc<Self>;

    fn load(
        side: DevicePixels,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move {
            let marks = MARKS
                .iter()
                .map(|(svg, _)| {
                    let tree = renderer.parse_svg(svg).ok()?;
                    renderer
                        .render_parsed(&tree, SvgSize::Size(size(side, side)))
                        .ok()
                })
                .collect();
            // A small, smooth plate supplies the ambient coloured light. GPU sampling
            // stretches this static texture; no full-window pixels are regenerated.
            let image = image::RgbaImage::from_fn(320, 200, |x, y| {
                let u = x as f32 / 319.;
                let v = y as f32 / 199.;
                let edge = (((u - 0.5) * 1.7).powi(2) + ((v - 0.48) * 1.4).powi(2)).min(1.);
                let blue = (u * (1. - v)).sqrt();
                let rose = ((1. - u) * v).sqrt();
                image::Rgba([
                    (255. - edge * (13. + 19. * rose)) as u8,
                    (255. - edge * (28. + 23. * rose)) as u8,
                    (255. - edge * (21. + 27. * blue)) as u8,
                    255,
                ]) // RenderImage expects BGRA.
            });
            Arc::new(Self {
                backdrop: Arc::new(RenderImage::new([image::Frame::new(image)])),
                marks,
            })
        }
    }
}

fn welcome_ground(alpha: f32) -> Background {
    linear_gradient(
        155.,
        linear_color_stop(rgb(0xf0f5fa).opacity(alpha), 0.),
        linear_color_stop(rgb(0xfcfafb).opacity(alpha), 1.),
    )
}

fn ease(start: f32, end: f32, t: f32) -> f32 {
    let v = ((t - start) / (end - start)).clamp(0., 1.);
    v * v * (3. - 2. * v)
}

// Deterministic strata keep adjacent lanes apart without generating a random
// scene on each frame. Depth is measured along the camera's travel axis.
fn noise(seed: usize) -> f32 {
    let mut bits = (seed as u32).wrapping_add(1).wrapping_mul(0x9e37_79b9);
    bits ^= bits >> 16;
    bits = bits.wrapping_mul(0x85eb_ca6b);
    (bits >> 8) as f32 / 16_777_216.
}

struct Camera {
    center: Point<Pixels>,
    radius: f32,
    travel: f32,
    bend: f32,
}

impl Camera {
    fn project(&self, angle: f32, radius: f32, depth: f32) -> Point<Pixels> {
        let distance = self.radius * radius / depth;
        let (sin, cos) = angle.sin_cos();
        self.center
            + point(
                px(distance * (cos + self.bend / depth)),
                px(distance * (sin * 0.87 + self.bend * 0.2 / depth)),
            )
    }
}

// A tapered strip with a rounded leading edge reads as a light trail arriving
// from the centre. Both surfaces are drawn with the same camera as the logos.
fn streak(window: &mut Window, tail: Point<Pixels>, head: Point<Pixels>, width: f32, color: Rgba) {
    let dx = f32::from(head.x - tail.x);
    let dy = f32::from(head.y - tail.y);
    let len = dx.hypot(dy);
    if len < 1. || width < 0.25 {
        return;
    }
    let r = (width * 0.5).min(len * 0.25);
    let normal = point(px(-dy * r / len), px(dx * r / len));
    let nose = point(px(dx * r / len), px(dy * r / len));
    let mut shape = PathBuilder::fill();
    shape.move_to(tail + normal * 0.14);
    shape.line_to(head + normal);
    shape.curve_to(head - normal, head + nose * 2.);
    shape.line_to(tail - normal * 0.14);
    shape.close();
    if let Ok(path) = shape.build() {
        window.paint_path(path, color);
    }
}

fn tunnel_arcs(camera: &Camera, window: &mut Window) {
    // Fragments of the circular wall reveal its curvature between the passing
    // trails. They grow with the same projection and vanish into the white core.
    for i in 0..4 {
        let depth = 0.36 + (i as f32 * 0.91 - camera.travel).rem_euclid(3.64);
        let mut path = PathBuilder::stroke(px((1.2 / depth).clamp(0.5, 2.5)));
        for sector in 0..5 {
            let angle = (sector as f32 * 1.26) + i as f32 * 0.58;
            let span = 0.51 + noise(i + sector) * 0.24;
            let project = |a| camera.project(a, 0.91, depth);
            path.move_to(project(angle));
            for step in 1..=8 {
                path.line_to(project(angle + span * step as f32 / 8.));
            }
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, rgb(0x93a9df).opacity((0.21 / depth).min(0.28)));
        }
    }
}

fn light_field(camera: &Camera, window: &mut Window) {
    const PALETTE: [u32; 8] = [
        0x7ddbe8, 0x95a9ff, 0xffac7c, 0xd4b1fa, 0xffda6d, 0x91e9b8, 0xfb98b5, 0x84c0fa,
    ];
    for i in 0..84 {
        let angle = TAU * ((i as f32 + 0.7 * noise(i + 20)) / 84.);
        let depth = 0.24 + (noise(i + 190) * 4.4 - camera.travel).rem_euclid(4.4);
        let radius = 0.70 + noise(i + 150) * 0.86;
        let tail_depth = depth + 0.28 + 1.24 * noise(i + 78);
        let tail = camera.project(angle, radius, tail_depth);
        let head = camera.project(angle, radius, depth);
        let tint = rgb(PALETTE[i % PALETTE.len()]);
        let face_width = (1.4 + 22. * noise(i + 163).powi(2)) / depth;
        let alpha = (0.52 / depth + 0.14).min(0.86);
        streak(
            window,
            tail,
            head,
            face_width * 1.28,
            tint.opacity(alpha * 0.23),
        );
        streak(window, tail, head, face_width, tint.opacity(alpha));
        if face_width > 3. {
            // A longitudinal highlight gives the larger bands a glass face.
            streak(
                window,
                tail,
                head,
                face_width * 0.25,
                rgb(0xffffff).opacity(alpha * 0.52),
            );
        }
    }
}

fn passing_marks(camera: &Camera, art: &FlightArtwork, window: &mut Window) {
    // Equal spacing in Z bounds the number of large foreground marks. Draw
    // back to front, including the partial lap that straddles a wrap.
    const TRACK: f32 = 4.2;
    let gap = TRACK / MARKS.len() as f32;
    let first = ((camera.travel / gap).floor() as usize + 1) % MARKS.len();
    for offset in (0..MARKS.len()).rev() {
        let i = (first + offset) % MARKS.len();
        let Some(image) = &art.marks[i] else { continue };
        let depth = 0.31 + (i as f32 * gap - camera.travel).rem_euclid(TRACK);
        let angle = i as f32 * 2.399_963 + 0.38;
        let radius = 0.65 + 0.29 * noise(i + 32);
        let pos = camera.project(angle, radius, depth);
        let extent = (camera.radius * 0.162 / depth).clamp(7., 138.);
        let dims = image.size(0);
        let aspect = dims.width.0 as f32 / dims.height.0.max(1) as f32;
        let sz = size(px(extent * aspect.min(1.)), px(extent / aspect.max(1.)));
        let tail = camera.project(angle, radius, depth + 1.2);
        let lead = camera.project(angle, radius, depth + 0.09);
        let tint = rgb(MARKS[i].1);
        streak(window, tail, lead, extent * 0.23, tint.opacity(0.24));
        streak(window, tail, lead, extent * 0.07, tint.opacity(0.53));
        let bounds = Bounds::new(pos - point(sz.width / 2., sz.height / 2.), sz);
        let _ = window.paint_image(bounds, bounds, Corners::default(), image.clone(), 0, false);
    }
}

pub(crate) fn film(elapsed: Duration, reduced: bool, window: &mut Window, cx: &mut App) -> Div {
    let t = elapsed.as_secs_f32();
    let viewport = window.viewport_size();
    let width = f32::from(viewport.width);
    let title_size = (width * 0.065).clamp(30., 62.);
    let welcome = if reduced { 1. } else { ease(3.65, 4.22, t) };
    let flight = !reduced && t < TUNNEL_EXIT;
    let artwork = flight
        .then(|| {
            // Rasterize once per device scale, at the largest possible projected
            // size; changing the Z coordinate never adds an image to the cache.
            let side = DevicePixels((138. * window.scale_factor()).ceil() as i32);
            window.use_asset::<FlightArtwork>(&side, cx)
        })
        .flatten();

    div()
        .relative()
        .size_full()
        .bg(welcome_ground(1.))
        .overflow_hidden()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .when(flight, |stage| {
            stage.child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        if let Some(art) = &artwork {
                            let _ = window.paint_image(
                                bounds,
                                bounds,
                                Corners::default(),
                                art.backdrop.clone(),
                                0,
                                false,
                            );
                        }
                        let w = f32::from(bounds.size.width);
                        let h = f32::from(bounds.size.height);
                        let fly = (t - 0.24).max(0.);
                        let camera = Camera {
                            center: bounds.origin
                                + point(
                                    px(w * (0.5 + 0.035 * (fly * 1.6).sin())),
                                    px(h * (0.48 + 0.025 * (fly * 1.22).sin())),
                                ),
                            radius: w.min(h * 1.35) * 0.44,
                            travel: fly * 0.52 + fly * fly * 0.47,
                            bend: 0.042 * (fly * 1.9).sin(),
                        };
                        tunnel_arcs(&camera, window);
                        light_field(&camera, window);
                        if let Some(art) = &artwork {
                            passing_marks(&camera, art, window);
                        }
                        let exit = ease(2.94, TUNNEL_EXIT, t);
                        if exit > 0. {
                            window.paint_quad(fill(bounds, welcome_ground(exit)));
                        }
                        let ignition = 1. - ease(0.05, 0.68, t);
                        if ignition > 0. {
                            // The aperture opens onto the white field; no single-frame flash.
                            window.paint_quad(fill(bounds, rgb(0x101326).opacity(ignition * 0.97)));
                            let length = (w * 0.11 + w * ease(0., 0.55, t) * 0.32).min(450.);
                            let slit = Bounds::new(
                                camera.center - point(px(length * 0.5), px(0.7)),
                                size(px(length), px(1.4)),
                            );
                            window.paint_quad(fill(slit, rgb(0xd9f9ff).opacity(ignition)));
                        }
                    },
                )
                .absolute()
                .inset_0(),
            )
        })
        .when(reduced || t >= 3.56, |stage| {
            stage.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .opacity(welcome)
                    .text_center()
                    .child(
                        div()
                            .text_size(px((title_size * 0.43).max(17.)))
                            .font_weight(FontWeight::LIGHT)
                            .text_color(rgb(0x58667f))
                            .child("Welcome To"),
                    )
                    .child(
                        div()
                            .text_size(px(title_size))
                            .line_height(px(title_size * 1.14))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(0x15213c))
                            .child("Kense Agent Online"),
                    )
                    .child(div().mt(px(18.)).w(px(66.)).h(px(2.)).bg(linear_gradient(
                        90.,
                        linear_color_stop(rgb(0x73c6df), 0.),
                        linear_color_stop(rgb(0xc7a9ea), 1.),
                    ))),
            )
        })
}
