//! Pointer and keyboard interaction for the two horizontal workspace dividers.
use super::*;

pub(super) const SIDEBAR_MIN: f32 = 192.;
const SIDEBAR_MAX: f32 = 480.;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Divider {
    Sidebar,
    Document,
}

/// A single rounded stroke. The shallow bend points into the window and opens up on hover.
fn folded_grip(divider: Divider, strength: f32, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let center = bounds.center();
            let inward = if divider == Divider::Sidebar { 1. } else { -1. };
            let bend = px(inward * (1. + 5. * strength));
            let half_height = px(8.5 - 1.5 * strength);
            let end_x = center.x - bend * 0.5;
            let top = point(end_x, center.y - half_height);
            let tip = point(end_x + bend, center.y);
            let bottom = point(end_x, center.y + half_height);
            let radius = px(1.25);
            let mut stroke = PathBuilder::stroke(radius * 2.);
            stroke.move_to(top);
            stroke.line_to(tip);
            stroke.line_to(bottom);
            if let Ok(path) = stroke.build() {
                window.paint_path(path, color);
                for end in [top, bottom] {
                    let cap = Bounds::centered_at(end, size(radius * 2., radius * 2.));
                    window.paint_quad(fill(cap, color).corner_radii(radius));
                }
            }
        },
    )
    .w(px(14.))
    .h(px(24.))
}

#[derive(Clone, Copy)]
pub(super) struct Span {
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub extent: f32,
}

#[derive(Clone, Copy)]
pub(super) struct PaneDrag {
    divider: Divider,
    start_x: f32,
    raw: f32,
    moved: bool,
    span: Span,
    original_sidebar: f32,
    original_ratio: f32,
    original_mode: WorkspaceLayoutMode,
    document_width: Option<f32>,
}

pub(super) fn sidebar_max(width: f32, split: bool) -> f32 {
    (width - if split { 640. } else { CHAT_MIN_W }).clamp(SIDEBAR_MIN, SIDEBAR_MAX)
}

pub(super) fn initial_sizes(config: &WorkbenchConfig) -> (f32, f32) {
    let read = |key| {
        crate::client_store::desktop_preference(&config.chat_dir, key)
            .and_then(|raw| raw.trim().parse::<f32>().ok())
            .filter(|n| n.is_finite())
    };
    (
        read("sidebar-width")
            .unwrap_or(SIDEBAR_W)
            .clamp(SIDEBAR_MIN, SIDEBAR_MAX),
        read("split-ratio")
            .unwrap_or(SPLIT_DEFAULT)
            .clamp(SPLIT_MIN, SPLIT_MAX),
    )
}

pub(super) fn chat_span(usable: f32, ratio: f32) -> Span {
    // Keep some travel for both dividers on smaller desktop layouts.
    let min = CHAT_MIN_W.min(usable * 0.44).max(usable * SPLIT_MIN);
    let max = (usable - DOC_MIN_W.min(usable * 0.44)).min(usable * SPLIT_MAX);
    Span {
        value: (usable * ratio).clamp(min, max),
        min,
        max,
        extent: usable,
    }
}

impl KarWorkbench {
    fn store_pane_sizes(&self) {
        for (name, value) in [
            ("sidebar-width", self.sidebar_width.round().to_string()),
            ("split-ratio", self.split_ratio.to_string()),
        ] {
            let _ =
                crate::client_store::write_desktop_preference(&self.config.chat_dir, name, &value);
        }
    }

    fn toggle_divider(&mut self, divider: Divider, cx: &mut Context<Self>) {
        match divider {
            Divider::Sidebar => {
                self.collapsed_sidebar_grip = self.sidebar_open;
                self.set_sidebar_open(!self.sidebar_open, cx);
            }
            Divider::Document => {
                self.collapsed_doc_grip = self.layout_mode != WorkspaceLayoutMode::ChatFocus;
                self.toggle_document_pane(cx);
            }
        }
    }

    fn size_divider(
        &mut self,
        divider: Divider,
        span: Span,
        value: f32,
        document_width: Option<f32>,
        settle: bool,
        cx: &mut Context<Self>,
    ) {
        let value = value.clamp(span.min, span.max);
        let animate = |motion: &mut Slide, target| {
            if settle {
                motion.duration(target, SHELL_SETTLE, cx);
            } else {
                motion.reset(target);
            }
        };
        match divider {
            Divider::Sidebar => {
                self.sidebar_width = value;
                animate(&mut self.sidebar_anim, value);
                if let Some(document) = document_width {
                    let usable = (span.extent - value - SHEET_GUTTER).max(1.);
                    let chat = chat_span(usable, (usable - document) / usable);
                    self.split_ratio = chat.value / usable;
                    self.layout_mode = WorkspaceLayoutMode::Split;
                    animate(&mut self.split_anim, self.split_ratio);
                }
            }
            Divider::Document => {
                self.split_ratio = value / span.extent;
                self.layout_mode = WorkspaceLayoutMode::Split;
                animate(&mut self.split_anim, self.split_ratio);
            }
        }
        cx.notify();
    }

