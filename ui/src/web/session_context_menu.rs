//! Session-row and title gestures and their captured menu actions.

use super::header_chrome::{
    HeaderMenuAppearance, OpenHeaderMenu, header_menu_action, header_menu_row, menu_card,
    menu_separator,
};
use super::*;
use crate::shortcuts::Command;
use gpui::{
    Bounds, FocusHandle, KeyDownEvent, MouseDownEvent, Pixels, Point, Stateful, canvas, svg,
};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionMenuOrigin {
    Row,
    Title,
}

#[derive(Clone, Copy)]
pub(super) enum SessionMenuAnchor {
    Button(Bounds<Pixels>),
    Gesture(Point<Pixels>),
}

impl SessionMenuAnchor {
    pub(super) fn position(self) -> Point<Pixels> {
        match self {
            Self::Button(bounds) => bounds.bottom_left(),
            Self::Gesture(point) => point,
        }
    }

    pub(super) fn trigger_bounds(self) -> Option<Bounds<Pixels>> {
        match self {
            Self::Button(bounds) => Some(bounds),
            Self::Gesture(_) => None,
        }
    }

    pub(super) fn repeats(self, previous: Self) -> bool {
        match (self, previous) {
            (Self::Button(_), Self::Button(_)) => true,
            (Self::Gesture(a), Self::Gesture(b)) => {
                let delta = a - b;
                f32::from(delta.x).hypot(f32::from(delta.y)) <= 4.
            }
            _ => false,
        }
    }
}

pub(super) struct SessionContext {
    pub(super) session_id: String,
    pub(super) origin: SessionMenuOrigin,
    pub(super) anchor: SessionMenuAnchor,
    pub(super) rows: Vec<(SessionContextAction, FocusHandle)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionContextAction {
    Pin,
    Rename,
    ToggleRead,
    Fork,
    CopyId,
    OpenIn,
    Archive,
}

struct Presentation {
    id: &'static str,
    label: &'static str,
    icon: &'static [u8],
    shortcut: Option<Command>,
    separator: bool,
}

impl SessionContextAction {
    pub(super) fn visible(self, appearance: HeaderMenuAppearance) -> bool {
        !appearance.archived || !matches!(self, Self::Pin | Self::ToggleRead)
    }

    pub(super) fn enabled(self, appearance: HeaderMenuAppearance) -> bool {
        match self {
            Self::ToggleRead => appearance.read_enabled,
            Self::OpenIn => appearance.app_enabled,
            _ => true,
        }
    }

    pub(super) fn accessibility_label(
        self,
        appearance: HeaderMenuAppearance,
    ) -> (&'static str, &'static str) {
        let presentation = self.presentation(appearance);
        (presentation.id, presentation.label)
    }

    fn presentation(self, appearance: HeaderMenuAppearance) -> Presentation {
        let (id, label, icon, shortcut, separator): (_, _, &'static [u8], _, _) = match self {
            Self::Pin => (
                "pin",
                if appearance.pinned {
                    "取消置顶"
                } else {
                    "置顶会话"
                },
                if appearance.pinned {
                    include_bytes!("../../assets/session-pin-fill.svg")
                } else {
                    include_bytes!("../../assets/session-pin-outline.svg")
                },
                None,
                false,
            ),
            Self::Rename => (
                "rename-session",
                "重命名",
                include_bytes!("../../assets/workspace-edit.svg"),
                Some(Command::RenameSession),
                false,
            ),
            Self::ToggleRead => (
                "toggle-read",
                if appearance.read_unread {
                    "标为已读"
                } else {
                    "标为未读"
                },
                if appearance.read_unread {
                    include_bytes!("../../assets/session-read.svg")
                } else {
                    include_bytes!("../../assets/session-unread.svg")
                },
                Some(Command::ToggleRead),
                false,
            ),
            Self::Fork => (
                "fork",
                "分叉会话",
                include_bytes!("../../assets/turn-branch.svg"),
                Some(Command::ForkSession),
                false,
            ),
            Self::CopyId => (
                "copy-id",
                "复制 Session ID",
                include_bytes!("../../assets/session-copy-id.svg"),
                None,
                true,
            ),
            Self::Archive => (
                "archive",
                if appearance.archived {
                    "取消归档"
                } else {
                    "归档会话"
                },
                if appearance.archived {
                    include_bytes!("../../assets/workspace-show-archived.svg")
                } else {
                    include_bytes!("../../assets/workspace-hide-archived.svg")
                },
                (!appearance.archived).then_some(Command::ArchiveSession),
                true,
            ),
            Self::OpenIn => (
                "open-in",
                "在应用中打开",
                include_bytes!("../../assets/open-in-application.svg"),
                None,
                false,
            ),
        };
        Presentation {
            id,
            label,
            icon,
            shortcut,
            separator,
        }
    }
}

impl DshApp {
    pub(super) fn render_session_context_parent(
        &self,
        context: &SessionContext,
        menu: &OpenHeaderMenu,
        appearance: HeaderMenuAppearance,
        interactive: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let p = dsh(cx);
        let mut card = menu_card(p, "session-context-menu", 16., interactive);
        for (action, focus) in &context.rows {
            let action = *action;
            if !action.visible(appearance) {
                continue;
            }
            if action == SessionContextAction::OpenIn {
                card = card.child(self.render_open_in_row(
                    menu,
                    focus,
                    appearance.app_enabled,
                    interactive,
                    cx,
                ));
                continue;
            }
            let spec = action.presentation(appearance);
            if spec.separator {
                card = card.child(menu_separator(p));
            }
            let enabled = interactive && action.enabled(appearance);
            let item = header_menu_row(p, spec.id, enabled.then_some(focus), true, enabled)
                .when(enabled, |item| item.aria_label(spec.label))
                .when(enabled && menu.visibly_accessible_focused(focus), |item| {
                    item.bg(p.hover)
                })
                .child(
                    svg()
                        .data(spec.icon)
                        .size(px(14.))
                        .flex_none()
                        .text_color(p.label_secondary),
                )
                .child(div().flex_1().min_w_0().truncate().child(spec.label))
                .when(enabled, |item| {
                    item.on_hover(cx.listener(Self::leave_header_submenu))
                });
            let item = item.when_some(spec.shortcut, |item, command| {
                let keys = self.shortcuts.keycaps(command);
                item.when_some(self.shortcuts.aria(command), |item, shortcut| {
                    item.aria_keyshortcuts(shortcut)
                })
                .when(!keys.is_empty(), |item| {
                    item.child(
                        row()
                            .flex_none()
                            .gap(px(3.))
                            .text_size(px(11.))
                            .line_height(px(16.))
                            .text_color(p.label_caption)
                            .children(keys),
                    )
                })
            });
            let id = context.session_id.clone();
            card = card.child(header_menu_action(
                item,
                enabled,
                cx.listener(move |app, _, w, cx| {
                    app.run_session_context_action(&id, action, w, cx);
                }),
            ));
        }
        card
    }

