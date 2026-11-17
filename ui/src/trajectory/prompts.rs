//! Prompt history follows upstream system-prompt.ts and request-inspection.ts.
//! Surface replacements inherit endpoint positions; request snapshots stay historical.
use super::*;

#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct Snapshot {
    pub config: Value,
    pub system: String,
    pub tools: Vec<Value>,
}

impl Snapshot {
    pub fn apply(&self, record: &mut Record) {
        record.provider = self.config["provider"].as_str().map(str::to_owned);
        record.model = self.config["model"].as_str().map(str::to_owned);
        record.options = Some(self.config.clone());
        record.system_prompt = Some(self.system.clone());
        record.tool_schemas = Some(Value::Array(self.tools.clone()).to_string());
        record.detail.prompt = Some(self.clone());
    }
}

#[derive(Clone)]
struct Node {
    position: f64,
    event: Value,
    text: String,
    update: bool,
}

#[derive(Default)]
pub(super) struct Prompts {
    first_seq: Option<f64>,
    uncertain: bool,
    nodes: Vec<Node>,
    replacements: Vec<(f64, f64)>,
    effective: Option<Node>,
    pub latest: Option<Snapshot>,
    pub records: Vec<Record>,
}

impl Prompts {
    fn emit(
        &mut self,
        event: &Value,
        turn: u64,
        step: u64,
        label: &str,
        previous: Option<Snapshot>,
    ) {
        let mut record = raw_record(event, Kind::System, label.into(), turn, step);
        record.key = format!("prompt-{}", seq(event).unwrap_or(0.));
        record.name = label.into();
        record.entry.role = EntryRole::SystemPrompt;
        record.detail.previous_prompt = previous;
        if let Some(snapshot) = &self.latest {
            snapshot.apply(&mut record);
        }
        if let Some(existing) = self.records.iter_mut().find(|item| item.key == record.key) {
            *existing = record;
        } else {
            self.records.push(record);
        }
    }

