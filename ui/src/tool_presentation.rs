//! Pure presentation models from durable tool arguments, result content and metadata.
//! Live ACP updates and replayed session events use the same projection.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Result fields the transcript needs after flattening the model-facing text.
#[derive(Clone, Debug, Default)]
pub(crate) struct ToolResult {
    pub(crate) text: String,
    pub(crate) status: String,
    pub(crate) presentation: Option<String>,
}

/// One persisted mutation hunk, not a Markdown diff fence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiffHunk {
    pub(crate) path: String,
    pub(crate) old_text: Option<String>,
    pub(crate) new_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiffRow {
    pub(crate) path: Option<String>,
    pub(crate) kind: DiffRowKind,
    pub(crate) text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiffRowKind {
    Added,
    Removed,
    Context,
    Gap,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DiffCard {
    pub(crate) rows: Vec<DiffRow>,
    pub(crate) added: usize,
    pub(crate) removed: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ToolImage {
    pub(crate) attachment_id: String,
    pub(crate) media_type: String,
    pub(crate) name: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) data_url: Option<String>,
    pub(crate) failed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ImageCard {
    pub(crate) path: String,
    pub(crate) images: Vec<ToolImage>,
    pub(crate) extra_text: String,
}

/// Normalize the same `tool/result` fields carried by ACP and by session.v4.
pub(crate) fn result_projection(data: &Value) -> ToolResult {
    let message = &data["message"];
    ToolResult {
        text: crate::tool_calls::result_text(message),
        status: crate::tool_calls::result_status(data, message).to_owned(),
        presentation: presentation_projection(
            message.get("content").and_then(Value::as_array),
            data.get("meta"),
            data.get("error"),
        ),
    }
}

fn presentation_projection(
    content: Option<&Vec<Value>>,
    meta: Option<&Value>,
    error: Option<&Value>,
) -> Option<String> {
    let mut projection = serde_json::Map::new();
    if let Some(content) = content
        && content.iter().any(|block| block["type"] == "image")
    {
        projection.insert("content".into(), Value::Array(content.clone()));
    }
    if let Some(meta) = meta.filter(|meta| !meta.is_null()) {
        projection.insert("meta".into(), meta.clone());
    }
    if let Some(error) = error.filter(|error| error.is_object()) {
        projection.insert("error".into(), error.clone());
    }
    (!projection.is_empty()).then(|| Value::Object(projection).to_string())
}

/// Official web_fetch transport summary; the raw failure stays in expanded output.
pub(crate) fn web_fetch_failure_summary(
    tool: &str,
    status: &str,
    presentation: Option<&str>,
) -> Option<&'static str> {
    if tool != "web_fetch" || status != "failed" {
        return None;
    }
    let presentation = serde_json::from_str::<Value>(presentation?).ok()?;
    match presentation["error"]["code"].as_str()? {
        "WEB_FETCH_DNS_FAILED" => Some("无法解析域名，请检查网络、代理或 DNS 设置"),
        "WEB_FETCH_CONNECTION_REFUSED" => Some("连接被拒绝，请检查地址、端口或代理"),
        "WEB_FETCH_CONNECTION_TIMEOUT" => Some("连接超时，请检查网络或代理后重试"),
        "WEB_FETCH_CERTIFICATE_INVALID" => Some("安全证书校验失败，请检查系统时间、代理或证书配置"),
        "WEB_FETCH_CONNECTION_INTERRUPTED" => Some("连接中断，请稍后重试"),
        "WEB_FETCH_NETWORK_UNREACHABLE" => Some("网络或主机不可达，请检查网络连接或代理"),
        "WEB_FETCH_NETWORK_ERROR" => Some("网络请求失败，详情见输出"),
        _ => None,
    }
}

/// One call's optional diff and image cards, derived from durable facts only.
pub(crate) fn cards(
    name: &str,
    arguments: &str,
    status: &str,
    nested: bool,
    presentation: Option<&str>,
) -> (Option<DiffCard>, Option<ImageCard>) {
    let parsed = serde_json::from_str::<Value>(arguments).unwrap_or(Value::Null);
    let presentation = presentation.and_then(|json| serde_json::from_str::<Value>(json).ok());
    let meta = presentation.as_ref().and_then(|value| value.get("meta"));
    let settled = !matches!(status, "in_progress" | "pending" | "preparing" | "approval");
    let failed = matches!(
        status,
        "failed" | "ASK_CANCELLED" | "ASK_ABORTED" | "cancelled"
    );
    let intended = (!nested).then(|| intended_diff(name, &parsed)).flatten();
    let applied = if settled && !failed {
        applied_diffs(meta)
    } else {
        None
    };
    let diffs = if nested {
        None
    } else if let Some((tool, intended)) = intended {
        if !settled {
            Some(vec![intended])
        } else if failed || tool == "str_replace_editor" {
            None
        } else {
            match applied {
                Some(Some(diffs)) => {
                    let recorded = meta.and_then(|meta| meta["path"].as_str());
                    Some(
                        diffs
                            .into_iter()
                            .map(|mut diff| {
                                if recorded == Some(diff.path.as_str()) {
                                    diff.path = intended.path.clone();
                                }
                                diff
                            })
                            .collect(),
                    )
                }
                _ if tool == "write" => Some(vec![intended]),
                _ => None,
            }
        }
    } else {
        applied.flatten().filter(|diffs| !diffs.is_empty())
    };
    let diff = diffs.as_deref().map(diff_rows);
    let image = (name == "read_image" && settled && !failed)
        .then(|| {
            image_card(
                &parsed,
                nested,
                meta,
                presentation.as_ref()?.get("content")?.as_array()?,
            )
        })
        .flatten();
    (diff, image)
}

fn intended_diff(name: &str, arguments: &Value) -> Option<(&'static str, DiffHunk)> {
    if name == "str_replace_editor" {
        let path = nonempty(arguments["path"].as_str())?;
        let diff = match arguments["command"].as_str() {
            Some("create") => DiffHunk {
                path,
                old_text: None,
                new_text: string_or_empty(arguments.get("file_text"))?,
            },
            Some("str_replace") => DiffHunk {
                path,
                old_text: optional_string(arguments.get("old_str"))?,
                new_text: string_or_empty(arguments.get("new_str"))?,
            },
            _ => return None,
        };
        return Some(("str_replace_editor", diff));
    }
    let path = nonempty(arguments["file_path"].as_str())?;
    match name {
        "write" => Some((
            "write",
            DiffHunk {
                path,
                old_text: None,
                new_text: arguments["content"].as_str()?.to_owned(),
            },
        )),
        "edit" => {
            if arguments
                .get("replace_all")
                .is_some_and(|value| !value.is_boolean())
            {
                return None;
            }
            let old = arguments["old_string"].as_str()?;
            Some((
                "edit",
                DiffHunk {
                    path,
                    old_text: (!old.is_empty()).then(|| old.to_owned()),
                    new_text: arguments["new_string"].as_str()?.to_owned(),
                },
            ))
        }
        _ => None,
    }
}

fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn string_or_empty(value: Option<&Value>) -> Option<String> {
    Some(value.map_or(Some(""), Value::as_str)?.to_owned())
}

fn optional_string(value: Option<&Value>) -> Option<Option<String>> {
    Some(match value {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_str()?.to_owned()),
    })
}

/// `Some(None)` means a valid, explicitly empty applied diff array.
fn applied_diffs(meta: Option<&Value>) -> Option<Option<Vec<DiffHunk>>> {
    let values = meta?.get("diffs")?.as_array()?;
    let mut diffs = Vec::with_capacity(values.len());
    for value in values {
        diffs.push(serde_json::from_value(value.clone()).ok()?);
    }
    Some((!diffs.is_empty()).then_some(diffs))
}

fn content_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.strip_suffix('\n')
            .unwrap_or(text)
            .split('\n')
            .collect()
    }
}

