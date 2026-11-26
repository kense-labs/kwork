//! Pure-GPUI + KUI Interactive Brief renderer (`KarBrief`).
//!
//! Renders rich versioned interactive reports directly inside the conversation timeline without
//! HTML, WebViews, or JavaScript: KUI `AnimatedCounter` digit wheels, `BarChart`, `DonutChart`,
//! `Sparkline`, `SankeyFlow` (with flow particles), `DataFlow`, and revision-pinned `ReviewForm`s.

use crate::ui_motion::selection::{Group, Item, SelectionStrip};
use std::collections::BTreeMap;

use gpui::{prelude::*, *};
use kui::{
    components::{
        animated_counter::AnimatedCounter,
        bar_chart::BarChart,
        data_flow::{DataFlow, DataFlowNode, DataFlowSnapshot, DataFlowStatus},
        donut_chart::DonutChart,
        funnel_chart::{FunnelChart, FunnelSegment, FunnelStage},
        gauge::Gauge,
        json_viewer::JsonViewer,
        line_chart::LineChart,
        pro_chrome::pro_fold,
        radar_chart::{RadarAxis, RadarChart, RadarProfile},
        sankey_flow::{FlowLink, FlowNode, SankeyFlow},
        sparkline::Sparkline,
    },
    primitives::{
        editor::{EditEvent, Editor, InputMode},
        glyph::Glyph,
        ops_surface::arrive,
        plot::Sample,
        pro_icons::{ProIcon, pro_icon},
    },
};

use crate::{
    brief::{
        BriefBlock, BriefFieldKind, BriefFormDraft, BriefStepState, BriefTone, KarBrief,
        KarBriefRevision,
    },
    ui_motion::{Tactile, morph_glyph, primary_button, soft_button, text_button},
    ui_primitives::{
        CTRL_H, FS_META, FS_SECTION, FS_TAG, FS_UI, ICON_SM, ICON_XS, LH_META, R_CARD, R_CTRL,
        R_PILL, R_SM, StudioPalette, render_brief_chart_icon, tnum_features,
    },
    workbench::KarWorkbench,
};

/// Cached stateful KUI child entities owned per `(brief_id, revision, block_idx)` so spring
/// physics, rolling digit wheels, flow particles, and pointer hover scrubbers persist across frames.
#[derive(Default)]
pub struct BriefWidgetCache {
    pub counters: BTreeMap<String, Entity<AnimatedCounter>>,
    pub bar_charts: BTreeMap<String, Entity<BarChart>>,
    pub line_charts: BTreeMap<String, Entity<LineChart>>,
    pub donut_charts: BTreeMap<String, Entity<DonutChart>>,
    pub sparklines: BTreeMap<String, Entity<Sparkline>>,
    pub radar_charts: BTreeMap<String, Entity<RadarChart>>,
    pub funnel_charts: BTreeMap<String, Entity<FunnelChart>>,
    pub gauges: BTreeMap<String, Entity<Gauge>>,
    pub json_viewers: BTreeMap<String, Entity<JsonViewer>>,
    pub sankeys: BTreeMap<String, Entity<SankeyFlow>>,
    pub pipelines: BTreeMap<String, Entity<DataFlow>>,
    pub note_editors: BTreeMap<String, Entity<Editor>>,
    pub collapsed: BTreeMap<String, bool>,
    pub viewed_revision: BTreeMap<String, u64>,
    pub drafts: BTreeMap<String, BriefFormDraft>,
}

impl KarWorkbench {
    pub(crate) fn render_brief_cards_for_chat(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.active_briefs.is_empty() {
            return None;
        }
        let briefs = self.active_briefs.clone();
        let mut stack = div().w_full().flex().flex_col().gap(px(16.));
        for (idx, brief) in briefs.iter().enumerate() {
            let card = self.render_single_brief(brief, s, window, cx);
            stack = stack.child(arrive(
                card,
                ElementId::Name(format!("brief-arrive-{}-{}", brief.id, brief.latest_revision).into()),
                (idx.min(4) as u64) * 35,
                8.,
                cx,
            ));
        }
        Some(stack.into_any_element())
    }

