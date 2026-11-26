//! Live Model Context Protocol (`kar::serve_mcp_tools`) bridge connecting Kar CLI agents
//! directly to open Kantor Office (`DocsView`, `SheetsView`, `SlidesView`) tabs in the window.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use docs_engine::{Position as DocPos, Selection as DocSel, agent::NewParagraph};
use gpui::*;
use kar::host_tools::{HostTool, HostToolContent, HostToolResult, ServedMcpTools};
use serde_json::{Map, Value, json};
use sheets_engine::{CellRange, cell_name, formula::parse_range};
use tokio::sync::oneshot;

use crate::{
    kar_bridge::KarBridgeEvent,
    office_host::{OfficeEditorView, OfficeTab, clean_doc_para_text},
    ui_primitives::clean_display_doc_name,
    workbench::KarWorkbench,
};

const MCP_INSTRUCTIONS: &str = "\
Kar Desktop hosts live Kantor Office editors (`Word .docx`, `Excel .xlsx`, `PPT .pptx`) AND native KUI Interactive Briefs beside this chat. \
Use `office_list_tabs` and `office_get_selection` to inspect open documents and their monotonic `revision`, \
`office_read_*` / `office_edit_*` / `office_write_excel` (passing optional `operation_id` and `expected_revision` for safe revision-fenced co-editing), \
and `kar_write_brief` / `kar_read_brief` to render pure-GPUI KUI interactive charts, Sankey flows, rolling KPIs, and review forms directly in the chat.";

type McpReplyTx = oneshot::Sender<Result<String, String>>;

#[derive(Clone)]
pub struct OfficeMcpCall {
    pub tool: String,
    pub args: Map<String, Value>,
    reply: Arc<Mutex<Option<McpReplyTx>>>,
}

impl std::fmt::Debug for OfficeMcpCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OfficeMcpCall")
            .field("tool", &self.tool)
            .field("args", &self.args)
            .finish()
    }
}

impl OfficeMcpCall {
    pub fn respond(self, result: Result<String, String>) {
        if let Ok(mut guard) = self.reply.lock()
            && let Some(tx) = guard.take()
        {
            let _ = tx.send(result);
        }
    }
}

