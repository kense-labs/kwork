//! Compact views of the provider's public activity records.
use super::*;
use crate::types::office_mcp_summary;
use kar::{OutputPart, ToolStatus};
use kui::components::{
    json_viewer::JsonViewer, morph_loader::MorphLoader,
};
use serde_json::Value;
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Ended,
    Unfinished,
}

pub(super) fn phase(tool: &Tool, running_turn: bool) -> Phase {
    let result = tool.result.as_ref();
    let exit = result.and_then(|r| r.exit_code);
    match result.and_then(|r| r.status.as_ref()) {
        Some(ToolStatus::Cancelled) => Phase::Cancelled,
        Some(ToolStatus::Failed) => Phase::Failed,
        _ if exit.is_some_and(|code| code != 0) => Phase::Failed,
        Some(ToolStatus::Succeeded) => Phase::Succeeded,
        _ if exit == Some(0) => Phase::Succeeded,
        _ if tool.finished || result.is_some() => Phase::Ended,
        _ if running_turn => Phase::Running,
        _ => Phase::Unfinished,
    }
}

impl Phase {
    fn label(self, loc: UiLocale) -> &'static str {
        match self {
            Self::Running => loc.pick("运行中", "Running"),
            Self::Succeeded => loc.pick("已完成", "Completed"),
            Self::Failed => loc.pick("失败", "Failed"),
            Self::Cancelled => loc.pick("已取消", "Cancelled"),
            Self::Ended => loc.pick("已结束", "Ended"),
            Self::Unfinished => loc.pick("未记录结果", "No result recorded"),
        }
    }

    fn groupable(self) -> bool {
        matches!(self, Self::Succeeded | Self::Ended)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Search,
    Read,
    List,
    JavaScript,
    Command,
    Other,
}

impl Kind {
    fn label(self, loc: UiLocale) -> &'static str {
        match self {
            Self::Search => loc.pick("搜索代码", "Search code"),
            Self::Read => loc.pick("读取文件", "Read file"),
            Self::List => loc.pick("列出目录", "List directory"),
            Self::JavaScript => loc.pick("运行 JavaScript", "Run JavaScript"),
            Self::Command => loc.pick("运行命令", "Run command"),
            Self::Other => loc.pick("调用工具", "Tool call"),
        }
    }
}

fn short(text: &str) -> String {
    let line = text.trim().lines().next().unwrap_or_default();
    let mut chars = line.chars();
    let mut brief: String = chars.by_ref().take(44).collect();
    if chars.next().is_some() {
        brief.push('…');
    }
    brief
}

fn args(tool: &Tool) -> Option<Cow<'_, Value>> {
    let input = tool.input.as_ref()?;
    if !tool.input_partial
        && let Some(text) = input.as_str()
        && text.trim_start().starts_with('{')
        && let Ok(value @ Value::Object(_)) = serde_json::from_str(text)
    {
        return Some(Cow::Owned(value));
    }
    Some(Cow::Borrowed(input))
}

fn string_arg<'a>(input: Option<&'a Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| input?.get(key)?.as_str().filter(|s| !s.trim().is_empty()))
}

fn command(tool: &Tool, input: Option<&Value>) -> Option<String> {
    let value = input?;
    let name = tool
        .native_type
        .as_deref()
        .or(tool.name.as_deref())
        .unwrap_or("");
    let raw = string_arg(input, &["command", "cmd", "CommandLine"])
        .map(str::to_owned)
        .or_else(|| {
            let words = value
                .get("command")
                .or_else(|| value.get("cmd"))?
                .as_array()?;
            Some(shell_words::join(
                words
                    .iter()
                    .map(Value::as_str)
                    .collect::<Option<Vec<_>>>()?,
            ))
        })
        .or_else(|| {
            matches!(
                name,
                "commandExecution" | "Bash" | "bash" | "shell" | "exec_command"
            )
            .then(|| value.as_str().map(str::to_owned))
            .flatten()
        })?;
    Some(unwrap_shell_command(&raw))
}

struct Summary {
    kind: Kind,
    title: String,
}

