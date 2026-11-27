//! Sword Art Online-inspired FullDive launch sequence ("Link Start -> Welcome To Kense Agent Online").
//!
//! All four acts are pure deterministic functions of elapsed film time `t`:
//! 1. `0.00s .. 0.92s` — NerveGear optical visor ignition, concentric wireframe calibration
//!    reticles, and 5-channel sensory/neural `OK` handshake.
//! 2. `0.84s .. 3.70s` — High-contrast prismatic 3D curved wormhole with 210 curved neon
//!    light ribbons, cylindrical vortex rings, dynamic singularity bloom, and 14 sculpted
//!    holographic crystal Agent Medallions flying past.
//! 3. `3.36s .. 3.76s` — Event horizon breakthrough whiteout into the infinite calibration room.
//! 4. `3.68s .. 6.20s` — Canonical SAO "Welcome To Kense Agent Online" geometric lockup with
//!    horizon calibration grid and razor-tapered sword-blade divider.

use gpui::{prelude::*, *};
use std::{f32::consts::TAU, sync::Arc, time::Duration};

use crate::ui_primitives::{mono_font, tnum_features, ui_font};

pub(crate) const FILM_DURATION: Duration = Duration::from_millis(6_200);
const TUNNEL_START: f32 = 0.80;
const TUNNEL_EXIT: f32 = 3.76;
const THROAT_DEPTH: f32 = 4.9;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkKind {
    Symbol,
    CompactSymbol,
    FullTile,
}

struct AgentSpec {
    svg: &'static [u8],
    /// Aura & slipstream accent in `0xRRGGBB`.
    aura: u32,
    /// Optional hex replacement for SVGs authored with `fill="currentColor"`.
    ink_hex: Option<&'static str>,
    kind: MarkKind,
}

const MARKS: &[AgentSpec] = &[
    AgentSpec {
        svg: include_bytes!("../../../assets/icons/agents/claude.svg"),
        aura: 0xff7b54,
        ink_hex: None,
        kind: MarkKind::Symbol,
    },
    AgentSpec {
        svg: include_bytes!("../../../assets/icons/agents/gemini.svg"),
        aura: 0x4d96ff,
        ink_hex: None,
        kind: MarkKind::FullTile,
    },
    AgentSpec {
        svg: include_bytes!("../../../assets/icons/agents/codex.svg"),
        aura: 0x19d399,
        ink_hex: Some("#0d2620"),
        kind: MarkKind::Symbol,
    },
    AgentSpec {
        svg: include_bytes!("../../../assets/icons/agents/antigravity.svg"),
        aura: 0x5f8cff,
        ink_hex: None,
        kind: MarkKind::Symbol,
    },
    AgentSpec {
        svg: include_bytes!("../../../assets/icons/agents/cursor.svg"),
        aura: 0x8b95ff,
        ink_hex: Some("#141729"),
        kind: MarkKind::Symbol,
    },
    AgentSpec {
        svg: include_bytes!("../../../assets/icons/agents/dsh.svg"),
        aura: 0x4d7cfe,
        ink_hex: Some("#2752eb"),
        kind: MarkKind::Symbol,
    },
    AgentSpec {
        svg: include_bytes!("../../../assets/icons/agents/kimi.svg"),
        aura: 0x38a3ff,
        ink_hex: Some("#10213d"),
        kind: MarkKind::Symbol,
    },
    AgentSpec {
        svg: include_bytes!("../../../assets/icons/agents/grok.svg"),
        aura: 0x4de1ff,
        ink_hex: Some("#0f172a"),
        kind: MarkKind::Symbol,
    },
    AgentSpec {
        svg: include_bytes!("../../../assets/icons/agents/devin.svg"),
        aura: 0x2dd4bf,
        ink_hex: Some("#0c2b30"),
        kind: MarkKind::Symbol,
    },
    AgentSpec {
        svg: include_bytes!("../../../assets/icons/agents/mistral-vibe.svg"),
        aura: 0xffa62b,
        ink_hex: Some("#e04f16"),
        kind: MarkKind::Symbol,
    },
    AgentSpec {
        svg: include_bytes!("../../../assets/icons/agents/kiro.svg"),
        aura: 0xa855f7,
        ink_hex: None,
        kind: MarkKind::FullTile,
    },
    AgentSpec {
        svg: include_bytes!("../../../assets/icons/agents/opencode-light.svg"),
        aura: 0x7dd3fc,
        ink_hex: None,
        kind: MarkKind::CompactSymbol,
    },
    AgentSpec {
        svg: include_bytes!("../../../assets/icons/agents/qoder.svg"),
        aura: 0x22d3ee,
        ink_hex: Some("#0d3648"),
        kind: MarkKind::Symbol,
    },
    AgentSpec {
        svg: include_bytes!("../../../assets/icons/agents/factory.svg"),
        aura: 0xfb923c,
        ink_hex: Some("#1c1917"),
        kind: MarkKind::Symbol,
    },
];

pub(crate) struct FlightArtwork {
    wormhole_backdrop: Arc<RenderImage>,
    core_bloom: Arc<RenderImage>,
    room_bloom: Arc<RenderImage>,
    marks: Vec<Option<Arc<RenderImage>>>,
}

fn sao_display_font(cx: &App) -> SharedString {
    let installed = kantor_ui::fonts::installed(cx);
    for candidate in ["Avenir Next", "Futura", "Segoe UI", "Inter"] {
        if installed.iter().any(|name| name == candidate) {
            return SharedString::from(candidate);
        }
    }
    ui_font(cx)
}

fn unpack_rgb(hex: u32) -> [f32; 3] {
    [
        ((hex >> 16) & 0xff) as f32,
        ((hex >> 8) & 0xff) as f32,
        (hex & 0xff) as f32,
    ]
}

fn bgra_pixel(r: f32, g: f32, b: f32, a: f32) -> image::Rgba<u8> {
    let byte = |v: f32| v.round().clamp(0., 255.) as u8;
    // GPUI's atlas stores straight BGRA.
    image::Rgba([byte(b), byte(g), byte(r), byte(a)])
}

