//! Host-confirmed reading reminders and visible-main-view acknowledgement.

use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReadRecord {
    completed_turn: u64,
    failed: bool,
    read_turn: u64,
    manual_unread: bool,
}

impl ReadRecord {
    fn from_value(value: &Value) -> Option<(String, Self)> {
        Some((
            value["sessionId"].as_str()?.to_owned(),
            Self {
                completed_turn: value["completedTurn"].as_u64()?,
                failed: value["failed"].as_bool()?,
                read_turn: value["readTurn"].as_u64()?,
                manual_unread: value["manualUnread"].as_bool()?,
            },
        ))
    }
}

#[derive(Default)]
pub(crate) struct SessionReading {
    ready: bool,
    records: HashMap<String, ReadRecord>,
    entered: Option<String>,
    attempted: Option<(String, u64)>,
    pending: bool,
}

impl SessionReading {
    pub(crate) fn replace(&mut self, snapshot: &Value) {
        self.ready = false;
        self.records.clear();
        self.pending = false;
        self.attempted = None;
        let Some(items) = snapshot["items"]
            .as_array()
            .filter(|_| snapshot["available"] == true)
        else {
            return;
        };
        let parsed = items
            .iter()
            .map(ReadRecord::from_value)
            .collect::<Option<Vec<_>>>();
        let Some(parsed) = parsed else { return };
        self.records.extend(parsed);
        self.ready = true;
        self.settle_entry();
    }

    pub(crate) fn disconnect(&mut self) {
        self.ready = false;
        self.pending = false;
        self.attempted = None;
    }

    pub(crate) fn accept(&mut self, value: &Value) -> bool {
        if !self.ready {
            return false;
        }
        let Some((id, record)) = ReadRecord::from_value(value) else {
            return false;
        };
        self.records.insert(id, record);
        self.settle_entry();
        true
    }

    pub(crate) fn enter(&mut self, id: &str) {
        self.entered = Some(id.to_owned());
        self.attempted = None;
        self.settle_entry();
    }

    pub(crate) fn focus_changed(&mut self) {
        self.attempted = None;
    }

    fn settle_entry(&mut self) {
        if self.ready
            && let Some(entered) = &self.entered
            && !self
                .records
                .get(entered)
                .is_some_and(|record| record.manual_unread)
        {
            self.entered = None;
        }
    }

    pub(crate) fn completion_unread(&self, id: &str, viewing: bool) -> bool {
        self.ready
            && self.records.get(id).is_some_and(|record| {
                record.manual_unread || (!viewing && record.completed_turn > record.read_turn)
            })
    }

    pub(crate) fn completion_failed(&self, id: &str, viewing: bool) -> bool {
        self.completion_unread(id, viewing)
            && self.records.get(id).is_some_and(|record| record.failed)
    }

    pub(crate) fn mark(&self, id: &str, unread: bool) -> Option<Value> {
        if !self.ready {
            return None;
        }
        let through_turn = self
            .records
            .get(id)
            .map_or(0, |record| record.completed_turn);
        Some(json!({
            "sessionId": id,
            "action": if unread {
                json!({"type": "unread"})
            } else {
                json!({"type": "read", "throughTurn": through_turn})
            },
        }))
    }

    /// Commit at most one acknowledgement after the visible open view receives the turn.
    pub(crate) fn acknowledge(
        &mut self,
        id: &str,
        open: bool,
        visible: bool,
        received_turn: u64,
    ) -> Option<Value> {
        if !self.ready || !open || !visible || self.pending {
            return None;
        }
        let record = *self.records.get(id)?;
        if self
            .attempted
            .as_ref()
            .is_some_and(|(attempted, turn)| attempted == id && *turn == record.completed_turn)
        {
            return None;
        }
        let clearing = self.entered.as_deref() == Some(id);
        if !clearing && record.completed_turn <= record.read_turn {
            return None;
        }
        if record.completed_turn > received_turn {
            return None;
        }
        self.attempted = Some((id.to_owned(), record.completed_turn));
        self.pending = true;
        if clearing {
            self.entered = None;
        }
        Some(json!({
            "sessionId": id,
            "action": {
                "type": if clearing { "read" } else { "acknowledge" },
                "throughTurn": record.completed_turn,
            },
        }))
    }

