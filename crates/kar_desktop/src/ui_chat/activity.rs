//! Compact views of the provider's public activity records.
use super::*;
use crate::types::office_mcp_summary;
use crate::ui_motion::selection::{Group, SelectionStrip};
use kar::{OutputPart, ToolStatus, observe::ToolActionKind};
use kui::components::{json_viewer::JsonViewer, text_shimmer::TextShimmer};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Search,
    Read,
    List,
    JavaScript,
    Command,
    Edit,
    Web,
    Fetch,
    Agent,
    Wait,
    Image,
    Other,
}

impl Kind {
    fn exploratory(self) -> bool {
        matches!(self, Self::Search | Self::Read | Self::List | Self::Command)
    }

    fn icon(self) -> ProIcon {
        match self {
            Self::Search => ProIcon::Search,
            Self::Read => ProIcon::PageDoc,
            Self::List => ProIcon::Folder,
            Self::Command => ProIcon::Terminal,
            Self::JavaScript => ProIcon::Code,
            Self::Edit => ProIcon::NewChat,
            Self::Web | Self::Fetch => ProIcon::Globe,
            Self::Agent => ProIcon::Users,
            Self::Wait => ProIcon::Clock,
            Self::Image => ProIcon::Eye,
            Self::Other => ProIcon::Grid,
        }
    }

    fn phrase(self, running: bool, loc: UiLocale) -> &'static str {
        let (present, past) = match self {
            Self::Search => (("正在搜索", "Searching"), ("已搜索", "Searched")),
            Self::Read => (
                ("正在读取文件", "Reading file"),
                ("已读取文件", "Read file"),
            ),
            Self::List => (
                ("正在查看目录", "Listing directory"),
                ("已查看目录", "Listed directory"),
            ),
            Self::Command => (
                ("正在运行命令", "Running command"),
                ("运行了命令", "Ran command"),
            ),
            Self::JavaScript => (
                ("正在运行 JavaScript", "Running JavaScript"),
                ("已运行 JavaScript", "Ran JavaScript"),
            ),
            Self::Edit => (
                ("正在编辑文件", "Editing file"),
                ("已编辑文件", "Edited file"),
            ),
            Self::Web => (
                ("正在搜索网页", "Searching the web"),
                ("已搜索网页", "Searched the web"),
            ),
            Self::Fetch => (
                ("正在获取网页", "Fetching page"),
                ("已获取网页", "Fetched page"),
            ),
            Self::Agent => (
                ("正在运行子任务", "Running subagent"),
                ("已运行子任务", "Ran subagent"),
            ),
            Self::Wait => (("正在等待", "Waiting"), ("等待已结束", "Finished waiting")),
            Self::Image => (
                ("正在查看图像", "Viewing image"),
                ("已查看图像", "Viewed image"),
            ),
            Self::Other => (
                ("正在调用工具", "Calling tool"),
                ("已调用工具", "Called tool"),
            ),
        };
        let (zh, en) = if running { present } else { past };
        loc.pick(zh, en)
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
    subject: Option<String>,
}

impl Summary {
    fn label(&self, state: Phase, loc: UiLocale) -> String {
        let label = self.kind.phrase(state == Phase::Running, loc);
        // A failure or cancellation must never read as a successfully performed action.
        let subject = self.subject.as_deref().filter(|s| !s.is_empty());
        if matches!(state, Phase::Failed | Phase::Cancelled | Phase::Unfinished) {
            return subject.map_or_else(
                || state.label(loc).to_owned(),
                |name| format!("{} · {name}", state.label(loc)),
            );
        }
        match subject {
            Some(subject) if self.kind == Kind::Other => subject.to_owned(),
            Some(subject) => format!("{label} · {subject}"),
            None => label.to_owned(),
        }
    }
}

