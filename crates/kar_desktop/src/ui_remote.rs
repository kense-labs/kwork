//! Pair a local chat with a remote controller, inspect permissions, and revoke access.

use gpui::{prelude::*, *};
use kui::{
    components::pro_chrome::{pro_person_avatar, pro_progress_ring},
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
        FS_HERO, FS_META, FS_TAG, FS_UI, ICON_XS, LH_META, R_CARD, R_CTRL, R_PILL,
        StudioPalette, mono_font, tnum_features,
    },
    workbench::KarWorkbench,
};

impl KarWorkbench {
    pub(crate) fn render_remote_panel(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
            .child(self.remote_mode_segmented.clone())
            .when(hosting || remote_connected, |row| row.child(
                div()
                    .font_features(tnum_features())
                    .text_size(px(FS_META))
                    .text_color(s.muted)
                    .child(format!(
                        "已同步 {} 事件 · 受理 {} 指令",
                        self.client_reducer.synced_event_count + telem.total_events_broadcast,
                        telem.total_commands_admitted
                    )),
            ));

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
        let mut col = div().p(px(20.)).flex().flex_col().gap(px(14.));
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
                                    if url_copied { "已复制地址" } else { "复制地址" },
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
                                .on_press(cx, move |this, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        url_to_copy.clone(),
                                    ));
                                    this.remote_code_copied_at =
                                        Some((String::new(), true, std::time::Instant::now()));
                                    cx.notify();
                                }),
                            )
                            .child(
                                text_button("remote-stop-host", "停止服务", s)
                                    .on_press(cx, |this, _, cx| this.stop_sharing(cx)),
                            ),
                    ),
            );

            if let Some(ticket) = active_ticket {
                let frac = ticket.remaining_fraction(now_ms);
                let secs = ticket.remaining_seconds(now_ms);
                let code_raw = ticket.code.clone();
                let code_copied = self.remote_code_copied_at.as_ref().is_some_and(
                    |(c, is_url, at)| !*is_url && c == &code_raw && at.elapsed().as_secs_f32() < 1.8,
                );

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
                            .w(px(40.))
                            .h(px(48.))
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
                                        .child(format!("一次性短码 · {secs} 秒后自动失效")),
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
                                        if code_copied { "已复制短码" } else { "复制 6 位码" },
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
                                    .on_press(cx, move |this, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            code_raw.clone(),
                                        ));
                                        this.remote_code_copied_at = Some((
                                            code_raw.clone(),
                                            false,
                                            std::time::Instant::now(),
                                        ));
                                        cx.notify();
                                    }),
                                )
                                .child(
                                    soft_button("remote-refresh-code", "换一组", s)
                                        .child(pro_icon(ProIcon::Refresh, ICON_XS, s.muted))
                                        .on_press(cx, |this, _, _| {
                                            this.bridge.send(KarCommand::IssuePairingCode);
                                        }),
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
                                .child("当前没有生效中的一次性短码（已配对设备仍保持连接）"),
                        )
                        .child(
                            primary_button("remote-gen-code", "生成 6 位配对码", s)
                                .child(pro_icon(ProIcon::KeyLock, ICON_XS, s.on_primary))
                                .on_press(cx, |this, _, _| {
                                    this.bridge.send(KarCommand::IssuePairingCode);
                                }),
                        ),
                );
            }
            col = col.child(code_card);
        } else {
            let has_chat = self.current_chat_id().is_some();
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
                            .child("共享当前对话"),
                    )
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .line_height(px(LH_META))
                            .text_color(s.muted)
                            .child(if has_chat {
                                "启动后自动绑定本机端口并签发 60 秒有效的一次性 6 位配对码。未配对者无法控制。"
                            } else {
                                "先打开一个本地对话，再生成配对码。"
                            }),
                    )
                    .child(
                        div().flex().justify_end().child(
                            primary_button("remote-start-host", "开始配对", s)
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
        let mut peers_box = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(FS_META))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.muted)
                    .child(format!("已授权设备（{}）", active_peers.len())),
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
                    .child("暂无已配对控制端。在另一窗口输入上方地址与 6 位短码即可接入。"),
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
                                .child(pro_person_avatar(
                                    &peer.device_name,
                                    28.,
                                    true,
                                    None,
                                    cx,
                                ))
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
                                                        .child(peer.role.label()),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .font_features(tnum_features())
                                                .text_size(px(FS_META))
                                                .text_color(s.muted)
                                                .child(format!(
                                                    "已提交 {} 条指令", peer.commands_admitted
                                                )),
                                        ),
                                ),
                        )
                        .child(
                            soft_button(
                                SharedString::from(format!("revoke-peer-{pid}")),
                                "撤销授权",
                                s,
                            )
                            .on_press(cx, move |this, _, _| {
                                this.bridge.send(KarCommand::RevokePairedPeer { peer_id: pid });
                            }),
                        ),
                );
            }
        }

        col.when(listen_url.is_some() || !self.remote_telemetry.peers.is_empty(), |col| col.child(peers_box))
    }

    fn render_remote_connect_tab(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut col = div().p(px(20.)).flex().flex_col().gap(px(14.));

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
                                                "已配对接管：{host_label}（{}）",
                                                role.label()
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
                        soft_button("remote-disconnect-btn", "断开连接", s)
                            .on_press(cx, |this, _, _| {
                                this.bridge.send(KarCommand::DisconnectFeed);
                            }),
                    ),
            );
        }

        col = col
            .child(
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
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        soft_button(
                                            "remote-role-ctrl",
                                            "双向控制",
                                            s,
                                        )
                                        .lit(self.remote_requested_role == PeerRole::Controller)
                                        .on_press(cx, |this, _, cx| {
                                            this.remote_requested_role = PeerRole::Controller;
                                            cx.notify();
                                        }),
                                    )
                                    .child(
                                        soft_button(
                                            "remote-role-view",
                                            "只读观察",
                                            s,
                                        )
                                        .lit(self.remote_requested_role == PeerRole::Viewer)
                                        .on_press(cx, |this, _, cx| {
                                            this.remote_requested_role = PeerRole::Viewer;
                                            cx.notify();
                                        }),
                                    ),
                            )
                            .child(
                                primary_button("remote-pair-submit", "配对并接入节点", s)
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
