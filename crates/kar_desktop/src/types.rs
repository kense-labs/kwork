//! Shared domain types for Kar Desktop Workbench (Chat, Office Selection Binding, Providers).

use std::path::PathBuf;

use kar::{
    FailureClass, RetryTiming, TurnFailure,
    chat::{ChatInfo, ChatTurn, Item, Role, TurnStatus},
    observe::Tool,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Opening line of the machine prompt that wraps a user request aimed at an Office selection.
const SELECTION_ENVELOPE_HEAD: &str = "请针对以下 Office 文档选区";
/// Marker after which the user's own words begin inside that prompt.
const SELECTION_ENVELOPE_MARK: &str = "用户修改要求：\n";
/// Opening line of the machine prompt that wraps a user request aimed at a source code selection.
const CODE_SELECTION_HEAD: &str = "请针对以下工作区源码文件";
/// Marker after which the user's own words begin inside a source code prompt.
const CODE_SELECTION_MARK: &str = "用户要求：\n";

/// What the person actually typed, without the Office/code selection prompt wrapped around it.
pub fn strip_selection_envelope(text: &str) -> &str {
    if text.starts_with(SELECTION_ENVELOPE_HEAD)
        && let Some(pos) = text.find(SELECTION_ENVELOPE_MARK)
    {
        return text[pos + SELECTION_ENVELOPE_MARK.len()..].trim();
    }
    if text.starts_with(CODE_SELECTION_HEAD)
        && let Some(pos) = text.find(CODE_SELECTION_MARK)
    {
        return text[pos + CODE_SELECTION_MARK.len()..].trim();
    }
    text
}

/// Product name of a Kar runtime, sourced from `kar::brands::runtime_brands()` with fallback.
pub fn runtime_display_name(id: &str) -> String {
    if let Some(brand) = kar::brands::runtime_brands().get(id) {
        return brand.name.clone();
    }
    match id {
        "codex" => "Codex",
        "claude" => "Claude Code",
        "opencode" => "OpenCode",
        "grok" => "Grok",
        "kimi" => "Kimi",
        "devin" => "Devin",
        "codebuddy" => "CodeBuddy",
        "antigravity" => "Antigravity",
        "qwen" => "Qwen Code",
        "cursor" => "Cursor",
        "qoder" => "Qoder",
        "dsh" => "DSH",
        "factory" => "Factory Droid",
        "pi" => "Pi",
        "kiro" => "Kiro",
        "fx" => "Fx",
        "muse" => "Muse",
        other => return other.to_owned(),
    }
    .to_owned()
}

/// Official SVG data URI for a runtime from `kar::brands::runtime_brands()`,
/// with crisp vector fallbacks for workbench-registered runtimes (`kiro`, `fx`, `muse`).
pub fn runtime_brand_svg(id: &str, dark_bg: bool) -> Option<&'static str> {
    if let Some(brand) = kar::brands::runtime_brands().get(id) {
        let bg = if dark_bg {
            kar::brands::Background::Dark
        } else {
            kar::brands::Background::Light
        };
        if let Some(svg) = brand.icon_for(bg) {
            return Some(svg);
        }
    }
    match (id, dark_bg) {
        ("kiro", false) => Some("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%2318181b' stroke-width='2.2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M7 5v14M17 5l-7 7 7 7'/%3E%3C/svg%3E"),
        ("kiro", true) => Some("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%23f4f4f5' stroke-width='2.2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M7 5v14M17 5l-7 7 7 7'/%3E%3C/svg%3E"),
        ("fx", false) => Some("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%2318181b' stroke-width='2.2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M9 19c1.5 0 2.5-1 3-4l1-6c.5-3 1.5-4 3-4M8 11h6M15 13l5 6M20 13l-5 6'/%3E%3C/svg%3E"),
        ("fx", true) => Some("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%23f4f4f5' stroke-width='2.2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M9 19c1.5 0 2.5-1 3-4l1-6c.5-3 1.5-4 3-4M8 11h6M15 13l5 6M20 13l-5 6'/%3E%3C/svg%3E"),
        ("muse", false) => Some("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%2318181b' stroke-width='2.2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M4 18V6l8 7 8-7v12'/%3E%3C/svg%3E"),
        ("muse", true) => Some("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%23f4f4f5' stroke-width='2.2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M4 18V6l8 7 8-7v12'/%3E%3C/svg%3E"),
        _ => None,
    }
}

