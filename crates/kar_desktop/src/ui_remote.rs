//! LAN QR Code Pairing, Persistent Trusted Devices, Guest Web Share Links & Saved Environments.

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
use uuid::Uuid;

use crate::{
    kar_bridge::KarCommand,
    prefs::SavedRemoteEnv,
    qr::render_qr_canvas,
    remote_link::{PeerRole, now_epoch_ms},
    types::ChatAccessMode,
    ui_motion::{
        danger_button_sm, morph_glyph, outline_button, primary_button, pulse_beacon,
        soft_button_sm, text_button,
    },
    ui_primitives::{
        FS_METRIC, FS_META, FS_TAG, FS_TITLE, FS_UI, ICON_XS, LH_META, R_CTRL, R_PILL, StudioPalette, UiLocale,
        card_box, mono_font, settings_divider, settings_page_shell, settings_row_item,
        settings_section_title, tnum_features,
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
                "http://192.168.1.10:18090",
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
        let now_ms = now_epoch_ms();
        let telem = &self.remote_telemetry;
        let hosting = telem.listen_url.is_some() || self.active_http_server_url.is_some();
        let remote_connected = matches!(self.access_mode, ChatAccessMode::RemoteController { .. });

        let mode_strip = SelectionStrip::options(
            "remote-mode-strip",
            loc.pick("远端连接模式", "Connection mode"),
            Group::Choices,
            [
                (false, loc.pick("本机作为主机共享", "Host & Share")),
                (true, loc.pick("连接远端主机 / 环境", "Connect to Host")),
            ],
            self.remote_tab_connect,
            s,
            cx,
            |wb, connect, _, cx| {
                wb.remote_tab_connect = connect;
                cx.notify();
            },
        );

        let top_bar = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .flex_wrap()
            .gap(px(10.))
            .child(mode_strip)
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

        settings_page_shell()
            .child(top_bar)
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
        let stacked = narrow;
        let mut col = div().flex().flex_col().gap(px(18.));

        // LAN Access Toggle Card
        let lan_sw = Self::settings_switch(
            "remote-lan-listen-sw",
            loc.pick("允许同一 Wi-Fi 下的手机或电脑扫码连接", "Allow devices on the same Wi-Fi to connect"),
            self.prefs.lan_listen_enabled,
            |wb, cx| {
                wb.prefs.lan_listen_enabled = !wb.prefs.lan_listen_enabled;
                wb.save_prefs();
                if wb.active_http_server_url.is_some() {
                    wb.start_sharing(cx);
                }
                cx.notify();
            },
            s,
            window,
            cx,
        );

        let lan_card = card_box(s).child(settings_row_item(
            loc.pick("允许同一局域网跨设备扫码", "Cross-device Wi-Fi pairing"),
            loc.pick(
                "开启后，连接同一 Wi-Fi 的手机或平板可直接扫描二维码查看并操作对话；关闭时仅限本机浏览器访问。",
                "Generate Wi-Fi QR codes for phones and tablets when enabled; localhost only when off.",
            ),
            lan_sw,
            stacked,
            s,
        ));
        col = col.child(lan_card);

        let listen_url = self
            .remote_telemetry
            .lan_url
            .clone()
            .or_else(|| self.remote_telemetry.listen_url.clone())
            .or_else(|| self.active_http_server_url.clone());

        let selected = match self.access_mode {
            ChatAccessMode::WriteController { chat_id } | ChatAccessMode::LocalReadOnlySubscriber {chat_id,..} | ChatAccessMode::ArchivedReadOnly {chat_id,..} => Some(chat_id),
            _ => None,
        };
        let sharing_selected = selected.is_some() && self.remote_telemetry.chat_id == selected;
        if let Some(url) = listen_url.clone() {
            let active_ticket = self
                .remote_telemetry
                .active_ticket
                .clone()
                .filter(|t| now_ms < t.expires_ms);

            if active_ticket.is_some() {
                window.request_animation_frame();
            }

            let mut code_card = card_box(s)
                .w_full()
                .p(px(18.))
                .flex()
                .flex_col()
                .gap(px(14.));

            if let Some(hosted) = self.remote_telemetry.chat_id {
                let title = self.chat_list.iter().find(|chat|chat.info.id==hosted).map(|chat|chat.title.clone()).unwrap_or_else(||hosted.to_string());
                code_card=code_card.child(div().text_size(px(FS_META)).text_color(s.muted).truncate()
                    .child(format!("{} · {title}",loc.pick("正在共享","Sharing"))));
            }
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
                            .gap(px(8.))
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
                            .gap(px(6.))
                            .child(
                                soft_button_sm(
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
                let qr_payload = format!("{url}/?pair_code={code_raw}");
                let code_copied =
                    self.remote_code_copied_at
                        .as_ref()
                        .is_some_and(|(c, is_url, at)| {
                            !*is_url && c == &code_raw && at.elapsed().as_secs_f32() < 1.8
                        });

                let mut digits_row = div().flex().items_center().gap(px(6.));
                for (ix, ch) in ticket.code.chars().enumerate() {
                    if ix == 3 {
                        digits_row = digits_row.child(
                            div()
                                .px(px(3.))
                                .text_size(px(FS_TITLE))
                                .text_color(s.faint)
                                .child("·"),
                        );
                    }
                    digits_row = digits_row.child(
                        div()
                            .w(px(if narrow { 30. } else { 36. }))
                            .h(px(if narrow { 36. } else { 42. }))
                            .rounded(px(R_CTRL))
                            .bg(s.surface)
                            .border_1()
                            .border_color(s.line_strong)
                            .flex()
                            .items_center()
                            .justify_center()
                            .font_family(mono_font(cx))
                            .font_features(tnum_features())
                            .text_size(px(FS_METRIC))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(s.ink)
                            .child(ch.to_string()),
                    );
                }

                let qr_el = render_qr_canvas(&qr_payload, 128., rgb(0x18181b).into(), rgb(0xffffff).into());

                code_card = code_card.child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .flex_wrap()
                        .gap(px(18.))
                        .child(qr_el)
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(220.))
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                .child(digits_row)
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .flex_wrap()
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
                                                            loc.pick("扫码或输入配对码 · 剩余", "Expires in"),
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
                                                    soft_button_sm(
                                                        "remote-copy-code",
                                                        if code_copied {
                                                            loc.pick("已复制短码", "Copied")
                                                        } else {
                                                            loc.pick("复制 6 位码", "Copy code")
                                                        },
                                                        s,
                                                    )
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
                                                    soft_button_sm(
                                                        "remote-refresh-code",
                                                        loc.pick("换一组", "New code"),
                                                        s,
                                                    )
                                                    .child(pro_icon(ProIcon::Refresh, ICON_XS, s.muted))
                                                    .on_press(cx, |this, _, _| {
                                                        this.bridge.send(KarCommand::IssuePairingCode);
                                                    }),
                                                ),
                                        ),
                                ),
                        ),
                );
            } else {
                code_card = code_card.child(
                    div()
                        .w_full()
                        .py(px(6.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .text_size(px(FS_UI))
                                .text_color(s.muted)
                                .child(loc.pick(
                                    "一次性短码已过期（已配对设备凭证已持久化保存，无需重复扫码）",
                                    "Ticket expired. Previously paired devices stay authorized.",
                                )),
                        )
                        .child(
                            primary_button(
                                "remote-gen-code",
                                loc.pick("生成新二维码与短码", "New QR & code"),
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

            // Guest Read-Only Web Share Links Card
            let mut guest_card = card_box(s).flex().flex_col().child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(FS_UI))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(loc.pick("限时网页只读分享链接", "Time-limited Guest Web Links")),
                            )
                            .child(
                                div()
                                    .text_size(px(FS_META))
                                    .text_color(s.muted)
                                    .child(loc.pick(
                                        "访客可在浏览器查看此对话的公开消息。生成新链接会撤销旧链接。",
                                        "Share the public messages of this thread. A new link replaces the previous one.",
                                    )),
                            ),
                    )
                    .child(
                        outline_button("issue-guest-link", loc.pick("生成 60 分钟访客链接", "Create 60m link"), s)
                            .disabled(!sharing_selected)
                            .on_press(cx, move |wb, _, _| {
                                let Some(chat_id) = selected else { return; };
                                wb.bridge.send(KarCommand::IssueGuestShare {
                                    chat_id,
                                    label: "访客网页围观".into(),
                                    ttl_minutes: 60,
                                });
                            }),
                    ),
            );

            if !sharing_selected {
                guest_card = guest_card.child(settings_divider(s)).child(div().px(px(16.)).py(px(10.)).flex().items_center().justify_between().gap(px(8.))
                    .child(div().text_size(px(FS_META)).text_color(s.muted).child(loc.pick("共享服务当前未连接此对话。","This thread is outside the current share.")))
                    .when(selected.is_some(), |row| row.child(soft_button_sm("share-selected-chat",loc.pick("切换到此对话","Share this thread"),s)
                        .on_press(cx,|wb,_,cx|wb.start_sharing(cx)))));
            }
            for (gix, share) in self.remote_telemetry.guest_shares.iter().filter(|g| sharing_selected && g.chat_id == selected && g.is_active(now_ms)).cloned().enumerate() {
                let full_link = format!("{url}/guest/{}?token={}", share.share_id, share.token);
                let sid = share.share_id;
                let rem_min = share.expires_ms.saturating_sub(now_ms).div_ceil(60_000);
                guest_card = guest_card.child(settings_divider(s)).child(
                    div()
                        .px(px(16.))
                        .py(px(10.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(10.))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .truncate()
                                        .font_family(mono_font(cx))
                                        .text_size(px(FS_META))
                                        .text_color(s.ink)
                                        .child(full_link.clone()),
                                )
                                .child(
                                    div()
                                        .font_features(tnum_features())
                                        .text_size(px(FS_TAG))
                                        .text_color(s.muted)
                                        .child(format!("剩余 {rem_min} 分钟有效")),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    soft_button_sm(("copy-guest", gix), loc.pick("复制链接", "Copy link"), s)
                                        .on_press(cx, move |wb, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(full_link.clone()));
                                            wb.notify_toast("已复制访客网页分享链接", cx);
                                        }),
                                )
                                .child(
                                    danger_button_sm(("rev-guest", gix), loc.pick("吊销", "Revoke"), s)
                                        .on_press(cx, move |wb, _, _| {
                                            wb.bridge.send(KarCommand::RevokeGuestShare { share_id: sid });
                                        }),
                                ),
                        ),
                );
            }
            col = col.child(guest_card);
        } else {
            let has_chat = matches!(
                self.access_mode,
                ChatAccessMode::WriteController { .. }
                    | ChatAccessMode::LocalReadOnlySubscriber { .. }
                    | ChatAccessMode::ArchivedReadOnly { .. }
                    | ChatAccessMode::DemoShowcase
            );
            col = col.child(
                card_box(s)
                    .w_full()
                    .p(px(18.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(FS_UI))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child(loc.pick("共享当前对话与扫码配对", "Share this thread & QR pair")),
                    )
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .line_height(px(LH_META))
                            .text_color(s.muted)
                            .child(if has_chat {
                                loc.pick(
                                    "启动后自动签发 60 秒一次性 6 位配对码与二维码；已授权设备凭证持久化保存，重启无需重配。",
                                    "Starts local/LAN listener with a 60s QR code & 6-digit ticket.",
                                )
                            } else {
                                loc.pick("请先打开或新建一个本地对话，再开启配对。", "Open a local thread first to start sharing.")
                            }),
                    )
                    .child(
                        div().flex().justify_end().child(
                            primary_button("remote-start-host", loc.pick("开启共享并生成二维码", "Start sharing"), s)
                                .disabled(!has_chat)
                                .child(pro_icon(ProIcon::Bolt, ICON_XS, s.on_primary))
                                .on_press(cx, |this, _, cx| {
                                    this.start_sharing_and_issue_code(cx);
                                }),
                        ),
                    ),
            );
        }

        // Persistent Trusted Paired Devices
        let active_peers: Vec<_> = self
            .remote_telemetry
            .peers
            .iter()
            .filter(|p| !p.revoked)
            .cloned()
            .collect();
        if listen_url.is_some() || !active_peers.is_empty() {
            let mut peers_card = card_box(s).flex().flex_col();
            if active_peers.is_empty() {
                peers_card = peers_card.child(
                    div()
                        .p(px(16.))
                        .text_size(px(FS_META))
                        .text_color(s.muted)
                        .child(loc.pick(
                            "暂无已授权远端设备。扫码或输入上方地址与 6 位短码即可绑定。",
                            "No paired devices yet.",
                        )),
                );
            } else {
                for (pix, peer) in active_peers.into_iter().enumerate() {
                    if pix > 0 {
                        peers_card = peers_card.child(settings_divider(s));
                    }
                    let pid = peer.peer_id;
                    peers_card = peers_card.child(
                        div()
                            .px(px(16.))
                            .py(px(10.))
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
                                danger_button_sm(
                                    SharedString::from(format!("revoke-peer-{pid}")),
                                    loc.pick("撤销设备授权", "Revoke"),
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
            col = col.child(
                div()
                    .child(settings_section_title(loc.pick("已授权受信设备", "Trusted Paired Devices"), s))
                    .child(peers_card),
            );
        }

        col
    }

    pub(crate) fn kremote_invite_field(
        loc: UiLocale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Input> {
        cx.new(|cx| {
            Input::new(
                "kremote-invitation",
                loc.pick("远程桌面协助邀请", "Remote desktop invitation"),
                window,
                cx,
            )
            .with_placeholder(loc.pick("粘贴协助链接", "Paste an assistance link"), cx)
        })
    }

    fn kremote_card(&self, s: &StudioPalette, cx: &mut Context<Self>) -> Div {
        let loc = self.ui_locale;
        let description = if cfg!(target_os = "macos") {
            loc.pick(
                "在本机的 UU 远程兼容客户端中查看设备，或通过协助邀请连接屏幕。",
                "Open your UU Remote-compatible macOS client to view devices or accept a screen-sharing invitation.",
            )
        } else {
            loc.pick(
                "在 KRemote 中查看设备，或通过 UU 远程协助邀请连接屏幕。",
                "Open KRemote to view devices or accept a UU Remote screen-sharing invitation.",
            )
        };
        div()
            .child(settings_section_title(loc.pick("远程桌面", "Remote desktop"), s))
            .child(
                card_box(s).w_full().p(px(16.)).flex().flex_col().gap(px(10.))
                    .child(div().flex().items_center().justify_between().flex_wrap().gap(px(8.))
                        .child(div().font_weight(FontWeight::MEDIUM).text_size(px(FS_UI)).text_color(s.ink).child("KRemote"))
                        .child(soft_button_sm("kremote-open-client", loc.pick("打开客户端", "Open client"), s)
                            .on_press(cx, |this, _, cx| {
                                if let Err(error) = crate::kremote::open(None) {
                                    this.show_toast(error.message(this.ui_locale), true, cx);
                                }
                            })))
                    .child(div().text_size(px(FS_META)).text_color(s.muted).child(description))
                    .child(self.kremote_invite_input.clone())
                    .child(div().flex().justify_end().child(
                        outline_button("kremote-open-invitation", loc.pick("打开协助邀请", "Open invitation"), s)
                            .on_press(cx, |this, window, cx| {
                                let invite = this.kremote_invite_input.read(cx).value(cx).to_string();
                                match crate::kremote::open(Some(&invite)) {
                                    Ok(()) => this.kremote_invite_input.update(cx, |field, cx| {
                                        field.set_error("", cx);
                                        field.set_value("", window, cx);
                                    }),
                                    Err(error) => {
                                        let message = error.message(this.ui_locale);
                                        this.kremote_invite_input.update(cx, |field, cx| field.set_error(message, cx));
                                    }
                                }
                            }))),
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
        let mut col = div().flex().flex_col().gap(px(16.));

        col = col.child(self.kremote_card(s, cx))
            .child(settings_section_title(loc.pick("远端对话", "Remote conversation"), s));

        if let ChatAccessMode::RemoteController {
            ref url,
            ref host_name,
            role,
            ..
        } = self.access_mode
        {
            let host_label = host_name.clone();
            let host_url = url.clone();
            let save_name = host_label.clone();
            let save_url = host_url.clone();
            col = col.child(
                card_box(s)
                    .w_full()
                    .p(px(16.))
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
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                soft_button_sm("save-cur-env", loc.pick("保存为常用环境", "Save host"), s)
                                    .on_press(cx, move |wb, _, cx| {
                                        if !wb.prefs.saved_environments.iter().any(|e| e.url == save_url) {
                                            wb.prefs.saved_environments.push(SavedRemoteEnv {
                                                id: Uuid::new_v4(),
                                                name: save_name.clone(),
                                                url: save_url.clone(),
                                                token: None,
                                                peer_id: None,
                                                read_only: role == PeerRole::Viewer,
                                            });
                                            wb.save_prefs();
                                            wb.notify_toast("已保存至常用远端环境", cx);
                                        }
                                    }),
                            )
                            .child(
                                soft_button_sm(
                                    "remote-disconnect-btn",
                                    loc.pick("断开连接", "Disconnect"),
                                    s,
                                )
                                .on_press(cx, |this, _, _| {
                                    this.bridge.send(KarCommand::DisconnectFeed);
                                }),
                            ),
                    ),
            );
        }

        if !self.access_mode.is_remote_paired() {
            col = col.child(
                card_box(s)
                    .w_full()
                    .p(px(16.))
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
                                    let url = this.remote_connect_url_input.read(cx).value(cx).trim().to_owned();
                                    if !url.is_empty() && !this.prefs.saved_environments.iter().any(|e| e.url == url) {
                                        this.prefs.saved_environments.push(SavedRemoteEnv {
                                            id: Uuid::new_v4(),
                                            name: url.clone(),
                                            url,
                                            token: None,
                                            peer_id: None,
                                            read_only: this.remote_requested_role == PeerRole::Viewer,
                                        });
                                        this.save_prefs();
                                    }
                                    this.submit_remote_pair_form(cx);
                                }),
                            ),
                    ),
            );
        }

        // Saved Remote Environments List
        if !self.prefs.saved_environments.is_empty() {
            let mut env_card = card_box(s).flex().flex_col();
            for (eix, env) in self.prefs.saved_environments.clone().into_iter().enumerate() {
                if eix > 0 {
                    env_card = env_card.child(settings_divider(s));
                }
                let eid = env.id;
                let eurl_fill = env.url.clone();
                let eurl_ro = env.url.clone();
                env_card = env_card.child(
                    div()
                        .px(px(16.))
                        .py(px(10.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(10.))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(FS_UI))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(s.ink)
                                        .child(env.name),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .font_family(mono_font(cx))
                                        .text_size(px(FS_META))
                                        .text_color(s.muted)
                                        .child(env.url),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    soft_button_sm(("env-fill", eix), loc.pick("填入地址", "Fill URL"), s)
                                        .on_press(cx, move |wb, win, cx| {
                                            wb.remote_connect_url_input.update(cx, |i, cx| {
                                                i.set_value(eurl_fill.clone(), win, cx)
                                            });
                                        }),
                                )
                                .child(
                                    soft_button_sm(("env-sub-ro", eix), loc.pick("只读接入", "Subscribe"), s)
                                        .on_press(cx, move |wb, _, cx| {
                                            wb.close_all_overlays(cx);
                                            wb.bridge.send(KarCommand::ConnectHttpFeed {
                                                url: eurl_ro.clone(),
                                            });
                                        }),
                                )
                                .child(
                                    danger_button_sm(("env-del", eix), loc.pick("移除", "Remove"), s)
                                        .on_press(cx, move |wb, _, cx| {
                                            wb.prefs.saved_environments.retain(|e| e.id != eid);
                                            wb.save_prefs();
                                            cx.notify();
                                        }),
                                ),
                        ),
                );
            }
            col = col.child(
                div()
                    .child(settings_section_title(loc.pick("已保存远端主机环境", "Saved Remote Environments"), s))
                    .child(env_card),
            );
        }

        col
    }
}

fn role_label(role: PeerRole, loc: UiLocale) -> &'static str {
    match role {
        PeerRole::Controller => loc.pick("双向控制", "Controller"),
        PeerRole::Viewer => loc.pick("只读观察", "Viewer"),
    }
}
