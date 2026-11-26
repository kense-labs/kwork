//! How Kar Desktop feels under the hand: buttons, menus, disclosures and the toast.
//!
//! [`Tactile`] is the one pressable surface in the app. It carries the physics of KUI's
//! `pro_btn` to any content: the pointer's wash blends in on a snappy spring, a press insets
//! the painted face while the hit box stays put so neighbours never shift, and a lit state
//! (selected row, open menu trigger) glides in rather than snapping. Surfaces are told apart
//! by fill, never by an outline.

use std::{
    rc::Rc,
    sync::OnceLock,
    time::{Duration, Instant},
};

use gpui::{prelude::*, *};
use gpui_kit::component::tooltip::Tooltip;
use kui::{
    components::pro_chrome::{pro_ease, pro_eid, pro_fold, pro_mix_color, pro_presence},
    primitives::{
        family::{PhaseClock, Slide, disabled_motion},
        glyph::{self, Glyph, morph_mark},
        pro_icons::{ProIcon, pro_icon},
    },
};

use crate::ui_primitives::{
    CTRL_H, FS_CONTROL, FS_META, FS_UI, LH_CONTROL, ICON, ICON_SM, ICON_XS, R_CARD, R_CTRL, R_PILL, ROW_H, StudioPalette,
};
pub(crate) mod selection;

type ClickFn = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
/// Called when a menu should close because of a press somewhere else.
pub type DismissFn = Rc<dyn Fn(&mut Window, &mut App)>;

struct TactileAnim {
    hover: Slide,
    press: Slide,
    lit: Slide,
    focus: Option<FocusHandle>,
}

/// A pressable surface with spring hover, tactile press and an optional lit state.
///
/// Lay it out with the usual style methods and give it children like a `div`; those apply to
/// the hit box. The fill and corner radius are painted beneath the children.
#[derive(IntoElement)]
pub struct Tactile {
    id: ElementId,
    style: StyleRefinement,
    children: Vec<AnyElement>,
    rest: Hsla,
    hover: Hsla,
    lit_fill: Hsla,
    lit: bool,
    radius: f32,
    lifts: bool,
    dark: bool,
    disabled: bool,
    inert: bool,
    ink: Option<(Hsla, Hsla)>,
    on_click: Option<ClickFn>,
    tip: Option<SharedString>,
    aria: Option<SharedString>,
    role: Role,
    selection: Option<bool>,
    expanded: Option<bool>,
    focus: Option<FocusHandle>,
    tab_stop: bool,
}

impl Tactile {
    /// Nothing at rest, a quiet wash under the pointer. Toolbar buttons, rows, menu items.
    pub fn ghost(id: impl Into<ElementId>, s: &StudioPalette) -> Self {
        Self {
            id: id.into(),
            style: StyleRefinement::default(),
            children: Vec::new(),
            rest: transparent_black(),
            hover: s.hover,
            lit_fill: s.active,
            lit: false,
            radius: R_CTRL,
            lifts: false,
            dark: s.dark,
            disabled: false,
            inert: false,
            ink: None,
            on_click: None,
            tip: None,
            aria: None,
            role: Role::Button,
            selection: None,
            expanded: None,
            focus: None,
            tab_stop: true,
        }
    }

    /// A quiet fill. Secondary actions and chips: present without being outlined.
    pub fn soft(id: impl Into<ElementId>, s: &StudioPalette) -> Self {
        Self {
            rest: s.soft,
            hover: s.soft_hover,
            lit_fill: s.soft_hover,
            ..Self::ghost(id, s)
        }
    }

    /// Solid ink. The one primary action in a group.
    pub fn primary(id: impl Into<ElementId>, s: &StudioPalette) -> Self {
        Self {
            rest: s.primary,
            hover: pro_mix_color(s.primary, s.on_primary, 0.14),
            lit_fill: s.primary,
            ink: Some((s.on_primary, s.on_primary)),
            lifts: true,
            ..Self::ghost(id, s)
        }
    }

    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// Override the resting and hovered fills.
    pub fn fills(mut self, rest: Hsla, hover: Hsla) -> Self {
        self.rest = rest;
        self.hover = hover;
        self
    }

    /// Selected, or the trigger of an open menu.
    pub fn lit(mut self, lit: bool) -> Self {
        self.lit = lit;
        self
    }

    /// Fill used while lit. Transparent leaves the highlight to a shared sliding indicator.
    pub fn lit_fill(mut self, fill: Hsla) -> Self {
        self.lit_fill = fill;
        self
    }

