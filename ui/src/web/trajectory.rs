//! Trajectory ledger, timing overview and local record inspector.

use super::*;
use crate::trajectory::{
    DetailTab, FoldDomain, Kind, Record, State, Visible, double_click_folds, fold_states,
    format_duration, toggle_global_fold, visible,
};
use gpui::canvas;
use std::cell::Cell;
use std::rc::Rc;

mod accessibility;
mod details;
pub(crate) mod json;
mod results;
mod timeline;

const HISTORY_ROW_HEIGHT: f32 = 30.;
const OLDER_HISTORY_THRESHOLD: f32 = 48.;
const OVERSCAN: f32 = 360.;

struct LedgerPosition {
    key: String,
    summary: bool,
    top: f32,
    bottom: f32,
}

struct LedgerRecord<'a> {
    display: Visible<'a>,
    turn_start: bool,
    foldable: Option<FoldDomain>,
    boundary: Option<&'a Record>,
}

enum LedgerCell<'a> {
    Record(LedgerRecord<'a>),
    Request(&'a Record),
}

impl<'a> LedgerCell<'a> {
    fn record(&self) -> &'a Record {
        match self {
            Self::Record(row) => row.display.record,
            Self::Request(record) => record,
        }
    }

    fn summary(&self) -> bool {
        matches!(self, Self::Record(row) if row.display.summary.is_some())
    }
}

struct LedgerRow<'a> {
    cell: LedgerCell<'a>,
    top: f32,
    height: f32,
}

fn ledger_rows<'a>(state: &'a State, query: &str, start: f32) -> Vec<LedgerRow<'a>> {
    let fold_actions = double_click_folds(&state.records, &state.folded);
    let mut first_in_step = HashMap::new();
    for record in &state.records {
        if !matches!(record.kind, Kind::User | Kind::Context | Kind::System) {
            first_in_step
                .entry((record.turn, record.step))
                .or_insert(record.key.as_str());
        }
    }
    let boundaries: HashMap<_, _> = state
        .requests
        .iter()
        .map(|request| ((request.turn, request.step), request))
        .collect();
    let mut request_only: Vec<_> = if query.trim().is_empty() {
        boundaries
            .iter()
            .filter_map(|(step, request)| (!first_in_step.contains_key(step)).then_some(*request))
            .collect()
    } else {
        Vec::new()
    };
    request_only.sort_by_key(|request| (request.turn, request.step));
    let mut request_only = request_only.into_iter().peekable();
    let mut rows = Vec::new();
    let mut top = start;
    let mut push = |cell, height| {
        rows.push(LedgerRow { cell, top, height });
        top += height;
    };
    let mut current_turn = 0;
    for row in visible(&state.records, &state.requests, &state.folded, query) {
        let record = row.record;
        let step = (record.turn, record.step);
        while let Some(request) =
            request_only.next_if(|request| (request.turn, request.step) <= step)
        {
            push(LedgerCell::Request(request), 0.);
        }
        let turn_start = record.turn > 0 && record.turn != current_turn;
        current_turn = record.turn;
        let boundary = (row.summary.is_none()
            && first_in_step.get(&step) == Some(&record.key.as_str()))
        .then_some(boundaries.get(&step).copied())
        .flatten();
        let height = if row.summary.is_some() { 20. } else { 30. };
        push(
            LedgerCell::Record(LedgerRecord {
                display: row,
                turn_start,
                foldable: fold_actions.get(record.key.as_str()).copied(),
                boundary,
            }),
            height,
        );
    }
    for request in request_only {
        push(LedgerCell::Request(request), 9.);
    }
    rows
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ConversationView {
    Chat,
    Trajectory,
}

pub(super) struct TrajectoryView {
    pub(super) active: ConversationView,
    owner: Option<String>,
    coding_view: bool,
    pub(super) search: String,
    pub(super) scroll: ScrollHandle,
    timeline_focus: gpui::FocusHandle,
    _timeline_focus: [Subscription; 2],
    inspector_scroll: ScrollHandle,
    inspector_focus: gpui::FocusHandle,
    close_focus: gpui::FocusHandle,
    detail_tab: DetailTab,
    detail_tab_history: Vec<DetailTab>,
    result_watch: Option<results::ResultWatch>,
    result_watch_connected: bool,
    detail_tab_focus: std::cell::RefCell<HashMap<DetailTab, gpui::FocusHandle>>,
    details_width: Option<f32>,
    tool_request_offset: Option<f32>,
    split_width: f32,
    thinking_disclosure: Option<(String, bool)>,
    unix_timestamp: bool,
    record_focus: Rc<std::cell::RefCell<std::collections::HashMap<String, gpui::FocusHandle>>>,
    pub(super) selected: Option<String>,
    pub(super) actual_duration: bool,
    pub(super) actual_time: bool,
    pub(super) range: Option<(f64, f64)>,
    range_indexes: Option<std::collections::HashSet<usize>>,
    focus_range: bool,
    focused_record: Option<String>,
    positions: Vec<LedgerPosition>,
    prepend_anchor: Option<(String, bool, f32)>,
    ledger_height: Rc<Cell<f32>>,
    follow: bool,
}

