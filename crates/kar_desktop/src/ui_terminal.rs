//! Native workspace command stream shared by the drawer and the right panel.

use gpui::{prelude::*, *};
use kui::primitives::pro_icons::{ProIcon, pro_icon};
use std::rc::Rc;
use uuid::Uuid;

use crate::{
    kar_bridge::KarCommand,
    studio_hub::TerminalRunRecord,
    ui_motion::{MenuPlacement, Tactile, menu_host, menu_icon_item, menu_popup, spinner},
    ui_primitives::{FS_META, FS_TAG, StudioPalette, mono_font},
    workbench::KarWorkbench,
};

const QUICK_COMMANDS: [(&str, &str); 4] = [
    ("git status", "git status -sb"),
    ("cargo check", "cargo check"),
    ("cargo test", "cargo test"),
    ("目录列表", "ls -la"),
];

impl KarWorkbench {
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
        let menu_state = window.use_keyed_state((view_id, 0usize), cx, |_, _| false);
        let is_open = *menu_state.read(cx);
        let terminal_editor = self.terminal_cmd_input.read(cx).editor();
        let focus = terminal_editor.focus_handle(cx);
        let scroll_state = window.use_keyed_state((view_id, 1usize), cx, |window, cx| {
            window.focus(&focus, cx);
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
            quick_actions.push(
                menu_icon_item(("terminal-preset", ix), ProIcon::Terminal, *command, s)
                    .on_press(cx, move |this, win, cx| {
                        state.update(cx, |open, cx| {
                            *open = false;
                            cx.notify();
                        });
                        this.terminal_cmd_input
                            .update(cx, |input, cx| input.set_value(*command, win, cx));
                        win.focus(&this.terminal_cmd_input.focus_handle(cx), cx);
                    })
                    .into_any_element(),
            );
        }
        let close_menu = menu_state.clone();
        let quick_menu = menu_popup(
            "terminal-preset-list",
            is_open,
            MenuPlacement::BelowEnd,
            192.,
            quick_actions,
            Rc::new(move |_, cx| {
                close_menu.update(cx, |open, cx| {
                    *open = false;
                    cx.notify();
                })
            }),
            s,
        );
        let icon_button = |id: &'static str, icon: ProIcon, label: &'static str| {
            Tactile::ghost(id, s)
                .size(px(23.))
                .radius(4.)
                .justify_center()
                .tip(label)
                .child(pro_icon(icon, 13., s.muted))
        };
        let completed = self
            .terminal_runs
            .iter()
            .any(|r| r.cwd == cwd && !r.running);
        let clear_cwd = cwd.clone();
        let mut toolbar = div()
            .absolute()
            .top(px(8.))
            .right(px(8.))
            .flex()
            .items_center()
            .h(px(25.))
            .bg(s.surface)
            .border_1()
            .border_color(s.line)
            .rounded(px(6.))
            .child(menu_host(
                icon_button(
                    "terminal-presets",
                    ProIcon::List,
                    loc.pick("填入常用命令", "Insert a command"),
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
            .child(div().w(px(1.)).h(px(14.)).bg(s.line_soft))
            .child(
                icon_button(
                    "terminal-clear-history",
                    ProIcon::Trash,
                    loc.pick(
                        "清空此工作区的已完成记录",
                        "Clear completed commands in this workspace",
                    ),
                )
                .disabled(!completed)
                .on_press(cx, move |this, _, cx| {
                    this.terminal_runs
                        .retain(|run| run.cwd != clear_cwd || run.running);
                    cx.notify();
                }),
            );
        if drawer {
            toolbar = toolbar
                .child(div().w(px(1.)).h(px(14.)).bg(s.line_soft))
                .child(
                    icon_button(
                        "terminal-hide",
                        ProIcon::Close,
                        loc.pick("收起终端", "Hide terminal"),
                    )
                    .on_press(cx, |this, win, cx| {
                        this.set_bottom_terminal_open(false, cx);
                        win.focus(&this.composer_editor.focus_handle(cx), cx);
                    }),
                );
        }

        let mut lines = div()
            .w_full()
            .pl(px(6.))
            .pr(px(8.))
            .py(px(2.))
            .flex()
            .flex_col()
            .font_family(mono_font(cx))
            .text_size(px(13.))
            .line_height(px(19.))
            .text_color(s.ink);
        let mut first_line = true;
        for run in self.terminal_runs.iter().filter(|r| r.cwd == cwd) {
            let line = div()
                .min_h(px(20.))
                .flex()
                .items_center()
                .gap(px(8.))
                .when(first_line, |d| d.pr(px(82.)))
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
                        Tactile::ghost(SharedString::from(format!("terminal-stop-{run_id}")), s)
                            .size(px(23.))
                            .justify_center()
                            .tip(loc.pick("终止命令", "Stop command"))
                            .child(pro_icon(ProIcon::Pause, 13., s.muted))
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
                                .text_size(px(FS_TAG))
                                .text_color(if failed { s.pro.red_ink } else { s.muted })
                                .child(format!("exit {code}")),
                        )
                    })
                    .when(failed, |d| {
                        d.child(
                            Tactile::ghost(
                                SharedString::from(format!("terminal-explain-{run_id}")),
                                s,
                            )
                            .h(px(22.))
                            .px(px(6.))
                            .text_size(px(FS_META))
                            .text_color(s.muted)
                            .disabled(!enabled)
                            .child(loc.pick("追问", "Ask"))
                            .tip(loc.pick(
                                "将该命令和报错添加到对话草稿",
                                "Add this error to the chat draft",
                            ))
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
            first_line = false;
        }
        let populated = !terminal_editor.read(cx).value().trim().is_empty();
        let prompt = div()
            .min_h(px(22.))
            .flex()
            .items_center()
            .gap(px(6.))
            .when(first_line, |d| d.pr(px(82.)))
            .child(
                div()
                    .max_w(px(190.))
                    .truncate()
                    .flex_none()
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
                    icon_button(
                        "terminal-submit",
                        ProIcon::ArrowRight,
                        loc.pick("运行命令", "Run command"),
                    )
                    .on_press(cx, |this, win, cx| this.submit_terminal_input(win, cx)),
                )
            });
        let line_idle = s.line;
        let line_hot = s.brand_action;
        let dragging = self.dragging_terminal;
        let editor_for_focus = terminal_editor.clone();
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
            .child(
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
                    .child(lines.child(prompt)),
            )
            .child(toolbar)
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