fn summary(tool: &Tool, runtime: &str, loc: UiLocale) -> Summary {
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
    let returned_image = tool
        .result
        .as_ref()
        .and_then(|r| r.content.as_ref())
        .is_some_and(|parts| parts.iter().any(|p| matches!(p, OutputPart::Image { .. })));
    let agent_wait = matches!(tool.native_type.as_deref(), Some("collabAgentToolCall" | "collabToolCall"))
        && matches!(bare, "wait" | "wait_agent" | "waitAgent");
    let (kind, subject) = if agent_wait {
        (Kind::Wait, Some(loc.pick("子任务", "Subagent").to_owned()))
    } else if returned_image && matches!(bare, "read" | "Read" | "read_file") {
        (
            Kind::Image,
            string_arg(input, &["file_path", "path"]).map(short),
        )
    } else if matches!(bare, "js" | "node_repl" | "javascript") {
        (Kind::JavaScript, hint.map(short))
    } else if matches!(
        bare,
        "view_image" | "imageView" | "viewImage" | "open_image"
    ) {
        (
            Kind::Image,
            hint.map(short)
                .or_else(|| string_arg(input, &["path", "file_path"]).map(short)),
        )
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
        let native = tool.action(runtime);
        let kind = match native.kind {
            ToolActionKind::RunCommand => Kind::Command,
            ToolActionKind::ReadFile => Kind::Read,
            ToolActionKind::EditFile => Kind::Edit,
            ToolActionKind::Search => Kind::Search,
            ToolActionKind::Web => Kind::Web,
            ToolActionKind::Fetch => Kind::Fetch,
            ToolActionKind::Subagent => Kind::Agent,
            ToolActionKind::Wait => Kind::Wait,
            _ => Kind::Other,
        };
        let subject = hint
            .map(short)
            .or_else(|| native.paths.first().map(|s| short(s)))
            .or_else(|| {
                string_arg(
                    input,
                    &["agent_name", "task_name", "query", "pattern", "path", "url"],
                )
                .map(short)
            })
            .or_else(|| {
                (kind == Kind::Web)
                    .then(|| input?.as_str().map(short))
                    .flatten()
            });
        if kind == Kind::Other {
            let office = (loc == UiLocale::Zh)
                .then(|| office_mcp_summary(bare, input))
                .flatten();
            let name = (!bare.is_empty()).then(|| short(bare));
            (kind, office.or(subject).or(name))
        } else {
            (kind, subject)
        }
    };
    Summary { kind, subject }
}

/// Split a public reasoning summary into its short disclosure and the remaining body.
pub(super) fn reasoning_summary(text: &str) -> Option<(&str, &str)> {
    let (line, body) = text.trim().split_once('\n')?;
    let line = line.trim();
    let heading = if let Some(bold) = line.strip_prefix("**") {
        bold.strip_suffix("**")?
    } else {
        line.trim_start_matches('#')
    };
    let heading = heading.trim();
    let body = body.trim_start();
    (!heading.is_empty() && !body.is_empty() && heading.chars().take(81).count() <= 80)
        .then_some((heading, body))
}

pub(super) fn shimmer(
    id: SharedString,
    text: SharedString,
    size: f32,
    window: &mut Window,
    cx: &mut App,
) -> Entity<TextShimmer> {
    let view = window
        .use_keyed_state(id, cx, |_, cx| {
            cx.new(|cx| {
                TextShimmer::new(text.clone(), cx)
                    .with_font_size(size)
                    .with_highlight_strength(0.88)
                    .with_duration(std::time::Duration::from_millis(1650))
            })
        })
        .read(cx)
        .clone();
    view.update(cx, |view, cx| view.set_text(text, cx));
    view
}

