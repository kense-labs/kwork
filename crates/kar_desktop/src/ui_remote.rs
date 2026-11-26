//! Pair a local chat with a remote controller, inspect permissions, and revoke access.

use crate::ui_motion::selection::{Group, SelectionStrip};
use gpui::{prelude::*, *};
use kui::{
    components::{
        glass_segmented::{GlassSegmentChanged, GlassSegmentOption, GlassSegmented},
        input::Input,
        pro_chrome::{pro_person_avatar, pro_progress_ring},
    },
    primitives::{
        glyph::Glyph,
        pro_icons::{ProIcon, pro_icon},
    },
};

use crate::{
    kar_bridge::KarCommand,
    remote_link::{PeerRole, now_epoch_ms},
    types::ChatAccessMode,
    ui_motion::{morph_glyph, primary_button, pulse_beacon, soft_button, text_button},
    ui_primitives::{
        FS_HERO, FS_META, FS_TAG, FS_UI, ICON_XS, LH_META, R_CARD, R_CTRL, R_PILL, StudioPalette,
        UiLocale, mono_font, tnum_features,
    },
    workbench::KarWorkbench,
};

impl KarWorkbench {
    pub(crate) fn remote_mode_selector(
        loc: UiLocale,
        connect: bool,
        cx: &mut Context<Self>,
    ) -> Entity<GlassSegmented> {
        let control = cx.new(|cx| {
            GlassSegmented::new(
                loc.pick("连接模式", "Connection mode"),
                vec![
                    GlassSegmentOption::new("host", loc.pick("共享对话", "Host")),
                    GlassSegmentOption::new("connect", loc.pick("连接主机", "Connect")),
                ],
                cx,
            )
            .with_compact(true)
            .with_width(212.)
            .with_value(if connect { "connect" } else { "host" })
        });
        cx.subscribe(&control, |this, _, event: &GlassSegmentChanged, cx| {
            this.remote_tab_connect = event.0.as_ref() == "connect";
            cx.notify();
        })
        .detach();
        control
    }

