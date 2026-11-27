//! Connect an installed agent before opening the first conversation.
use gpui::{prelude::*, *};
use kui::primitives::pro_icons::{ProIcon, pro_icon};

use crate::{
    kar_bridge::KarCommand,
    onboarding_audio::UiSfx,
    types::{ProviderAvailability, ProviderReadiness, runtime_display_name},
    ui_agents::accepts_provider_key,
    ui_motion::{Tactile, primary_button, spinner, text_button_sm},
    ui_primitives::{
        FS_META, FS_UI, ICON_SM, StudioPalette, card_box, mono_font, render_provider_color_tile,
    },
    ui_settings::SettingsTab,
    workbench::KarWorkbench,
};

impl KarWorkbench {
    pub(crate) fn finish_ready_onboarding_login(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.provider_accounts_checked {
            return false;
        }
        let Some(id) = &self.onboarding_provider else {
            return false;
        };
        if !crate::aster::is_aster(id)
            && self
                .providers
                .iter()
                .any(|p| p.id == *id && p.readiness() == ProviderReadiness::Ready)
        {
            let id = id.clone();
            self.select_runtime_by_id(&id, cx);
            self.save_agent_choice(cx);
            self.complete_onboarding(window, cx);
            return true;
        }
        false
    }

    fn choose_onboarding_provider(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.providers.iter().find(|p| {
            p.id == id && !crate::aster::is_aster(&p.id) && self.provider_accounts_checked
        }) else {
            return;
        };
        if agent.is_ready() {
            self.select_runtime_by_id(id, cx);
            self.save_agent_choice(cx);
            self.complete_onboarding(window, cx);
        } else {
            self.play_onboarding_sfx(UiSfx::Confirm, cx);
            self.onboarding_provider = Some(id.to_owned());
            if agent.readiness() == ProviderReadiness::NeedsLogin
                && kar::runtime(id).is_ok_and(|runtime| runtime.supports_login())
                && !self.provider_sign_in.waiting_for(id)
            {
                self.bridge.send(KarCommand::LoginProvider {
                    runtime_id: id.to_owned(),
                    sub_provider: None,
                    api_key: None,
                });
            }
            cx.notify();
        }
    }

    fn clear_onboarding_provider(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.onboarding_provider.take()
            && self.provider_sign_in.waiting_for(&id)
        {
            self.provider_sign_in.dismiss();
        }
        cx.notify();
    }