fn schema(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

fn make_tool(
    name: &'static str,
    description: &'static str,
    input_schema: Value,
    evt_tx: async_channel::Sender<KarBridgeEvent>,
) -> HostTool {
    let tool_name = name.to_owned();
    let mut tool = HostTool::new(
        name,
        schema(input_schema),
        move |args, cancel| {
            let evt_tx = evt_tx.clone();
            let tool_name = tool_name.clone();
            async move {
                let (reply_tx, reply_rx) = oneshot::channel();
                let call = OfficeMcpCall {
                    tool: tool_name,
                    args,
                    reply: Arc::new(Mutex::new(Some(reply_tx))),
                };
                if evt_tx
                    .send(KarBridgeEvent::OfficeMcpCall(call))
                    .await
                    .is_err()
                {
                    return Err("桌面工作台未连接".into());
                }
                tokio::select! {
                    _ = cancel.cancelled() => Err("操作已取消".into()),
                    res = tokio::time::timeout(Duration::from_secs(10), reply_rx) => {
                        match res {
                            Ok(Ok(Ok(text))) => Ok(HostToolResult {
                                content: vec![HostToolContent::Text { text }],
                                is_error: false,
                            }),
                            Ok(Ok(Err(err))) => Ok(HostToolResult {
                                content: vec![HostToolContent::Text { text: err }],
                                is_error: true,
                            }),
                            Ok(Err(_)) => Err("桌面文档窗口当前未打开".into()),
                            Err(_) => Err("等待桌面文档响应超时".into()),
                        }
                    }
                }
            }
        },
    );
    tool.description = Some(description.to_owned());
    tool
}

pub(crate) const OFFICE_SERVER_NAME: &str = "kantor";

pub(crate) fn office_tool_specs() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        (
            "office_list_tabs",
            "List open Word (.docx), Excel (.xlsx), and PowerPoint (.pptx) tabs in the Kar Desktop Office pane.",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        ),
        (
            "office_get_selection",
            "Read the active Office tab's current cursor/selection (Word paragraph, Excel cell/range + formula, or PPT slide shape).",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        ),
        (
            "office_read_word",
            "Read paragraphs (1-based index, style, text) from an open Word (.docx) tab.",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": { "type": "integer", "description": "Optional tab_id from office_list_tabs; defaults to active Word tab." },
                    "start_paragraph": { "type": "integer", "description": "1-based starting paragraph (default 1)." },
                    "limit": { "type": "integer", "description": "Max paragraphs to return (default 40)." }
                },
                "additionalProperties": false
            }),
        ),
        (
            "office_edit_word",
            "Replace a 1-based paragraph's text or insert new paragraphs in an open Word (.docx) tab as one undoable step.",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": { "type": "integer", "description": "Optional tab_id; defaults to active Word tab." },
                    "operation_id": { "type": "string", "description": "Optional UUID for idempotent replay protection." },
                    "expected_revision": { "type": "integer", "description": "Optional revision fence from `office_list_tabs` / `office_read_word`; prevents overwriting concurrent human edits." },
                    "paragraph": { "type": "integer", "description": "1-based paragraph index to replace." },
                    "text": { "type": "string", "description": "Replacement text when `paragraph` is set." },
                    "insert_after": { "type": "integer", "description": "0 to insert at top, or 1-based paragraph index after which to insert `paragraphs`." },
                    "paragraphs": { "type": "array", "items": { "type": "string" }, "description": "New paragraphs to insert when `insert_after` is provided." }
                },
                "additionalProperties": false
            }),
        ),
        (
            "office_read_excel",
            "Read sheet names and cell formulas/computed values for an A1 range in an open Excel (.xlsx) tab.",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": { "type": "integer", "description": "Optional tab_id; defaults to active Excel tab." },
                    "operation_id": { "type": "string", "description": "Optional UUID for idempotent replay protection." },
                    "expected_revision": { "type": "integer", "description": "Optional revision fence from `office_list_tabs` / `office_read_excel`; prevents overwriting concurrent human edits." },
                    "sheet_index": { "type": "integer", "description": "0-based sheet index (default 0)." },
                    "range": { "type": "string", "description": "A1 cell or range such as `A1:F20` (default `A1:H20`)." }
                },
                "additionalProperties": false
            }),
        ),
        (
            "office_write_excel",
            "Write values or `=...` formulas into cells of an open Excel (.xlsx) tab, recalculating formulas immediately.",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": { "type": "integer", "description": "Optional tab_id; defaults to active Excel tab." },
                    "operation_id": { "type": "string", "description": "Optional UUID for idempotent replay protection." },
                    "expected_revision": { "type": "integer", "description": "Optional revision fence from `office_list_tabs` / `office_read_excel`; prevents overwriting concurrent human edits." },
                    "sheet_index": { "type": "integer", "description": "0-based sheet index (default 0)." },
                    "cell": { "type": "string", "description": "Single A1 cell address (e.g. `B4`) when writing one cell." },
                    "value": { "type": "string", "description": "Raw input or formula (e.g. `=SUM(B2:B3)`) when `cell` is set." },
                    "edits": {
                        "type": "array",
                        "description": "Batch of cell writes `[{ \"cell\": \"B4\", \"value\": \"=SUM(B2:B3)\" }]`.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "cell": { "type": "string" },
                                "value": { "type": "string" }
                            },
                            "required": ["cell", "value"]
                        }
                    }
                },
                "additionalProperties": false
            }),
        ),
        (
            "office_read_ppt",
            "Read slide count and shape texts/positions (cm) from an open PowerPoint (.pptx) tab.",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": { "type": "integer", "description": "Optional tab_id; defaults to active PPT tab." },
                    "slide": { "type": "integer", "description": "1-based slide number (default 1)." }
                },
                "additionalProperties": false
            }),
        ),
        (
            "office_edit_ppt",
            "Update shape text and/or `(x_cm, y_cm)` coordinates on a 1-based slide in an open PowerPoint (.pptx) tab.",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": { "type": "integer", "description": "Optional tab_id; defaults to active PPT tab." },
                    "operation_id": { "type": "string", "description": "Optional UUID for idempotent replay protection." },
                    "expected_revision": { "type": "integer", "description": "Optional revision fence from `office_list_tabs` / `office_read_ppt`." },
                    "slide": { "type": "integer", "description": "1-based slide number (default 1)." },
                    "shape_id": { "type": "integer", "description": "Shape ID from `office_read_ppt`." },
                    "text": { "type": "string", "description": "Optional new text for the shape." },
                    "x_cm": { "type": "number", "description": "Optional new X position in cm." },
                    "y_cm": { "type": "number", "description": "Optional new Y position in cm." }
                },
                "required": ["shape_id"],
                "additionalProperties": false
            }),
        ),
        (
            "office_save_tab",
            "Save an open Word, Excel, or PowerPoint tab to disk.",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": { "type": "integer", "description": "Optional tab_id; defaults to active tab." }
                },
                "additionalProperties": false
            }),
        ),
        (
            "kar_write_brief",
            "Create or revise a native GPUI + KUI Interactive Brief card in the current conversation (AnimatedCounter metrics, BarChart, DonutChart, Sparkline, SankeyFlow, Pipeline DataFlow, Callout, Steps, and revision-pinned ReviewForm).",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Optional stable brief ID when revising an existing brief." },
                    "expected_revision": { "type": "integer", "description": "Required when revising an existing brief (`1..=latest_revision`)." },
                    "title": { "type": "string", "description": "Brief card title." },
                    "summary": { "type": "string", "description": "Concise one-line subtitle." },
                    "badge": { "type": "string", "description": "Optional short tag badge." },
                    "blocks": {
                        "type": "array",
                        "description": "Ordered KUI blocks (`metrics`, `bar_chart`, `donut_chart`, `sparkline`, `sankey`, `pipeline`, `callout`, `steps`, `review_form`).",
                        "items": { "type": "object" }
                    }
                },
                "required": ["title", "blocks"],
                "additionalProperties": true
            }),
        ),
        (
            "kar_read_brief",
            "Read a session-owned KUI Interactive Brief and its latest revision before revising.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Brief ID; omit to list all briefs in this session." },
                    "revision": { "type": "integer", "description": "Optional historical revision number." }
                },
                "additionalProperties": false
            }),
        ),
    ]
}

