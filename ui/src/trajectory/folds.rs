//! Ledger folding follows TrajectoryView/TrajectoryTable at 39163dd.
//! Turns, assistant calls, and tool subcalls retain independent identities.
use super::{Kind, Record, matches};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FoldDomain {
    Turn,
    Calls,
    Subcalls,
}

impl FoldDomain {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "$turn" => Some(Self::Turn),
            "$call" => Some(Self::Calls),
            "$subcall" => Some(Self::Subcalls),
            _ => None,
        }
    }

    pub fn key(self, record: &Record) -> String {
        match self {
            Self::Turn => format!("turn:{}", record.turn),
            Self::Calls => format!("assistant:{}", record.key),
            Self::Subcalls => format!("tool:{}", record.key),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Visible<'a> {
    pub record: &'a Record,
    pub summary: Option<String>,
    pub summary_domain: Option<FoldDomain>,
}

pub(crate) fn first_turn_content(records: &[Record]) -> HashMap<u64, &str> {
    let mut first = HashMap::new();
    for record in records {
        if record.turn > 0 && record.kind != Kind::System {
            first.entry(record.turn).or_insert(record.key.as_str());
        }
    }
    first
}

fn collapsible(records: &[Record], domain: FoldDomain) -> Vec<&Record> {
    match domain {
        FoldDomain::Turn => {
            let first = first_turn_content(records);
            let mut counts = HashMap::<u64, usize>::new();
            for record in records.iter().filter(|record| record.kind != Kind::System) {
                *counts.entry(record.turn).or_default() += 1;
            }
            records
                .iter()
                .filter(|record| {
                    first.get(&record.turn) == Some(&record.key.as_str())
                        && counts.get(&record.turn).is_some_and(|count| *count > 1)
                })
                .collect()
        }
        FoldDomain::Calls | FoldDomain::Subcalls => records
            .windows(2)
            .filter_map(|pair| {
                let [record, next] = pair else { unreachable!() };
                (record.turn == next.turn
                    && match domain {
                        FoldDomain::Calls => {
                            record.kind == Kind::Message
                                && matches!(next.kind, Kind::Tool | Kind::Subtool)
                        }
                        FoldDomain::Subcalls => {
                            record.kind == Kind::Tool
                                && next.kind == Kind::Subtool
                                && record.step == next.step
                        }
                        FoldDomain::Turn => unreachable!(),
                    })
                .then_some(record)
            })
            .collect(),
    }
}

pub(crate) fn fold_states(records: &[Record], folded: &HashSet<String>) -> [bool; 3] {
    [FoldDomain::Turn, FoldDomain::Calls, FoldDomain::Subcalls].map(|domain| {
        let records = collapsible(records, domain);
        !records.is_empty()
            && records
                .iter()
                .all(|record| folded.contains(&domain.key(record)))
    })
}

pub(crate) fn toggle_global_fold(
    folded: &mut HashSet<String>,
    records: &[Record],
    domain: FoldDomain,
) {
    let keys: Vec<_> = collapsible(records, domain)
        .into_iter()
        .map(|record| domain.key(record))
        .collect();
    if keys.iter().all(|key| folded.contains(key)) {
        for key in keys {
            folded.remove(&key);
        }
    } else {
        folded.extend(keys);
    }
}

pub(crate) fn folded_turns(records: &[Record], folded: &HashSet<String>) -> HashSet<u64> {
    collapsible(records, FoldDomain::Turn)
        .into_iter()
        .filter(|record| folded.contains(&FoldDomain::Turn.key(record)))
        .map(|record| record.turn)
        .collect()
}

/// A double-click expands a folded turn, otherwise folds assistant calls before
/// considering the turn's first content row. Tool rows do not fold on double-click.
pub(crate) fn double_click_folds<'a>(
    records: &'a [Record],
    folded: &HashSet<String>,
) -> HashMap<&'a str, FoldDomain> {
    let mut actions = HashMap::new();
    for domain in [FoldDomain::Turn, FoldDomain::Calls] {
        for record in collapsible(records, domain) {
            actions.insert(record.key.as_str(), domain);
        }
    }
    for record in records {
        if folded.contains(&FoldDomain::Turn.key(record)) {
            actions.insert(record.key.as_str(), FoldDomain::Turn);
        }
    }
    actions
}

