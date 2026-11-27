//! A single selectable Markdown document per reply, including while it is arriving.
use super::*;
use gpui_kit::{
    base::Easing,
    component::{
        highlighter::HighlightTheme,
        text::{TextView, TextViewMotion, TextViewState, TextViewStyle},
    },
};
use kui::primitives::family::disabled_motion;
use std::{collections::VecDeque, time::Instant};
use unicode_segmentation::UnicodeSegmentation;

// Enough time for a burst to unfold over several frames, even on a 60 Hz display.
const REVEAL: Duration = Duration::from_millis(100);
const TICK: Duration = Duration::from_millis(20);

#[derive(Default)]
struct Reveal {
    settled: usize,
    arrivals: VecDeque<(Instant, usize)>,
}

impl Reveal {
    fn reset(&mut self, len: usize) {
        self.arrivals.clear();
        self.settled = len;
    }

    fn position(&mut self, now: Instant) -> usize {
        while let Some(&(at, end)) = self.arrivals.front() {
            if now.saturating_duration_since(at) < REVEAL {
                break;
            }
            self.settled = end;
            self.arrivals.pop_front();
        }
        let mut previous = self.settled;
        let mut position = self.settled as f64;
        for &(at, end) in &self.arrivals {
            let fraction = now.saturating_duration_since(at).as_secs_f64() / REVEAL.as_secs_f64();
            position += (end - previous) as f64 * fraction;
            previous = end;
        }
        position.floor() as usize
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
        let domain_len = label.bytes().take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')).count();
        if domain_len != 0 && let Some(destination) = label[domain_len..].strip_prefix("](<")
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
    reveal: Reveal,
    timer: Option<Task<()>>,
    animated: bool,
}

impl ReplyStream {
    fn new(source: &SharedString, cx: &mut Context<Self>) -> Self {
        // Opening an existing turn immediately presents everything already received.
        let document = cx.new(|cx| TextViewState::markdown(source, cx));
        Self {
            document,
            source: source.clone(),
            painted: source.len(),
            reveal: Reveal {
                settled: source.len(),
                ..Default::default()
            },
            timer: None,
            animated: false,
        }
    }

    fn receive(&mut self, source: SharedString, animate: bool, cx: &mut Context<Self>) {
        if animate != self.animated {
            self.animated = animate;
            let motion = if animate {
                TextViewMotion::default()
                    .with_stream_fade(Duration::from_millis(145))
                    .with_stream_fade_stagger(Duration::from_millis(4))
                    .with_stream_fade_easing(Easing::EaseOut)
            } else {
                TextViewMotion::default()
            };
            self.document.update(cx, |doc, _| doc.set_motion(motion));
        }

        if source == self.source && (animate || self.painted == source.len()) {
            return;
        }
        if !animate || !source.starts_with(self.source.as_ref()) {
            // Replacements, completion and interruptions always reconcile to Kar's full text.
            self.timer = None;
            self.reveal.reset(source.len());
            self.painted = source.len();
            self.document
                .update(cx, |doc, cx| doc.set_text(&source, cx));
            self.source = source;
            return;
        }

        self.reveal
            .arrivals
            .push_back((Instant::now(), source.len()));
        self.source = source;
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

    fn advance(&mut self, cx: &mut Context<Self>) -> bool {
        let desired = self.reveal.position(Instant::now());
        let end = next_boundary(&self.source, self.painted, desired);
        if end > self.painted {
            let delta = &self.source[self.painted..end];
            self.document.update(cx, |doc, cx| doc.push_str(delta, cx));
            self.painted = end;
        }
        if self.painted == self.source.len() {
            self.reveal.reset(self.painted);
            self.timer = None;
            return false;
        }
        true
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
        } = self;
        let state = window.use_keyed_state(
            ElementId::Name(format!("reply-{turn_id}-{item_ix}").into()),
            cx,
            |_, cx| ReplyStream::new(&source, cx),
        );
        let animate = streaming && window.is_visible() && !disabled_motion(cx);
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
            .on_link_click(|href, _, _, _| {
                let url = href.as_ref();
                if let Some(local_path) = url.strip_prefix("file://") {
                    let clean = local_path.split('#').next().unwrap_or(local_path);
                    let _ = std::process::Command::new("open").arg(clean).spawn();
                } else if url.starts_with("http://") || url.starts_with("https://") {
                    let _ = std::process::Command::new("open").arg(url).spawn();
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
    use super::{REVEAL, Reveal, next_boundary};
    use std::time::Instant;
    use unicode_segmentation::UnicodeSegmentation;

    #[test]
    fn overlapping_bursts_grow_monotonically_and_finish_on_deadline() {
        let start = Instant::now();
        let mut reveal = Reveal {
            settled: 12,
            ..Default::default()
        };
        reveal.arrivals.push_back((start, 52));
        assert_eq!(reveal.position(start + REVEAL / 2), 32);
        reveal.arrivals.push_back((start + REVEAL / 2, 72));
        assert_eq!(reveal.position(start + REVEAL), 62);
        assert_eq!(reveal.position(start + REVEAL + REVEAL / 2), 72);
        assert!(reveal.arrivals.is_empty());
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