impl TrajectoryView {
    pub(super) fn new(w: &mut Window, cx: &mut Context<DshApp>) -> Self {
        let timeline_focus = cx.focus_handle().tab_stop(true);
        let timeline_focus_in = cx.on_focus(&timeline_focus, w, |_, _, cx| cx.notify());
        let timeline_focus_out = cx.on_blur(&timeline_focus, w, |_, _, cx| cx.notify());
        Self {
            active: ConversationView::Chat,
            owner: None,
            coding_view: false,
            search: String::new(),
            scroll: ScrollHandle::new(),
            timeline_focus,
            _timeline_focus: [timeline_focus_in, timeline_focus_out],
            inspector_scroll: ScrollHandle::new(),
            inspector_focus: cx.focus_handle(),
            close_focus: cx.focus_handle().tab_stop(true),
            detail_tab: DetailTab::Overview,
            detail_tab_history: Vec::new(),
            result_watch: None,
            result_watch_connected: false,
            detail_tab_focus: Default::default(),
            details_width: None,
            tool_request_offset: None,
            split_width: 0.,
            thinking_disclosure: None,
            unix_timestamp: false,
            record_focus: Default::default(),
            selected: None,
            actual_duration: false,
            actual_time: false,
            range: None,
            range_indexes: None,
            focus_range: false,
            focused_record: None,
            positions: Vec::new(),
            prepend_anchor: None,
            ledger_height: Rc::new(Cell::new(0.)),
            follow: true,
        }
    }

    fn reset(&mut self) {
        self.selected = None;
        self.detail_tab = DetailTab::Overview;
        self.detail_tab_history.clear();
        self.thinking_disclosure = None;
        self.detail_tab_focus.borrow_mut().clear();
        self.record_focus.borrow_mut().clear();
        self.range = None;
        self.range_indexes = None;
        self.focus_range = false;
        self.focused_record = None;
        self.positions.clear();
        self.prepend_anchor = None;
        self.follow = true;
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
    }

    pub(super) fn preserve_viewport_on_prepend(&mut self) {
        if self.active != ConversationView::Trajectory {
            return;
        }
        let top = -f32::from(self.scroll.offset().y);
        self.prepend_anchor = self
            .positions
            .iter()
            .find(|row| row.bottom > top)
            .map(|row| (row.key.clone(), row.summary, top - row.top));
        self.follow = false;
    }
}

impl DshApp {
    pub(super) fn sync_conversation_view(&mut self) {
        if self.trajectory.owner == self.active_session
            && self.trajectory.coding_view == self.developer_tools
        {
            return;
        }
        self.trajectory.owner.clone_from(&self.active_session);
        self.trajectory.coding_view = self.developer_tools;
        let preferred = self
            .active_session
            .as_deref()
            .map(bridge::preferred_conversation_view);
        self.trajectory.active =
            if self.developer_tools && preferred.as_deref() == Some("trajectory") {
                ConversationView::Trajectory
            } else {
                ConversationView::Chat
            };
    }

    pub(super) fn open_conversation_view(
        &mut self,
        view: ConversationView,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if view == ConversationView::Trajectory && !self.developer_tools {
            return;
        }
        self.trajectory.active = view;
        if let Some(session) = &self.active_session {
            bridge::remember_conversation_view(
                session,
                if view == ConversationView::Trajectory {
                    "trajectory"
                } else {
                    "chat"
                },
            );
        }
        if view == ConversationView::Trajectory {
            self.focus_trajectory_search(w, cx);
        } else {
            self.focus_composer(w, cx);
        }
        cx.notify();
    }

    fn focus_trajectory_search(&self, w: &mut Window, cx: &mut Context<Self>) {
        self.trajectory.timeline_focus.focus(w, cx);
        timeline::focus_search();
    }

