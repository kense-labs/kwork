//! Trajectory ledger, timing overview and local record inspector.

use super::*;
use crate::trajectory::{
    Kind, Record, State, Visible, first_turn_content, format_duration, global_fold,
    toggle_global_fold, turn_folded, visible,
};
use gpui::{DispatchPhase, Entity, MouseButton, MouseMoveEvent, MouseUpEvent, canvas};
use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ConversationView {
    Chat,
    Trajectory,
}

pub(super) struct TrajectoryView {
    pub(super) active: ConversationView,
    pub(super) search: Entity<Editor>,
    _search: Subscription,
    pub(super) scroll: ScrollHandle,
    timeline: ScrollHandle,
    timeline_focus: gpui::FocusHandle,
    timeline_bounds: Rc<Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    inspector_scroll: ScrollHandle,
    inspector_focus: gpui::FocusHandle,
    close_focus: gpui::FocusHandle,
    record_focus: Rc<std::cell::RefCell<std::collections::HashMap<String, gpui::FocusHandle>>>,
    pub(super) selected: Option<String>,
    pub(super) actual_duration: bool,
    pub(super) actual_time: bool,
    pub(super) range: Option<(f32, f32)>,
    viewport: Option<(f32, f32)>,
    visible: usize,
    dragging: Option<(f32, f32)>,
    panning: Option<(f32, f32)>,
    focused_record: Option<(String, usize)>,
    follow: bool,
}

impl TrajectoryView {
    pub(super) fn new(w: &mut Window, cx: &mut Context<DshApp>) -> Self {
        let search = editor("搜索轨迹".into(), InputMode::Search, w, cx);
        search.update(cx, |input, _| input.font_size = 12.);
        let subscription = cx.subscribe_in(&search, w, |this, _, event: &EditEvent, w, cx| {
            match event {
                EditEvent::Changed => {
                    this.trajectory.visible = 100;
                    this.trajectory
                        .scroll
                        .set_offset(gpui::point(px(0.), px(0.)));
                }
                EditEvent::Escape => replace(&this.trajectory.search, "", w, cx),
                _ => {}
            }
            cx.notify();
        });
        Self {
            active: ConversationView::Chat,
            search,
            _search: subscription,
            scroll: ScrollHandle::new(),
            timeline: ScrollHandle::new(),
            timeline_focus: cx.focus_handle(),
            timeline_bounds: Rc::new(Cell::new(None)),
            inspector_scroll: ScrollHandle::new(),
            inspector_focus: cx.focus_handle(),
            close_focus: cx.focus_handle(),
            record_focus: Default::default(),
            selected: None,
            actual_duration: false,
            actual_time: false,
            range: None,
            viewport: None,
            visible: 100,
            dragging: None,
            panning: None,
            focused_record: None,
            follow: true,
        }
    }

    fn reset(&mut self) {
        self.selected = None;
        self.record_focus.borrow_mut().clear();
        self.range = None;
        self.viewport = None;
        self.dragging = None;
        self.panning = None;
        self.focused_record = None;
        self.visible = 100;
        self.follow = true;
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
    }
}

impl DshApp {
    pub(super) fn trajectory_follow_tail(&self) -> bool {
        self.trajectory.follow
    }

    pub(super) fn open_conversation_view(
        &mut self,
        view: ConversationView,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.trajectory.active = view;
        if view == ConversationView::Trajectory {
            self.focus_trajectory_search(w, cx);
            self.settle_trajectory_scroll(cx);
        } else {
            self.focus_composer(w, cx);
        }
        cx.notify();
    }

    fn settle_trajectory_scroll(&mut self, cx: &mut Context<Self>) {
        if !self.trajectory.follow {
            return;
        }
        let session = self.active_session.clone();
        self.trajectory.scroll.scroll_to_bottom();
        cx.spawn(async move |this, cx| {
            let mut confirmations = 0;
            for _ in 0..10 {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
                let settled = this.update(cx, |app, cx| {
                    if app.active_session != session || !app.trajectory.follow {
                        return None;
                    }
                    let max = app.trajectory.scroll.max_offset().y;
                    if max <= px(0.) {
                        return Some(false);
                    }
                    if app.trajectory.scroll.offset().y + max <= px(1.) {
                        return Some(true);
                    }
                    app.trajectory.scroll.scroll_to_bottom();
                    cx.notify();
                    Some(false)
                });
                match settled.ok().flatten() {
                    Some(true) => {
                        confirmations += 1;
                        if confirmations >= 2 {
                            break;
                        }
                    }
                    Some(false) => confirmations = 0,
                    None => break,
                }
            }
        })
        .detach();
    }

    fn focus_trajectory_search(&self, w: &mut Window, cx: &mut Context<Self>) {
        self.trajectory
            .search
            .update(cx, |editor, cx| editor.focus(w, cx));
    }