    pub(crate) fn remote_fields(
        loc: UiLocale,
        values: [String; 3],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> [Entity<Input>; 3] {
        let [url, code, name] = values;
        [
            (
                "kar-remote-url",
                loc.pick("主机地址", "Host address"),
                "http://127.0.0.1:18090",
                url,
            ),
            (
                "kar-remote-code",
                loc.pick("六位配对码", "Pairing code"),
                loc.pick("输入六位数字", "Enter six digits"),
                code,
            ),
            (
                "kar-remote-dev",
                loc.pick("本机名称", "Device name"),
                loc.pick("设备名称", "Device name"),
                name,
            ),
        ]
        .map(|(id, label, placeholder, value)| {
            cx.new(|cx| {
                Input::new(id, label, window, cx)
                    .with_value(value, window, cx)
                    .with_placeholder(placeholder, cx)
            })
        })
    }

    pub(crate) fn render_remote_panel(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let narrow = f32::from(window.viewport_size().width) < 700.;
        let now_ms = now_epoch_ms();
        let telem = &self.remote_telemetry;
        let hosting = telem.listen_url.is_some() || self.active_http_server_url.is_some();
        let remote_connected = matches!(self.access_mode, ChatAccessMode::RemoteController { .. });

        let seg_bar = div()
            .px(px(20.))
            .pt(px(12.))
            .pb(px(4.))
            .flex()
            .items_center()
            .justify_between()
            .when(narrow, |r| r.flex_col().items_start().gap(px(6.)))
            .child(self.remote_mode_segmented.clone())
            .when(hosting || remote_connected, |row| {
                row.child(
                    div()
                        .font_features(tnum_features())
                        .text_size(px(FS_META))
                        .text_color(s.muted)
                        .child(format!(
                            "{} {} · {} {}",
                            self.client_reducer.synced_event_count + telem.total_events_broadcast,
                            loc.pick("个事件", "events"),
                            telem.total_commands_admitted,
                            loc.pick("条指令", "commands")
                        )),
                )
            });

        let body = if self.remote_tab_connect {
            self.render_remote_connect_tab(s, window, cx)
        } else {
            self.render_remote_host_tab(now_ms, s, window, cx)
        };

        div()
            .w_full()
            .flex()
            .flex_col()
            .child(seg_bar)
            .child(body)
            .into_any_element()
    }

    fn render_remote_host_tab(
        &mut self,
        now_ms: u64,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let narrow = f32::from(window.viewport_size().width) < 700.;
        let mut col = div()
            .p(px(if narrow { 12. } else { 20. }))
            .flex()
            .flex_col()
            .gap(px(14.));
        let listen_url = self
            .remote_telemetry
            .listen_url
            .clone()
            .or_else(|| self.active_http_server_url.clone());

        if let Some(url) = listen_url.clone() {
            let active_ticket = self
                .remote_telemetry
                .active_ticket
                .clone()
                .filter(|t| now_ms < t.expires_ms);

            if active_ticket.is_some() {
                // Keep 60s countdown ring animating smoothly while open!
                window.request_animation_frame();
            }

            let mut code_card = div()
                .w_full()
                .p(px(16.))
                .rounded(px(R_CARD))
                .bg(s.surface)
                .border_1()
                .border_color(s.line)
                .flex()
                .flex_col()
                .gap(px(12.));

            let url_to_copy = url.clone();
            let url_copied = self
                .remote_code_copied_at
                .as_ref()
                .is_some_and(|(_, is_url, at)| *is_url && at.elapsed().as_secs_f32() < 1.8);

            code_card = code_card.child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .when(narrow, |row| row.flex_col().items_start())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(pulse_beacon(s.pro.green, 6., true, window, cx))
                            .child(
                                div()
                                    .font_family(mono_font(cx))
                                    .text_size(px(FS_META))
                                    .text_color(s.ink)
                                    .child(url.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                soft_button(
                                    "remote-copy-url",
                                    if url_copied {
                                        loc.pick("已复制地址", "Copied")
                                    } else {
                                        loc.pick("复制地址", "Copy address")
                                    },
                                    s,
                                )
                                .child(morph_glyph(
                                    "remote-copy-url-glyph",
                                    [Glyph::Copy, Glyph::Check],
                                    url_copied,
                                    ICON_XS,
                                    s.ink,
                                    s.soft,
                                    window,
                                    cx,
                                ))
                                .on_press(
                                    cx,
                                    move |this, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            url_to_copy.clone(),
                                        ));
                                        this.remote_code_copied_at =
                                            Some((String::new(), true, std::time::Instant::now()));
                                        cx.notify();
                                    },
                                ),
                            )
                            .child(
                                text_button(
                                    "remote-stop-host",
                                    loc.pick("停止服务", "Stop sharing"),
                                    s,
                                )
                                .on_press(cx, |this, _, cx| this.stop_sharing(cx)),
                            ),
                    ),
            );

            if let Some(ticket) = active_ticket {
                let frac = ticket.remaining_fraction(now_ms);
                let secs = ticket.remaining_seconds(now_ms);
                let code_raw = ticket.code.clone();
                let code_copied =
                    self.remote_code_copied_at
                        .as_ref()
                        .is_some_and(|(c, is_url, at)| {
                            !*is_url && c == &code_raw && at.elapsed().as_secs_f32() < 1.8
                        });

                let mut digits_row = div().flex().items_center().justify_center().gap(px(6.));
                for (ix, ch) in ticket.code.chars().enumerate() {
                    if ix == 3 {
                        digits_row = digits_row.child(
                            div()
                                .px(px(3.))
                                .text_size(px(18.))
                                .text_color(s.faint)
                                .child("·"),
                        );
                    }
                    digits_row = digits_row.child(
                        div()
                            .w(px(if narrow { 27. } else { 40. }))
                            .h(px(if narrow { 34. } else { 48. }))
                            .rounded(px(10.))
                            .bg(s.card)
                            .border_1()
                            .border_color(s.line_strong)
                            .flex()
                            .items_center()
                            .justify_center()
                            .font_family(mono_font(cx))
                            .font_features(tnum_features())
                            .text_size(px(FS_HERO))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(s.ink)
                            .child(ch.to_string()),
                    );
                }

                code_card = code_card.child(digits_row).child(
                    div()
                        .w_full()
                        .pt(px(4.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .when(narrow, |r| r.flex_col().items_start())
                        .gap(px(8.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(pro_progress_ring(
                                    frac,
                                    18.,
                                    2.2,
                                    if secs <= 12 {
                                        s.pro.amber
                                    } else {
                                        s.pro.accent_info
                                    },
                                    s.line_strong,
                                ))
                                .child(
                                    div()
                                        .font_features(tnum_features())
                                        .text_size(px(FS_META))
                                        .text_color(s.muted)
                                        .child(format!(
                                            "{} {secs} {}",
                                            loc.pick("剩余", "Expires in"),
                                            loc.pick("秒", "s")
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    soft_button(
                                        "remote-copy-code",
                                        if code_copied {
                                            loc.pick("已复制短码", "Copied")
                                        } else {
                                            loc.pick("复制 6 位码", "Copy code")
                                        },
                                        s,
                                    )
                                    .child(morph_glyph(
                                        "remote-copy-code-glyph",
                                        [Glyph::Copy, Glyph::Check],
                                        code_copied,
                                        ICON_XS,
                                        s.ink,
                                        s.soft,
                                        window,
                                        cx,
                                    ))
                                    .on_press(
                                        cx,
                                        move |this, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                code_raw.clone(),
                                            ));
                                            this.remote_code_copied_at = Some((
                                                code_raw.clone(),
                                                false,
                                                std::time::Instant::now(),
                                            ));
                                            cx.notify();
                                        },
                                    ),
                                )
                                .child(
                                    soft_button(
                                        "remote-refresh-code",
                                        loc.pick("换一组", "New code"),
                                        s,
                                    )
                                    .child(pro_icon(ProIcon::Refresh, ICON_XS, s.muted))
                                    .on_press(
                                        cx,
                                        |this, _, _| {
                                            this.bridge.send(KarCommand::IssuePairingCode);
                                        },
                                    ),
                                ),
                        ),
                );
            } else {
                code_card = code_card.child(
                    div()
                        .w_full()
                        .py(px(8.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(FS_UI))
                                .text_color(s.muted)
                                .child(loc.pick(
                                "当前没有生效中的一次性短码（已配对设备仍保持连接）",
                                "Paired devices stay connected. Generate a code to add another.",
                            )),
                        )
                        .child(
                            primary_button(
                                "remote-gen-code",
                                loc.pick("生成 6 位配对码", "Generate code"),
                                s,
                            )
                            .child(pro_icon(ProIcon::KeyLock, ICON_XS, s.on_primary))
                            .on_press(cx, |this, _, _| {
                                this.bridge.send(KarCommand::IssuePairingCode);
                            }),
                        ),
                );
            }
            col = col.child(code_card);
        } else {
            let has_chat = matches!(
                self.access_mode,
                ChatAccessMode::WriteController { .. }
                    | ChatAccessMode::LocalReadOnlySubscriber { .. }
                    | ChatAccessMode::ArchivedReadOnly { .. }
                    | ChatAccessMode::DemoShowcase
            );
            col = col.child(
                div()
                    .w_full()
                    .p(px(18.))
                    .rounded(px(R_CARD))
                    .bg(s.surface)
                    .border_1()
                    .border_color(s.line)
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(FS_UI))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child(loc.pick("共享当前对话", "Share this thread")),
                    )
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .line_height(px(LH_META))
                            .text_color(s.muted)
                            .child(if has_chat {
                                loc.pick("启动后自动绑定本机端口并签发 60 秒有效的一次性 6 位配对码。未配对者无法控制。", "Open another Kwrok window on this computer and pair it using a code valid for 60 seconds.")
                            } else {
                                loc.pick("先打开一个本地对话，再生成配对码。", "Open a local thread to generate a pairing code.")
                            }),
                    )
                    .child(
                        div().flex().justify_end().child(
                            primary_button("remote-start-host", loc.pick("开始配对", "Start sharing"), s)
                                .disabled(!has_chat)
                                .child(pro_icon(ProIcon::Bolt, ICON_XS, s.on_primary))
                                .on_press(cx, |this, _, cx| {
                                    this.start_sharing_and_issue_code(cx);
                                }),
                        ),
                    ),
            );
        }

        // Paired devices section
        let active_peers: Vec<_> = self
            .remote_telemetry
            .peers
            .iter()
            .filter(|p| !p.revoked)
            .cloned()
            .collect();
        let mut peers_box = div().w_full().flex().flex_col().gap(px(6.)).child(
            div()
                .text_size(px(FS_META))
                .font_weight(FontWeight::MEDIUM)
                .text_color(s.muted)
                .child(format!(
                    "{} ({})",
                    loc.pick("已授权设备", "Paired devices"),
                    active_peers.len()
                )),
        );

        if active_peers.is_empty() {
            peers_box = peers_box.child(
                div()
                    .w_full()
                    .px(px(12.))
                    .py(px(12.))
                    .rounded(px(R_CTRL))
                    .bg(s.soft)
                    .text_size(px(FS_META))
                    .text_color(s.muted)
                    .child(loc.pick(
                        "暂无已配对控制端。在另一窗口输入上方地址与 6 位短码即可接入。",
                        "Enter the address and code in another Kwrok window to pair.",
                    )),
            );
        } else {
            for peer in active_peers {
                let pid = peer.peer_id;
                peers_box = peers_box.child(
                    div()
                        .w_full()
                        .px(px(12.))
                        .py(px(8.))
                        .rounded(px(R_CTRL))
                        .bg(s.surface)
                        .border_1()
                        .border_color(s.line_soft)
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .child(pro_person_avatar(&peer.device_name, 28., true, None, cx))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(6.))
                                                .child(
                                                    div()
                                                        .text_size(px(FS_UI))
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .text_color(s.ink)
                                                        .child(peer.device_name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .px(px(6.))
                                                        .py(px(1.))
                                                        .rounded(px(R_PILL))
                                                        .bg(s.pro.accent_soft)
                                                        .text_size(px(FS_TAG))
                                                        .text_color(s.pro.accent_info)
                                                        .child(role_label(peer.role, loc)),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .font_features(tnum_features())
                                                .text_size(px(FS_META))
                                                .text_color(s.muted)
                                                .child(format!(
                                                    "{} {}",
                                                    peer.commands_admitted,
                                                    loc.pick("条指令", "commands")
                                                )),
                                        ),
                                ),
                        )
                        .child(
                            soft_button(
                                SharedString::from(format!("revoke-peer-{pid}")),
                                loc.pick("撤销授权", "Revoke"),
                                s,
                            )
                            .on_press(cx, move |this, _, _| {
                                this.bridge
                                    .send(KarCommand::RevokePairedPeer { peer_id: pid });
                            }),
                        ),
                );
            }
        }

        col.when(
            listen_url.is_some() || !self.remote_telemetry.peers.is_empty(),
            |col| col.child(peers_box),
        )
    }

    fn render_remote_connect_tab(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let narrow = f32::from(window.viewport_size().width) < 700.;
        let mut col = div()
            .p(px(if narrow { 12. } else { 20. }))
            .flex()
            .flex_col()
            .gap(px(14.));

        if let ChatAccessMode::RemoteController {
            ref url,
            ref host_name,
            role,
            ..
        } = self.access_mode
        {
            let host_label = host_name.clone();
            let host_url = url.clone();
            col = col.child(
                div()
                    .w_full()
                    .p(px(14.))
                    .rounded(px(R_CARD))
                    .bg(s.pro.green_soft)
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(pulse_beacon(s.pro.green, 7., true, window, cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(FS_UI))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.ink)
                                            .child(format!(
                                                "{host_label} · {}",
                                                role_label(role, loc)
                                            )),
                                    )
                                    .child(
                                        div()
                                            .font_family(mono_font(cx))
                                            .text_size(px(FS_META))
                                            .text_color(s.text)
                                            .child(host_url),
                                    ),
                            ),
                    )
                    .child(
                        soft_button(
                            "remote-disconnect-btn",
                            loc.pick("断开连接", "Disconnect"),
                            s,
                        )
                        .on_press(cx, |this, _, _| {
                            this.bridge.send(KarCommand::DisconnectFeed);
                        }),
                    ),
            );
        }

        if self.access_mode.is_remote_paired() {
            return col;
        }
        col = col.child(
            div()
                .w_full()
                .p(px(16.))
                .rounded(px(R_CARD))
                .bg(s.surface)
                .border_1()
                .border_color(s.line)
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(self.remote_connect_url_input.clone())
                .child(self.remote_connect_code_input.clone())
                .child(self.remote_device_name_input.clone())
                .child(
                    div()
                        .w_full()
                        .pt(px(4.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .when(narrow, |r| r.flex_col().items_start())
                        .gap(px(8.))
                        .child(SelectionStrip::options(
                            "remote-requested-role",
                            loc.pick("连接权限", "Connection permission"),
                            Group::Choices,
                            [
                                (PeerRole::Controller, loc.pick("双向控制", "Controller")),
                                (PeerRole::Viewer, loc.pick("只读观察", "Viewer")),
                            ],
                            self.remote_requested_role,
                            s,
                            cx,
                            |wb, role, _, cx| {
                                wb.remote_requested_role = role;
                                cx.notify();
                            },
                        ))
                        .child(
                            primary_button(
                                "remote-pair-submit",
                                if self.remote_pair_pending {
                                    loc.pick("正在配对…", "Pairing…")
                                } else {
                                    loc.pick("配对并接入节点", "Pair")
                                },
                                s,
                            )
                            .disabled(self.remote_pair_pending)
                            .child(pro_icon(ProIcon::Link, ICON_XS, s.on_primary))
                            .on_press(cx, |this, _, cx| {
                                this.submit_remote_pair_form(cx);
                            }),
                        ),
                ),
        );

        col
    }
}

fn role_label(role: PeerRole, loc: UiLocale) -> &'static str {
    match role {
        PeerRole::Controller => loc.pick("双向控制", "Controller"),
        PeerRole::Viewer => loc.pick("只读观察", "Viewer"),
    }
}
