//! Tool-result details and row summaries follow ui-trajectory/layout.ts.
use serde_json::Value;

fn text(block: &Value) -> Option<&str> {
    (block["type"] == "text")
        .then(|| block["text"].as_str())
        .flatten()
}

pub(super) fn summary(blocks: &[Value], failed: bool, error: Option<&Value>) -> String {
    if let Some(value) = blocks
        .iter()
        .find_map(|block| text(block).filter(|value| !value.is_empty()))
    {
        return super::preview(value);
    }
    if failed {
        return error
            .and_then(|value| value["code"].as_str())
            .unwrap_or("error")
            .to_owned();
    }
    let images = blocks
        .iter()
        .filter(|block| block["type"] == "image")
        .count();
    if images > 0 {
        format!("图片 ×{images}")
    } else {
        "无输出".to_owned()
    }
}

pub(super) fn detail(blocks: &[Value], failed: bool, error: Option<&Value>) -> String {
    let value = blocks
        .iter()
        .filter_map(text)
        .collect::<Vec<_>>()
        .join("\n");
    if !value.is_empty()
        && (!failed
            || blocks
                .iter()
                .any(|block| text(block).is_some_and(|value| !value.is_empty())))
    {
        return value;
    }
    if failed {
        return error
            .map(|value| {
                format!(
                    "{}: {}",
                    value["name"].as_str().unwrap_or("undefined"),
                    value["code"].as_str().unwrap_or("undefined")
                )
            })
            .unwrap_or_else(|| "error".to_owned());
    }
    let images = blocks
        .iter()
        .filter(|block| block["type"] == "image")
        .count();
    if images > 0 {
        return format!("图片 ×{images}");
    }
    if blocks.is_empty()
        || blocks
            .iter()
            .all(|block| block["type"] == "text" && text(block).is_none_or(str::is_empty))
    {
        return "无输出".to_owned();
    }
    serde_json::to_string_pretty(blocks).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn retains_block_boundaries_and_image_only_results() {
        let blocks = vec![
            json!({"type":"text","text":"first"}),
            json!({"type":"image","attachment":{"attachmentId":"sha256:a"}}),
            json!({"type":"text","text":"second\n"}),
        ];
        assert_eq!(detail(&blocks, false, None), "first\nsecond\n");
        assert_eq!(summary(&blocks, false, None), "first");
        assert_eq!(detail(&blocks[1..2], false, None), "图片 ×1");
        assert_eq!(detail(&[], false, None), "无输出");
        let empty = [
            json!({"type":"text","text":""}),
            json!({"type":"text","text":""}),
        ];
        assert_eq!(detail(&empty, false, None), "\n");
    }
    #[test]
    fn error_detail_uses_recorded_text_or_failure_name_and_code() {
        let failure = json!({"name":"ToolError","code":"E_IMAGE"});
        let blocks = [json!({"type":"image","attachment":{"attachmentId":"sha256:a"}})];
        assert_eq!(detail(&blocks, true, Some(&failure)), "ToolError: E_IMAGE");
        assert_eq!(summary(&blocks, true, Some(&failure)), "E_IMAGE");
        assert_eq!(
            detail(&[json!({"type":"text","text":" "})], true, Some(&failure)),
            " "
        );
        assert_eq!(detail(&[], true, None), "error");
    }
}