    /// Text colour at rest and under the pointer, for children that inherit it.
    pub fn ink(mut self, rest: Hsla, hover: Hsla) -> Self {
        self.ink = Some((rest, hover));
        self
    }

    /// Dimmed and not pressable.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Shown at full strength but not pressable: a fact on display rather than a choice.
    pub fn inert(mut self) -> Self {
        self.disabled = true;
        self.inert = true;
        self
    }

    /// Hover hint. Icon-only buttons always have one.
    pub fn tip(mut self, text: impl Into<SharedString>) -> Self {
        let text = text.into();
        if self.aria.is_none() {
            self.aria = Some(text.clone());
        }
        self.tip = Some(text);
        self
    }

    pub fn aria(mut self, label: impl Into<SharedString>) -> Self {
        self.aria = Some(label.into());
        self
    }

    pub fn expanded(mut self, open: bool) -> Self {
        self.expanded = Some(open);
        self
    }

    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }

    pub fn on_press<V: 'static>(
        mut self,
        cx: &Context<V>,
        f: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(cx.listener(
            move |view, _: &ClickEvent, window, cx| f(view, window, cx),
        )));
        self
    }
}

impl Styled for Tactile {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Tactile {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Tactile {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let lit_target = f32::from(self.lit);
        let anim = window.use_keyed_state(pro_eid(&self.id, "tactile"), cx, |_, _| TactileAnim {
            hover: Slide::snappy(0.),
            press: Slide::snappy(0.),
            lit: Slide::snappy(lit_target),
            focus: None,
        });
        let (hov, down, lit, button_focus) = anim.update(cx, |a, cx| {
            a.lit.set(lit_target, cx);
            let focus = (!self.disabled && self.selection.is_none()).then(|| {
                let handle = a
                    .focus
                    .get_or_insert_with(|| cx.focus_handle().tab_stop(self.tab_stop));
                if handle.tab_stop != self.tab_stop {
                    *handle = handle.clone().tab_stop(self.tab_stop);
                }
                handle.clone()
            });
            (
                a.hover.get(window, cx).clamp(0., 1.),
                a.press.get(window, cx).clamp(0., 1.),
                a.lit.get(window, cx).clamp(0., 1.),
                focus,
            )
        });

        let disabled = self.disabled;
        let dimmed = disabled && !self.inert;
        let base = pro_mix_color(self.rest, self.lit_fill, lit);
        let face = if disabled {
            base
        } else {
            pro_mix_color(base, self.hover, hov * (1. - 0.55 * lit))
        };
        let radius = self.radius;
        let reduce_motion = disabled_motion(cx);
        let down = if reduce_motion { 0. } else { down };
        let lifts = self.lifts && !disabled && !self.dark && !reduce_motion;
        let focus_ring = StudioPalette::get(cx).pro.focus;
        let show_button_ring = window.last_input_was_keyboard()
            && button_focus
                .as_ref()
                .is_some_and(|handle| handle.is_focused(window));

        // The face is painted, not laid out: pressing insets it without moving the hit box.
        let paint_face = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let bw = f32::from(bounds.size.width);
                let bh = f32::from(bounds.size.height);
                if bw <= 1. || bh <= 1. {
                    return;
                }
                let ix = (bw * 0.022 * down).clamp(0., 2.2);
                let iy = (bh * 0.042 * down).clamp(0., 1.4);
                let rect = Bounds::new(
                    point(bounds.origin.x + px(ix), bounds.origin.y + px(iy)),
                    size(px((bw - 2. * ix).max(1.)), px((bh - 2. * iy).max(1.))),
                );
                let r = px(radius)
                    .min(rect.size.width / 2.)
                    .min(rect.size.height / 2.);
                if lifts && hov > 0.02 && down < 0.7 {
                    window.paint_quad(
                        fill(
                            Bounds::new(point(rect.origin.x, rect.origin.y + px(1.)), rect.size),
                            black().opacity(0.05 * hov * (1. - down)),
                        )
                        .corner_radii(r),
                    );
                }
                if face.a > 0.001 {
                    window.paint_quad(fill(rect, face).corner_radii(r));
                }
                if show_button_ring {
                    window.paint_quad(
                        outline(bounds, focus_ring, BorderStyle::default()).corner_radii(r),
                    );
                }
            },
        )
        .absolute()
        .inset_0();