fn summary(tool: &Tool, loc: UiLocale) -> Summary {
    let data = args(tool);
    let input = data.as_deref();
    let name = tool
        .tool_name
        .as_deref()
        .or(tool.name.as_deref())
        .unwrap_or("");
    let bare = name
        .rsplit(['/', '.'])
        .next()
        .unwrap_or(name)
        .rsplit("__")
        .next()
        .unwrap_or(name);
    let hint = string_arg(input, &["title", "description", "summary", "toolSummary"]);
    let (kind, subject) = if matches!(bare, "js" | "node_repl" | "javascript") {
        (Kind::JavaScript, hint.map(short))
    } else if let Some(cmd) = command(tool, input) {
        let words = shell_words::split(&cmd).unwrap_or_default();
        let executable = words
            .first()
            .and_then(|s| s.rsplit('/').next())
            .unwrap_or("");
        let args = words.get(1..).unwrap_or_default();
        let operand = args.iter().find(|s| !s.starts_with('-'));
        let compound = args
            .iter()
            .any(|a| matches!(a.as_str(), "|" | "||" | "&&" | ";" | ">" | ">>"));
        let (kind, subject) = match executable {
            "rg" | "grep" if !compound => (Kind::Search, operand.map(|s| short(s))),
            "ls" | "dir" if !compound => (Kind::List, operand.map(|s| short(s))),
            "cat" | "head" | "tail" if !compound => (
                Kind::Read,
                args.last()
                    .filter(|p| !p.starts_with('-'))
                    .map(|p| short(p)),
            ),
            "sed"
                if !compound
                    && args.first().is_some_and(|a| a == "-n")
                    && args.get(1).is_some_and(|a| {
                        a.ends_with('p')
                            && a.chars().all(|c| c.is_ascii_digit() || ",;$p".contains(c))
                    }) =>
            {
                (Kind::Read, args.get(2).map(|p| short(p)))
            }
            "git" | "cargo" | "npm" if !compound => (
                Kind::Command,
                Some(short(
                    &words.iter().take(2).cloned().collect::<Vec<_>>().join(" "),
                )),
            ),
            _ => (
                Kind::Command,
                (!executable.is_empty()).then(|| short(executable)),
            ),
        };
        (kind, hint.map(short).or(subject))
    } else {
        let office = (loc == UiLocale::Zh)
            .then(|| office_mcp_summary(bare, input))
            .flatten();
        let title = hint.map(short).or(office).unwrap_or_else(|| {
            let subject = string_arg(input, &["file_path", "path", "query", "pattern", "url"]);
            let tool = if bare.is_empty() {
                Kind::Other.label(loc)
            } else {
                bare
            };
            match subject {
                Some(value) => format!("{} · {}", short(tool), short(value)),
                None => short(tool),
            }
        });
        return Summary {
            kind: Kind::Other,
            title,
        };
    };
    let title = match subject.filter(|s| !s.is_empty()) {
        Some(subject) => format!("{} · {subject}", kind.label(loc)),
        None => kind.label(loc).to_owned(),
    };
    Summary { kind, title }
}

fn status_mark(
    id: SharedString,
    state: Phase,
    s: &StudioPalette,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    match state {
        Phase::Running => window
            .use_keyed_state(id.clone(), cx, |_, _| {
                MorphLoader::new(id)
                    .with_size(15.)
                    .with_tone(false)
                    .decorative(true)
            })
            .into_any_element(),
        Phase::Succeeded => pro_icon(ProIcon::Check, ICON_SM, s.muted),
        Phase::Failed => pro_icon(ProIcon::AlertTriangle, ICON_SM, s.pro.red),
        Phase::Ended => pro_icon(ProIcon::Activity, ICON_SM, s.muted),
        Phase::Unfinished | Phase::Cancelled => pro_icon(ProIcon::Minus, ICON_SM, s.muted),
    }
}