/// Semantic icon, one line of status, and a disclosure affordance flush with the reply.
pub(super) fn activity_line(
    id: SharedString,
    icon: ProIcon,
    label: String,
    state: Phase,
    open: bool,
    s: &StudioPalette,
    window: &mut Window,
    cx: &mut App,
) -> Tactile {
    let active = state == Phase::Running;
    let tone = if state == Phase::Failed {
        s.pro.red_ink
    } else {
        s.muted
    };
    let text: AnyElement = if active {
        let text: SharedString = label.clone().into();
        let shimmer = shimmer(format!("{id}-shimmer").into(), text, FS_UI, window, cx);
        div()
            .min_w_0()
            .overflow_hidden()
            .child(shimmer)
            .into_any_element()
    } else {
        div()
            .min_w_0()
            .truncate()
            .child(label.clone())
            .into_any_element()
    };
    let body = div()
        .group(id.clone())
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(div().flex_none().child(pro_icon(icon, ICON_SM, tone)))
        .child(text)
        .child(
            div()
                .flex_none()
                .when(
                    !open && !window.last_input_was_keyboard() && !window.last_input_was_touch(),
                    |d| d.opacity(0.).group_hover(id.clone(), |h| h.opacity(1.)),
                )
                .child(pro_icon(
                    if open {
                        ProIcon::ChevronDown
                    } else {
                        ProIcon::ChevronRight
                    },
                    ICON_XS,
                    s.faint,
                )),
        );
    Tactile::ghost(id, s)
        .aria(label)
        .expanded(open)
        .ml(px(-6.))
        .px(px(6.))
        .max_w_full()
        .h(px(CTRL_SM_H))
        .text_size(s.font_size(FS_UI))
        .line_height(s.line_height(LH_UI))
        .ink(tone, s.ink)
        .child(body)
}

pub(super) fn branch_header(
    turn: Uuid,
    ix: usize,
    path: &[&str],
    loc: UiLocale,
    s: &StudioPalette,
    window: &mut Window,
    cx: &mut Context<KarWorkbench>,
) -> (AnyElement, bool) {
    let key = SharedString::from(format!("branch-{turn}-{ix}"));
    let state = window.use_keyed_state(key.clone(), cx, |_, _| true);
    let open = *state.read(cx);
    let path_label = path
        .iter()
        .map(|id| {
            if Uuid::parse_str(id).is_ok() {
                format!("…{}", &id[id.len() - 8..])
            } else {
                short(id)
            }
        })
        .collect::<Vec<_>>()
        .join(" › ");
    let label = loc.pick("子任务", "Subagent");
    let full = format!("{label} · {}", path.join(" › "));
    let header = Tactile::ghost(key, s)
        .aria(full.clone())
        .tip(full)
        .expanded(open)
        .h(px(CTRL_SM_H))
        .max_w_full()
        .ml(px(-5.))
        .px(px(5.))
        .gap(px(6.))
        .ink(s.muted, s.ink)
        .child(pro_icon(ProIcon::GitBranch, ICON_SM, s.muted))
        .child(
            div()
                .flex_none()
                .text_size(s.font_size(FS_META))
                .font_weight(FontWeight::MEDIUM)
                .child(label),
        )
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(s.font_size(FS_META))
                .child(path_label),
        )
        .child(pro_icon(
            if open {
                ProIcon::ChevronDown
            } else {
                ProIcon::ChevronRight
            },
            ICON_XS,
            s.muted,
        ))
        .on_press(cx, move |_, _, cx| {
            state.update(cx, |open, cx| {
                *open = !*open;
                cx.notify();
            })
        });
    (header.into_any_element(), open)
}