        // Defaults first, so the caller's layout choices win.
        let mut root = div()
            .id(self.id)
            .text_size(px(FS_UI))
            .line_height(px(crate::ui_primitives::LH_UI))
            .relative()
            .flex()
            .items_center()
            .rounded(px(radius));
        root.style().refine(&self.style);

        let set_hover = {
            let anim = anim.clone();
            move |inside: &bool, window: &mut Window, cx: &mut App| {
                anim.update(cx, |a, cx| {
                    a.hover.set(f32::from(*inside), cx);
                    if !*inside {
                        a.press.set(0., cx);
                    }
                    cx.notify();
                });
                window.refresh();
            }
        };
        let set_press = |to: f32| {
            let anim = anim.clone();
            move |window: &mut Window, cx: &mut App| {
                anim.update(cx, |a, cx| {
                    a.press.set(to, cx);
                    cx.notify();
                });
                window.refresh();
            }
        };
        let press_down = set_press(1.);
        let press_up = set_press(0.);
        let press_up_out = set_press(0.);
        let key_down = set_press(1.);
        let key_up = set_press(0.);
        let on_click = self.on_click;
        let selection_control = self.selection.is_some();

        root.role(self.role)
            .when_some(self.focus.or(button_focus), |d, focus| d.track_focus(&focus))
            .when_some(self.selection, |d, selected| {
                if self.role == Role::RadioButton {
                    d.aria_toggled(selected.into())
                } else {
                    d.aria_selected(selected)
                }
            })
            .when_some(self.aria, |d, label| d.aria_label(label))
            .when_some(self.expanded, |d, open| d.aria_expanded(open))
            .when_some(self.ink, |d, (rest, hovered)| {
                d.text_color(if disabled {
                    rest
                } else {
                    pro_mix_color(rest, hovered, hov.max(lit))
                })
            })
            .when(dimmed, |d| d.opacity(0.5))
            .when(!disabled, |d| {
                d.tab_index(0)
                    .tab_stop(self.tab_stop)
                    .cursor_pointer()
                    .when(selection_control, |d| {
                        d.border_1()
                            .border_color(transparent_black())
                            .focus_visible(move |st| st.border_color(focus_ring))
                    })
                    .on_hover(set_hover)
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        press_down(window, cx)
                    })
                    .on_mouse_up(MouseButton::Left, move |_, window, cx| press_up(window, cx))
                    .on_mouse_up_out(MouseButton::Left, move |_, window, cx| {
                        press_up_out(window, cx)
                    })
                    .on_key_down(move |ev, window, cx| {
                        if matches!(ev.keystroke.key.as_str(), "enter" | "space") {
                            key_down(window, cx);
                        }
                    })
                    .on_key_up(move |ev, window, cx| {
                        if matches!(ev.keystroke.key.as_str(), "enter" | "space") {
                            key_up(window, cx);
                        }
                    })
                    .when_some(on_click, |d, f| {
                        d.on_click(move |ev, window, cx| f(ev, window, cx))
                    })
            })
            .when_some(self.tip, |d, text| {
                d.tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
            })
            .child(paint_face)
            .children(self.children)
    }
}

// ── Buttons ─────────────────────────────────────────────────────────────────

/// Square icon-only button. Always carries a hover hint.
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: ProIcon,
    tip: impl Into<SharedString>,
    s: &StudioPalette,
) -> Tactile {
    Tactile::ghost(id, s)
        .size(px(CTRL_H))
        .flex_none()
        .justify_center()
        .tip(tip)
        .child(pro_icon(icon, ICON, s.text))
}

/// A labelled button with no fill at rest, for secondary actions.
pub fn text_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    s: &StudioPalette,
) -> Tactile {
    let label = label.into();
    Tactile::ghost(id, s)
        .aria(label.clone())
        .flex_none()
        .h(px(CTRL_H))
        .px(px(8.))
        .gap(px(6.))
        .text_size(px(FS_CONTROL))
        .line_height(px(LH_CONTROL))
        .ink(s.text, s.ink)
        .child(label)
}

/// A labelled button on a quiet fill, for the action worth noticing.
pub fn soft_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    s: &StudioPalette,
) -> Tactile {
    let label = label.into();
    Tactile::soft(id, s)
        .aria(label.clone())
        .flex_none()
        .h(px(CTRL_H))
        .px(px(10.))
        .gap(px(6.))
        .text_size(px(FS_CONTROL))
        .line_height(px(LH_CONTROL))
        .font_weight(FontWeight::MEDIUM)
        .text_color(s.ink)
        .child(label)
}