/// These runtimes reject a nonempty effort in their Kar session options.
pub fn runtime_supports_effort(runtime: &str) -> bool {
    !matches!(runtime, "morph" | "cybion")
}

/// The version number inside a CLI's `--version` output, e.g. `grok 1.0.70 (f509…)` -> `1.0.70`.
pub fn short_version(raw: &str) -> Option<String> {
    raw.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',')
        .map(|tok| tok.trim_start_matches('v'))
        .find(|tok| {
            tok.contains('.')
                && tok.starts_with(|c: char| c.is_ascii_digit())
                && tok.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        })
        .map(str::to_owned)
}

/// Plain-language account of a failed turn: what happened, the provider's words, what to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailureCopy {
    pub title: &'static str,
    pub detail: String,
    pub hint: Option<&'static str>,
}

pub fn describe_failure(failure: &TurnFailure) -> FailureCopy {
    let title = match failure.class {
        FailureClass::Auth => "登录已失效",
        FailureClass::Quota => "额度已用完",
        FailureClass::RateLimited => "请求过于频繁",
        FailureClass::Billing => "账户计费出现问题",
        FailureClass::ModelUnavailable => "模型暂不可用",
        FailureClass::InputTooLarge => "内容超出了模型的长度上限",
        FailureClass::ModelRefused => "模型拒绝了这个请求",
        FailureClass::InvalidRequest => "请求未被接受",
        FailureClass::Overloaded => "服务繁忙",
        FailureClass::Provider => "服务端出错",
        FailureClass::RuntimeExited => "智能体意外退出",
        FailureClass::LimitReached => "已达到运行上限",
        FailureClass::OutputSchema => "回复格式不符合要求",
        FailureClass::Unknown => "这一步没有完成",
    };
    let advice = failure.class.advice();
    let hint = match (advice.retry, advice.user_action) {
        (RetryTiming::Now, _) => Some("可以直接重试。"),
        (RetryTiming::Later, _) => Some("额度恢复后再试。"),
        (RetryTiming::No, true) => Some("处理后再重试。"),
        (RetryTiming::No, false) => None,
    };
    FailureCopy {
        title,
        detail: failure.reason.trim().to_owned(),
        hint,
    }
}

/// Where a tool call stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolPhase {
    Running,
    Done,
    Failed,
    Cancelled,
}

pub fn tool_phase(tool: &Tool) -> ToolPhase {
    if !tool.finished {
        return ToolPhase::Running;
    }
    match tool.result.as_ref().and_then(|r| r.status.as_ref()) {
        Some(kar::ToolStatus::Failed) => ToolPhase::Failed,
        Some(kar::ToolStatus::Cancelled) => ToolPhase::Cancelled,
        _ => ToolPhase::Done,
    }
}

/// The command a shell wrapper runs: `/bin/zsh -lc "ls -la"` becomes `ls -la`.
pub fn unwrap_shell_command(command: &str) -> String {
    let trimmed = command.trim();
    for flag in [" -lc ", " -c "] {
        if let Some(pos) = trimmed.find(flag) {
            let inner = trimmed[pos + flag.len()..].trim();
            let unquoted = inner
                .strip_prefix('"')
                .and_then(|r| r.strip_suffix('"'))
                .map(|r| r.replace("\\\"", "\"").replace("\\\\", "\\"))
                .or_else(|| {
                    inner
                        .strip_prefix('\'')
                        .and_then(|r| r.strip_suffix('\''))
                        .map(str::to_owned)
                });
            return unquoted.unwrap_or_else(|| inner.to_owned());
        }
    }
    trimmed.to_owned()
}