    fn render_single_brief(
        &mut self,
        brief: &KarBrief,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let bid = brief.id.clone();
        let folded = self
            .brief_widgets
            .collapsed
            .get(&bid)
            .copied()
            .unwrap_or(false);
        let selected_rev = self
            .brief_widgets
            .viewed_revision
            .get(&bid)
            .copied()
            .filter(|&r| r >= 1 && r <= brief.latest_revision)
            .unwrap_or(brief.latest_revision);
        let snap: KarBriefRevision = brief
            .at_revision(Some(selected_rev))
            .unwrap_or_else(|| brief.current())
            .clone();

        let bid_toggle = bid.clone();
        let header = div()
            .w_full()
            .px(px(16.))
            .py(px(12.))
            .border_b_1()
            .border_color(if folded {
                transparent_black()
            } else {
                s.line_soft
            })
            .flex()
            .items_center()
            .justify_between()
            .gap(px(10.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .size(px(30.))
                            .flex_none()
                            .rounded(px(R_CTRL))
                            .bg(s.pro.accent_soft)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(render_brief_chart_icon(16., s.pro.accent_info)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(FS_SECTION))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(s.ink)
                                            .child(snap.title.clone()),
                                    )
                                    .child(
                                        div()
                                            .h(px(18.))
                                            .px(px(6.))
                                            .rounded(px(R_SM))
                                            .bg(s.soft)
                                            .font_features(tnum_features())
                                            .text_size(px(FS_TAG))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.muted)
                                            .flex()
                                            .items_center()
                                            .child(format!("v{}", snap.revision)),
                                    )
                                    .when_some(snap.badge.clone(), |row, badge| {
                                        row.child(
                                            div()
                                                .h(px(18.))
                                                .px(px(6.))
                                                .rounded(px(R_SM))
                                                .bg(s.pro.accent_soft)
                                                .text_size(px(FS_TAG))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(s.pro.accent_info)
                                                .flex()
                                                .items_center()
                                                .child(badge),
                                        )
                                    }),
                            )
                            .when(!snap.summary.is_empty(), |col| {
                                col.child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(FS_META))
                                        .text_color(s.muted)
                                        .child(snap.summary.clone()),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .when(brief.history.len() > 1, |row| {
                        let brief_id = bid.clone();
                        row.child(SelectionStrip::options(
                            SharedString::from(format!("brief-revisions-{bid}")),
                            "简报版本",
                            Group::Tabs,
                            brief
                                .history
                                .iter()
                                .map(|r| (r.revision, format!("v{}", r.revision))),
                            selected_rev,
                            s,
                            cx,
                            move |wb, revision, _, cx| {
                                wb.brief_widgets
                                    .viewed_revision
                                    .insert(brief_id.clone(), revision);
                                cx.notify();
                            },
                        ))
                    })
                    .child(
                        Tactile::ghost(SharedString::from(format!("brief-fold-{bid}")), s)
                            .size(px(CTRL_H))
                            .justify_center()
                            .tip(if folded { "展开简报" } else { "收起简报" })
                            .child(morph_glyph(
                                SharedString::from(format!("brief-chev-{bid}")),
                                [Glyph::Down, Glyph::Up],
                                !folded,
                                ICON_SM,
                                s.muted,
                                s.card,
                                window,
                                cx,
                            ))
                            .on_press(cx, move |this, _, cx| {
                                let next = !this
                                    .brief_widgets
                                    .collapsed
                                    .get(&bid_toggle)
                                    .copied()
                                    .unwrap_or(false);
                                this.brief_widgets.collapsed.insert(bid_toggle.clone(), next);
                                cx.notify();
                            }),
                    ),
            );

        let body_el = if !folded {
            Some(self.render_brief_blocks(brief, &snap, s, window, cx))
        } else {
            None
        };
        let body_fold = pro_fold(
            SharedString::from(format!("brief-body-fold-{bid}")),
            !folded,
            420.,
            0.,
            move || body_el.unwrap_or_else(|| div().into_any_element()),
        );

