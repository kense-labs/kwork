//! Controlled peer views and mutually exclusive settings. Workbench state remains authoritative.
use super::Tactile;
use crate::ui_primitives::{
    CTRL_SM_H, FS_CONTROL, LH_CONTROL, R_CTRL, R_SM, StudioPalette, tnum_features,
};
use gpui::{prelude::*, *};
use kui::components::pro_chrome::{
    pro_eid, pro_mark_selected, pro_pill_indicator, pro_selection_sensor,
};
use std::rc::Rc;

type Select = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Choices,
    Tabs,
    Documents,
}

pub struct Item {
    key: SharedString,
    label: SharedString,
    selected: bool,
    content: Option<AnyElement>,
    close: Option<Tactile>,
    select: Select,
}

impl Item {
    pub fn new<V: 'static>(
        key: impl Into<SharedString>,
        label: impl Into<SharedString>,
        selected: bool,
        cx: &Context<V>,
        action: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
    ) -> Self {
        let view = cx.weak_entity();
        Self {
            key: key.into(),
            label: label.into(),
            selected,
            content: None,
            close: None,
            select: Rc::new(move |win, cx| {
                let _ = view.update(cx, |view, cx| action(view, win, cx));
            }),
        }
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    pub fn close(mut self, close: Tactile) -> Self {
        self.close = Some(close);
        self
    }
}

#[derive(IntoElement)]
pub struct SelectionStrip {
    id: ElementId,
    label: SharedString,
    kind: Group,
    items: Vec<Item>,
    palette: StudioPalette,
    disabled: bool,
}

impl SelectionStrip {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        kind: Group,
        items: Vec<Item>,
        palette: &StudioPalette,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind,
            items,
            palette: *palette,
            disabled: false,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn options<V: 'static, T: Clone + PartialEq + 'static, L: Into<SharedString>>(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        kind: Group,
        options: impl IntoIterator<Item = (T, L)>,
        selected: T,
        palette: &StudioPalette,
        cx: &Context<V>,
        action: impl Fn(&mut V, T, &mut Window, &mut Context<V>) + 'static,
    ) -> Self {
        let action = Rc::new(action);
        let items = options
            .into_iter()
            .enumerate()
            .map(|(i, (value, label))| {
                let choose = action.clone();
                Item::new(
                    i.to_string(),
                    label,
                    value == selected,
                    cx,
                    move |v, w, cx| choose(v, value.clone(), w, cx),
                )
            })
            .collect();
        Self::new(id, label, kind, items, palette)
    }
}

impl RenderOnce for SelectionStrip {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let s = self.palette;
        let dock = self.kind == Group::Documents;
        let tabs = self.kind != Group::Choices;
        let selected = self.items.iter().position(|item| item.selected);
        let active = selected.unwrap_or(0);
        let key = selected
            .map(|i| self.items[i].key.clone())
            .unwrap_or_default();
        let sensor = pro_selection_sensor();
        let state = window.use_keyed_state(pro_eid(&self.id, "navigation"), cx, |_, cx| {
            (
                cx.focus_handle().tab_stop(true),
                ScrollHandle::new(),
                None::<SharedString>,
            )
        });
        let (focus, scroll) = state.update(cx, |(focus, scroll, previous), _| {
            if dock && previous.as_ref() != Some(&key) {
                scroll.scroll_to_item(active + 1);
                *previous = Some(key.clone());
            }
            (focus.clone(), scroll.clone())
        });
        let actions: Vec<_> = self.items.iter().map(|item| item.select.clone()).collect();
        let key_focus = focus.clone();
        let mut strip = div()
            .id(self.id.clone())
            .font_features(tnum_features())
            .relative()
            .flex()
            .items_center()
            .gap(px(2.))
            .role(if tabs {
                Role::TabList
            } else {
                Role::RadioGroup
            })
            .aria_label(self.label)
            .when(dock, |d| {
                d.min_w_0().overflow_x_scroll().track_scroll(&scroll)
            })
            .when(!dock, |d| {
                d.max_w_full()
                    .flex_wrap()
                    .p(px(2.))
                    .rounded(px(R_CTRL))
                    .bg(s.tray)
            })
            .on_key_down(move |ev, win, cx| {
                if self.disabled || actions.is_empty() || !key_focus.is_focused(win) {
                    return;
                }
                let next = match ev.keystroke.key.as_str() {
                    "home" => 0,
                    "end" => actions.len() - 1,
                    "left" | "up" => (active + actions.len() - 1) % actions.len(),
                    "right" | "down" => (active + 1) % actions.len(),
                    _ => return,
                };
                actions[next](win, cx);
                key_focus.focus(win, cx);
                win.prevent_default();
                cx.stop_propagation();
            })
            .child(pro_pill_indicator(
                pro_eid(&self.id, "indicator"),
                key,
                sensor.clone(),
                R_SM,
                if dock { s.sidebar_selected } else { s.card },
                Some(s.line_soft),
                !dock,
            ));
        let has_other_tabs = self.items.len() > 1;
        for (i, item) in self.items.into_iter().enumerate() {
            let group = SharedString::from(format!("{}-{}", self.id, item.key));
            let id: ElementId = group.clone().into();
            let mut button = Tactile::ghost(id, &s)
                .role(if tabs { Role::Tab } else { Role::RadioButton })
                .disabled(self.disabled)
                .aria(item.label.clone())
                .radius(R_SM)
                .h(px(CTRL_SM_H))
                .px(px(if dock { 6. } else { 9. }))
                .justify_center()
                .gap(px(5.))
                .text_size(px(FS_CONTROL))
                .line_height(px(LH_CONTROL))
                .font_weight(FontWeight::MEDIUM)
                .ink(if item.selected { s.ink } else { s.muted }, s.ink)
                .child(
                    item.content
                        .unwrap_or_else(|| item.label.into_any_element()),
                );
            button.selection = Some(item.selected);
            button.tab_stop = active == i;
            if active == i && !self.disabled {
                button.focus = Some(focus.clone());
            }
            let press_focus = focus.clone();
            button.on_click = Some(Rc::new(move |_, win, cx| {
                (item.select)(win, cx);
                press_focus.focus(win, cx);
            }));
            strip = strip.child(
                div()
                    .relative()
                    .flex_none()
                    .flex()
                    .items_center()
                    .group(group.clone())
                    .when(item.selected, |d| d.child(pro_mark_selected(&sensor)))
                    .child(button)
                    .when_some(item.close, |d, mut close| {
                        close.tab_stop = item.selected;
                        if has_other_tabs && let Some(action) = close.on_click.take() {
                            let focus = focus.clone();
                            close.on_click = Some(Rc::new(move |event, win, cx| {
                                action(event, win, cx);
                                if matches!(event, ClickEvent::Keyboard(_)) {
                                    focus.focus(win, cx);
                                }
                            }));
                        }
                        d.pr(px(2.)).child(
                            div()
                                .when(!item.selected, |d| {
                                    d.opacity(0.).group_hover(group, |s| s.opacity(1.))
                                })
                                .child(close),
                        )
                    }),
            );
        }
        strip
    }
}
