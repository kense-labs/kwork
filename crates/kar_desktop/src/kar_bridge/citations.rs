//! Resolve Codex's inline web references on the desktop's copy of each turn.
//! Kar keeps the original transcript for resuming the provider session.
use kar::{
    OutputPart,
    chat::{ChatTurn, Item, Role, TimelineItem},
    observe::Agent,
};
use markdown::{ParseOptions, mdast::Node, to_mdast};
use std::{collections::BTreeMap, ops::Range};
use url::Url;

const OPEN: char = '\u{e200}';
const FIELD: char = '\u{e202}';
const END: char = '\u{e201}';

type Sources = BTreeMap<String, Url>;

#[derive(Default)]
pub(super) struct CitationCache {
    turns: BTreeMap<uuid::Uuid, CachedTurn>,
}

struct CachedTurn {
    sources: BTreeMap<Agent, Sources>,
    // Only messages containing a native marker need a Markdown parse.
    messages: BTreeMap<(Agent, usize), (String, String)>,
}

impl CitationCache {
    pub(super) fn display_turns(&mut self, mut turns: Vec<ChatTurn>) -> Vec<ChatTurn> {
        let mut retained = BTreeMap::new();
        let mut budget = 4 * 1024 * 1024;
        for turn in turns.iter_mut().rev() {
            if !turn.items.iter().any(|row| {
                matches!(&row.item, Item::Message { role: Role::Assistant | Role::Reasoning, text, .. } if text.contains(OPEN))
            }) {
                continue;
            }
            let sources = search_sources(&turn.items);
            let mut cached = self
                .turns
                .remove(&turn.id)
                .filter(|old| old.sources == sources)
                .unwrap_or_else(|| CachedTurn {
                    sources,
                    messages: BTreeMap::new(),
                });
            // Reconcile against actual bytes: providers may revise prior text or return sources late.
            let mut messages = BTreeMap::new();
            for (index, row) in turn.items.iter_mut().enumerate() {
                if let Item::Message {
                    role: Role::Assistant | Role::Reasoning,
                    text,
                    ..
                } = &mut row.item
                    && text.contains(OPEN)
                {
                    let key = (row.source.clone(), index);
                    let entry = cached
                        .messages
                        .remove(&key)
                        .filter(|(raw, _)| raw == text)
                        .unwrap_or_else(|| {
                            let resolved = resolve(text, cached.sources.get(&row.source));
                            (std::mem::take(text), resolved)
                        });
                    text.clone_from(&entry.1);
                    messages.insert(key, entry);
                }
            }
            let bytes = messages
                .values()
                .map(|(raw, display)| raw.len() + display.len())
                .sum::<usize>()
                + cached
                    .sources
                    .values()
                    .flat_map(|m| m.iter())
                    .map(|(id, url)| id.len() + url.as_str().len())
                    .sum::<usize>();
            if bytes <= budget {
                budget -= bytes;
                cached.messages = messages;
                retained.insert(turn.id, cached);
            }
        }
        self.turns = retained;
        turns
    }
}

fn search_sources(items: &[TimelineItem]) -> BTreeMap<Agent, Sources> {
    let mut by_agent = BTreeMap::<Agent, Sources>::new();
    for row in items {
        let Item::Tool { activity, .. } = &row.item else {
            continue;
        };
        if activity.name.as_deref() != Some("webSearch") {
            continue;
        }
        let Some(parts) = activity.result.as_ref().and_then(|r| r.content.as_ref()) else {
            continue;
        };
        for part in parts {
            let OutputPart::Other { value } = part else {
                continue;
            };
            let Some(id) = value["ref_id"].as_str() else {
                continue;
            };
            let Some(url) = value["url"].as_str().and_then(|s| Url::parse(s).ok()) else {
                continue;
            };
            if matches!(url.scheme(), "https" | "http")
                && url.domain().is_some()
                && url.username().is_empty()
                && url.password().is_none()
            {
                by_agent
                    .entry(row.source.clone())
                    .or_default()
                    .insert(id.to_owned(), url);
            }
        }
    }
    by_agent
}

/// Rewrite prose by original byte offsets so that code examples and existing Markdown
/// destinations are kept verbatim. The overwhelmingly common unmarked reply never gets parsed.
fn resolve(text: &str, sources: Option<&Sources>) -> String {
    fn prose_ranges(node: &Node, inside_link: bool, ranges: &mut Vec<(Range<usize>, bool)>) {
        if let Node::Text(text) = node {
            if text.value.contains(OPEN)
                && let Some(pos) = &text.position
            {
                ranges.push((pos.start.offset..pos.end.offset, inside_link));
            }
        } else if let Some(children) = node.children() {
            for child in children {
                prose_ranges(
                    child,
                    inside_link || matches!(node, Node::Link(_) | Node::LinkReference(_)),
                    ranges,
                );
            }
        }
    }
    let mut ranges = Vec::new();
    if let Ok(doc) = to_mdast(text, &ParseOptions::gfm()) {
        prose_ranges(&doc, false, &mut ranges);
    }
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for (range, inside_link) in ranges {
        out.push_str(&text[copied..range.start]);
        resolve_prose(
            &text[range.clone()],
            if inside_link { None } else { sources },
            &mut out,
        );
        copied = range.end;
    }
    out.push_str(&text[copied..]);
    out
}