/// Start the loopback HTTP+SSE MCP server exposing live Kantor Office tools.
pub async fn start_office_mcp_server(
    evt_tx: async_channel::Sender<KarBridgeEvent>,
) -> kar::Result<ServedMcpTools> {
    let tools = office_tool_specs().into_iter()
        .map(|(name, description, input_schema)| make_tool(name, description, input_schema, evt_tx.clone()))
        .collect();

    kar::serve_mcp_tools(OFFICE_SERVER_NAME, tools, Some(MCP_INSTRUCTIONS.to_owned())).await
}

impl KarWorkbench {
    pub(crate) fn handle_office_mcp_call(
        &mut self,
        call: OfficeMcpCall,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = call.tool.clone();
        let args = call.args.clone();
        let result = self.execute_office_mcp_tool(&name, &args, window, cx);
        call.respond(result);
        cx.notify();
    }

    fn resolve_mcp_tab(
        &self,
        args: &Map<String, Value>,
        expected_kind: Option<crate::types::OfficeKind>,
    ) -> Result<OfficeTab, String> {
        if let Some(id) = args.get("tab_id").and_then(Value::as_u64) {
            let tab = self
                .office_tabs
                .iter()
                .find(|t| t.id == id)
                .cloned()
                .ok_or_else(|| format!("未找到 tab_id={id} 的文档标签"))?;
            if let Some(kind) = expected_kind
                && tab.kind != kind
            {
                return Err(format!("标签 #{id} 不是 {} 文档", kind.label()));
            }
            return Ok(tab);
        }
        if let Some(active) = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .filter(|t| expected_kind.is_none_or(|k| t.kind == k))
        {
            return Ok(active.clone());
        }
        if let Some(kind) = expected_kind
            && let Some(any_matching) = self.office_tabs.iter().find(|t| t.kind == kind)
        {
            return Ok(any_matching.clone());
        }
        Err("当前没有打开匹配的 Office 文档标签".into())
    }

