//! Full-access acknowledgement dialog and shared modal actions.

use super::workspace::conversation_bg;
use super::*;
use serde::Serialize;

pub(super) struct PermissionDialogFocus {
    close: gpui::FocusHandle,
    acknowledgement: gpui::FocusHandle,
    cancel: gpui::FocusHandle,
    confirm: gpui::FocusHandle,
}

impl PermissionDialogFocus {
    pub(super) fn new(cx: &App) -> Self {
        Self {
            close: cx.focus_handle(),
            acknowledgement: cx.focus_handle(),
            cancel: cx.focus_handle(),
            confirm: cx.focus_handle(),
        }
    }
}

#[derive(PartialEq, Eq, Serialize)]
pub(super) struct AccessiblePermissionRisk {
    session: String,
    preset: String,
    from_command: bool,
    title: &'static str,
    description: &'static str,
    acknowledgement: &'static str,
    action: &'static str,
    acknowledged: bool,
}

impl DshApp {
    pub(super) fn open_permission_confirmation(
        &mut self,
        preset: String,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_permission = Some(preset);
        self.full_access_acknowledged = false;
        self.permission_dialog_focus.acknowledgement.focus(w, cx);
    }

    pub(super) fn close_permission_confirmation(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        self.confirm_permission = None;
        self.full_access_acknowledged = false;
        if self.composer_menu == Some(ComposerMenu::PermissionChoices) {
            self.focus_command_picker(w, cx);
        } else {
            self.permission_menu.trigger.focus(w, cx);
        }
        cx.notify();
    }

    pub(super) fn handle_permission_confirmation_key(
        &mut self,
        combo: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.confirm_permission.is_none() {
            return false;
        }
        match combo {
            "Escape" => self.close_permission_confirmation(w, cx),
            "Tab" | "shift+Tab" => {
                let focus = &self.permission_dialog_focus;
                let controls = [
                    &focus.close,
                    &focus.acknowledgement,
                    &focus.cancel,
                    &focus.confirm,
                ];
                let count = usize::from(self.full_access_acknowledged) + 3;
                let index = controls[..count]
                    .iter()
                    .position(|control| control.is_focused(w));
                let next = index.map_or(1, |index| {
                    (index + if combo == "Tab" { 1 } else { count - 1 }) % count
                });
                controls[next].focus(w, cx);
            }
            _ => return false,
        }
        true
    }

    fn confirm_permission_change(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.full_access_acknowledged
            && let Some(preset) = self.confirm_permission.take()
        {
            self.close_permission_confirmation(w, cx);
            self.apply_permission(preset, true);
        }
    }

    pub(super) fn act_on_permission_risk(
        &mut self,
        action: &str,
        target: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok((session, preset)) = serde_json::from_str::<(String, String)>(target) else {
            return;
        };
        if self.active_session.as_deref() != Some(session.as_str())
            || self.confirm_permission.as_deref() != Some(preset.as_str())
        {
            return;
        }
        match action {
            "permission-risk-acknowledge" => {
                self.full_access_acknowledged = !self.full_access_acknowledged;
                cx.notify();
            }
            "permission-risk-confirm" => self.confirm_permission_change(w, cx),
            "permission-risk-cancel" => self.close_permission_confirmation(w, cx),
            _ => {}
        }
    }

    pub(super) fn accessible_permission_risk(&self) -> Option<AccessiblePermissionRisk> {
        let session = self.active_session.clone()?;
        let preset = self.confirm_permission.clone()?;
        let (title, description, acknowledgement, action) = permission_risk(&preset);
        Some(AccessiblePermissionRisk {
            session,
            preset,
            from_command: self.composer_menu == Some(ComposerMenu::PermissionChoices),
            title,
            description,
            acknowledgement,
            action,
            acknowledged: self.full_access_acknowledged,
        })
    }