fn resolve_prose(mut prose: &str, sources: Option<&Sources>, out: &mut String) {
    while let Some((before, tail)) = prose.split_once(OPEN) {
        out.push_str(before);
        if "cite".starts_with(tail) {
            return; // The next delta may finish this control word.
        }
        let Some(fields) = tail
            .strip_prefix("cite")
            .filter(|s| s.starts_with([FIELD, END]))
        else {
            out.push(OPEN);
            prose = tail;
            continue;
        };
        prose = fields;
        let mut links = Vec::new();
        while let Some(field) = prose.strip_prefix(FIELD) {
            let n = field
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
                .count();
            let (id, rest) = field.split_at(n);
            if let Some(url) = sources.and_then(|map| map.get(id))
                && !links.contains(&url)
            {
                links.push(url);
            }
            prose = rest;
        }
        // An unfinished reference remains invisible while streaming. If interrupted,
        // the next ordinary character (including a newline) still belongs to the reply.
        if let Some(rest) = prose.strip_prefix(END) {
            prose = rest;
            for url in links {
                if !out.is_empty() && !out.ends_with(char::is_whitespace) {
                    out.push(' ');
                }
                let domain = url.domain().expect("indexed web source has a domain");
                out.push('[');
                out.push_str(domain.strip_prefix("www.").unwrap_or(domain));
                out.push_str("](<");
                // Angle-delimited Markdown destinations can contain balanced or unmatched parentheses.
                out.push_str(&url.as_str().replace('<', "%3C").replace('>', "%3E"));
                out.push_str(">)");
            }
        }
    }
    out.push_str(prose);
}

#[cfg(test)]
mod tests {
    use super::*;
    use kar::{ToolResult, observe::Tool};
    use serde_json::json;

    fn agent(id: &str) -> Agent {
        Agent {
            session_id: Some(id.into()),
            path: vec![],
        }
    }

    fn result(source: &str, id: &str, href: &str) -> TimelineItem {
        TimelineItem {
            source: agent(source),
            item: Item::Tool {
                id: "search".into(),
                activity: Tool {
                    name: Some("webSearch".into()),
                    result: Some(ToolResult {
                        content: Some(vec![OutputPart::Other {
                            value: json!({"type":"text_result", "ref_id":id,"url":href}),
                        }]),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            },
        }
    }

    #[test]
    fn cites_only_matching_agents_web_results_and_uses_clickable_markdown() {
        let map = search_sources(&[
            result("root", "turn0search8", "https://www.example.com/page_(1)"),
            result("child", "turn0search8", "https://other.example/"),
            result("root", "turn0search3", "javascript:alert(1)"),
        ]);
        let sources = map.get(&agent("root"));
        let cite =
            format!("{OPEN}cite{FIELD}turn0search8{FIELD}turn0search8{FIELD}turn0search3{END}");
        let visible = resolve(
            &format!("答案。{cite} 后文 [官网](https://example.net/)"),
            sources,
        );
        assert_eq!(
            visible,
            "答案。 [example.com](<https://www.example.com/page_(1)>) 后文 [官网](https://example.net/)"
        );
        let doc = to_mdast(&visible, &ParseOptions::gfm()).unwrap();
        assert!(
            matches!(&doc.children().unwrap()[0].children().unwrap()[1],Node::Link(link) if link.url == "https://www.example.com/page_(1)")
        );
        assert_eq!(resolve(&cite, None), "");
        assert_eq!(
            resolve(&format!("[现有标签{cite}](https://example.org/)"), sources),
            "[现有标签](https://example.org/)"
        );
    }

    #[test]
    fn split_stream_controls_never_show_in_prose_but_code_and_unrelated_glyphs_survive() {
        let cite = format!("{OPEN}cite{FIELD}turn0search8{END}");
        for (end, _) in cite.char_indices() {
            assert_eq!(resolve(&format!("前{}", &cite[..end]), None), "前");
        }
        assert_eq!(resolve(&format!("前{cite}后"), None), "前后");
        assert_eq!(
            resolve(&format!("前{OPEN}cite{FIELD}turn0search\n\n后"), None),
            "前\n\n后"
        );
        let code = format!("`{cite}`\n\n~~~text\n{cite}\n~~~\n\n{OPEN}unrelated");
        assert_eq!(resolve(&code, None), code);
    }
}