fn prepare_svg_bytes(spec: &AgentSpec) -> Vec<u8> {
    let Ok(raw) = std::str::from_utf8(spec.svg) else {
        return spec.svg.to_vec();
    };
    let mut text = raw
        .replace(r#"width="16""#, r#"width="256""#)
        .replace(r#"height="16""#, r#"height="256""#);
    if let Some(hex) = spec.ink_hex {
        text = text.replace("currentColor", hex);
    }
    text.into_bytes()
}

fn compose_crystal_medallion(
    renderer: &SvgRenderer,
    spec: &AgentSpec,
    total_px: u32,
) -> Option<Arc<RenderImage>> {
    let total = total_px.max(64);
    // Leave a generous 23% circular margin around the squircle so the radial brand aura
    // decays strictly to 0 inside an inscribed circle—never touching atlas tile borders.
    let body_r = total as f32 * 0.27;
    let glyph_target = match spec.kind {
        MarkKind::Symbol => (body_r * 1.18).round().max(24.) as i32,
        MarkKind::CompactSymbol => (body_r * 0.96).round().max(20.) as i32,
        MarkKind::FullTile => (body_r * 1.86).round().max(32.) as i32,
    };

    let svg_bytes = prepare_svg_bytes(spec);
    let tree = renderer.parse_svg(&svg_bytes).ok()?;
    let raster = renderer
        .render_parsed(
            &tree,
            SvgSize::Size(size(DevicePixels(glyph_target), DevicePixels(glyph_target))),
        )
        .ok()?;

    let g_size = raster.size(0);
    let gw = g_size.width.0.max(1) as u32;
    let gh = g_size.height.0.max(1) as u32;
    let g_bytes = raster.as_bytes(0)?;
    let ox = (total.saturating_sub(gw)) / 2;
    let oy = (total.saturating_sub(gh)) / 2;

    let [ar, ag, ab] = unpack_rgb(spec.aura);
    let half = (total - 1) as f32 * 0.5;
    let aura_sigma = total as f32 * 0.105;

    let canvas = image::RgbaImage::from_fn(total, total, |x, y| {
        // Explicit 2-pixel transparent guard border prevents sprite-atlas bilinear bleed.
        if x <= 1 || y <= 1 || x + 2 >= total || y + 2 >= total {
            return image::Rgba([0, 0, 0, 0]);
        }
        let dx = x as f32 - half;
        let dy = y as f32 - half;
        let radial = dx.hypot(dy) / half.max(1.);
        if radial >= 0.93 {
            return image::Rgba([0, 0, 0, 0]);
        }

        // Super-ellipse squircle signed distance (`p = 4.2`).
        let r_sq = (dx.abs().powf(4.2) + dy.abs().powf(4.2)).powf(1. / 4.2);
        let s_dist = r_sq - body_r;

        // Smooth circular aura falloff hitting exact 0 at radial = 0.93.
        let edge_taper = ((0.93 - radial) / 0.36).clamp(0., 1.).powi(3);
        let aura_dist = s_dist.max(0.);
        let aura_a = 0.60 * (-(aura_dist / aura_sigma).powi(2)).exp() * edge_taper;

        let mut out_r = ar * 0.86 + 35.;
        let mut out_g = ag * 0.86 + 35.;
        let mut out_b = ab * 0.86 + 35.;
        let mut out_a = aura_a;

        // Anti-aliased frosted crystal glass body.
        let body_cov = (0.75 - s_dist).clamp(0., 1.);
        if body_cov > 0. {
            let u = (dx / body_r.max(1.)).clamp(-1., 1.);
            let v = (dy / body_r.max(1.)).clamp(-1., 1.);
            let diag = (u * 0.58 + v * 0.81) * 0.5 + 0.5;

            let mut cr = 252. - diag * 14. + (ar - 200.) * 0.08;
            let mut cg = 254. - diag * 11. + (ag - 200.) * 0.08;
            let mut cb = 255. - diag * 5. + (ab - 200.) * 0.09;
            let mut ca = 0.97;

            // Upper crystal specular reflection band.
            let sheen = ((0.22 - (u * 0.45 + v * 0.9)) * 2.4).clamp(0., 1.) * 0.28;
            cr += (255. - cr) * sheen;
            cg += (255. - cg) * sheen;
            cb += (255. - cb) * sheen;

            // Composite the publisher SVG artwork in the center.
            if x >= ox && x < ox + gw && y >= oy && y < oy + gh {
                let gi = (((y - oy) * gw + (x - ox)) * 4) as usize;
                if gi + 3 < g_bytes.len() {
                    let sb = f32::from(g_bytes[gi]);
                    let sg = f32::from(g_bytes[gi + 1]);
                    let sr = f32::from(g_bytes[gi + 2]);
                    let mut sa = f32::from(g_bytes[gi + 3]) / 255.;
                    if spec.kind == MarkKind::FullTile {
                        let inner_mask = (0.75 - (r_sq - body_r * 0.89)).clamp(0., 1.);
                        sa *= inner_mask;
                    }
                    if sa > 0. {
                        let comb_a = sa + ca * (1. - sa);
                        cr = (sr * sa + cr * ca * (1. - sa)) / comb_a.max(1e-4);
                        cg = (sg * sa + cg * ca * (1. - sa)) / comb_a.max(1e-4);
                        cb = (sb * sa + cb * ca * (1. - sa)) / comb_a.max(1e-4);
                        ca = comb_a;
                    }
                }
            }

            // Faceted specular crystal bezel rim (white-cyan top-left -> brand glow bottom-right).
            let rim = (1. - ((s_dist + 1.15).abs() / 1.45)).clamp(0., 1.);
            if rim > 0. {
                let rr = 255. * (1. - diag * 0.55) + ar * (diag * 0.55);
                let rg = 255. * (1. - diag * 0.55) + ag * (diag * 0.55);
                let rb = 255. * (1. - diag * 0.55) + ab * (diag * 0.55);
                let ra = rim * 0.88;
                cr = cr * (1. - ra) + rr * ra;
                cg = cg * (1. - ra) + rg * ra;
                cb = cb * (1. - ra) + rb * ra;
            }

            let src_a = ca * body_cov;
            let final_a = src_a + out_a * (1. - src_a);
            if final_a > 1e-4 {
                out_r = (cr * src_a + out_r * out_a * (1. - src_a)) / final_a;
                out_g = (cg * src_a + out_g * out_a * (1. - src_a)) / final_a;
                out_b = (cb * src_a + out_b * out_a * (1. - src_a)) / final_a;
                out_a = final_a;
            }
        }

        bgra_pixel(out_r, out_g, out_b, out_a * 255.)
    });

    Some(Arc::new(RenderImage::new([image::Frame::new(canvas)])))
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
            let total_px = side.0.max(96) as u32;
            let marks = MARKS
                .iter()
                .map(|spec| compose_crystal_medallion(&renderer, spec, total_px))
                .collect();

            // Deep celestial midnight-cobalt & aurora-indigo hyperspace atmosphere.
            let wormhole_backdrop = image::RgbaImage::from_fn(384, 240, |x, y| {
                let u = x as f32 / 383. - 0.5;
                let v = y as f32 / 239. - 0.5;
                let dist2 = (u * 1.62).powi(2) + (v * 1.42).powi(2);
                let center_lift = (-dist2 * 2.1).exp();
                let aurora_cyan = ((u + 0.5) * (0.5 - v)).max(0.).sqrt();
                let aurora_violet = ((0.5 - u) * (v + 0.5)).max(0.).sqrt();

                let r = 5. + 24. * center_lift + 20. * aurora_violet;
                let g = 10. + 56. * center_lift + 22. * aurora_cyan;
                let b = 30. + 114. * center_lift + 32. * aurora_cyan + 24. * aurora_violet;
                bgra_pixel(r, g, b, 255.)
            });

            // High-dynamic-range singularity bloom sprite with 2px transparent atlas guard border.
            let core_bloom = image::RgbaImage::from_fn(224, 224, |x, y| {
                if x <= 1 || y <= 1 || x >= 222 || y >= 222 {
                    return image::Rgba([0, 0, 0, 0]);
                }
                let nx = (x as f32 - 111.5) / 109.5;
                let ny = (y as f32 - 111.5) / 109.5;
                let d = nx.hypot(ny);
                if d >= 1. {
                    return image::Rgba([0, 0, 0, 0]);
                }
                let taper = (1. - d).powi(3);
                let hot = (-d * d * 15.).exp();
                let cyan = (-d * d * 4.6).exp();
                let outer = (1. - d).powi(3);

                let a = (hot + 0.82 * cyan + 0.44 * outer).min(1.) * taper;
                let r = (255. * hot + 95. * cyan + 55. * outer).min(255.);
                let g = (255. * hot + 238. * cyan + 145. * outer).min(255.);
                let b = (255. * (hot + cyan + outer * 0.95)).min(255.);
                bgra_pixel(r, g, b, a * 255.)
            });

            // Iridescent cyan-violet ambient bloom for the SAO White Calibration Room.
            let room_bloom = image::RgbaImage::from_fn(192, 192, |x, y| {
                if x <= 1 || y <= 1 || x >= 190 || y >= 190 {
                    return image::Rgba([0, 0, 0, 0]);
                }
                let nx = (x as f32 - 95.5) / 93.5;
                let ny = (y as f32 - 95.5) / 93.5;
                let d = nx.hypot(ny);
                if d >= 1. {
                    return image::Rgba([0, 0, 0, 0]);
                }
                let a = (-d * d * 3.8).exp() * (1. - d).powi(3) * 0.28;
                let mix = (nx * 0.5 + 0.5).clamp(0., 1.);
                let r = 120. * (1. - mix) + 186. * mix;
                let g = 212. * (1. - mix) + 168. * mix;
                let b = 250.;
                bgra_pixel(r, g, b, a * 255.)
            });

            Arc::new(Self {
                wormhole_backdrop: Arc::new(RenderImage::new([image::Frame::new(
                    wormhole_backdrop,
                )])),
                core_bloom: Arc::new(RenderImage::new([image::Frame::new(core_bloom)])),
                room_bloom: Arc::new(RenderImage::new([image::Frame::new(room_bloom)])),
                marks,
            })
        }
    }
}

