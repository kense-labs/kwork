//! Adaptive 60 Hz token-velocity streaming for Assistant Markdown replies and Reasoning steps.
//!
//! Network SSE / ACP deltas arrive in uneven bursts (for example, a reasoning model may pause
//! 400–900 ms and then flush 180 characters at once). A rigid 100 ms linear reveal dumps the
//! entire burst in 5 frames and then freezes completely until the next packet. Instead,
//! [`AdaptivePacer`] acts as a smooth proportional buffer (`v = v_floor + k * backlog_graphemes`):
//! large bursts accelerate smoothly so the UI never lags behind live generation, while as the
//! buffer thins out, velocity eases down to a gentle ~42 graphemes/sec trickle so text keeps
//! flowing continuously across inter-packet gaps without freezing or jumping.

use super::*;
use gpui_kit::{
    base::Easing,
    component::{
        highlighter::HighlightTheme,
        text::{TextView, TextViewMotion, TextViewState, TextViewStyle},
    },
};
use kui::primitives::family::disabled_motion;
use std::time::Instant;
use unicode_segmentation::UnicodeSegmentation;

const TICK: Duration = Duration::from_millis(16);
/// Gentle baseline trickle speed (graphemes/sec) when only a small tail remains in the buffer,
/// bridging 300–600 ms network gaps without freezing.
const MIN_GRAPHEMES_PER_SEC: f64 = 42.0;
/// Proportional catch-up gain (`1/s`): keeps steady-state lag bounded to ~0.22s at any speed.
const BACKLOG_GAIN_PER_SEC: f64 = 4.2;
/// Fast drain horizon when a turn/step finishes (`streaming == false`).
const FINISH_DRAIN_SECS: f64 = 0.065;

struct AdaptivePacer {
    last_tick: Instant,
    credit_graphemes: f64,
}

impl Default for AdaptivePacer {
    fn default() -> Self {
        Self {
            last_tick: Instant::now(),
            credit_graphemes: 0.0,
        }
    }
}

impl AdaptivePacer {
    fn reset(&mut self, now: Instant) {
        self.last_tick = now;
        self.credit_graphemes = 0.0;
    }

    fn on_arrival(&mut self, was_idle: bool, now: Instant) {
        if was_idle {
            self.last_tick = now;
            // Seed a tiny fractional credit so the very first frame after an idle gap advances immediately.
            self.credit_graphemes = self.credit_graphemes.max(1.0);
        }
    }

    /// Compute the next byte boundary (`>= painted`) to reveal at `now`.
    fn step(&mut self, source: &str, painted: usize, finishing: bool, now: Instant) -> usize {
        if painted >= source.len() {
            self.credit_graphemes = 0.0;
            self.last_tick = now;
            return source.len();
        }
        let dt = now
            .saturating_duration_since(self.last_tick)
            .as_secs_f64()
            .clamp(0.0, 0.08);
        self.last_tick = now;

        let suffix = &source[painted..];
        let rem_graphemes = suffix.graphemes(true).count();
        if rem_graphemes == 0 {
            return source.len();
        }

        let rate = if finishing {
            (rem_graphemes as f64 / FINISH_DRAIN_SECS).max(180.0)
        } else {
            MIN_GRAPHEMES_PER_SEC + (rem_graphemes as f64) * BACKLOG_GAIN_PER_SEC
        };
        self.credit_graphemes += rate * dt;
        let take = self.credit_graphemes.floor() as usize;
        if take == 0 {
            return painted;
        }
        self.credit_graphemes -= take as f64;

        let desired = if take >= rem_graphemes {
            source.len()
        } else {
            let rel = suffix
                .grapheme_indices(true)
                .nth(take)
                .map_or(suffix.len(), |(off, _)| off);
            painted + rel
        };
        next_boundary(source, painted, desired)
    }
}

