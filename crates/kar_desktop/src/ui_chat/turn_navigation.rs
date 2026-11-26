//! Connect the conversation rail to the same laid-out children that GPUI scrolls.
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::Instant,
};

use super::{turn_outline, turn_rail};
use crate::{
    kar_bridge::ProjectedChatState,
    ui_primitives::{StudioPalette, UiLocale},
    workbench::KarWorkbench,
};
use gpui::{prelude::*, *};
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct TurnNavigation {
    scope: Option<Uuid>,
    previews: BTreeMap<Uuid, turn_outline::TurnPreview>,
    marks: BTreeSet<Uuid>,
    entries: Option<(UiLocale, Rc<[turn_rail::TurnRailEntry]>)>,
    jump: Option<Jump>,
    landed: Option<(usize, f32)>,
}

struct Jump {
    index: usize,
    from: f32,
    start: Instant,
}

impl TurnNavigation {
    pub(crate) fn invalidate(&mut self) {
        self.previews.clear();
        self.entries = None;
    }

    pub(crate) fn update_turns(
        &mut self,
        previous: &ProjectedChatState,
        next: &ProjectedChatState,
    ) {
        if previous.chat_id != next.chat_id
            || previous.info.as_ref().map(|info| &info.native_id)
                != next.info.as_ref().map(|info| &info.native_id)
        {
            self.invalidate();
            self.cancel_jump();
            return;
        }
        // Finalized turns are immutable on ordinary streaming updates. Only reparse
        // Markdown for turns that actually changed, including a rewritten/forked history.
        let mut changed = previous.turns.len() != next.turns.len();
        for (old, new) in previous.turns.iter().zip(&next.turns) {
            if old != new {
                self.previews.remove(&old.id);
                self.previews.remove(&new.id);
                changed = true;
            }
        }
        if changed {
            self.entries = None;
            if previous.turns.len() > next.turns.len() {
                self.previews
                    .retain(|id, _| next.turns.iter().any(|turn| turn.id == *id));
                self.cancel_jump();
            }
        }
    }

    pub(crate) fn cancel_jump(&mut self) {
        self.jump = None;
        self.landed = None;
    }
}

impl KarWorkbench {
    pub(super) fn jump_to_rail_turn(
        &mut self,
        index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.projected.turns.len() {
            return;
        }
        self.turn_navigation.landed = None;
        self.turn_navigation.jump = Some(Jump {
            index,
            from: f32::from(self.chat_scroll.offset().y),
            start: Instant::now(),
        });
        cx.notify();
    }

    fn advance_turn_jump(&mut self, window: &mut Window, cx: &App) {
        let Some(jump) = self.turn_navigation.jump.as_ref() else {
            return;
        };
        let Some(anchor) = self.chat_scroll.bounds_for_item(jump.index) else {
            self.turn_navigation.cancel_jump();
            return;
        };
        let max_y = f32::from(self.chat_scroll.max_offset().y);
        let target = f32::from(self.chat_scroll.bounds().top() - anchor.top()).clamp(-max_y, 0.);
        let progress = if cx.reduce_motion() || (target - jump.from).abs() < 1. {
            1.
        } else {
            (jump.start.elapsed().as_secs_f32() / 0.20).min(1.)
        };
        let y = target + (jump.from - target) * (1. - progress).powi(3);
        self.chat_scroll
            .set_offset(point(self.chat_scroll.offset().x, px(y)));
        if progress < 1. {
            window.request_animation_frame();
        } else {
            self.turn_navigation.landed = Some((jump.index, target));
            self.turn_navigation.jump = None;
        }
    }

