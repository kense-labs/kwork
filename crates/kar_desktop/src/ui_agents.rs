//! KUI `settings_models.rs` + `agents.rs` Application Screen:
//! Default Model & Autonomy Routing, Colorful Provider Cards, Model Capsules & One-Click CLI Setup.
//!
//! Powered by `kar::orientation::orient` (`Depth::Models`) and `kar::brands::runtime_brands()`.

use crate::ui_motion::selection::{Group, SelectionStrip};
use gpui::{prelude::*, *};
use kar::ApprovalMode;
use kui::{
    components::pro_chrome::pro_fold,
    primitives::pro_icons::{ProIcon, pro_icon},
};

use crate::{
    kar_bridge::KarCommand,
    types::{ProviderAvailability, ProviderReadiness},
    ui_motion::{Tactile, disclosure_line, soft_button, spinner},
    ui_primitives::{
        FS_META, FS_UI, ICON_SM, ICON_XS, LH_META, LH_UI, R_CARD, R_CTRL,
        StudioPalette, mono_font, render_provider_color_tile,
    },
    workbench::KarWorkbench,
};

impl KarWorkbench {
    pub(crate) fn render_agents_panel(
        &self,
        s: &StudioPalette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
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

        // ── 1. KUI `settings_models.rs` Default Routing & Autonomy Card ──────
        let effort_menu = crate::ui_motion::menu_host(
            crate::ui_motion::soft_button(
                "settings-effort",
                self.selected_effort()
                    .unwrap_or(self.ui_locale.pick("使用模型默认值", "Model default"))
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
        let approval_seg = SelectionStrip::options(
            "settings-approval",
            loc.pick("授权请求", "Tool permissions"),
            Group::Choices,
            [
                (ApprovalMode::Host, loc.pick("需确认", "Ask first")),
                (ApprovalMode::Deny, loc.pick("自动拒绝", "Deny")),
            ],
            self.approval_mode,
            s,
            cx,
            |wb, mode, _, cx| wb.set_new_chat_approvals(mode, cx),
        );
        let wt_seg = SelectionStrip::options(
            "settings-worktree",
            loc.pick("工作目录", "Working directory"),
            Group::Choices,
            [
                (false, loc.pick("当前目录", "Checkout")),
                (true, loc.pick("独立工作树", "Worktree")),
            ],
            self.new_chat_use_worktree,
            s,
            cx,
            |wb, enabled, _, cx| wb.set_new_chat_worktree(enabled, cx),
        );

        let routing_card = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(px(FS_META))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(s.muted)
                    .child(loc.pick("新对话默认设置", "Defaults for new threads")),
            )
            .when(!self.offered_efforts().is_empty(), |d| {
                d.child(routing_setting_row(
                    loc.pick("思考深度", "Reasoning effort"),
                    loc.pick("当前模型提供的推理档位", "Levels reported by the selected model"),
                    effort_menu,
                    s,
                ))
                .child(div().h(px(1.)).w_full().bg(s.line_soft))
            })
            .child(routing_setting_row(
                loc.pick("授权请求", "Tool permissions"),
                loc.pick("智能体需要额外权限时如何处理", "When an agent requests additional access"),
                approval_seg,
                s,
            ))
            .child(div().h(px(1.)).w_full().bg(s.line_soft))
            .child(routing_setting_row(
                loc.pick("工作目录", "Working directory"),
                loc.pick("独立工作树会为新对话创建 Git 分支", "A worktree creates a separate branch for a new thread"),
                wt_seg,
                s,
            ));

        let mut body = div().p(px(20.)).flex().flex_col().gap(px(16.)).child(routing_card);

        if agents.is_empty() {
            body = body.child(
                div()
                    .px(px(12.))
                    .py(px(22.))
                    .rounded(px(R_CARD))
                    .bg(s.surface)
                    .border_1()
                    .border_color(s.line_soft)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(FS_UI))
                    .line_height(px(LH_UI))
                    .text_color(s.muted)
                    .child(spinner(ICON_SM, s.muted, window, cx))
                    .child(loc.pick("正在并行检查本机已安装的智能体与可用模型…", "Checking installed agents and models…")),
            );
        }

        // ── 2. Ready Agents Fleet (`agents.rs` Sculpted Cards + Model Capsules) ─
        if !usable.is_empty() {
            let mut ready_col = div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(group_label(loc.pick("可用智能体", "Available agents"), s));

            for (ix, agent) in usable.iter().enumerate() {
                let chosen = agent.id == self.selected_runtime;
                let id = agent.id.clone();
                let detail = agent.ready_detail_line_in(loc);


                let mut card = div()
                    .w_full()
                    .p(px(12.))
                    .rounded(px(R_CARD))
                    .bg(s.surface)
                    .border_1()
                    .border_color(s.line_soft)
                    .flex()
                    .flex_col()
                    .gap(px(9.))
                    .child(
                        div()
                            .w_full()
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
                                    .gap(px(11.))
                                    .child(render_provider_color_tile(&agent.id, 36., s, cx))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.5))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .flex_wrap()
                                                    .gap(px(6.))
                                                    .child(
                                                        div()
                                                            .text_size(px(FS_UI))
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_color(s.ink)
                                                            .child(agent.display_name()),
                                                    )
                                            )
                                            .when(!detail.is_empty(), |col| {
                                                col.child(
                                                    div()
                                                        .truncate()
                                                        .text_size(px(FS_META))
                                                        .line_height(px(LH_META))
                                                        .text_color(s.muted)
                                                        .child(detail),
                                                )
                                            }),
                                    ),
                            )
                            .child(if chosen {
                                div()
                                    .h(px(26.))
                                    .px(px(9.))
                                    .rounded(px(R_CTRL))
                                    .bg(s.tray)
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .text_size(px(FS_META))
                                    .text_color(s.ink)
                                    .child(pro_icon(ProIcon::Check, 12., s.pro.green_ink))
                                    .child(loc.pick("当前默认", "Default"))
                                    .into_any_element()
                            } else {
                                soft_button(("agent-usable", ix), loc.pick("设为默认", "Set default"), s)
                                    .on_press(cx, move |this, _, cx| {
                                        this.select_runtime_by_id(&id, cx);
                                        this.save_agent_choice(cx);
                                        this.provider_sheet_open = false;
                                        cx.notify();
                                    })
                                    .into_any_element()
                            }),
                    );

                if chosen && !agent.models.is_empty() {
                    let selected = self.selected_model.clone();
                    let mut models = vec![(None, loc.pick("自动推荐", "Agent default").to_owned())];
                    for (index, model) in agent.models.iter().enumerate() {
                        if index < 6 || selected.as_ref() == Some(&model.id) {
                            let label = if model.label.trim().is_empty() {
                                &model.id
                            } else {
                                &model.label
                            };
                            models.push((Some(model.id.clone()), label.clone()));
                        }
                    }
                    let title = loc.pick("首选模型", "Preferred model");
                    card = card.child(
                        div()
                            .pt(px(8.))
                            .border_t_1()
                            .border_color(s.line_soft)
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap(px(5.))
                            .child(
                                div()
                                    .text_size(px(FS_META))
                                    .text_color(s.muted)
                                    .child(title),
                            )
                            .child(SelectionStrip::options(
                                SharedString::from(format!("preferred-model-{}", agent.id)),
                                title,
                                Group::Choices,
                                models,
                                selected,
                                s,
                                cx,
                                |wb, model, _, cx| {
                                    wb.selected_model = model;
                                    wb.save_agent_choice(cx);
                                    cx.notify();
                                },
                            )),
                    );
                }

                ready_col = ready_col.child(card);
            }
            body = body.child(ready_col);
        }

        // ── 3. Needs Login / Fixable Section ────────────────────────────────
        if !fixable.is_empty() {
            let mut fix_col = div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(group_label(loc.pick("需要登录或修复", "Sign-in or repair needed"), s));
            for (ix, agent) in fixable.iter().enumerate() {
                let detail = match &agent.availability {
                    ProviderAvailability::Failed { reason } => {
                        format!(
                            "{}: {reason}",
                            loc.pick("启动检查未通过", "Startup check failed")
                        )
                    }
                    _ => loc.pick("已安装 CLI，尚未登录账号", "Installed; sign in to use this agent")
                        .to_owned(),
                };
                fix_col = fix_col.child(
                    div()
                        .w_full()
                        .p(px(11.))
                        .rounded(px(R_CARD))
                        .bg(s.surface)
                        .border_1()
                        .border_color(s.line_soft)
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(render_provider_color_tile(&agent.id, 34., s, cx))
                        .child(agent_text(agent.display_name(), detail, s))
                        .when_some(agent.fix_command(), |row, command| {
                            row.child(self.command_chip(("agent-fix", ix), command, s, cx))
                        }),
                );
            }
            body = body.child(fix_col);
        }

        // ── 4. Collapsible Installable CLI Agents (`pro_fold`) ──────────────
        if !absent.is_empty() {
            let show_absent =
                self.agents_show_uninstalled || (usable.is_empty() && fixable.is_empty());
            let absent_count = absent.len();
            let mut absent_list = div().w_full().pt(px(6.)).flex().flex_col().gap(px(6.));
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
                    div()
                        .w_full()
                        .px(px(10.))
                        .py(px(8.))
                        .rounded(px(R_CTRL + 2.))
                        .bg(s.surface)
                        .border_1()
                        .border_color(s.line_soft)
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .opacity(0.75)
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
                                soft_button(("agent-oneclick-install", ix), loc.pick("一键安装", "Install"), s)
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
                                    soft_button(
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
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        disclosure_line(
                            "agents-toggle-absent",
                            pro_icon(ProIcon::Download, ICON_SM, s.muted),
                            format!(
                                "{} ({absent_count})",
                                loc.pick("更多可安装的智能体", "More agents to install")
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
                        absent_count as f32 * 48.,
                        0.,
                        move || absent_list.into_any_element(),
                    )),
            );
        }

        div().flex().flex_col().child(body).into_any_element()
    }

    /// A terminal command that can be copied with one click.
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
            .radius(R_CTRL)
            .flex_none()
            .h(px(26.))
            .pl(px(8.))
            .pr(px(7.))
            .gap(px(6.))
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

fn routing_setting_row(
    title: &'static str,
    subtitle: &'static str,
    control: impl IntoElement,
    s: &StudioPalette,
) -> Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(FS_UI))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.ink)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(FS_META))
                        .text_color(s.muted)
                        .child(subtitle),
                ),
        )
        .child(div().flex_none().child(control))
}

fn group_label(text: &'static str, s: &StudioPalette) -> Div {
    div()
        .px(px(2.))
        .text_size(px(FS_META))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(s.muted)
        .child(text)
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
                    .truncate()
                    .text_size(px(FS_META))
                    .line_height(px(LH_META))
                    .text_color(s.muted)
                    .child(detail),
            )
        })
}