pub(crate) fn visible<'a>(
    records: &'a [Record],
    requests: &[Record],
    folded: &HashSet<String>,
    query: &str,
) -> Vec<Visible<'a>> {
    let row = |record| Visible {
        record,
        summary: None,
        summary_domain: None,
    };
    // Upstream search ignores folding entirely; it restores the fold state when cleared.
    if !query.trim().is_empty() {
        return records
            .iter()
            .filter(|record| matches(record, query))
            .map(row)
            .collect();
    }
    let first = first_turn_content(records);
    let turns = folded_turns(records, folded);
    let requests: HashSet<_> = requests
        .iter()
        .map(|request| (request.turn, request.step))
        .collect();
    let mut rows = Vec::new();
    for record in records {
        if record.kind == Kind::System || !turns.contains(&record.turn) {
            rows.push(row(record));
        } else if first.get(&record.turn) == Some(&record.key.as_str()) {
            let contents: Vec<_> = records
                .iter()
                .filter(|other| {
                    other.turn == record.turn
                        && other.kind != Kind::System
                        && other.key != record.key
                })
                .collect();
            let steps: HashSet<_> = contents
                .iter()
                .map(|record| (record.turn, record.step))
                .filter(|step| requests.contains(step))
                .collect();
            let calls = contents
                .iter()
                .filter(|record| matches!(record.kind, Kind::Tool | Kind::Subtool))
                .count();
            rows.push(row(record));
            rows.push(Visible {
                record,
                summary: Some(format!("{} 个步骤 · {calls} 个工具调用", steps.len())),
                summary_domain: Some(FoldDomain::Turn),
            });
        }
    }
    for domain in [FoldDomain::Calls, FoldDomain::Subcalls] {
        let mut out = Vec::new();
        let mut remaining = rows.into_iter().peekable();
        while let Some(row) = remaining.next() {
            let record = row.record;
            let target = if domain == FoldDomain::Calls {
                Kind::Message
            } else {
                Kind::Tool
            };
            let collapse = row.summary.is_none()
                && record.kind == target
                && folded.contains(&domain.key(record));
            out.push(row);
            if !collapse {
                continue;
            }
            let mut names = Vec::new();
            let mut count = 0;
            while let Some(call) = remaining.next_if(|call| {
                call.summary.is_none()
                    && matches!(call.record.kind, Kind::Tool | Kind::Subtool)
                    && (domain == FoldDomain::Calls || call.record.kind == Kind::Subtool)
            }) {
                count += 1;
                if !call.record.name.is_empty() && !names.contains(&call.record.name.as_str()) {
                    names.push(call.record.name.as_str());
                }
            }
            if count > 0 {
                let mut summary = format!("{count} 个工具调用");
                if !names.is_empty() {
                    summary.push_str(&format!(" · {}", names.join(", ")));
                }
                out.push(Visible {
                    record,
                    summary: Some(summary),
                    summary_domain: Some(domain),
                });
            }
        }
        rows = out;
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::{State, project};
    use serde_json::json;

    fn fixture() -> State {
        project(&[
            json!({"seq":1,"type":"turn/start","data":{"turn":1}}),
            json!({"seq":2,"type":"user/message","data":{"id":"user","content":[{"type":"text","text":"inspect"}]}}),
            json!({"seq":3,"type":"step/start","data":{"turn":1,"step":1}}),
            json!({"seq":4,"type":"assistant/message","data":{"turn":1,"step":1,"message":{"id":"a","content":[{"type":"text","text":"working"}]}}}),
            json!({"seq":5,"type":"tool/call","data":{"turn":1,"step":1,"callId":"root","name":"run_code","arguments":"{}"}}),
            json!({"seq":6,"type":"tool/ptc-dispatch-start","data":{"rootCallId":"root","parentCallId":"root","subCallId":"child","name":"read_file","arguments":{"path":"README.md"}}}),
            json!({"seq":7,"type":"tool/ptc-dispatch-start","data":{"rootCallId":"root","parentCallId":"child","subCallId":"grandchild","name":"grep","arguments":{"query":"needle"}}}),
            json!({"seq":8,"type":"tool/call","data":{"turn":1,"step":1,"callId":"leaf","name":"bash","arguments":"{}"}}),
            json!({"seq":9,"type":"assistant/message","data":{"turn":1,"step":1,"message":{"id":"end","content":[{"type":"text","text":"done"}]}}}),
        ])
    }

    fn rows<'a>(state: &'a State, query: &str) -> Vec<Visible<'a>> {
        visible(&state.records, &state.requests, &state.folded, query)
    }

    fn double_click_fold(
        records: &[Record],
        folded: &HashSet<String>,
        record: &Record,
    ) -> Option<FoldDomain> {
        double_click_folds(records, folded)
            .get(record.key.as_str())
            .copied()
    }

    #[test]
    fn calls_fold_under_assistants_and_subcalls_under_root_tools() {
        let mut state = fixture();
        toggle_global_fold(&mut state.folded, &state.records, FoldDomain::Calls);
        assert_eq!(
            fold_states(&state.records, &state.folded),
            [false, true, false]
        );
        let rows = rows(&state, "");
        assert_eq!(
            rows.iter()
                .map(|row| row.record.key.as_str())
                .collect::<Vec<_>>(),
            ["user", "assistant-a", "assistant-a", "assistant-end"]
        );
        assert_eq!(
            rows[2].summary.as_deref(),
            Some("4 个工具调用 · run_code, read_file, grep, bash")
        );
        assert_eq!(rows[2].summary_domain, Some(FoldDomain::Calls));
        toggle_global_fold(&mut state.folded, &state.records, FoldDomain::Calls);
        toggle_global_fold(&mut state.folded, &state.records, FoldDomain::Subcalls);
        let rows = super::visible(&state.records, &state.requests, &state.folded, "");
        assert_eq!(
            rows.iter()
                .map(|row| row.record.key.as_str())
                .collect::<Vec<_>>(),
            [
                "user",
                "assistant-a",
                "tool-root",
                "tool-root",
                "tool-leaf",
                "assistant-end"
            ]
        );
        assert_eq!(
            rows[3].summary.as_deref(),
            Some("2 个工具调用 · read_file, grep")
        );
        assert_eq!(
            fold_states(&state.records, &state.folded),
            [false, false, true]
        );
    }

    #[test]
    fn search_bypasses_all_folds_and_clearing_restores_independent_domains() {
        let mut state = fixture();
        for domain in [FoldDomain::Subcalls, FoldDomain::Calls, FoldDomain::Turn] {
            toggle_global_fold(&mut state.folded, &state.records, domain);
        }
        let filtered = rows(&state, "needle");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].record.key, "tool-grandchild");
        assert_eq!(filtered[0].summary, None);
        assert!(rows(&state, "missing").is_empty());
        assert_eq!(
            rows(&state, "")[1].summary.as_deref(),
            Some("1 个步骤 · 4 个工具调用")
        );
        toggle_global_fold(&mut state.folded, &state.records, FoldDomain::Turn);
        assert_eq!(rows(&state, "")[2].summary_domain, Some(FoldDomain::Calls));
        toggle_global_fold(&mut state.folded, &state.records, FoldDomain::Calls);
        assert_eq!(
            rows(&state, "")[3].summary_domain,
            Some(FoldDomain::Subcalls)
        );
    }

    #[test]
    fn newly_arriving_calls_are_not_folded_by_a_previous_global_toggle() {
        let mut state = fixture();
        toggle_global_fold(&mut state.folded, &state.records, FoldDomain::Calls);
        state.apply_page(&[json!({"seq":10,"type":"tool/call","data":{"turn":1,"step":1,"callId":"new","name":"bash","arguments":"{}"}})], false);
        assert!(!fold_states(&state.records, &state.folded)[1]);
        assert!(
            rows(&state, "")
                .iter()
                .any(|row| row.record.key == "tool-new")
        );
        toggle_global_fold(&mut state.folded, &state.records, FoldDomain::Calls);
        assert!(fold_states(&state.records, &state.folded)[1]);
        assert!(
            !rows(&state, "")
                .iter()
                .any(|row| row.record.key == "tool-new")
        );
    }

    #[test]
    fn turns_include_tool_only_history_and_keep_system_rows() {
        let mut state = fixture();
        state
            .records
            .retain(|record| record.kind != Kind::User && record.kind != Kind::Message);
        let mut system = state.records[0].clone();
        system.key = "system".into();
        system.kind = Kind::System;
        state.records.insert(1, system);
        toggle_global_fold(&mut state.folded, &state.records, FoldDomain::Turn);
        let rows = rows(&state, "");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].record.key, "tool-root");
        assert_eq!(rows[1].summary.as_deref(), Some("1 个步骤 · 3 个工具调用"));
        assert_eq!(rows[2].record.kind, Kind::System);
    }

    #[test]
    fn double_click_prefers_assistant_calls_and_expands_folded_turns() {
        let mut state = fixture();
        state.records.remove(0);
        let first = &state.records[0];
        assert_eq!(
            double_click_fold(&state.records, &state.folded, first),
            Some(FoldDomain::Calls)
        );
        assert_eq!(
            double_click_fold(&state.records, &state.folded, &state.records[1]),
            None
        );
        state.folded.insert(FoldDomain::Turn.key(first));
        assert_eq!(
            double_click_fold(&state.records, &state.folded, first),
            Some(FoldDomain::Turn)
        );
    }
}
