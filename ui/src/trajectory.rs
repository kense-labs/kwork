//! Turn-aware event ledger projected from raw Session events.

use crate::state::{ChatEntry, EntryRole};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

mod folds;
mod output;
#[cfg(all(test, not(target_family = "wasm")))]
use folds::visible;
#[cfg(target_family = "wasm")]
pub(crate) use folds::{
    FoldDomain, Visible, double_click_folds, fold_states, toggle_global_fold, visible,
};
pub(crate) mod hierarchy;
pub(crate) mod program;
mod prompts;
pub(crate) mod timeline;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    System,
    User,
    Context,
    Compacted,
    Message,
    Tool,
    Subtool,
}

/// Inspector tabs follow ui-trajectory/TrajectoryTable.tsx. A request boundary
/// has its own tabs even though its ledger record uses the Message kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum DetailTab {
    #[default]
    Overview,
    Rendered,
    Raw,
    Source,
    Input,
    Output,
    Schema,
    Timing,
    SystemPrompt,
    Diff,
    Tools,
    Options,
    Usage,
}

impl DetailTab {
    pub fn label(self, record: &Record) -> &'static str {
        match self {
            Self::Overview => "概述",
            Self::Rendered => "预览",
            Self::Raw if record.kind == Kind::Compacted => "原始输出",
            Self::Raw => "原始内容",
            Self::Source => "来源",
            Self::Input if record.program().is_some() => "源码",
            Self::Input => "参数",
            Self::Output => "结果",
            Self::Schema => "Schema",
            Self::Timing => "计时",
            Self::SystemPrompt => "系统提示词",
            Self::Diff => "差异",
            Self::Tools => "工具",
            Self::Options => "选项",
            Self::Usage => "用量",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DetailData {
    pub source: Option<Value>,
    pub blocks: Vec<Value>,
    pub output_error: Option<Value>,
    pub schema: Option<Value>,
    pub result_seq: Option<u64>,
    pub prompt: Option<prompts::Snapshot>,
    pub previous_prompt: Option<prompts::Snapshot>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Usage {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
    pub thinking: Option<u64>,
}

impl Usage {
    fn from(value: &Value) -> Option<Self> {
        (!value.is_null()).then(|| Self {
            input: value["inputTokens"].as_u64(),
            output: value["outputTokens"].as_u64(),
            cache_read: value["cacheReadTokens"].as_u64(),
            cache_write: value["cacheWriteTokens"].as_u64(),
            thinking: value["reasoningTokens"].as_u64(),
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Timing {
    pub started_at: Option<f64>,
    pub first_token_at: Option<f64>,
    pub completed_at: Option<f64>,
}

impl Timing {
    pub fn ttft_ms(self) -> Option<f64> {
        Some(self.first_token_at? - self.started_at?).filter(|value| *value >= 0.)
    }

    pub fn decoding_ms(self) -> Option<f64> {
        Some(self.completed_at? - self.first_token_at?).filter(|value| *value >= 0.)
    }

    pub fn duration_ms(self) -> Option<f64> {
        Some(self.completed_at? - self.started_at?).filter(|value| *value >= 0.)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub key: String,
    pub source_seq: Option<f64>,
    pub turn: u64,
    pub step: u64,
    pub index: usize,
    pub kind: Kind,
    pub name: String,
    pub input: String,
    pub output: String,
    pub thinking: String,
    pub status: String,
    pub timing: Timing,
    pub usage: Option<Usage>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub options: Option<Value>,
    pub system_prompt: Option<String>,
    pub tool_schemas: Option<String>,
    pub parent_call: Option<String>,
    pub entry: ChatEntry,
    pub detail: DetailData,
}

impl Record {
    pub fn detail_tabs(&self, request: bool) -> Vec<DetailTab> {
        use DetailTab::*;
        if request {
            let mut tabs = vec![Overview];
            if self.options.is_some() {
                tabs.push(Options);
            }
            tabs.extend([Usage, Timing]);
            return tabs;
        }
        match self.kind {
            Kind::System if self.detail.previous_prompt.is_some() => {
                vec![Diff, SystemPrompt, Tools]
            }
            Kind::System if self.tool_schemas.is_some() => vec![SystemPrompt, Tools],
            Kind::System => vec![SystemPrompt],
            Kind::Compacted => vec![Overview, Raw],
            Kind::User | Kind::Context | Kind::Message => {
                let mut tabs = vec![Overview, Rendered, Raw];
                if self.kind != Kind::Message && self.detail.source.is_some() {
                    tabs.push(Source);
                }
                tabs
            }
            Kind::Tool | Kind::Subtool => {
                let mut tabs = vec![Overview];
                if !self.entry.input.is_empty() && self.entry.input != "null" {
                    tabs.push(Input);
                }
                if !self.output.is_empty() || self.program().is_some() {
                    tabs.push(Output);
                }
                tabs.extend([Schema, Timing]);
                tabs
            }
        }
    }

    pub fn preferred_detail_tab(&self, request: bool, history: &[DetailTab]) -> DetailTab {
        if request {
            return DetailTab::Overview;
        }
        let tabs = self.detail_tabs(false);
        history
            .iter()
            .rev()
            .find(|tab| tabs.contains(tab))
            .copied()
            .unwrap_or(tabs[0])
    }

    pub fn label(&self) -> &'static str {
        match self.kind {
            Kind::System => "系统",
            Kind::User => "用户",
            Kind::Context => "上下文",
            Kind::Compacted => "已压缩",
            Kind::Message => "助手",
            Kind::Tool => "工具",
            Kind::Subtool => "子工具",
        }
    }

    pub fn failed(&self) -> bool {
        self.status == "failed" || self.status == "error" || self.status.starts_with("ASK_")
    }

    pub fn pending(&self) -> bool {
        matches!(
            self.status.as_str(),
            "pending" | "in_progress" | "preparing" | "approval" | "running"
        )
    }

    pub fn preview(&self) -> String {
        match self.kind {
            Kind::Tool | Kind::Subtool => {
                let args = self.tool_arguments_preview();
                if args.is_empty() {
                    self.name.clone()
                } else {
                    format!("{} {args}", self.name)
                }
            }
            Kind::Message => preview(if self.output.is_empty() {
                &self.thinking
            } else {
                &self.output
            }),
            _ => preview(&self.output),
        }
    }

    pub fn tool_arguments_preview(&self) -> String {
        if let Some(program) = self.program() {
            program
                .description
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        } else if self.entry.input == "null" {
            String::new()
        } else {
            preview(&self.entry.input)
        }
    }

    pub fn result_preview(&self) -> Option<String> {
        if !matches!(self.kind, Kind::Tool | Kind::Subtool) || self.pending() {
            return None;
        }
        if !self.detail.blocks.is_empty() || self.detail.output_error.is_some() {
            return Some(output::summary(
                &self.detail.blocks,
                self.failed(),
                self.detail.output_error.as_ref(),
            ));
        }
        Some(if self.output.is_empty() {
            "无输出".to_owned()
        } else {
            preview(&self.output)
        })
    }
}

/// The first visible row is bounded; details keep all retained text.
#[cfg(target_family = "wasm")]
pub fn preview(text: &str) -> String {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(raw_module = "../trajectory-json.js")]
    extern "C" {
        #[wasm_bindgen(js_name = trajectoryRecordPreview)]
        fn upstream_preview(text: &str) -> String;
    }
    upstream_preview(text)
}

// Native builds only exercise projection fixtures; the shipped WASM UI uses
// the upstream GFM parser above (including local images and CJK emphasis).
#[cfg(not(target_family = "wasm"))]
pub fn preview(text: &str) -> String {
    let source: String = text.chars().take(2048).collect();
    let mut compact = String::with_capacity(source.len());
    let mut whitespace = false;
    let mut fences = false;
    for character in source.chars() {
        if character == '`' {
            fences = !fences;
            continue;
        }
        if !fences && matches!(character, '#' | '*' | '>' | '[' | ']') {
            continue;
        }
        if character.is_whitespace() {
            whitespace = !compact.is_empty();
        } else {
            if whitespace {
                compact.push(' ');
            }
            whitespace = false;
            compact.push(character);
        }
    }
    let compact: String = compact.trim().chars().take(512).collect();
    if text.chars().count() > 2048 || compact.chars().count() == 512 {
        format!("{compact}…")
    } else {
        compact
    }
}

pub fn format_duration(milliseconds: Option<f64>) -> String {
    let Some(milliseconds) = milliseconds.filter(|value| value.is_finite() && *value >= 0.) else {
        return "—".to_owned();
    };
    if milliseconds < 1000. {
        return format!("{} 毫秒", milliseconds.round() as u64);
    }
    format!("{:.2} 秒", milliseconds / 1000.)
}

#[derive(Default)]
pub(crate) struct State {
    pub events: Vec<Value>,
    pub requests: Vec<Record>,
    pub records: Vec<Record>,
    pub folded: HashSet<String>,
}

impl State {
    /// Older pages prepend, while a current tail merges with already received live events.
    pub fn apply_page(&mut self, events: &[Value], prepend: bool) {
        if prepend {
            let mut incoming = events.to_vec();
            incoming.append(&mut self.events);
            self.events = incoming;
        } else {
            self.events.extend_from_slice(events);
        }
        self.events.sort_by(|left, right| {
            let seq = |event: &Value| event["seq"].as_f64().unwrap_or(f64::MAX);
            seq(left).total_cmp(&seq(right))
        });
        let mut seen = HashSet::new();
        self.events.retain(|event| {
            if let Some(seq) = event["seq"].as_f64() {
                seen.insert(seq.to_bits())
            } else {
                true
            }
        });
        self.rebuild();
    }

    #[cfg(test)]
    pub fn observe(&mut self, entry: &ChatEntry) {
        if self.records.iter().any(|record| record.key == entry.key) {
            return;
        }
        let turn = 1 + self
            .records
            .iter()
            .filter(|record| record.kind == Kind::User)
            .count() as u64;
        let mut record = from_entry(entry, turn, 1);
        if self
            .records
            .last()
            .is_some_and(|last| last.kind == Kind::User)
        {
            record.turn = self.records.last().unwrap().turn;
        }
        self.records.push(record);
        self.reindex();
    }

    fn rebuild(&mut self) {
        let previous: HashMap<String, Record> = self
            .records
            .drain(..)
            .chain(self.requests.drain(..))
            .map(|record| (record.key.clone(), record))
            .collect();
        let mut projected = project(&self.events);
        projected.folded = std::mem::take(&mut self.folded);
        projected.reindex();
        for record in projected.requests.iter_mut().chain(&mut projected.records) {
            if let Some(old) = previous.get(&record.key)
                && record.pending()
                && !old.pending()
            {
                record.status = old.status.clone();
                record.output.clone_from(&old.output);
                record.entry.text.clone_from(&old.entry.text);
                record.detail = old.detail.clone();
                record.timing.completed_at = old.timing.completed_at;
            }
        }
        *self = projected;
    }

    fn reindex(&mut self) {
        for (index, record) in self.requests.iter_mut().enumerate() {
            record.index = index + 1;
        }
        for (index, record) in self.records.iter_mut().enumerate() {
            record.index = index + 1;
        }
    }
}

fn number(event: &Value, key: &str) -> Option<f64> {
    event[key].as_f64().or_else(|| event["data"][key].as_f64())
}

fn seq(event: &Value) -> Option<f64> {
    event["seq"].as_f64()
}

fn coordinate(event: &Value, key: &str) -> u64 {
    event["data"][key].as_u64().unwrap_or(0)
}

fn event_text(blocks: Option<&Vec<Value>>) -> String {
    blocks
        .into_iter()
        .flatten()
        .filter_map(|block| match block["type"].as_str()? {
            "text" | "reasoning" => block["text"].as_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_token_time(stream: &Value) -> Option<f64> {
    stream.as_array()?.iter().find_map(|record| {
        let kind = record["type"].as_str()?;
        match kind {
            "text-chunks" | "reasoning-chunks" | "tool-call-chunks" => record["time0"].as_f64(),
            "chunk"
                if matches!(
                    record["chunk"]["type"].as_str(),
                    Some("text-delta" | "reasoning-delta" | "tool-call-delta")
                ) =>
            {
                record["time"].as_f64()
            }
            _ => None,
        }
    })
}

fn json_object(block: &Value, added: bool) -> Value {
    serde_json::json!({ "added": added, "name": block["toolName"].clone() })
}

fn raw_record(event: &Value, kind: Kind, text: String, turn: u64, step: u64) -> Record {
    let name = match kind {
        Kind::System => "系统提示词",
        Kind::Context => "上下文",
        Kind::Compacted => "上下文压缩",
        _ => "消息",
    };
    let id = event["data"]["id"].as_str().map(str::to_owned);
    let key = id
        .clone()
        .unwrap_or_else(|| format!("event-{}", seq(event).unwrap_or(0.)));
    let entry = ChatEntry::local(
        match kind {
            Kind::User => EntryRole::User,
            Kind::Context | Kind::System => EntryRole::Context,
            _ => EntryRole::System,
        },
        &key,
        &text,
    );
    Record {
        key,
        source_seq: seq(event),
        turn,
        step,
        index: 0,
        kind,
        name: name.to_owned(),
        input: preview(&text),
        output: text,
        thinking: String::new(),
        status: "completed".to_owned(),
        timing: Timing {
            started_at: number(event, "time"),
            completed_at: matches!(kind, Kind::User | Kind::Context | Kind::System)
                .then(|| number(event, "time"))
                .flatten(),
            ..Timing::default()
        },
        usage: None,
        provider: None,
        model: None,
        options: None,
        system_prompt: None,
        tool_schemas: None,
        parent_call: None,
        entry,
        detail: DetailData::default(),
    }
}

fn tool_event(event: &Value) -> Option<Record> {
    let data = &event["data"];
    let id = data["callId"]
        .as_str()
        .or_else(|| {
            data.pointer("/message/source/callId")
                .and_then(Value::as_str)
        })
        .or_else(|| data["message"]["toolCallId"].as_str())
        .or_else(|| data["subCallId"].as_str())?;
    let name = data["name"].as_str().unwrap_or("tool");
    let parent = data["parentCallId"].as_str().map(str::to_owned);
    let kind = if parent.is_some() {
        Kind::Subtool
    } else {
        Kind::Tool
    };
    let input = data["arguments"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| data["arguments"].to_string());
    let result = if data["message"].is_object() {
        &data["message"]
    } else {
        data
    };
    let running = event["type"] == "tool/call" || event["type"] == "tool/ptc-dispatch-start";
    let status = if running {
        "in_progress".to_owned()
    } else {
        crate::tool_calls::result_status(data, result).to_owned()
    };
    let blocks = result["content"].as_array().cloned().unwrap_or_default();
    let error = data
        .get("error")
        .or_else(|| result.get("error"))
        .filter(|value| value.is_object())
        .cloned();
    let text = if running {
        String::new()
    } else {
        output::detail(&blocks, result["isError"] == true, error.as_ref())
    };
    let mut entry = ChatEntry::local(EntryRole::Tool, format!("tool-{id}"), &text);
    entry.external_id = Some(id.to_owned());
    entry.title = name.to_owned();
    entry.input.clone_from(&input);
    entry.status.clone_from(&status);
    entry.tool = Some(crate::tool_calls::tool_call(
        name,
        parent.as_deref(),
        data["rootCallId"].as_str(),
    ));
    let command = crate::tool_cards::shell_command(name, &input);
    let input = command
        .map(|command| command.command)
        .unwrap_or_else(|| preview(&input));
    Some(Record {
        key: format!("tool-{id}"),
        source_seq: seq(event),
        turn: coordinate(event, "turn"),
        step: coordinate(event, "step"),
        index: 0,
        kind,
        name: name.to_owned(),
        input,
        output: text,
        thinking: String::new(),
        status,
        timing: Timing {
            started_at: number(event, "time"),
            ..Timing::default()
        },
        usage: None,
        provider: None,
        model: None,
        options: None,
        system_prompt: None,
        tool_schemas: None,
        parent_call: parent,
        entry,
        detail: DetailData {
            blocks,
            output_error: error,
            result_seq: (!running).then(|| event["seq"].as_u64()).flatten(),
            ..DetailData::default()
        },
    })
}

fn project(events: &[Value]) -> State {
    let mut records = Vec::new();
    let mut requests = Vec::new();
    let mut turn = 0;
    let mut step = 0;
    let mut step_start: Option<f64> = None;
    let mut prompts = prompts::Prompts::default();
    let mut tools: HashMap<String, usize> = HashMap::new();
    let mut schemas: HashMap<String, Value> = HashMap::new();
    let mut tool_starts: HashMap<String, f64> = HashMap::new();
    let mut closed: HashMap<(u64, u64), f64> = HashMap::new();
    for event in events {
        if event["type"] == "step/end" {
            closed.insert(
                (coordinate(event, "turn"), coordinate(event, "step")),
                number(event, "time").unwrap_or(0.),
            );
        }
    }
    let mut parents: HashMap<String, String> = HashMap::new();
    for event in events {
        if let Some(id) = event["data"]["subCallId"].as_str()
            && let Some(parent) = event["data"]["parentCallId"].as_str()
            && id != parent
        {
            parents
                .entry(id.to_owned())
                .or_insert_with(|| parent.to_owned());
        }
    }
    for event in events {
        prompts.surface(event, turn, step);
        match event["type"].as_str().unwrap_or("") {
            "turn/start" => {
                turn = coordinate(event, "turn");
                step = 0;
            }
            "step/start" => {
                step = coordinate(event, "step");
                step_start = number(event, "time");
                let mut record = raw_record(event, Kind::Message, String::new(), turn, step);
                record.key = format!("request-{turn}-{step}-{}", seq(event).unwrap_or(0.));
                record.name = "模型请求".into();
                record.status = "running".into();
                record.timing.started_at = step_start;
                if let Some(prompt) = &prompts.latest {
                    prompt.apply(&mut record);
                }
                requests.push(record);
            }
            "step/end" => {
                if let Some(request) = requests
                    .iter_mut()
                    .rev()
                    .find(|record| record.turn == turn && record.step == step)
                {
                    if request.pending() {
                        request.status = if request.output.is_empty() {
                            "failed"
                        } else {
                            "completed"
                        }
                        .into();
                    }
                    request.timing.completed_at = number(event, "time");
                }
            }
            "request/header" => {
                prompts.header(event, turn, step);
                if let Some(prompt) = &prompts.latest {
                    schemas = prompt
                        .tools
                        .iter()
                        .filter_map(|schema| {
                            schema["name"]
                                .as_str()
                                .map(|name| (name.to_owned(), schema.clone()))
                        })
                        .collect();
                    if let Some(request) = requests
                        .last_mut()
                        .filter(|request| request.status == "running")
                    {
                        prompt.apply(request);
                    }
                }
            }
            "request/context" => {
                if let Some(request) = requests
                    .iter_mut()
                    .rev()
                    .find(|record| record.turn == turn && record.step == step)
                {
                    request.options = event["data"]
                        .get("config")
                        .cloned()
                        .or_else(|| Some(event["data"].clone()));
                }
            }
            "user/message" => {
                if event["data"]
                    .pointer("/source/kind")
                    .and_then(Value::as_str)
                    == Some("compact-checkpoint")
                {
                    continue;
                }
                let text = event_text(event["data"]["content"].as_array());
                let kind = if event["data"]
                    .pointer("/source/kind")
                    .and_then(Value::as_str)
                    .is_none_or(|kind| kind == "user")
                {
                    Kind::User
                } else {
                    Kind::Context
                };
                let mut record = raw_record(event, kind, text, turn, step);
                record.detail.source = event["data"].get("source").cloned();
                record.detail.blocks = event["data"]["content"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                records.push(record);
            }
            "developer/message" => {
                let message = &event["data"]["message"];
                let text = event_text(message["content"].as_array());
                let changes: Vec<Value> = message["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|block| {
                        let added = match block["type"].as_str()? {
                            "tool-addition" => true,
                            "tool-removal" => false,
                            _ => return None,
                        };
                        Some(json_object(block, added))
                    })
                    .collect();
                let text = if changes.is_empty() {
                    text
                } else {
                    crate::state::tool_changes(&Value::Array(changes))
                        .into_iter()
                        .map(|change| {
                            format!(
                                "{}{}",
                                if change.added {
                                    "已添加工具："
                                } else {
                                    "已移除工具："
                                },
                                change.name
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                let mut record = raw_record(event, Kind::Context, text, turn, step);
                record.name = "工具已更新".into();
                record.detail.source = message.get("source").cloned();
                record.detail.blocks = message["content"].as_array().cloned().unwrap_or_default();
                records.push(record);
            }
            "assistant/attempt" => {
                if let Some(request) = requests
                    .iter_mut()
                    .rev()
                    .find(|record| record.turn == turn && record.step == step)
                {
                    let at = first_token_time(&event["data"]["stream"]);
                    request.timing.first_token_at = request.timing.first_token_at.or(at);
                }
            }
            "assistant/message" => {
                let message = &event["data"]["message"];
                let content = message["content"].as_array();
                let text = content
                    .into_iter()
                    .flatten()
                    .filter(|block| block["type"] == "text")
                    .map(|block| block["text"].as_str().unwrap_or(""))
                    .collect::<Vec<_>>()
                    .join("\n");
                let thinking = content
                    .into_iter()
                    .flatten()
                    .filter(|block| block["type"] == "reasoning")
                    .map(|block| block["text"].as_str().unwrap_or(""))
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut record = raw_record(event, Kind::Message, text.clone(), turn, step);
                record.key = format!("assistant-{}", message["id"].as_str().unwrap_or("message"));
                record.name = "助手消息".into();
                record.input = preview(&record.output);
                record.thinking = thinking.clone();
                record.detail.source = message.get("source").cloned();
                record.detail.blocks = content.cloned().unwrap_or_default();
                record.usage = Usage::from(&event["data"]["usage"]);
                record.provider = message
                    .pointer("/source/provider")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                record.model = message
                    .pointer("/source/model")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                record.timing.started_at = step_start;
                record.timing.first_token_at = first_token_time(&event["data"]["stream"]);
                record.timing.completed_at = number(event, "time");
                record.entry.role = EntryRole::Assistant;
                if event["data"]["interrupted"] == true {
                    record.status = "failed".into();
                }
                if let Some(request) = requests
                    .iter_mut()
                    .rev()
                    .find(|request| request.turn == turn && request.step == step)
                {
                    request.usage = record.usage;
                    if record.provider.is_some() {
                        request.provider.clone_from(&record.provider);
                    }
                    if record.model.is_some() {
                        request.model.clone_from(&record.model);
                    }
                    request.status = record.status.clone();
                    request.timing.completed_at = number(event, "time");
                    request.timing.first_token_at = request
                        .timing
                        .first_token_at
                        .or(record.timing.first_token_at);
                    request.input = if text.is_empty() && !thinking.is_empty() {
                        "仅思考".into()
                    } else {
                        preview(&text)
                    };
                    request.output = record.output.clone();
                    request.thinking.clone_from(&record.thinking);
                }
                records.push(record);
            }
            "llm/retry" => {
                let failure = &event["data"]["failure"];
                let message = failure["message"].as_str().unwrap_or("模型请求重试");
                let message = if failure["code"].as_str() == Some("AUTH") {
                    "模型认证失败"
                } else {
                    message
                };
                let mut record = raw_record(event, Kind::System, message.to_owned(), turn, step);
                record.name = "模型重试".into();
                record.status = "failed".into();
                if let Some(request) = requests
                    .iter_mut()
                    .rev()
                    .find(|request| request.turn == turn && request.step == step)
                {
                    request.status = "failed".into();
                }
                records.push(record);
            }
            "tool/call" | "tool/result" | "tool/ptc-dispatch-start" | "tool/ptc-dispatch" => {
                let Some(mut record) = tool_event(event) else {
                    continue;
                };
                let id = record.entry.external_id.clone().unwrap();
                record.parent_call = parents.get(&id).cloned();
                record.turn = record.turn.max(turn);
                record.step = record.step.max(step);
                record.detail.schema = schemas.get(&record.name).cloned();
                if record.pending()
                    && let Some(ended) = closed.get(&(record.turn, record.step)).copied()
                {
                    record.status = "failed".into();
                    record.entry.status = "failed".into();
                    record.timing.completed_at = Some(ended);
                    if record.output.is_empty() {
                        record.output = "调用已中断".into();
                        record.entry.text = record.output.clone();
                    }
                }
                if event["type"] == "tool/call" || event["type"] == "tool/ptc-dispatch-start" {
                    tool_starts.insert(id.clone(), number(event, "time").unwrap_or(0.));
                } else {
                    record.timing.started_at = tool_starts.get(&id).copied();
                    record.timing.completed_at = number(event, "time");
                }
                if let Some(previous) = tools.get(&id).and_then(|index| records.get(*index)) {
                    if record.name == "tool" {
                        record.name.clone_from(&previous.name);
                    }
                    if record.input == "null" || record.input.is_empty() {
                        record.input.clone_from(&previous.input);
                    }
                    if record.entry.input.is_empty() || record.entry.input == "null" {
                        record.entry.input.clone_from(&previous.entry.input);
                    }
                    record.turn = if record.turn == 0 {
                        previous.turn
                    } else {
                        record.turn
                    };
                    record.step = if record.step == 0 {
                        previous.step
                    } else {
                        record.step
                    };
                    record.entry.tool = previous.entry.tool.clone().or(record.entry.tool);
                    record.entry.title.clone_from(&record.name);
                    if record.detail.schema.is_none() {
                        record.detail.schema.clone_from(&previous.detail.schema);
                    }
                }
                if let Some(index) = tools.get(&id).copied() {
                    if !record.pending() || records[index].pending() {
                        records[index] = record;
                    }
                } else {
                    tools.insert(id, records.len());
                    records.push(record);
                }
            }
            "compaction/start" => {
                let mut record =
                    raw_record(event, Kind::Compacted, "正在压缩上下文…".into(), turn, step);
                record.status = "running".into();
                records.push(record);
            }
            "compaction/end" | "compaction/summary" => {
                let mut record = raw_record(
                    event,
                    Kind::Compacted,
                    event["data"]["summary"]
                        .as_str()
                        .unwrap_or("上下文已压缩")
                        .to_owned(),
                    turn,
                    step,
                );
                if event["data"]["error"].is_object() || !event["data"]["error"].is_null() {
                    record.status = "failed".into();
                }
                records.push(record);
            }
            _ => {}
        }
    }
    records.extend(prompts.records);
    records.sort_by(|a, b| {
        a.source_seq
            .unwrap_or(f64::MAX)
            .total_cmp(&b.source_seq.unwrap_or(f64::MAX))
    });
    for request in &mut requests {
        if request.pending()
            && let Some(ended) = closed.get(&(request.turn, request.step)).copied()
        {
            request.status = "failed".into();
            request.timing.completed_at = Some(ended);
        }
    }
    let mut projected = State {
        events: events.to_vec(),
        requests,
        records,
        folded: HashSet::new(),
    };
    projected.reindex();
    projected
}

#[cfg(test)]
fn from_entry(entry: &ChatEntry, turn: u64, step: u64) -> Record {
    let kind = match entry.role {
        EntryRole::User => Kind::User,
        EntryRole::Assistant | EntryRole::Thought => Kind::Message,
        EntryRole::Tool => {
            if entry.is_subcall() {
                Kind::Subtool
            } else {
                Kind::Tool
            }
        }
        EntryRole::Compaction => Kind::Compacted,
        EntryRole::System | EntryRole::SystemPrompt => Kind::System,
        _ => Kind::Context,
    };
    let input = if matches!(kind, Kind::Tool | Kind::Subtool) {
        crate::tool_cards::shell_command(entry.tool_name(), &entry.input)
            .map(|command| command.command)
            .unwrap_or_else(|| preview(&entry.input))
    } else if entry.role == EntryRole::Thought {
        String::new()
    } else {
        preview(&entry.text)
    };
    Record {
        key: entry.key.clone(),
        source_seq: None,
        turn,
        step,
        index: 0,
        kind,
        name: if entry.role == EntryRole::Thought {
            "思考".into()
        } else {
            entry.tool_name().to_owned()
        },
        input,
        output: entry.text.clone(),
        thinking: if entry.role == EntryRole::Thought {
            entry.text.clone()
        } else {
            String::new()
        },
        status: entry.status.clone(),
        timing: Timing {
            started_at: entry.at_ms,
            ..Timing::default()
        },
        usage: None,
        provider: None,
        model: None,
        options: None,
        system_prompt: None,
        tool_schemas: None,
        parent_call: None,
        entry: entry.clone(),
        detail: DetailData::default(),
    }
}

/// Matching includes folded records and copyable full input/output.
pub fn matches(record: &Record, query: &str) -> bool {
    query.split_whitespace().all(|term| {
        let term = term.to_lowercase();
        record.name.to_lowercase().contains(&term)
            || record.label().to_lowercase().contains(&term)
            || record.input.to_lowercase().contains(&term)
            || record.output.to_lowercase().contains(&term)
            || record.thinking.to_lowercase().contains(&term)
            || format!("#{}", record.index).contains(&term)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn events() -> Vec<Value> {
        vec![
            json!({"seq":1,"time":1000,"type":"turn/start","data":{"turn":1}}),
            json!({"seq":2,"time":1100,"type":"user/message","data":{"id":"u","source":{"kind":"user"},"content":[{"type":"text","text":"Inspect this"}]}}),
            json!({"seq":3,"time":1200,"type":"step/start","data":{"turn":1,"step":1}}),
            json!({"seq":4,"time":1600,"type":"assistant/message","data":{"turn":1,"step":1,"message":{"id":"a","source":{"provider":"deepseek","model":"v4"},"content":[{"type":"reasoning","text":"Think"},{"type":"text","text":"Done"}]},"usage":{"inputTokens":20,"outputTokens":5}}}),
            json!({"seq":5,"time":1700,"type":"tool/call","data":{"turn":1,"step":1,"callId":"c","name":"bash","arguments":"{\"command\":\"pwd\"}"}}),
            json!({"seq":6,"time":1900,"type":"tool/result","data":{"turn":1,"step":1,"message":{"source":{"callId":"c"},"content":[{"type":"text","text":"/tmp"}]}}}),
            json!({"seq":7,"time":2000,"type":"step/end","data":{"turn":1,"step":1}}),
        ]
    }

    #[test]
    fn durable_records_keep_usage_timing_and_tool_pairs() {
        let state = project(&events());
        assert_eq!(state.records.len(), 3);
        assert_eq!(state.records[2].input, "pwd");
        assert_eq!(state.records[1].preview(), "Done");
        assert_eq!(state.records[2].preview(), "bash {\"command\":\"pwd\"}");
        assert_eq!(state.records[2].result_preview().as_deref(), Some("/tmp"));
        assert_eq!(state.records[2].timing.duration_ms(), Some(200.));
        assert_eq!(state.requests[0].usage.unwrap().input, Some(20));
        assert_eq!(state.requests[0].timing.duration_ms(), Some(800.));
    }

    #[test]
    fn inspector_retains_full_reasoning_arguments_and_message_source() {
        let mut events = events();
        let reasoning = "保留 **完整** 思考\n".repeat(600);
        events[3]["data"]["message"]["content"][0]["text"] = json!(reasoning);
        let state = project(&events);
        let assistant = &state.records[1];
        assert_eq!(assistant.thinking, reasoning);
        assert_eq!(assistant.detail.blocks[0]["text"], reasoning);
        assert_eq!(assistant.detail.source.as_ref().unwrap()["model"], "v4");
        assert_eq!(
            assistant
                .detail
                .source
                .as_ref()
                .unwrap()
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["provider", "model"],
            "the JSON inspector preserves the original field order",
        );
        assert!(assistant.preview().chars().count() < 520);
        assert_eq!(state.records[2].entry.input, r#"{"command":"pwd"}"#);
        // Durable message content is not the live, unrendered tool return value.
        assert_eq!(state.records[2].detail.result_seq, Some(6));
    }

    #[test]
    fn inspector_restores_compatible_tab_but_request_opens_overview() {
        use DetailTab::*;
        let state = project(&events());
        let history = [Raw, Input];
        assert_eq!(state.records[1].preferred_detail_tab(false, &history), Raw);
        assert_eq!(
            state.records[2].preferred_detail_tab(false, &history),
            Input
        );
        assert_eq!(
            state.requests[0].preferred_detail_tab(true, &[Timing]),
            Overview
        );
        assert_eq!(
            state.requests[0].detail_tabs(true),
            vec![Overview, Usage, Timing]
        );
        let mut system = state.records[0].clone();
        system.kind = Kind::System;
        assert_eq!(system.preferred_detail_tab(false, &history), SystemPrompt);
        assert_eq!(system.detail_tabs(false), vec![SystemPrompt]);
    }

    #[test]
    fn tool_completion_preserves_call_schema_and_header_model() {
        let mut events = events();
        events[3]["data"]["message"]
            .as_object_mut()
            .unwrap()
            .remove("source");
        events.insert(3, json!({"seq":3.5,"time":1250,"type":"request/header","data":{"header":{
            "config":{"provider":"configured-provider","model":"configured-model"},
            "tools":[{"name":"bash","description":"Run shell commands","parameters":{"type":"object"}}]
        }}}));
        let state = project(&events);
        let tool = state
            .records
            .iter()
            .find(|record| record.kind == Kind::Tool)
            .unwrap();
        assert_eq!(tool.detail.schema.as_ref().unwrap()["name"], "bash");
        assert_eq!(
            tool.detail.schema.as_ref().unwrap()["parameters"]["type"],
            "object"
        );
        assert_eq!(
            state.requests[0].provider.as_deref(),
            Some("configured-provider")
        );
        assert_eq!(state.requests[0].model.as_deref(), Some("configured-model"));
        assert!(
            state.requests[0]
                .detail_tabs(true)
                .contains(&DetailTab::Options)
        );
    }

    #[test]
    fn header_retry_and_interruption_use_their_recorded_boundaries() {
        let state = project(&[
            json!({"seq":1,"time":1000,"type":"step/start","data":{"turn":1,"step":1}}),
            json!({"seq":2,"time":1100,"type":"request/header","data":{"reason":"initial","header":{"config":{"provider":"deepseek","model":"v4","temperature":0.2},"tools":[{"name":"read_file"}]}}}),
            json!({"seq":3,"time":1200,"type":"assistant/attempt","data":{"turn":1,"step":1,"stream":[{"type":"text-chunks","time0":1400}]}}),
            json!({"seq":4,"time":1500,"type":"llm/retry","data":{"turn":1,"step":1,"retry":1,"failure":{"code":"TRANSPORT","message":"temporary"}}}),
            json!({"seq":5,"time":1600,"type":"tool/call","data":{"turn":1,"step":1,"callId":"root","name":"run_code","arguments":"{}"}}),
            json!({"seq":6,"time":1700,"type":"tool/ptc-dispatch-start","data":{"rootCallId":"root","parentCallId":"root","subCallId":"child","name":"read_file","arguments":{"path":"README.md"}}}),
            json!({"seq":7,"time":1800,"type":"tool/ptc-dispatch","data":{"rootCallId":"root","parentCallId":"root","subCallId":"child","name":"read_file","arguments":{"path":"README.md"},"content":[{"type":"text","text":"contents"}]}}),
            json!({"seq":8,"time":1900,"type":"step/end","data":{"turn":1,"step":1}}),
        ]);
        let request = &state.requests[0];
        assert_eq!(request.provider.as_deref(), Some("deepseek"));
        assert_eq!(request.timing.ttft_ms(), Some(400.));
        assert_eq!(request.status, "failed");
        assert!(state.records.iter().any(|record| record.name == "模型重试"));
        let child = state
            .records
            .iter()
            .find(|record| record.key == "tool-child")
            .unwrap();
        assert!(child.entry.is_subcall());
        assert_eq!(child.parent_call.as_deref(), Some("root"));
        assert_eq!(child.output, "contents");
        assert_eq!(child.timing.duration_ms(), Some(100.));
        assert!(
            state
                .records
                .iter()
                .find(|record| record.key == "tool-root")
                .unwrap()
                .failed()
        );
        let folded = HashSet::from(["tool:tool-root".to_owned()]);
        let rows = visible(&state.records, &state.requests, &folded, "");
        assert!(!rows.iter().any(|row| row.record.key == "tool-child"));
        assert!(rows.iter().any(|row| row.record.key == "tool-root"));
        let rows = visible(&state.records, &state.requests, &folded, "README.md");
        assert!(rows.iter().any(|row| row.record.key == "tool-child"));
    }

    #[test]
    fn older_pages_merge_without_duplicates_and_search_folds() {
        let mut state = State::default();
        state.apply_page(&events()[2..], false);
        state.apply_page(&events()[..2], true);
        assert_eq!(state.events.len(), 7);
        let folded = HashSet::from(["turn:1".to_owned()]);
        let rows = visible(&state.records, &state.requests, &folded, "pwd");
        assert!(rows.iter().any(|row| row.record.name == "bash"));
        assert_eq!(format_duration(None), "—");
    }
}