fn diff_rows(diffs: &[DiffHunk]) -> DiffCard {
    let mut card = DiffCard::default();
    let mut previous_path = None;
    for diff in diffs {
        if previous_path.as_deref() == Some(diff.path.as_str()) {
            card.rows.push(DiffRow {
                path: None,
                kind: DiffRowKind::Gap,
                text: "⋯".into(),
            });
        } else {
            card.rows.push(DiffRow {
                path: Some(diff.path.clone()),
                kind: DiffRowKind::Context,
                text: diff.path.clone(),
            });
        }
        previous_path = Some(diff.path.clone());
        let old = content_lines(diff.old_text.as_deref().unwrap_or(""));
        let new = content_lines(&diff.new_text);
        let old_source = old.join("\n");
        let new_source = new.join("\n");
        let patch = similar::TextDiff::from_lines(&old_source, &new_source);
        let mut previous_change = false;
        for change in patch.iter_all_changes() {
            let kind = match change.tag() {
                similar::ChangeTag::Delete => {
                    card.removed += 1;
                    DiffRowKind::Removed
                }
                similar::ChangeTag::Insert => {
                    card.added += 1;
                    DiffRowKind::Added
                }
                similar::ChangeTag::Equal => DiffRowKind::Context,
            };
            let gap = kind == DiffRowKind::Context && previous_change;
            previous_change = kind != DiffRowKind::Context;
            if gap {
                card.rows.push(DiffRow {
                    path: None,
                    kind: DiffRowKind::Gap,
                    text: "⋯".into(),
                });
            }
            card.rows.push(DiffRow {
                path: None,
                kind,
                text: change.value().trim_end_matches(['\r', '\n']).to_owned(),
            });
        }
    }
    card
}