        div()
            .w_full()
            .rounded(px(R_CARD + 2.))
            .bg(s.card)
            .border_1()
            .border_color(s.line)
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(header)
            .child(body_fold)
    }

    fn render_brief_blocks(
        &mut self,
        brief: &KarBrief,
        snap: &KarBriefRevision,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut col = div()
            .w_full()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(14.));

        for (b_idx, block) in snap.blocks.iter().enumerate() {
            let key_prefix = format!("{}#v{}#b{}", brief.id, snap.revision, b_idx);
            match block {
                BriefBlock::Metrics { items } => {
                    let mut grid = div().w_full().flex().flex_wrap().gap(px(10.));
                    for (m_idx, m) in items.iter().enumerate() {
                        let ckey = format!("{key_prefix}#m{m_idx}");
                        let val = m.value;
                        let decimals = m.decimals;
                        let prefix = m.prefix.clone();
                        let suffix = m.suffix.clone();
                        let label = m.label.clone();
                        let counter = self
                            .brief_widgets
                            .counters
                            .entry(ckey)
                            .or_insert_with(|| {
                                cx.new(|_| {
                                    AnimatedCounter::new(val)
                                        .with_decimals(decimals)
                                        .with_prefix(prefix)
                                        .with_suffix(suffix)
                                        .with_size(24.)
                                })
                            })
                            .clone();
                        counter.update(cx, |c, cx| c.set_value(val, cx));

                        let delta_color = if m.positive {
                            s.pro.green_ink
                        } else {
                            s.pro.amber_ink
                        };
                        let delta_bg = if m.positive {
                            s.pro.green_soft
                        } else {
                            s.pro.amber_soft
                        };
                        grid = grid.child(
                            div()
                                .flex_1()
                                .min_w(px(150.))
                                .p(px(12.))
                                .rounded(px(R_CARD))
                                .bg(s.surface)
                                .border_1()
                                .border_color(s.line_soft)
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_size(px(FS_META))
                                        .text_color(s.muted)
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_baseline()
                                        .justify_between()
                                        .gap(px(6.))
                                        .child(counter)
                                        .when_some(m.delta.clone(), |row, d| {
                                            row.child(
                                                div()
                                                    .px(px(6.))
                                                    .py(px(2.))
                                                    .rounded(px(R_PILL))
                                                    .bg(delta_bg)
                                                    .font_features(tnum_features())
                                                    .text_size(px(FS_TAG))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(delta_color)
                                                    .child(d),
                                            )
                                        }),
                                ),
                        );
                    }
                    col = col.child(grid);
                }
                BriefBlock::BarChart { title, samples } => {
                    let chart = self
                        .brief_widgets
                        .bar_charts
                        .entry(key_prefix)
                        .or_insert_with(|| {
                            let pts = samples
                                .iter()
                                .map(|sp| {
                                    let mut p = Sample::new(sp.label.clone(), sp.value);
                                    if let Some(c) = sp.comparison {
                                        p = p.compare(c);
                                    }
                                    p
                                })
                                .collect();
                            cx.new(|_| BarChart::new(title.clone(), pts))
                        })
                        .clone();
                    col = col.child(div().w_full().child(chart));
                }
                BriefBlock::LineChart { title, samples } => {
                    let chart = self
                        .brief_widgets
                        .line_charts
                        .entry(key_prefix)
                        .or_insert_with(|| {
                            let pts = samples
                                .iter()
                                .map(|sp| {
                                    let mut p = Sample::new(sp.label.clone(), sp.value);
                                    if let Some(c) = sp.comparison {
                                        p = p.compare(c);
                                    }
                                    p
                                })
                                .collect();
                            cx.new(|_| LineChart::new(title.clone(), pts))
                        })
                        .clone();
                    col = col.child(div().w_full().child(chart));
                }
                BriefBlock::DonutChart { title, samples } => {
                    let chart = self
                        .brief_widgets
                        .donut_charts
                        .entry(key_prefix)
                        .or_insert_with(|| {
                            let pts = samples
                                .iter()
                                .map(|sp| Sample::new(sp.label.clone(), sp.value))
                                .collect();
                            cx.new(|_| DonutChart::new(title.clone(), pts))
                        })
                        .clone();
                    col = col.child(div().w_full().child(chart));
                }
                BriefBlock::Sparkline { title, samples } => {
                    let chart = self
                        .brief_widgets
                        .sparklines
                        .entry(key_prefix)
                        .or_insert_with(|| {
                            let pts = samples
                                .iter()
                                .map(|sp| Sample::new(sp.label.clone(), sp.value))
                                .collect();
                            cx.new(|_| Sparkline::new(title.clone(), pts))
                        })
                        .clone();
                    col = col.child(div().w_full().child(chart));
                }
                BriefBlock::RadarChart {
                    title,
                    axes,
                    series,
                } => {
                    let palette = *s;
                    let chart = self
                        .brief_widgets
                        .radar_charts
                        .entry(key_prefix)
                        .or_insert_with(|| {
                            let r_axes = axes
                                .iter()
                                .enumerate()
                                .map(|(i, a)| RadarAxis::new(format!("ax-{i}"), a.clone()))
                                .collect();
                            let r_profiles = series
                                .iter()
                                .map(|sr| {
                                    let color = match sr.tone {
                                        BriefTone::Info => palette.pro.blue,
                                        BriefTone::Success => palette.pro.teal,
                                        BriefTone::Warning => palette.pro.amber,
                                        BriefTone::Danger => palette.pro.red,
                                    };
                                    RadarProfile::new(
                                        sr.id.clone(),
                                        sr.label.clone(),
                                        sr.values.clone(),
                                        color,
                                    )
                                })
                                .collect();
                            cx.new(|_| RadarChart::new(title.clone(), r_axes, r_profiles))
                        })
                        .clone();
                    col = col.child(div().w_full().child(chart));
                }
                BriefBlock::FunnelChart {
                    title,
                    segments,
                    stages,
                } => {
                    let chart = self
                        .brief_widgets
                        .funnel_charts
                        .entry(key_prefix)
                        .or_insert_with(|| {
                            let f_segs = segments
                                .iter()
                                .enumerate()
                                .map(|(i, sg)| FunnelSegment::new(format!("seg-{i}"), sg.clone()))
                                .collect();
                            let f_stages = stages
                                .iter()
                                .map(|st| {
                                    FunnelStage::new(
                                        st.id.clone(),
                                        st.label.clone(),
                                        st.values.clone(),
                                    )
                                })
                                .collect();
                            cx.new(|cx| FunnelChart::new(title.clone(), f_stages, f_segs, cx))
                        })
                        .clone();
                    col = col.child(div().w_full().child(chart));
                }
                BriefBlock::Gauge {
                    label,
                    value,
                    detail,
                } => {
                    let gauge = self
                        .brief_widgets
                        .gauges
                        .entry(key_prefix)
                        .or_insert_with(|| {
                            cx.new(|_| {
                                Gauge::new(label.clone(), *value).with_detail(detail.clone())
                            })
                        })
                        .clone();
                    col = col.child(div().w_full().child(gauge));
                }
                BriefBlock::JsonTree { title, data } => {
                    let viewer = self
                        .brief_widgets
                        .json_viewers
                        .entry(key_prefix)
                        .or_insert_with(|| {
                            cx.new(|cx| {
                                JsonViewer::new(title.clone(), data.clone(), window, cx)
                                    .with_max_height(240.)
                            })
                        })
                        .clone();
                    col = col.child(div().w_full().child(viewer));
                }
                BriefBlock::Sankey {
                    title,
                    nodes,
                    links,
                } => {
                    let palette = *s;
                    let sankey = self
                        .brief_widgets
                        .sankeys
                        .entry(key_prefix)
                        .or_insert_with(|| {
                            let fnodes = nodes
                                .iter()
                                .map(|n| {
                                    let color = match n.tone {
                                        BriefTone::Info => palette.pro.blue,
                                        BriefTone::Success => palette.pro.teal,
                                        BriefTone::Warning => palette.pro.amber,
                                        BriefTone::Danger => palette.pro.red,
                                    };
                                    let mut node = FlowNode::new(n.id.clone(), n.label.clone(), color);
                                    if let Some(c) = n.column {
                                        node = node.with_column(c);
                                    }
                                    node
                                })
                                .collect();
                            let flinks = links
                                .iter()
                                .map(|l| FlowLink::new(l.source.clone(), l.target.clone(), l.value))
                                .collect();
                            cx.new(|_| SankeyFlow::new(title.clone(), fnodes, flinks).with_height(215.))
                        })
                        .clone();
                    col = col.child(div().w_full().child(sankey));
                }
                BriefBlock::Pipeline {
                    sources,
                    core,
                    destinations,
                    processed,
                    message,
                } => {
                    let flow = self
                        .brief_widgets
                        .pipelines
                        .entry(key_prefix)
                        .or_insert_with(|| {
                            let map_node = |n: &crate::brief::BriefPipeNode| {
                                DataFlowNode::new(n.id.clone(), n.label.clone())
                                    .with_meta(n.meta.clone())
                                    .with_detail(n.detail.clone())
                                    .with_share(n.share)
                            };
                            let srcs = sources.iter().map(map_node).collect();
                            let dsts = destinations.iter().map(map_node).collect();
                            let snap_df = DataFlowSnapshot::new(*processed, DataFlowStatus::Running)
                                .with_message(message.clone());
                            cx.new(|_| DataFlow::new(srcs, map_node(core), dsts, snap_df))
                        })
                        .clone();
                    col = col.child(div().w_full().child(flow));
                }
                BriefBlock::Callout { tone, title, body } => {
                    let (bg, icon, ic) = match tone {
                        BriefTone::Info => (s.pro.accent_soft, ProIcon::Sparkles, s.pro.accent_info),
                        BriefTone::Success => (s.pro.green_soft, ProIcon::Check, s.pro.green),
                        BriefTone::Warning => {
                            (s.pro.amber_soft, ProIcon::AlertTriangle, s.pro.amber)
                        }
                        BriefTone::Danger => (s.pro.red_soft, ProIcon::AlertTriangle, s.pro.red),
                    };
                    col = col.child(
                        div()
                            .w_full()
                            .px(px(12.))
                            .py(px(10.))
                            .rounded(px(R_CARD))
                            .bg(bg)
                            .flex()
                            .items_start()
                            .gap(px(9.))
                            .child(div().pt(px(2.)).flex_none().child(pro_icon(icon, ICON_SM, ic)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(FS_UI))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.ink)
                                            .child(title.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(FS_META))
                                            .line_height(px(LH_META))
                                            .text_color(s.text)
                                            .child(body.clone()),
                                    ),
                            ),
                    );
                }
                BriefBlock::Steps { items } => {
                    let mut list = div()
                        .w_full()
                        .p(px(12.))
                        .rounded(px(R_CARD))
                        .bg(s.surface)
                        .border_1()
                        .border_color(s.line_soft)
                        .flex()
                        .flex_col()
                        .gap(px(8.));
                    for st in items {
                        let (icon, color) = match st.state {
                            BriefStepState::Done => (ProIcon::Check, s.pro.green),
                            BriefStepState::Active => (ProIcon::Bolt, s.pro.accent_info),
                            BriefStepState::Pending => (ProIcon::Clock, s.muted),
                        };
                        list = list.child(
                            div()
                                .w_full()
                                .flex()
                                .items_start()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .pt(px(2.))
                                        .flex_none()
                                        .child(pro_icon(icon, ICON_SM, color)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(px(FS_UI))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(s.ink)
                                                .child(st.title.clone()),
                                        )
                                        .when(!st.detail.is_empty(), |d| {
                                            d.child(
                                                div()
                                                    .text_size(px(FS_META))
                                                    .text_color(s.muted)
                                                    .child(st.detail.clone()),
                                            )
                                        }),
                                ),
                        );
                    }
                    col = col.child(list);
                }
                BriefBlock::ReviewForm {
                    submit_label,
                    fields,
                } => {
                    col = col.child(self.render_brief_review_form(
                        brief,
                        snap,
                        submit_label,
                        fields,
                        s,
                        window,
                        cx,
                    ));
                }
            }
        }

        col.into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_brief_review_form(
        &mut self,
        brief: &KarBrief,
        snap: &KarBriefRevision,
        submit_label: &str,
        fields: &[crate::brief::BriefFormField],
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let bid = brief.id.clone();
        let rev_num = snap.revision;
        let already_submitted = brief.submitted_revisions.contains(&rev_num);
        let active_draft = self
            .brief_widgets
            .drafts
            .get(&bid)
            .cloned()
            .unwrap_or_default();
        let missing_required = active_draft
            .missing_required(snap)
            .map(|field| field.label.clone());
        let other_revision = active_draft
            .pinned_revision
            .is_some_and(|rev| rev != rev_num);
        let locked = already_submitted || other_revision || !self.access_mode.is_writable();
        let stale_pin = self
            .brief_widgets
            .drafts
            .get(&bid)
            .and_then(|d| d.has_stale_pin(brief.latest_revision));

        let mut form_box = div()
            .w_full()
            .p(px(14.))
            .rounded(px(R_CARD))
            .bg(s.surface)
            .border_1()
            .border_color(s.line)
            .flex()
            .flex_col()
            .gap(px(12.));

        if let Some(pinned_rev) = stale_pin {
            let bid_up = bid.clone();
            let latest_rev = brief.latest_revision;
            form_box = form_box.child(
                div()
                    .w_full()
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(px(R_CTRL))
                    .bg(s.pro.amber_soft)
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(FS_META))
                            .text_color(s.pro.amber_ink)
                            .child(format!(
                                "填写期间智能体发布了新版 v{latest_rev}；你的草稿已安全锁定在 v{pinned_rev}，不会被自动覆盖。"
                            )),
                    )
                    .child(
                        text_button(
                            SharedString::from(format!("brief-upgrade-pin-{bid}")),
                            format!("同步到 v{latest_rev}"),
                            s,
                        )
                        .on_press(cx, move |this, _, cx| {
                            if let Some(d) = this.brief_widgets.drafts.get_mut(&bid_up) {
                                d.upgrade_to_latest(latest_rev);
                            }
                            this.brief_widgets
                                .viewed_revision
                                .insert(bid_up.clone(), latest_rev);
                            cx.notify();
                        }),
                    ),
            );
        }

        for (f_ix, field) in fields.iter().enumerate() {
            let fid = field.id.clone();
            let mut field_col = div().w_full().flex().flex_col().gap(px(6.)).child(
                div()
                    .text_size(px(FS_UI))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.ink)
                    .child(if field.required {
                        format!("{} · 必填", field.label)
                    } else {
                        field.label.clone()
                    }),
            );

            match field.kind {
                BriefFieldKind::SingleChoice => {
                    let chosen = self
                        .brief_widgets
                        .drafts
                        .get(&bid)
                        .and_then(|d| d.single.get(&fid))
                        .cloned()
                        .or_else(|| field.default_value.clone());
                    let choices = field
                        .options
                        .iter()
                        .map(|option| {
                            let brief_id = bid.clone();
                            let field_id = fid.clone();
                            let value = option.clone();
                            Item::new(
                                option.clone(),
                                option.clone(),
                                chosen.as_ref() == Some(option),
                                cx,
                                move |wb, _, cx| {
                                    let draft = wb
                                        .brief_widgets
                                        .drafts
                                        .entry(brief_id.clone())
                                        .or_default();
                                    draft.touch_revision(rev_num);
                                    draft.single.insert(field_id.clone(), value.clone());
                                    cx.notify();
                                },
                            )
                        })
                        .collect();
                    field_col = field_col.child(
                        SelectionStrip::new(
                            SharedString::from(format!("brief-choice-{bid}-{fid}")),
                            field.label.clone(),
                            Group::Choices,
                            choices,
                            s,
                        )
                        .disabled(locked),
                    );
                }
                BriefFieldKind::MultiChoice => {
                    let selected_set = self
                        .brief_widgets
                        .drafts
                        .get(&bid)
                        .and_then(|d| d.multi.get(&fid))
                        .cloned()
                        .unwrap_or_else(|| field.default_value.iter().cloned().collect());
                    let mut opts = div().flex().flex_wrap().gap(px(6.));
                    for (o_ix, opt) in field.options.iter().enumerate() {
                        let checked = selected_set.contains(opt);
                        let initial_set = selected_set.clone();
                        let bid_c = bid.clone();
                        let fid_c = fid.clone();
                        let opt_c = opt.clone();
                        opts = opts.child(
                            soft_button(
                                SharedString::from(format!("bf-mc-{bid}-{f_ix}-{o_ix}")),
                                opt.clone(),
                                s,
                            )
                            .lit(checked)
                            .disabled(locked)
                            .child(morph_glyph(
                                SharedString::from(format!("bf-mc-g-{bid}-{f_ix}-{o_ix}")),
                                [Glyph::Plus, Glyph::Check],
                                checked,
                                ICON_XS,
                                if checked { s.ink } else { s.muted },
                                s.soft,
                                window,
                                cx,
                            ))
                            .on_press(cx, move |this, _, cx| {
                                let draft = this.brief_widgets.drafts.entry(bid_c.clone()).or_default();
                                draft.touch_revision(rev_num);
                                let set = draft
                                    .multi
                                    .entry(fid_c.clone())
                                    .or_insert_with(|| initial_set.clone());
                                if !set.remove(&opt_c) {
                                    set.insert(opt_c.clone());
                                }
                                cx.notify();
                            }),
                        );
                    }
                    field_col = field_col.child(opts);
                }
                BriefFieldKind::Boolean => {
                    let value = active_draft
                        .boolean
                        .get(&fid)
                        .copied()
                        .or_else(|| field.default_value.as_ref()?.parse().ok());
                    let choices = [
                        (true, self.ui_locale.pick("是", "Yes")),
                        (false, self.ui_locale.pick("否", "No")),
                    ]
                    .into_iter()
                    .map(|(answer, label)| {
                        let brief_id = bid.clone();
                        let field_id = fid.clone();
                        Item::new(
                            answer.to_string(),
                            label,
                            value == Some(answer),
                            cx,
                            move |wb, _, cx| {
                                let draft =
                                    wb.brief_widgets.drafts.entry(brief_id.clone()).or_default();
                                draft.touch_revision(rev_num);
                                draft.boolean.insert(field_id.clone(), answer);
                                cx.notify();
                            },
                        )
                    })
                    .collect();
                    field_col = field_col.child(
                        SelectionStrip::new(
                            SharedString::from(format!("brief-boolean-{bid}-{fid}")),
                            field.label.clone(),
                            Group::Choices,
                            choices,
                            s,
                        )
                        .disabled(locked),
                    );
                }
                BriefFieldKind::Note => {
                    let ekey = format!("{bid}#{rev_num}#note#{fid}");
                    let initial_note = active_draft
                        .notes
                        .get(&fid)
                        .or(field.default_value.as_ref())
                        .cloned()
                        .unwrap_or_default();
                    let bid_note = bid.clone();
                    let fid_note = fid.clone();
                    let editor = self
                        .brief_widgets
                        .note_editors
                        .entry(ekey)
                        .or_insert_with(|| {
                            let ed = cx.new(|cx| {
                                let mut e = Editor::new("补充说明", InputMode::Single, window, cx);
                                e.set_placeholder("输入补充意见或约束条件…");
                                e.set_value(initial_note, window, cx);
                                e.font_size = FS_UI;
                                e
                            });
                            cx.subscribe(&ed, move |this, ed_ent, ev: &EditEvent, cx| {
                                if *ev == EditEvent::Changed {
                                    let txt = ed_ent.read(cx).value().to_owned();
                                    let draft = this
                                        .brief_widgets
                                        .drafts
                                        .entry(bid_note.clone())
                                        .or_default();
                                    draft.touch_revision(rev_num);
                                    draft.notes.insert(fid_note.clone(), txt);
                                    cx.notify();
                                }
                            })
                            .detach();
                            ed
                        })
                        .clone();
                    editor.update(cx, |ed, cx| ed.set_read_only(locked, window, cx));
                    field_col = field_col.child(
                        div()
                            .w_full()
                            .h(px(34.))
                            .px(px(10.))
                            .rounded(px(R_CTRL))
                            .bg(s.card)
                            .border_1()
                            .border_color(s.line)
                            .flex()
                            .items_center()
                            .child(editor),
                    );
                }
            }

            form_box = form_box.child(field_col);
        }

        let bid_submit = bid.clone();
        let footer = div()
            .w_full()
            .pt(px(4.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_size(px(FS_META))
                    .text_color(if already_submitted {
                        s.pro.green_ink
                    } else {
                        s.muted
                    })
                    .child(pro_icon(
                        if already_submitted {
                            ProIcon::Check
                        } else {
                            ProIcon::Shield
                        },
                        ICON_XS,
                        if already_submitted {
                            s.pro.green
                        } else {
                            s.muted
                        },
                    ))
                    .child(if already_submitted {
                        format!("v{rev_num} 评审结论已送入对话队列")
                    } else if let Some(label) = &missing_required {
                        format!("请填写「{label}」")
                    } else {
                        format!("将以普通消息回传至对话（绑定 v{rev_num}）")
                    }),
            )
            .child(
                primary_button(
                    SharedString::from(format!("bf-submit-{bid}-{rev_num}")),
                    if already_submitted {
                        "已提交反馈".to_owned()
                    } else {
                        submit_label.to_owned()
                    },
                    s,
                )
                .disabled(locked || missing_required.is_some())
                .child(pro_icon(
                    if already_submitted {
                        ProIcon::Check
                    } else {
                        ProIcon::ArrowUp
                    },
                    ICON_XS,
                    s.on_primary,
                ))
                .on_press(cx, move |this, window, cx| {
                    this.submit_brief_review(&bid_submit, rev_num, window, cx);
                }),
            );

        form_box.child(footer)
    }
}