impl KarWorkbench {
    pub(super) fn render_tool_run(
        &mut self,
        turn_id: Uuid,
        calls: &[(usize, &Tool)],
        running: bool,
        subagent: bool,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let items: Vec<_> = calls
            .iter()
            .map(|(ix, tool)| (*ix, *tool, summary(tool, loc), phase(tool, running)))
            .collect();
        let mut list = div().w_full().min_w_0().flex().flex_col().gap(px(2.));
        let mut start = 0;
        while start < items.len() {
            let (ix, _, first, state) = &items[start];
            let mut end = start + 1;
            if state.groupable() && first.kind != Kind::Other {
                while end < items.len() && items[end].2.kind == first.kind && items[end].3 == *state
                {
                    end += 1;
                }
            }
            let group = &items[start..end];
            let key = (turn_id, *ix);
            let multiple = group.len() > 1;
            let group_open = self.part_is_open(key, false);
            let mut rows = div().w_full().flex().flex_col().gap(px(1.));
            if !multiple || group_open {
                for (number, (ix, tool, item, state)) in group.iter().enumerate() {
                    let address = (turn_id, *ix);
                    let open = self.tool_detail == Some(address);
                    let mut title = item.title.clone();
                    if multiple && title == item.kind.label(loc) {
                        title.push_str(&format!(" · {}", number + 1));
                    }
                    if subagent {
                        title = format!("{} · {title}", loc.pick("子智能体", "Subagent"));
                    }
                    if !matches!(state, Phase::Succeeded | Phase::Ended) {
                        title.push_str(&format!(" · {}", state.label(loc)));
                    }
                    let id = SharedString::from(format!("activity-{turn_id}-{ix}"));
                    let mark = status_mark(
                        SharedString::from(format!("activity-mark-{turn_id}-{ix}")),
                        *state,
                        s,
                        window,
                        cx,
                    );
                    let row = disclosure_line(id, mark, title, open, s)
                        .ink(
                            if *state == Phase::Failed {
                                s.pro.red_ink
                            } else {
                                s.muted
                            },
                            s.ink,
                        )
                        .on_press(cx, move |wb, _, cx| {
                            wb.tool_detail = (!open).then_some(address);
                            cx.notify();
                        });
                    rows = rows.child(row);
                    if open {
                        rows = rows.child(self.render_activity_detail(
                            address, tool, item.kind, *state, s, window, cx,
                        ));
                    }
                }
            }
            if multiple {
                let count = group.len();
                let mut title = format!(
                    "{} · {count} {}",
                    first.kind.label(loc),
                    loc.pick("次", "calls")
                );
                if subagent {
                    title = format!("{} · {title}", loc.pick("子智能体", "Subagent"));
                }
                let mark = pro_icon(
                    match first.kind {
                        Kind::Search => ProIcon::Search,
                        Kind::Read => ProIcon::PageDoc,
                        Kind::List => ProIcon::Folder,
                        Kind::JavaScript => ProIcon::Code,
                        _ => ProIcon::Terminal,
                    },
                    ICON_SM,
                    s.muted,
                );
                list = list
                    .child(
                        disclosure_line(
                            SharedString::from(format!("activity-group-{turn_id}-{ix}")),
                            mark,
                            title,
                            group_open,
                            s,
                        )
                        .on_press(cx, move |wb, _, cx| {
                            wb.part_open.insert(key, !group_open);
                            cx.notify();
                        }),
                    )
                    .child(disclosure_body(
                        SharedString::from(format!("activity-list-{turn_id}-{ix}")),
                        group_open,
                        count as f32 * 30.,
                        rows.into_any_element(),
                        s,
                    ));
            } else {
                list = list.child(rows);
            }
            start = end;
        }
        list
    }