    pub fn surface(&mut self, event: &Value, turn: u64, step: u64) {
        let system = event["type"] == "system/message";
        let replacement = event.get("surfaceOp").filter(|op| op.is_object());
        if !system && replacement.is_none() {
            return;
        }
        let sequence = seq(event).unwrap_or(0.);
        let first = *self.first_seq.get_or_insert(sequence);
        let position_of = |sequence| {
            self.replacements
                .iter()
                .find(|(seq, _)| *seq == sequence)
                .map(|(_, pos)| *pos)
        };
        let previous_survivor = self
            .nodes
            .iter()
            .rev()
            .find(|node| !node.text.is_empty())
            .map(|node| seq(&node.event));
        let mut position = sequence;
        if let Some(op) = replacement {
            let start = op["startSeq"].as_f64().unwrap_or(sequence);
            let end = op["endSeq"].as_f64().unwrap_or(sequence);
            self.uncertain |= (start < first && position_of(start).is_none())
                || (end < first && position_of(end).is_none());
            position = position_of(start).unwrap_or(start);
            let end = position_of(end).unwrap_or(end);
            self.nodes
                .retain(|node| node.position < position || node.position > end);
            self.replacements
                .retain(|(_, pos)| *pos < position || *pos > end);
            self.replacements.push((sequence, position));
        }
        if self.uncertain {
            self.nodes.clear();
            self.replacements.clear();
            self.effective = None;
            if let Some(snapshot) = &mut self.latest {
                snapshot.system.clear();
            }
            return;
        }
        let introduced = system.then(|| Node {
            position,
            event: event.clone(),
            text: event["data"]["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|block| block["type"] == "text")
                .filter_map(|block| block["text"].as_str())
                .collect(),
            update: event["surfaceOp"] == "append"
                && self.nodes.iter().any(|node| !node.text.is_empty()),
        });
        if let Some(node) = &introduced {
            self.nodes.push(node.clone());
            self.nodes.sort_by(|a, b| a.position.total_cmp(&b.position));
        }
        let survivor = self.nodes.iter().rev().find(|node| !node.text.is_empty());
        let prior_text = self.effective.as_ref().map(|node| node.text.clone());
        // Compare the effective event, not just its text: removal/replacement is
        // anchored at this event even when an older surviving prompt reappears.
        let survivor_unchanged = survivor.map(|node| seq(&node.event)) == previous_survivor;
        if !survivor_unchanged {
            self.effective = Some(match (&introduced, survivor) {
                (Some(node), Some(current)) if seq(&node.event) == seq(&current.event) => {
                    node.clone()
                }
                _ => Node {
                    position,
                    event: event.clone(),
                    text: survivor.map(|node| node.text.clone()).unwrap_or_default(),
                    update: false,
                },
            });
        }
        let effective = self.effective.clone();
        if let Some(node) = effective
            && Some(&node.text) != prior_text.as_ref()
            && introduced.as_ref().is_none_or(|node| node.update)
            && let Some(previous) = self.latest.clone()
        {
            let mut current = previous.clone();
            current.system.clone_from(&node.text);
            self.latest = Some(current);
            self.emit(&node.event, turn, step, "系统提示词已更新", Some(previous));
            return;
        }
        if let Some(node) = introduced
            && !node.text.is_empty()
            && event["surfaceOp"] == "append"
        {
            let saved = self.latest.take();
            self.emit(
                event,
                turn,
                step,
                if node.update {
                    "系统提示词已更新"
                } else {
                    "初始系统提示词"
                },
                None,
            );
            let record = self.records.last_mut().unwrap();
            record.system_prompt = Some(node.text);
            self.latest = saved;
        }
    }

    pub fn header(&mut self, event: &Value, turn: u64, step: u64) {
        let data = &event["data"]["header"];
        let prompt = Snapshot {
            config: data["config"].clone(),
            system: self
                .effective
                .as_ref()
                .map(|node| node.text.clone())
                .unwrap_or_default(),
            tools: data["tools"].as_array().cloned().unwrap_or_default(),
        };
        let previous = self.latest.replace(prompt.clone());
        if previous.is_none() && event["data"]["reason"] != "initial" {
            return;
        }
        let system_changed = previous
            .as_ref()
            .is_some_and(|old| old.system != prompt.system)
            && self.effective.as_ref().is_none_or(|node| !node.update);
        let tools_changed = previous.as_ref().is_some_and(|old| {
            // Upstream compares JSON.stringify, including original object key order.
            serde_json::to_string(&old.tools).ok() != serde_json::to_string(&prompt.tools).ok()
        });
        if previous.is_some() && !system_changed && !tools_changed {
            return;
        }
        let label = if previous.is_none() {
            "初始系统提示词"
        } else if system_changed && tools_changed {
            "系统提示词和工具已更新"
        } else if system_changed {
            "系统提示词已更新"
        } else {
            "工具已更新"
        };
        let origin = self
            .effective
            .as_ref()
            .filter(|_| previous.is_none() || system_changed)
            .map(|node| node.event.clone())
            .unwrap_or_else(|| event.clone());
        self.emit(&origin, turn, step, label, previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn system(seq: u64, text: &str, op: Value) -> Value {
        json!({"type":"system/message","seq":seq,"time":seq * 100,"surfaceOp":op,
            "data":{"turn":1,"step":1,"message":{"content":[{"type":"text","text":text}]}}})
    }
    fn header(seq: u64, reason: &str, tools: Value) -> Value {
        json!({"type":"request/header","seq":seq,"time":seq * 100,
            "data":{"reason":reason,"header":{"config":{"model":"v4"},"tools":tools}}})
    }
    fn apply(state: &mut Prompts, event: Value) {
        state.surface(&event, 1, 1);
        if event["type"] == "request/header" {
            state.header(&event, 1, 1);
        }
    }

    #[test]
    fn prompt_updates_keep_historical_snapshots_and_do_not_duplicate_retries() {
        let tools = json!([{"name":"bash","description":"Shell","parameters":{"type":"object"}}]);
        let mut state = Prompts::default();
        apply(&mut state, system(1, "First\n", json!("append")));
        assert_eq!(
            state.records[0].detail_tabs(false),
            [DetailTab::SystemPrompt]
        );
        apply(&mut state, header(3, "initial", tools.clone()));
        apply(&mut state, header(4, "series", tools.clone()));
        assert_eq!(state.records.len(), 1);
        assert_eq!(
            state.records[0].detail_tabs(false),
            [DetailTab::SystemPrompt, DetailTab::Tools]
        );
        apply(&mut state, system(5, "Second\n", json!("append")));
        apply(&mut state, header(6, "series", tools));
        apply(&mut state, header(7, "tools", json!([])));
        apply(
            &mut state,
            json!({"type":"user/message","seq":8,"time":800,
            "surfaceOp":{"startSeq":1,"endSeq":5},"data":{"content":[]}}),
        );
        assert_eq!(
            state
                .records
                .iter()
                .map(|row| row.source_seq.unwrap() as u64)
                .collect::<Vec<_>>(),
            [1, 5, 7, 8]
        );
        assert_eq!(state.records[0].system_prompt.as_deref(), Some("First\n"));
        assert_eq!(
            state.records[1]
                .detail
                .previous_prompt
                .as_ref()
                .unwrap()
                .system,
            "First\n"
        );
        assert_eq!(state.records[1].system_prompt.as_deref(), Some("Second\n"));
        assert_eq!(state.records[2].name, "工具已更新");
        assert_eq!(
            state.records[2]
                .detail
                .previous_prompt
                .as_ref()
                .unwrap()
                .tools
                .len(),
            1
        );
        assert_eq!(state.records[3].system_prompt.as_deref(), Some(""));
        assert_eq!(
            state.records[3].preferred_detail_tab(false, &[]),
            DetailTab::Diff
        );
        assert_eq!(
            state.records[3].preferred_detail_tab(false, &[DetailTab::Tools]),
            DetailTab::Tools
        );
    }

    #[test]
    fn replacement_endpoints_inherit_positions_and_unrelated_replacements_keep_origin() {
        let mut state = Prompts::default();
        apply(&mut state, system(1, "A", json!("append")));
        apply(&mut state, system(2, "B", json!("append")));
        apply(&mut state, header(3, "initial", json!([])));
        apply(&mut state, system(4, "C", json!({"startSeq":2,"endSeq":2})));
        apply(&mut state, header(5, "series", json!([])));
        assert_eq!(state.records.last().unwrap().source_seq, Some(4.));
        apply(
            &mut state,
            json!({"type":"user/message","seq":6,"time":600,"surfaceOp":{"startSeq":4,"endSeq":4}}),
        );
        assert_eq!(state.latest.as_ref().unwrap().system, "A");
        assert_eq!(state.records.last().unwrap().source_seq, Some(6.));
        apply(
            &mut state,
            json!({"type":"user/message","seq":7,"time":700,"surfaceOp":{"startSeq":6,"endSeq":6}}),
        );
        assert_eq!(seq(&state.effective.as_ref().unwrap().event), Some(6.));
    }

    #[test]
    fn partial_history_does_not_invent_an_initial_prompt_or_a_replacement_order() {
        let mut state = Prompts::default();
        apply(&mut state, header(10, "series", json!([])));
        assert!(state.records.is_empty());
        apply(
            &mut state,
            system(12, "Unavailable", json!({"startSeq":1,"endSeq":5})),
        );
        apply(&mut state, header(13, "series", json!([])));
        assert!(state.uncertain);
        assert!(state.latest.unwrap().system.is_empty());
        assert!(state.records.is_empty());
    }
}