    fn execute_office_mcp_tool(
        &mut self,
        name: &str,
        args: &Map<String, Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        match name {
            "office_list_tabs" => {
                let mut items: Vec<Value> = Vec::with_capacity(self.office_tabs.len());
                for (idx, t) in self.office_tabs.iter().enumerate() {
                    let doc_name = clean_display_doc_name(&t.doc_name(cx));
                    let rev = self.fence_ledger.current_revision(&doc_name);
                    items.push(json!({
                        "tab_id": t.id,
                        "name": doc_name,
                        "kind": t.kind.short_tag(),
                        "revision": rev,
                        "path": t.path(cx).map(|p| p.display().to_string()),
                        "is_active": self.active_office_tab == Some(idx),
                        "is_dirty": t.is_dirty(cx),
                        "is_sample": t.is_demo,
                    }));
                }
                serde_json::to_string_pretty(&json!({ "tabs": items }))
                    .map_err(|e| e.to_string())
            }
            "office_get_selection" => {
                let tab = self.resolve_mcp_tab(args, None)?;
                let sel = tab.capture_selection(cx);
                serde_json::to_string_pretty(&sel).map_err(|e| e.to_string())
            }
            "office_read_word" => {
                let tab = self.resolve_mcp_tab(args, Some(crate::types::OfficeKind::Word))?;
                let OfficeEditorView::Word(ref e) = tab.view else {
                    return Err("不是 Word 文档".into());
                };
                let start_1 = args
                    .get("start_paragraph")
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .max(1) as usize;
                let limit = args
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(40)
                    .clamp(1, 200) as usize;
                let payload = e.read(cx).agent_read(|doc, _| {
                    let total = doc.len();
                    let start_0 = (start_1 - 1).min(total);
                    let end_0 = (start_0 + limit).min(total);
                    let paras: Vec<Value> = (start_0..end_0)
                        .map(|idx| {
                            let style = doc
                                .paragraph(idx)
                                .props
                                .style
                                .as_deref()
                                .map_or_else(|| "Normal".to_owned(), |s| doc.styles.display_name(s));
                            json!({
                                "paragraph": idx + 1,
                                "style": style,
                                "text": clean_doc_para_text(doc, idx),
                            })
                        })
                        .collect();
                    json!({
                        "tab_id": tab.id,
                        "document": clean_display_doc_name(&tab.doc_name(cx)),
                        "total_paragraphs": total,
                        "paragraphs": paras,
                    })
                });
                serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())
            }
            "office_edit_word" => {
                let tab = self.resolve_mcp_tab(args, Some(crate::types::OfficeKind::Word))?;
                let doc_key = clean_display_doc_name(&tab.doc_name(cx));
                let op_id = args
                    .get("operation_id")
                    .and_then(Value::as_str)
                    .and_then(|s| uuid::Uuid::parse_str(s).ok());
                let exp_rev = args.get("expected_revision").and_then(Value::as_u64);
                let now_ms = crate::remote_link::now_epoch_ms();
                if let Some(para_1) = args.get("paragraph").and_then(Value::as_u64) {
                    let text = args
                        .get("text")
                        .and_then(Value::as_str)
                        .ok_or_else(|| "指定 `paragraph` 时需要提供 `text`".to_owned())?;
                    let idx = para_1.checked_sub(1).ok_or("段落序号从 1 开始")? as usize;
                    let cur_summary = match &tab.view {
                        OfficeEditorView::Word(e) => e
                            .read(cx)
                            .agent_read(|doc, _| clean_doc_para_text(doc, idx)),
                        _ => String::new(),
                    };
                    match self.fence_ledger.check_admission(
                        &doc_key,
                        crate::types::OfficeKind::Word,
                        op_id,
                        exp_rev,
                        format!("段落 #{para_1}"),
                        cur_summary,
                        text.to_owned(),
                        Some(crate::coedit::FenceMutationPayload::WordParagraph {
                            paragraph_1based: para_1 as usize,
                            text: text.to_owned(),
                        }),
                        now_ms,
                    ) {
                        crate::coedit::FenceAdmission::Duplicate(rec) => {
                            return Ok(serde_json::to_string_pretty(&rec).unwrap_or_default());
                        }
                        crate::coedit::FenceAdmission::BlockedConflict(conflict) => {
                            self.fence_ledger.save(&self.config.docs_dir);
                            return Ok(json!({
                                "status": "fenced_conflict",
                                "operation_id": conflict.operation_id,
                                "expected_revision": conflict.expected_revision,
                                "live_revision": conflict.live_revision,
                                "message": "文档已被人工更新，本次修改已被 Revision Fence 拦截并挂入协同裁决面板，未覆盖人工内容"
                            })
                            .to_string());
                        }
                        crate::coedit::FenceAdmission::Proceed {
                            operation_id,
                            base_revision,
                        } => {
                            tab.replace_word_paragraph(idx, text, window, cx)
                                .map_err(|e| e.to_string())?;
                            let new_rev = self.fence_ledger.commit_applied(
                                &doc_key,
                                crate::types::OfficeKind::Word,
                                operation_id,
                                base_revision,
                                text,
                                now_ms,
                            );
                            self.fence_ledger.save(&self.config.docs_dir);
                            return Ok(json!({
                                "status": "updated",
                                "operation_id": operation_id,
                                "tab_id": tab.id,
                                "paragraph": para_1,
                                "revision": new_rev,
                            })
                            .to_string());
                        }
                    }
                }
                if let Some(paras_arr) = args.get("paragraphs").and_then(Value::as_array) {
                    let after_raw = args.get("insert_after").and_then(Value::as_u64).unwrap_or(0);
                    let items: Vec<NewParagraph> = paras_arr
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|s| NewParagraph {
                            text: s.to_owned(),
                            style: None,
                            list: None,
                            level: 0,
                        })
                        .collect();
                    if items.is_empty() {
                        return Err("`paragraphs` 不能为空".into());
                    }
                    let OfficeEditorView::Word(ref e) = tab.view else {
                        return Err("不是 Word 文档".into());
                    };
                    let count = items.len();
                    e.update(cx, |v, cx| {
                        v.agent_edit("智能体插入段落", window, cx, |doc, sel| {
                            let after = if after_raw == 0 {
                                None
                            } else {
                                Some(((after_raw - 1) as usize).min(doc.len().saturating_sub(1)))
                            };
                            let r = doc
                                .insert_paragraphs(after, &items, sel)
                                .map_err(anyhow::Error::msg)?;
                            let end = doc.paragraph(r.end - 1).text.len();
                            Ok((
                                (),
                                DocSel {
                                    anchor: DocPos::new(r.start, 0),
                                    head: DocPos::new(r.end - 1, end),
                                },
                            ))
                        })
                    })
                    .map_err(|e| e.to_string())?;
                    return Ok(json!({
                        "status": "inserted",
                        "tab_id": tab.id,
                        "inserted_count": count,
                    })
                    .to_string());
                }
                Err("请提供 (`paragraph` + `text`) 或 (`insert_after` + `paragraphs`)".into())
            }
            "office_read_excel" => {
                let tab = self.resolve_mcp_tab(args, Some(crate::types::OfficeKind::Excel))?;
                let OfficeEditorView::Excel(ref e) = tab.view else {
                    return Err("不是 Excel 表格".into());
                };
                if e.read(cx).is_busy() {
                    return Err("该工作簿正在后台加载或重算，请稍候片刻重试".into());
                }
                let sheet = args.get("sheet_index").and_then(Value::as_u64).unwrap_or(0) as u32;
                let range_str = args
                    .get("range")
                    .and_then(Value::as_str)
                    .unwrap_or("A1:H20");
                let parsed = parse_range(range_str)
                    .ok_or_else(|| format!("无效的单元格范围 `{range_str}`"))?;
                let payload = e.read(cx).agent_read(cx, |book, _| {
                    let names = book.sheet_names();
                    let max_r = parsed.bottom.min(parsed.top + 60);
                    let max_c = parsed.right.min(parsed.left + 20);
                    let mut cells = Vec::new();
                    for r in parsed.top..=max_r {
                        for c in parsed.left..=max_c {
                            let raw = book.input(sheet, r, c);
                            let val = book.display_value(sheet, r, c);
                            if !raw.is_empty() || !val.is_empty() {
                                cells.push(json!({
                                    "cell": cell_name(r, c),
                                    "input": raw,
                                    "value": val,
                                }));
                            }
                        }
                    }
                    json!({
                        "tab_id": tab.id,
                        "workbook": clean_display_doc_name(&tab.doc_name(cx)),
                        "sheets": names,
                        "sheet_index": sheet,
                        "range": range_str,
                        "non_empty_cells": cells,
                    })
                });
                serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())
            }
            "office_write_excel" => {
                let tab = self.resolve_mcp_tab(args, Some(crate::types::OfficeKind::Excel))?;
                let OfficeEditorView::Excel(ref e) = tab.view else {
                    return Err("不是 Excel 表格".into());
                };
                if e.read(cx).is_busy() {
                    return Err("该工作簿正在后台加载或重算，请稍候片刻重试".into());
                }
                let sheet = args.get("sheet_index").and_then(Value::as_u64).unwrap_or(0) as u32;
                let mut pairs: Vec<(String, String)> = Vec::new();
                if let (Some(cell), Some(val)) = (
                    args.get("cell").and_then(Value::as_str),
                    args.get("value").and_then(Value::as_str),
                ) {
                    pairs.push((cell.to_owned(), val.to_owned()));
                }
                if let Some(arr) = args.get("edits").and_then(Value::as_array) {
                    for item in arr {
                        if let (Some(c), Some(v)) = (
                            item.get("cell").and_then(Value::as_str),
                            item.get("value").and_then(Value::as_str),
                        ) {
                            pairs.push((c.to_owned(), v.to_owned()));
                        }
                    }
                }
                if pairs.is_empty() {
                    return Err("请提供 (`cell` + `value`) 或 `edits` 数组".into());
                }
                let doc_key = clean_display_doc_name(&tab.doc_name(cx));
                let op_id = args
                    .get("operation_id")
                    .and_then(Value::as_str)
                    .and_then(|s| uuid::Uuid::parse_str(s).ok());
                let exp_rev = args.get("expected_revision").and_then(Value::as_u64);
                let now_ms = crate::remote_link::now_epoch_ms();
                let (first_cell, first_val) = &pairs[0];
                let mut writes: Vec<(u32, u32, String)> = Vec::with_capacity(pairs.len());
                let (mut min_r, mut min_c, mut max_r, mut max_c) = (u32::MAX, u32::MAX, 0, 0);
                for (a1, val) in &pairs {
                    let rg =
                        parse_range(a1).ok_or_else(|| format!("无效的单元格地址 `{a1}`"))?;
                    min_r = min_r.min(rg.top);
                    min_c = min_c.min(rg.left);
                    max_r = max_r.max(rg.top);
                    max_c = max_c.max(rg.left);
                    writes.push((rg.top, rg.left, val.clone()));
                }
                let cur_summary = e
                    .read(cx)
                    .agent_read(cx, |book, _| book.display_value(sheet, writes[0].0, writes[0].1));
                let (admitted_op, base_rev) = match self.fence_ledger.check_admission(
                    &doc_key,
                    crate::types::OfficeKind::Excel,
                    op_id,
                    exp_rev,
                    format!("Sheet {} · {first_cell}", sheet + 1),
                    cur_summary,
                    first_val.clone(),
                    Some(crate::coedit::FenceMutationPayload::ExcelCell {
                        sheet_idx: sheet,
                        cell_a1: first_cell.clone(),
                        value: first_val.clone(),
                    }),
                    now_ms,
                ) {
                    crate::coedit::FenceAdmission::Duplicate(rec) => {
                        return Ok(serde_json::to_string_pretty(&rec).unwrap_or_default());
                    }
                    crate::coedit::FenceAdmission::BlockedConflict(conflict) => {
                        self.fence_ledger.save(&self.config.docs_dir);
                        return Ok(json!({
                            "status": "fenced_conflict",
                            "operation_id": conflict.operation_id,
                            "expected_revision": conflict.expected_revision,
                            "live_revision": conflict.live_revision,
                            "message": "工作簿已被人工更新，本次写入已被 Revision Fence 拦截并挂入裁决栏"
                        })
                        .to_string());
                    }
                    crate::coedit::FenceAdmission::Proceed {
                        operation_id,
                        base_revision,
                    } => (operation_id, base_revision),
                };
                let highlight = CellRange::from_corners((min_r, min_c), (max_r, max_c));
                let coords: Vec<(u32, u32, String)> = writes
                    .iter()
                    .map(|(r, c, _)| (*r, *c, cell_name(*r, *c)))
                    .collect();
                e.update(cx, |v, cx| {
                    v.agent_edit(
                        "智能体写入单元格",
                        Some((sheet, highlight)),
                        window,
                        cx,
                        move |book| book.set_inputs(sheet, &writes),
                    )
                })
                .map_err(|e| e.to_string())?;
                let new_rev = self.fence_ledger.commit_applied(
                    &doc_key,
                    crate::types::OfficeKind::Excel,
                    admitted_op,
                    base_rev,
                    first_val,
                    now_ms,
                );
                self.fence_ledger.save(&self.config.docs_dir);
                let results: Vec<Value> = e.read(cx).agent_read(cx, |book, _| {
                    coords
                        .iter()
                        .map(|(r, c, a1)| {
                            json!({
                                "cell": a1,
                                "value": book.display_value(sheet, *r, *c),
                            })
                        })
                        .collect()
                });
                serde_json::to_string_pretty(&json!({
                    "status": "updated",
                    "operation_id": admitted_op,
                    "revision": new_rev,
                    "tab_id": tab.id,
                    "cells": results,
                }))
                .map_err(|e| e.to_string())
            }
            "office_read_ppt" => {
                let tab = self.resolve_mcp_tab(args, Some(crate::types::OfficeKind::Ppt))?;
                let OfficeEditorView::Ppt(ref e) = tab.view else {
                    return Err("不是 PPT 演示文稿".into());
                };
                let slide_1 = args.get("slide").and_then(Value::as_u64).unwrap_or(1).max(1) as usize;
                let payload = e.read(cx).agent_read(|pres, _| {
                    let total = pres.len();
                    let s_idx = (slide_1 - 1).min(total.saturating_sub(1));
                    let shapes: Vec<Value> = if total == 0 {
                        Vec::new()
                    } else {
                        pres.slide(s_idx)
                            .shapes
                            .iter()
                            .enumerate()
                            .map(|(ix, sh)| {
                                let xfrm =
                                    sh.xfrm.or_else(|| pres.placed((s_idx, ix)).map(|p| p.xfrm));
                                let cm = xfrm.map(|x| {
                                    json!({
                                        "x_cm": (x.x as f64 / 360_000.0 * 100.0).round() / 100.0,
                                        "y_cm": (x.y as f64 / 360_000.0 * 100.0).round() / 100.0,
                                        "w_cm": (x.cx as f64 / 360_000.0 * 100.0).round() / 100.0,
                                        "h_cm": (x.cy as f64 / 360_000.0 * 100.0).round() / 100.0,
                                    })
                                });
                                json!({
                                    "shape_id": sh.id,
                                    "name": sh.name,
                                    "text": sh.text.as_ref().map(|t| t.plain_text()).unwrap_or_default(),
                                    "bounds": cm,
                                })
                            })
                            .collect()
                    };
                    json!({
                        "tab_id": tab.id,
                        "presentation": clean_display_doc_name(&tab.doc_name(cx)),
                        "slide_count": total,
                        "slide": s_idx + 1,
                        "shapes": shapes,
                    })
                });
                serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())
            }
            "office_edit_ppt" => {
                let tab = self.resolve_mcp_tab(args, Some(crate::types::OfficeKind::Ppt))?;
                let slide_1 = args.get("slide").and_then(Value::as_u64).unwrap_or(1).max(1) as usize;
                let shape_id = args
                    .get("shape_id")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| "缺少 `shape_id`".to_owned())? as u32;
                let new_text = args.get("text").and_then(Value::as_str);
                let new_xy = match (
                    args.get("x_cm").and_then(Value::as_f64),
                    args.get("y_cm").and_then(Value::as_f64),
                ) {
                    (Some(x), Some(y)) => Some((x, y)),
                    _ => None,
                };
                tab.update_ppt_shape(slide_1 - 1, shape_id, new_text, new_xy, window, cx)
                    .map_err(|e| e.to_string())?;
                Ok(json!({
                    "status": "updated",
                    "tab_id": tab.id,
                    "slide": slide_1,
                    "shape_id": shape_id,
                })
                .to_string())
            }
            "office_save_tab" => {
                let tab = self.resolve_mcp_tab(args, None)?;
                let saved = tab.save_in_place(window, cx).map_err(|e| e.to_string())?;
                Ok(json!({ "status": "saved", "tab_id": tab.id, "detail": saved }).to_string())
            }
            "kar_write_brief" => {
                let (id, exp_rev, title, summary, badge, blocks) =
                    crate::brief::parse_mcp_write_brief(args).map_err(|e| e.to_string())?;
                let brief = self
                    .upsert_brief_for_active_chat(id, exp_rev, title, summary, badge, blocks, cx)
                    .map_err(|e| e.to_string())?;
                Ok(json!({
                    "status": "published",
                    "id": brief.id,
                    "revision": brief.latest_revision,
                    "title": brief.current().title,
                })
                .to_string())
            }
            "kar_read_brief" => {
                let id_opt = args.get("id").and_then(Value::as_str);
                let rev_opt = args.get("revision").and_then(Value::as_u64);
                if let Some(id) = id_opt {
                    let b = self
                        .active_briefs
                        .iter()
                        .find(|b| b.id == id)
                        .ok_or_else(|| format!("未找到简报 `{id}`"))?;
                    let snap = b
                        .at_revision(rev_opt)
                        .ok_or_else(|| format!("未找到简报 `{id}` 的指定版本"))?;
                    serde_json::to_string_pretty(&json!({
                        "id": b.id,
                        "latest_revision": b.latest_revision,
                        "snapshot": snap,
                    }))
                    .map_err(|e| e.to_string())
                } else {
                    let list: Vec<Value> = self
                        .active_briefs
                        .iter()
                        .map(|b| {
                            json!({
                                "id": b.id,
                                "latest_revision": b.latest_revision,
                                "title": b.current().title,
                                "summary": b.current().summary,
                            })
                        })
                        .collect();
                    serde_json::to_string_pretty(&json!({ "briefs": list }))
                        .map_err(|e| e.to_string())
                }
            }
            other => Err(format!("未知工具: {other}")),
        }
    }
}