/// Friendly Chinese summary for hosted Kantor Office MCP calls.
fn office_mcp_summary(raw_name: &str, input: Option<&serde_json::Value>) -> Option<String> {
    let bare = raw_name
        .rsplit(['/', '.', ':'])
        .next()
        .unwrap_or(raw_name)
        .trim_start_matches("mcp__kantor__");
    let args = input.and_then(serde_json::Value::as_object);
    match bare {
        "office_list_tabs" => Some("查看已打开的 Office 文档".into()),
        "office_get_selection" | "inspect_office_selection" => Some("读取当前文档选区".into()),
        "office_read_word" | "docs_get_paragraphs" => Some("读取 Word 文档段落".into()),
        "office_edit_word" => {
            if let Some(p) = args.and_then(|m| m.get("paragraph")).and_then(|v| v.as_u64()) {
                Some(format!("修改 Word 第 {p} 段"))
            } else {
                Some("插入 Word 段落".into())
            }
        }
        "office_read_excel" => {
            if let Some(r) = args.and_then(|m| m.get("range")).and_then(|v| v.as_str()) {
                Some(format!("读取 Excel 区域 {r}"))
            } else {
                Some("读取 Excel 表格数据".into())
            }
        }
        "office_write_excel" => {
            if let Some(c) = args.and_then(|m| m.get("cell")).and_then(|v| v.as_str()) {
                Some(format!("写入 Excel 单元格 {c}"))
            } else {
                Some("批量更新 Excel 单元格".into())
            }
        }
        "office_read_ppt" => Some("读取 PPT 幻灯片结构".into()),
        "office_edit_ppt" => {
            if let Some(s) = args.and_then(|m| m.get("slide")).and_then(|v| v.as_u64()) {
                Some(format!("更新 PPT 第 {s} 页对象"))
            } else {
                Some("更新 PPT 幻灯片对象".into())
            }
        }
        "office_save_tab" => Some("保存打开的文档".into()),
        "kar_write_brief" | "write_brief" => {
            if let Some(t) = args.and_then(|m| m.get("title")).and_then(|v| v.as_str()) {
                Some(format!("生成交互简报 · {t}"))
            } else {
                Some("生成 KUI 交互式简报卡片".into())
            }
        }
        "kar_read_brief" | "read_brief" => Some("读取简报历史版本".into()),
        _ => None,
    }
}

/// What a tool call did, in the words a person would use: the command it ran, the file it
/// touched, the Office action it took, or the thing it looked up.
pub fn tool_summary(tool: &Tool) -> String {
    let name = tool
        .tool_name
        .as_deref()
        .or(tool.name.as_deref())
        .unwrap_or("工具");
    if let Some(office) = office_mcp_summary(name, tool.input.as_ref()) {
        return office;
    }
    let first_line = |text: &str| text.lines().next().unwrap_or("").trim().to_owned();
    let summary = match tool.input.as_ref() {
        // Codex reports a command as a bare string.
        Some(serde_json::Value::String(command)) => first_line(&unwrap_shell_command(command)),
        Some(serde_json::Value::Object(args)) => {
            let pick = |keys: &[&str]| {
                keys.iter().find_map(|k| match args.get(*k) {
                    Some(serde_json::Value::String(v)) if !v.trim().is_empty() => Some(v.clone()),
                    Some(serde_json::Value::Array(parts)) if !parts.is_empty() => Some(
                        parts
                            .iter()
                            .filter_map(|p| p.as_str())
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    _ => None,
                })
            };
            if let Some(command) = pick(&["command", "cmd", "script"]) {
                first_line(&unwrap_shell_command(&command))
            } else if let Some(subject) = pick(&[
                "file_path",
                "path",
                "filename",
                "url",
                "query",
                "pattern",
                "prompt",
                "description",
            ]) {
                format!("{name} · {}", first_line(&subject))
            } else {
                name.to_owned()
            }
        }
        _ if !tool.preview.trim().is_empty() => first_line(&tool.preview),
        _ => name.to_owned(),
    };
    if summary.is_empty() {
        name.to_owned()
    } else {
        summary
    }
}

/// A tool call's input as text to show: a command as typed, anything else as tidy JSON.
pub fn tool_input_text(tool: &Tool) -> Option<String> {
    match tool.input.as_ref()? {
        serde_json::Value::String(text) => Some(unwrap_shell_command(text)),
        serde_json::Value::Null => None,
        value => Some(serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())),
    }
}

/// What a tool call produced, as text to show.
pub fn tool_output_text(tool: &Tool) -> Option<String> {
    let Some(result) = &tool.result else {
        let preview = tool.preview.trim();
        return (!preview.is_empty()).then(|| preview.to_owned());
    };
    if let Some(text) = result.text().filter(|t| !t.trim().is_empty()) {
        return Some(text.trim_end().to_owned());
    }
    match result.exit_code {
        Some(code) if code != 0 => Some(format!("退出码 {code}")),
        _ => None,
    }
}