pub(crate) fn welcome_ground(alpha: f32) -> Background {
    linear_gradient(
        162.,
        linear_color_stop(rgb(0xf2f7fd).opacity(alpha), 0.),
        linear_color_stop(rgb(0xfdfdff).opacity(alpha), 1.),
    )
}

fn ease(start: f32, end: f32, t: f32) -> f32 {
    let v = ((t - start) / (end - start)).clamp(0., 1.);
    v * v * (3. - 2. * v)
}

fn ease_out_cubic(start: f32, end: f32, t: f32) -> f32 {
    let v = ((t - start) / (end - start)).clamp(0., 1.);
    1. - (1. - v).powi(3)
}

fn noise(seed: usize) -> f32 {
    let mut bits = (seed as u32).wrapping_add(1).wrapping_mul(0x9e37_79b9);
    bits ^= bits >> 16;
    bits = bits.wrapping_mul(0x85eb_ca6b);
    (bits >> 8) as f32 / 16_777_216.
}

struct Camera {
    vanishing: Point<Pixels>,
    radius: f32,
    travel: f32,
    bend_x: f32,
    bend_y: f32,
    roll: f32,
}

impl Camera {
    fn spine(&self, depth: f32) -> Point<Pixels> {
        self.vanishing
            + point(
                px(self.radius * self.bend_x * depth),
                px(self.radius * self.bend_y * depth),
            )
    }

    fn project(&self, angle: f32, radius: f32, depth: f32) -> Point<Pixels> {
        let distance = self.radius * radius / depth.max(0.06);
        let theta = angle + self.roll - 0.095 * depth;
        let (sin, cos) = theta.sin_cos();
        self.spine(depth) + point(px(distance * cos), px(distance * sin * 0.90))
    }
}

fn curved_ribbon(
    window: &mut Window,
    tail: Point<Pixels>,
    mid: Point<Pixels>,
    head: Point<Pixels>,
    tail_width: f32,
    head_width: f32,
    color: Rgba,
) {
    let dx = f32::from(head.x - tail.x);
    let dy = f32::from(head.y - tail.y);
    let len = dx.hypot(dy);
    if len < 1.2 || head_width < 0.25 {
        return;
    }

    let normal_at = |a: Point<Pixels>, b: Point<Pixels>, half_w: f32| -> Point<Pixels> {
        let vx = f32::from(b.x - a.x);
        let vy = f32::from(b.y - a.y);
        let d = vx.hypot(vy).max(0.001);
        point(px(-vy * half_w / d), px(vx * half_w / d))
    };

    let hr = (head_width * 0.5).min(len * 0.26);
    let tr = (tail_width * 0.5).min(hr);
    let mr = (hr + tr) * 0.5;

    let n_tail = normal_at(tail, mid, tr);
    let n_mid = normal_at(tail, head, mr);
    let n_head = normal_at(mid, head, hr);

    let hdx = f32::from(head.x - mid.x);
    let hdy = f32::from(head.y - mid.y);
    let hlen = hdx.hypot(hdy).max(0.001);
    let nose = point(px(hdx * hr * 1.4 / hlen), px(hdy * hr * 1.4 / hlen));

    let ctrl_base = point(
        mid.x * 2. - (tail.x + head.x) * 0.5,
        mid.y * 2. - (tail.y + head.y) * 0.5,
    );

    let mut shape = PathBuilder::fill();
    shape.move_to(tail + n_tail);
    shape.curve_to(head + n_head, ctrl_base + n_mid);
    shape.curve_to(head - n_head, head + nose);
    shape.curve_to(tail - n_tail, ctrl_base - n_mid);
    shape.close();
    if let Ok(path) = shape.build() {
        window.paint_path(path, color);
    }
}