fn next_boundary(source: &str, painted: usize, desired: usize) -> usize {
    if desired <= painted || desired >= source.len() {
        return desired.clamp(painted, source.len());
    }
    let suffix = &source[painted..];
    let end = painted
        + suffix
            .grapheme_indices(true)
            .map(|(offset, g)| offset + g.len())
            .find(|&end| painted + end >= desired)
            .unwrap_or(suffix.len());
    // Revealing half a citation would momentarily paint its raw Markdown and URL.
    if let Some(open) = source[painted..end].rfind('[') {
        let start = painted + open;
        let label = &source[start + 1..];
        let domain_len = label
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
            .count();
        if domain_len != 0
            && let Some(destination) = label[domain_len..].strip_prefix("](<")
            && (destination.starts_with("https://") || destination.starts_with("http://"))
            && let Some(close) = destination.find(">)")
            && end < source.len() - destination.len() + close + 2
        {
            return start;
        }
    }
    end
}

struct ReplyStream {
    document: Entity<TextViewState>,
    source: SharedString,
    painted: usize,
    pacer: AdaptivePacer,
    timer: Option<Task<()>>,
    animated: bool,
    finishing: bool,
}

impl ReplyStream {
    fn new(source: &SharedString, animate: bool, cx: &mut Context<Self>) -> Self {
        let initial_painted = if animate && source.len() <= 96 {
            0
        } else {
            source.len()
        };
        let initial_slice = SharedString::from(source[..initial_painted].to_owned());
        let document = cx.new(|cx| TextViewState::markdown(&initial_slice, cx));
        let mut s = Self {
            document,
            source: SharedString::default(),
            painted: initial_painted,
            pacer: AdaptivePacer::default(),
            timer: None,
            animated: false,
            finishing: false,
        };
        if initial_painted < source.len() {
            s.receive(source.clone(), true, cx);
        } else {
            s.source = source.clone();
        }
        s
    }

    fn ensure_timer(&mut self, cx: &mut Context<Self>) {
        if self.timer.is_none() {
            self.timer = Some(cx.spawn(async |weak, cx| {
                loop {
                    cx.background_executor().timer(TICK).await;
                    if !weak
                        .update(cx, |stream, cx| stream.advance(cx))
                        .unwrap_or(false)
                    {
                        break;
                    }
                }
            }));
        }
    }

    fn receive(&mut self, source: SharedString, animate: bool, cx: &mut Context<Self>) {
        if animate != self.animated {
            self.animated = animate;
            let motion = if animate {
                TextViewMotion::default()
                    .with_stream_fade(Duration::from_millis(150))
                    .with_stream_fade_stagger(Duration::from_millis(5))
                    .with_stream_fade_easing(Easing::EaseOut)
            } else {
                TextViewMotion::default()
            };
            self.document.update(cx, |doc, _| doc.set_motion(motion));
        }

        // Non-prefix replacement (e.g. citation rewrite or turn switch): reconcile cleanly.
        if !source.starts_with(self.source.as_ref()) {
            self.timer = None;
            self.finishing = false;
            self.pacer.reset(Instant::now());
            self.painted = source.len();
            self.document
                .update(cx, |doc, cx| doc.set_text(&source, cx));
            self.source = source;
            return;
        }

        if !animate {
            if self.painted < source.len() && self.timer.is_some() {
                // Smoothly finish draining the tail over ~65 ms instead of snapping abruptly.
                self.source = source;
                self.finishing = true;
                return;
            }
            if source != self.source || self.painted != source.len() {
                self.timer = None;
                self.finishing = false;
                self.pacer.reset(Instant::now());
                self.painted = source.len();
                self.document
                    .update(cx, |doc, cx| doc.set_text(&source, cx));
                self.source = source;
            }
            return;
        }

        self.finishing = false;
        if source == self.source && self.painted == source.len() {
            return;
        }
        let was_idle = self.timer.is_none();
        self.source = source;
        if self.painted < self.source.len() {
            self.pacer.on_arrival(was_idle, Instant::now());
            self.ensure_timer(cx);
        }
    }