/// One line of a file change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffLine {
    Context(String),
    Added(String),
    Removed(String),
}

/// A unified diff reduced to what a reader needs: the file, and the lines that changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffView {
    pub file: String,
    pub lines: Vec<DiffLine>,
    pub added: usize,
    pub removed: usize,
}

pub fn parse_diff(diff: &str) -> DiffView {
    let mut view = DiffView::default();
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            let path = rest.trim().trim_start_matches("b/");
            if path != "/dev/null" {
                view.file = path.to_owned();
            }
        } else if let Some(rest) = line.strip_prefix("--- ") {
            let path = rest.trim().trim_start_matches("a/");
            if view.file.is_empty() && path != "/dev/null" {
                view.file = path.to_owned();
            }
        } else if line.starts_with("@@")
            || line.starts_with("diff ")
            || line.starts_with("index ")
            || line.starts_with("new file")
            || line.starts_with("deleted file")
            || line.starts_with('\\')
        {
            continue;
        } else if let Some(added) = line.strip_prefix('+') {
            view.added += 1;
            view.lines.push(DiffLine::Added(added.to_owned()));
        } else if let Some(removed) = line.strip_prefix('-') {
            view.removed += 1;
            view.lines.push(DiffLine::Removed(removed.to_owned()));
        } else {
            view.lines
                .push(DiffLine::Context(line.strip_prefix(' ').unwrap_or(line).to_owned()));
        }
    }
    view
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OfficeKind {
    Word,
    Excel,
    Ppt,
}

impl OfficeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Word => "Word (.docx)",
            Self::Excel => "Excel (.xlsx)",
            Self::Ppt => "PPT (.pptx)",
        }
    }

    pub fn short_tag(self) -> &'static str {
        match self {
            Self::Word => "DOCX",
            Self::Excel => "XLSX",
            Self::Ppt => "PPTX",
        }
    }

    pub fn for_path(path: &std::path::Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "docx" | "doc" | "rtf" | "odt" => Some(Self::Word),
            "xlsx" | "xls" | "csv" | "ods" => Some(Self::Excel),
            "pptx" | "ppt" | "odp" => Some(Self::Ppt),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SelectionLocator {
    WordParagraph {
        start_para: usize,
        end_para: usize,
        total_paras: usize,
        style: String,
    },
    ExcelRange {
        sheet_index: u32,
        sheet_name: String,
        range: String,
        active_cell: String,
        raw_input: String,
        computed_value: String,
    },
    PptObject {
        slide_index: usize,
        slide_count: usize,
        shape_id: Option<u32>,
        shape_index: Option<usize>,
        shape_name: String,
        shape_kind: String,
        bounds_cm: Option<(f64, f64, f64, f64)>,
    },
    CodeLines {
        rel_path: PathBuf,
        language: String,
        start_line: usize,
        end_line: usize,
        total_lines: usize,
    },
}

impl SelectionLocator {
    pub fn target_label(&self) -> String {
        match self {
            Self::WordParagraph {
                start_para,
                end_para,
                ..
            } => {
                if start_para == end_para {
                    format!("第 {} 段", start_para + 1)
                } else {
                    format!("第 {}–{} 段", start_para + 1, end_para + 1)
                }
            }
            Self::ExcelRange {
                sheet_name,
                range,
                active_cell,
                ..
            } => {
                if range == active_cell {
                    format!("{sheet_name}!{active_cell}")
                } else {
                    format!("{sheet_name}!{range}")
                }
            }
            Self::PptObject {
                slide_index,
                shape_id,
                shape_name,
                ..
            } => match shape_id {
                Some(_) if !shape_name.is_empty() => {
                    format!("第 {} 页 · {shape_name}", slide_index + 1)
                }
                Some(id) => format!("第 {} 页 · 对象 {id}", slide_index + 1),
                None => format!("第 {} 页", slide_index + 1),
            },
            Self::CodeLines {
                start_line,
                end_line,
                ..
            } => {
                if start_line == end_line {
                    format!("第 {start_line} 行")
                } else {
                    format!("L{start_line}–{end_line}")
                }
            }
        }
    }
}

/// Captures a concrete Office document or workspace code selection so Chat turns permanently remember
/// which file & paragraph/cell/slide-object/line-range a user request targeted, even across tab switches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectionTarget {
    pub tab_id: u64,
    pub file_path: Option<PathBuf>,
    pub doc_name: String,
    pub kind: OfficeKind,
    pub base_revision: Option<u64>,
    pub locator: SelectionLocator,
    pub excerpt: String,
}