    pub(super) fn toggle_rail_bookmark(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some((scope, turn)) = self
            .turn_navigation
            .scope
            .zip(self.projected.turns.get(index))
        else {
            return;
        };
        let id = turn.id;
        let marked = !self.turn_navigation.marks.contains(&id);
        match turn_outline::set_bookmark(&self.config.chat_dir, scope, id, marked) {
            Ok(()) => {
                if marked {
                    self.turn_navigation.marks.insert(id);
                } else {
                    self.turn_navigation.marks.remove(&id);
                }
                self.turn_navigation.entries = None;
                cx.notify();
            }
            Err(error) => self.notify_warning(
                format!(
                    "{} · {error}",
                    self.ui_locale
                        .pick("无法保存书签", "Could not save bookmark")
                ),
                cx,
            ),
        }
    }

    pub(super) fn render_turn_navigation(
        &mut self,
        width: f32,
        inset: f32,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let first = self.projected.turns.first()?;
        let scope = self
            .projected
            .chat_id
            .or_else(|| self.current_chat_id())
            .unwrap_or(first.id);
        if self.turn_navigation.scope != Some(scope) {
            let marks =
                turn_outline::bookmarks(&self.config.chat_dir, scope).unwrap_or_else(|error| {
                    log::warn!("Could not read turn bookmarks: {error}");
                    BTreeSet::new()
                });
            self.turn_navigation = TurnNavigation {
                scope: Some(scope),
                marks,
                ..Default::default()
            };
        }
        let loc = self.ui_locale;
        if self
            .turn_navigation
            .entries
            .as_ref()
            .is_none_or(|(lang, _)| *lang != loc)
        {
            let root = self
                .projected
                .info
                .as_ref()
                .map(|info| info.native_id.as_str());
            let mut entries = Vec::with_capacity(self.projected.turns.len());
            for (index, turn) in self.projected.turns.iter().enumerate() {
                let preview = self
                    .turn_navigation
                    .previews
                    .entry(turn.id)
                    .or_insert_with(|| {
                        turn_outline::preview(turn, root, self.turn_targets.get(&turn.id))
                    });
                let title = if preview.question.is_empty() {
                    match loc {
                        UiLocale::Zh => format!("第 {} 轮", index + 1),
                        UiLocale::En => format!("Turn {}", index + 1),
                    }
                } else {
                    preview.question.clone()
                };
                entries.push(turn_rail::TurnRailEntry {
                    title: title.into(),
                    reply: preview.answer.clone().into(),
                    bookmarked: self.turn_navigation.marks.contains(&turn.id),
                });
            }
            self.turn_navigation.entries = Some((loc, entries.into()));
        }
        self.advance_turn_jump(window, cx);
        let offset = f32::from(self.chat_scroll.offset().y);
        let max = f32::from(self.chat_scroll.max_offset().y);
        let current = self
            .turn_navigation
            .jump
            .as_ref()
            .map(|jump| jump.index)
            .or_else(|| {
                self.turn_navigation
                    .landed
                    .filter(|(_, y)| (offset - y).abs() <= 1.)
                    .map(|(index, _)| index)
            })
            .unwrap_or_else(|| {
                if max > 1. && max + offset < 2. {
                    self.projected.turns.len() - 1
                } else {
                    self.chat_scroll.top_item()
                }
            })
            .min(self.projected.turns.len() - 1);
        let height = f32::from(self.chat_scroll.bounds().size.height);
        let entries = self.turn_navigation.entries.as_ref()?.1.clone();
        let rail = turn_rail::render(
            scope.to_string().into(),
            entries,
            current,
            inset,
            width,
            height,
            loc,
            s,
            window,
            cx,
        );
        let handle = self.chat_scroll.clone();
        // Layout (including the first frame or a growing composer) resolves after render.
        // Schedule one refresh when the measured viewport differs from the one the rail used.
        let view = cx.entity().downgrade();
        let measure = canvas(
            move |_, window, _| {
                if (f32::from(handle.bounds().size.height) - height).abs() > 0.5 {
                    let view = view.clone();
                    window.on_next_frame(move |_, cx| {
                        let _ = view.update(cx, |_, cx| cx.notify());
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0();
        Some(
            div()
                .absolute()
                .inset_0()
                .child(measure)
                .child(rail)
                .into_any_element(),
        )
    }
}