    fn advance(&mut self, cx: &mut Context<Self>) -> bool {
        let end = self
            .pacer
            .step(&self.source, self.painted, self.finishing, Instant::now());
        if end > self.painted {
            let delta = &self.source[self.painted..end];
            self.document.update(cx, |doc, cx| doc.push_str(delta, cx));
            self.painted = end;
        }
        if self.painted >= self.source.len() {
            self.pacer.reset(Instant::now());
            self.finishing = false;
            self.timer = None;
            return false;
        }
        true
    }
}

/// Adaptive 60 Hz plain-text stream state for live Reasoning / Thinking steps (`render_step_text`).
struct ReasoningStream {
    source: SharedString,
    painted: usize,
    pacer: AdaptivePacer,
    timer: Option<Task<()>>,
    finishing: bool,
    started_at: Instant,
}

impl ReasoningStream {
    fn new(source: &SharedString, animate: bool, cx: &mut Context<Self>) -> Self {
        let initial_painted = if animate && source.len() <= 160 {
            0
        } else {
            source.len()
        };
        let mut s = Self {
            source: SharedString::default(),
            painted: initial_painted,
            pacer: AdaptivePacer::default(),
            timer: None,
            finishing: false,
            started_at: Instant::now(),
        };
        if initial_painted < source.len() {
            s.receive(source.clone(), true, cx);
        } else {
            s.source = source.clone();
        }
        s
    }

    fn receive(&mut self, source: SharedString, animate: bool, cx: &mut Context<Self>) {
        if !source.starts_with(self.source.as_ref()) {
            self.timer = None;
            self.finishing = false;
            self.pacer.reset(Instant::now());
            self.painted = source.len();
            self.source = source;
            cx.notify();
            return;
        }
        if !animate {
            if self.painted < source.len() && self.timer.is_some() {
                self.source = source;
                self.finishing = true;
                return;
            }
            if self.painted != source.len() || self.source != source {
                self.timer = None;
                self.finishing = false;
                self.pacer.reset(Instant::now());
                self.painted = source.len();
                self.source = source;
                cx.notify();
            }
            return;
        }
        self.finishing = false;
        let was_idle = self.timer.is_none();
        self.source = source;
        if self.painted < self.source.len() {
            self.pacer.on_arrival(was_idle, Instant::now());
            if self.timer.is_none() {
                self.timer = Some(cx.spawn(async |weak, cx| {
                    loop {
                        cx.background_executor().timer(TICK).await;
                        if !weak
                            .update(cx, |stream, cx| stream.advance(cx))
                            .unwrap_or(false)
                        {
                            break;
                        }
                    }
                }));
            }
        }
    }

    fn advance(&mut self, cx: &mut Context<Self>) -> bool {
        let end = self
            .pacer
            .step(&self.source, self.painted, self.finishing, Instant::now());
        if end > self.painted {
            self.painted = end;
            cx.notify();
        }
        if self.painted >= self.source.len() {
            self.pacer.reset(Instant::now());
            self.finishing = false;
            self.timer = None;
            return false;
        }
        true
    }

    fn visible_text(&self, active: bool) -> String {
        let slice = &self.source[..self.painted.min(self.source.len())];
        if active {
            let phase = (self.started_at.elapsed().as_secs_f32() * 3.4).sin();
            if phase >= -0.25 {
                return format!("{slice}▍");
            }
        }
        slice.to_owned()
    }
}

#[derive(IntoElement)]
pub(super) struct ReasoningText {
    pub turn_id: Uuid,
    pub step_ix: usize,
    pub source: SharedString,
    pub active: bool,
    pub font_size: Pixels,
    pub line_height: Pixels,
    pub color: Hsla,
}

