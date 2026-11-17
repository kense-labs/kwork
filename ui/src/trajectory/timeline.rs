//! Host records for the upstream timeline projection. Request boundaries are
//! intentionally excluded: the assistant record owns the model operation.
use super::*;
use serde_json::json;

pub(crate) fn turns(state: &State) -> Vec<Value> {
    let mut turns = Vec::new();
    let mut current = None;
    let mut cells = Vec::new();
    for record in &state.records {
        let turn = (record.turn != 0).then_some(record.turn);
        if current != turn && !cells.is_empty() {
            turns.push(json!({ "turn": current, "groups": [{ "cells": cells }] }));
            cells = Vec::new();
        }
        current = turn;
        let kind = match record.kind {
            Kind::System => "system",
            Kind::User => "user",
            Kind::Context => "context",
            Kind::Compacted => "compacted",
            Kind::Message => "message",
            Kind::Tool => "tool",
            Kind::Subtool => "subtool",
        };
        let mut cell = json!({
            "index": record.index,
            "kind": kind,
            "text": record.name,
            "isError": record.failed(),
            "startedAt": record.timing.started_at,
            "timeSeconds": record.timing.duration_ms().map(|ms| ms / 1000.),
        });
        if record.kind == Kind::Message {
            cell["assistantMetrics"] = json!({
                "timingRecorded": record.timing.started_at.is_some(),
                "stepStartTime": record.timing.started_at,
                "firstTokenTime": record.timing.first_token_at,
                "completedTime": record.timing.completed_at,
                "usageProvided": record.usage.is_some(),
                "outputTokens": record.usage.as_ref().and_then(|usage| usage.output),
            });
        }
        cells.push(cell);
    }
    if !cells.is_empty() {
        turns.push(json!({ "turn": current, "groups": [{ "cells": cells }] }));
    }
    turns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_keeps_untimed_records_and_omits_request_boundaries() {
        let state = project(&[
            json!({"type":"turn/start","seq":1,"data":{"turn":1}}),
            json!({"type":"user/message","seq":2,"data":{"id":"u","content":[{"type":"text","text":"Hello"}]}}),
            json!({"type":"step/start","seq":3,"time":1000,"data":{"turn":1,"step":1}}),
            json!({"type":"assistant/message","seq":4,"time":1500,"data":{"turn":1,"step":1,"message":{"id":"a","content":[{"type":"text","text":"Done"}]}}}),
            json!({"type":"tool/call","seq":5,"time":1600,"data":{"turn":1,"step":1,"callId":"c","name":"bash","arguments":"{}"}}),
        ]);
        assert_eq!(state.requests.len(), 1);
        let turns = turns(&state);
        let cells = turns[0]["groups"][0]["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0]["kind"], "user");
        assert!(cells[0]["startedAt"].is_null());
        assert_eq!(cells[1]["startedAt"], 1000.);
        assert_eq!(cells[1]["timeSeconds"], 0.5);
        assert_eq!(cells[2]["startedAt"], 1600.);
        assert!(cells[2]["timeSeconds"].is_null());
    }
}