    pub(crate) fn finish_acknowledge(&mut self) {
        self.pending = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(turn: u64, read: u64, manual: bool) -> Value {
        json!({"sessionId":"s","completedTurn":turn,"failed":false,"readTurn":read,"manualUnread":manual})
    }

    fn ready(records: Vec<Value>) -> SessionReading {
        let mut reading = SessionReading::default();
        reading.replace(&json!({"available":true,"items":records}));
        reading
    }

    #[test]
    fn only_a_ready_baseline_enables_non_optimistic_marks() {
        let mut reading = SessionReading::default();
        assert_eq!(reading.mark("s", true), None);
        assert!(!reading.accept(&record(1, 0, false)));
        reading.replace(&json!({"available":true,"items":[]}));
        assert_eq!(
            reading.mark("s", true),
            Some(json!({"sessionId":"s","action":{"type":"unread"}}))
        );
        assert!(!reading.completion_unread("s", false));
        assert!(reading.accept(&record(1, 0, false)));
        assert_eq!(
            reading.mark("s", false),
            Some(json!({"sessionId":"s","action":{"type":"read","throughTurn":1}}))
        );
        assert!(reading.completion_unread("s", false));
    }

    #[test]
    fn visible_view_hides_only_the_natural_reminder_and_failure_requires_unread() {
        let mut reading = ready(vec![record(2, 1, false)]);
        assert!(!reading.completion_unread("s", true));
        assert!(reading.completion_unread("s", false));
        assert!(!reading.completion_failed("s", false));
        let mut failed = record(3, 1, false);
        failed["failed"] = true.into();
        assert!(reading.accept(&failed));
        assert!(reading.completion_failed("s", false));
        assert!(!reading.completion_failed("s", true));
        assert!(reading.accept(&record(3, 3, false)));
        assert!(!reading.completion_failed("s", false));
    }

    #[test]
    fn acknowledgement_waits_for_open_visible_content_and_preserves_manual_marks() {
        let mut reading = ready(vec![record(2, 0, true)]);
        assert_eq!(reading.acknowledge("s", false, true, 2), None);
        assert_eq!(reading.acknowledge("s", true, false, 2), None);
        assert_eq!(reading.acknowledge("s", true, true, 1), None);
        assert_eq!(
            reading.acknowledge("s", true, true, 2),
            Some(json!({"sessionId":"s","action":{"type":"acknowledge","throughTurn":2}}))
        );
        assert_eq!(reading.acknowledge("s", true, true, 2), None);
        reading.finish_acknowledge();
        assert!(reading.accept(&record(2, 2, true)));
        assert_eq!(reading.acknowledge("s", true, true, 2), None);
        assert!(reading.completion_unread("s", true));
    }

    #[test]
    fn entering_clears_a_manual_reminder_once_but_refocusing_does_not() {
        let mut reading = ready(vec![record(2, 2, true)]);
        reading.enter("s");
        assert_eq!(
            reading.acknowledge("s", true, true, 2),
            Some(json!({"sessionId":"s","action":{"type":"read","throughTurn":2}}))
        );
        reading.finish_acknowledge();
        assert!(reading.accept(&record(2, 2, false)));
        reading.focus_changed();
        assert_eq!(reading.acknowledge("s", true, true, 2), None);
        assert!(reading.accept(&record(2, 2, true)));
        reading.focus_changed();
        assert_eq!(reading.acknowledge("s", true, true, 2), None);
    }

    #[test]
    fn a_new_attempt_follows_a_later_completion_but_not_a_duplicate() {
        let mut reading = ready(vec![record(1, 0, false)]);
        assert!(reading.acknowledge("s", true, true, 1).is_some());
        reading.finish_acknowledge();
        assert!(reading.acknowledge("s", true, true, 1).is_none());
        assert!(reading.accept(&record(2, 0, false)));
        assert!(reading.acknowledge("s", true, true, 1).is_none());
        assert!(reading.acknowledge("s", true, true, 2).is_some());
        reading.disconnect();
        assert!(reading.mark("s", true).is_none());
    }
}
