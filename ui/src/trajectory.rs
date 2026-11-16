//! Turn-aware event ledger projected from raw Session events.

use crate::state::{ChatEntry, EntryRole};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub thinking: u64,
}

impl Usage {
    fn from(value: &Value) -> Option<Self> {
        (!value.is_null()).then(|| Self {
            input: value["inputTokens"].as_u64().unwrap_or(0),
            output: value["outputTokens"].as_u64().unwrap_or(0),
            cache_read: value["cacheReadTokens"].as_u64().unwrap_or(0),
            cache_write: value["cacheWriteTokens"].as_u64().unwrap_or(0),
            thinking: value["reasoningTokens"].as_u64().unwrap_or(0),
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
    pub at_ms: Option<f64>,
    pub timing: Timing,
    pub usage: Option<Usage>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub options: Option<Value>,
    pub system_prompt: Option<String>,
    pub tool_schemas: Option<String>,
    pub parent_call: Option<String>,
    pub has_children: bool,
    pub entry: ChatEntry,
}

impl Record {
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
        preview(&match self.kind {
            Kind::Tool | Kind::Subtool if !self.output.is_empty() => {
                format!("{} → {}", self.input, preview(&self.output))
            }
            Kind::Message => [&self.input, &self.thinking]
                .into_iter()
                .filter(|text| !text.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" · "),
            _ => self.input.clone(),
        })
    }
}

/// The first visible row is bounded; details keep all retained text.
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
            }
        }
        *self = projected;
    }

    fn reindex(&mut self) {
        derive_turn_folds(&mut self.records);
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
        at_ms: number(event, "time"),
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
        parent_call: None,
        has_children: false,
        entry,
    }
}

fn tool_event(event: &Value, children: &HashMap<String, Vec<String>>) -> Option<Record> {
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
    let text = crate::tool_calls::result_text(result);
    let running = event["type"] == "tool/call" || event["type"] == "tool/ptc-dispatch-start";
    let status = if running {
        "in_progress".to_owned()
    } else {
        crate::tool_calls::result_status(data, result).to_owned()
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
        at_ms: number(event, "time"),
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
        has_children: children.contains_key(id),
        entry,
    })
}

fn project(events: &[Value]) -> State {
    let mut records = Vec::new();
    let mut requests = Vec::new();
    let mut turn = 0;
    let mut step = 0;
    let mut step_start: Option<f64> = None;
    let mut tools: HashMap<String, usize> = HashMap::new();
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
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    for event in events {
        if let Some(id) = event["data"]["subCallId"].as_str()
            && let Some(parent) = event["data"]["parentCallId"].as_str()
            && id != parent
            && parents.insert(id.to_owned(), parent.to_owned()).is_none()
        {
            children
                .entry(parent.to_owned())
                .or_default()
                .push(id.to_owned());
        }
    }
    for event in events {
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
                let data = &event["data"]["header"];
                let config = &data["config"];
                let provider = config["provider"].as_str().map(str::to_owned);
                let model = config["model"].as_str().map(str::to_owned);
                if let Some(request) = requests
                    .last_mut()
                    .filter(|request| request.status == "running")
                {
                    request.provider = provider;
                    request.model = model;
                    request.options = Some(config.clone());
                    request.system_prompt = data["system"].as_str().map(str::to_owned);
                    request.tool_schemas = data.get("tools").map(Value::to_string);
                } else {
                    let mut record = raw_record(event, Kind::System, String::new(), turn, step);
                    record.key = format!("header-{}", seq(event).unwrap_or(0.));
                    record.name = "请求配置".into();
                    record.provider = provider;
                    record.model = model;
                    record.options = Some(config.clone());
                    record.system_prompt = data["system"].as_str().map(str::to_owned);
                    record.tool_schemas = data.get("tools").map(Value::to_string);
                    records.push(record);
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
                records.push(raw_record(event, kind, text, turn, step));
            }
            "developer/message" | "system/message" => {
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
                if event["type"] == "system/message" {
                    record.kind = Kind::System;
                    record.name = "系统提示词".into();
                    record.entry.role = EntryRole::SystemPrompt;
                } else {
                    record.name = "工具已更新".into();
                }
                record.system_prompt = message["text"].as_str().map(str::to_owned);
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
                record.thinking = preview(&thinking);
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
                    request.provider.clone_from(&record.provider);
                    request.model.clone_from(&record.model);
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
                let Some(mut record) = tool_event(event, &children) else {
                    continue;
                };
                let id = record.entry.external_id.clone().unwrap();
                record.parent_call = parents.get(&id).cloned();
                record.turn = record.turn.max(turn);
                record.step = record.step.max(step);
                record.has_children = children.contains_key(&id);
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
    for record in &mut records {
        record.has_children = record
            .entry
            .external_id
            .as_ref()
            .is_some_and(|id| children.contains_key(id));
    }
    derive_turn_folds(&mut records);
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
            preview(&entry.text)
        } else {
            String::new()
        },
        status: entry.status.clone(),
        at_ms: entry.at_ms,
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
        has_children: entry.role == EntryRole::User,
        entry: entry.clone(),
    }
}