    fn render_activity_detail(
        &mut self,
        key: (Uuid, usize),
        tool: &Tool,
        kind: Kind,
        state: Phase,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let data = args(tool);
        let input = data.as_deref();
        let raw_command = command(tool, input);
        let code = (kind == Kind::JavaScript)
            .then(|| string_arg(input, &["code", "script", "js"]).or_else(|| input?.as_str()))
            .flatten();
        let mut parts = Vec::new();
        if let Some(cmd) = raw_command.as_ref() {
            parts.push(("command", loc.pick("命令", "Command"), cmd.to_owned()));
        } else if let Some(code) = code {
            parts.push(("code", "JavaScript", code.to_owned()));
        }
        if let Some(mut value) = data.as_deref().cloned() {
            if raw_command.is_some() || code.is_some() {
                if let Some(map) = value.as_object_mut() {
                    for key in [
                        "cmd",
                        "command",
                        "CommandLine",
                        "code",
                        "js",
                        "script",
                        "title",
                        "description",
                        "summary",
                        "toolSummary",
                    ] {
                        map.remove(key);
                    }
                } else {
                    value = Value::Null;
                }
            }
            if !value.is_null() && !value.as_object().is_some_and(|v| v.is_empty()) {
                let args = value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| serde_json::to_string_pretty(&value).unwrap_or_default());
                parts.push(("input", loc.pick("参数", "Input"), args));
            }
        }
        let result = tool.result.as_ref();
        let output = result.and_then(|r| r.content.as_ref()).map(|parts| {
            parts
                .iter()
                .map(|part| match part {
                    OutputPart::Text { text } => text.clone(),
                    OutputPart::Image { media_type, .. } => {
                        format!("[{} · {media_type}]", loc.pick("图片", "Image"))
                    }
                    OutputPart::Other { value } => {
                        serde_json::to_string_pretty(value).unwrap_or_default()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        });
        if let Some(out) = output.filter(|s| !s.is_empty()) {
            parts.push(("output", loc.pick("输出", "Output"), out));
        } else if !tool.preview.is_empty() {
            parts.push((
                "progress",
                loc.pick("过程输出", "Progress output"),
                tool.preview.clone(),
            ));
        }
        let mut box_el = div()
            .w_full()
            .min_w_0()
            .my(px(3.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(R_CTRL))
            .bg(s.soft)
            .flex()
            .flex_col()
            .gap(px(6.));
        let status = state.label(loc);
        let meta = result
            .and_then(|r| r.exit_code)
            .map(|code| format!("{status} · {} {code}", loc.pick("退出码", "Exit code")))
            .unwrap_or_else(|| status.to_owned());
        box_el = box_el.child(
            div()
                .text_size(px(FS_META))
                .text_color(if state == Phase::Failed {
                    s.pro.red_ink
                } else {
                    s.muted
                })
                .child(meta),
        );
        for (part, label, text) in parts {
            let id = format!("activity-{}-{}-{part}", key.0, key.1);
            let is_final_json =
                !tool.input_partial && tool.finished && matches!(part, "input" | "output");
            if is_final_json
                && text.trim_start().starts_with(['{', '['])
                && let Ok(value @ (Value::Object(_) | Value::Array(_))) =
                    serde_json::from_str(&text)
            {
                let viewer = self.tool_json_viewers.entry(id).or_insert_with(|| {
                    cx.new(|cx| JsonViewer::new(label, value, window, cx).with_max_height(180.))
                });
                box_el = box_el.child(viewer.clone());
                continue;
            }
            let copy = copy_text_button(format!("{id}-copy"), format!("{} {label}", loc.pick("复制", "Copy")), text.clone().into(), window, cx);
            let view = div()
                .id(SharedString::from(id))
                .w_full()
                .min_w_0()
                .max_h(px(160.))
                .overflow_y_scroll()
                .tab_stop(true)
                .font_family(mono_font(cx))
                .text_size(px(FS_META))
                .line_height(px(18.))
                .text_color(if part == "output" && state == Phase::Failed {
                    s.pro.red_ink
                } else {
                    s.ink
                })
                .child(text);
            box_el = box_el
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(FS_META))
                                .text_color(s.muted)
                                .child(label),
                        )
                        .child(copy),
                )
                .child(view);
        }
        box_el
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Phase, Tool, UiLocale, Value, args, command, phase, summary, unwrap_shell_command,
    };
    use serde_json::json;

    #[test]
    fn command_display_preserves_flags_quotes_and_legacy_argument_vectors() {
        let command_text = "python -c 'print(\"a b\")'";
        assert_eq!(unwrap_shell_command(command_text), command_text);
        let words = json!({"command": ["/bin/zsh", "-lc", command_text]});
        let tool = Tool {
            input: Some(Value::String(words.to_string())),
            name: Some("Bash".into()),
            ..Default::default()
        };
        assert_eq!(
            command(&tool, args(&tool).as_deref()).as_deref(),
            Some(command_text)
        );
    }

    #[test]
    fn missing_end_or_exit_status_does_not_imply_success() {
        let mut tool = Tool::default();
        assert_eq!(phase(&tool, true), Phase::Running);
        assert_eq!(phase(&tool, false), Phase::Unfinished);
        tool.finished = true;
        assert_eq!(phase(&tool, false), Phase::Ended);
        tool.result = Some(kar::ToolResult {
            exit_code: Some(2),
            ..Default::default()
        });
        assert_eq!(phase(&tool, false), Phase::Failed);
    }

    #[test]
    fn unrelated_output_and_javascript_source_are_never_row_titles() {
        let tool = Tool {
            name: Some("inspect".into()),
            preview: "An arbitrary long program output".into(),
            ..Default::default()
        };
        assert_eq!(summary(&tool, UiLocale::Zh).title, "inspect");
        let js = Tool {
            tool_name: Some("js".into()),
            input: Some(json!({"code":"console.log(1)","title":"Inspect document"})),
            ..Default::default()
        };
        assert_eq!(
            summary(&js, UiLocale::En).title,
            "Run JavaScript · Inspect document"
        );
    }
}