fn positive(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .filter(|value| *value > 0)
        .and_then(|value| value.try_into().ok())
}

fn image_envelope(text: &str) -> bool {
    text.starts_with("<path>")
        && text.contains("</path>\n<type>image</type>\n<content>\n")
        && text.ends_with("\n</content>")
}

fn image_card(
    arguments: &Value,
    nested: bool,
    meta: Option<&Value>,
    content: &[Value],
) -> Option<ImageCard> {
    if !content.iter().all(|block| {
        block.is_object()
            && (block["type"] == "image" || (block["type"] == "text" && block["text"].is_string()))
    }) {
        return None;
    }
    if !content
        .iter()
        .any(|block| block["type"] == "text" && block["text"].as_str().is_some_and(image_envelope))
    {
        return None;
    }
    let meta_path = meta
        .and_then(|meta| meta.get("path"))
        .and_then(Value::as_str);
    let argument_path = nonempty(arguments["file_path"].as_str())?;
    let path = meta_path
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .or_else(|| nested.then_some(argument_path))?;
    let mut images = Vec::new();
    let mut extra_text = Vec::new();
    for block in content {
        match block["type"].as_str() {
            Some("text") => {
                let text = block["text"].as_str()?;
                if !image_envelope(text) {
                    extra_text.push(text);
                }
            }
            Some("image") => {
                let attachment = block.get("attachment")?;
                let media_type = attachment["mediaType"].as_str()?;
                if !matches!(
                    media_type,
                    "image/png" | "image/jpeg" | "image/webp" | "image/gif"
                ) {
                    return None;
                }
                let id = nonempty(attachment["attachmentId"].as_str())?;
                positive(&attachment["bytes"])?;
                if attachment.get("name").is_some_and(|name| !name.is_string()) {
                    return None;
                }
                if let Some(original) = attachment.get("originalDimensions") {
                    positive(&original["width"])?;
                    positive(&original["height"])?;
                }
                images.push(ToolImage {
                    attachment_id: id,
                    media_type: media_type.into(),
                    name: attachment["name"].as_str().unwrap_or("").into(),
                    width: positive(&attachment["width"])?,
                    height: positive(&attachment["height"])?,
                    data_url: attachment["dataUrl"].as_str().map(str::to_owned),
                    failed: attachment["failed"] == Value::Bool(true),
                });
            }
            _ => return None,
        }
    }
    (!images.is_empty()).then(|| ImageCard {
        path,
        images,
        extra_text: extra_text.join("\n"),
    })
}