    pub(super) fn run_session_context_action(
        &mut self,
        id: &str,
        action: SessionContextAction,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self
            .header_menu
            .as_ref()
            .filter(|_| self.session_context_for(id).is_some())
        else {
            return;
        };
        let appearance = self.header_menu_appearance(menu);
        if !action.visible(appearance) || !action.enabled(appearance) {
            return;
        }
        if action == SessionContextAction::OpenIn {
            self.open_header_submenu(true, w, cx);
            return;
        }
        self.dismiss_header_menu(true, w, cx);
        match action {
            SessionContextAction::Pin => self.toggle_session_pin(id, cx),
            SessionContextAction::Rename => self.begin_session_rename(id.to_owned(), w, cx),
            SessionContextAction::ToggleRead => self.toggle_session_read(id, cx),
            SessionContextAction::Fork => self.fork_session(id, ForkOrigin::Sidebar, cx),
            SessionContextAction::CopyId => {
                bridge::copy_text(id);
                self.push_toast("已复制 Session ID", cx);
            }
            SessionContextAction::Archive => {
                self.set_session_archived(id.to_owned(), !appearance.archived, false, w, cx);
            }
            SessionContextAction::OpenIn => {}
        }
    }
}

pub(super) fn measure_session_menu_anchor(
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
) -> impl IntoElement {
    canvas(move |rect, _, _| bounds.set(Some(rect)), |_, _, _, _| {})
        .absolute()
        .inset_0()
}

pub(super) fn session_menu_gestures(
    element: Stateful<gpui::Div>,
    id: String,
    origin: SessionMenuOrigin,
    focus: Option<&FocusHandle>,
    cx: &mut Context<DshApp>,
) -> Stateful<gpui::Div> {
    let bounds = Rc::new(Cell::new(None));
    let mouse_id = id.clone();
    let mouse_focus = focus.cloned();
    let key_focus = mouse_focus.clone();
    let gesture = Rc::new(cx.listener(move |app, event: &MouseDownEvent, w, cx| {
        app.open_session_context(
            &mouse_id,
            origin,
            SessionMenuAnchor::Gesture(event.position),
            mouse_focus.clone(),
            w,
            cx,
        );
    }));
    let control_click = gesture.clone();
    element
        .relative()
        .child(measure_session_menu_anchor(bounds.clone()))
        .on_mouse_down(gpui::MouseButton::Right, move |event, w, cx| {
            gesture(event, w, cx)
        })
        .on_mouse_down(gpui::MouseButton::Left, move |event, w, cx| {
            if mac_control_click(event.modifiers) {
                control_click(event, w, cx);
            }
        })
        .on_key_down(cx.listener(move |app, event: &KeyDownEvent, w, cx| {
            let stroke = &event.keystroke;
            let mods = stroke.modifiers;
            if event.is_held
                || mods.control
                || mods.alt
                || mods.platform
                || w.default_prevented()
                || !(stroke.key == "contextmenu" || stroke.key == "f10" && mods.shift)
                || key_focus.as_ref().is_some_and(|focus| !focus.is_focused(w))
            {
                return;
            }
            let Some(anchor) = bounds.get() else { return };
            app.open_session_context(
                &id,
                origin,
                SessionMenuAnchor::Gesture(anchor.bottom_left()),
                key_focus.clone(),
                w,
                cx,
            );
        }))
}

pub(super) fn mac_control_click(modifiers: gpui::Modifiers) -> bool {
    modifiers.control && bridge::is_mac()
}