/// Normalized record spans for the overview and its inclusive range filter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TimelineSpan {
    pub start: f32,
    pub end: f32,
}

pub(crate) fn time_spans(
    state: &State,
    actual_time: bool,
    actual_duration: bool,
) -> HashMap<String, TimelineSpan> {
    let records: Vec<&Record> = state
        .requests
        .iter()
        .chain(&state.records)
        .filter(|record| record.at_ms.is_some())
        .collect();
    let mut timed_records = records.clone();
    timed_records.sort_by(|left, right| {
        let at = |record: &Record| record.timing.started_at.or(record.at_ms).unwrap_or(0.);
        at(left).total_cmp(&at(right))
    });
    let min = records
        .iter()
        .filter_map(|record| record.timing.started_at.or(record.at_ms))
        .fold(f64::INFINITY, f64::min);
    let max = records
        .iter()
        .filter_map(|record| record.timing.completed_at.or(record.at_ms))
        .fold(f64::NEG_INFINITY, f64::max);
    let span = (max - min).max(1.);
    let total: f64 = records
        .iter()
        .filter_map(|record| record.timing.duration_ms())
        .sum();
    let mut before = 0.;
    timed_records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            let (start, end) = if actual_time {
                let at = record.at_ms.unwrap_or(min);
                let start = record.timing.started_at.unwrap_or(at);
                let end = record.timing.completed_at.unwrap_or(at);
                ((start - min) / span, (end - min) / span)
            } else if actual_duration && record.timing.duration_ms().is_some() {
                let start = before / total.max(1.);
                before += record.timing.duration_ms().unwrap_or(0.);
                (start, before / total.max(1.))
            } else {
                (
                    index as f64 / records.len().max(1) as f64,
                    (index + 1) as f64 / records.len().max(1) as f64,
                )
            };
            (
                record.key.clone(),
                TimelineSpan {
                    start: start.clamp(0., 1.) as f32,
                    end: end.clamp(start, 1.) as f32,
                },
            )
        })
        .collect()
}

/// Order and clamp a normalized timeline selection.
pub(crate) fn ordered_range((start, end): (f32, f32)) -> (f32, f32) {
    (start.min(end).clamp(0., 1.), start.max(end).clamp(0., 1.))
}

/// Expand a click-sized selection to one record in the active projection.
pub(crate) fn minimum_range(center: f32, records: usize) -> (f32, f32) {
    let length = 1. / records.max(1) as f32;
    let start = (center - length / 2.).clamp(0., 1. - length);
    (start, start + length)
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

/// Fold the ledger into rows, retaining matched descendants of folded records.
#[derive(Clone, Debug)]
pub(crate) struct Visible<'a> {
    pub record: &'a Record,
    #[cfg(test)]
    pub collapsed: bool,
    pub summary: Option<String>,
}

fn derive_turn_folds(records: &mut [Record]) {
    let mut first = HashMap::new();
    let mut counts: HashMap<u64, usize> = HashMap::new();
    for (index, record) in records.iter().enumerate() {
        if record.turn > 0 && !matches!(record.kind, Kind::System | Kind::Tool | Kind::Subtool) {
            first.entry(record.turn).or_insert(index);
            *counts.entry(record.turn).or_default() += 1;
        }
    }
    for (index, record) in records.iter_mut().enumerate() {
        if first.get(&record.turn) == Some(&index) {
            record.has_children = counts.get(&record.turn).is_some_and(|count| *count > 1);
        }
    }
}