    pub(crate) fn render_onboarding_providers(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let s = StudioPalette::get(cx);
        let loc = self.ui_locale;
        let compact = f32::from(window.viewport_size().height) < 520.;
        let chosen = self.onboarding_provider.as_deref();
        let title = match chosen {
            Some(id) => format!(
                "{} {}",
                loc.pick("连接", "Connect to"),
                runtime_display_name(id)
            ),
            None => loc.pick("选择智能体", "Choose an agent").to_owned(),
        };

        let mut body = div().flex().flex_col().gap(px(4.));
        let checking = !self.provider_accounts_checked;
        if let Some(id) = chosen {
            let mut inner = div().p(px(14.)).flex().flex_col().gap(px(12.));
            // Auth URLs and device codes belong to a single attempt and a single provider.
            if let Some((_, challenge)) = self
                .provider_sign_in
                .challenge()
                .filter(|(runtime, _)| *runtime == id)
            {
                inner = inner.child(self.provider_challenge_content(
                    &challenge.url,
                    challenge.device_code.as_deref(),
                    &challenge.message,
                    &s,
                    cx,
                ));
            } else if checking || self.provider_sign_in.waiting_for(id) {
                inner = inner.child(
                    div()
                        .py(px(6.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_color(s.muted)
                        .child(spinner(ICON_SM, s.muted, window, cx))
                        .child(loc.pick("正在检查账号…", "Checking account…")),
                );
            } else if let Some(agent) = self.providers.iter().find(|p| p.id == id) {
                let browser_login = agent.readiness() == ProviderReadiness::NeedsLogin
                    && kar::runtime(id).is_ok_and(|rt| rt.supports_login());
                let key_login =
                    agent.readiness() == ProviderReadiness::NeedsLogin && accepts_provider_key(id);
                if browser_login {
                    let id = id.to_owned();
                    inner = inner.child(
                        primary_button(
                            "onboarding-browser-login",
                            loc.pick("在浏览器中登录", "Sign in with browser"),
                            &s,
                        )
                        .on_press(cx, move |wb, _, cx| {
                            wb.play_onboarding_sfx(UiSfx::Confirm, cx);
                            if !wb.provider_sign_in.waiting_for(&id) {
                                wb.bridge.send(KarCommand::LoginProvider {
                                    runtime_id: id.clone(),
                                    sub_provider: None,
                                    api_key: None,
                                });
                            }
                        }),
                    );
                }
                if key_login {
                    inner = inner.child(self.provider_key_form(id, &s, window, cx));
                }
                if !browser_login && !key_login {
                    let message = match &agent.availability {
                        ProviderAvailability::Failed { reason } => reason.clone(),
                        ProviderAvailability::Missing => loc
                            .pick(
                                "此设备上未检测到这个智能体。",
                                "This agent was not found on this device.",
                            )
                            .to_owned(),
                        _ => loc
                            .pick(
                                "请在智能体客户端登录，然后重新检测。",
                                "Sign in using the agent’s own client, then check again.",
                            )
                            .to_owned(),
                    };
                    inner = inner.child(div().text_color(s.muted).child(message));
                }
            } else {
                inner = inner.child(div().text_color(s.muted).child(loc.pick(
                    "此设备上未检测到这个智能体。",
                    "This agent was not found on this device.",
                )));
            }
            body = body.child(card_box(&s).child(inner));
        } else if checking {
            body = body.child(
                card_box(&s).p(px(16.)).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_color(s.muted)
                        .child(spinner(ICON_SM, s.muted, window, cx))
                        .child(
                            loc.pick("正在检查此设备的账号…", "Checking accounts on this device…"),
                        ),
                ),
            );
        } else {
            let mut installed: Vec<_> = self
                .providers
                .iter()
                .filter(|p| {
                    !crate::aster::is_aster(&p.id)
                        && p.readiness() != ProviderReadiness::NotInstalled
                })
                .collect();
            installed.sort_by_key(|p| (p.readiness(), p.id != self.selected_runtime));
            if installed.is_empty() {
                body = body.child(
                    card_box(&s)
                        .p(px(16.))
                        .child(div().text_color(s.muted).child(loc.pick(
                            "此设备上还没有检测到智能体。",
                            "No agents found on this device.",
                        ))),
                );
            } else {
                let mut list_card = card_box(&s).p(px(5.)).flex().flex_col().gap(px(2.));
                for agent in installed {
                    let name = agent.display_name();
                    let readiness = agent.readiness();
                    let action = match readiness {
                        ProviderReadiness::Ready => loc.pick("已登录", "Signed in"),
                        ProviderReadiness::AccountUnverified => loc.pick("已安装", "Installed"),
                        ProviderReadiness::NeedsLogin => loc.pick("登录", "Sign in"),
                        ProviderReadiness::Broken => loc.pick("检查连接", "Check connection"),
                        ProviderReadiness::NotInstalled => continue,
                    };
                    let dot_color = match readiness {
                        ProviderReadiness::Ready => Some(rgb(0x10b981)),
                        ProviderReadiness::Broken => Some(rgb(0xf59e0b)),
                        _ => None,
                    };
                    let id = agent.id.clone();
                    list_card = list_card.child(
                        Tactile::ghost(SharedString::from(format!("onboarding-choose-{id}")), &s)
                            .w_full()
                            .min_w_0()
                            .h(px(48.))
                            .px(px(10.))
                            .radius(8.)
                            .gap(px(12.))
                            .aria(format!("{name} · {action}"))
                            .child(render_provider_color_tile(&id, 30., &s, cx))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(s.ink)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(name),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .when_some(dot_color, |d, col| {
                                        d.child(div().size(px(6.)).rounded_full().bg(col))
                                    })
                                    .child(
                                        div()
                                            .text_size(s.font_size(FS_META))
                                            .text_color(if readiness == ProviderReadiness::Ready {
                                                s.text
                                            } else {
                                                s.muted
                                            })
                                            .child(action),
                                    ),
                            )
                            .child(pro_icon(ProIcon::ChevronRight, 13., s.faint))
                            .on_press(cx, move |wb, win, cx| {
                                wb.choose_onboarding_provider(&id, win, cx)
                            }),
                    );
                }
                body = body.child(list_card);
            }
        }

        let footer = div()
            .flex_none()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.))
            .when(chosen.is_none() && !checking, |d| {
                d.child(
                    text_button_sm(
                        "onboarding-add-agent",
                        loc.pick("添加智能体…", "Add an agent…"),
                        &s,
                    )
                    .on_press(cx, |wb, win, cx| {
                        wb.skip_onboarding(win, cx);
                        wb.open_settings(SettingsTab::Agents, cx);
                        wb.agents_show_uninstalled = Some(true);
                    }),
                )
            })
            .child(
                text_button_sm("onboarding-rescan", loc.pick("重新检测", "Check again"), &s)
                    .disabled(self.providers_probing)
                    .on_press(cx, |wb, _, cx| {
                        wb.play_onboarding_sfx(UiSfx::Navigate, cx);
                        wb.bridge.send(KarCommand::ProbeProvidersOnly);
                    }),
            )
            .child(div().flex_1())
            .child(
                text_button_sm(
                    "onboarding-skip-picker",
                    loc.pick("稍后设置", "Set up later"),
                    &s,
                )
                .on_press(cx, |wb, win, cx| {
                    wb.clear_onboarding_provider(cx);
                    wb.skip_onboarding(win, cx);
                }),
            );

        div()
            .size_full()
            .bg(s.stage)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px(px(20.))
            .pt(px(if compact { 32. } else { 48. }))
            .pb(px(18.))
            .text_size(s.font_size(FS_UI))
            .child(
                div()
                    .w_full()
                    .max_w(px(452.))
                    .max_h_full()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap(px(if compact { 10. } else { 14. }))
                    .when(chosen.is_some(), |d| {
                        d.child(
                            div().flex_none().child(
                                text_button_sm(
                                    "onboarding-back-agents",
                                    loc.pick("返回智能体列表", "Back to agents"),
                                    &s,
                                )
                                .on_press(cx, |wb, _, cx| {
                                    wb.play_onboarding_sfx(UiSfx::Navigate, cx);
                                    wb.clear_onboarding_provider(cx);
                                }),
                            ),
                        )
                    })
                    .when(chosen.is_none() && !compact, |d| {
                        d.child(
                            div()
                                .flex_none()
                                .px(px(4.))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .font_family(mono_font(cx))
                                .text_size(px(10.5))
                                .text_color(s.muted)
                                .child(div().size(px(5.)).rounded_full().bg(rgb(0x10b981)))
                                .child("KENSE AGENT ONLINE // SELECT RUNTIME"),
                        )
                    })
                    .child(
                        div()
                            .flex_none()
                            .px(px(4.))
                            .text_size(s.font_size(if compact { 22. } else { 26. }))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(s.ink)
                            .child(title),
                    )
                    .child(
                        div()
                            .id("onboarding-agent-list")
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(body),
                    )
                    .child(footer),
            )
            .into_any_element()
    }
}