pub fn format_rev(rev: u64) -> String {
    if rev > 1_000_000 {
        format!("rev #{:08x}", rev as u32)
    } else {
        format!("rev #{rev}")
    }
}

impl SelectionTarget {
    pub fn is_code(&self) -> bool {
        matches!(self.locator, SelectionLocator::CodeLines { .. })
    }

    pub fn short_badge(&self) -> String {
        format!("{} · {}", self.doc_name, self.locator.target_label())
    }

    pub fn detail_line(&self) -> String {
        if let SelectionLocator::CodeLines { language, .. } = &self.locator {
            return format!(
                "[CODE:{language}] {} · {}",
                self.doc_name,
                self.locator.target_label()
            );
        }
        let rev = self
            .base_revision
            .map_or_else(|| "rev -".to_owned(), format_rev);
        format!(
            "[{}] {} · {} ({rev})",
            self.kind.short_tag(),
            self.doc_name,
            self.locator.target_label()
        )
    }

    pub fn compose_prompt(&self, instruction: &str) -> String {
        let path_str = self
            .file_path
            .as_ref()
            .map_or_else(|| self.doc_name.clone(), |p| p.display().to_string());
        if let SelectionLocator::CodeLines { language, .. } = &self.locator {
            return format!(
                "请针对以下工作区源码文件给出修改方案或直接执行修改：\n\
                 - 文件路径: {path_str}\n\
                 - 语言类型: {language}\n\
                 - 目标范围: {}\n\
                 - 代码快照:\n```{}\n{}\n```\n\n\
                 用户要求：\n{instruction}",
                self.locator.target_label(),
                language.to_ascii_lowercase(),
                if self.excerpt.is_empty() {
                    "(空)"
                } else {
                    &self.excerpt
                },
            );
        }
        let extra = match &self.locator {
            SelectionLocator::WordParagraph { style, .. } => format!("段落样式: {style}"),
            SelectionLocator::ExcelRange {
                raw_input,
                computed_value,
                ..
            } => format!("单元格输入/公式: `{raw_input}`，当前计算结果: `{computed_value}`"),
            SelectionLocator::PptObject {
                shape_kind,
                bounds_cm,
                ..
            } => match bounds_cm {
                Some((x, y, w, h)) => {
                    format!(
                        "对象类型: {shape_kind}，位置尺寸: x={x:.2}cm, y={y:.2}cm, w={w:.2}cm, h={h:.2}cm"
                    )
                }
                None => format!("对象类型: {shape_kind}"),
            },
            SelectionLocator::CodeLines { language, .. } => format!("语言: {language}"),
        };
        format!(
            "请针对以下 Office 文档选区给出修改方案或执行修改（如果连接了 `kantor` MCP 工具，可直接读取或更新画布中的文档标签 #{tab_id}）：\n\
             - 文件路径: {path_str}\n\
             - 文档类型: {}\n\
             - 目标位置: {}\n\
             - {extra}\n\
             - 当前内容快照: {}\n\n\
             用户修改要求：\n{instruction}",
            self.kind.label(),
            self.locator.target_label(),
            if self.excerpt.is_empty() {
                "(空)"
            } else {
                &self.excerpt
            },
            tab_id = self.tab_id,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderAvailability {
    Available {
        version: String,
        command: Option<PathBuf>,
    },
    Missing,
    Failed {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderAuthState {
    LoggedIn {
        summary: String,
        plan: Option<String>,
    },
    LoggedOut {
        source: String,
    },
    Unknown {
        note: String,
    },
    NotChecked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderModelOption {
    pub id: String,
    pub label: String,
    pub is_default: bool,
    pub effort_levels: Vec<String>,
}

impl ProviderModelOption {
    // Kar validates against the explicitly selected model, or the catalogue default.
    fn levels_for<'a>(models: &'a [Self], selected: Option<&str>) -> &'a [String] {
        models
            .iter()
            .find(|entry| match selected {
                Some(id) => entry.id == id,
                None => entry.is_default,
            })
            .map_or(&[], |entry| entry.effort_levels.as_slice())
    }

    pub fn from_kar_entry(entry: &kar::forms::ModelEntry) -> Option<Self> {
        if entry.disabled.is_some() || entry.id.trim().is_empty() {
            return None;
        }
        let label = ["displayName", "name", "label", "title"]
            .iter()
            .find_map(|key| entry.metadata.get(*key).and_then(|v| v.as_str()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&entry.id)
            .to_owned();
        let effort_levels = entry
            .effort_levels
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect();
        Some(Self {
            id: entry.id.clone(),
            label,
            is_default: entry.default,
            effort_levels,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderStatusInfo {
    pub id: String,
    pub protocol: String,
    pub availability: ProviderAvailability,
    pub auth: ProviderAuthState,
    pub mcp_office_supported: bool,
    pub steering_supported: bool,
    pub goals_supported: bool,
    pub can_install: bool,
    pub install_lines: Vec<(String, String)>,
    pub models: Vec<ProviderModelOption>,
    pub quota_summary: Option<String>,
}

/// Whether an agent can take a new chat right now, and if not, which step is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderReadiness {
    /// Installed and signed in.
    Ready,
    /// Installed; the CLI could not tell us whether an account is signed in.
    AccountUnverified,
    /// Installed, signed out.
    NeedsLogin,
    /// The program is there but did not pass its start-up check.
    Broken,
    /// Not installed on this machine.
    NotInstalled,
}

impl ProviderStatusInfo {
    pub fn effort_levels(&self, model: Option<&str>) -> &[String] {
        if runtime_supports_effort(&self.id) {
            ProviderModelOption::levels_for(&self.models, model)
        } else {
            &[]
        }
    }

    pub fn readiness(&self) -> ProviderReadiness {
        match (&self.availability, &self.auth) {
            (ProviderAvailability::Available { .. }, ProviderAuthState::LoggedIn { .. }) => {
                ProviderReadiness::Ready
            }
            (ProviderAvailability::Available { .. }, ProviderAuthState::LoggedOut { .. }) => {
                ProviderReadiness::NeedsLogin
            }
            (ProviderAvailability::Available { .. }, _) => ProviderReadiness::AccountUnverified,
            (ProviderAvailability::Missing, _) => ProviderReadiness::NotInstalled,
            (ProviderAvailability::Failed { .. }, _) => ProviderReadiness::Broken,
        }
    }

    /// A chat can be started with this agent.
    pub fn is_ready(&self) -> bool {
        matches!(
            self.readiness(),
            ProviderReadiness::Ready | ProviderReadiness::AccountUnverified
        )
    }

    pub fn display_name(&self) -> String {
        runtime_display_name(&self.id)
    }

    /// One or two words for menus and lists.
    pub fn status_label(&self) -> &'static str {
        match self.readiness() {
            ProviderReadiness::Ready => "可用",
            ProviderReadiness::AccountUnverified => "已安装",
            ProviderReadiness::NeedsLogin => "未登录",
            ProviderReadiness::Broken => "无法启动",
            ProviderReadiness::NotInstalled => "未安装",
        }
    }

    pub fn version_label(&self) -> Option<String> {
        match &self.availability {
            ProviderAvailability::Available { version, .. } => short_version(version),
            _ => None,
        }
    }

    /// The signed-in account or sign-in method, when the CLI reported one.
    pub fn account_label(&self) -> Option<String> {
        match &self.auth {
            ProviderAuthState::LoggedIn { summary, plan } => {
                let mut parts = Vec::new();
                if !summary.trim().is_empty() {
                    parts.push(summary.trim().to_owned());
                }
                if let Some(p) = plan.as_deref().filter(|p| !p.trim().is_empty()) {
                    parts.push(p.trim().to_owned());
                }
                (!parts.is_empty()).then(|| parts.join(" · "))
            }
            _ => None,
        }
    }

    /// Human-readable capability & account line for the Agents sheet.
    pub fn ready_detail_line(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(v) = self.version_label() {
            parts.push(format!("v{v}"));
        }
        if let Some(acc) = self.account_label() {
            parts.push(acc);
        } else if self.readiness() == ProviderReadiness::AccountUnverified {
            parts.push("已安装，登录状态由程序自行管理".into());
        }
        if let Some(ref quota) = self.quota_summary {
            parts.push(quota.clone());
        }
        if !self.models.is_empty() {
            parts.push(format!("{} 个模型", self.models.len()));
        }
        if self.mcp_office_supported {
            parts.push("支持 Office 直连".into());
        }
        parts.join(" · ")
    }

    pub fn default_model(&self) -> Option<&ProviderModelOption> {
        self.models
            .iter()
            .find(|m| m.is_default)
            .or_else(|| self.models.first())
    }

    /// The terminal command that resolves what is missing, if one exists.
    pub fn fix_command(&self) -> Option<String> {
        match self.readiness() {
            ProviderReadiness::NeedsLogin => Some(format!("kar account {} login", self.id)),
            ProviderReadiness::NotInstalled => Some(format!("kar install {}", self.id)),
            _ => None,
        }
    }

    /// One sentence on why a chat cannot start, or `None` when it can.
    pub fn diagnostic_reason(&self) -> Option<String> {
        let name = self.display_name();
        match (&self.availability, self.readiness()) {
            (_, ProviderReadiness::NotInstalled) => Some(format!(
                "这台电脑上还没有安装 {name}。在终端运行 `kar install {}` 安装后即可使用。",
                self.id
            )),
            (ProviderAvailability::Failed { reason }, _) => {
                Some(format!("{name} 已安装，但启动检查没有通过：{reason}"))
            }
            (_, ProviderReadiness::NeedsLogin) => Some(format!(
                "{name} 还没有登录账号。在终端运行 `kar account {} login` 登录后即可使用。",
                self.id
            )),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChatListEntry {
    pub info: ChatInfo,
    pub title: String,
    pub turn_count: usize,
    pub last_status: Option<TurnStatus>,
    pub last_activity_ms: Option<u64>,
}

/// Format relative thread age in T3 Code's compact tabular style (`now`, `24m`, `2h`, `1d`).
pub fn format_relative_age(last_ms: Option<u64>, now_ms: u64) -> String {
    let Some(ts) = last_ms.filter(|&v| v > 0) else {
        return "now".into();
    };
    let diff_secs = now_ms.saturating_sub(ts) / 1000;
    if diff_secs < 60 {
        "now".into()
    } else if diff_secs < 3600 {
        format!("{}m", diff_secs / 60)
    } else if diff_secs < 86_400 {
        format!("{}h", diff_secs / 3600)
    } else if diff_secs < 86_400 * 30 {
        format!("{}d", diff_secs / 86_400)
    } else {
        format!("{}w", diff_secs / (86_400 * 7))
    }
}

impl ChatListEntry {
    pub fn from_info_and_turns(info: ChatInfo, turns: &[ChatTurn]) -> Self {
        Self::from_info_and_turns_with_title(info, turns, None)
    }

    pub fn from_info_and_turns_with_title(
        info: ChatInfo,
        turns: &[ChatTurn],
        custom_title: Option<&str>,
    ) -> Self {
        let title = custom_title
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| info.settings.name.clone())
            .or_else(|| {
                turns.iter().find_map(|t| {
                    t.items.iter().find_map(|item| match &item.item {
                        Item::Message {
                            role: Role::User,
                            text,
                            ..
                        } => {
                            let own = strip_selection_envelope(text);
                            let first_line = own.lines().next().unwrap_or(own).trim();
                            if first_line.is_empty() {
                                None
                            } else {
                                let clipped: String = first_line.chars().take(60).collect();
                                Some(if first_line.chars().count() > 60 {
                                    format!("{clipped}…")
                                } else {
                                    clipped
                                })
                            }
                        }
                        _ => None,
                    })
                })
            })
            .unwrap_or_else(|| "新对话".to_owned());
        let last_status = turns.last().map(|t| t.status);
        let last_activity_ms = turns
            .last()
            .map(|t| t.finished_ms.unwrap_or(t.started_ms));
        Self {
            info,
            title,
            turn_count: turns.len(),
            last_status,
            last_activity_ms,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatAccessMode {
    NoneSelected,
    WriteController {
        chat_id: Uuid,
    },
    LocalReadOnlySubscriber {
        chat_id: Uuid,
        reason: String,
    },
    ArchivedReadOnly {
        chat_id: Uuid,
        reason: String,
    },
    HttpReadOnly {
        url: String,
        chat_id: Option<Uuid>,
    },
    RemoteController {
        url: String,
        host_name: String,
        peer_id: Uuid,
        token: String,
        role: crate::remote_link::PeerRole,
        chat_id: Option<Uuid>,
    },
    DemoShowcase,
}

impl ChatAccessMode {
    pub fn is_writable(&self) -> bool {
        match self {
            Self::WriteController { .. } | Self::DemoShowcase => true,
            Self::RemoteController { role, .. } => role.can_mutate(),
            _ => false,
        }
    }

    pub fn is_http_read_only(&self) -> bool {
        matches!(self, Self::HttpReadOnly { .. })
            || matches!(
                self,
                Self::RemoteController {
                    role: crate::remote_link::PeerRole::Viewer,
                    ..
                }
            )
    }

    pub fn is_remote_paired(&self) -> bool {
        matches!(self, Self::RemoteController { .. })
    }

    pub fn selected_chat_id(&self) -> Option<Uuid> {
        match self {
            Self::WriteController { chat_id }
            | Self::LocalReadOnlySubscriber { chat_id, .. }
            | Self::ArchivedReadOnly { chat_id, .. } => Some(*chat_id),
            Self::HttpReadOnly { chat_id, .. } | Self::RemoteController { chat_id, .. } => *chat_id,
            Self::NoneSelected | Self::DemoShowcase => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeedConnectionState {
    Idle,
    Connecting {
        endpoint: String,
    },
    Connected {
        cursor: Option<String>,
        duplicates_ignored: u64,
        gaps_recovered: u64,
    },
    Reconnecting {
        last_cursor: Option<String>,
        reason: String,
        attempt: u32,
    },
    Disconnected {
        last_cursor: Option<String>,
        reason: String,
    },
}

/// Format compact token / context readout for menus and subtle indicators.
pub fn format_tokens_compact(n: i64) -> String {
    let n = n.max(0);
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Captures a user prompt attempt so retries reuse the exact same request UUID.
#[derive(Clone, Debug)]
pub struct PendingPromptAttempt {
    pub chat_id: Option<Uuid>,
    pub request_id: Uuid,
    pub raw_user_text: String,
    pub sent_payload: String,
    pub target: Option<SelectionTarget>,
    pub attachments: Vec<PathBuf>,
    pub attempts: u32,
    pub in_flight: bool,
    pub last_error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effort_choices_follow_the_selected_or_default_model() {
        let models = [
            ProviderModelOption {
                id: "fast".into(),
                label: "Fast".into(),
                is_default: false,
                effort_levels: vec![],
            },
            ProviderModelOption {
                id: "deep".into(),
                label: "Deep".into(),
                is_default: true,
                effort_levels: vec!["high".into(), "xhigh".into()],
            },
        ];
        assert_eq!(
            ProviderModelOption::levels_for(&models, None),
            ["high", "xhigh"]
        );
        assert!(ProviderModelOption::levels_for(&models, Some("fast")).is_empty());
        assert!(ProviderModelOption::levels_for(&models, Some("unlisted")).is_empty());
        assert!(ProviderModelOption::levels_for(&models[..1], None).is_empty());
    }

    #[test]
    fn official_kar_brands_provide_svg_data_uris_for_all_registered_runtimes() {
        let runtimes = kar::runtimes();
        assert!(!runtimes.is_empty());
        for rt in runtimes {
            let id = &rt.id;
            let light = runtime_brand_svg(id, false).unwrap_or_default();
            let dark = runtime_brand_svg(id, true).unwrap_or_default();
            assert!(
                light.starts_with("data:image/svg+xml,"),
                "expected light SVG data URI for runtime {id}"
            );
            assert!(
                dark.starts_with("data:image/svg+xml,"),
                "expected dark SVG data URI for runtime {id}"
            );
        }
    }

    #[test]
    fn archived_read_only_mode_preserves_selected_chat_id_without_writability() {
        let cid = Uuid::new_v4();
        let mode = ChatAccessMode::ArchivedReadOnly {
            chat_id: cid,
            reason: "原工作区目录已不存在".into(),
        };
        assert!(!mode.is_writable());
        assert!(!mode.is_http_read_only());
        assert_eq!(mode.selected_chat_id(), Some(cid));
    }
}
