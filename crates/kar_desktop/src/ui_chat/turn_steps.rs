use kar::{
    chat::{ChatTurn, Item, Role},
    observe::Agent,
};

pub(crate) struct TurnSteps {
    root: Option<String>,
    answer: Vec<usize>,
}

impl TurnSteps {
    pub(crate) fn new(turn: &ChatTurn, root: Option<&str>) -> Self {
        let root = turn_root(turn, root);
        let mut answer = Vec::new();
        let mut after_work = false;
        for (index, entry) in turn.items.iter().enumerate() {
            match &entry.item {
                // A diff is a mutable turn snapshot and can arrive between two pieces of prose.
                Item::Diff { .. } => {}
                Item::Message { text, .. } if text.trim().is_empty() => {}
                Item::Message { role: Role::Assistant, .. }
                    if !child_source(turn, &entry.source, root) =>
                {
                    // A trailing event does not erase a reply. Replace an earlier
                    // group only when the provider actually emits more root text.
                    if after_work {
                        answer.clear();
                    }
                    answer.push(index);
                    after_work = false;
                }
                _ => after_work = true,
            }
        }
        Self { answer, root: root.map(str::to_owned) }
    }

    pub(crate) fn is_answer(&self, index: usize) -> bool {
        self.answer.binary_search(&index).is_ok()
    }

    pub(super) fn is_child(&self, turn: &ChatTurn, index: usize) -> bool {
        turn.items
            .get(index)
            .is_some_and(|entry| child_source(turn, &entry.source, self.root.as_deref()))
    }

    pub(super) fn has_multiple_answers(&self) -> bool {
        self.answer.len() > 1
    }

    pub(crate) fn has_answer(&self) -> bool {
        !self.answer.is_empty()
    }
}

// Inherited turns retain the source of the original prompt after a fork.
fn turn_root<'a>(turn: &'a ChatTurn, root: Option<&'a str>) -> Option<&'a str> {
    turn.items
        .iter()
        .find_map(|entry| {
            let session = entry.source.session_id.as_deref()?;
            (matches!(entry.item, Item::Message { role: Role::User, .. })
                && entry.source.path.is_empty()
                && !turn.child_sessions.contains_key(session))
            .then_some(session)
        })
        .or(root)
}

fn child_source(turn: &ChatTurn, source: &Agent, root: Option<&str>) -> bool {
    let session = source.session_id.as_deref();
    !source.path.is_empty()
        || session.is_some_and(|id| turn.child_sessions.contains_key(id))
        || session.zip(root).is_some_and(|(id, root)| id != root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kar::{chat::TimelineItem, observe::Tool};
    use serde_json::json;
    use uuid::Uuid;

    fn turn(rows: Vec<TimelineItem>) -> ChatTurn {
        serde_json::from_value(json!({
            "id":Uuid::new_v4(), "started_ms":1, "status":"completed", "items":rows
        }))
        .unwrap()
    }

    fn row(session: Option<&str>, item: Item) -> TimelineItem {
        TimelineItem {
            source: Agent { session_id: session.map(str::to_owned), path: vec![] },
            item,
        }
    }

    fn say(session: Option<&str>, role: Role, text: &str) -> TimelineItem {
        row(session, Item::Message { role, text: text.into(), provider_id: None, complete: true })
    }

    #[test]
    fn keeps_final_markdown_blocks_across_a_diff_and_ignores_trailing_work() {
        let root = Some("original");
        let tool = row(root, Item::Tool { id: "read".into(), activity: Tool::default() });
        let mut turn = turn(vec![
            say(root, Role::User, "start"),
            say(root, Role::Assistant, "same words"),
            tool.clone(),
            say(root, Role::Assistant, "same words"),
            row(root, Item::Diff { turn_id: "native".into(), diff: "diff".into() }),
            say(None, Role::Assistant, "    code\n"),
            say(Some("worker"), Role::Assistant, "late child"),
            tool,
        ]);
        turn.child_sessions.insert("worker".into(), "original".into());
        let fork_root = Some("new-chat-after-fork");
        let steps = TurnSteps::new(&turn, fork_root);
        assert!(steps.has_answer());
        let indices: Vec<_> = (0..turn.items.len()).filter(|&i| steps.is_answer(i)).collect();
        assert_eq!(indices, [3, 5]);
        assert!(!steps.is_child(&turn, 3));
        assert!(steps.is_child(&turn, 6));
    }

    #[test]
    fn only_child_or_whitespace_output_does_not_create_a_final_reply() {
        let root = Some("root");
        let mut nested = say(root, Role::Assistant, "nested result");
        nested.source.path.push("worker".into());
        let turn = turn(vec![
            say(root, Role::Reasoning, "reasoning"),
            say(root, Role::Assistant, "  \n"),
            say(Some("child"), Role::Assistant, "child result"),
            nested,
        ]);
        let steps = TurnSteps::new(&turn, root);
        assert!(!steps.has_answer());
        assert!(steps.is_child(&turn, 2));
        assert!(steps.is_child(&turn, 3));
    }
}
