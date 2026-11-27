//! Agent accounts, defaults for new conversations, and custom ACP connections.

use crate::ui_motion::selection::{Group, SelectionStrip};
use gpui::{prelude::*, *};
use kar::ApprovalMode;
use kui::{
    components::input::Input,
    primitives::{
        editor::{Editor, InputMode},
        pro_icons::{ProIcon, pro_icon},
    },
};

use crate::{
    kar_bridge::KarCommand,
    prefs::{CustomAcpAgent, all_approval_modes},
    types::{ProviderAvailability, ProviderReadiness, runtime_display_name},
    ui_motion::{
        Tactile, disclosure_line, icon_button_sm, overflow_label, primary_button_sm, soft_button,
        soft_button_sm, spinner, text_button_sm,
    },
    ui_primitives::{
        FS_CONTROL, FS_META, FS_TITLE, FS_UI, ICON_SM, ICON_XS, LH_META, LH_UI,
        R_SM, StudioPalette, card_box, mono_font, render_provider_color_tile,
        search_input_box, settings_divider, settings_input_action_row, settings_page_shell, settings_row_item,
        settings_section_title, sub_card_box, tnum_features,
    },
    workbench::KarWorkbench,
};

const AGENT_TILE: f32 = 28.;

impl crate::ui_primitives::UiLocale {
    pub(crate) fn approval_label(self, mode: ApprovalMode) -> &'static str {
        #[allow(unreachable_patterns)]
        match mode {
            ApprovalMode::Host => self.pick("每次确认", "Ask me"),
            ApprovalMode::Deny => self.pick("仅只读", "Read-only"),
            _ => self.pick("自动执行", "Full auto"),
        }
    }
}

impl KarWorkbench {
    pub(crate) fn new_chat_approval_picker(
        &self,
        id: &'static str,
        s: &StudioPalette,
        cx: &Context<Self>,
    ) -> SelectionStrip {
        let loc = self.ui_locale;
        SelectionStrip::options(
            id,
            loc.pick("新对话的权限请求", "Permission requests in new chats"),
            Group::Choices,
            all_approval_modes()
                .into_iter()
                .map(|mode| (mode, loc.approval_label(mode))),
            self.approval_mode,
            s,
            cx,
            |wb, mode, _, cx| wb.set_new_chat_approvals(mode, cx),
        )
    }