pub(crate) fn first_turn_content(records: &[Record]) -> HashMap<u64, &str> {
    records
        .iter()
        .filter(|record| {
            record.turn > 0 && !matches!(record.kind, Kind::System | Kind::Tool | Kind::Subtool)
        })
        .fold(HashMap::new(), |mut first, record| {
            first.entry(record.turn).or_insert(record.key.as_str());
            first
        })
}

pub(crate) fn global_fold(record: &Record, first_in_turn: bool) -> &'static str {
    match record.kind {
        Kind::Tool => "$call",
        Kind::Subtool => "$subcall",
        Kind::User | Kind::Context | Kind::Message | Kind::Compacted if first_in_turn => "$turn",
        _ => "",
    }
}

pub fn visible<'a>(
    records: &'a [Record],
    folded: &HashSet<String>,
    query: &str,
) -> Vec<Visible<'a>> {
    let first_turn_content = first_turn_content(records);
    let folded_turns = folded_turns(records, folded, &first_turn_content);
    let global_fold = |record: &Record| {
        global_fold(
            record,
            first_turn_content.get(&record.turn) == Some(&record.key.as_str()),
        )
    };

    let calls: HashMap<&str, &Record> = records
        .iter()
        .filter_map(|record| record.entry.external_id.as_deref().map(|id| (id, record)))
        .collect();
    let searching = !query.trim().is_empty();
    let rows = records
        .iter()
        .filter_map(|record| {
            let global = global_fold(record);
            let collapsed = record.has_children
                && !global.is_empty()
                && (folded.contains(global) || folded.contains(&record.key));
            let is_folded = |record: &Record| {
                let global = global_fold(record);
                record.has_children
                    && !global.is_empty()
                    && (folded.contains(global) || folded.contains(&record.key))
            };
            let mut parent = record.parent_call.as_deref();
            let mut seen = HashSet::new();
            let mut hidden_by_call = false;
            while let Some(id) = parent {
                if !seen.insert(id) {
                    break;
                }
                let Some(ancestor) = calls.get(id) else { break };
                if is_folded(ancestor) {
                    hidden_by_call = true;
                    break;
                }
                parent = ancestor.parent_call.as_deref();
            }
            let turn_hidden = folded_turns.contains(&record.turn)
                && first_turn_content
                    .get(&record.turn)
                    .is_some_and(|key| *key != record.key);
            let hidden = hidden_by_call || turn_hidden;
            let matched = matches(record, query);
            if hidden && !(searching && (matched || collapsed)) {
                return None;
            }
            if searching && !matched && !collapsed {
                return None;
            }
            let summary = (record.has_children
                && !global.is_empty()
                && (folded.contains(global) || folded.contains(&record.key)))
            .then(|| {
                let descendants = records
                    .iter()
                    .filter(|other| {
                        other.index > record.index
                            && match global_fold(record) {
                                "$turn" => other.turn == record.turn && other.kind != Kind::System,
                                "$call" | "$subcall" => {
                                    let mut parent = other.parent_call.as_deref();
                                    let mut seen = HashSet::new();
                                    while let Some(id) = parent {
                                        if !seen.insert(id) {
                                            break;
                                        }
                                        if Some(id) == record.entry.external_id.as_deref() {
                                            return true;
                                        }
                                        parent = calls
                                            .get(id)
                                            .and_then(|record| record.parent_call.as_deref());
                                    }
                                    false
                                }
                                _ => false,
                            }
                    })
                    .count();
                format!("已收起{} · {descendants} 条记录", record.label())
            });
            Some(Visible {
                record,
                #[cfg(test)]
                collapsed: false,
                summary,
            })
        })
        .collect::<Vec<_>>();
    let mut folded = Vec::with_capacity(rows.len() + 1);
    for mut row in rows {
        let summary = row.summary.take();
        let record = row.record;
        folded.push(row);
        if let Some(summary) = summary {
            folded.push(Visible {
                record,
                #[cfg(test)]
                collapsed: true,
                summary: Some(summary),
            });
        }
    }
    folded
}