impl KarWorkbench {
    pub(super) fn render_tool_run(
        &mut self,
        turn_id: Uuid,
        calls: &[(usize, &Tool)],
        running: bool,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let items: Vec<_> = calls
            .iter()
            .map(|(ix, tool)| {
                (
                    *ix,
                    *tool,
                    summary(
                        tool,
                        self.projected.info.as_ref().map_or("", |i| &i.runtime),
                        loc,
                    ),
                    phase(tool, running),
                )
            })
            .collect();
        let mut list = div().w_full().min_w_0().flex().flex_col().gap(px(2.));
        let mut start = 0;
        while start < items.len() {
            let (ix, _, first, state) = &items[start];
            let mut end = start + 1;
            if state.groupable() && !matches!(first.kind, Kind::Other | Kind::Agent | Kind::Image) {
                while end < items.len()
                    && items[end].3 == *state
                    && (items[end].2.kind == first.kind
                        || first.kind.exploratory() && items[end].2.kind.exploratory())
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
                    let mut title = item.label(*state, loc);
                    if multiple && item.subject.is_none() {
                        title.push_str(&format!(" · {}", number + 1));
                    }
                    let id = SharedString::from(format!("activity-{turn_id}-{ix}"));
                    let row =
                        activity_line(id, item.kind.icon(), title, *state, open, s, window, cx)
                            .on_press(cx, move |wb, _, cx| {
                                wb.tool_detail = (!open).then_some(address);
                                cx.notify();
                            });
                    rows = rows.child(if running {
                        arrive(div().child(row), SharedString::from(format!("tool-arrival-{turn_id}-{ix}")), 0, 6., cx)
                    } else { row.into_any_element() });
                    if open {
                        rows = rows.child(self.render_activity_detail(
                            address, tool, item.kind, *state, s, window, cx,
                        ));
                    }
                }
            }
            if multiple {
                let count = group.len();
                let mut kinds = Vec::new();
                for (_, _, item, _) in group {
                    if !kinds.contains(&item.kind) {
                        kinds.push(item.kind);
                    }
                }
                let title = if kinds.len() == 1 {
                    format!(
                        "{} · {count} {}",
                        first.kind.phrase(false, loc),
                        loc.pick("次", "calls")
                    )
                } else {
                    kinds
                        .iter()
                        .map(|kind| kind.phrase(false, loc))
                        .collect::<Vec<_>>()
                        .join(loc.pick("、", ", "))
                };
                list = list
                    .child(
                        activity_line(
                            SharedString::from(format!("activity-group-{turn_id}-{ix}")),
                            first.kind.icon(),
                            title,
                            *state,
                            group_open,
                            s,
                            window,
                            cx,
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
        let result = tool.result.as_ref();
        // Decode only images the tool actually returned, and only while its details are open.
        let mut previews = Vec::new();
        if let Some(content) = result.and_then(|r| r.content.as_ref()) {
            for (index, part) in content.iter().enumerate() {
                if let OutputPart::Image { media_type, data } = part
                    && let Some(preview) =
                        recorded_image(key, index, media_type, data, s, window, cx)
                {
                    previews.push(preview);
                }
            }
        }
        let mut gallery = div().w_full().min_w_0().flex().flex_col().gap(px(4.));
        if !previews.is_empty() {
            let key: SharedString = format!("image-metadata-{}-{}", key.0, key.1).into();
            let details = window.use_keyed_state(key.clone(), cx, |_, _| kind != Kind::Image);
            let open = *details.read(cx);
            let label = loc.pick("调用详情", "Call details");
            gallery = gallery.child(
                div()
                    .w_full()
                    .min_w_0()
                    .ml(px(6.))
                    .pl(px(14.))
                    .py(px(4.))
                    .border_l_1()
                    .border_color(s.line)
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(4.))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_wrap()
                            .gap(px(8.))
                            .children(previews),
                    )
                    .child(
                        Tactile::ghost(key, s)
                            .aria(label)
                            .expanded(open)
                            .ml(px(-4.))
                            .px(px(4.))
                            .h(px(24.))
                            .text_size(s.font_size(FS_META))
                            .ink(s.muted, s.ink)
                            .gap(px(4.))
                            .child(label)
                            .child(pro_icon(
                                if open {
                                    ProIcon::ChevronDown
                                } else {
                                    ProIcon::ChevronRight
                                },
                                ICON_XS,
                                s.faint,
                            ))
                            .on_press(cx, move |_, _, cx| {
                                details.update(cx, |open, _| *open = !*open);
                                cx.notify();
                            }),
                    ),
            );
            if !open {
                return gallery;
            }
        }
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
        let identity = tool.tool_name.as_deref().or(tool.name.as_deref());
        let identity = match (tool.server.as_deref(), identity) {
            (Some(server), Some(name)) => Some(format!("{server} / {name}")),
            (_, name) => name.map(str::to_owned),
        };
        box_el = box_el.child(
            div()
                .w_full().min_w_0().flex().items_center().gap(px(8.))
                .text_size(s.font_size(FS_META))
                .text_color(if state == Phase::Failed {
                    s.pro.red_ink
                } else {
                    s.muted
                })
                .child(div().flex_none().child(meta))
                .when_some(identity, |row, name| row.child(
                    div().min_w_0().truncate().text_color(s.muted).child(name)
                )),
        );
        let is_output = |part: &str| matches!(part, "output" | "progress");
        let has_input = parts.iter().any(|(part, _, _)| !is_output(part));
        let has_output = parts.iter().any(|(part, _, _)| is_output(part));
        let mut tabs = None;
        if has_input && has_output {
            let id = SharedString::from(format!("tool-payload-{}-{}", key.0, key.1));
            let side = window.use_keyed_state(id.clone(), cx, |_, _| true);
            let show_output = *side.read(cx);
            let strip = SelectionStrip::options(
                id, loc.pick("调用详情", "Call details"), Group::Tabs,
                [(false, loc.pick("输入", "Input")), (true, loc.pick("输出", "Output"))],
                show_output, s, cx, move |_, selected, _, cx| {
                    side.update(cx, |side, cx| { *side = selected; cx.notify(); });
                },
            );
            parts.retain(|(part, _, _)| is_output(part) == show_output);
            // A single payload needs only one heading. Share its row with Copy.
            if parts.len() == 1 {
                tabs = Some(strip);
            } else {
                box_el = box_el.child(div().flex().child(strip));
            }
        }
        let code_size = crate::ui_primitives::effective_code_font_size(cx);
        for (part, label, text) in parts {
            let id = format!("activity-{}-{}-{part}", key.0, key.1);
            let is_final_json =
                !tool.input_partial && tool.finished && matches!(part, "input" | "output");
            if is_final_json
                && text.trim_start().starts_with(['{', '['])
                && let Ok(value @ (Value::Object(_) | Value::Array(_))) =
                    serde_json::from_str(&text)
            {
                if let Some(strip) = tabs.take() {
                    box_el = box_el.child(div().flex().child(strip));
                }
                let viewer = self.tool_json_viewers.entry(id).or_insert_with(|| {
                    cx.new(|cx| JsonViewer::new(label, value, window, cx).with_max_height(180.))
                });
                box_el = box_el.child(viewer.clone());
                continue;
            }
            let copy = copy_text_button(
                format!("{id}-copy"),
                format!("{} {label}", loc.pick("复制", "Copy")),
                text.clone().into(),
                window,
                cx,
            );
            let view = div()
                .id(SharedString::from(id))
                .w_full()
                .min_w_0()
                .max_h(px(160.))
                .overflow_y_scroll()
                .tab_stop(true)
                .font_family(mono_font(cx))
                .text_size(px(code_size))
                .line_height(px(code_size * 1.5))
                .text_color(if part == "output" && state == Phase::Failed {
                    s.pro.red_ink
                } else {
                    s.ink
                })
                .child(text);
            let heading = match tabs.take() {
                Some(strip) => strip.into_any_element(),
                None => div().text_size(s.font_size(FS_META)).text_color(s.muted)
                    .child(label).into_any_element(),
            };
            box_el = box_el
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(heading)
                        .child(copy),
                )
                .child(view);
        }
        gallery.child(box_el)
    }
}