impl RenderOnce for ReasoningText {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            turn_id,
            step_ix,
            source,
            active,
            font_size,
            line_height,
            color,
        } = self;
        let animate = active && window.is_visible() && !disabled_motion(cx);
        let state = window.use_keyed_state(
            ElementId::Name(format!("reasoning-{turn_id}-{step_ix}").into()),
            cx,
            |_, cx| ReasoningStream::new(&source, animate, cx),
        );
        let shown = state.update(cx, |stream, cx| {
            stream.receive(source, animate, cx);
            stream.visible_text(animate)
        });
        if animate {
            window.request_animation_frame();
        }
        div()
            .w_full()
            .min_w_0()
            .text_size(font_size)
            .line_height(line_height)
            .text_color(color)
            .child(shown)
    }
}

#[derive(IntoElement)]
pub(super) struct ReplyMarkdown {
    pub turn_id: Uuid,
    pub item_ix: usize,
    pub source: SharedString,
    pub streaming: bool,
    pub muted: Hsla,
    pub copy_label: &'static str,
    pub workbench: WeakEntity<crate::workbench::KarWorkbench>,
}

impl RenderOnce for ReplyMarkdown {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            turn_id,
            item_ix,
            source,
            streaming,
            muted,
            copy_label,
            workbench,
        } = self;
        let animate = streaming && window.is_visible() && !disabled_motion(cx);
        let state = window.use_keyed_state(
            ElementId::Name(format!("reply-{turn_id}-{item_ix}").into()),
            cx,
            |_, cx| ReplyStream::new(&source, animate, cx),
        );
        let document = state.update(cx, |stream, cx| {
            stream.receive(source, animate, cx);
            stream.document.clone()
        });

        let s = StudioPalette::get(cx);
        let mono = crate::ui_primitives::mono_font(cx);
        let code_size = px(crate::ui_primitives::effective_code_font_size(cx));
        let inline_ink = s.ink;
        let inline_bg = s.ink.opacity(if s.dark { 0.075 } else { 0.05 });

        let mut table = StyleRefinement::default()
            .border_1()
            .border_color(s.line_soft)
            .bg(s.card.opacity(0.55))
            .rounded(px(crate::ui_primitives::R_CTRL));
        table.overflow.x = Some(Overflow::Scroll);

        let base_style = TextViewStyle {
            paragraph_gap: rems(0.82),
            heading_base_font_size: s.font_size(FS_READ),
            highlight_theme: if s.dark {
                HighlightTheme::default_dark()
            } else {
                HighlightTheme::default_light()
            },
            is_dark: s.dark,
            ..TextViewStyle::default()
        };
        let style = base_style
            .heading_font_size(move |level, _| {
                s.font_size(match level {
                    1 => 19.,
                    2 => 17.,
                    3 => 15.5,
                    _ => FS_READ,
                })
            })
            .inline_code(HighlightStyle {
                color: Some(inline_ink),
                background_color: Some(inline_bg),
                font_weight: Some(FontWeight::MEDIUM),
                ..Default::default()
            })
            .code_block(
                StyleRefinement::default()
                    .font_family(mono.clone())
                    .text_size(code_size)
                    .pt(px(38.))
                    .pb(px(10.))
                    .rounded(px(R_CARD))
                    .bg(s.stage)
                    .border_1()
                    .border_color(s.line_soft),
            )
            .table(table)
            .table_head(
                StyleRefinement::default()
                    .bg(s.soft)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(s.ink),
            )
            .table_cell(
                StyleRefinement::default()
                    .py(px(7.))
                    .px(px(10.))
                    .border_color(s.line_soft),
            );

        let mono_for_badge = mono.clone();
        TextView::new(&document)
            .style(style)
            .inline_code_style(
                StyleRefinement::default()
                    .font_family(mono)
                    .text_size(code_size)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(inline_ink)
                    .bg(inline_bg)
                    .px(px(3.))
                    .rounded(px(crate::ui_primitives::R_XS)),
            )
            .blockquote_style(
                StyleRefinement::default()
                    .border_l(px(3.))
                    .border_color(s.pro.accent_info)
                    .bg(s.pro.accent_soft.opacity(if s.dark { 0.22 } else { 0.38 }))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(crate::ui_primitives::R_SM))
                    .text_color(s.ink),
            )
            .list_style(StyleRefinement::default().text_color(s.muted))
            .on_link_click(move |href, _, win, cx| {
                let url = href.as_ref();
                if let Some(local_path) = url.strip_prefix("file://") {
                    let clean = local_path.split('#').next().unwrap_or(local_path);
                    let _ = std::process::Command::new("open").arg(clean).spawn();
                } else if (url.starts_with("http://") || url.starts_with("https://"))
                    && workbench
                        .update(cx, |wb, cx| wb.open_url_in_browser(url, false, win, cx))
                        .is_err()
                {
                    cx.open_url(url);
                }
            })
            .selectable(true)
            .code_block_actions(move |code, window, cx| {
                let key = format!(
                    "code-{turn_id}-{item_ix}-{}",
                    code.span.map_or(0, |span| span.start)
                );
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .when_some(code.lang().filter(|lang| !lang.is_empty()), |row, lang| {
                        row.child(
                            div()
                                .max_w(px(110.))
                                .px(px(6.))
                                .py(px(1.))
                                .rounded(px(crate::ui_primitives::R_XS))
                                .bg(s.soft)
                                .font_family(mono_for_badge.clone())
                                .truncate()
                                .text_size(px(FS_TAG))
                                .text_color(muted)
                                .child(lang),
                        )
                    })
                    .child(copy_text_button(key, copy_label, code.code(), window, cx))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{AdaptivePacer, TICK, next_boundary};
    use std::time::{Duration, Instant};
    use unicode_segmentation::UnicodeSegmentation;

    #[test]
    fn adaptive_pacer_spreads_burst_smoothly_without_instant_dump_or_stall() {
        let start = Instant::now();
        let source = "思考过程：正在为你规划文章结构、居中标题、首行两字符缩进与三线表格排版方案，确保视觉效果清晰美观。".repeat(3);
        let mut pacer = AdaptivePacer {
            last_tick: start,
            credit_graphemes: 0.0,
        };
        pacer.on_arrival(true, start);
        let mut painted = 0usize;
        let mut now = start;
        let mut advances = 0usize;
        for _ in 0..45 {
            now += TICK;
            let next = pacer.step(&source, painted, false, now);
            assert!(next >= painted, "pacer must advance monotonically");
            // Never dump more than half the entire paragraph in a single 16ms frame!
            assert!(
                next - painted < source.len() / 2,
                "single frame should not dump entire burst"
            );
            if next > painted {
                advances += 1;
            }
            painted = next;
            if painted == source.len() {
                break;
            }
        }
        assert!(advances >= 10, "burst must unfold smoothly across many frames");
        // When finishing turns true, remaining tail drains promptly.
        for _ in 0..8 {
            now += Duration::from_millis(16);
            painted = pacer.step(&source, painted, true, now);
        }
        assert_eq!(painted, source.len());
    }

    #[test]
    fn frames_show_a_resolved_citation_as_one_link() {
        let link = "[example.org](<https://example.org/report_(2026)>)";
        let source = format!("A {link} B");
        for cut in 3..2 + link.len() {
            assert_eq!(next_boundary(&source, 2, cut), 2);
        }
        assert_eq!(next_boundary(&source, 2, 2 + link.len()), 2 + link.len());
    }

    #[test]
    fn frames_never_split_a_received_grapheme() {
        let source = "好e\u{301}👩🏽‍💻🇨🇳。";
        let mut painted = 0;
        for expected in source.graphemes(true) {
            let end = next_boundary(source, painted, painted + 1);
            assert_eq!(&source[painted..end], expected);
            painted = end;
        }
        assert_eq!(painted, source.len());
    }
}
