//! TrajectoryTable parentRecords/openRecordSummary at upstream 39163dd.
use super::{Kind, Record, State, folds::FoldDomain};
use serde_json::Value;

pub(crate) fn parents<'a>(
    records: &'a [Record],
    record: &Record,
) -> (Option<&'a Record>, Option<&'a Record>) {
    if !matches!(record.kind, Kind::Tool | Kind::Subtool) {
        return (None, None);
    }
    let Some(at) = records.iter().position(|item| item.key == record.key) else {
        return (None, None);
    };
    let tool = (record.kind == Kind::Subtool)
        .then(|| {
            records[..at]
                .iter()
                .rev()
                .take_while(|item| item.turn == record.turn && item.step == record.step)
                .find(|item| item.kind == Kind::Tool)
        })
        .flatten();
    let message = tool
        .unwrap_or(record)
        .entry
        .external_id
        .as_deref()
        .and_then(|call| {
            records.iter().find(|item| {
                item.turn == record.turn
                    && item.kind == Kind::Message
                    && item.detail.blocks.iter().any(|block| {
                        block["type"] == "tool-call" && block["id"].as_str() == Some(call)
                    })
            })
        });
    (message, tool)
}

pub(crate) fn source_label(source: &Value) -> String {
    match source["kind"].as_str() {
        Some("user") => "用户".into(),
        Some("goal") => match source["round"].as_f64().filter(|round| *round > 0.) {
            Some(round) => format!("目标 · Round {round}"),
            None => "目标".into(),
        },
        Some(kind) if !kind.is_empty() => {
            let mut chars = kind.chars();
            format!("{}{}", chars.next().unwrap().to_uppercase(), chars.as_str())
        }
        _ => "未知".into(),
    }
}

pub(crate) fn unfold_for_navigation(state: &mut State, key: &str) {
    let Some(at) = state.records.iter().position(|item| item.key == key) else {
        return;
    };
    let record = &state.records[at];
    state.folded.remove(&FoldDomain::Turn.key(record));
    if record.kind == Kind::Subtool
        && let Some(tool) = state.records[..at]
            .iter()
            .rev()
            .find(|item| item.kind != Kind::Subtool)
        && tool.kind == Kind::Tool
    {
        state.folded.remove(&FoldDomain::Subcalls.key(tool));
    }
    if matches!(record.kind, Kind::Tool | Kind::Subtool)
        && let Some(message) = state.records[..at]
            .iter()
            .rev()
            .take_while(|item| item.turn == record.turn)
            .find(|item| item.kind == Kind::Message)
    {
        state.folded.remove(&FoldDomain::Calls.key(message));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state() -> State {
        super::super::project(&[
            json!({"type":"turn/start","seq":1,"data":{"turn":1}}),
            json!({"type":"step/start","seq":2,"data":{"turn":1,"step":1}}),
            json!({"type":"assistant/message","seq":3,"data":{"turn":1,"step":1,"message":{"id":"caller","content":[{"type":"tool-call","id":"root","name":"run_code","arguments":"{}"}]}}}),
            json!({"type":"assistant/message","seq":4,"data":{"turn":1,"step":1,"message":{"id":"other","content":[{"type":"text","text":"Unrelated"}]}}}),
            json!({"type":"tool/call","seq":5,"data":{"turn":1,"step":1,"callId":"root","name":"run_code","arguments":"{}"}}),
            json!({"type":"tool/ptc-dispatch-start","seq":6,"data":{"rootCallId":"root","parentCallId":"root","subCallId":"child","name":"read_file","arguments":{}}}),
            json!({"type":"tool/ptc-dispatch-start","seq":7,"data":{"rootCallId":"root","parentCallId":"child","subCallId":"grandchild","name":"grep","arguments":{}}}),
        ])
    }

    #[test]
    fn hierarchy_uses_call_identity_and_root_tool_within_the_same_group() {
        let mut state = state();
        let child = state.records.last().unwrap().clone();
        let (message, tool) = parents(&state.records, &child);
        assert_eq!(message.unwrap().key, "assistant-caller");
        assert_eq!(tool.unwrap().key, "tool-root");
        state.records[0].turn = 2;
        assert!(
            parents(&state.records, &child).0.is_none(),
            "same call in another turn is not its parent"
        );
        state.records.last_mut().unwrap().step = 2;
        assert!(
            parents(&state.records, state.records.last().unwrap())
                .1
                .is_none(),
            "root search stops at the step boundary"
        );
    }

    #[test]
    fn hierarchy_navigation_reveals_only_the_required_fold_ancestors() {
        let mut state = state();
        state.folded.extend(
            [
                "turn:1",
                "turn:2",
                "assistant:assistant-other",
                "tool:tool-root",
                "tool:unrelated",
            ]
            .map(str::to_owned),
        );
        unfold_for_navigation(&mut state, "tool-grandchild");
        assert_eq!(
            state.folded,
            ["turn:2", "tool:unrelated"].map(str::to_owned).into()
        );
        assert_eq!(
            source_label(&json!({"kind":"goal","round":3})),
            "目标 · Round 3"
        );
        assert_eq!(source_label(&json!({"kind":"plugin"})), "Plugin");
        assert_eq!(source_label(&Value::Null), "未知");
    }
}