    fn trajectory_pointer_capture(&self, cx: &Context<Self>) -> gpui::Canvas<()> {
        let view = cx.weak_entity();
        canvas(
            |_, _, _| {},
            move |_, _, w, _| {
                let move_view = view.clone();
                w.on_mouse_event(move |event: &MouseMoveEvent, phase, w, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    let Some(view) = move_view.upgrade() else {
                        return;
                    };
                    let active = {
                        let this = view.read(cx);
                        this.trajectory.active == ConversationView::Trajectory
                            && (this.trajectory.dragging.is_some()
                                || this.trajectory.panning.is_some())
                    };
                    if active {
                        let event = event.clone();
                        w.defer(cx, move |_, cx| {
                            view.update(cx, |this, cx| {
                                this.move_trajectory_pointer(&event, cx);
                            });
                        });
                    }
                });
                w.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture
                        || !matches!(event.button, MouseButton::Left | MouseButton::Right)
                    {
                        return;
                    }
                    let Some(view) = view.upgrade() else {
                        return;
                    };
                    let active = {
                        let this = view.read(cx);
                        this.trajectory.active == ConversationView::Trajectory
                            && (this.trajectory.dragging.is_some()
                                || this.trajectory.panning.is_some())
                    };
                    if active {
                        let event = event.clone();
                        view.update(cx, |this, cx| {
                            this.release_trajectory_pointer(&event, cx);
                        });
                    }
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        // Global mouse listeners are registered during paint; this canvas must not
        // cover the timeline and record controls that follow it in the layout.
        .size_0()
    }

    fn move_trajectory_pointer(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(bounds) = self.trajectory.timeline_bounds.get() else {
            return;
        };
        let origin = f32::from(bounds.origin.x);
        let width = f32::from(bounds.size.width).max(1.);
        if let Some((anchor, start)) = self.trajectory.panning {
            let (left, right) = self.trajectory.viewport.unwrap_or((0., 1.));
            let length = right - left;
            let delta = (f32::from(event.position.x) - anchor) / width * length;
            let left = (start - delta).clamp(0., 1. - length);
            self.trajectory.viewport = Some((left, left + length));
        } else if let Some((anchor, domain)) = self.trajectory.dragging {
            let (left, right) = self.trajectory.viewport.unwrap_or((0., 1.));
            let start = left + (anchor - domain) / width * (right - left);
            let end = left + (f32::from(event.position.x) - origin) / width * (right - left);
            self.trajectory.range = Some((start.clamp(0., 1.), end.clamp(0., 1.)));
        } else {
            return;
        }
        cx.notify();
    }

    fn release_trajectory_pointer(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let positions = crate::trajectory::time_spans(
            &self.conversation.trajectory,
            self.trajectory.actual_time,
            self.trajectory.actual_duration,
        );
        match event.button {
            MouseButton::Left if self.trajectory.dragging.is_some() => {
                if let Some((anchor, _)) = self.trajectory.dragging {
                    let range = self.trajectory.range.take().unwrap_or((0., 0.));
                    let range = crate::trajectory::ordered_range(range);
                    let click = (f32::from(event.position.x) - anchor).abs() < 3.;
                    self.trajectory.range =
                        if click || range.1 - range.0 < 1. / positions.len().max(1) as f32 {
                            Some(crate::trajectory::minimum_range(
                                (range.0 + range.1) / 2.,
                                positions.len(),
                            ))
                        } else {
                            Some(range)
                        };
                    if click {
                        let nearest = self
                            .conversation
                            .trajectory
                            .requests
                            .iter()
                            .chain(&self.conversation.trajectory.records)
                            .filter_map(|record| {
                                let span = positions.get(&record.key)?;
                                let distance =
                                    (range.0 - span.end).max(span.start - range.0).max(0.);
                                Some((distance, record.index, record.key.clone()))
                            })
                            .min_by(|left, right| {
                                left.0
                                    .total_cmp(&right.0)
                                    .then_with(|| left.1.cmp(&right.1))
                            });
                        if let Some((_, _, key)) = nearest {
                            self.trajectory.selected = None;
                            self.trajectory.visible = 100;
                            self.trajectory.follow = false;
                            self.trajectory.focused_record = Some((key, 0));
                        }
                    }
                }
                self.trajectory.dragging = None;
            }
            MouseButton::Right if self.trajectory.panning.is_some() => {
                if let Some((anchor, _)) = self.trajectory.panning
                    && (f32::from(event.position.x) - anchor).abs() < 3.
                {
                    self.trajectory.range = None;
                }
                self.trajectory.panning = None;
            }
            _ => return,
        }
        cx.notify();
    }

    fn clear_trajectory_viewport(&mut self, cx: &mut Context<Self>) {
        self.trajectory.range = None;
        self.trajectory.viewport = None;
        self.trajectory.dragging = None;
        self.trajectory.panning = None;
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
            .gap(px(2.))
            .p(px(2.))
            .rounded(px(8.))
            .bg(p.bg_module_platform)
            .children(
                [ConversationView::Chat, ConversationView::Trajectory].map(|view| {
                    let (id, label) = match view {
                        ConversationView::Chat => ("chat", "聊天"),
                        ConversationView::Trajectory => ("trajectory", "轨迹"),
                    };
                    row()
                        .id(gpui::ElementId::Name(
                            format!("conversation-view-{id}").into(),
                        ))
                        .role(gpui::Role::Tab)
                        .aria_label(label)
                        .aria_selected(active == view)
                        .tab_index(0)
                        .h(px(24.))
                        .px(px(9.))
                        .justify_center()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .text_size(px(12.))
                        .text_color(if active == view { p.ink } else { p.faint })
                        .when(active == view, |tab| tab.bg(p.bg_layer_1))
                        .hover(|tab| tab.text_color(p.ink))
                        .child(label)
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

    fn trajectory_toolbar(
        &self,
        state: &State,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = dsh(cx);
        let first_in_turn = first_turn_content(&state.records);
        let all = |kind: Kind| {
            let records: Vec<_> = state
                .records
                .iter()
                .filter(|record| {
                    record.has_children
                        && (record.kind == kind
                            || (kind == Kind::User
                                && global_fold(record, true) == "$turn"
                                && first_in_turn.get(&record.turn) == Some(&record.key.as_str())))
                })
                .collect();
            let global = match kind {
                Kind::User => "$turn",
                Kind::Tool => "$call",
                Kind::Subtool => "$subcall",
                _ => "",
            };
            !records.is_empty()
                && records.iter().all(|record| {
                    state.folded.contains(global) || state.folded.contains(&record.key)
                })
        };
        let button = |id: &'static str,
                      label: &'static str,
                      active: bool,
                      action: fn(&mut DshApp, &mut Context<DshApp>)| {
            row()
                .id(id)
                .role(gpui::Role::Button)
                .aria_label(label)
                .aria_selected(active)
                .tab_index(0)
                .h(px(24.))
                .px(px(6.))
                .gap(px(4.))
                .rounded(px(5.))
                .cursor_pointer()
                .text_size(px(12.))
                .text_color(if active { p.ink } else { p.faint })
                .when(active, |button| button.bg(p.hover))
                .hover(|button| button.bg(p.hover).text_color(p.ink))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    action(this, cx);
                }))
                .capture_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, w, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space")
                        && !event.keystroke.modifiers.modified()
                        && !w.default_prevented()
                    {
                        action(this, cx);
                        w.prevent_default();
                        cx.stop_propagation();
                    }
                }))
        };
        let turns = all(Kind::User);
        let calls = all(Kind::Tool);
        let subcalls = all(Kind::Subtool);
        row()
            .id("trajectory-toolbar")
            .role(gpui::Role::Toolbar)
            .aria_label("轨迹工具栏")
            .w_full()
            .min_h(px(34.))
            .px(px(6.))
            .py(px(4.))
            .gap(px(4.))
            .flex_wrap()
            .border_b(px(0.5))
            .border_color(p.border_l2)
            .child(button(
                "trajectory-duration",
                "时长",
                self.trajectory.actual_duration,
                |this, cx| {
                    this.trajectory.actual_duration = !this.trajectory.actual_duration;
                    this.clear_trajectory_viewport(cx);
                },
            ))
            .child(button(
                "trajectory-fold-turns",
                if turns {
                    "展开轮次"
                } else {
                    "收起轮次"
                },
                turns,
                |this, cx| this.toggle_trajectory_fold("$turn", cx),
            ))
            .child(button(
                "trajectory-fold-calls",
                if calls {
                    "展开调用"
                } else {
                    "收起调用"
                },
                calls,
                |this, cx| this.toggle_trajectory_fold("$call", cx),
            ))
            .child(button(
                "trajectory-fold-subcalls",
                if subcalls {
                    "展开子调用"
                } else {
                    "收起子调用"
                },
                subcalls,
                |this, cx| this.toggle_trajectory_fold("$subcall", cx),
            ))
            .child(
                row()
                    .id("trajectory-search-field")
                    .ml_auto()
                    .w(px(164.))
                    .h(px(24.))
                    .px(px(6.))
                    .gap(px(4.))
                    .rounded(px(5.))
                    .border(px(0.5))
                    .border_color(p.border_l4)
                    .bg(p.bg_layer_2)
                    .child(icon(Glyph::Search, 12., p.label_caption))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(self.trajectory.search.clone()),
                    )
                    .on_click(cx.listener(|this, _, w, cx| this.focus_trajectory_search(w, cx))),
            )
    }

    fn toggle_trajectory_fold(&mut self, key: &str, cx: &mut Context<Self>) {
        if key.starts_with('$') {
            toggle_global_fold(
                &mut self.conversation.trajectory.folded,
                &self.conversation.trajectory.records,
                key,
            );
            cx.notify();
            return;
        }
        let first_in_turn = first_turn_content(&self.conversation.trajectory.records);
        let Some(record) = self
            .conversation
            .trajectory
            .records
            .iter()
            .find(|record| record.key == key)
        else {
            return;
        };
        let global = global_fold(record, first_in_turn.get(&record.turn) == Some(&key));
        if global.is_empty() {
            return;
        }
        let collapsed = record.has_children
            && (self.conversation.trajectory.folded.contains(global)
                || self.conversation.trajectory.folded.contains(key));
        if collapsed {
            if self.conversation.trajectory.folded.remove(global) {
                let keys: Vec<String> = self
                    .conversation
                    .trajectory
                    .records
                    .iter()
                    .filter(|record| {
                        record.has_children
                            && global_fold(
                                record,
                                first_in_turn.get(&record.turn) == Some(&record.key.as_str()),
                            ) == global
                    })
                    .map(|record| record.key.clone())
                    .collect();
                self.conversation.trajectory.folded.extend(keys);
            }
            self.conversation.trajectory.folded.remove(key);
        } else {
            self.conversation.trajectory.folded.insert(key.to_owned());
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
        self.trajectory.selected = Some(key);
        self.trajectory.inspector_focus.focus(w, cx);
        self.trajectory
            .inspector_scroll
            .set_offset(gpui::point(px(0.), px(0.)));
        if self.trajectory.range.take().is_some() {
            self.trajectory.viewport = None;
        }
        cx.notify();
    }

    fn trajectory_request_boundary(
        &self,
        record: &Record,
        narrow: bool,
        empty_step: bool,
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
            .tab_index(0)
            .justify_center()
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

    fn trajectory_row(
        &self,
        record: &Record,
        summary: Option<String>,
        narrow: bool,
        turn_start: bool,
        request_boundary: Option<Record>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = dsh(cx);
        let collapsed = summary.is_some();
        let selected = !collapsed && self.trajectory.selected.as_deref() == Some(&record.key);
        let foldable = record.has_children
            && !global_fold(
                record,
                first_turn_content(&self.conversation.trajectory.records).get(&record.turn)
                    == Some(&record.key.as_str()),
            )
            .is_empty();
        let (ink, fill) = match record.kind {
            Kind::User => (p.state_success_primary, p.state_success_tertiary),
            Kind::Tool | Kind::Subtool => (p.state_warn_label, p.state_warn_tertiary),
            Kind::Message => (p.brand_accent, p.state_business_tertiary),
            Kind::Context | Kind::Compacted => {
                (p.state_success_secondary, p.state_success_tertiary)
            }
            Kind::System => (p.secondary, p.bg_module_platform),
        };
        let key = record.key.clone();
        let click_key = key.clone();
        let double_click_key = key.clone();
        let keyboard_key = key.clone();
        let focus = self
            .trajectory
            .record_focus
            .borrow()
            .get(&key)
            .cloned()
            .unwrap_or_else(|| cx.focus_handle());
        self.trajectory
            .record_focus
            .borrow_mut()
            .insert(key.clone(), focus.clone());
        let kind = || {
            let glyph = match record.kind {
                Kind::User => "↑",
                Kind::Message => "◆",
                Kind::Tool | Kind::Subtool => "⌘",
                Kind::Context | Kind::Compacted => "⊞",
                Kind::System => "•",
            };
            div()
                .h(px(19.))
                .rounded(px(2.))
                .bg(fill)
                .text_size(px(10.))
                .line_height(px(16.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(ink)
                .when(narrow, |kind| kind.w(px(19.)).text_center().child(glyph))
                .when(!narrow, |kind| kind.px(px(5.)).child(record.label()))
        };
        let boundary = request_boundary
            .as_ref()
            .map(|request| self.trajectory_request_boundary(request, narrow, false, cx));
        let text = summary.unwrap_or_else(|| {
            let text = record.preview();
            if text.is_empty() {
                "无内容".to_owned()
            } else {
                text
            }
        });
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
            .aria_label(if collapsed {
                format!("展开 {}", text)
            } else {
                format!("#{} {} {}", record.index, record.label(), record.input)
            })
            .aria_selected(selected)
            .track_focus(&focus)
            .tab_index(0)
            .focus_visible(|row| row.border(px(1.)).border_color(p.state_business_primary))
            .w_full()
            .h(px(if collapsed { 20. } else { 30. }))
            .overflow_hidden()
            .items_center()
            .pl(px(if narrow { 28. } else { 36. }))
            .pr(px(if narrow { 3. } else { 4. }))
            .gap(px(4.))
            .border_b(px(0.5))
            .border_color(p.border_l1)
            .when(selected, |row| row.bg(p.hover_active))
            .cursor_pointer()
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
                        .font_family("monospace")
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
                    .children((!collapsed).then(kind)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .whitespace_nowrap()
                    .text_size(px(12.))
                    .text_color(p.ink)
                    .child(text),
            )
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
            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, w, cx| {
                if event.click_count() == 2 && foldable {
                    this.toggle_trajectory_fold(&double_click_key, cx);
                } else if collapsed {
                    this.toggle_trajectory_fold(&click_key, cx);
                } else {
                    this.select_trajectory_record(key.clone(), w, cx);
                }
            }))
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, w, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    if collapsed {
                        this.toggle_trajectory_fold(&keyboard_key, cx);
                    } else {
                        this.select_trajectory_record(keyboard_key.clone(), w, cx);
                    }
                    w.prevent_default();
                    cx.stop_propagation();
                }
            }))
    }

    fn trajectory_timeline(&self, state: &State, cx: &mut Context<Self>) -> gpui::Div {
        let p = dsh(cx);
        let lanes = [
            (Kind::User, "输入"),
            (Kind::Message, "模型"),
            (Kind::Tool, "工具"),
        ];
        let labels = column()
            .w(px(44.))
            .h_full()
            .py(px(7.))
            .gap(px(6.))
            .border_r(px(0.5))
            .border_color(p.border_l1)
            .children(lanes.map(|(_, label)| {
                row()
                    .h(px(8.))
                    .pr(px(3.))
                    .justify_end()
                    .text_size(px(10.))
                    .text_color(p.label_caption)
                    .child(label)
            }));
        let mut track = div()
            .id("trajectory-timeline-track")
            .role(gpui::Role::Slider)
            .aria_label("时间线概览；左键拖动选择，右键拖动平移，Escape 重置")
            .aria_value(
                self.trajectory
                    .range
                    .map(|(start, end)| format!("{:.0}% 至 {:.0}%", start * 100., end * 100.))
                    .unwrap_or_else(|| "全部记录".into()),
            )
            .tab_index(0)
            .track_focus(&self.trajectory.timeline_focus)
            .relative()
            .w_full()
            .h_full()
            .cursor_crosshair()
            .track_scroll(&self.trajectory.timeline)
            .child(
                canvas(
                    {
                        let bounds = self.trajectory.timeline_bounds.clone();
                        move |rect, _, _| bounds.set(Some(rect))
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.clear_trajectory_viewport(cx);
                    cx.stop_propagation();
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                this.move_trajectory_pointer(event, cx);
            }))
            .on_scroll(cx.listener(move |this, event: &gpui::ScrollEvent, _, cx| {
                let Some(bounds) = this.trajectory.timeline_bounds.get() else {
                    return;
                };
                let width = f32::from(bounds.size.width).max(1.);
                let (left, right) = this.trajectory.viewport.unwrap_or((0., 1.));
                let length = right - left;
                let delta = event.delta.pixel_delta(px(20.)).y.as_f32();
                let length = (length * (delta * 0.0015).exp()).clamp(0.04, 1.);
                let origin = f32::from(bounds.origin.x);
                let anchor = ((f32::from(event.position.x) - origin) / width).clamp(0., 1.);
                let left = (left + anchor * (right - left - length)).clamp(0., 1. - length);
                this.trajectory.viewport = (length < 0.999).then_some((left, left + length));
                cx.stop_propagation();
                cx.notify();
            }))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, w, cx| {
                    w.focus(&this.trajectory.timeline_focus, cx);
                    this.trajectory.panning = None;
                    this.trajectory.dragging = Some((
                        f32::from(event.position.x),
                        this.trajectory
                            .timeline_bounds
                            .get()
                            .map_or(f32::from(event.position.x), |bounds| {
                                f32::from(bounds.origin.x)
                            }),
                    ));
                    this.trajectory.range = Some(this.trajectory.timeline_bounds.get().map_or(
                        (0., 0.),
                        |bounds| {
                            let width = f32::from(bounds.size.width).max(1.);
                            let (left, right) = this.trajectory.viewport.unwrap_or((0., 1.));
                            let at = left
                                + (f32::from(event.position.x) - f32::from(bounds.origin.x))
                                    / width
                                    * (right - left);
                            (at.clamp(0., 1.), at.clamp(0., 1.))
                        },
                    ));
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|this, event: &gpui::MouseDownEvent, w, cx| {
                    w.focus(&this.trajectory.timeline_focus, cx);
                    this.trajectory.dragging = None;
                    this.trajectory.panning = Some((
                        f32::from(event.position.x),
                        this.trajectory.viewport.unwrap_or((0., 1.)).0,
                    ));
                    cx.stop_propagation();
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                    this.release_trajectory_pointer(event, cx);
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Right,
                cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                    this.release_trajectory_pointer(event, cx);
                }),
            )
            .on_mouse_up_out(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                    this.release_trajectory_pointer(event, cx);
                }),
            )
            .on_mouse_up_out(
                gpui::MouseButton::Right,
                cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                    this.release_trajectory_pointer(event, cx);
                }),
            );
        let width = self
            .trajectory
            .timeline_bounds
            .get()
            .map_or(0., |bounds| f32::from(bounds.size.width).max(2.));
        if let Some((start, end)) = self.trajectory.range {
            let (start, end) = crate::trajectory::ordered_range((start, end));
            let left = start * width;
            let selection_width = ((end - start) * width).max(1.);
            track = track.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(left))
                    .w(px(selection_width))
                    .border_x(px(if self.trajectory.dragging.is_some() {
                        2.
                    } else {
                        3.
                    }))
                    .border_color(p.state_business_primary)
                    .bg(p
                        .state_business_primary
                        .opacity(if self.trajectory.dragging.is_some() {
                            0.18
                        } else {
                            0.12
                        })),
            );
        }
        let records: Vec<_> = state
            .requests
            .iter()
            .chain(&state.records)
            .filter(|record| record.at_ms.is_some())
            .collect();
        let positions = crate::trajectory::time_spans(
            state,
            self.trajectory.actual_time,
            self.trajectory.actual_duration,
        );
        for record in records {
            let selected = self.trajectory.range.is_none_or(|range| {
                let (start, end) = crate::trajectory::ordered_range(range);
                positions
                    .get(&record.key)
                    .is_some_and(|span| span.start <= end && span.end >= start)
            });
            let lane = match record.kind {
                Kind::User | Kind::Context | Kind::System => 0,
                Kind::Message | Kind::Compacted => 1,
                Kind::Tool | Kind::Subtool => 2,
            };
            let Some(span) = positions.get(&record.key).copied() else {
                continue;
            };
            let (start, end) = self.trajectory.viewport.unwrap_or((0., 1.));
            if span.end < start || span.start > end {
                continue;
            }
            let x = ((span.start - start) / (end - start).max(0.001) * width)
                .clamp(0., (width - 2.).max(0.));
            let span_width = if !self.trajectory.actual_time || !self.trajectory.actual_duration {
                3.
            } else {
                ((span.end - span.start) / (end - start).max(0.001) * width)
                    .max(2.)
                    .min(width - x)
            };
            let color = if lane == 0 {
                p.state_business_primary
            } else if lane == 1 {
                p.brand_accent
            } else {
                p.state_warn_label
            };
            let key = record.key.clone();
            track = track.child(
                div()
                    .id(gpui::ElementId::Name(
                        format!("trajectory-timeline-{}", record.key).into(),
                    ))
                    .role(gpui::Role::Button)
                    .aria_label(format!(
                        "{} {}",
                        record.label(),
                        format_duration(record.timing.duration_ms())
                    ))
                    .tab_index(0)
                    .absolute()
                    .left(px(x))
                    .top(px(7. + lane as f32 * 14.))
                    .w(px(span_width))
                    .h(px(8.))
                    .rounded(px(1.))
                    .bg(color)
                    .opacity(if selected { 1. } else { 0.2 })
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_, _, _, cx| cx.stop_propagation()),
                    )
                    .on_click(cx.listener(move |this, _, w, cx| {
                        cx.stop_propagation();
                        this.trajectory.dragging = None;
                        this.select_trajectory_record(key.clone(), w, cx);
                    })),
            );
        }
        row()
            .w_full()
            .h(px(50.))
            .bg(p.bg_layer_2)
            .border_b(px(0.5))
            .border_color(p.border_l2)
            .child(labels)
            .child(track)
    }

    pub(super) fn render_trajectory(
        &mut self,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = dsh(cx);
        let query = self.trajectory.search.read(cx).value().to_owned();
        let positions = crate::trajectory::time_spans(
            &self.conversation.trajectory,
            self.trajectory.actual_time,
            self.trajectory.actual_duration,
        );
        let in_range = |record: &Record| {
            self.trajectory.range.is_none_or(|range| {
                let (start, end) = crate::trajectory::ordered_range(range);
                positions
                    .get(&record.key)
                    .is_some_and(|span| span.start <= end && span.end >= start)
            })
        };
        let rows = visible(
            &self.conversation.trajectory.records,
            &self.conversation.trajectory.folded,
            &query,
        )
        .into_iter()
        .filter(|row| in_range(row.record))
        .collect::<Vec<_>>();
        let request_boundaries: Vec<_> = self
            .conversation
            .trajectory
            .requests
            .iter()
            .filter(|record| {
                in_range(record)
                    && !turn_folded(
                        &self.conversation.trajectory.records,
                        &self.conversation.trajectory.folded,
                        record.turn,
                    )
            })
            .collect();
        // Official request-only rows exist only for steps without an assistant
        // response or tool call. Their metadata still supplies every boundary.
        let requests: Vec<_> =
            request_boundaries
                .iter()
                .filter(|request| {
                    query.trim().is_empty()
                        && !self.conversation.trajectory.records.iter().any(|record| {
                            record.turn == request.turn && record.step == request.step
                        })
                })
                .copied()
                .collect();
        let composer_clearance = self
            .composer_seat_bounds
            .get()
            .map_or(168., |bounds| f32::from(bounds.size.height) + 16.);
        let mut body = column()
            .w_full()
            .flex_none()
            .gap_0()
            .pb(px(composer_clearance));
        if self.conversation.history_has_more || rows.len() > self.trajectory.visible {
            body = body.child(
                row()
                    .id("trajectory-load-earlier")
                    .role(gpui::Role::Button)
                    .aria_label("加载更早的历史")
                    .tab_index(0)
                    .w_full()
                    .h(px(30.))
                    .justify_center()
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(p.secondary)
                    .child(if self.conversation.history_loading {
                        "正在加载更早的历史…"
                    } else {
                        "加载更早的历史"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.trajectory.visible <= this.conversation.trajectory.records.len() {
                            this.trajectory.visible += 100;
                        } else {
                            this.request_older_history(cx);
                        }
                        cx.notify();
                    })),
            );
        }
        let start = rows.len().saturating_sub(self.trajectory.visible);
        let row_count = rows.len();
        let visible_requests: Vec<_> = rows
            .iter()
            .filter_map(|row| {
                let request = requests.iter().find(|request| {
                    request.turn == row.record.turn && request.step == row.record.step
                })?;
                self.conversation
                    .trajectory
                    .records
                    .iter()
                    .find(|candidate| {
                        candidate.turn == request.turn
                            && candidate.step == request.step
                            && !matches!(candidate.kind, Kind::User | Kind::Context | Kind::System)
                    })
                    .map_or(Some((*request).clone()), |content| {
                        (content.key == row.record.key).then(|| (*request).clone())
                    })
            })
            .collect();
        let narrow = f32::from(w.viewport_size().width)
            - self.sidebar_track_width(w)
            - self.right_dock_frame.track
            < 620.;
        let mut current_turn = 0;
        let mut current_step = 0;
        let mut shown = 0;
        let mut last_step = None;
        let mut scroll_index =
            usize::from(self.conversation.history_has_more || rows.len() > self.trajectory.visible);
        let mut focused_record = self
            .trajectory
            .focused_record
            .take()
            .map(|(key, _)| (key, None));
        let display_rows: Vec<_> = rows.into_iter().skip(start).collect();
        let final_step = display_rows
            .last()
            .map(|row| (row.record.turn, row.record.step));
        for Visible {
            record, summary, ..
        } in display_rows
        {
            shown += 1;
            last_step = Some((record.turn, record.step));
            let record_scroll_index = scroll_index;
            let has_request = requests
                .iter()
                .any(|request| request.turn == record.turn && request.step == record.step);
            scroll_index +=
                usize::from(has_request && record.step > 0 && record.step != current_step) + 1;
            if focused_record
                .as_ref()
                .is_some_and(|(key, _)| key == &record.key)
            {
                focused_record.as_mut().unwrap().1 = Some(record_scroll_index);
            }
            if focused_record.as_ref().is_some_and(|(key, _)| {
                visible_requests.iter().any(|request| {
                    request.turn == record.turn
                        && request.step == record.step
                        && &request.key == key
                })
            }) {
                focused_record.as_mut().unwrap().1 = Some(record_scroll_index + 1);
            }
            let turn_start = record.turn > 0 && record.turn != current_turn;
            let request_boundary =
                (!matches!(record.kind, Kind::User | Kind::Context | Kind::System)
                    && summary.is_none()
                    && !self
                        .conversation
                        .trajectory
                        .records
                        .iter()
                        .any(|candidate| {
                            candidate.turn == record.turn
                                && candidate.step == record.step
                                && candidate.index < record.index
                                && !matches!(
                                    candidate.kind,
                                    Kind::User | Kind::Context | Kind::System
                                )
                        }))
                .then(|| {
                    request_boundaries
                        .iter()
                        .find(|request| request.turn == record.turn && request.step == record.step)
                        .map(|request| (*request).clone())
                })
                .flatten();
            if turn_start {
                current_turn = record.turn;
                current_step = 0;
            }
            if record.step != current_step && record.step > 0 {
                current_step = record.step;
                if let Some(request) = visible_requests
                    .iter()
                    .find(|request| request.turn == current_turn && request.step == current_step)
                {
                    body = body.child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(if Some((record.turn, record.step)) == final_step {
                                9.
                            } else {
                                0.
                            }))
                            .child(self.trajectory_request_boundary(request, narrow, true, cx)),
                    );
                }
            }
            body = body.child(self.trajectory_row(
                record,
                summary,
                narrow,
                turn_start,
                request_boundary,
                cx,
            ));
            scroll_index += 1;
        }
        for request in requests.iter().filter(|request| {
            Some((request.turn, request.step)) != last_step
                && last_step.is_none_or(|(turn, step)| (request.turn, request.step) > (turn, step))
        }) {
            body = body.child(
                div()
                    .relative()
                    .w_full()
                    .h(px(9.))
                    .child(self.trajectory_request_boundary(request, narrow, true, cx)),
            );
        }
        if let Some((key, index)) = focused_record {
            if let Some(index) = index {
                self.trajectory.follow = false;
                self.trajectory.scroll.scroll_to_item(index);
            } else if row_count > self.trajectory.visible || self.conversation.history_has_more {
                self.trajectory.visible += 100;
                self.trajectory.focused_record = Some((key, 0));
                if self.trajectory.visible > self.conversation.trajectory.records.len()
                    && self.conversation.history_has_more
                    && !self.conversation.history_loading
                {
                    self.request_older_history(cx);
                }
            }
        }
        if shown == 0 {
            body = body.child(
                row()
                    .w_full()
                    .h(px(180.))
                    .justify_center()
                    .items_center()
                    .text_color(p.faint)
                    .text_size(px(13.))
                    .child(if !query.is_empty() {
                        "没有匹配的轨迹记录"
                    } else if self.trajectory.range.is_some() {
                        "所选时间范围暂无记录；按 Escape 重置时间线"
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
            Some(self.render_trajectory_inspector(record, narrow, composer_clearance, cx))
        });
        let table = column()
            .id("trajectory-table")
            .role(gpui::Role::Table)
            .aria_label("会话轨迹")
            .flex_1()
            .w_full()
            .h_full()
            .min_w_0()
            .child(self.trajectory_toolbar(&self.conversation.trajectory, cx))
            .child(self.trajectory_pointer_capture(cx))
            .child(self.trajectory_timeline(&self.conversation.trajectory, cx))
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
                            // Div's bubble-phase scroll listener is registered first.
                            // Read the offset after that listener applies this delta.
                            .on_scroll(cx.listener(|this, event: &gpui::ScrollEvent, _, cx| {
                                let scroll = &this.trajectory.scroll;
                                let delta = event.delta.pixel_delta(px(20.)).y;
                                let offset = (scroll.offset().y + delta)
                                    .clamp(-scroll.max_offset().y, px(0.));
                                this.trajectory.follow =
                                    delta < px(0.) && offset + scroll.max_offset().y <= px(2.);
                                cx.notify();
                            }))
                            .child(body),
                    )
                    .child(super::workspace::scrollbar_overlay(
                        "trajectory-scrollbar",
                        &self.trajectory.scroll,
                        cx,
                    )),
            );
        row()
            .relative()
            .id("trajectory-view")
            .w_full()
            .flex_1()
            .min_h_0()
            .pt(px(8.))
            .px(px(8.))
            .gap(px(8.))
            .child(table)
            .children(inspector)
    }

    fn render_trajectory_inspector(
        &self,
        record: &Record,
        narrow: bool,
        composer_clearance: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = dsh(cx);
        let title = if record.name.is_empty() {
            record.label()
        } else {
            &record.name
        };
        let detail = |label: &'static str, value: String| {
            column()
                .w_full()
                .gap(px(4.))
                .child(div().text_size(px(11.)).text_color(p.faint).child(label))
                .child(
                    div()
                        .w_full()
                        .p(px(8.))
                        .rounded(px(6.))
                        .bg(p.bg_layer_2)
                        .text_size(px(12.))
                        .line_height(px(19.))
                        .child(value),
                )
        };
        let status = if record.failed() {
            "失败"
        } else if record.pending() {
            "等待中"
        } else {
            "已完成"
        };
        let copied = format!("trajectory-{}", record.key);
        let is_copied = self.copied_mark.as_deref() == Some(copied.as_str());
        let text = format!(
            "#{} {}\n输入：\n{}\n输出：\n{}\n思考：\n{}",
            record.index,
            record.label(),
            record.input,
            record.output,
            record.thinking
        );
        let timing = [
            ("总时长", format_duration(record.timing.duration_ms())),
            ("首 token 延迟", format_duration(record.timing.ttft_ms())),
            ("生成", format_duration(record.timing.decoding_ms())),
        ];
        let usage = record.usage.map(|usage| {
            format!(
                "输入 {} · 输出 {} · 推理 {}\n缓存读取 {} · 缓存写入 {}",
                usage.input, usage.output, usage.thinking, usage.cache_read, usage.cache_write
            )
        });
        column()
            .id("trajectory-inspector")
            .role(gpui::Role::Complementary)
            .aria_label("事件详情")
            .track_focus(&self.trajectory.inspector_focus)
            .tab_index(0)
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, w, cx| {
                if event.keystroke.key == "tab"
                    && !event.keystroke.modifiers.modified()
                    && this.trajectory.inspector_focus.is_focused(w)
                {
                    this.trajectory.close_focus.focus(w, cx);
                    w.prevent_default();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape"
                    && !event.keystroke.modifiers.modified()
                    && !w.default_prevented()
                {
                    this.close_trajectory_inspector(w, cx);
                }
            }))
            .when(narrow, |panel| {
                panel
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .w(gpui::relative(0.92))
                    .max_w(px(420.))
            })
            .when(!narrow, |panel| panel.w(px(320.)).max_w(px(360.)))
            .h_full()
            .flex_none()
            .rounded(px(10.))
            .border(px(0.5))
            .border_color(p.border_l2)
            .bg(p.bg_layer_1)
            .child(
                row()
                    .w_full()
                    .h(px(42.))
                    .px(px(12.))
                    .gap(px(8.))
                    .border_b(px(0.5))
                    .border_color(p.border_l2)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .text_size(px(13.))
                            .child("事件详情"),
                    )
                    .child(
                        row()
                            .id("trajectory-copy")
                            .role(gpui::Role::Button)
                            .aria_label("复制记录")
                            .tab_index(0)
                            .cursor_pointer()
                            .text_size(px(12.))
                            .text_color(p.link)
                            .child(if is_copied { "已复制" } else { "复制" })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    text.clone(),
                                ));
                                this.copied_mark = Some(copied.clone());
                                cx.notify();
                            })),
                    )
                    .child(
                        row()
                            .id("trajectory-inspector-close")
                            .role(gpui::Role::Button)
                            .aria_label("关闭详情")
                            .track_focus(&self.trajectory.close_focus)
                            .tab_index(0)
                            .size(px(24.))
                            .justify_center()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(|row| row.bg(p.hover))
                            .child("×")
                            .capture_key_down(cx.listener(
                                |this, event: &gpui::KeyDownEvent, w, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                        && !event.keystroke.modifiers.modified()
                                        && !w.default_prevented()
                                    {
                                        this.close_trajectory_inspector(w, cx);
                                    }
                                },
                            ))
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.close_trajectory_inspector(w, cx);
                            })),
                    ),
            )
            .child(
                column()
                    .id("trajectory-inspector-scroll")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.trajectory.inspector_scroll)
                    .p(px(12.))
                    .pb(px(composer_clearance))
                    .gap(px(14.))
                    .child(detail(
                        "记录",
                        format!(
                            "#{} · 第 {} 轮 · {}{}",
                            record.index,
                            record.turn,
                            record.label(),
                            record
                                .source_seq
                                .map(|seq| format!(" · seq {seq}"))
                                .unwrap_or_default()
                        ),
                    ))
                    .child(detail("状态", format!("{status} · {title}")))
                    .when_some(record.provider.clone(), |panel, provider| {
                        panel.child(detail("提供方", provider))
                    })
                    .when_some(record.model.clone(), |panel, model| {
                        panel.child(detail("模型", model))
                    })
                    .child(detail(
                        "计时",
                        timing
                            .into_iter()
                            .map(|(label, value)| format!("{label}：{value}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ))
                    .child(detail(
                        "用量",
                        usage.unwrap_or_else(|| "未报告逐请求用量".into()),
                    ))
                    .child(detail(
                        "输入",
                        if record.input.is_empty() {
                            "未捕获参数".into()
                        } else {
                            record.input.clone()
                        },
                    ))
                    .child(detail(
                        "输出",
                        if record.output.is_empty() {
                            "无输出".into()
                        } else {
                            record.output.clone()
                        },
                    ))
                    .when(!record.thinking.is_empty(), |panel| {
                        panel.child(detail("思考", record.thinking.clone()))
                    })
                    .when_some(record.options.clone(), |panel, options| {
                        panel.child(detail("请求选项 JSON", options.to_string()))
                    })
                    .when_some(record.system_prompt.clone(), |panel, prompt| {
                        panel.child(detail("系统提示词", prompt))
                    })
                    .when_some(record.tool_schemas.clone(), |panel, schema| {
                        panel.child(detail("工具 Schema", schema))
                    })
                    .child(detail(
                        "计时来源",
                        "会话事件时间戳；未记录的值保持为 —".into(),
                    )),
            )
    }
}

pub(super) fn reset_on_switch(view: &mut TrajectoryView) {
    view.reset();
}