/// The primary action of a group.
pub fn primary_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    s: &StudioPalette,
) -> Tactile {
    let label = label.into();
    Tactile::primary(id, s)
        .aria(label.clone())
        .flex_none()
        .h(px(CTRL_H))
        .px(px(12.))
        .gap(px(6.))
        .text_size(px(FS_CONTROL))
        .line_height(px(LH_CONTROL))
        .font_weight(FontWeight::MEDIUM)
        .text_color(s.on_primary)
        .child(label)
}

// ── Menus ───────────────────────────────────────────────────────────────────

/// Which corner of its trigger a menu hangs from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuPlacement {
    /// Below the trigger, left edges aligned.
    BelowStart,
    /// Below the trigger, right edges aligned.
    BelowEnd,
    /// Above the trigger, left edges aligned.
    AboveStart,
    /// Above the trigger, right edges aligned.
    AboveEnd,
}

/// A trigger and the menu that hangs from it.
pub fn menu_host(trigger: impl IntoElement, menu: AnyElement) -> Div {
    div().relative().flex_none().child(trigger).child(menu)
}

pub(crate) struct MenuSize {
    pub width: f32,
    pub max_height: f32,
}

impl From<f32> for MenuSize {
    fn from(width: f32) -> Self {
        Self {
            width,
            max_height: 360.,
        }
    }
}

/// A floating menu. Drawn at `deferred` priority 3 with an occluding full-window dismiss shield
/// while open so outside clicks cleanly dismiss the menu without clicking through to underlying
/// controls, and bounded in height so long lists scroll smoothly inside narrow windows.
pub(crate) fn menu_popup(
    id: impl Into<ElementId>,
    open: bool,
    placement: MenuPlacement,
    dimensions: impl Into<MenuSize>,
    rows: Vec<AnyElement>,
    on_dismiss: DismissFn,
    s: &StudioPalette,
) -> AnyElement {
    let MenuSize { width, max_height } = dimensions.into();
    let id = id.into();
    let surface_id = pro_eid(&id, "surface");
    let s = *s;
    pro_presence(id, open, 0.16, 0.12, move |amount| {
        let above = matches!(
            placement,
            MenuPlacement::AboveStart | MenuPlacement::AboveEnd
        );
        let end = matches!(placement, MenuPlacement::BelowEnd | MenuPlacement::AboveEnd);
        let corner = match (above, end) {
            (false, false) => Anchor::TopLeft,
            (false, true) => Anchor::TopRight,
            (true, false) => Anchor::BottomLeft,
            (true, true) => Anchor::BottomRight,
        };
        // 6px clear of the trigger, travelling the last 4px as it appears.
        let travel = 4. * (1. - amount);
        let gap = if above { -(6. - travel) } else { 6. - travel };

        let dismiss = on_dismiss.clone();
        let surface = div()
            .id(surface_id)
            .role(Role::Menu)
            .w(px(width))
            .max_h(px(max_height))
            .overflow_y_scroll()
            .p(px(4.))
            .rounded(px(R_CARD))
            .bg(s.card)
            .border_1()
            .border_color(s.line)
            .shadow(s.popover_shadow())
            .opacity(amount)
            .when(open, |d| {
                d.occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down_out(move |_, window, cx| dismiss(window, cx))
            })
            .flex()
            .flex_col()
            .children(rows);

        div()
            .absolute()
            .size_0()
            .map(|d| if above { d.top_0() } else { d.top(relative(1.)) })
            .map(|d| if end { d.right_0() } else { d.left_0() })
            .when(open, |d| {
                d.child(
                    deferred(div().absolute().size_0().child(kantor_ui::overlay::shield()))
                        .with_priority(3),
                )
            })
            .child(
                deferred(
                    anchored()
                        .anchor(corner)
                        .offset(point(px(0.), px(gap)))
                        .snap_to_window_with_margin(Edges::all(px(10.)))
                        .child(surface),
                )
                .with_priority(3),
            )
            .into_any_element()
    })
}