    fn move_divider(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(mut drag) = self.pane_drag else {
            return;
        };
        drag.raw = drag.span.value + f32::from(x) - drag.start_x;
        drag.moved |= (drag.raw - drag.span.value).abs() > 2.;
        self.pane_drag = Some(drag);
        if !drag.moved {
            return;
        }
        self.size_divider(
            drag.divider,
            drag.span,
            drag.raw,
            drag.document_width,
            false,
            cx,
        );
    }

    pub(super) fn capture_divider_drag(&self, cx: &mut Context<Self>) -> AnyElement {
        let moved = cx.listener(|wb, e: &MouseMoveEvent, _, cx| {
            if e.pressed_button == Some(MouseButton::Left) {
                wb.move_divider(e.position.x, cx);
            } else {
                wb.finish_divider(true, cx);
            }
        });
        let up = cx.listener(|wb, e: &MouseUpEvent, _, cx| {
            wb.move_divider(e.position.x, cx);
            wb.finish_divider(false, cx);
        });
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, w, cx| {
                    if phase.capture() {
                        moved(e, w, cx);
                    }
                });
                window.on_mouse_event(move |e: &MouseUpEvent, phase, w, cx| {
                    if phase.capture() && e.button == MouseButton::Left {
                        up(e, w, cx);
                    }
                });
            },
        )
        .absolute()
        .size(px(0.))
        .into_any_element()
    }

    pub(super) fn finish_divider(&mut self, cancel: bool, cx: &mut Context<Self>) {
        let Some(drag) = self.pane_drag.take() else {
            return;
        };
        if !drag.moved {
            cx.notify();
            return;
        }
        let collapse = match drag.divider {
            Divider::Sidebar => drag.raw < drag.span.min - (drag.span.min * 0.5).max(40.),
            Divider::Document => {
                drag.raw > drag.span.max + ((drag.span.extent - drag.span.max) * 0.5).max(40.)
            }
        };
        if cancel || collapse {
            self.sidebar_width = drag.original_sidebar;
            self.split_ratio = drag.original_ratio;
            self.layout_mode = drag.original_mode;
            self.sidebar_anim.duration(
                if self.sidebar_open {
                    self.sidebar_width
                } else {
                    0.
                },
                SHELL_SETTLE,
                cx,
            );
            self.split_anim
                .duration(self.split_target(self.layout_mode), SHELL_SETTLE, cx);
            if collapse && !cancel {
                self.toggle_divider(drag.divider, cx);
            }
        } else {
            self.store_pane_sizes();
        }
        cx.notify();
    }

    pub(super) fn workspace_divider(
        &self,
        divider: Divider,
        boundary: f32,
        span: Span,
        collapsed: bool,
        document_width: Option<f32>,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let (id, resize_label, expand_label, focus) = match divider {
            Divider::Sidebar => (
                "sidebar-width-drag",
                loc.pick("调整侧栏宽度", "Resize sidebar"),
                loc.pick("展开侧栏", "Expand sidebar"),
                self.sidebar_divider_focus.clone(),
            ),
            Divider::Document => (
                "kar-split-divider",
                loc.pick("调整文档面板宽度", "Resize document pane"),
                loc.pick("展开右侧面板", "Expand right panel"),
                self.doc_divider_focus.clone(),
            ),
        };
        let focused = focus.is_focused(window);
        let keyboard_focus = focused && window.last_input_was_keyboard();
        let active = if collapsed {
            keyboard_focus
        } else {
            focused || self.pane_drag.is_some_and(|d| d.divider == divider)
        };
        let hint = if collapsed {
            expand_label
        } else {
            loc.pick("拖动或用左右键调整，Shift 加速，Enter 折叠，双击复位",
                "Drag or use arrow keys to resize. Shift moves faster. Enter collapses the pane. Double-click to reset.")
        };
        let width = f32::from(window.viewport_size().width);
        let hit = if collapsed { 28. } else { 13. };
        let left = (boundary - hit / 2.).clamp(0., (width - hit).max(0.));
        let ink = if active { s.primary } else { s.muted };
        let motion = window.use_keyed_state((id, 0usize), cx, |_, _| Slide::new(0.));
        let strength = if active {
            1.
        } else {
            motion.update(cx, |slide, cx| slide.get(window, cx))
        };
        // Give the grip a little elastic travel while the panes keep their readable minimums.
        let recoil = window.use_keyed_state((id, 1usize), cx, |_, _| Slide::new(0.));
        let resistance = recoil.update(cx, |slide, cx| {
            if let Some(drag) = self.pane_drag.filter(|drag| drag.divider == divider) {
                let x = drag.raw - drag.raw.clamp(drag.span.min, drag.span.max);
                slide.reset(x.signum() * (3. * (1. - (-x.abs() / 20.).exp())));
            } else {
                slide.duration(0., 0.20, cx);
            }
            slide.get(window, cx)
        });
        let key_focus = focus.clone();
        let mut control = div()
            .id(id)
            .map(|control| {
                if collapsed {
                    control
                        .role(Role::Button)
                        .aria_label(expand_label)
                        .aria_expanded(false)
                } else {
                    let pane = if divider == Divider::Sidebar {
                        span.value
                    } else {
                        span.extent - span.value
                    };
                    control
                        .role(Role::Splitter)
                        .aria_label(resize_label)
                        .aria_min_numeric_value(0.)
                        .aria_max_numeric_value(100.)
                        .aria_numeric_value(f64::from(pane / span.extent * 100.))
                }
            })
            .track_focus(&focus)
            .tab_stop(true)
            .absolute()
            .left(px(left))
            .w(px(hit))
            .map(|d| {
                if collapsed {
                    d.cursor_pointer()
                } else {
                    d.cursor_col_resize()
                }
            })
            .flex()
            .items_center()
            .justify_center()
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(hint).build(window, cx)
            })
            .on_hover(cx.listener(move |_, hover: &bool, _, cx| {
                motion.update(cx, |m, cx| {
                    let target = if *hover { 1. } else { 0. };
                    if cx.reduce_motion() {
                        m.reset(target);
                    } else {
                        m.duration(target, 0.16, cx);
                    }
                });
                cx.notify();
            }))
            .when(collapsed, |d| {
                d.on_click(cx.listener(move |wb, _, win, cx| {
                    wb.toggle_divider(divider, cx);
                    focus.focus(win, cx);
                    cx.stop_propagation();
                }))
            })
            .when(!collapsed, |d| {
                d.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |wb, ev: &MouseDownEvent, win, cx| {
                        key_focus.focus(win, cx);
                        win.prevent_default();
                        cx.stop_propagation();
                        if ev.click_count >= 2 {
                            let default = if divider == Divider::Sidebar {
                                SIDEBAR_W
                            } else {
                                span.extent * SPLIT_DEFAULT
                            };
                            wb.size_divider(divider, span, default, document_width, true, cx);
                            wb.store_pane_sizes();
                        } else {
                            wb.pane_drag = Some(PaneDrag {
                                divider,
                                start_x: f32::from(ev.position.x),
                                raw: span.value,
                                moved: false,
                                span,
                                original_sidebar: wb.sidebar_width,
                                original_ratio: wb.split_ratio,
                                original_mode: wb.layout_mode,
                                document_width,
                            });
                            cx.notify();
                        }
                    }),
                )
            })
            .on_key_down(cx.listener(move |wb, ev: &KeyDownEvent, win, cx| {
                let step = if ev.keystroke.modifiers.shift {
                    64.
                } else {
                    16.
                };
                let value = match ev.keystroke.key.as_str() {
                    "tab" => {
                        if ev.keystroke.modifiers.shift {
                            win.focus_prev(cx);
                        } else {
                            win.focus_next(cx);
                        }
                        None
                    }
                    "escape" if wb.pane_drag.is_some() => {
                        wb.finish_divider(true, cx);
                        None
                    }
                    "space" if !collapsed => return,
                    "enter" | "space" => {
                        wb.toggle_divider(divider, cx);
                        None
                    }
                    "left" | "right" | "home" | "end" if !collapsed => {
                        if wb.pane_drag.is_some() {
                            wb.finish_divider(true, cx);
                        }
                        let current = match divider {
                            Divider::Sidebar => wb.sidebar_width.clamp(span.min, span.max),
                            Divider::Document => (span.extent * wb.split_target(wb.layout_mode))
                                .clamp(span.min, span.max),
                        };
                        Some(match ev.keystroke.key.as_str() {
                            "left" => current - step,
                            "right" => current + step,
                            "home" => span.min,
                            _ => span.max,
                        })
                    }
                    _ => return,
                };
                if let Some(width) = value {
                    wb.size_divider(divider, span, width, document_width, true, cx);
                    wb.store_pane_sizes();
                }
                win.prevent_default();
                cx.stop_propagation();
            }));
        if collapsed {
            let grip_ink = if keyboard_focus {
                s.primary
            } else {
                s.surface
                    .blend(s.muted.opacity(0.86))
                    .blend(s.ink.opacity(0.52 * strength))
            };
            control = control
                .top(px((f32::from(window.viewport_size().height) - 48.) * 0.5))
                .h(px(48.))
                .child(folded_grip(divider, strength, grip_ink));
        } else {
            control = control.top_0().bottom_0().child(
                div()
                    .relative()
                    .left(px(resistance))
                    .w(px(3.))
                    .h(px(19. + strength * 13.))
                    .rounded_full()
                    .bg(ink.opacity(0.38 + strength * 0.60)),
            );
        }
        control.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{SHEET_GUTTER, chat_span, sidebar_max};

    #[test]
    fn narrow_split_keeps_both_panes_visible_and_the_divider_movable() {
        let usable = 716. - sidebar_max(716., true) - SHEET_GUTTER;
        let initial = chat_span(usable, 0.50);
        let resized = chat_span(usable, (initial.value + 16.) / usable);
        assert!((resized.value - initial.value - 16.).abs() < 0.01);
        let at_left_limit = chat_span(usable, 0.);
        let at_right_limit = chat_span(usable, 1.);
        assert!(at_left_limit.value > 0. && at_left_limit.value < initial.value);
        assert!(at_right_limit.value < usable && at_right_limit.value > resized.value);
    }
}