    fn clear_trajectory_viewport(&mut self, cx: &mut Context<Self>) {
        self.trajectory.range = None;
        self.trajectory.range_indexes = None;
        self.trajectory.focus_range = false;
        cx.notify();
    }

    pub(super) fn render_conversation_view_switcher(
        &self,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = dsh(cx);
        let active = self.trajectory.active;
        row()
            .id("conversation-view-switcher")
            .role(gpui::Role::TabList)
            .flex_none()
            .w_full()
            .h(px(35.))
            .gap(px(32.))
            .pl(px(28.))
            .children(
                [ConversationView::Chat, ConversationView::Trajectory].map(|view| {
                    let (id, label) = match view {
                        ConversationView::Chat => ("chat", "对话"),
                        ConversationView::Trajectory => ("trajectory", "轨迹"),
                    };
                    row()
                        .relative()
                        .id(gpui::ElementId::Name(
                            format!("conversation-view-{id}").into(),
                        ))
                        .role(gpui::Role::Tab)
                        .aria_label(label)
                        .aria_selected(active == view)
                        .track_focus(&self.trajectory_action_focus("view", id, cx))
                        .tab_index(0)
                        .h(px(35.))
                        .mx(px(-10.))
                        .px(px(10.))
                        .pt(px(10.))
                        .pb(px(9.))
                        .line_height(px(16.))
                        .font_weight(FontWeight::MEDIUM)
                        .cursor_pointer()
                        .text_size(px(13.))
                        .text_color(if active == view {
                            p.state_business_primary
                        } else {
                            p.label_tertiary
                        })
                        .when(active == view, |tab| {
                            tab.child(
                                div()
                                    .absolute()
                                    .left(px(10.))
                                    .right(px(10.))
                                    .bottom(px(-1.))
                                    .h(px(2.))
                                    .rounded(px(2.))
                                    .bg(p.state_business_primary),
                            )
                        })
                        .child(label)
                        .child(self.trajectory_accessible(
                            "view",
                            id,
                            label,
                            Some(active == view),
                            None,
                            false,
                        ))
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.open_conversation_view(view, w, cx)
                        }))
                        .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, w, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.open_conversation_view(view, w, cx);
                                cx.stop_propagation();
                            }
                        }))
                }),
            )
    }

    pub(super) fn trajectory_fold_states(&self) -> [bool; 3] {
        let state = &self.conversation.trajectory;
        fold_states(&state.records, &state.folded)
    }

    fn toggle_all_trajectory_folds(&mut self, domain: FoldDomain, cx: &mut Context<Self>) {
        let state = &mut self.conversation.trajectory;
        toggle_global_fold(&mut state.folded, &state.records, domain);
        cx.notify();
    }

    fn toggle_trajectory_fold(&mut self, key: &str, domain: FoldDomain, cx: &mut Context<Self>) {
        let state = &mut self.conversation.trajectory;
        let Some(record) = state.records.iter().find(|record| record.key == key) else {
            return;
        };
        let key = domain.key(record);
        if !state.folded.remove(&key) {
            state.folded.insert(key);
        }
        cx.notify();
    }

    fn close_trajectory_inspector(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.trajectory.selected.take() else {
            return;
        };
        if let Some(focus) = self.trajectory.record_focus.borrow().get(&key).cloned() {
            focus.focus(w, cx);
        }
        w.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }

    fn select_trajectory_record(&mut self, key: String, w: &mut Window, cx: &mut Context<Self>) {
        let request = self
            .conversation
            .trajectory
            .requests
            .iter()
            .any(|record| record.key == key);
        if let Some(record) = self
            .conversation
            .trajectory
            .records
            .iter()
            .chain(&self.conversation.trajectory.requests)
            .find(|record| record.key == key)
        {
            self.trajectory.detail_tab =
                record.preferred_detail_tab(request, &self.trajectory.detail_tab_history);
            if request {
                self.trajectory
                    .detail_tab_history
                    .retain(|tab| *tab != DetailTab::Overview);
                self.trajectory.detail_tab_history.push(DetailTab::Overview);
            }
        }
        self.trajectory.selected = Some(key);
        self.trajectory.inspector_focus.focus(w, cx);
        self.trajectory
            .inspector_scroll
            .set_offset(gpui::point(px(0.), px(0.)));
        if self
            .trajectory
            .range_indexes
            .as_ref()
            .is_some_and(|indexes| {
                self.conversation
                    .trajectory
                    .records
                    .iter()
                    .find(|record| Some(&record.key) == self.trajectory.selected.as_ref())
                    .is_some_and(|record| !indexes.contains(&record.index))
            })
        {
            self.trajectory.range = None;
            self.trajectory.range_indexes = None;
        }
        cx.notify();
    }

    fn trajectory_request_boundary(
        &self,
        record: &Record,
        narrow: bool,
        empty_step: bool,
        covered: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = dsh(cx);
        let key = record.key.clone();
        let keyboard_key = record.key.clone();
        row()
            .id(gpui::ElementId::Name(
                format!("request-boundary-{key}").into(),
            ))
            .absolute()
            .top(px(-8.))
            .left(px(if narrow { 6. } else { 12. }))
            .size(px(16.))
            .flex_none()
            .items_start()
            .role(gpui::Role::Button)
            .aria_label(format!(
                "第 {} 轮第 {} 步模型请求",
                record.turn, record.step
            ))
            .child(self.trajectory_accessible(
                "record",
                record.key.clone(),
                format!("第 {} 轮第 {} 步模型请求", record.turn, record.step),
                None,
                None,
                covered,
            ))
            .tab_index(0)
            .justify_center()
            .track_focus(&self.trajectory_action_focus("record", &record.key, cx))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, w, cx| {
                this.select_trajectory_record(key.clone(), w, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, w, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.select_trajectory_record(keyboard_key.clone(), w, cx);
                    w.prevent_default();
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .absolute()
                    .top(px(if empty_step { 13.5 } else { 5.5 }))
                    .left(px(5.5))
                    .size(px(5.))
                    .rounded(px(3.))
                    .bg(p.label_caption)
                    .border(px(2.))
                    .border_color(p.bg_layer_1),
            )
    }

    // TrajectoryTable.module.css kindTag colors, including its sRGB color-mix.
    fn trajectory_kind_badge(
        &self,
        record: &Record,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let p = dsh(cx);
        let (ink, fill) = match record.kind {
            Kind::User => (p.state_business_primary, p.state_business_tertiary),
            Kind::Message => (
                p.state_error_secondary.blend(p.brand_accent.opacity(0.60)),
                p.bg_layer_1.blend(
                    p.state_error_secondary
                        .blend(p.brand_accent.opacity(0.55))
                        .opacity(0.15),
                ),
            ),
            Kind::Tool => (p.state_warn_label, p.state_warn_tertiary),
            Kind::Subtool => (
                p.label_tertiary.blend(p.state_warn_label.opacity(0.62)),
                p.bg_layer_1.blend(p.state_warn_tertiary.opacity(0.58)),
            ),
            Kind::Context => (
                p.label_secondary
                    .blend(p.state_success_primary.opacity(0.68)),
                p.state_success_tertiary,
            ),
            Kind::System | Kind::Compacted => (p.label_secondary, p.bg_module_platform),
        };
        let glyph: &'static [u8] = match record.kind {
            Kind::User => include_bytes!("../../assets/trajectory-user.svg"),
            Kind::Message => include_bytes!("../../assets/trajectory-assistant.svg"),
            Kind::Tool | Kind::Subtool => include_bytes!("../../assets/trajectory-tool.svg"),
            Kind::Context => include_bytes!("../../assets/trajectory-context.svg"),
            Kind::System => include_bytes!("../../assets/trajectory-system.svg"),
            Kind::Compacted => include_bytes!("../../assets/trajectory-compacted.svg"),
        };
        row()
            .gap_0()
            .items_center()
            .justify_center()
            .flex_none()
            .h(px(19.))
            .border_1()
            .border_color(gpui::transparent_black())
            .rounded(px(2.))
            .bg(fill)
            .text_color(ink)
            .text_size(px(10.))
            .line_height(px(16.))
            .font_weight(FontWeight(650.))
            .when(compact, |badge| {
                badge.w(px(19.)).child(
                    gpui::svg()
                        .data(glyph)
                        .size(px(if record.kind == Kind::Context {
                            14.
                        } else {
                            13.
                        }))
                        .flex_none()
                        .text_color(ink),
                )
            })
            .when(!compact, |badge| badge.px(px(5.)).child(record.label()))
    }

    fn trajectory_row(
        &self,
        ledger_record: LedgerRecord<'_>,
        narrow: bool,
        request_width: f32,
        covered: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let LedgerRecord {
            display:
                Visible {
                    record,
                    summary,
                    summary_domain,
                },
            turn_start,
            foldable,
            boundary,
        } = ledger_record;
        let p = dsh(cx);
        let collapsed = summary.is_some();
        let selected = !collapsed && self.trajectory.selected.as_deref() == Some(&record.key);
        let key = record.key.clone();
        let click_key = key.clone();
        let double_click_key = key.clone();
        let keyboard_key = key.clone();
        let focus_key = summary_domain
            .map_or_else(|| key.clone(), |domain| format!("summary:{domain:?}:{key}"));
        let focus = self
            .trajectory
            .record_focus
            .borrow()
            .get(&focus_key)
            .cloned()
            .unwrap_or_else(|| cx.focus_handle().tab_stop(true));
        self.trajectory
            .record_focus
            .borrow_mut()
            .insert(focus_key, focus.clone());
        let double_click_focus = focus.clone();
        let kind = (!collapsed).then(|| self.trajectory_kind_badge(record, narrow, cx));
        let boundary = boundary
            .map(|request| self.trajectory_request_boundary(request, narrow, false, covered, cx));
        let text = summary.unwrap_or_else(|| {
            let text = record.preview();
            if text.is_empty() {
                if record.kind == Kind::Message {
                    "（仅工具调用）"
                } else {
                    "—"
                }
                .to_owned()
            } else {
                text
            }
        });
        let accessible_label = if collapsed {
            format!("展开 {text}")
        } else {
            format!("#{} {} {text}", record.index, record.label())
        };
        let accessible_action = match summary_domain {
            Some(FoldDomain::Turn) => "fold-turn",
            Some(FoldDomain::Calls) => "fold-calls",
            Some(FoldDomain::Subcalls) => "fold-subcalls",
            None => "record",
        };
        let content = if !collapsed && matches!(record.kind, Kind::Tool | Kind::Subtool) {
            let args = record.tool_arguments_preview();
            let result = record.result_preview();
            let request = row()
                .gap_0()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .line_height(px(18.))
                .font_family(crate::web::document_fonts::code_family(cx))
                .map(|mut element| {
                    element.text_style().font_fallbacks =
                        Some(crate::web::document_fonts::code_fallbacks(cx));
                    element
                })
                .when(
                    crate::trajectory::program::is_program_tool(&record.name),
                    |request| {
                        request.child(
                            gpui::svg()
                                .data(include_bytes!("../../assets/trajectory-code.svg"))
                                .size(px(12.))
                                .mr(px(4.))
                                .flex_none()
                                .text_color(p.label_secondary),
                        )
                    },
                )
                .child(div().flex_none().child(record.name.clone()))
                .when(!args.is_empty(), |request| {
                    request.child(
                        div()
                            .ml(px(
                                if crate::trajectory::program::is_program_tool(&record.name) {
                                    5.
                                } else {
                                    7.
                                },
                            ))
                            .when(
                                crate::trajectory::program::is_program_tool(&record.name),
                                |args| {
                                    args.font_family(
                                        document_fonts::interface_family(cx)
                                            .unwrap_or_else(|| "system-ui".into()),
                                    )
                                },
                            )
                            .min_w_0()
                            .truncate()
                            .text_color(p.label_secondary)
                            .child(args),
                    )
                });
            row()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .gap(px(8.))
                .when(record.kind == Kind::Subtool, |content| content.pl(px(18.)))
                .child(request.when(result.is_some(), |request| {
                    request.w(px(request_width)).flex_none()
                }))
                .when_some(result, |content, result| {
                    content.child(
                        row()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .gap(px(8.))
                            .font_family(crate::web::document_fonts::code_family(cx))
                            .map(|mut element| {
                                element.text_style().font_fallbacks =
                                    Some(crate::web::document_fonts::code_fallbacks(cx));
                                element
                            })
                            .text_color(if record.failed() {
                                p.danger
                            } else {
                                p.label_secondary
                            })
                            .child(div().flex_none().text_color(p.label_caption).child("→"))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .whitespace_nowrap()
                                    .when(result == "无输出", |text| {
                                        text.text_color(p.label_caption)
                                    })
                                    .child(result),
                            ),
                    )
                })
        } else {
            row()
                .flex_1()
                .min_w_0()
                .gap(px(6.))
                .text_color(if collapsed { p.label_secondary } else { p.ink })
                .when(collapsed, |content| {
                    content.child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.label_tertiary)
                            .child("…"),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .whitespace_nowrap()
                        .child(text.clone()),
                )
        }
        .text_size(px(12.))
        .line_height(px(16.));
        row()
            .relative()
            .id(gpui::ElementId::Name(
                format!(
                    "trajectory-{}-{}",
                    if collapsed { "summary" } else { "row" },
                    record.key
                )
                .into(),
            ))
            .role(gpui::Role::Button)
            .aria_label(accessible_label.clone())
            .aria_selected(selected)
            .track_focus(&focus)
            .tab_index(0)
            .focus_visible(|row| row.border(px(1.)).border_color(p.state_business_primary))
            .w_full()
            .h(px(if collapsed { 20. } else { 30. }))
            .overflow_hidden()
            .items_center()
            .pl(px(if narrow { 28. } else { 36. }))
            .pr(px(8.))
            .gap(px(if narrow { 7. } else { 8. }))
            .border_b(px(0.5))
            .border_color(p.border_l1)
            .when(selected, |row| row.bg(p.hover_active))
            .when(collapsed, |row| row.cursor_pointer())
            .hover(|row| row.bg(p.hover))
            .when(selected, |row| {
                row.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .w(px(3.))
                        .bg(p.brand_accent),
                )
            })
            .when(turn_start && !collapsed, |row| {
                row.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .px(px(5.))
                        .py(px(1.))
                        .rounded_br(px(2.))
                        .bg(p.bg_module_platform)
                        .font_family(crate::web::document_fonts::code_family(cx))
                        .map(|mut element| {
                            element.text_style().font_fallbacks =
                                Some(crate::web::document_fonts::code_fallbacks(cx));
                            element
                        })
                        .text_size(px(8.))
                        .line_height(px(10.))
                        .text_color(p.label_tertiary)
                        .child(if narrow {
                            format!("#{}", record.turn)
                        } else {
                            format!("第 {} 轮", record.turn)
                        }),
                )
            })
            .child(
                row()
                    .w(px(if narrow { 19. } else { 44. }))
                    .flex_none()
                    .justify_end()
                    .children(kind),
            )
            .child(content)
            .child(
                div()
                    .w(px(14.))
                    .flex_none()
                    .when(record.failed(), |row| row.text_color(p.danger).child("!"))
                    .when(record.pending(), |row| {
                        row.text_color(p.state_business_primary).child("●")
                    }),
            )
            .children(boundary)
            .child(self.trajectory_accessible(
                accessible_action,
                record.key.clone(),
                accessible_label,
                (!collapsed).then_some(selected),
                collapsed.then_some(false),
                covered,
            ))
            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, w, cx| {
                if event.click_count() == 2
                    && let Some(domain) = foldable
                {
                    this.toggle_trajectory_fold(&double_click_key, domain, cx);
                    double_click_focus.focus(w, cx);
                } else if let Some(domain) = summary_domain {
                    this.toggle_trajectory_fold(&click_key, domain, cx);
                } else {
                    this.select_trajectory_record(key.clone(), w, cx);
                }
            }))
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, w, cx| {
                if event.keystroke.key == "tab"
                    && !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.alt
                    && !event.keystroke.modifiers.platform
                {
                    if event.keystroke.modifiers.shift {
                        w.focus_prev(cx);
                    } else {
                        w.focus_next(cx);
                    }
                    w.prevent_default();
                    cx.stop_propagation();
                } else if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    if let Some(domain) = summary_domain {
                        this.toggle_trajectory_fold(&keyboard_key, domain, cx);
                    } else {
                        this.select_trajectory_record(keyboard_key.clone(), w, cx);
                    }
                    w.prevent_default();
                    cx.stop_propagation();
                }
            }))
    }

    fn load_trajectory_history(&mut self, cx: &mut Context<Self>) {
        if self.conversation.history_has_more {
            self.request_older_history(cx);
        } else if self.conversation.history_error.is_some() {
            self.request_session_history(cx);
        }
    }

    fn measure_trajectory_ledger(&self, cx: &Context<Self>) -> impl IntoElement {
        let height = self.trajectory.ledger_height.clone();
        let view = cx.weak_entity();
        canvas(
            move |bounds, w, cx| {
                let measured = f32::from(bounds.size.height);
                if (height.replace(measured) - measured).abs() > 0.1 {
                    let view = view.clone();
                    w.defer(cx, move |_, cx| {
                        let _ = view.update(cx, |_, cx| cx.notify());
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    }

    fn on_trajectory_scroll(
        &mut self,
        event: &gpui::ScrollEvent,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(w.line_height()).y;
        if delta == px(0.) {
            return;
        }
        // The native scroll listener owns the offset. Reconcile after it handles the gesture.
        self.trajectory.follow = false;
        self.trajectory.prepend_anchor = None;
        let view = cx.weak_entity();
        w.defer(cx, move |_, cx| {
            let _ = view.update(cx, |app, cx| {
                let scroll = &app.trajectory.scroll;
                let top = -f32::from(scroll.offset().y);
                if delta < px(0.) {
                    app.trajectory.follow = f32::from(scroll.max_offset().y) - top <= 2.;
                } else if top <= OLDER_HISTORY_THRESHOLD && app.conversation.history_error.is_none()
                {
                    app.request_older_history(cx);
                }
                cx.notify();
            });
        });
    }

    pub(super) fn render_trajectory(
        &mut self,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = dsh(cx);
        let query = self.trajectory.search.clone();
        let timeline_source = self.trajectory_timeline_source(&query);
        self.trajectory_timeline_focus(&timeline_source);
        let loading = self.conversation.history_loading;
        let has_more = self.conversation.history_has_more;
        let history_error = self.conversation.history_error.is_some();
        let show_history_control = has_more
            || history_error
            || (loading && self.conversation.trajectory.events.is_empty());
        let history_height = if show_history_control {
            HISTORY_ROW_HEIGHT
        } else {
            0.
        };
        let rows = ledger_rows(&self.conversation.trajectory, &query, history_height);
        // With an inspector open the composer has a separate flex row and both
        // detail and ledger scrollers end at its top edge.
        let composer_clearance = if self.trajectory.selected.is_some() {
            0.
        } else {
            self.composer_seat_bounds
                .get()
                .map_or(168., |bounds| f32::from(bounds.size.height) + 16.)
        };
        let ledger_end = rows
            .last()
            .map_or(history_height + 180., |row| row.top + row.height);
        let content_height = ledger_end + composer_clearance;
        let viewport = self.trajectory.ledger_height.get();
        let viewport = if viewport > 0. {
            viewport
        } else {
            f32::from(w.viewport_size().height)
        };
        let max_top = (content_height - viewport).max(0.);
        let scroll = &self.trajectory.scroll;
        let current_top = -f32::from(scroll.offset().y);
        let mut top = if self.trajectory.follow {
            max_top
        } else {
            current_top
        };
        if let Some((key, summary, offset)) = self.trajectory.prepend_anchor.take()
            && let Some(row) = rows
                .iter()
                .find(|row| row.cell.record().key == key && row.cell.summary() == summary)
        {
            top = row.top + offset;
        }
        if std::mem::take(&mut self.trajectory.focus_range)
            && let Some(indexes) = &self.trajectory.range_indexes
        {
            let focused: Vec<_> = rows
                .iter()
                .filter(|row| {
                    !row.cell.summary()
                        && matches!(row.cell, LedgerCell::Record(_))
                        && indexes.contains(&row.cell.record().index)
                })
                .collect();
            if let (Some(first), Some(last)) = (focused.first(), focused.last()) {
                self.trajectory.follow = false;
                top = if last.top + last.height - first.top > viewport {
                    first.top
                } else {
                    let middle = focused[(focused.len() - 1) / 2];
                    middle.top + (middle.height - viewport) / 2.
                };
            }
        }
        if let Some(key) = self.trajectory.focused_record.take()
            && let Some(row) = rows.iter().find(|row| match &row.cell {
                LedgerCell::Record(row) => {
                    row.display.record.key == key
                        || row.boundary.is_some_and(|request| request.key == key)
                }
                LedgerCell::Request(request) => request.key == key,
            })
        {
            self.trajectory.follow = false;
            top = row.top + (row.height - viewport) / 2.;
        }
        top = top.clamp(0., max_top);
        if (top - current_top).abs() > 0.1 {
            scroll.set_offset(gpui::point(px(0.), px(-top)));
        }
        self.trajectory.positions = rows
            .iter()
            .filter(|row| row.height > 0.)
            .map(|row| LedgerPosition {
                key: row.cell.record().key.clone(),
                summary: row.cell.summary(),
                top: row.top,
                bottom: row.top + row.height,
            })
            .collect();

        let viewport_width = f32::from(w.viewport_size().width);
        let view_width = viewport_width - self.sidebar_track_width(w) - self.right_dock_frame.track;
        let inspector_overlays = viewport_width <= 760.;
        let split_width = view_width.max(0.);
        self.trajectory.split_width = split_width;
        let inspector_width = if inspector_overlays {
            (view_width * 0.92).min(420.)
        } else {
            self.trajectory
                .details_width
                .unwrap_or((split_width * 0.38).clamp(320., 440.))
                .min((split_width - 280.).max(0.))
        };
        // Upstream uses the table pane's container width, including the exact
        // 620px boundary, independently of the window's overlay breakpoint.
        let table_width = view_width
            - if self.trajectory.selected.is_some() && !inspector_overlays {
                inspector_width
            } else {
                0.
            };
        let narrow = table_width <= 620.;
        let request_width = self
            .trajectory
            .tool_request_offset
            .map_or(table_width * 0.36 - 56., |offset| {
                table_width * 0.58 - offset
            })
            .clamp(180., 480.);
        let mut body = div().relative().w_full().flex_none().h(px(content_height));
        if show_history_control && top <= HISTORY_ROW_HEIGHT + OVERSCAN {
            let label = if loading {
                if has_more {
                    "正在加载更早的历史…"
                } else {
                    "正在加载轨迹…"
                }
            } else if history_error {
                "重新加载历史"
            } else {
                "加载更早的历史"
            };
            let button = row()
                .id("trajectory-load-earlier")
                .role(if loading {
                    gpui::Role::Status
                } else {
                    gpui::Role::Button
                })
                .aria_label(label)
                .absolute()
                .top_0()
                .w_full()
                .h(px(HISTORY_ROW_HEIGHT))
                .justify_center()
                .items_center()
                .text_size(px(12.))
                .text_color(p.secondary)
                .child(label)
                .when(!loading, |button| {
                    button
                        .tab_index(0)
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| this.load_trajectory_history(cx)))
                });
            body = body.child(button);
        }
        let first = rows.partition_point(|row| row.top + row.height < top - OVERSCAN);
        for LedgerRow {
            cell,
            top: row_top,
            height,
        } in rows
            .into_iter()
            .skip(first)
            .take_while(|row| row.top <= top + viewport + OVERSCAN)
        {
            let dimmed = !cell.summary()
                && self
                    .trajectory
                    .range_indexes
                    .as_ref()
                    .is_some_and(|indexes| !indexes.contains(&cell.record().index));
            let element = match cell {
                LedgerCell::Record(row) => self
                    .trajectory_row(
                        row,
                        narrow,
                        request_width,
                        inspector_overlays && self.trajectory.selected.is_some(),
                        cx,
                    )
                    .into_any_element(),
                LedgerCell::Request(request) => self
                    .trajectory_request_boundary(
                        request,
                        narrow,
                        true,
                        inspector_overlays && self.trajectory.selected.is_some(),
                        cx,
                    )
                    .into_any_element(),
            };
            body = body.child(
                div()
                    .absolute()
                    .top(px(row_top))
                    .w_full()
                    .h(px(height))
                    .when(dimmed, |row| row.opacity(0.24))
                    .child(element),
            );
        }
        if self.trajectory.positions.is_empty() && !loading {
            body = body.child(
                row()
                    .absolute()
                    .top(px(history_height))
                    .w_full()
                    .h(px(180.))
                    .justify_center()
                    .items_center()
                    .text_color(p.faint)
                    .text_size(px(13.))
                    .child(if !query.is_empty() {
                        "没有匹配的轨迹记录"
                    } else {
                        "暂无可查看的轨迹"
                    }),
            );
        }
        let inspector = self.trajectory.selected.clone().and_then(|key| {
            let record = self
                .conversation
                .trajectory
                .records
                .iter()
                .chain(&self.conversation.trajectory.requests)
                .find(|record| record.key == key)?;
            Some(self.render_trajectory_inspector(record, inspector_overlays, inspector_width, cx))
        });
        let table = column()
            .gap_0()
            .id("trajectory-table")
            .role(gpui::Role::Table)
            .aria_label("会话轨迹")
            .flex_1()
            .w_full()
            .h_full()
            .min_w_0()
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(
                        div()
                            .id("trajectory-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.trajectory.scroll)
                            .on_scroll(cx.listener(Self::on_trajectory_scroll))
                            .child(body),
                    )
                    .child(self.measure_trajectory_ledger(cx))
                    .child(super::workspace::scrollbar_overlay(
                        "trajectory-scrollbar",
                        &self.trajectory.scroll,
                        cx,
                    )),
            );
        column()
            .gap_0()
            .relative()
            .id("trajectory-view")
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(p.bg_layer_1)
            .child(self.trajectory_timeline(timeline_source, cx))
            .child(
                row()
                    .gap_0()
                    .relative()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(table)
                    .children(inspector),
            )
    }
}

pub(super) fn reset_on_switch(view: &mut TrajectoryView) {
    view.reset();
}
