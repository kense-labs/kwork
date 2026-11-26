//! Interactive PTY terminal (`portable-pty` + `vt100` ANSI 16/256/RGB grid) + workspace command stream,
//! shared seamlessly between the bottom drawer and the right panel.

use gpui::{prelude::*, *};
use kui::primitives::pro_icons::{ProIcon, pro_icon};
use std::rc::Rc;
use uuid::Uuid;

use crate::{
    kar_bridge::KarCommand,
    pty_terminal::{InteractivePtySession, keystroke_to_pty_bytes},
    studio_hub::TerminalRunRecord,
    ui_motion::{
        MenuPlacement, Tactile, icon_button_xs, menu_host, menu_icon_item, menu_popup,
        selection::{Group, SelectionStrip},
        soft_button_sm, spinner,
    },
    ui_primitives::{FS_TAG, R_PILL, R_SM, StudioPalette, mono_font},
    workbench::KarWorkbench,
};

const QUICK_COMMANDS: [(&str, &str); 4] = [
    ("git status", "git status -sb"),
    ("cargo check", "cargo check"),
    ("cargo test", "cargo test"),
    ("目录列表", "ls -la"),
];

impl KarWorkbench {
    pub fn ensure_interactive_pty(&mut self, cx: &mut Context<Self>) {
        let cwd = self.effective_workspace_dir();
        let needs_spawn = self
            .active_pty
            .as_ref()
            .is_none_or(|p| p.cwd != cwd || !p.is_alive());
        if needs_spawn {
            match InteractivePtySession::spawn(&self.config.chat_dir, &cwd, 32, 110) {
                Ok(pty) => {
                    self.active_pty = Some(pty);
                    cx.notify();
                }
                Err(err) => {
                    self.notify_warning(format!("启动交互终端失败：{err}"), cx);
                }
            }
        }
    }

    pub fn run_terminal_command_str(
        &mut self,
        raw_cmd: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cmd = raw_cmd.into().trim().to_owned();
        if cmd.is_empty() {
            return;
        }
        let cwd = self.effective_workspace_dir();
        let id = Uuid::new_v4();
        let started_at = chrono::Local::now().format("%H:%M:%S").to_string();
        self.terminal_runs.push(TerminalRunRecord {
            id,
            command: cmd.clone(),
            cwd: cwd.clone(),
            output: String::new(),
            running: true,
            exit_code: None,
            started_at,
        });
        if self.terminal_runs.len() > 24
            && let Some(oldest) = self.terminal_runs.iter().position(|run| !run.running)
        {
            self.terminal_runs.remove(oldest);
        }
        self.terminal_cmd_input
            .update(cx, |inp, cx| inp.set_value("", window, cx));
        self.bridge.send(KarCommand::RunWorkspaceCommand {
            id,
            cwd,
            command: cmd,
        });
        cx.notify();
    }