fn paint_sprite_centered(
    window: &mut Window,
    image: &Arc<RenderImage>,
    center: Point<Pixels>,
    diameter: f32,
) {
    if diameter <= 2. {
        return;
    }
    let sz = size(px(diameter), px(diameter));
    let bounds = Bounds::new(center - point(sz.width * 0.5, sz.height * 0.5), sz);
    let bleed = px((diameter * 0.03).clamp(1.5, 6.));
    let _ = window.paint_image(
        bounds,
        bounds.dilate(bleed),
        Corners::default(),
        image.clone(),
        0,
        false,
    );
}

fn paint_tunnel_rings(camera: &Camera, window: &mut Window) {
    for i in 0..12 {
        let depth = 0.20 + (i as f32 * 0.39 - camera.travel * 0.88).rem_euclid(4.68);
        let stroke_w = (1.65 / depth).clamp(0.55, 2.7);
        let mut path = PathBuilder::stroke(px(stroke_w));
        let sectors = 6;
        for sector in 0..sectors {
            let base = (sector as f32 * TAU / sectors as f32) + i as f32 * 0.44;
            let span = 0.58 + noise(i * 11 + sector) * 0.25;
            path.move_to(camera.project(base, 0.96, depth));
            for step in 1..=8 {
                let a = base + span * (step as f32 / 8.);
                path.line_to(camera.project(a, 0.96, depth));
            }
        }
        let fade_far = (1. - (depth - 3.6) / 1.0).clamp(0., 1.);
        let alpha = ((0.32 / depth).min(0.38)) * fade_far;
        let ring_color = if i % 2 == 0 {
            rgb(0x38bdf8)
        } else {
            rgb(0x818cf8)
        };
        if alpha > 0.01
            && let Ok(built) = path.build()
        {
            window.paint_path(built, ring_color.opacity(alpha));
        }
    }
}

fn paint_prismatic_ribbons(camera: &Camera, window: &mut Window) {
    const PALETTE: [u32; 8] = [
        0x00f0ff, 0x18f2a3, 0x4d8eff, 0xb26eff, 0xff5492, 0xffcf4d, 0x38bdf8, 0x7ef9ff,
    ];

    for i in 0..210 {
        let angle = TAU * ((i as f32 + 0.78 * noise(i + 23)) / 210.);
        let track = 4.75;
        let speed_mul = match i % 5 {
            0 => 1.22,
            2 => 0.92,
            _ => 1.04,
        };
        let depth = 0.16 + (noise(i + 190) * track - camera.travel * speed_mul).rem_euclid(track);
        let radius = if i % 6 == 0 {
            0.34 + noise(i + 71) * 0.38
        } else {
            0.72 + noise(i + 150) * 0.68
        };
        let length = 0.42 + 1.65 * noise(i + 78);
        let tail_depth = (depth + length).min(THROAT_DEPTH);
        if tail_depth <= depth + 0.04 {
            continue;
        }
        let mid_depth = (depth + tail_depth) * 0.5;

        let tail = camera.project(angle, radius, tail_depth);
        let mid = camera.project(angle, radius, mid_depth);
        let head = camera.project(angle, radius, depth);

        let tint = rgb(PALETTE[i % PALETTE.len()]);
        let base_w = 1.2 + 19. * noise(i + 163).powi(2);
        let head_w = (base_w / depth).min(42.);
        let tail_w = (base_w * 0.15 / tail_depth).max(0.35);

        let throat_fade = ((THROAT_DEPTH - depth) / 1.05).clamp(0., 1.);
        let alpha = ((0.66 / depth + 0.16).min(0.92)) * throat_fade;

        curved_ribbon(
            window,
            tail,
            mid,
            head,
            tail_w * 1.6,
            head_w * 1.95,
            tint.opacity(alpha * 0.21),
        );
        curved_ribbon(
            window,
            tail,
            mid,
            head,
            tail_w,
            head_w,
            tint.opacity(alpha * 0.85),
        );
        if head_w > 2.4 {
            curved_ribbon(
                window,
                tail,
                mid,
                head,
                tail_w * 0.3,
                head_w * 0.32,
                rgb(0xf4feff).opacity(alpha * 0.80),
            );
        }
    }
}

fn paint_passing_agents(
    camera: &Camera,
    art: &FlightArtwork,
    depth_range: std::ops::Range<f32>,
    window: &mut Window,
) {
    const TRACK: f32 = 4.75;
    let count = MARKS.len();
    let gap = TRACK / count as f32;

    let mut order: Vec<(usize, f32)> = (0..count)
        .map(|i| {
            let depth = 0.22 + (i as f32 * gap - camera.travel).rem_euclid(TRACK);
            (i, depth)
        })
        .filter(|(_, d)| depth_range.contains(d))
        .collect();
    order.sort_by(|a, b| b.1.total_cmp(&a.1));

    for (i, depth) in order {
        let Some(medallion) = &art.marks[i] else {
            continue;
        };
        // Golden-angle spiral + stratified radial lanes keep all 14 marks cleanly separated.
        let angle = i as f32 * 2.399_963 + 0.42;
        let radius = 0.48 + 0.30 * (((i * 5 + 3) % count) as f32 / (count - 1) as f32);
        let pos = camera.project(angle, radius, depth);

        let emerge = ((TRACK + 0.15 - depth) / 0.95).clamp(0., 1.);
        let sprite_diam = (camera.radius * 0.305 / depth).clamp(10., 228.) * (0.45 + 0.55 * emerge);
        let crystal_w = sprite_diam * 0.54;
        let mark_alpha = emerge * ((depth - 0.20) / 0.12).clamp(0.35, 1.);

        let wake_tail_z = (depth + 1.40).min(THROAT_DEPTH);
        let wake_mid_z = (depth + wake_tail_z) * 0.5;
        let wake_head_z = depth + 0.05;
        if wake_tail_z > wake_head_z + 0.06 {
            let tail = camera.project(angle, radius, wake_tail_z);
            let mid = camera.project(angle, radius, wake_mid_z);
            let lead = camera.project(angle, radius, wake_head_z);
            let tint = rgb(MARKS[i].aura);
            curved_ribbon(
                window,
                tail,
                mid,
                lead,
                crystal_w * 0.05,
                crystal_w * 0.50,
                tint.opacity(0.30 * mark_alpha),
            );
            curved_ribbon(
                window,
                tail,
                mid,
                lead,
                crystal_w * 0.02,
                crystal_w * 0.16,
                rgb(0xe8fcff).opacity(0.64 * mark_alpha),
            );
        }

        paint_sprite_centered(window, medallion, pos, sprite_diam);
    }
}