    pub(crate) fn render_agents_panel(
        &self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let stacked = f32::from(window.viewport_size().width) < 620.;
        let mut agents = if self.provider_accounts_checked { self.providers.clone() } else { Vec::new() };
        agents.sort_by_key(|a| a.readiness());

        let usable: Vec<_> = agents.iter().filter(|a| a.is_ready()).cloned().collect();
        let fixable: Vec<_> = agents
            .iter()
            .filter(|a| {
                matches!(
                    a.readiness(),
                    ProviderReadiness::NeedsLogin | ProviderReadiness::Broken
                )
            })
            .cloned()
            .collect();
        let absent: Vec<_> = agents
            .iter()
            .filter(|a| a.readiness() == ProviderReadiness::NotInstalled)
            .cloned()
            .collect();

        // ── 0. Interactive OAuth / Device-Code Banner (when active) ──────────
        let login_banner = self.provider_sign_in.challenge().map(|(rt_id, challenge)| {
            let url = challenge.url.clone();
            card_box(s)
                .p(px(16.))
                .border_color(s.pro.accent_info)
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(s.font_size(FS_UI))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(s.ink)
                                .child(format!(
                                    "{} · {}",
                                    runtime_display_name(rt_id),
                                    loc.pick("正在等待浏览器授权验证", "Waiting for sign-in")
                                )),
                        )
                        .child(
                            icon_button_sm("dismiss-login-prompt", ProIcon::Close, loc.pick("关闭", "Dismiss"), s)
                                .on_press(cx, |wb, _, cx| {
                                    wb.provider_sign_in.dismiss();
                                    cx.notify();
                                }),
                        ),
                )
                .when_some(challenge.device_code.clone(), |d, user_code| {
                    let to_copy = user_code.clone();
                    d.child(
                        sub_card_box(s)
                            .px(px(14.))
                            .py(px(10.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .font_family(mono_font(cx))
                                    .font_features(tnum_features())
                                    .text_size(s.font_size(FS_TITLE))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(s.ink)
                                    .child(user_code),
                            )
                            .child(
                                soft_button_sm("copy-device-code", loc.pick("复制验证码", "Copy code"), s)
                                    .on_press(cx, move |wb, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(to_copy.clone()));
                                        wb.notify_toast("已复制设备验证码", cx);
                                    }),
                            ),
                    )
                })
                .child(div().text_size(s.font_size(FS_META)).text_color(s.muted).child(challenge.message.clone()))
                .child(
                    div().flex().justify_end().child(
                        primary_button_sm("reopen-auth-url", loc.pick("在浏览器打开授权页 ↗", "Open sign-in page ↗"), s)
                            .on_press(cx, move |_, _, _| {
                                let _ = std::process::Command::new("open").arg(&url).status();
                            }),
                    ),
                )
        });

        let mut shell = settings_page_shell().when_some(login_banner, |page, banner| page.child(banner));

        if !self.provider_accounts_checked {
            shell = shell.child(
                card_box(s)
                    .px(px(16.))
                    .py(px(22.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(s.font_size(FS_UI))
                    .line_height(s.line_height(LH_UI))
                    .text_color(s.muted)
                    .child(spinner(ICON_SM, s.muted, window, cx))
                    .child(loc.pick(
                        "正在检查智能体与账号…",
                        "Checking agents and accounts…",
                    )),
            );
        }

        if self.provider_accounts_checked && agents.is_empty() {
            shell = shell.child(
                div()
                    .text_size(s.font_size(FS_UI))
                    .text_color(s.muted)
                    .child(loc.pick("还没有检测到智能体，可以重新扫描。", "No agents detected. Try scanning again.")),
            );
        }

        if !usable.is_empty() {
            let open_provider =
                window.use_keyed_state("agents-managed-provider", cx, |_, _| None::<String>);
            let mut ready_col = card_box(s).flex().flex_col();

            for (ix, agent) in usable.iter().enumerate() {
                let chosen = agent.id == self.selected_runtime;
                let expanded = open_provider.read(cx).as_deref() == Some(agent.id.as_str());
                let name = agent.display_name();
                let status = if agent.readiness() == ProviderReadiness::Ready {
                    loc.pick("已登录", "Signed in")
                } else {
                    loc.pick("已安装", "Installed")
                };
                let id = agent.id.clone();
                let default_action = if chosen {
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .px(px(6.))
                        .text_size(s.font_size(FS_META))
                        .text_color(s.muted)
                        .child(pro_icon(ProIcon::Check, ICON_XS, s.muted))
                        .child(loc.pick("当前默认", "Default"))
                        .into_any_element()
                } else {
                    soft_button_sm(("agent-usable", ix), loc.pick("设为默认", "Set default"), s)
                        .aria(format!("{} {name}", loc.pick("设为默认", "Set default")))
                        .on_press(cx, move |wb, _, cx| {
                            wb.select_runtime_by_id(&id, cx);
                            wb.save_agent_choice(cx);
                        })
                        .into_any_element()
                };
                let manage_label = format!("{} {name}", loc.pick("管理", "Manage"));
                let manage_id = SharedString::from(format!("provider-det-btn-{}", agent.id));
                let chevron = if expanded {
                    ProIcon::ChevronDown
                } else {
                    ProIcon::ChevronRight
                };
                let manage = if stacked {
                    icon_button_sm(manage_id, chevron, manage_label.clone(), s)
                } else {
                    text_button_sm(manage_id, loc.pick("管理", "Manage"), s)
                        .child(pro_icon(chevron, ICON_XS, s.muted))
                };
                let id = agent.id.clone();
                let active = open_provider.clone();
                let header = div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(render_provider_color_tile(&agent.id, AGENT_TILE, s, cx))
                    .child(agent_text(name, status.to_owned(), s))
                    .child(default_action)
                    .child(manage.aria(manage_label).expanded(expanded).on_press(
                        cx,
                        move |_, _, cx| {
                            active.update(cx, |value, _| *value = (!expanded).then(|| id.clone()));
                            cx.notify();
                        },
                    ));
                let mut card = div()
                    .min_w_0()
                    .px(px(12.))
                    .py(px(9.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(header);
                if ix > 0 {
                    ready_col = ready_col.child(settings_divider(s));
                }
                if !expanded {
                    ready_col = ready_col.child(card);
                    continue;
                }

                let runtime = kar::runtime(&agent.id).ok();
                let detail = agent.ready_detail_line_in(loc);
                let update = self.agent_update_statuses.get(&agent.id);
                let check_id = agent.id.clone();
                let upgrade_id = agent.id.clone();
                let logout_id = agent.id.clone();
                let supports_key = matches!(agent.id.as_str(), "opencode" | "pi" | "qwen");
                let actions = div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .when(crate::aster::is_aster(&agent.id), |row| {
                        row.child(
                            soft_button_sm(
                                ("aster-model-config", ix),
                                loc.pick("模型配置文件", "Model configuration"),
                                s,
                            )
                            .on_press(cx, |_, _, cx| {
                                if let Some(path) = crate::aster::config_path() {
                                    cx.reveal_path(&path);
                                }
                            }),
                        )
                    })
                    .when(runtime.is_some(), |row| {
                        row.child(
                            soft_button_sm(
                                ("agent-check-upd", ix),
                                loc.pick("检查更新", "Check for updates"),
                                s,
                            )
                            .on_press(cx, move |wb, _, _| {
                                wb.bridge.send(KarCommand::CheckOrUpgradeAgent {
                                    runtime_id: check_id.clone(),
                                    apply_upgrade: false,
                                });
                            }),
                        )
                    })
                    .when(update.is_some_and(|(_, available)| *available), |row| {
                        row.child(
                            soft_button_sm(("agent-apply-upg", ix), loc.pick("升级", "Update"), s)
                                .on_press(cx, move |wb, _, _| {
                                    wb.bridge.send(KarCommand::CheckOrUpgradeAgent {
                                        runtime_id: upgrade_id.clone(),
                                        apply_upgrade: true,
                                    });
                                }),
                        )
                    })
                    .when(runtime.is_some_and(|rt| rt.supports_logout()), |row| {
                        row.child(
                            text_button_sm(
                                ("agent-logout", ix),
                                loc.pick("退出账号", "Sign out"),
                                s,
                            )
                            .on_press(cx, move |wb, _, _| {
                                wb.bridge.send(KarCommand::LogoutProvider {
                                    runtime_id: logout_id.clone(),
                                });
                            }),
                        )
                    });
                card = card.child(
                    div()
                        .pt(px(8.))
                        .border_t_1()
                        .border_color(s.line_soft)
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .text_size(s.font_size(FS_META))
                        .line_height(s.line_height(LH_META))
                        .text_color(s.muted)
                        .when(!detail.is_empty(), |d| d.child(detail))
                        .when_some(update, |d, (summary, _)| d.child(summary.clone()))
                        .child(actions),
                );

                if supports_key {
                    card = card.child(self.provider_key_form(&agent.id, s, window, cx));
                }

                if chosen {
                    let model_search = window.use_keyed_state(
                        SharedString::from(format!("model-search-{}", agent.id)),
                        cx,
                        |win, cx| {
                            let mut ed = Editor::new(
                                loc.pick("搜索全部模型…", "Search all models…"),
                                InputMode::Search,
                                win,
                                cx,
                            );
                            ed.set_placeholder(loc.pick("搜索模型", "Search models"));
                            ed.font_size = FS_CONTROL;
                            ed
                        },
                    );
                    let show_hidden_models =
                        window.use_keyed_state("agents-show-hidden-models", cx, |_, _| false);
                    let custom_model_inp = window.use_keyed_state(
                        SharedString::from(format!("custom-model-{}", agent.id)),
                        cx,
                        |win, cx| {
                            Input::new(
                                "custom-model-id",
                                loc.pick("自定义模型 ID", "Custom model ID"),
                                win,
                                cx,
                            )
                            .with_placeholder("provider/model", cx)
                        },
                    );
                    let selected = self.selected_model.clone();
                    let mq = model_search.read(cx).value().trim().to_lowercase();
                    let m_focused = model_search.focus_handle(cx).is_focused(window);
                    let include_hidden = *show_hidden_models.read(cx);
                    let rt_id = agent.id.clone();

                    let mut catalog: Vec<(String, String)> = agent
                        .models
                        .iter()
                        .map(|m| {
                            let lbl = if m.label.trim().is_empty() {
                                m.id.clone()
                            } else {
                                m.label.clone()
                            };
                            (m.id.clone(), lbl)
                        })
                        .collect();
                    if let Some(customs) = self.prefs.custom_models.get(&rt_id) {
                        for cm in customs {
                            if !catalog.iter().any(|(id, _)| id == cm) {
                                catalog.push((cm.clone(), format!("{cm} · {}", loc.pick("自定义", "Custom"))));
                            }
                        }
                    }
                    // Sort favorites to the front
                    catalog.sort_by_key(|(mid, _)| !self.prefs.is_model_favorite(&rt_id, mid));

                    let mut pills = div().w_full().flex().flex_wrap().gap(px(6.));
                    // Auto-default chip
                    let auto_active = selected.is_none();
                    pills = pills.child(
                        Tactile::soft("model-chip-auto", s)
                            .h(px(28.))
                            .px(px(10.))
                            .radius(R_SM)
                            .lit(auto_active)
                            .outlined(
                                if auto_active { s.ink } else { s.line_soft },
                                s.line_strong,
                            )
                            .text_size(s.font_size(FS_META))
                            .child(loc.pick("使用智能体默认模型", "Agent default"))
                            .on_press(cx, |wb, _, cx| {
                                wb.selected_model = None;
                                wb.save_agent_choice(cx);
                                cx.notify();
                            }),
                    );

                    for (mix, (mid, mlbl)) in catalog.into_iter().enumerate() {
                        let is_hidden = self.prefs.is_model_hidden(&rt_id, &mid);
                        if is_hidden && !include_hidden && selected.as_ref() != Some(&mid) {
                            continue;
                        }
                        if !mq.is_empty()
                            && !mid.to_lowercase().contains(&mq)
                            && !mlbl.to_lowercase().contains(&mq)
                        {
                            continue;
                        }
                        let is_sel = selected.as_ref() == Some(&mid);
                        let is_fav = self.prefs.is_model_favorite(&rt_id, &mid);
                        let mid_sel = mid.clone();
                        let mid_fav = mid.clone();
                        let mid_hide = mid.clone();
                        let rt_fav = rt_id.clone();
                        let rt_hide = rt_id.clone();

                        pills = pills.child(
                            div()
                                .max_w_full()
                                .h(px(28.))
                                .pl(px(8.))
                                .pr(px(4.))
                                .rounded(px(R_SM))
                                .bg(if is_sel { s.active } else { s.surface })
                                .border_1()
                                .border_color(if is_sel { s.ink } else { s.line_soft })
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(
                                    Tactile::ghost(("model-fav", mix), s)
                                        .size(px(20.))
                                        .justify_center()
                                        .tip(loc.pick("收藏置顶模型", "Favorite model"))
                                        .text_size(s.font_size(FS_META))
                                        .text_color(if is_fav { s.pro.amber } else { s.faint })
                                        .child(if is_fav { "★" } else { "☆" })
                                        .on_press(cx, move |wb, _, cx| {
                                            wb.prefs.toggle_model_favorite(&rt_fav, &mid_fav);
                                            wb.save_prefs();
                                            cx.notify();
                                        }),
                                )
                                .child(
                                    Tactile::ghost(("model-pick", mix), s)
                                        .min_w_0()
                                        .aria(mlbl.clone())
                                        .h(px(22.))
                                        .px(px(4.))
                                        .text_size(s.font_size(FS_META))
                                        .font_weight(if is_sel {
                                            FontWeight::SEMIBOLD
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(if is_hidden { s.muted } else { s.ink })
                                        .child(
                                            div().min_w_0().truncate().child(overflow_label("model-name", mlbl)),
                                        )
                                        .on_press(cx, move |wb, _, cx| {
                                            wb.selected_model = Some(mid_sel.clone());
                                            wb.save_agent_choice(cx);
                                            cx.notify();
                                        }),
                                )
                                .child(
                                    Tactile::ghost(("model-hide", mix), s)
                                        .size(px(20.))
                                        .justify_center()
                                        .tip(if is_hidden {
                                            loc.pick("取消隐藏", "Unhide")
                                        } else {
                                            loc.pick("隐藏此模型", "Hide model")
                                        })
                                        .child(pro_icon(ProIcon::Eye, 11., if is_hidden { s.pro.amber } else { s.faint }))
                                        .on_press(cx, move |wb, _, cx| {
                                            wb.prefs.toggle_model_hidden(&rt_hide, &mid_hide);
                                            wb.save_prefs();
                                            cx.notify();
                                        }),
                                ),
                        );
                    }

                    let cm_inp = custom_model_inp.clone();
                    let rt_cm = rt_id.clone();
                    let sh_toggle = show_hidden_models.clone();
                    card = card.child(
                        div()
                            .pt(px(10.))
                            .border_t_1()
                            .border_color(s.line_soft)
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .flex_wrap()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_size(s.font_size(FS_META))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.muted)
                                            .child(loc.pick("默认模型", "Default model")),
                                    )
                                    .child(
                                        div()
                                            .max_w_full()
                                            .flex()
                                            .flex_wrap()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(
                                                text_button_sm(
                                                    "toggle-hidden-models",
                                                    if include_hidden {
                                                        loc.pick("收起已隐藏模型", "Hide hidden")
                                                    } else {
                                                        loc.pick("显示已隐藏模型", "Show hidden")
                                                    },
                                                    s,
                                                )
                                                .on_press(cx, move |_, _, cx| {
                                                    sh_toggle.update(cx, |v, _| *v = !*v);
                                                    cx.notify();
                                                }),
                                            )
                                            .child(
                                                div().min_w_0().w(px(if stacked { 128. } else { 210. })).child(search_input_box(
                                                    "agents-model-search-box",
                                                    model_search.clone(),
                                                    m_focused,
                                                    true,
                                                    s,
                                                )),
                                            ),
                                    ),
                            )
                            .child(pills)
                            .child(
                                settings_input_action_row()
                                    .child(div().flex_1().min_w_0().child(custom_model_inp.clone()))
                                    .child(
                                        soft_button("add-custom-model-btn", loc.pick("添加并选用", "Add model"), s)
                                            .on_press(cx, move |wb, win, cx| {
                                                let v = cm_inp.read(cx).value(cx).trim().to_owned();
                                                if !v.is_empty() {
                                                    let list = wb
                                                        .prefs
                                                        .custom_models
                                                        .entry(rt_cm.clone())
                                                        .or_default();
                                                    if !list.contains(&v) {
                                                        list.push(v.clone());
                                                    }
                                                    wb.selected_model = Some(v);
                                                    wb.save_prefs();
                                                    wb.save_agent_choice(cx);
                                                    cm_inp.update(cx, |i, cx| i.set_value("", win, cx));
                                                    cx.notify();
                                                }
                                            }),
                                    ),
                            ),
                    );
                }

                ready_col = ready_col.child(card);
            }
            shell = shell.child(
                div()
                    .child(settings_section_title(loc.pick("可用智能体", "Available agents"), s))
                    .child(ready_col),
            );
        }

        if !fixable.is_empty() {
            let mut accounts = card_box(s).flex().flex_col();
            for (ix, agent) in fixable.iter().enumerate() {
                let name = agent.display_name();
                let id = &agent.id;
                let needs_login = agent.readiness() == ProviderReadiness::NeedsLogin;
                let browser_login = needs_login && kar::runtime(id).is_ok_and(|rt| rt.supports_login());
                let key_login = needs_login && matches!(id.as_str(), "opencode" | "pi" | "qwen");
                let detail = match &agent.availability {
                    ProviderAvailability::Failed { reason } => {
                        format!("{} · {reason}", loc.pick("无法启动", "Could not start"))
                    }
                    _ if browser_login || key_login => loc.pick("未登录", "Signed out").to_owned(),
                    _ => loc
                        .pick(
                            "请在智能体客户端登录，然后重新检测。",
                            "Sign in using the agent’s own client, then check again.",
                        )
                        .to_owned(),
                };
                let mut row = div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(render_provider_color_tile(id, AGENT_TILE, s, cx))
                    .child(agent_text(name.clone(), detail, s));
                if browser_login {
                    let pending = self.provider_sign_in.waiting_for(id);
                    let id = id.clone();
                    row = row.child(
                        soft_button_sm(("agent-direct-login", ix), if pending { loc.pick("登录中…", "Signing in…") } else { loc.pick("登录", "Sign in") }, s)
                            .disabled(pending)
                            .aria(format!("{} {name}", loc.pick("登录", "Sign in to")))
                            .on_press(cx, move |wb, _, _| {
                                wb.bridge.send(KarCommand::LoginProvider {
                                    runtime_id: id.clone(),
                                    sub_provider: None,
                                    api_key: None,
                                });
                            }),
                    );
                } else if !key_login {
                    row = row.child(
                        soft_button_sm(("agent-recheck", ix), loc.pick("重新检测", "Check again"), s)
                            .on_press(cx, |wb, _, _| wb.bridge.send(KarCommand::ProbeProvidersOnly)),
                    );
                }
                let mut entry = div()
                    .px(px(12.))
                    .py(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(row);
                if key_login {
                    entry = entry.child(self.provider_key_form(id, s, window, cx));
                }
                if ix > 0 {
                    accounts = accounts.child(settings_divider(s));
                }
                accounts = accounts.child(entry);
            }
            shell = shell.child(
                div()
                    .child(settings_section_title(if usable.is_empty() { loc.pick("已安装的智能体", "Installed agents") } else { loc.pick("其他已安装的智能体", "Other installed agents") }, s))
                    .child(accounts),
            );
        }

        if !absent.is_empty() {
            let open = self.agents_show_uninstalled.unwrap_or_else(|| {
                usable.is_empty()
                    && !fixable
                        .iter()
                        .any(|agent| agent.readiness() == ProviderReadiness::NeedsLogin)
            });
            let mut add = div().flex().flex_col().items_start().gap(px(8.)).child(
                agent_disclosure(
                    "agents-toggle-absent",
                    ProIcon::Plus,
                    loc.pick("添加智能体", "Add an agent"),
                    open,
                    s,
                )
                .on_press(cx, move |wb, _, cx| {
                    wb.agents_show_uninstalled = Some(!open);
                    cx.notify();
                }),
            );
            if open {
                let mut list = card_box(s).w_full().flex().flex_col();
                for (ix, agent) in absent.iter().enumerate() {
                    let name = agent.display_name();
                    let id = &agent.id;
                    let mut row = div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(render_provider_color_tile(id, AGENT_TILE, s, cx))
                        .child(agent_text(name.clone(), String::new(), s));
                    let mut details = None;
                    if agent.can_install && agent.install_lines.len() <= 1 {
                        let id = id.clone();
                        let line = agent.install_lines.first().map(|(line, _)| line.clone());
                        row = row.child(
                            soft_button_sm(("agent-oneclick-install", ix), loc.pick("安装", "Install"), s)
                                .aria(format!("{} {name}", loc.pick("安装", "Install")))
                                .on_press(cx, move |wb, _, _| {
                                    wb.bridge.send(KarCommand::InstallAgent {
                                        runtime_id: id.clone(),
                                        line: line.clone(),
                                    })
                                }),
                        );
                    } else {
                        let state = window.use_keyed_state(
                            SharedString::from(format!("agent-install-options-{id}")),
                            cx,
                            |_, _| false,
                        );
                        let expanded = *state.read(cx);
                        let action = if agent.can_install {
                            loc.pick("选择版本", "Choose version")
                        } else {
                            loc.pick("手动安装", "Manual setup")
                        };
                        row = row.child(
                            text_button_sm(("agent-install-options", ix), action, s)
                                .aria(format!("{name} · {action}"))
                                .expanded(expanded)
                                .child(pro_icon(
                                    if expanded {
                                        ProIcon::ChevronDown
                                    } else {
                                        ProIcon::ChevronRight
                                    },
                                    ICON_XS,
                                    s.muted,
                                ))
                                .on_press(cx, move |_, _, cx| {
                                    state.update(cx, |open, _| *open = !expanded);
                                    cx.notify();
                                }),
                        );
                        if expanded {
                            let mut body = div().min_w_0().flex().flex_wrap().gap(px(6.));
                            if agent.can_install {
                                for (line, description) in &agent.install_lines {
                                    let id = id.clone();
                                    let line_id = line.clone();
                                    body = body.child(
                                        soft_button_sm(
                                            SharedString::from(format!("agent-install-line-{id}-{line}")),
                                            format!("{} {line}", loc.pick("安装", "Install")),
                                            s,
                                        )
                                        .tip(description.clone())
                                        .on_press(
                                            cx,
                                            move |wb, _, _| {
                                                wb.bridge.send(KarCommand::InstallAgent {
                                                    runtime_id: id.clone(),
                                                    line: Some(line_id.clone()),
                                                });
                                            },
                                        ),
                                    );
                                }
                            } else {
                                body = body
                                    .text_size(s.font_size(FS_META))
                                    .text_color(s.muted)
                                    .child(match loc {
                                        crate::ui_primitives::UiLocale::Zh => {
                                            format!("从 {name} 官方渠道安装命令行工具，然后重新检测。")
                                        }
                                        crate::ui_primitives::UiLocale::En => {
                                            format!("Install the CLI from {name}, then check again.")
                                        }
                                    })
                                    .child(
                                        text_button_sm(
                                            ("agent-install-recheck", ix),
                                            loc.pick("重新检测", "Check again"),
                                            s,
                                        )
                                        .on_press(cx, |wb, _, _| {
                                            wb.bridge.send(KarCommand::ProbeProvidersOnly)
                                        }),
                                    );
                            }
                            details = Some(body);
                        }
                    }
                    if ix > 0 {
                        list = list.child(settings_divider(s));
                    }
                    list = list.child(
                        div()
                            .px(px(12.))
                            .py(px(8.))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(row)
                            .when_some(details, |col, body| col.child(body)),
                    );
                }
                add = add.child(list);
            }
            shell = shell.child(add);
        }

        let advanced = window.use_keyed_state("agents-advanced-open", cx, |_, _| false);
        let advanced_open = *advanced.read(cx);
        let advanced_button = agent_disclosure(
            "agents-toggle-advanced",
            ProIcon::Sliders,
            loc.pick("高级设置", "Advanced settings"),
            advanced_open,
            s,
        )
        .on_press(cx, move |_, _, cx| {
            advanced.update(cx, |open, cx| {
                *open = !*open;
                cx.notify();
            });
        });
        shell = shell.child(div().flex().items_start().child(advanced_button));
        if !advanced_open {
            return shell.into_any_element();
        }

        let effort_menu = crate::ui_motion::menu_host(
            soft_button(
                "settings-effort",
                self.selected_effort()
                    .unwrap_or(loc.pick("使用模型默认值", "Model default"))
                    .to_owned(),
                s,
            )
            .child(pro_icon(ProIcon::ChevronDown, 12., s.muted))
            .on_press(cx, |this, _, cx| {
                this.toggle_menu(crate::workbench::Menu::SettingsEffort, cx)
            }),
            self.render_effort_menu(
                crate::workbench::Menu::SettingsEffort,
                crate::ui_motion::MenuPlacement::BelowEnd,
                s,
                cx,
            ),
        );
        let approval_seg = self.new_chat_approval_picker("settings-approval", s, cx);
        let wt_seg = SelectionStrip::options(
            "settings-worktree",
            loc.pick("工作目录", "Working directory"),
            Group::Choices,
            [
                (false, loc.pick("当前项目目录", "Checkout")),
                (true, loc.pick("隔离分支目录", "Worktree")),
            ],
            self.new_chat_use_worktree,
            s,
            cx,
            |wb, enabled, _, cx| wb.set_new_chat_worktree(enabled, cx),
        );

        let routing_card = card_box(s)
            .flex()
            .flex_col()
            .child(settings_row_item(
                loc.pick("新对话默认执行权限", "Default permission mode"),
                loc.pick("选择智能体请求执行操作时的处理方式。", "Choose how to handle action requests."),
                approval_seg.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("工作目录", "Working directory"),
                loc.pick("在当前项目或独立的 Git 工作树中修改文件。", "Edit files in this project or a separate Git worktree."),
                wt_seg.into_any_element(),
                stacked,
                s,
            ))
            .when(!self.offered_efforts().is_empty(), |d| {
                d.child(settings_divider(s)).child(settings_row_item(
                    loc.pick("推理深度", "Reasoning effort"),
                    loc.pick("使用所选模型支持的推理档位。", "Levels reported by the selected model."),
                    effort_menu.into_any_element(),
                    stacked,
                    s,
                ))
            });

        let acp_name_inp = window.use_keyed_state("acp-custom-name", cx, |win, cx| {
            Input::new("acp-name", loc.pick("智能体显示名", "Agent name"), win, cx)
                .with_placeholder("My ACP Agent", cx)
        });
        let acp_cmd_inp = window.use_keyed_state("acp-custom-cmd", cx, |win, cx| {
            Input::new("acp-cmd", loc.pick("ACP 启动命令", "ACP command"), win, cx)
                .with_placeholder("goose acp", cx)
        });
        let an = acp_name_inp.clone();
        let ac = acp_cmd_inp.clone();

        let search = window.use_keyed_state("acp-registry-query", cx, |w, cx| {
            Input::new("acp-search", loc.pick("搜索社区智能体插件", "Search ACP registry"), w, cx)
        });
        let query = search.clone();
        let mut acp_card = card_box(s).flex().flex_col().child(
            settings_input_action_row()
                .p(px(14.))
                .child(div().flex_1().min_w_0().child(search))
                .child(
                    soft_button("acp-search-submit", loc.pick("搜索", "Search"), s).on_press(
                        cx,
                        move |wb, _, cx| {
                            wb.bridge.send(KarCommand::Registry(crate::registry::Action::Search(
                                query.read(cx).value(cx).into(),
                            )))
                        },
                    ),
                ),
        );
        for (ix, installed) in self.registry_installed.iter().cloned().enumerate() {
            acp_card = acp_card.child(settings_divider(s)).child(div().flex().px(px(14.)).py(px(6.)).items_center().justify_between()
                .child(div().flex_1().min_w_0().truncate().child(installed.name.clone()))
                .child(soft_button_sm(("acp-remove", ix), loc.pick("移除", "Remove"), s)
                    .on_press(cx, move |wb, _, _| wb.bridge.send(KarCommand::Registry(crate::registry::Action::Remove(installed.clone()))))));
        }
        for (ix, item) in self.registry_catalog.iter().enumerate() {
            if self.registry_installed.iter().any(|i| i.id == item.id) { continue; }
            let id = item.id.clone();
            acp_card = acp_card.child(settings_divider(s)).child(div().flex().px(px(14.)).py(px(6.)).items_center().justify_between()
                .child(div().flex_1().min_w_0().truncate().child(format!("{} · {}",item.name,item.version)))
                .child(soft_button_sm(("acp-install", ix), loc.pick("安装", "Install"), s)
                    .tip(item.description.clone())
                    .on_press(cx, move |wb, _, _| wb.bridge.send(KarCommand::Registry(crate::registry::Action::Prepare(id.clone()))))));
        }
        acp_card = acp_card.child(settings_divider(s)).child(
            div()
                .p(px(14.))
                .flex()
                .flex_col()
                .items_start()
                .gap(px(8.))
                .child(div().w_full().child(acp_name_inp))
                .child(div().w_full().child(acp_cmd_inp))
                .child(
                    soft_button("acp-add-custom", loc.pick("添加自定义智能体", "Add ACP"), s)
                        .on_press(cx, move |wb, win, cx| {
                            let name = an.read(cx).value(cx).trim().to_owned();
                            let raw_cmd = ac.read(cx).value(cx).trim().to_owned();
                            if name.is_empty() || raw_cmd.is_empty() {
                                return;
                            }
                            let Ok(mut parts) = shell_words::split(&raw_cmd) else {
                                wb.notify_warning(loc.pick("请检查命令中的引号", "Check the quoting in the command"), cx);
                                return;
                            };
                            if parts.is_empty() { return; }
                            let command = parts.remove(0);
                            wb.bridge.send(KarCommand::Registry(crate::registry::Action::Register(CustomAcpAgent {
                                id: format!("custom-{}", uuid::Uuid::new_v4().simple()), name, command, args: parts, enabled: true,
                            })));
                            an.update(cx, |i, cx| i.set_value("", win, cx));
                            ac.update(cx, |i, cx| i.set_value("", win, cx));
                            cx.notify();
                        }),
                ),
        );

        shell
            .child(
                div()
                    .child(settings_section_title(loc.pick("新对话偏好", "New conversation defaults"), s))
                    .child(routing_card),
            )
            .child(
                div()
                    .child(settings_section_title(loc.pick("自定义智能体", "Custom agents"), s))
                    .child(acp_card),
            )
            .into_any_element()
    }

    fn provider_key_form(
        &self,
        id: &str,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        use kui::components::password_field::PasswordField;

        let loc = self.ui_locale;
        let open = window.use_keyed_state(SharedString::from(format!("agent-key-open-{id}")), cx, |_, _| {
            false
        });
        if !*open.read(cx) {
            return div().child(
                soft_button_sm(
                    SharedString::from(format!("agent-configure-key-{id}")),
                    loc.pick("配置密钥", "Set API key"),
                    s,
                )
                .expanded(false)
                .on_press(cx, move |_, _, cx| {
                    open.update(cx, |v, _| *v = true);
                    cx.notify();
                }),
            );
        }
        let input = window.use_keyed_state(
            SharedString::from(format!("agent-key-input-{id}")),
            cx,
            |win, cx| {
                PasswordField::new(
                    SharedString::from(format!("agent-api-key-{id}")),
                    "OpenAI API Key",
                    win,
                    cx,
                )
            },
        );
        let clear = input.clone();
        let close = open.clone();
        let save = input.clone();
        let runtime_id = id.to_owned();
        div().min_w_0().flex().flex_col().gap(px(6.)).child(input).child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    soft_button_sm(
                        SharedString::from(format!("agent-save-key-{id}")),
                        loc.pick("保存密钥", "Save key"),
                        s,
                    )
                    .on_press(cx, move |wb, win, cx| {
                        let key = save.read(cx).value(cx).trim();
                        if key.is_empty() {
                            save.read(cx).editor().focus_handle(cx).focus(win, cx);
                            return;
                        }
                        wb.bridge.send(KarCommand::LoginProvider {
                            runtime_id: runtime_id.clone(),
                            sub_provider: Some("openai".into()),
                            api_key: Some(key.to_owned()),
                        });
                        save.update(cx, |v, cx| v.set_value("", win, cx));
                        open.update(cx, |v, _| *v = false);
                        cx.notify();
                    }),
                )
                .child(
                    text_button_sm(
                        SharedString::from(format!("agent-cancel-key-{id}")),
                        loc.pick("取消", "Cancel"),
                        s,
                    )
                    .on_press(cx, move |_, win, cx| {
                        clear.update(cx, |v, cx| v.set_value("", win, cx));
                        close.update(cx, |v, _| *v = false);
                        cx.notify();
                    }),
                ),
        )
    }
}

// Use the same icon and label columns for disclosure controls and account rows.
fn agent_disclosure(id: &'static str, glyph: ProIcon, label: &'static str, open: bool, s: &StudioPalette) -> Tactile {
    let leading = div()
        .size(px(AGENT_TILE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(pro_icon(glyph, ICON_SM, s.muted));
    disclosure_line(id, leading.into_any_element(), label, open, s)
        .ml_0()
        .px(px(13.))
}

fn agent_text(name: String, detail: String, s: &StudioPalette) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .truncate()
                .text_size(s.font_size(FS_UI))
                .font_weight(FontWeight::MEDIUM)
                .text_color(s.ink)
                .child(name),
        )
        .when(!detail.is_empty(), |d| {
            d.child(
                div()
                    .text_size(s.font_size(FS_META))
                    .line_height(s.line_height(LH_META))
                    .text_color(s.muted)
                    .child(detail),
            )
        })
}