    pub(super) fn render_full_access_dialog(
        &mut self,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = dsh(cx);
        let width = (f32::from(w.viewport_size().width) - 48.).clamp(0., 440.);
        let acknowledged = self.full_access_acknowledged;
        let (title, description, acknowledgement, action) =
            permission_risk(self.confirm_permission.as_deref().unwrap_or_default());
        let focus = &self.permission_dialog_focus;
        div()
            .id("full-access-dialog")
            .absolute()
            .inset_0()
            .p(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .bg(p.bg_mask_1)
            .occlude()
            .on_click(cx.listener(|this, _, w, cx| this.close_permission_confirmation(w, cx)))
            .child(
                column()
                    .id("full-access-dialog-panel")
                    .role(gpui::Role::Dialog)
                    .aria_label(title)
                    .w(px(width))
                    .max_h_full()
                    .min_h_0()
                    .gap(px(20.))
                    .pb(px(24.))
                    .rounded(px(28.))
                    .bg(p.bg_layer_2)
                    .shadow(elevation_prominent())
                    .occlude()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
                    .child(
                        column()
                            .id("permission-risk-content")
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(dialog_header_with_warning(
                                title,
                                "close-full-access-dialog",
                                Some(&focus.close),
                                true,
                                cx,
                                |this, w, cx| this.close_permission_confirmation(w, cx),
                            ))
                            .child(
                                div()
                                    .px(px(24.))
                                    .text_size(px(14.))
                                    .line_height(px(22.))
                                    .text_color(p.secondary)
                                    .child(description),
                            )
                            .child(
                                row()
                                    .id("full-access-acknowledgement")
                                    .role(gpui::Role::CheckBox)
                                    .aria_label(acknowledgement)
                                    .aria_selected(acknowledged)
                                    .track_focus(&focus.acknowledgement)
                                    .tab_index(0)
                                    .w_full()
                                    .px(px(24.))
                                    .mt(px(16.))
                                    .items_start()
                                    .gap(px(8.))
                                    .cursor_pointer()
                                    .child(
                                        div()
                                            .size(px(14.))
                                            .mt(px(3.))
                                            .flex_none()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px(3.))
                                            .border(px(1.))
                                            .border_color(if acknowledged {
                                                p.ink
                                            } else {
                                                p.faint
                                            })
                                            .when(acknowledged, |element| {
                                                element.bg(p.ink).child(icon(
                                                    Glyph::Check,
                                                    11.,
                                                    conversation_bg(cx),
                                                ))
                                            }),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .text_color(p.secondary)
                                            .child(acknowledgement),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.full_access_acknowledged =
                                            !this.full_access_acknowledged;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        row()
                            .w_full()
                            .flex_none()
                            .px(px(24.))
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                dialog_action(
                                    "cancel-full-access",
                                    "取消",
                                    false,
                                    false,
                                    72.,
                                    cx,
                                    |this, w, cx| this.close_permission_confirmation(w, cx),
                                )
                                .track_focus(&focus.cancel),
                            )
                            .child(
                                dialog_action(
                                    "confirm-full-access",
                                    action,
                                    true,
                                    !acknowledged,
                                    136.,
                                    cx,
                                    |this, w, cx| this.confirm_permission_change(w, cx),
                                )
                                .track_focus(&focus.confirm),
                            ),
                    ),
            )
    }
}

fn permission_risk(preset: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    if preset == "auto" {
        (
            "确认启用自动审查（实验）？",
            "自动审查不使用沙箱。每次原生工具调用和 PTC 内层调用前，都会由与当前 agent 相同的模型进行审查；审查拒绝的调用由你批准或拒绝。此功能仍属实验性，可能误放行或误拒绝，并会消耗额外 token。",
            "我已了解这些风险，并愿意继续",
            "启用自动审查",
        )
    } else {
        (
            "确认启用完全权限？",
            "启用完全权限后，智能体将减少确认步骤，并且可以直接执行更多操作，包括敏感操作、文件修改或外部命令。仅建议在你信任当前任务时使用。",
            "我已了解风险，并愿意继续",
            "启用完全权限",
        )
    }
}

pub(super) fn dialog_header(
    title: impl Into<gpui::SharedString>,
    id: &'static str,
    focus: Option<&gpui::FocusHandle>,
    cx: &mut Context<DshApp>,
    action: impl Fn(&mut DshApp, &mut Window, &mut Context<DshApp>) + 'static,
) -> gpui::Div {
    dialog_header_with_warning(title, id, focus, false, cx, action)
}

pub(super) fn dialog_header_with_warning(
    title: impl Into<gpui::SharedString>,
    id: &'static str,
    focus: Option<&gpui::FocusHandle>,
    warning: bool,
    cx: &mut Context<DshApp>,
    action: impl Fn(&mut DshApp, &mut Window, &mut Context<DshApp>) + 'static,
) -> gpui::Div {
    let p = dsh(cx);
    row()
        .w_full()
        .pl(px(24.))
        .pr(px(14.))
        .pt(px(22.))
        .pb(px(12.))
        .gap(px(8.))
        .items_center()
        .justify_between()
        .child(
            row()
                .flex_1()
                .min_w_0()
                .gap(px(8.))
                .items_start()
                .when(warning, |title| {
                    title.child(
                        div().pt(px(3.)).flex_none().child(
                            gpui::svg()
                                .data(include_bytes!(
                                    "../../assets/plugin-install-warning-outline.svg"
                                ))
                                .size(px(18.))
                                .text_color(p.state_warn_primary),
                        ),
                    )
                })
                .child(
                    div()
                        .min_w_0()
                        .text_size(px(16.))
                        .line_height(px(24.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.ink)
                        .child(title.into()),
                ),
        )
        .child(super::dock_action_button(
            div()
                .id(id)
                .role(gpui::Role::Button)
                .aria_label("关闭")
                .when_some(focus, |button, focus| button.track_focus(focus))
                .tab_index(0)
                .size(px(28.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|style| style.bg(p.hover))
                .child(
                    gpui::svg()
                        .data(include_bytes!("../../assets/session-log-close.svg"))
                        .size(px(14.))
                        .text_color(p.label_secondary),
                ),
            cx.listener(move |this, _, w, cx| action(this, w, cx)),
        ))
}

pub(super) fn dialog_action(
    id: &'static str,
    label: &'static str,
    primary: bool,
    disabled: bool,
    min_width: f32,
    cx: &mut Context<DshApp>,
    action: impl Fn(&mut DshApp, &mut Window, &mut Context<DshApp>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let p = dsh(cx);
    let mut button = div()
        .id(id)
        .role(gpui::Role::Button)
        .aria_label(label)
        .tab_index(0)
        .tab_stop(!disabled)
        .h(px(36.))
        .min_w(px(min_width))
        .px(px(14.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(12.))
        .border(px(0.5))
        .border_color(if primary {
            gpui::transparent_black()
        } else {
            p.border_l3
        })
        .text_size(px(14.))
        .line_height(px(22.))
        .text_color(if primary {
            p.label_primary_foreground
        } else {
            p.label_primary
        })
        .child(label);
    if primary {
        button = button.bg(p.button_primary_fill);
    }
    if disabled {
        button
            .opacity(0.4)
            .cursor(gpui::CursorStyle::OperationNotAllowed)
    } else {
        super::dock_action_button(
            button.cursor_pointer().hover(move |style| {
                style.bg(if primary {
                    p.button_primary_hover
                } else {
                    p.hover
                })
            }),
            cx.listener(move |this, _, w, cx| action(this, w, cx)),
        )
    }
}