fn paint_nervegear_visor(
    bounds: Bounds<Pixels>,
    t: f32,
    art: Option<&Arc<FlightArtwork>>,
    window: &mut Window,
) {
    let void_alpha = 1. - ease(0.82, 1.04, t);
    if void_alpha <= 0. {
        return;
    }

    let w = f32::from(bounds.size.width);
    let h = f32::from(bounds.size.height);
    let center = bounds.center();
    let min_dim = w.min(h);

    window.paint_quad(fill(
        bounds,
        linear_gradient(
            180.,
            linear_color_stop(rgb(0x040712).opacity(void_alpha * 0.98), 0.),
            linear_color_stop(rgb(0x09132b).opacity(void_alpha * 0.96), 1.),
        ),
    ));

    let implosion = ease(0.73, 0.86, t);
    let breach = ease_out_cubic(0.84, 1.06, t);
    if let Some(art) = art {
        let glow_diam = min_dim * (0.42 + 0.25 * ease(0., 0.5, t) + 0.95 * breach) * void_alpha;
        paint_sprite_centered(window, &art.core_bloom, center, glow_diam);
    }

    let slit_fade = (1. - ease(0.22, 0.58, t)) * void_alpha;
    if slit_fade > 0.01 {
        let span = w * (0.14 + 0.68 * ease_out_cubic(0., 0.28, t));
        let glow_bar = Bounds::new(
            center - point(px(span * 0.5), px(3.5)),
            size(px(span), px(7.0)),
        );
        window.paint_quad(fill(glow_bar, rgb(0x38bdf8).opacity(slit_fade * 0.28)));
        let core_bar = Bounds::new(
            center - point(px(span * 0.5), px(0.9)),
            size(px(span), px(1.8)),
        );
        window.paint_quad(fill(core_bar, rgb(0xe0fcff).opacity(slit_fade * 0.95)));
    }

    let ring_appear = ease_out_cubic(0.05, 0.34, t);
    let ring_scale = (0.76 + 0.24 * ring_appear) * (1. - 0.52 * implosion) + 0.90 * breach;
    let base_r = (min_dim * 0.235).clamp(72., 168.) * ring_scale;
    let ring_alpha = void_alpha * ring_appear * (1. - 0.65 * breach);

    if ring_alpha > 0.01 {
        let rot1 = t * 0.58;
        let mut outer = PathBuilder::stroke(px(1.2));
        for k in 0..4 {
            let start_a = rot1 + k as f32 * (TAU * 0.25) + 0.14;
            let span = TAU * 0.25 - 0.28;
            outer.move_to(center + point(px(base_r * start_a.cos()), px(base_r * start_a.sin())));
            for s in 1..=12 {
                let a = start_a + span * (s as f32 / 12.);
                outer.line_to(center + point(px(base_r * a.cos()), px(base_r * a.sin())));
            }
        }
        for tick in 0..24 {
            let a = rot1 + tick as f32 * (TAU / 24.);
            let inner_t = if tick % 6 == 0 {
                base_r * 0.91
            } else {
                base_r * 0.955
            };
            outer.move_to(center + point(px(inner_t * a.cos()), px(inner_t * a.sin())));
            outer.line_to(center + point(px(base_r * 1.03 * a.cos()), px(base_r * 1.03 * a.sin())));
        }
        if let Ok(path) = outer.build() {
            window.paint_path(path, rgb(0x38bdf8).opacity(ring_alpha * 0.72));
        }

        let mid_r = base_r * 0.73;
        let rot2 = -t * 1.18 + 0.4;
        let mut mid_ring = PathBuilder::stroke(px(1.4));
        for k in 0..3 {
            let start_a = rot2 + k as f32 * (TAU / 3.);
            let span = TAU / 3. - 0.34;
            mid_ring.move_to(center + point(px(mid_r * start_a.cos()), px(mid_r * start_a.sin())));
            for s in 1..=10 {
                let a = start_a + span * (s as f32 / 10.);
                mid_ring.line_to(center + point(px(mid_r * a.cos()), px(mid_r * a.sin())));
            }
        }
        if let Ok(path) = mid_ring.build() {
            window.paint_path(path, rgb(0x7dd3fc).opacity(ring_alpha * 0.82));
        }

        let mut diamonds = PathBuilder::fill();
        for k in 0..6 {
            let a = rot2 + k as f32 * (TAU / 6.);
            let p = center + point(px(mid_r * a.cos()), px(mid_r * a.sin()));
            let d = 3.2;
            diamonds.move_to(p + point(px(0.), px(-d)));
            diamonds.line_to(p + point(px(d), px(0.)));
            diamonds.line_to(p + point(px(0.), px(d)));
            diamonds.line_to(p + point(px(-d), px(0.)));
            diamonds.close();
        }
        if let Ok(path) = diamonds.build() {
            window.paint_path(path, rgb(0x34d399).opacity(ring_alpha * 0.90));
        }

        let in_r = base_r * 0.42;
        let mut inner = PathBuilder::stroke(px(1.3));
        inner.move_to(center + point(px(in_r), px(0.)));
        for s in 1..=28 {
            let a = s as f32 * (TAU / 28.);
            inner.line_to(center + point(px(in_r * a.cos()), px(in_r * a.sin())));
        }
        for axis in 0..4 {
            let a = axis as f32 * (TAU * 0.25);
            inner.move_to(center + point(px(in_r * 0.38 * a.cos()), px(in_r * 0.38 * a.sin())));
            inner.line_to(center + point(px(in_r * 0.78 * a.cos()), px(in_r * 0.78 * a.sin())));
        }
        if let Ok(path) = inner.build() {
            window.paint_path(path, rgb(0x2dd4bf).opacity(ring_alpha * 0.86));
        }
    }

    if breach > 0.01 && breach < 0.99 {
        let shock_r = min_dim * (0.08 + 0.88 * breach);
        let mut shock = PathBuilder::stroke(px((4.5 * (1. - breach)).max(0.8)));
        shock.move_to(center + point(px(shock_r), px(0.)));
        for s in 1..=40 {
            let a = s as f32 * (TAU / 40.);
            shock.line_to(center + point(px(shock_r * a.cos()), px(shock_r * a.sin())));
        }
        if let Ok(path) = shock.build() {
            window.paint_path(path, rgb(0xa5f3fc).opacity((1. - breach) * 0.85));
        }
    }
}