pub(crate) fn folded_turns(
    records: &[Record],
    folded: &HashSet<String>,
    first: &HashMap<u64, &str>,
) -> HashSet<u64> {
    records
        .iter()
        .filter_map(|record| {
            (record.has_children
                && first.get(&record.turn) == Some(&record.key.as_str())
                && global_fold(record, true) == "$turn"
                && (folded.contains("$turn") || folded.contains(&record.key)))
            .then_some(record.turn)
        })
        .collect()
}

/// Global expansion clears only its own domain; per-record expansion materializes exceptions.
pub(crate) fn toggle_global_fold(folded: &mut HashSet<String>, records: &[Record], domain: &str) {
    let first_in_turn = first_turn_content(records);
    let keys: Vec<String> = records
        .iter()
        .filter(|record| {
            record.has_children
                && global_fold(
                    record,
                    first_in_turn.get(&record.turn) == Some(&record.key.as_str()),
                ) == domain
        })
        .map(|record| record.key.clone())
        .collect();
    if folded.remove(domain) {
        for key in keys {
            folded.remove(&key);
        }
        return;
    }
    if keys.iter().all(|key| folded.contains(key)) {
        for key in keys {
            folded.remove(&key);
        }
    } else {
        folded.insert(domain.to_owned());
        folded.extend(keys);
    }
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
        assert_eq!(state.records[2].preview(), "pwd → /tmp");
        assert_eq!(state.records[2].timing.duration_ms(), Some(200.));
        assert_eq!(state.requests[0].usage.unwrap().input, 20);
        assert_eq!(state.requests[0].timing.duration_ms(), Some(800.));
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
        let folded = HashSet::from(["tool-root".to_owned()]);
        let rows = visible(&state.records, &folded, "");
        assert!(!rows.iter().any(|row| row.record.key == "tool-child"));
        assert!(rows.iter().any(|row| row.record.key == "tool-root"));
        let rows = visible(&state.records, &folded, "README.md");
        assert!(rows.iter().any(|row| row.record.key == "tool-child"));
    }

    #[test]
    fn sequence_and_recorded_time_share_inclusive_span_filtering() {
        let state = project(&events());
        let sequence = time_spans(&state, false, false);
        assert_eq!(
            sequence["u"],
            TimelineSpan {
                start: 0.,
                end: 0.25
            }
        );
        let timed = time_spans(&state, true, false);
        let tool = timed["tool-c"];
        assert!((tool.start - 2. / 3.).abs() < 0.0001);
        assert!((tool.end - 8. / 9.).abs() < 0.0001);
        let selected = |span: TimelineSpan| span.start <= 0.8 && span.end >= 0.7;
        assert!(selected(tool));
        assert!(!selected(TimelineSpan {
            start: 0.,
            end: 0.5
        }));
    }

    #[test]
    fn click_ranges_expand_to_one_record_without_exceeding_the_domain() {
        assert_eq!(ordered_range((0.8, 0.2)), (0.2, 0.8));
        let range = minimum_range(0.5, 10);
        assert!((range.0 - 0.45).abs() < 0.0001);
        assert!((range.1 - 0.55).abs() < 0.0001);
        assert_eq!(minimum_range(1., 10), (0.9, 1.));
        assert_eq!(minimum_range(0.5, 0), (0., 1.));
    }

    #[test]
    fn turn_folds_keep_the_first_content_record_and_show_its_summary() {
        let state = project(&events());
        let rows = visible(&state.records, &HashSet::from(["$turn".to_owned()]), "");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].record.kind, Kind::User);
        assert!(!rows[0].collapsed);
        assert_eq!(rows[0].summary, None);
        assert!(rows[1].collapsed);
        assert_eq!(rows[1].summary.as_deref(), Some("已收起用户 · 2 条记录"));
        assert!(state.records[0].has_children);

        let rows = visible(&state.records, &HashSet::from(["u".to_owned()]), "");
        let first = first_turn_content(&state.records);
        assert!(
            folded_turns(&state.records, &HashSet::from(["u".to_owned()]), &first,).contains(&1)
        );
        assert!(folded_turns(&state.records, &HashSet::new(), &first).is_empty());
        assert!(
            !folded_turns(&state.records, &HashSet::from(["$turn".to_owned()]), &first,)
                .contains(&2)
        );
        assert_eq!(rows.len(), 2);
        assert!(rows[1].collapsed);
        let rows = visible(&state.records, &HashSet::new(), "");
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| !row.collapsed));
    }

    #[test]
    fn turns_without_a_user_record_fold_their_first_assistant_content() {
        let state = project(&[
            json!({"seq":1,"time":1000,"type":"turn/start","data":{"turn":9}}),
            json!({"seq":2,"time":1100,"type":"step/start","data":{"turn":9,"step":1}}),
            json!({"seq":3,"time":1200,"type":"assistant/message","data":{"turn":9,"step":1,"message":{"id":"first","content":[{"type":"text","text":"first"}]}}}),
            json!({"seq":4,"time":1300,"type":"assistant/message","data":{"turn":9,"step":1,"message":{"id":"second","content":[{"type":"text","text":"second"}]}}}),
        ]);
        assert!(state.records[0].has_children);
        let rows = visible(&state.records, &HashSet::from(["$turn".to_owned()]), "");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].record.key, "assistant-first");
        assert_eq!(rows[1].summary.as_deref(), Some("已收起助手 · 1 条记录"));
        let rows = visible(
            &state.records,
            &HashSet::from(["assistant-first".to_owned()]),
            "second",
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].record.key, "assistant-first");
        assert!(rows[1].collapsed);
        assert_eq!(rows[2].record.key, "assistant-second");
    }

    #[test]
    fn call_folds_ignore_leaf_calls_and_retain_real_subcalls() {
        let state = project(&[
            json!({"seq":1,"time":1000,"type":"turn/start","data":{"turn":1}}),
            json!({"seq":2,"time":1100,"type":"tool/call","data":{"turn":1,"step":1,"callId":"leaf","name":"bash","arguments":"{\"command\":\"pwd\"}"}}),
            json!({"seq":3,"time":1200,"type":"tool/result","data":{"turn":1,"step":1,"message":{"source":{"callId":"leaf"},"content":[{"type":"text","text":"/tmp"}]}}}),
            json!({"seq":4,"time":1300,"type":"tool/call","data":{"turn":1,"step":1,"callId":"root","name":"run_code","arguments":"{}"}}),
            json!({"seq":5,"time":1400,"type":"tool/ptc-dispatch-start","data":{"rootCallId":"root","parentCallId":"root","subCallId":"child","name":"read_file","arguments":{"path":"README.md"}}}),
            json!({"seq":6,"time":1500,"type":"tool/ptc-dispatch","data":{"rootCallId":"root","parentCallId":"root","subCallId":"child","name":"read_file","arguments":{"path":"README.md"},"content":[{"type":"text","text":"contents"}]}}),
        ]);
        let rows = visible(&state.records, &HashSet::from(["$call".to_owned()]), "");
        let leaf = rows
            .iter()
            .find(|row| row.record.key == "tool-leaf")
            .unwrap();
        assert!(!leaf.record.has_children);
        assert!(!leaf.collapsed);
        assert_eq!(leaf.summary, None);
        let root = rows
            .iter()
            .find(|row| row.record.key == "tool-root" && !row.collapsed)
            .unwrap();
        assert!(root.record.has_children);
        assert!(!root.collapsed);
        assert_eq!(
            rows.iter()
                .find(|row| row.record.key == "tool-root" && row.collapsed)
                .and_then(|row| row.summary.as_deref()),
            Some("已收起工具 · 1 条记录")
        );
        assert_eq!(leaf.summary, None);
        assert!(!rows.iter().any(|row| row.record.key == "tool-child"));

        let rows = visible(&state.records, &HashSet::from(["tool-leaf".to_owned()]), "");
        assert!(rows.iter().any(|row| row.record.key == "tool-child"));
        assert!(
            !rows
                .iter()
                .find(|row| row.record.key == "tool-root")
                .unwrap()
                .collapsed
        );
        let rows = visible(&state.records, &HashSet::from(["$subcall".to_owned()]), "");
        let child = rows
            .iter()
            .find(|row| row.record.key == "tool-child")
            .unwrap();
        assert!(!child.collapsed);
        assert_eq!(child.summary, None);

        let mut folded = HashSet::new();
        toggle_global_fold(&mut folded, &state.records, "$call");
        assert!(folded.contains("$call"));
        assert_eq!(visible(&state.records, &folded, "")[0].summary, None);
        toggle_global_fold(&mut folded, &state.records, "$call");
        assert_eq!(folded, HashSet::new());
        toggle_global_fold(&mut folded, &state.records, "$call");
        assert!(folded.contains("$call"));
        folded.insert("tool-root".to_owned());
        toggle_global_fold(&mut folded, &state.records, "$call");
        assert_eq!(folded, HashSet::new());
    }

    #[test]
    fn nested_global_expansion_keeps_hidden_individual_folds() {
        let state = project(&[
            json!({"seq":1,"time":1000,"type":"turn/start","data":{"turn":1}}),
            json!({"seq":2,"time":1100,"type":"user/message","data":{"id":"u","source":{"kind":"user"},"content":[{"type":"text","text":"nested"}]}}),
            json!({"seq":3,"time":1200,"type":"tool/call","data":{"turn":1,"step":1,"callId":"root","name":"run_code","arguments":"{}"}}),
            json!({"seq":4,"time":1300,"type":"tool/ptc-dispatch-start","data":{"rootCallId":"root","parentCallId":"root","subCallId":"child","name":"read_file","arguments":{"path":"README.md"}}}),
            json!({"seq":5,"time":1400,"type":"tool/ptc-dispatch","data":{"rootCallId":"root","parentCallId":"root","subCallId":"child","name":"read_file","arguments":{"path":"README.md"},"content":[{"type":"text","text":"contents"}]}}),
            json!({"seq":6,"time":1500,"type":"tool/ptc-dispatch-start","data":{"rootCallId":"root","parentCallId":"child","subCallId":"grandchild","name":"grep","arguments":{"query":"needle"}}}),
            json!({"seq":7,"time":1600,"type":"tool/ptc-dispatch","data":{"rootCallId":"root","parentCallId":"child","subCallId":"grandchild","name":"grep","arguments":{"query":"needle"},"content":[{"type":"text","text":"found"}]}}),
        ]);
        let mut folded = HashSet::from(["$subcall".to_owned()]);
        toggle_global_fold(&mut folded, &state.records, "$call");
        let rows = visible(&state.records, &folded, "");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].summary.as_deref(), Some("已收起工具 · 2 条记录"));
        toggle_global_fold(&mut folded, &state.records, "$call");
        assert_eq!(folded, HashSet::from(["$subcall".to_owned()]));
        let rows = visible(&state.records, &folded, "");
        assert!(
            rows.iter().any(|row| row.record.key == "tool-child"),
            "rows: {rows:#?}"
        );
        assert_eq!(rows[3].summary.as_deref(), Some("已收起子工具 · 1 条记录"));
        assert!(!rows.iter().any(|row| row.record.key == "tool-grandchild"));
        let rows = visible(&state.records, &folded, "needle");
        assert_eq!(rows.len(), 3, "rows: {rows:#?}");
        assert_eq!(rows[0].record.key, "tool-child");
        assert_eq!(rows[1].summary.as_deref(), Some("已收起子工具 · 1 条记录"));
        assert_eq!(rows[2].record.key, "tool-grandchild");
        let rows = visible(&state.records, &folded, "");
        assert_eq!(rows.len(), 4);
        folded.insert("tool-root".to_owned());
        let rows = visible(&state.records, &folded, "needle");
        assert_eq!(rows.len(), 5, "rows: {rows:#?}");
        assert_eq!(rows[0].record.key, "tool-root");
        assert_eq!(rows[1].summary.as_deref(), Some("已收起工具 · 2 条记录"));
        assert_eq!(rows[3].summary.as_deref(), Some("已收起子工具 · 1 条记录"));
        assert_eq!(rows[4].record.key, "tool-grandchild");
        let rows = visible(&state.records, &folded, "");
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn older_pages_merge_without_duplicates_and_search_folds() {
        let mut state = State::default();
        state.apply_page(&events()[2..], false);
        state.apply_page(&events()[..2], true);
        assert_eq!(state.events.len(), 7);
        let folded = HashSet::from(["$turn".to_owned()]);
        let rows = visible(&state.records, &folded, "pwd");
        assert!(rows.iter().any(|row| row.record.name == "bash"));
        assert!(state.records[0].has_children);
        assert_eq!(format_duration(None), "—");
    }
}