/// Return the structured projections and attach newly decoded image bytes.
pub(crate) fn set_image_data(
    presentation: &str,
    attachment_id: &str,
    data_url: &str,
) -> Option<String> {
    let mut value: Value = serde_json::from_str(presentation).ok()?;
    let mut changed = false;
    for block in value.get_mut("content")?.as_array_mut()? {
        let attachment = &mut block["attachment"];
        if attachment["attachmentId"].as_str() == Some(attachment_id) {
            attachment["dataUrl"] = Value::from(data_url.to_owned());
            changed = true;
        }
    }
    changed.then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: Value, result: Value, meta: Value) -> Vec<crate::state::ChatEntry> {
        crate::history::project_session_events(&[
            serde_json::json!({"type":"tool/call","data":{"callId":"call","name":name,"arguments":args.to_string()}}),
            serde_json::json!({"type":"tool/result","data":{"message":{"toolCallId":"call","content":result,"isError":false},"meta":meta}}),
        ])
        .entries
    }

    #[test]
    fn applied_diff_survives_history_and_shares_live_projection() {
        let arguments = serde_json::json!({"file_path":"src/main.rs","old_string":"let a = 1;","new_string":"let a = 2;"});
        let meta = serde_json::json!({"path":"/repo/src/main.rs","diffs":[{"path":"/repo/src/main.rs","oldText":"fn main() {\n    let a = 1;\n}","newText":"fn main() {\n    let a = 2;\n}"}]});
        let result = serde_json::json!([{"type":"text","text":"updated"}]);
        let entries = call("edit", arguments.clone(), result.clone(), meta.clone());
        let card = entries[0].cards().0.expect("applied diff");
        assert_eq!((card.added, card.removed), (1, 1));
        assert_eq!(card.rows[0].text, "src/main.rs");
        assert!(card.rows.iter().any(|row| row.kind == DiffRowKind::Context));
        let projection = result_projection(
            &serde_json::json!({"message":{"content":result,"isError":false},"meta":meta}),
        );
        let live = cards(
            "edit",
            &arguments.to_string(),
            "completed",
            false,
            projection.presentation.as_deref(),
        );
        assert_eq!(live.0, Some(card));
    }

    #[test]
    fn running_and_settled_mutation_rules_match_the_official_model() {
        let write = serde_json::json!({"file_path":"new.txt","content":"hello\n"});
        let running = cards("write", &write.to_string(), "in_progress", false, None)
            .0
            .unwrap();
        assert_eq!(running.added, 1);
        let settled = cards(
            "write",
            &write.to_string(),
            "completed",
            false,
            Some(r#"{"meta":{"diffs":[]}}"#),
        )
        .0
        .unwrap();
        assert_eq!(settled.added, 1);
        assert!(
            cards("write", &write.to_string(), "failed", false, None)
                .0
                .is_none()
        );
        let replace =
            serde_json::json!({"command":"str_replace","path":"a.txt","old_str":"a","new_str":"b"});
        assert!(
            cards(
                "str_replace_editor",
                &replace.to_string(),
                "in_progress",
                false,
                None
            )
            .0
            .is_some()
        );
        assert!(
            cards(
                "str_replace_editor",
                &replace.to_string(),
                "completed",
                false,
                None
            )
            .0
            .is_none()
        );
        assert!(
            cards("edit", &write.to_string(), "in_progress", true, None)
                .0
                .is_none()
        );
        let metadata = serde_json::json!({"diffs":[{"path":"a.txt","oldText":"a","newText":"b"}]});
        let live = result_projection(
            &serde_json::json!({"message":{"content":[],"isError":false},"meta":metadata}),
        );
        assert!(live.presentation.unwrap().contains("oldText"));
    }

    #[test]
    fn read_image_uses_references_not_xml_paths_and_rejects_unknown_blocks() {
        let arguments = serde_json::json!({"file_path":"authored.png"});
        let content = serde_json::json!([
            {"type":"text","text":"<path>/actual.png</path>\n<type>image</type>\n<content>\nimage\n</content>"},
            {"type":"image","attachment":{"attachmentId":"sha256:opaque","mediaType":"image/png","bytes":4,"width":2,"height":3,"name":"actual.png"}},
            {"type":"text","text":"additional note"}
        ]);
        let projection = result_projection(
            &serde_json::json!({"message":{"content":content,"isError":false},"meta":{"path":"/actual.png"}}),
        );
        let image = cards(
            "read_image",
            &arguments.to_string(),
            "completed",
            false,
            projection.presentation.as_deref(),
        )
        .1
        .unwrap();
        assert_eq!(image.path, "/actual.png");
        assert_eq!(image.images[0].width, 2);
        assert_eq!(image.extra_text, "additional note");
        let restored = set_image_data(
            projection.presentation.as_deref().unwrap(),
            "sha256:opaque",
            "data:image/png;base64,AAAA",
        )
        .unwrap();
        assert_eq!(
            cards(
                "read_image",
                &arguments.to_string(),
                "completed",
                false,
                Some(&restored)
            )
            .1
            .unwrap()
            .images[0]
                .data_url
                .as_deref(),
            Some("data:image/png;base64,AAAA")
        );
        let nested = cards(
            "read_image",
            &arguments.to_string(),
            "completed",
            true,
            projection
                .presentation
                .as_deref()
                .map(|presentation| {
                    let mut value: Value = serde_json::from_str(presentation).unwrap();
                    value.as_object_mut().unwrap().remove("meta");
                    value.to_string()
                })
                .as_deref(),
        );
        assert_eq!(nested.1.unwrap().path, "authored.png");
    }

    #[test]
    fn web_fetch_transport_codes_lead_with_the_official_localized_hint() {
        for (code, hint) in [
            (
                "WEB_FETCH_DNS_FAILED",
                "无法解析域名，请检查网络、代理或 DNS 设置",
            ),
            (
                "WEB_FETCH_CONNECTION_REFUSED",
                "连接被拒绝，请检查地址、端口或代理",
            ),
            (
                "WEB_FETCH_CONNECTION_TIMEOUT",
                "连接超时，请检查网络或代理后重试",
            ),
            (
                "WEB_FETCH_CERTIFICATE_INVALID",
                "安全证书校验失败，请检查系统时间、代理或证书配置",
            ),
            ("WEB_FETCH_CONNECTION_INTERRUPTED", "连接中断，请稍后重试"),
            (
                "WEB_FETCH_NETWORK_UNREACHABLE",
                "网络或主机不可达，请检查网络连接或代理",
            ),
            ("WEB_FETCH_NETWORK_ERROR", "网络请求失败，详情见输出"),
        ] {
            let projection = result_projection(&serde_json::json!({
                "message": {"content": [{"type":"text","text":"Error: web fetch failed"}],"isError":true},
                "error": {"name":"WebError","code":code}
            }));
            assert_eq!(
                web_fetch_failure_summary(
                    "web_fetch",
                    "failed",
                    projection.presentation.as_deref()
                ),
                Some(hint)
            );
        }
        let unsupported = result_projection(&serde_json::json!({
            "message":{"content":[],"isError":true},"error":{"code":"WEB_UNSUPPORTED_CONTENT_TYPE"}
        }));
        assert_eq!(
            web_fetch_failure_summary("web_fetch", "failed", unsupported.presentation.as_deref()),
            None
        );
        assert_eq!(
            web_fetch_failure_summary("web_search", "failed", unsupported.presentation.as_deref()),
            None
        );
        assert_eq!(
            web_fetch_failure_summary(
                "web_fetch",
                "completed",
                unsupported.presentation.as_deref()
            ),
            None
        );
    }

    #[test]
    fn malformed_image_data_falls_back_to_generic_output() {
        let arguments = serde_json::json!({"file_path":"authored.png"});
        for attachment in [
            serde_json::json!({"attachmentId":"","mediaType":"image/png","bytes":1,"width":1,"height":1}),
            serde_json::json!({"attachmentId":"id","mediaType":"image/svg+xml","bytes":1,"width":1,"height":1}),
            serde_json::json!({"attachmentId":"id","mediaType":"image/png","bytes":1,"width":0,"height":1}),
        ] {
            let content = serde_json::json!([
                {"type":"text","text":"<path>x</path>\n<type>image</type>\n<content>\nx\n</content>"},
                {"type":"image","attachment":attachment}
            ]);
            let projection = result_projection(&serde_json::json!({"message":{"content":content}}));
            assert!(
                cards(
                    "read_image",
                    &arguments.to_string(),
                    "completed",
                    true,
                    projection.presentation.as_deref()
                )
                .1
                .is_none()
            );
        }
    }
}
