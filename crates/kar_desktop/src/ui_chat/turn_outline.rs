use std::{collections::BTreeSet, fs, io, path::Path};

use kar::chat::{ChatTurn, Item};
use markdown::{ParseOptions, mdast::Node, to_mdast};
use uuid::Uuid;

use super::{original_user_text, turn_steps::TurnSteps, user_inputs};
use crate::{studio_hub::truncate_chars, types::SelectionTarget};

pub(crate) struct TurnPreview {
    pub question: String,
    pub answer: String,
}

pub(crate) fn preview(
    turn: &ChatTurn,
    root: Option<&str>,
    target: Option<&SelectionTarget>,
) -> TurnPreview {
    let steps = TurnSteps::new(turn, root);
    let question = user_inputs(turn)
        .find_map(|input| {
            let attachments = if input.primary {
                turn.attachments.as_slice()
            } else {
                input
                    .id
                    .and_then(|id| turn.steered_attachments.get(&id))
                    .map(Vec::as_slice)
                    .unwrap_or_default()
            };
            let source =
                original_user_text(input.text, target.filter(|_| input.primary), attachments);
            let text = excerpt(std::iter::once(source), 100);
            (!text.is_empty()).then_some(text)
        })
        .unwrap_or_default();
    let replies = turn.items.iter().enumerate().filter_map(|(index, entry)| match &entry.item {
        Item::Message { text, .. } if steps.is_answer(index) => Some(text.as_str()),
        _ => None,
    });
    TurnPreview { question, answer: excerpt(replies, 180) }
}

fn excerpt<'a>(messages: impl Iterator<Item = &'a str>, limit: usize) -> String {
    let mut text = String::new();
    for message in messages {
        if let Ok(node) = to_mdast(message, &ParseOptions::gfm()) {
            append_plain(&node, &mut text);
        } else {
            text.push_str(message);
        }
        text.push(' ');
        if text.split_whitespace().flat_map(str::chars).nth(limit).is_some() {
            break;
        }
    }
    truncate_chars(&text.split_whitespace().collect::<Vec<_>>().join(" "), limit)
}

fn append_plain(node: &Node, text: &mut String) {
    match node {
        Node::Text(n) => text.push_str(&n.value),
        Node::InlineCode(n) => text.push_str(&n.value),
        Node::Image(n) => text.push_str(&n.alt),
        Node::ImageReference(n) => text.push_str(&n.alt),
        Node::Code(n) => {
            text.push_str(&n.value);
            text.push(' ');
        }
        Node::Break(_) => text.push(' '),
        Node::FootnoteDefinition(_) => {}
        _ => {
            if let Some(children) = node.children() {
                for child in children {
                    append_plain(child, text);
                }
                if matches!(node, Node::Heading(_) | Node::Paragraph(_) | Node::TableCell(_)) {
                    text.push(' ');
                }
            }
        }
    }
}

pub(crate) fn bookmarks(store: &Path, chat: Uuid) -> io::Result<BTreeSet<Uuid>> {
    let dir = store.join("turn-bookmarks").join(chat.to_string());
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(e) => return Err(e),
    };
    let mut marked = BTreeSet::new();
    for entry in entries {
        let entry = entry?;
        if let Some(id) = entry.file_name().to_str().and_then(|name| Uuid::parse_str(name).ok())
            && entry.file_type()?.is_file()
        {
            marked.insert(id);
        }
    }
    Ok(marked)
}

pub(crate) fn set_bookmark(store: &Path, chat: Uuid, turn: Uuid, marked: bool) -> io::Result<()> {
    // A marker per (chat, turn) lets independent windows update different bookmarks atomically.
    let dir = store.join("turn-bookmarks").join(chat.to_string());
    let path = dir.join(turn.to_string());
    if marked {
        fs::create_dir_all(dir)?;
        match fs::File::create_new(path) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            result => result.map(drop),
        }
    } else {
        match fs::remove_file(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn outline_uses_the_question_and_visible_final_reply() {
        let file = Uuid::new_v4();
        let source = |name| json!({"session_id":name, "path":[]});
        let message = |name, role, text: &str| {
            json!({
                "source":source(name), "kind":"message", "role":role, "text":text, "complete":true
            })
        };
        let footer = json!([{"name":"a.txt", "mime":"text/plain", "path":format!("/store/{file}/content/a.txt")}]);
        let input = format!(
            "请针对以下工作区源码文件给出修改方案\n内部快照\n用户要求：\n检查**链接**\n和标题\n\nAttached files (JSON):\n{footer}"
        );
        let turn = serde_json::from_value(json!({
            "id":Uuid::new_v4(), "started_ms":1, "status":"completed",
            "child_sessions":{"worker":"main"},
            "attachments":[{"id":file, "name":"a.txt", "kind":"file", "mime":"text/plain", "bytes":1, "sha256":"x"}],
            "items":[message("worker","user","delegated prompt"), message("main","user",&input),
                     message("main","assistant","progress"), {"source":source("main"),"kind":"tool","id":"read","activity":{"finished":true}},
                     message("main","assistant","## 完成\n\n打开[帮助](https://example.test)查看 `path`。"),
                     message("main","assistant","- 第一项\n- 第二项"), message("worker","assistant","late worker")]
        })).unwrap();
        let result = preview(&turn, Some("main"), None);
        assert_eq!(result.question, "检查链接 和标题");
        assert_eq!(result.answer, "完成 打开帮助查看 path。 第一项 第二项");
    }

    #[test]
    fn long_chinese_and_multiline_code_fit_the_preview() {
        let content = "这是一段没有空格的中文。".repeat(120);
        let preview = excerpt([content.as_str()].into_iter(), 100);
        assert_eq!(preview.chars().count(), 100);
        assert!(preview.ends_with('…'));
        assert_eq!(
            excerpt(["```rs\nlet a = 3;\nuse_it(a);\n```"].into_iter(), 32),
            "let a = 3; use_it(a);"
        );
    }

    #[test]
    fn reopening_and_forking_keep_independent_bookmarks() -> io::Result<()> {
        let store = tempfile::tempdir()?;
        let store = store.path();
        let chat = Uuid::new_v4();
        let fork = Uuid::new_v4();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        assert!(bookmarks(store, chat)?.is_empty());
        assert!(!store.join("turn-bookmarks").exists());
        std::thread::scope(|scope| {
            for id in [first, second, first] {
                scope.spawn(move || set_bookmark(store, chat, id, true).unwrap());
            }
        });
        assert_eq!(bookmarks(store, chat)?, BTreeSet::from([first, second]));
        assert!(bookmarks(store, fork)?.is_empty());
        set_bookmark(store, fork, first, true)?;
        set_bookmark(store, chat, first, false)?;
        set_bookmark(store, chat, first, false)?;
        assert_eq!(bookmarks(store, chat)?, BTreeSet::from([second]));
        assert_eq!(bookmarks(store, fork)?, BTreeSet::from([first]));
        Ok(())
    }
}
