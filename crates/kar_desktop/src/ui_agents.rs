//! Providers, Accounts, Full Model Catalog, Fallback/Quota Routing & Community ACP Registry.

use crate::ui_motion::selection::{Group, SelectionStrip};
use gpui::{prelude::*, *};
use kar::ApprovalMode;
use kui::{
    components::{input::Input, pro_chrome::pro_fold},
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
        Tactile, disclosure_line, icon_button_sm,
        primary_button_sm, soft_button, soft_button_sm, spinner, text_button_sm,
    },
    ui_primitives::{
        DotTone, FS_HERO, FS_META, FS_TAG, FS_UI, ICON_SM, ICON_XS, LH_META, LH_UI,
        R_PILL, R_SM, StudioPalette, card_box, mono_font, render_provider_color_tile,
        render_status, search_input_box, settings_divider, settings_page_shell, settings_row_item,
        settings_section_title, sub_card_box, tnum_features,
    },
    workbench::KarWorkbench,
};

impl crate::ui_primitives::UiLocale {
    pub(crate) fn approval_label(self, mode: ApprovalMode) -> &'static str {
        #[allow(unreachable_patterns)]
        match mode {
            ApprovalMode::Host => self.pick("询问我", "Ask me"),
            ApprovalMode::Deny => self.pick("自动拒绝", "Auto-deny"),
            _ => self.pick("全自动执行", "Full auto"),
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
        let mut agents = self.providers.clone();
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
        let login_banner = self.login_prompt.as_ref().map(|(rt_id, url_opt, code_opt, msg)| {
            let url_clone = url_opt.clone();
            let code_clone = code_opt.clone();
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
                                .text_size(px(FS_UI))
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
                                    wb.login_prompt = None;
                                    cx.notify();
                                }),
                        ),
                )
                .when_some(code_clone, |d, user_code| {
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
                                    .text_size(px(FS_HERO))
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
                .child(div().text_size(px(FS_META)).text_color(s.muted).child(msg.clone()))
                .when_some(url_clone, |d, url| {
                    d.child(
                        div().flex().justify_end().child(
                            primary_button_sm("reopen-auth-url", loc.pick("在浏览器打开授权页 ↗", "Open sign-in page ↗"), s)
                                .on_press(cx, move |_, _, _| {
                                    let _ = std::process::Command::new("open").arg(&url).status();
                                }),
                        ),
                    )
                })
        });

        // ── 1. Default Routing, Autonomy, Fallback & Quota Recovery Card ─────
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
                (false, loc.pick("当前仓库目录", "Checkout")),
                (true, loc.pick("独立 Git 工作树", "Worktree")),
            ],
            self.new_chat_use_worktree,
            s,
            cx,
            |wb, enabled, _, cx| wb.set_new_chat_worktree(enabled, cx),
        );

        let mut fb_opts = vec![("none".to_owned(), loc.pick("不启用备用", "None").to_owned())];
        for ag in &usable {
            fb_opts.push((ag.id.clone(), ag.display_name()));
        }
        let fb_current = self
            .prefs
            .fallback_runtime
            .clone()
            .unwrap_or_else(|| "none".into());
        let fallback_strip = SelectionStrip::options(
            "settings-fallback-rt",
            loc.pick("备用兜底智能体", "Fallback agent"),
            Group::Choices,
            fb_opts,
            fb_current,
            s,
            cx,
            |wb, id, _, cx| {
                wb.prefs.fallback_runtime = (id != "none").then_some(id);
                wb.save_prefs();
                cx.notify();
            },
        );

        let auto_resume_sw = Self::settings_switch(
            "agents-quota-auto-resume",
            loc.pick("额度重置后自动唤醒排队任务", "Auto-resume after quota reset"),
            self.prefs.auto_resume_after_quota_reset,
            |wb, cx| {
                wb.prefs.auto_resume_after_quota_reset = !wb.prefs.auto_resume_after_quota_reset;
                wb.save_prefs();
                cx.notify();
            },
            s,
            window,
            cx,
        );

        let routing_card = card_box(s)
            .flex()
            .flex_col()
            .when(!self.offered_efforts().is_empty(), |d| {
                d.child(settings_row_item(
                    loc.pick("思考深度", "Reasoning effort"),
                    loc.pick("当前模型提供的推理档位。", "Levels reported by the selected model."),
                    effort_menu.into_any_element(),
                    stacked,
                    s,
                ))
                .child(settings_divider(s))
            })
            .child(settings_row_item(
                loc.pick("默认权限策略", "Default permission mode"),
                loc.pick("询问我（安全默认）、全自动放行（沙箱/工作树推荐）或自动拒绝。", "Ask me, Full auto, or Auto-deny for new chats."),
                approval_seg.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("新建对话隔离模式", "New thread isolation"),
                loc.pick("独立工作树会为新对话自动拉取分支，互不干扰代码改动。", "Create an isolated Git branch and worktree per thread."),
                wt_seg.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("备用兜底智能体", "Fallback provider"),
                loc.pick("首选账号遇到限流或网络异常时一键切换接力的候选智能体。", "Candidate provider when primary hits rate limits."),
                fallback_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("额度冷却后自动续跑", "Quota cooldown recovery"),
                loc.pick("遇到账号窗口限额时保留上下文并在冷却恢复后自动提醒继续。", "Preserve context on rate limits and resume when quota resets."),
                auto_resume_sw,
                stacked,
                s,
            ));

        let mut shell = settings_page_shell()
            .when_some(login_banner, |d, b| d.child(b))
            .child(
                div()
                    .child(settings_section_title(
                        loc.pick("新对话默认策略与容灾兜底", "Defaults & Quota Recovery"),
                        s,
                    ))
                    .child(routing_card),
            );

        if agents.is_empty() && self.providers_probing {
            shell = shell.child(
                card_box(s)
                    .px(px(16.))
                    .py(px(22.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(FS_UI))
                    .line_height(px(LH_UI))
                    .text_color(s.muted)
                    .child(spinner(ICON_SM, s.muted, window, cx))
                    .child(loc.pick(
                        "正在并行检查本机已安装的智能体与可用模型…",
                        "Checking installed agents and models…",
                    )),
            );
        }

        // Shared model search & custom model input state
        let model_search = window.use_keyed_state("agents-model-search", cx, |win, cx| {
            let mut ed = Editor::new(
                loc.pick("搜索全部模型…", "Search all models…"),
                InputMode::Search,
                win,
                cx,
            );
            ed.set_placeholder(loc.pick("搜索模型名称或 ID…", "Search model name or ID…"));
            ed.font_size = FS_UI;
            ed
        });
        let show_hidden_models = window.use_keyed_state("agents-show-hidden-models", cx, |_, _| false);
        let custom_model_inp = window.use_keyed_state("agents-custom-model-inp", cx, |win, cx| {
            Input::new("custom-model-id", loc.pick("添加自定义模型 ID", "Custom model ID"), win, cx)
                .with_placeholder("例如：claude-sonnet-4-5 或 gpt-5.1-codex", cx)
        });
        let api_key_drawer = window.use_keyed_state("agents-apikey-open", cx, |_, _| None::<String>);
        let api_key_inp = window.use_keyed_state("agents-apikey-val", cx, |win, cx| {
            Input::new("agent-api-key", "API Key", win, cx).with_placeholder("sk-...", cx)
        });

        // ── 2. Ready Agents Fleet ────────────────────────────────────────────
        if !usable.is_empty() {
            let mut ready_col = div().flex().flex_col().gap(px(10.));

            for (ix, agent) in usable.iter().enumerate() {
                let chosen = agent.id == self.selected_runtime;
                let id = agent.id.clone();
                let id_for_upd = agent.id.clone();
                let id_for_upg = agent.id.clone();
                let id_for_logout = agent.id.clone();
                let id_for_key_btn = agent.id.clone();
                let detail = agent.ready_detail_line_in(loc);
                let store = window.use_keyed_state(
                    SharedString::from(format!("provider-detail-{}", agent.id)),
                    cx,
                    |_, _| false,
                );
                let expanded = *store.read(cx);
                let (status, tone) = match agent.readiness() {
                    ProviderReadiness::Ready => (loc.pick("已登录就绪", "Signed in"), DotTone::Success),
                    _ => (loc.pick("已安装", "Installed"), DotTone::Neutral),
                };
                let status_line = render_status(status, tone, s);
                let upd_info = self.agent_update_statuses.get(&agent.id).cloned();
                let supports_key_cfg = matches!(agent.id.as_str(), "opencode" | "pi" | "qwen");
                let key_open = api_key_drawer.read(cx).as_deref() == Some(agent.id.as_str());

                let mut card = card_box(s)
                    .p(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .flex_wrap()
                            .gap(px(10.))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .gap(px(11.))
                                    .child(render_provider_color_tile(&agent.id, 36., s, cx))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .child(
                                                        div()
                                                            .text_size(px(FS_UI))
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_color(s.ink)
                                                            .child(agent.display_name()),
                                                    )
                                                    .when_some(
                                                        match &agent.availability {
                                                            ProviderAvailability::Available {
                                                                version,
                                                                ..
                                                            } if !version.is_empty() => {
                                                                Some(version.clone())
                                                            }
                                                            _ => None,
                                                        },
                                                        |d, v| {
                                                            d.child(
                                                                div()
                                                                    .px(px(6.))
                                                                    .rounded(px(R_PILL))
                                                                    .bg(s.tray)
                                                                    .font_family(mono_font(cx))
                                                                    .text_size(px(FS_TAG))
                                                                    .text_color(s.muted)
                                                                    .child(v),
                                                            )
                                                        },
                                                    )
                                                    .when_some(upd_info.as_ref(), |d, (summary, _)| {
                                                        d.child(
                                                            div()
                                                                .px(px(6.))
                                                                .rounded(px(R_PILL))
                                                                .bg(s.pro.accent_soft)
                                                                .text_size(px(FS_TAG))
                                                                .text_color(s.pro.accent_info)
                                                                .child(summary.clone()),
                                                        )
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .child(status_line)
                                                    .when(!detail.is_empty(), |d| {
                                                        d.child(
                                                            text_button_sm(
                                                                SharedString::from(format!("provider-det-btn-{}", agent.id)),
                                                                loc.pick("连接详情", "Details"),
                                                                s,
                                                            )
                                                            .on_press(cx, move |_, _, cx| {
                                                                store.update(cx, |o, _| *o = !*o);
                                                                cx.notify();
                                                            }),
                                                        )
                                                    }),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .flex_wrap()
                                    .gap(px(6.))
                                    .when(crate::aster::is_aster(&agent.id), |row| {
                                        row.child(
                                            soft_button_sm(("aster-model-config", ix), loc.pick("模型配置文件", "Model configuration"), s)
                                                .tip(loc.pick("在文件管理器中显示 Aster 的模型与凭据配置", "Reveal the Aster model and credential configuration"))
                                                .on_press(cx, |_, _, cx| {
                                                    if let Some(path) = crate::aster::config_path() {
                                                        cx.reveal_path(&path);
                                                    }
                                                }),
                                        )
                                    })
                                    .when(supports_key_cfg, |row| {
                                        let drawer = api_key_drawer.clone();
                                        row.child(
                                            soft_button_sm(
                                                ("agent-cfg-key", ix),
                                                loc.pick("配置密钥", "API Key"),
                                                s,
                                            )
                                            .on_press(cx, move |_, _, cx| {
                                                drawer.update(cx, |cur, _| {
                                                    if cur.as_deref() == Some(id_for_key_btn.as_str()) {
                                                        *cur = None;
                                                    } else {
                                                        *cur = Some(id_for_key_btn.clone());
                                                    }
                                                });
                                                cx.notify();
                                            }),
                                        )
                                    })
                                    .child(
                                        soft_button_sm(
                                            ("agent-check-upd", ix),
                                            loc.pick("检查更新", "Check update"),
                                            s,
                                        )
                                        .on_press(cx, move |wb, _, _| {
                                            wb.bridge.send(KarCommand::CheckOrUpgradeAgent {
                                                runtime_id: id_for_upd.clone(),
                                                apply_upgrade: false,
                                            });
                                        }),
                                    )
                                    .when(upd_info.is_some_and(|(_, avail)| avail), |row| {
                                        row.child(
                                            primary_button_sm(
                                                ("agent-apply-upg", ix),
                                                loc.pick("一键升级", "Upgrade"),
                                                s,
                                            )
                                            .on_press(cx, move |wb, _, _| {
                                                wb.bridge.send(KarCommand::CheckOrUpgradeAgent {
                                                    runtime_id: id_for_upg.clone(),
                                                    apply_upgrade: true,
                                                });
                                            }),
                                        )
                                    })
                                    .child(
                                        text_button_sm(
                                            ("agent-logout", ix),
                                            loc.pick("退出账号", "Sign out"),
                                            s,
                                        )
                                        .on_press(cx, move |wb, _, _| {
                                            wb.bridge.send(KarCommand::LogoutProvider {
                                                runtime_id: id_for_logout.clone(),
                                            });
                                        }),
                                    )
                                    .child(if chosen {
                                        div()
                                            .h(px(28.))
                                            .px(px(10.))
                                            .rounded(px(R_SM))
                                            .bg(s.tray)
                                            .border_1()
                                            .border_color(s.line_soft)
                                            .flex()
                                            .items_center()
                                            .gap(px(5.))
                                            .text_size(px(FS_META))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.ink)
                                            .child(pro_icon(ProIcon::Check, 12., s.pro.green_ink))
                                            .child(loc.pick("当前默认", "Default"))
                                            .into_any_element()
                                    } else {
                                        soft_button_sm(("agent-usable", ix), loc.pick("设为默认", "Set default"), s)
                                            .on_press(cx, move |this, _, cx| {
                                                this.select_runtime_by_id(&id, cx);
                                                this.save_agent_choice(cx);
                                                this.provider_sheet_open = false;
                                                cx.notify();
                                            })
                                            .into_any_element()
                                    }),
                            ),
                    );

                if key_open {
                    let k_inp = api_key_inp.clone();
                    let rt_save = agent.id.clone();
                    let drawer_close = api_key_drawer.clone();
                    card = card.child(
                        sub_card_box(s)
                            .p(px(10.))
                            .flex()
                            .items_end()
                            .gap(px(8.))
                            .child(div().flex_1().min_w_0().child(api_key_inp.clone()))
                            .child(
                                primary_button_sm(("save-rt-key", ix), loc.pick("保存并验证", "Save key"), s)
                                    .on_press(cx, move |wb, win, cx| {
                                        let key = k_inp.read(cx).value(cx).trim().to_owned();
                                        if !key.is_empty() {
                                            wb.bridge.send(KarCommand::LoginProvider {
                                                runtime_id: rt_save.clone(),
                                                sub_provider: None,
                                                api_key: Some(key),
                                            });
                                            k_inp.update(cx, |i, cx| i.set_value("", win, cx));
                                            drawer_close.update(cx, |v, _| *v = None);
                                            cx.notify();
                                        }
                                    }),
                            ),
                    );
                }

                if expanded && !detail.is_empty() {
                    card = card.child(
                        div()
                            .w_full()
                            .pt(px(8.))
                            .border_t_1()
                            .border_color(s.line_soft)
                            .text_size(px(FS_META))
                            .line_height(px(LH_META))
                            .text_color(s.text)
                            .child(detail),
                    );
                }

                // Full Searchable + Favoritable + Custom Model Catalog (NO `.take(6)` truncation!)
                if chosen {
                    let selected = self.selected_model.clone();
                    let mq = model_search.read(cx).value().trim().to_lowercase();
                    let m_focused = model_search.focus_handle(cx).is_focused(window);
                    let include_hidden = *show_hidden_models.read(cx);
                    let rt_id = agent.id.clone();

                    let mut catalog: Vec<(String, String, bool)> = agent
                        .models
                        .iter()
                        .map(|m| {
                            let lbl = if m.label.trim().is_empty() {
                                m.id.clone()
                            } else {
                                m.label.clone()
                            };
                            (m.id.clone(), lbl, false)
                        })
                        .collect();
                    if let Some(customs) = self.prefs.custom_models.get(&rt_id) {
                        for cm in customs {
                            if !catalog.iter().any(|(id, _, _)| id == cm) {
                                catalog.push((cm.clone(), format!("{cm} (自定义)"), true));
                            }
                        }
                    }
                    // Sort favorites to the front
                    catalog.sort_by_key(|(mid, _, _)| !self.prefs.is_model_favorite(&rt_id, mid));

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
                            .text_size(px(FS_META))
                            .child(loc.pick("自动推荐默认模型", "Agent default"))
                            .on_press(cx, |wb, _, cx| {
                                wb.selected_model = None;
                                wb.save_agent_choice(cx);
                                cx.notify();
                            }),
                    );

                    for (mix, (mid, mlbl, _is_custom)) in catalog.into_iter().enumerate() {
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
                                        .text_size(px(12.))
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
                                        .h(px(22.))
                                        .px(px(4.))
                                        .text_size(px(FS_META))
                                        .font_weight(if is_sel {
                                            FontWeight::SEMIBOLD
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(if is_hidden { s.muted } else { s.ink })
                                        .child(mlbl)
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
                                            .text_size(px(FS_META))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.muted)
                                            .child(loc.pick("全部可用模型（支持 ★ 收藏置顶与自定义模型）", "All models (star favorites or add custom IDs)")),
                                    )
                                    .child(
                                        div()
                                            .flex()
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
                                                div().w(px(210.)).child(search_input_box(
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
                                div()
                                    .flex()
                                    .items_end()
                                    .gap(px(8.))
                                    .child(div().flex_1().min_w_0().child(custom_model_inp.clone()))
                                    .child(
                                        soft_button_sm("add-custom-model-btn", loc.pick("添加并选用", "Add model"), s)
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
                    .child(settings_section_title(loc.pick("可用智能体与模型矩阵", "Available Providers"), s))
                    .child(ready_col),
            );
        }

        // ── 3. Needs Login / Fixable Section (with 1-Click Direct Sign-In!) ──
        if !fixable.is_empty() {
            let mut fix_col = div().flex().flex_col().gap(px(8.));
            for (ix, agent) in fixable.iter().enumerate() {
                let rt_login = agent.id.clone();
                let can_native_login = kar::runtime(&agent.id).is_ok_and(|r| r.supports_login());
                let detail = match &agent.availability {
                    ProviderAvailability::Failed { reason } => {
                        format!(
                            "{}: {reason}",
                            loc.pick("启动检查未通过", "Startup check failed")
                        )
                    }
                    _ => loc
                        .pick("已安装 CLI，点击右侧一键登录授权即可使用", "Installed; click Sign in to authorize")
                        .to_owned(),
                };
                fix_col = fix_col.child(
                    card_box(s)
                        .p(px(12.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(10.))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .child(render_provider_color_tile(&agent.id, 34., s, cx))
                                .child(agent_text(agent.display_name(), detail, s)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .when(can_native_login, |row| {
                                    row.child(
                                        primary_button_sm(("agent-direct-login", ix), loc.pick("直接登录", "Sign in"), s)
                                            .on_press(cx, move |wb, _, _| {
                                                wb.bridge.send(KarCommand::LoginProvider {
                                                    runtime_id: rt_login.clone(),
                                                    sub_provider: None,
                                                    api_key: None,
                                                });
                                            }),
                                    )
                                })
                                .when_some(agent.fix_command(), |row, command| {
                                    row.child(self.command_chip(("agent-fix", ix), command, s, cx))
                                }),
                        ),
                );
            }
            shell = shell.child(
                div()
                    .child(settings_section_title(loc.pick("待登录或修复的智能体", "Sign-in Needed"), s))
                    .child(fix_col),
            );
        }

        // ── 4. Collapsible Installable CLI Agents (`pro_fold`) ───────────────
        if !absent.is_empty() {
            let show_absent =
                self.agents_show_uninstalled || (usable.is_empty() && fixable.is_empty());
            let absent_count = absent.len();
            let mut absent_list = div().w_full().pt(px(8.)).flex().flex_col().gap(px(6.));
            for (ix, agent) in absent.into_iter().enumerate() {
                let id_for_install = agent.id.clone();
                let can_install = agent.can_install;
                let install_lines = agent.install_lines.clone();
                let subtitle: Option<String> = (!install_lines.is_empty()).then(|| {
                    install_lines
                        .iter()
                        .map(|(l, d)| format!("{l}: {d}"))
                        .collect::<Vec<_>>()
                        .join(" · ")
                });
                absent_list = absent_list.child(
                    card_box(s)
                        .px(px(12.))
                        .py(px(9.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .opacity(0.8)
                                .child(render_provider_color_tile(&agent.id, 30., s, cx)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(1.))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(FS_UI))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(s.text)
                                        .child(agent.display_name()),
                                )
                                .when_some(subtitle, |col, sub| {
                                    col.child(
                                        div()
                                            .truncate()
                                            .text_size(px(FS_META))
                                            .text_color(s.muted)
                                            .child(sub),
                                    )
                                }),
                        )
                        .when(can_install && install_lines.is_empty(), |row| {
                            row.child(
                                soft_button_sm(("agent-oneclick-install", ix), loc.pick("一键安装", "Install"), s)
                                    .on_press(cx, move |this, _, _| {
                                        this.bridge.send(KarCommand::InstallAgent {
                                            runtime_id: id_for_install.clone(),
                                            line: None,
                                        });
                                    }),
                            )
                        })
                        .when(can_install && !install_lines.is_empty(), |mut row| {
                            for (lix, (line_val, line_desc)) in install_lines.into_iter().enumerate()
                            {
                                let rt_id = agent.id.clone();
                                let lv = line_val.clone();
                                row = row.child(
                                    soft_button_sm(
                                        ("agent-install-line", ix * 16 + lix),
                                        format!("{} {line_val}", loc.pick("安装", "Install")),
                                        s,
                                    )
                                    .tip(line_desc)
                                    .on_press(cx, move |this, _, _| {
                                        this.bridge.send(KarCommand::InstallAgent {
                                            runtime_id: rt_id.clone(),
                                            line: Some(lv.clone()),
                                        });
                                    }),
                                );
                            }
                            row
                        })
                        .when_some(agent.fix_command(), |row, command| {
                            row.child(self.command_chip(("agent-install", ix), command, s, cx))
                        }),
                );
            }
            shell = shell.child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        disclosure_line(
                            "agents-toggle-absent",
                            pro_icon(ProIcon::Download, ICON_SM, s.muted),
                            format!(
                                "{} ({absent_count})",
                                loc.pick("更多官方支持的一键安装智能体", "More supported agents to install")
                            ),
                            show_absent,
                            s,
                        )
                        .on_press(cx, move |this, _, cx| {
                            this.agents_show_uninstalled = !show_absent;
                            cx.notify();
                        }),
                    )
                    .child(pro_fold(
                        "agents-absent-fold",
                        show_absent,
                        absent_count as f32 * 54.,
                        0.,
                        move || absent_list.into_any_element(),
                    )),
            );
        }

        // ── 5. Community ACP Registry & Custom ACP Command Registration ──────
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
            Input::new("acp-search", loc.pick("搜索 ACP 目录", "Search ACP registry"), w, cx)
        });
        let query = search.clone();
        let mut acp_card = card_box(s).flex().flex_col().child(
            div().flex().items_center().p(px(12.)).gap(px(8.)).child(div().flex_1().min_w_0().child(search))
                .child(soft_button_sm("acp-search-submit", loc.pick("搜索", "Search"), s)
                    .on_press(cx, move |wb, _, cx| wb.bridge.send(KarCommand::Registry(crate::registry::Action::Search(query.read(cx).value(cx).into())))))
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
                .p(px(16.))
                .flex()
                .items_end()
                .gap(px(8.))
                .child(div().w(px(180.)).child(acp_name_inp))
                .child(div().flex_1().min_w_0().child(acp_cmd_inp))
                .child(
                    soft_button("acp-add-custom", loc.pick("注册自定义 ACP", "Add ACP"), s)
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

        shell = shell.child(
            div()
                .child(settings_section_title(
                    loc.pick("第三方社区 ACP 智能体商店与自定义接入", "Community ACP Registry"),
                    s,
                ))
                .child(acp_card),
        );

        shell.into_any_element()
    }

    fn command_chip(
        &self,
        id: impl Into<ElementId>,
        command: String,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Tactile {
        let loc = self.ui_locale;
        let to_copy = command.clone();
        let mono = mono_font(cx);
        Tactile::soft(id, s)
            .radius(R_SM)
            .flex_none()
            .h(px(28.))
            .pl(px(8.))
            .pr(px(7.))
            .gap(px(6.))
            .outlined(s.line_soft, s.line_strong)
            .tip(loc.pick("复制终端命令", "Copy terminal command"))
            .child(
                div()
                    .text_size(px(FS_META))
                    .font_family(mono)
                    .text_color(s.ink)
                    .child(command),
            )
            .child(pro_icon(ProIcon::Copy, ICON_XS, s.muted))
            .on_press(cx, move |this, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(to_copy.clone()));
                this.notify_toast(this.ui_locale.pick("已复制终端命令", "Command copied"), cx);
            })
    }
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
                .text_size(px(FS_UI))
                .font_weight(FontWeight::MEDIUM)
                .text_color(s.ink)
                .child(name),
        )
        .when(!detail.is_empty(), |d| {
            d.child(
                div()
                    .text_size(px(FS_META))
                    .line_height(px(LH_META))
                    .text_color(s.muted)
                    .child(detail),
            )
        })
}