    pub fn submit_terminal_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cmd = self.terminal_cmd_input.read(cx).value(cx).trim().to_owned();
        if cmd.is_empty() {
            self.notify_warning("请输入要在当前工作目录执行的终端命令", cx);
            return;
        }
        self.run_terminal_command_str(cmd, window, cx);
    }

    pub(crate) fn render_terminal_dock_body(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_command_stream(false, s, window, cx)
    }

    pub(crate) fn render_bottom_terminal_drawer(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_command_stream(true, s, window, cx)
    }

    fn render_command_stream(
        &mut self,
        drawer: bool,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let cwd = self.effective_workspace_dir();
        let workspace_name = cwd
            .file_name()
            .and_then(|p| p.to_str())
            .unwrap_or_else(|| cwd.to_str().unwrap_or("."));
        let view_id = if drawer {
            "terminal-drawer"
        } else {
            "terminal-panel"
        };

        // Mode state: `true` = Interactive PTY Shell (`zsh -i`), `false` = Command Run Stream
        let pty_mode_state = window.use_keyed_state((view_id, 9usize), cx, |_, _| true);
        let use_pty_mode = *pty_mode_state.read(cx) && self.terminal_runs.is_empty();
        let explicit_mode_state = window.use_keyed_state((view_id, 10usize), cx, |_, _| None::<bool>);
        let in_pty = explicit_mode_state.read(cx).unwrap_or(use_pty_mode);

        if in_pty && self.active_pty.as_ref().is_none_or(|p| p.cwd != cwd) {
            self.ensure_interactive_pty(cx);
        }

        let menu_state = window.use_keyed_state((view_id, 0usize), cx, |_, _| false);
        let is_open = *menu_state.read(cx);
        let terminal_editor = self.terminal_cmd_input.read(cx).editor();
        let stream_focus = terminal_editor.focus_handle(cx);
        let pty_focus = self.pty_focus.clone();
        let scroll_state = window.use_keyed_state((view_id, 1usize), cx, |window, cx| {
            if in_pty {
                window.focus(&pty_focus, cx);
            } else {
                window.focus(&stream_focus, cx);
            }
            ScrollHandle::new()
        });
        let scroll = scroll_state.read(cx).clone();
        if scroll.max_offset().y + scroll.offset().y <= px(2.) {
            scroll.scroll_to_bottom();
        }

        let has_cargo = self
            .workspace_tree
            .iter()
            .any(|file| file.name == "Cargo.toml");
        let mut quick_actions = Vec::new();
        for (ix, (_, command)) in QUICK_COMMANDS.iter().enumerate() {
            if (*command == "git status -sb" && !self.git_snapshot.is_repo)
                || (command.starts_with("cargo ") && !has_cargo)
            {
                continue;
            }
            let state = menu_state.clone();
            let pty_opt = self.active_pty.clone();
            quick_actions.push(
                menu_icon_item(("terminal-preset", ix), ProIcon::Terminal, *command, s)
                    .on_press(cx, move |this, win, cx| {
                        state.update(cx, |open, cx| {
                            *open = false;
                            cx.notify();
                        });
                        if in_pty
                            && let Some(ref pty) = pty_opt
                            && pty.is_alive()
                        {
                            let _ = pty.send_line(command);
                            win.focus(&this.pty_focus, cx);
                        } else {
                            this.terminal_cmd_input
                                .update(cx, |input, cx| input.set_value(*command, win, cx));
                            win.focus(&this.terminal_cmd_input.focus_handle(cx), cx);
                        }
                    })
                    .into_any_element(),
            );
        }
        let close_menu = menu_state.clone();
        let quick_menu = menu_popup(
            "terminal-preset-list",
            is_open,
            MenuPlacement::BelowEnd,
            196.,
            quick_actions,
            Rc::new(move |_, cx| {
                close_menu.update(cx, |open, cx| {
                    *open = false;
                    cx.notify();
                })
            }),
            s,
        );

        let exp_for_strip = explicit_mode_state.clone();
        let mode_strip = SelectionStrip::options(
            (view_id, 20usize),
            loc.pick("终端模式", "Terminal mode"),
            Group::Choices,
            [
                (true, loc.pick("交互终端", "Shell")),
                (false, loc.pick("命令流", "Tasks")),
            ],
            in_pty,
            s,
            cx,
            move |wb, next_pty, win, cx| {
                exp_for_strip.update(cx, |v, _| *v = Some(next_pty));
                if next_pty {
                    wb.ensure_interactive_pty(cx);
                    win.focus(&wb.pty_focus, cx);
                } else {
                    let f = wb.terminal_cmd_input.focus_handle(cx);
                    win.focus(&f, cx);
                }
                cx.notify();
            },
        );

        let pty_snap = if in_pty {
            if let Some(ref pty) = self.active_pty {
                if pty.is_alive() {
                    // Request periodic repaint so live shell output streams smoothly
                    window.request_animation_frame();
                }
                let snap = pty.snapshot(s.dark);
                for port in &snap.detected_ports {
                    if !self.detected_dev_ports.contains(port) {
                        self.detected_dev_ports.push(*port);
                    }
                }
                Some(snap)
            } else {
                None
            }
        } else {
            None
        };

        let pty_alive = pty_snap.as_ref().is_some_and(|sn| sn.running);
        let pty_tail_for_ai = pty_snap.as_ref().map(|sn| sn.plain_tail.clone()).unwrap_or_default();
        let pty_has_err = pty_snap.as_ref().is_some_and(|sn| sn.has_error_hint);

        // Top compact header bar
        let clear_cwd = cwd.clone();
        let mut header = div()
            .w_full()
            .h(px(34.))
            .px(px(8.))
            .border_b_1()
            .border_color(s.line_soft)
            .bg(s.tray)
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(mode_strip)
                    .children(self.detected_dev_ports.iter().take(2).copied().map(|port| {
                        let url = format!("http://localhost:{port}");
                        soft_button_sm(
                            ("term-port-pill", port as usize),
                            format!("localhost:{port} ↗"),
                            s,
                        )
                        .on_press(cx, move |_, _, _| {
                            let _ = std::process::Command::new("open").arg(&url).status();
                        })
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .when(in_pty && pty_has_err, |row| {
                        let tail = pty_tail_for_ai.clone();
                        row.child(
                            soft_button_sm("pty-ask-ai", loc.pick("报错追问 AI", "Ask AI"), s)
                                .on_press(cx, move |wb, win, cx| {
                                    let excerpt: String = tail
                                        .lines()
                                        .rev()
                                        .take(36)
                                        .collect::<Vec<_>>()
                                        .into_iter()
                                        .rev()
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    let q = format!("请分析以下终端报错并给出修复方案：\n```\n{excerpt}\n```");
                                    wb.fill_starter_prompt(&q, win, cx);
                                }),
                        )
                    })
                    .when(in_pty, |row| {
                        row.child(
                            icon_button_xs(
                                "pty-ctrl-c",
                                ProIcon::Pause,
                                loc.pick("发送中断信号 (Ctrl+C)", "Send Ctrl+C"),
                                s,
                            )
                            .on_press(cx, |wb, _, _| {
                                if let Some(ref pty) = wb.active_pty {
                                    let _ = pty.send_interrupt();
                                }
                            }),
                        )
                        .child(
                            icon_button_xs(
                                "pty-restart",
                                ProIcon::Refresh,
                                loc.pick("重启交互 Shell", "Restart shell"),
                                s,
                            )
                            .on_press(cx, |wb, _, cx| {
                                let alive = wb.active_pty.as_ref().is_some_and(|p| p.is_alive());
                                wb.confirm_or_run(
                                    "restart-pty",
                                    alive && wb.prefs.confirm_terminal_close,
                                    "当前终端进程仍在运行，再次点击确认重启终端",
                                    cx,
                                    |wb, cx| {
                                        if let Some(old) = wb.active_pty.take() {
                                            old.terminate();
                                        }
                                        wb.ensure_interactive_pty(cx);
                                    },
                                );
                            }),
                        )
                    })
                    .child(menu_host(
                        icon_button_xs(
                            "terminal-presets",
                            ProIcon::List,
                            loc.pick("填入常用命令", "Quick commands"),
                            s,
                        )
                        .lit(is_open)
                        .on_press(cx, move |_, _, cx| {
                            menu_state.update(cx, |open, cx| {
                                *open = !*open;
                                cx.notify();
                            })
                        }),
                        quick_menu,
                    ))
                    .child(
                        icon_button_xs(
                            "terminal-clear-history",
                            ProIcon::Trash,
                            loc.pick("清屏", "Clear"),
                            s,
                        )
                        .on_press(cx, move |this, _, cx| {
                            if in_pty {
                                if let Some(ref pty) = this.active_pty {
                                    let _ = pty.clear_screen();
                                }
                            } else {
                                this.terminal_runs
                                    .retain(|run| run.cwd != clear_cwd || run.running);
                            }
                            cx.notify();
                        }),
                    ),
            );

        if drawer {
            header = header.child(
                icon_button_xs(
                    "terminal-hide",
                    ProIcon::Close,
                    loc.pick("收起终端", "Hide terminal"),
                    s,
                )
                .on_press(cx, move |this, win, cx| {
                    let has_running_task =
                        this.terminal_runs.iter().any(|r| r.running) || (in_pty && pty_alive);
                    let need_confirm = has_running_task && this.prefs.confirm_terminal_close;
                    if need_confirm {
                        let composer_f = this.composer_editor.focus_handle(cx);
                        this.confirm_or_run(
                            "close-bottom-terminal",
                            true,
                            "终端内仍有活跃会话，再次点击收起（或直接按快捷键切换）",
                            cx,
                            |wb, cx| {
                                wb.set_bottom_terminal_open(false, cx);
                            },
                        );
                        if !this.bottom_terminal_open {
                            win.focus(&composer_f, cx);
                        }
                    } else {
                        this.set_bottom_terminal_open(false, cx);
                        win.focus(&this.composer_editor.focus_handle(cx), cx);
                    }
                }),
            );
        }

        let font_sz = self.prefs.code_font_size;

        let content_area: AnyElement = if in_pty {
            let pty_f = self.pty_focus.clone();
            let pty_focused = pty_f.is_focused(window);
            let mut screen_col = div()
                .w_full()
                .p(px(10.))
                .flex()
                .flex_col()
                .font_family(mono_font(cx))
                .text_size(px(font_sz))
                .line_height(px((font_sz * 1.45).round()))
                .text_color(s.ink);

            if let Some(snap) = pty_snap {
                for (r_ix, row_spans) in snap.lines.iter().enumerate() {
                    let mut row_div = div().min_h(px(font_sz * 1.45)).flex().items_center().whitespace_nowrap();
                    if row_spans.is_empty() && r_ix == snap.cursor_row as usize && pty_focused {
                        row_div = row_div.child(
                            div()
                                .w(px(7.))
                                .h(px(font_sz + 2.))
                                .rounded(px(1.5))
                                .bg(s.ink.opacity(0.75)),
                        );
                    } else {
                        for sp in row_spans {
                            row_div = row_div.child(
                                div()
                                    .text_color(sp.fg.unwrap_or(s.ink))
                                    .when(sp.bold, |d| d.font_weight(FontWeight::SEMIBOLD))
                                    .child(sp.text.clone()),
                            );
                        }
                        if r_ix == snap.cursor_row as usize && pty_focused {
                            row_div = row_div.child(
                                div()
                                    .ml(px(1.))
                                    .w(px(7.))
                                    .h(px(font_sz + 2.))
                                    .rounded(px(1.5))
                                    .bg(s.ink.opacity(0.75)),
                            );
                        }
                    }
                    screen_col = screen_col.child(row_div);
                }
            }

            let pty_ref = self.active_pty.clone();
            div()
                .id("pty-interactive-surface")
                .track_focus(&pty_f)
                .track_scroll(&scroll)
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .cursor_text()
                .on_mouse_down(MouseButton::Left, move |_, win, cx| {
                    win.focus(&pty_f, cx);
                    cx.stop_propagation();
                })
                .on_key_down(cx.listener(move |wb, ev: &KeyDownEvent, _, cx| {
                    if (ev.keystroke.modifiers.platform || ev.keystroke.modifiers.control)
                        && ev.keystroke.key.eq_ignore_ascii_case("v")
                        && ev.keystroke.modifiers.platform
                    {
                        if let Some(clip) = cx.read_from_clipboard().and_then(|c| c.text())
                            && let Some(ref pty) = pty_ref
                        {
                            let _ = pty.write_bytes(clip.as_bytes());
                            cx.stop_propagation();
                        }
                        return;
                    }
                    if let Some(bytes) = keystroke_to_pty_bytes(&ev.keystroke)
                        && let Some(ref pty) = wb.active_pty
                    {
                        let _ = pty.write_bytes(&bytes);
                        cx.notify();
                        cx.stop_propagation();
                    }
                }))
                .child(screen_col)
                .into_any_element()
        } else {
            let mut lines = div()
                .w_full()
                .px(px(10.))
                .py(px(6.))
                .flex()
                .flex_col()
                .font_family(mono_font(cx))
                .text_size(px(font_sz))
                .line_height(px((font_sz * 1.45).round()))
                .text_color(s.ink);

            for run in self.terminal_runs.iter().filter(|r| r.cwd == cwd) {
                let line = div()
                    .min_h(px(22.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(format!("{workspace_name} $ {}", run.command)),
                    );
                let run_id = run.id;
                let failed = run.exit_code.is_some_and(|code| code != 0);
                let actions = if run.running {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(spinner(10., s.muted, window, cx))
                        .child(
                            icon_button_xs(
                                SharedString::from(format!("terminal-stop-{run_id}")),
                                ProIcon::Pause,
                                loc.pick("终止命令", "Stop command"),
                                s,
                            )
                            .on_press(cx, move |this, _, _| {
                                this.bridge
                                    .send(KarCommand::KillWorkspaceCommand { id: run_id });
                            }),
                        )
                        .into_any_element()
                } else {
                    let enabled = self.access_mode.is_writable()
                        || matches!(self.access_mode, crate::types::ChatAccessMode::NoneSelected);
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .when_some(run.exit_code, |d, code| {
                            d.child(
                                div()
                                    .px(px(5.))
                                    .rounded(px(R_PILL))
                                    .bg(if failed { s.pro.red_soft } else { s.tray })
                                    .text_size(px(FS_TAG))
                                    .text_color(if failed { s.pro.red_ink } else { s.muted })
                                    .child(format!("exit {code}")),
                            )
                        })
                        .when(failed, |d| {
                            d.child(
                                soft_button_sm(
                                    SharedString::from(format!("terminal-explain-{run_id}")),
                                    loc.pick("追问 AI", "Ask AI"),
                                    s,
                                )
                                .disabled(!enabled)
                                .on_press(cx, move |this, win, cx| {
                                    this.draft_terminal_question(run_id, win, cx)
                                }),
                            )
                        })
                        .into_any_element()
                };
                lines = lines.child(line.child(actions));
                if !run.output.is_empty() {
                    lines = lines.child(
                        div()
                            .w_full()
                            .text_color(s.text)
                            .child(run.output.trim_end_matches('\n').to_owned()),
                    );
                }
            }

            let populated = !terminal_editor.read(cx).value().trim().is_empty();
            let prompt = div()
                .min_h(px(24.))
                .pt(px(4.))
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .max_w(px(190.))
                        .truncate()
                        .flex_none()
                        .text_color(s.muted)
                        .child(format!("{workspace_name} $")),
                )
                .child(
                    div()
                        .h(px(22.))
                        .flex_1()
                        .min_w_0()
                        .child(terminal_editor.clone()),
                )
                .when(populated, |d| {
                    d.child(
                        Tactile::primary("terminal-submit", s)
                            .size(px(24.))
                            .radius(R_SM)
                            .justify_center()
                            .tip(loc.pick("运行命令", "Run command"))
                            .child(pro_icon(ProIcon::ArrowRight, 12., s.on_primary))
                            .on_press(cx, |this, win, cx| this.submit_terminal_input(win, cx)),
                    )
                });
            let editor_for_focus = terminal_editor.clone();
            div()
                .id("terminal-output-scroll")
                .track_scroll(&scroll)
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .on_mouse_down(MouseButton::Left, move |_, win, cx| {
                    win.focus(&editor_for_focus.focus_handle(cx), cx);
                    win.prevent_default();
                    cx.stop_propagation();
                })
                .child(lines.child(prompt))
                .into_any_element()
        };

        let line_idle = s.line;
        let line_hot = s.brand_action;
        let dragging = self.dragging_terminal;
        div()
            .id(view_id)
            .relative()
            .size_full()
            .bg(s.surface)
            .flex()
            .flex_col()
            .overflow_hidden()
            .when(drawer, |d| {
                d.child(
                    div()
                        .id("kar-bottom-terminal-resize")
                        .h(px(4.))
                        .flex_none()
                        .cursor_row_resize()
                        .group("term-resize-handle")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|wb, ev: &MouseDownEvent, _, cx| {
                                wb.dragging_terminal = true;
                                wb.terminal_drag_anchor =
                                    Some((f32::from(ev.position.y), wb.bottom_terminal_h));
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .h(px(if dragging { 2. } else { 1. }))
                                .bg(if dragging { line_hot } else { line_idle })
                                .group_hover("term-resize-handle", move |d| d.bg(line_hot)),
                        ),
                )
            })
            .child(header)
            .child(content_area)
            .into_any_element()
    }

    fn draft_terminal_question(
        &mut self,
        run_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = self.terminal_runs.iter().find(|run| run.id == run_id) else {
            return;
        };
        let output = run
            .output
            .lines()
            .rev()
            .take(40)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        let question = format!(
            "{}: `{}` ({}: {}, exit {})\n\n{}",
            self.ui_locale
                .pick("请分析这条命令的错误", "Explain this command failure"),
            run.command,
            self.ui_locale.pick("目录", "directory"),
            run.cwd.display(),
            run.exit_code.unwrap_or(-1),
            output
        );
        self.composer_editor.update(cx, |editor, cx| {
            let draft = if editor.value().is_empty() {
                question
            } else {
                format!("{}\n\n{question}", editor.value())
            };
            editor.set_value(draft, window, cx);
        });
        self.reveal_chat_composer(window, cx);
        self.notify_toast(
            self.ui_locale
                .pick("已将报错添加到对话草稿", "Error added to the chat draft"),
            cx,
        );
    }
}