fn recorded_image(
    key: (Uuid, usize),
    index: usize,
    mime: &str,
    data: &str,
    s: &StudioPalette,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use std::sync::Arc;

    let format = ImageFormat::from_mime_type(mime)?;
    let id: SharedString = format!("tool-image-{}-{}-{index}", key.0, key.1).into();
    let cached = window.use_keyed_state(id, cx, |_, _| {
        None::<(String, ImageFormat, Option<Arc<Image>>)>
    });
    let image = cached.update(cx, |cache, _| {
        if cache
            .as_ref()
            .is_none_or(|(old, fmt, _)| old != data || *fmt != format)
        {
            let image = STANDARD
                .decode(data)
                .ok()
                .filter(|b| !b.is_empty())
                .map(|bytes| Arc::new(Image::from_bytes(format, bytes)));
            *cache = Some((data.to_owned(), format, image));
        }
        cache.as_ref()?.2.clone()
    })?;
    Some(
        div()
            .w(px(132.))
            .max_w_full()
            .h(px(82.))
            .rounded(px(R_CTRL))
            .border_1()
            .border_color(s.line_soft)
            .overflow_hidden()
            .child(img(image).size_full().object_fit(ObjectFit::Contain))
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        Phase, Tool, UiLocale, Value, args, command, phase, reasoning_summary, summary,
        unwrap_shell_command,
    };
    use serde_json::json;

    #[test]
    fn reasoning_disclosure_separates_complete_headings_from_their_details() {
        assert_eq!(reasoning_summary("**Inspecting changes"), None);
        assert_eq!(reasoning_summary("Scoping the"), None);
        assert_eq!(reasoning_summary("**Checking files**\n\n"), None);
        assert_eq!(reasoning_summary("**Inspecting changes**\n\nThree files."), Some(("Inspecting changes", "Three files.")));
        assert_eq!(reasoning_summary("## 检查项目\n内容"), Some(("检查项目", "内容")));
        assert_eq!(reasoning_summary("Checking the manifest\nNext step"), Some(("Checking the manifest", "Next step")));
        assert_eq!(reasoning_summary(&format!("{}\nbody", "字".repeat(81))), None);
    }

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
    fn semantic_actions_follow_the_provider_and_completed_commands_keep_their_subject() {
        use super::Kind;
        let read = Tool {
            name: Some("Read".into()),
            input: Some(json!({"file_path": "notes.txt"})),
            ..Default::default()
        };
        let action = summary(&read, "claude", UiLocale::Zh);
        assert_eq!(action.kind, Kind::Read);
        assert_eq!(
            action.label(Phase::Running, UiLocale::Zh),
            "正在读取文件 · notes.txt"
        );
        assert_eq!(
            action.label(Phase::Succeeded, UiLocale::Zh),
            "已读取文件 · notes.txt"
        );
        let agent = Tool {
            native_type: Some("collabAgentToolCall".into()),
            ..Default::default()
        };
        assert_eq!(summary(&agent, "codex", UiLocale::En).kind, Kind::Agent);
        let wait = Tool { tool_name: Some("wait".into()), ..agent };
        for runtime in ["codex", ""] {
            let action = summary(&wait, runtime, UiLocale::En);
            assert_eq!(action.kind, Kind::Wait);
            assert_eq!(action.label(Phase::Succeeded, UiLocale::En), "Finished waiting · Subagent");
        }
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
        assert_eq!(
            summary(&tool, "codex", UiLocale::Zh).label(Phase::Succeeded, UiLocale::Zh),
            "inspect"
        );
        let js = Tool {
            tool_name: Some("js".into()),
            input: Some(json!({"code":"console.log(1)","title":"Inspect document"})),
            ..Default::default()
        };
        assert_eq!(
            summary(&js, "codex", UiLocale::En).label(Phase::Running, UiLocale::En),
            "Running JavaScript · Inspect document"
        );
    }
}