fn paint_sao_calibration_room(
    bounds: Bounds<Pixels>,
    t: f32,
    reduced: bool,
    room_alpha: f32,
    art: Option<&Arc<FlightArtwork>>,
    window: &mut Window,
) {
    if room_alpha <= 0.005 {
        return;
    }
    let w = f32::from(bounds.size.width);
    let h = f32::from(bounds.size.height);
    let center = bounds.center();
    let min_dim = w.min(h);

    if let Some(art) = art {
        let d = min_dim * 0.72 * room_alpha;
        paint_sprite_centered(
            window,
            &art.room_bloom,
            center + point(px(-w * 0.14), px(-h * 0.08)),
            d,
        );
        paint_sprite_centered(
            window,
            &art.room_bloom,
            center + point(px(w * 0.15), px(h * 0.10)),
            d * 0.9,
        );
    }

    // Architectural SAO White Room perspective floor & ceiling horizon grid.
    let mut grid = PathBuilder::stroke(px(1.0));
    let floor_top = h * 0.72;
    let ceil_bot = h * 0.28;
    for row in 0..4 {
        let frac = (row as f32 / 3.).powf(1.35);
        let yf = bounds.origin.y + px(floor_top + (h - floor_top) * frac);
        grid.move_to(point(bounds.origin.x, yf));
        grid.line_to(point(bounds.origin.x + px(w), yf));

        let yc = bounds.origin.y + px(ceil_bot * (1. - frac));
        grid.move_to(point(bounds.origin.x, yc));
        grid.line_to(point(bounds.origin.x + px(w), yc));
    }
    for col in -5..=5 {
        let norm = col as f32 / 5.;
        let x_near = bounds.origin.x + px(w * (0.5 + norm * 0.68));
        let x_far = bounds.origin.x + px(w * (0.5 + norm * 0.18));
        grid.move_to(point(x_far, bounds.origin.y + px(floor_top)));
        grid.line_to(point(x_near, bounds.origin.y + px(h)));
        grid.move_to(point(x_far, bounds.origin.y + px(ceil_bot)));
        grid.line_to(point(x_near, bounds.origin.y));
    }
    if let Ok(path) = grid.build() {
        window.paint_path(path, rgb(0x5274a3).opacity(room_alpha * 0.055));
    }

    // Whisper-thin concentric SAO geometric calibration circles behind the title.
    let spin = if reduced { 0.35 } else { (t - 3.6) * 0.09 };
    let r_outer = (min_dim * 0.36).clamp(130., 260.);
    let r_inner = r_outer * 0.78;
    let mut rings = PathBuilder::stroke(px(1.0));
    for k in 0..4 {
        let start_a = spin + k as f32 * (TAU * 0.25) + 0.12;
        let span = TAU * 0.25 - 0.24;
        rings.move_to(center + point(px(r_outer * start_a.cos()), px(r_outer * start_a.sin())));
        for s in 1..=14 {
            let a = start_a + span * (s as f32 / 14.);
            rings.line_to(center + point(px(r_outer * a.cos()), px(r_outer * a.sin())));
        }
    }
    for k in 0..2 {
        let start_a = -spin * 1.3 + k as f32 * (TAU * 0.5) + 0.22;
        let span = TAU * 0.5 - 0.44;
        rings.move_to(center + point(px(r_inner * start_a.cos()), px(r_inner * start_a.sin())));
        for s in 1..=18 {
            let a = start_a + span * (s as f32 / 18.);
            rings.line_to(center + point(px(r_inner * a.cos()), px(r_inner * a.sin())));
        }
    }
    if let Ok(path) = rings.build() {
        window.paint_path(path, rgb(0x3b6ea5).opacity(room_alpha * 0.09));
    }
}

/// In-flow SAO Razor-Tapered Sword-Blade Horizon Divider so layout spacing is exact at all sizes.
fn render_sword_blade_divider(
    width: f32,
    t: f32,
    reduced: bool,
    room_alpha: f32,
    blade_progress: f32,
) -> Div {
    let max_span = (width * 0.54).clamp(200., 460.);
    div()
        .mt(px(18.))
        .mb(px(14.))
        .w(px(max_span))
        .h(px(14.))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    if blade_progress <= 0.01 || room_alpha <= 0.01 {
                        return;
                    }
                    let center = bounds.center();
                    let half_span = f32::from(bounds.size.width) * 0.5 * blade_progress;
                    let gap = 10.5;

                    if half_span > gap + 4. {
                        let mut wings = PathBuilder::fill();
                        wings.move_to(point(center.x - px(half_span), center.y));
                        wings.line_to(point(center.x - px(gap), center.y - px(1.35)));
                        wings.line_to(point(center.x - px(gap), center.y + px(1.35)));
                        wings.close();

                        wings.move_to(point(center.x + px(half_span), center.y));
                        wings.line_to(point(center.x + px(gap), center.y - px(1.35)));
                        wings.line_to(point(center.x + px(gap), center.y + px(1.35)));
                        wings.close();

                        if let Ok(path) = wings.build() {
                            window.paint_path(
                                path,
                                linear_gradient(
                                    90.,
                                    linear_color_stop(rgb(0x38bdf8).opacity(room_alpha * 0.88), 0.),
                                    linear_color_stop(rgb(0x6366f1).opacity(room_alpha * 0.88), 1.),
                                ),
                            );
                        }
                    }

                    // Central faceted SAO diamond (`◆`).
                    let d = 4.6 * blade_progress;
                    let mut gem = PathBuilder::fill();
                    gem.move_to(point(center.x, center.y - px(d)));
                    gem.line_to(point(center.x + px(d * 1.28), center.y));
                    gem.line_to(point(center.x, center.y + px(d)));
                    gem.line_to(point(center.x - px(d * 1.28), center.y));
                    gem.close();
                    if let Ok(path) = gem.build() {
                        window.paint_path(path, rgb(0x2563eb).opacity(room_alpha * 0.92));
                    }

                    // Live sweeping specular white-cyan glint traveling across the blade.
                    if !reduced {
                        let sweep = ((t - 4.16) / 1.08).clamp(0., 1.);
                        if sweep > 0. && sweep < 1. {
                            let gx = center.x + px((sweep * 2. - 1.) * half_span * 0.88);
                            let glint_a = (1. - (sweep * 2. - 1.).abs()) * room_alpha * 0.95;
                            let glint_box = Bounds::new(
                                point(gx - px(28.), center.y - px(1.5)),
                                size(px(56.), px(3.0)),
                            );
                            window.paint_quad(fill(glint_box, rgb(0xffffff).opacity(glint_a)));
                        }
                    }
                },
            )
            .size_full(),
        )
}