/// One row of a menu.
pub fn menu_item(
    id: impl Into<ElementId>,
    lead: Option<AnyElement>,
    label: impl Into<SharedString>,
    s: &StudioPalette,
) -> Tactile {
    let label = label.into();
    Tactile::ghost(id, s)
        .role(Role::MenuItem)
        .aria(label.clone())
        .w_full()
        .flex_none()
        .h(px(ROW_H))
        .px(px(8.))
        .gap(px(8.))
        .text_size(px(FS_UI))
        .text_color(s.ink)
        .when_some(lead, |row, el| {
            row.child(
                div()
                    .w(px(ICON))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .child(el),
            )
        })
        .child(div().flex_1().min_w_0().truncate().child(label))
}

/// A menu row led by one of KUI's line icons.
pub fn menu_icon_item(
    id: impl Into<ElementId>,
    icon: ProIcon,
    label: impl Into<SharedString>,
    s: &StudioPalette,
) -> Tactile {
    menu_item(id, Some(pro_icon(icon, ICON, s.muted)), label, s)
}

/// Keyboard shortcut or other quiet note at the end of a menu row.
pub fn menu_hint(text: impl Into<SharedString>, s: &StudioPalette) -> Div {
    div()
        .flex_none()
        .text_size(px(FS_META))
        .text_color(s.faint)
        .child(text.into())
}

/// The check at the end of the chosen row.
pub fn menu_check(s: &StudioPalette) -> AnyElement {
    pro_icon(ProIcon::Check, ICON_SM, s.ink)
}

/// Quiet heading inside a menu.
pub fn menu_caption(text: impl Into<SharedString>, s: &StudioPalette) -> Div {
    div()
        .flex_none()
        .px(px(8.))
        .pt(px(8.))
        .pb(px(4.))
        .text_size(px(FS_META))
        .text_color(s.muted)
        .child(text.into())
}

pub fn menu_divider(s: &StudioPalette) -> Div {
    div().flex_none().my(px(4.)).mx(px(4.)).h(px(1.)).bg(s.line)
}

// ── Disclosure ──────────────────────────────────────────────────────────────

/// One quiet line that opens to show what happened behind it: the commands an agent ran,
/// what it was thinking, the lines a change touched. The label sits flush with the text
/// around it; the wash under the pointer extends 8px beyond.
pub fn disclosure_line(
    id: impl Into<ElementId>,
    lead: AnyElement,
    label: impl Into<SharedString>,
    open: bool,
    s: &StudioPalette,
) -> Tactile {
    let label = label.into();
    Tactile::ghost(id, s)
        .aria(label.clone())
        .expanded(open)
        .ml(px(-8.))
        .max_w_full()
        .h(px(CTRL_H))
        .px(px(8.))
        .gap(px(8.))
        .text_size(px(FS_UI))
        .ink(s.muted, s.text)
        .child(lead)
        .child(div().min_w_0().truncate().child(label))
        .child(pro_icon(
            if open {
                ProIcon::ChevronDown
            } else {
                ProIcon::ChevronRight
            },
            ICON_XS,
            s.faint,
        ))
}

/// The body under a [`disclosure_line`]: indented behind a hairline, folding open and shut.
pub fn disclosure_body(
    id: impl Into<ElementId>,
    open: bool,
    estimate: f32,
    body: AnyElement,
    s: &StudioPalette,
) -> AnyElement {
    let rule = s.line;
    pro_fold(id, open, estimate, 0., move || {
        div()
            .w_full()
            .pt(px(4.))
            .pb(px(4.))
            .child(
                div()
                    .ml(px(6.))
                    .pl(px(14.))
                    .border_l_1()
                    .border_color(rule)
                    .child(body),
            )
            .into_any_element()
    })
}

// ── Motion ──────────────────────────────────────────────────────────────────

/// A turning open ring for work in progress. Still when motion is reduced.
pub fn spinner(side: f32, color: Hsla, window: &Window, cx: &App) -> AnyElement {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let turn = if disabled_motion(cx) {
        0.
    } else {
        window.request_animation_frame();
        EPOCH.get_or_init(Instant::now).elapsed().as_secs_f32() * 1.15
    };
    glyph::spinner(turn, side, color).into_any_element()
}

struct MorphGlyphAnim {
    on: bool,
    slide: Slide,
    clock: PhaseClock,
}

