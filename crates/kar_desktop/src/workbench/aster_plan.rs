use super::*;
use crate::ui_motion::soft_button_sm;

#[derive(Default)]
pub(super) struct AsterPlanning {
    pub(super) draft: bool,
    active: Option<(Uuid, bool)>,
    pending: Option<(Uuid, Option<String>)>,
}

impl KarWorkbench {
    pub(super) fn supports_aster_planning(&self) -> bool {
        if self.hosted_writer.is_some() || self.aster_switching() || self.turn_in_progress() {
            return false;
        }
        match self.access_mode {
            ChatAccessMode::NoneSelected => {
                crate::aster::is_aster(&self.selected_runtime)
                    && self
                        .providers
                        .iter()
                        .any(|p| crate::aster::is_aster(&p.id) && p.is_ready())
            }
            ChatAccessMode::WriteController { chat_id } => {
                self.projected.chat_id == Some(chat_id)
                    && self
                        .current_chat_info()
                        .is_some_and(|i| crate::aster::is_aster(&i.runtime))
            }
            _ => false,
        }
    }

    pub(super) fn aster_switching(&self) -> bool {
        self.aster_planning
            .pending
            .as_ref()
            .is_some_and(|(id, _)| Some(*id) == self.current_chat_id())
    }

    fn aster_in_plan(&self) -> bool {
        if matches!(self.access_mode, ChatAccessMode::NoneSelected) {
            return self.aster_planning.draft && crate::aster::is_aster(&self.selected_runtime);
        }
        let Some(info) = self.current_chat_info() else {
            return false;
        };
        if !crate::aster::is_aster(&info.runtime) || Some(info.id) != self.current_chat_id() {
            return false;
        }
        self.aster_planning
            .active
            .filter(|(id, _)| *id == info.id)
            .map_or(
                info.settings.acp_mode.as_deref() == Some("plan"),
                |(_, plan)| plan,
            )
    }

    pub(super) fn enter_aster_planning(
        &mut self,
        topic: &str,
        submit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.supports_aster_planning() {
            return;
        }
        self.composer_editor.update(cx, |ed, cx| {
            ed.set_value(topic.to_owned(), window, cx);
            ed.focus(window, cx);
        });
        let topic = (submit && !topic.trim().is_empty()).then(|| topic.to_owned());
        if self.aster_in_plan() || matches!(self.access_mode, ChatAccessMode::NoneSelected) {
            if matches!(self.access_mode, ChatAccessMode::NoneSelected) {
                self.aster_planning.draft = true;
            }
            if topic.is_some() {
                self.submit_composer_prompt(window, cx);
            }
        } else if let ChatAccessMode::WriteController { chat_id } = self.access_mode {
            self.aster_planning.pending = Some((chat_id, topic));
            self.bridge.send(KarCommand::SetAsterPlanMode {
                chat_id,
                plan: true,
            });
        }
        cx.notify();
    }

    fn leave_aster_plan(&mut self, cx: &mut Context<Self>) {
        if self.turn_in_progress() || self.aster_switching() {
            return;
        }
        if let ChatAccessMode::WriteController { chat_id } = self.access_mode {
            self.aster_planning.pending = Some((chat_id, None));
            self.bridge.send(KarCommand::SetAsterPlanMode {
                chat_id,
                plan: false,
            });
        } else {
            self.aster_planning.draft = false;
        }
        cx.notify();
    }

    pub(super) fn aster_mode_result(
        &mut self,
        chat_id: Uuid,
        result: Result<bool, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pending = if self
            .aster_planning
            .pending
            .as_ref()
            .is_some_and(|p| p.0 == chat_id)
        {
            self.aster_planning
                .pending
                .take()
                .and_then(|(_, topic)| topic)
        } else {
            None
        };
        if self.current_chat_id() != Some(chat_id) {
            return;
        }
        match result {
            Ok(plan) => {
                self.aster_planning.active = Some((chat_id, plan));
                if plan
                    && let Some(topic) = pending
                    && self.composer_editor.read(cx).value().trim() == topic.trim()
                {
                    self.submit_composer_prompt(window, cx);
                }
            }
            Err(detail) => self.notify_toast(detail, cx),
        }
        cx.notify();
    }

    pub(crate) fn render_aster_mode(
        &self,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let switching = self.aster_switching();
        if !switching && !self.aster_in_plan() {
            return None;
        }
        let loc = self.ui_locale;
        let label = if switching {
            loc.pick("正在切换模式…", "Switching mode…")
        } else {
            loc.pick("Aster 计划模式", "Aster plan mode")
        };
        let can_leave = !switching
            && !self.turn_in_progress()
            && matches!(
                self.access_mode,
                ChatAccessMode::WriteController { .. } | ChatAccessMode::NoneSelected
            );
        Some(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .justify_between()
                .pb(px(6.))
                .text_size(px(12.))
                .text_color(s.muted)
                .child(label)
                .when(can_leave, |d| {
                    d.child(
                        soft_button_sm(
                            "aster-leave-plan",
                            loc.pick("退出计划模式", "Exit plan mode"),
                            s,
                        )
                        .tip(loc.pick(
                            "切回普通模式后，可以继续追问或要求实施计划",
                            "Return to normal mode to continue or implement the plan",
                        ))
                        .on_press(cx, |this: &mut Self, _, cx| this.leave_aster_plan(cx)),
                    )
                })
                .into_any_element(),
        )
    }
}