fn render_nervegear_hud_overlay(t: f32, width: f32, height: f32, cx: &App) -> Div {
    let hud_alpha = ease(0.06, 0.24, t) * (1. - ease(0.76, 0.90, t));
    let sync_pct = ((ease(0.08, 0.72, t) * 100.).round() as u32).min(100);
    let ring_offset = (width.min(height) * 0.235).clamp(72., 168.);
    let compact = width < 560.;

    const CHANNELS: [(&str, f32); 5] = [
        ("OPTICS", 0.18),
        ("AUDIO", 0.30),
        ("NEURAL", 0.42),
        ("CONTEXT", 0.54),
        ("AGENTS", 0.66),
    ];

    let mut strip = div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_center()
        .gap(px(if compact { 5. } else { 8. }));

    for (label, lock_at) in CHANNELS {
        let locked = t >= lock_at;
        let pulse = if locked {
            (1. - ((t - lock_at) / 0.16).clamp(0., 1.)) * 0.35
        } else {
            0.
        };
        let border = if locked {
            rgb(0x2ee59d).opacity(0.62 + pulse)
        } else {
            rgb(0x38bdf8).opacity(0.26)
        };
        let bg_col = if locked {
            rgb(0x062c28).opacity(0.72)
        } else {
            rgb(0x081730).opacity(0.64)
        };
        strip = strip.child(
            div()
                .h(px(22.))
                .px(px(if compact { 7. } else { 9. }))
                .rounded(px(4.))
                .bg(bg_col)
                .border_1()
                .border_color(border)
                .flex()
                .items_center()
                .gap(px(6.))
                .font_family(mono_font(cx))
                .font_features(tnum_features())
                .text_size(px(10.5))
                .child(div().size(px(5.)).rounded_full().bg(if locked {
                    rgb(0x34d399)
                } else {
                    rgb(0x38bdf8).opacity(0.55)
                }))
                .child(div().text_color(rgb(0xcbd5e1)).child(label))
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .text_color(if locked { rgb(0x34d399) } else { rgb(0x64748b) })
                        .child(if locked { "OK" } else { "··" }),
                ),
        );
    }

    div()
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .opacity(hud_alpha)
        .child(
            div()
                .mb(px(ring_offset * 2.0 + 26.))
                .flex()
                .items_center()
                .gap(px(10.))
                .font_family(mono_font(cx))
                .font_features(tnum_features())
                .text_size(px(11.))
                .text_color(rgb(0x7dd3fc))
                .child("NERVEGEAR // LINK START")
                .child(div().w(px(24.)).h(px(1.)).bg(rgb(0x38bdf8).opacity(0.45)))
                .child(
                    div()
                        .text_color(if sync_pct == 100 {
                            rgb(0x34d399)
                        } else {
                            rgb(0xbae6fd)
                        })
                        .child(format!("SYNC {sync_pct:03}%")),
                ),
        )
        .child(div().mt(px(10.)).px(px(16.)).child(strip))
}

#[allow(clippy::too_many_arguments)]
fn render_tracked_line(
    text: &'static str,
    family: SharedString,
    start_t: f32,
    stagger: f32,
    t: f32,
    reduced: bool,
    font_px: f32,
    weight: FontWeight,
    letter_gap: f32,
    word_gap: f32,
    settled_rgb: u32,
    shimmer_rgb: u32,
) -> Div {
    if reduced {
        return div()
            .font_family(family)
            .text_size(px(font_px))
            .line_height(px(font_px * 1.15))
            .font_weight(weight)
            .text_color(rgb(settled_rgb))
            .child(text);
    }

    let [sr, sg, sb] = unpack_rgb(settled_rgb).map(|c| c / 255.);
    let [hr, hg, hb] = unpack_rgb(shimmer_rgb).map(|c| c / 255.);

    let mut row = div()
        .font_family(family)
        .flex()
        .flex_wrap()
        .items_center()
        .justify_center()
        .gap(px(word_gap));

    let mut char_index = 0usize;
    for word in text.split(' ') {
        let mut word_row = div().flex().items_baseline().gap(px(letter_gap));
        for ch in word.chars() {
            let c_start = start_t + char_index as f32 * stagger;
            char_index += 1;
            let p = ease_out_cubic(c_start, c_start + 0.34, t);
            let flash = ((1. - ((t - (c_start + 0.09)) / 0.18).abs().min(1.)) * p).clamp(0., 1.);
            let r = sr + (hr - sr) * flash;
            let g = sg + (hg - sg) * flash;
            let b = sb + (hb - sb) * flash;
            let lift = (1. - p) * 8.;

            word_row = word_row.child(
                div()
                    .mt(px(lift))
                    .opacity(p)
                    .text_size(px(font_px))
                    .line_height(px(font_px * 1.15))
                    .font_weight(weight)
                    .text_color(Rgba { r, g, b, a: 1. })
                    .child(SharedString::from(ch.to_string())),
            );
        }
        row = row.child(word_row);
    }
    row
}