/// Smoothly stroke-interpolating KUI vector icon (`Play↔Pause`, `Copy↔Check`, `Plus↔Check`, `Down↔Up`, `Bell↔BellOff`).
#[allow(clippy::too_many_arguments)]
pub fn morph_glyph(
    id: impl Into<ElementId>,
    pair: [Glyph; 2],
    on: bool,
    side: f32,
    ink: Hsla,
    surface: Hsla,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let id = id.into();
    let init = f32::from(on);
    let st = window.use_keyed_state(pro_eid(&id, "morph-glyph"), cx, |_, _| MorphGlyphAnim {
        on,
        slide: Slide::snappy(init),
        clock: PhaseClock::default(),
    });
    let (t, age) = st.update(cx, |a, cx| {
        if a.on != on {
            a.on = on;
            a.slide.set(f32::from(on), cx);
            a.clock.restart(cx);
        }
        (a.slide.get(window, cx).clamp(0., 1.), a.clock.time(0.65, window, cx))
    });
    morph_mark(pair, t, age, side, ink, ink, surface).into_any_element()
}

/// Breathing live indicator dot with subtle expanding halo ring when active.
pub fn pulse_beacon(color: Hsla, side: f32, active: bool, window: &Window, cx: &App) -> AnyElement {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let wave = if active && !disabled_motion(cx) {
        window.request_animation_frame();
        let secs = EPOCH.get_or_init(Instant::now).elapsed().as_secs_f32();
        0.5 + 0.5 * (secs * 3.2).sin()
    } else {
        0.0
    };
    let outer = side + 6.0;
    let halo_r = side + 5.0 * wave;
    div()
        .relative()
        .size(px(outer))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .when(active && wave > 0.01, |d| {
            d.child(
                div()
                    .absolute()
                    .size(px(halo_r))
                    .rounded_full()
                    .bg(color.opacity(0.26 * (1.0 - wave * 0.7))),
            )
        })
        .child(div().size(px(side)).rounded_full().bg(color))
        .into_any_element()
}

// ── Toast ───────────────────────────────────────────────────────────────────

/// A brief confirmation that needs no reply.
#[derive(Clone, Debug)]
pub struct ToastState {
    pub text: SharedString,
    /// Something did not go as asked, as opposed to a plain confirmation.
    pub warning: bool,
    pub shown_at: Instant,
}

const TOAST_LIFE: Duration = Duration::from_millis(2800);
const TOAST_LIFE_WARNING: Duration = Duration::from_millis(5200);

impl ToastState {
    pub fn new(text: impl Into<SharedString>, warning: bool) -> Self {
        Self {
            text: text.into(),
            warning,
            shown_at: Instant::now(),
        }
    }

    fn life(&self) -> Duration {
        if self.warning {
            TOAST_LIFE_WARNING
        } else {
            TOAST_LIFE
        }
    }

    pub fn expired(&self) -> bool {
        self.shown_at.elapsed() >= self.life()
    }
}

/// Top-level toast capsule (`deferred` priority 5 so it stays visible even above open modals).
pub fn render_toast(
    toast: Option<&ToastState>,
    s: &StudioPalette,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let Some(toast) = toast.filter(|t| !t.expired()) else {
        return div().into_any_element();
    };
    let age = toast.shown_at.elapsed().as_secs_f32();
    let life = toast.life().as_secs_f32();
    let presence = if disabled_motion(cx) {
        1.
    } else if age < 0.2 {
        pro_ease(age / 0.2)
    } else if age > life - 0.26 {
        1. - pro_ease((age - (life - 0.26)) / 0.26)
    } else {
        1.
    };
    window.request_animation_frame();

    let (icon, tint) = if toast.warning {
        (ProIcon::AlertTriangle, s.pro.amber)
    } else {
        (ProIcon::Check, s.pro.green)
    };
    let capsule = div()
        .relative()
        .top(px(-6. * (1. - presence)))
        .opacity(presence)
        .max_w(px(440.))
        .min_h(px(ROW_H))
        .pl(px(11.))
        .pr(px(15.))
        .py(px(6.))
        .rounded(px(R_PILL))
        .bg(if s.dark { rgb(0x27272a) } else { rgb(0x18181b) })
        .border_1()
        .border_color(hsla(0., 0., 1., if s.dark { 0.14 } else { 0.10 }))
        .shadow(s.popover_shadow())
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(FS_UI))
        .line_height(px(18.))
        .text_color(rgb(0xfafafa))
        .child(pro_icon(icon, ICON_SM, tint))
        .child(div().min_w_0().child(toast.text.clone()));

    deferred(
        div()
            .absolute()
            .top(px(14.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .px(px(16.))
            .child(capsule),
    )
    .with_priority(5)
    .into_any_element()
}