pub(crate) fn film(elapsed: Duration, reduced: bool, window: &mut Window, cx: &mut App) -> Div {
    let t = elapsed.as_secs_f32();
    let viewport = window.viewport_size();
    let width = f32::from(viewport.width);
    let height = f32::from(viewport.height);
    let title_size = (width * 0.060).clamp(28., 58.);
    let display_font = sao_display_font(cx);

    let in_tunnel = !reduced && t < TUNNEL_EXIT;
    let in_visor = !reduced && t < 1.04;
    let show_room = reduced || t >= 3.56;

    let outro_fade = if reduced {
        1.
    } else {
        1. - ease(5.84, 6.18, t)
    };
    let outro_lift = if reduced {
        0.
    } else {
        ease(5.84, 6.18, t) * -10.
    };

    let room_alpha = if reduced { 1. } else { ease(3.56, 4.02, t) };
    let blade_progress = if reduced {
        1.
    } else {
        ease_out_cubic(4.06, 4.76, t) * outro_fade
    };
    let header_alpha = if reduced {
        1.
    } else {
        ease(3.74, 4.14, t) * outro_fade
    };
    let footer_alpha = if reduced {
        1.
    } else {
        ease(4.48, 4.92, t) * outro_fade
    };

    let side = DevicePixels((208. * window.scale_factor()).ceil() as i32);
    let artwork = window.use_asset::<FlightArtwork>(&side, cx);

    div()
        .relative()
        .size_full()
        .bg(welcome_ground(1.))
        .overflow_hidden()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .when(in_tunnel, |stage| {
            let art_for_tunnel = artwork.clone();
            stage.child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let w = f32::from(bounds.size.width);
                        let h = f32::from(bounds.size.height);

                        if t >= TUNNEL_START - 0.05 {
                            if let Some(art) = &art_for_tunnel {
                                // Dilate `image_bounds` by 4px while clipping to `bounds` so Metal's
                                // bilinear filtering never samples neighboring sprite-atlas tiles.
                                let _ = window.paint_image(
                                    bounds,
                                    bounds.dilate(px(4.)),
                                    Corners::default(),
                                    art.wormhole_backdrop.clone(),
                                    0,
                                    false,
                                );
                            } else {
                                window.paint_quad(fill(bounds, rgb(0x071029)));
                            }

                            let fly = (t - TUNNEL_START).max(0.);
                            let camera = Camera {
                                vanishing: bounds.origin
                                    + point(
                                        px(w * (0.5 + 0.048 * (fly * 1.38).sin())),
                                        px(h * (0.49 + 0.036 * (fly * 1.06 - 0.35).sin())),
                                    ),
                                radius: w.min(h * 1.35) * 0.45,
                                travel: fly * 1.52 + fly * fly * 0.64,
                                bend_x: 0.056 * (fly * 1.52).sin(),
                                bend_y: -0.034 * (fly * 1.15).cos(),
                                roll: fly * 0.28 + 0.14 * (fly * 0.92).sin(),
                            };

                            paint_tunnel_rings(&camera, window);
                            paint_prismatic_ribbons(&camera, window);

                            if let Some(art) = &art_for_tunnel {
                                paint_passing_agents(&camera, art, 2.85..5.20, window);
                            }

                            let breakthrough = ease(3.15, TUNNEL_EXIT, t);
                            if let Some(art) = &art_for_tunnel {
                                let pulse = 1. + 0.09 * (fly * 8.5).sin();
                                let throat_diam = camera.radius
                                    * (0.92 * pulse + 5.2 * breakthrough * breakthrough);
                                paint_sprite_centered(
                                    window,
                                    &art.core_bloom,
                                    camera.spine(THROAT_DEPTH),
                                    throat_diam,
                                );
                            }

                            if let Some(art) = &art_for_tunnel {
                                paint_passing_agents(&camera, art, 0.18..2.85, window);
                            }

                            let whiteout = ease(3.34, TUNNEL_EXIT, t);
                            if whiteout > 0. {
                                window.paint_quad(fill(bounds, welcome_ground(whiteout)));
                            }
                        }

                        if t < 1.05 {
                            paint_nervegear_visor(bounds, t, art_for_tunnel.as_ref(), window);
                        }
                    },
                )
                .absolute()
                .inset_0(),
            )
        })
        .when(in_visor, |stage| {
            stage.child(render_nervegear_hud_overlay(t, width, height, cx))
        })
        .when(show_room, |stage| {
            let art_for_room = artwork.clone();
            stage
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            paint_sao_calibration_room(
                                bounds,
                                t,
                                reduced,
                                room_alpha,
                                art_for_room.as_ref(),
                                window,
                            );
                        },
                    )
                    .absolute()
                    .inset_0(),
                )
                .child(
                    div()
                        .mt(px(outro_lift))
                        .px(px(20.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .opacity(room_alpha * outro_fade)
                        .text_center()
                        .child(
                            div()
                                .mb(px(16.))
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .opacity(header_alpha)
                                .font_family(mono_font(cx))
                                .text_size(px(10.5))
                                .text_color(rgb(0x526b91))
                                .child(div().w(px(34.)).h(px(1.)).bg(rgb(0x7aa2d4).opacity(0.55)))
                                .child("◇")
                                .child("SYSTEM LINK ESTABLISHED")
                                .child("◇")
                                .child(div().w(px(34.)).h(px(1.)).bg(rgb(0x7aa2d4).opacity(0.55))),
                        )
                        .child(render_tracked_line(
                            "Welcome To",
                            display_font.clone(),
                            3.80,
                            0.024,
                            t,
                            reduced,
                            (title_size * 0.40).clamp(16., 24.),
                            FontWeight::LIGHT,
                            4.2,
                            13.0,
                            0x4b5f82,
                            0x0ea5e9,
                        ))
                        .child(div().mt(px(8.)).child(render_tracked_line(
                            "Kense Agent Online",
                            display_font,
                            4.06,
                            0.022,
                            t,
                            reduced,
                            title_size,
                            FontWeight::MEDIUM,
                            2.4,
                            (title_size * 0.30).max(12.),
                            0x0f1d38,
                            0x22b8cf,
                        )))
                        .child(render_sword_blade_divider(
                            width,
                            t,
                            reduced,
                            room_alpha,
                            blade_progress,
                        ))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .opacity(footer_alpha)
                                .font_family(mono_font(cx))
                                .text_size(px(10.5))
                                .text_color(rgb(0x647a9e))
                                .child(div().size(px(5.)).rounded_full().bg(rgb(0x10b981)))
                                .child("CARDINAL // FULLDIVE WORKBENCH READY"),
                        ),
                )
        })
}
