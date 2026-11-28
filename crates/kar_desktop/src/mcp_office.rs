//! Live Model Context Protocol (`kar::serve_mcp_tools`) bridge connecting Kar CLI agents
//! directly to open Kantor Office (`DocsView`, `SheetsView`, `SlidesView`) tabs, KUI Briefs,
//! and Kwork's built-in System WebView Browser (`WKWebView` / `WebView2`).

use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use docs_engine::{Position as DocPos, Selection as DocSel};
use gpui::*;
use kar::host_tools::{HostTool, HostToolContent, HostToolResult, ServedMcpTools};
use serde_json::{Map, Value, json};
use sheets_engine::{CellRange, cell_name, formula::parse_range};
use tokio::sync::oneshot;

use crate::{
    kar_bridge::KarBridgeEvent,
    office_host::{OfficeEditorView, OfficeTab, clean_doc_para_text},
    ui_browser::SnapshotPurpose,
    ui_primitives::clean_display_doc_name,
    workbench::{KarWorkbench, RightPaneSurface},
};

const MCP_INSTRUCTIONS: &str = "\
Kwork hosts live WPS-grade Kantor Office editors (`Word .docx`, `Excel .xlsx`, `PPT .pptx`, `PDF .pdf`), native KUI Interactive Briefs, AND a live built-in System WebView Browser right beside this conversation.\n\
\n\
IMPORTANT DOCUMENT AUTHORING WORKFLOW:\n\
1. PROACTIVELY USE BUILT-IN OFFICE: Whenever the user asks to write, draft, compose, or polish an article, essay, report, notice, contract, Word document, spreadsheet, or presentation — even if no file is open yet — ALWAYS call `office_create_document` first (or `office_open_document` for an existing local file) to open it live in Kwork's side-by-side Office canvas instead of dumping long walls of text into chat or running ad-hoc scripts.\n\
2. WPS-GRADE TYPOGRAPHY: When authoring Word documents with `office_edit_word` / `office_format_word` / `office_page_setup` / `office_edit_word_table`:\n\
   - Set proper hierarchy (`Title` centered + bold, `Heading1`–`Heading3` with `keep_next: true`), Chinese fonts (`黑体`/`宋体`/`仿宋`/`楷体`/`PingFang SC`) & sizes (`二号`/`三号`/`小四` or `size_pt`), body `first_line_indent_chars: 2` (2em first-line indent for Chinese prose), comfortable `line_spacing` (`1.5`–`1.6` or `line_spacing_exact_pt: 28` for `official_zh`), structured tables (`border_style: \"three_line\"` or `\"grid\"` with `header_row: true`), and page setup / page numbers.\n\
3. VISUAL & LIFECYCLE CLOSURE: After writing or formatting a document, call `office_render_preview` to visually inspect the rendered page PNG (verifying centered title, indentation, table borders, and zero `layout_warnings` such as orphaned headings), then call `office_save_tab` (passing `path` if `NEEDS_SAVE_AS` is returned) so the file is cleanly saved on disk, and reply in chat with a concise summary.\n\
4. KUI & BROWSER: Use `kar_write_brief` / `kar_read_brief` for interactive KUI Brief cards, and `browser_open` / `browser_status` / `browser_snapshot` / `browser_act` / `browser_eval` / `browser_console` / `browser_screenshot` for live web pages and local dev servers.";

#[derive(Clone, Debug)]
pub struct McpToolOutput {
    pub text: String,
    pub image_png_base64: Option<String>,
}

impl From<String> for McpToolOutput {
    fn from(text: String) -> Self {
        Self {
            text,
            image_png_base64: None,
        }
    }
}

type McpReplyTx = oneshot::Sender<Result<McpToolOutput, String>>;

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
    pub fn respond(&self, result: Result<String, String>) {
        self.respond_output(result.map(McpToolOutput::from));
    }

    pub fn respond_output(&self, result: Result<McpToolOutput, String>) {
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
                    res = tokio::time::timeout(Duration::from_secs(30), reply_rx) => {
                        match res {
                            Ok(Ok(Ok(out))) => {
                                let mut content = vec![HostToolContent::Text { text: out.text }];
                                if let Some(data) = out.image_png_base64 {
                                    content.push(HostToolContent::Image {
                                        data,
                                        mime_type: "image/png".into(),
                                    });
                                }
                                Ok(HostToolResult {
                                    content,
                                    is_error: false,
                                })
                            }
                            Ok(Ok(Err(err))) => Ok(HostToolResult {
                                content: vec![HostToolContent::Text { text: err }],
                                is_error: true,
                            }),
                            Ok(Err(_)) => Err("桌面工作台窗口当前未打开".into()),
                            Err(_) => Err("等待桌面工作台响应超时".into()),
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
            "office_create_document",
            "Create a new Word (.docx), Excel (.xlsx), or PowerPoint (.pptx) document, bind its file path on disk (default `<workspace>/<title>.<ext>`), apply an optional typography preset (`clean_article`, `official_zh`, `academic`, `modern_business`), and open it live in Kwork's built-in Office pane.",
            json!({
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["word", "excel", "ppt"],
                        "description": "Document format to create (`word` -> .docx, `excel` -> .xlsx, `ppt` -> .pptx; default `word`)."
                    },
                    "title": { "type": "string", "description": "Document title (used as default filename stem and inserted as centered Title in Word when provided)." },
                    "path": { "type": "string", "description": "Optional relative (to workspace) or absolute save path. When omitted, defaults to `<workspace>/<title>.<ext>`." },
                    "preset": {
                        "type": "string",
                        "enum": ["clean_article", "official_zh", "academic", "modern_business"],
                        "description": "Typography & page-setup preset for Word (`official_zh` = GB/T 9704 official document, `academic` = thesis/paper, `modern_business` = business report, `clean_article` = polished essay/article)."
                    },
                    "untitled": { "type": "boolean", "description": "Set true only to open an unsaved scratch tab without writing an initial file to disk (default false)." },
                    "overwrite": { "type": "boolean", "description": "Overwrite `path` if a file already exists there (default false)." }
                },
                "additionalProperties": false
            }),
        ),
        (
            "office_open_document",
            "Open an existing local Word (.docx/.doc/.rtf/.odt), Excel (.xlsx/.xls/.ods/.csv), PowerPoint (.pptx/.ppt/.odp), or PDF (.pdf) file into Kwork's built-in Office pane.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Workspace-relative or absolute path to the document." }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        ),
        (
            "office_list_tabs",
            "List open Word (.docx), Excel (.xlsx), PowerPoint (.pptx), and PDF (.pdf) tabs in the Kwork Office pane.",
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
            "office_inspect_document",
            "Deeply inspect an open Word, Excel, PowerPoint, or PDF tab: returns exact page count, paper & margin setup (cm), headers & footers, heading outline, all tables with cell previews, inline/floating pictures & textboxes, footnotes/endnotes, comments, and layout warnings.",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": { "type": "integer", "description": "Optional tab_id from `office_list_tabs`; defaults to active tab." }
                },
                "additionalProperties": false
            }),
        ),
        (
            "office_read_word",
            "Read paragraphs (1-based index, effective style, alignment, font, size_pt, first-line indent, line spacing, list/table membership, and text) from an open Word (.docx) tab.",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": { "type": "integer", "description": "Optional tab_id from office_list_tabs; defaults to active Word tab." },
                    "start_paragraph": { "type": "integer", "description": "1-based starting paragraph (default 1)." },
                    "limit": { "type": "integer", "description": "Max paragraphs to return (default 60, max 200)." }
                },
                "additionalProperties": false
            }),
        ),
        (
            "office_edit_word",
            "Author or edit paragraphs in an open Word (.docx) tab as one undoable step. Supports replacing a paragraph, replacing the whole document (`replace_all: true`), deleting a range, find-and-replace, or inserting rich styled paragraphs (headings, centered titles, Chinese fonts/sizes like `黑体`/`宋体` + `二号`/`三号`/`小四`, `first_line_indent_chars: 2`, line spacing, bullet/numbered lists, inline `**bold**` markdown or `runs`).",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": { "type": "integer", "description": "Optional tab_id; defaults to active Word tab." },
                    "operation_id": { "type": "string", "description": "Optional UUID for idempotent replay protection." },
                    "expected_revision": { "type": "integer", "description": "Optional revision fence from `office_list_tabs` / `office_read_word`." },
                    "paragraph": { "type": "integer", "description": "1-based paragraph index to replace." },
                    "text": { "type": "string", "description": "Replacement text when `paragraph` is set." },
                    "replace_all": { "type": "boolean", "description": "When true with `paragraphs`, replaces the entire document body cleanly with the rich `paragraphs` array in one undo step." },
                    "insert_after": { "type": "integer", "description": "0 to insert at top, or 1-based paragraph index after which to insert `paragraphs` (omit to append at end)." },
                    "paragraphs": {
                        "type": "array",
                        "description": "Array of plain strings OR rich paragraph objects `{ text, style (Title|Subtitle|Heading1..Heading6|Quote|Normal|标题|一级标题..), align (left|center|right|justify), font, ascii_font, size_pt, size_zh (初号..小五), bold, italic, underline, color (#RRGGBB), highlight, first_line_indent_chars (e.g. 2), line_spacing (e.g. 1.5), line_spacing_exact_pt, space_before_pt, space_after_pt, list (bullet|number), level, keep_next, page_break_before, markdown (bool), runs: [{ text, bold, italic, underline, color, font, size_pt }] }`.",
                        "items": {}
                    },
                    "delete_range": {
                        "type": "object",
                        "description": "Delete body paragraphs `{ start_paragraph: 1-based, count: integer }`.",
                        "properties": {
                            "start_paragraph": { "type": "integer" },
                            "count": { "type": "integer" }
                        },
                        "required": ["start_paragraph", "count"]
                    },
                    "find_replace": {
                        "type": "object",
                        "description": "Global find-and-replace across the document.",
                        "properties": {
                            "find": { "type": "string" },
                            "replace": { "type": "string" },
                            "match_case": { "type": "boolean" },
                            "whole_word": { "type": "boolean" }
                        },
                        "required": ["find", "replace"]
                    }
                },
                "additionalProperties": false
            }),
        ),
        (
            "office_format_word",
            "Format existing paragraphs (`start_paragraph..=end_paragraph`) or apply a document-wide typography preset (`clean_article`, `official_zh`, `academic`, `modern_business`) in an open Word (.docx) tab.",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": { "type": "integer", "description": "Optional tab_id; defaults to active Word tab." },
                    "preset": {
                        "type": "string",
                        "enum": ["clean_article", "official_zh", "academic", "modern_business"],
                        "description": "Apply a complete document typography preset across all headings & body paragraphs."
                    },
                    "start_paragraph": { "type": "integer", "description": "1-based start paragraph index." },
                    "end_paragraph": { "type": "integer", "description": "1-based inclusive end paragraph index (defaults to `start_paragraph`)." },
                    "style": { "type": "string", "description": "Paragraph style (`Title`, `Subtitle`, `Heading1`..`Heading6`, `Quote`, `Normal`, or Chinese aliases `标题`, `一级标题`..)." },
                    "align": { "type": "string", "enum": ["left", "center", "right", "justify"] },
                    "font": { "type": "string", "description": "Font family (e.g. `宋体`, `黑体`, `楷体`, `仿宋`, `微软雅黑`, `PingFang SC`, `Times New Roman`)." },
                    "ascii_font": { "type": "string", "description": "Optional Western/ASCII font paired with East Asian `font` (e.g. `Times New Roman`)." },
                    "size_pt": { "type": "number", "description": "Font size in points (e.g. 22, 16, 14, 12, 10.5)." },
                    "size_zh": { "type": "string", "description": "Chinese font size name (`初号`, `小初`, `一号`, `小一`, `二号`, `小二`, `三号`, `小三`, `四号`, `小四`, `五号`, `小五`)." },
                    "bold": { "type": "boolean" },
                    "italic": { "type": "boolean" },
                    "underline": { "type": "boolean" },
                    "color": { "type": "string", "description": "Hex text color `#RRGGBB`." },
                    "highlight": { "type": "string", "description": "Highlight color (`yellow`, `green`, `cyan`, `magenta`, `blue`, `red`, `lightGray`)." },
                    "first_line_indent_chars": { "type": "number", "description": "First-line indent in characters (e.g. `2` for standard Chinese 2em indent, `0` to clear)." },
                    "first_line_indent_cm": { "type": "number" },
                    "left_indent_cm": { "type": "number" },
                    "right_indent_cm": { "type": "number" },
                    "line_spacing": { "type": "number", "description": "Line spacing multiple (e.g. `1.5`)." },
                    "line_spacing_exact_pt": { "type": "number", "description": "Exact line spacing in points (e.g. `28` for GB/T 9704 official docs)." },
                    "space_before_pt": { "type": "number" },
                    "space_after_pt": { "type": "number" },
                    "list": { "type": "string", "enum": ["bullet", "number"] },
                    "keep_next": { "type": "boolean", "description": "Keep heading on the same page as the following paragraph." },
                    "page_break_before": { "type": "boolean" }
                },
                "additionalProperties": false
            }),
        ),
        (
            "office_page_setup",
            "Configure paper size (`A4`, `Letter`, `A5`), orientation (`portrait`, `landscape`), margins in cm or preset (`normal`, `narrow`, `official_zh` GB/T 9704), headers, footers, and automatic page numbers on an open Word (.docx) tab.",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": { "type": "integer" },
                    "paper": { "type": "string", "enum": ["A4", "Letter", "Legal", "A5", "F4"] },
                    "orientation": { "type": "string", "enum": ["portrait", "landscape"] },
                    "margin_preset": { "type": "string", "enum": ["normal", "narrow", "moderate", "wide", "official_zh"] },
                    "top_cm": { "type": "number" },
                    "bottom_cm": { "type": "number" },
                    "left_cm": { "type": "number" },
                    "right_cm": { "type": "number" },
                    "header_text": { "type": "string" },
                    "header_align": { "type": "string", "enum": ["left", "center", "right"] },
                    "footer_text": { "type": "string" },
                    "footer_align": { "type": "string", "enum": ["left", "center", "right"] },
                    "page_numbers": { "type": "boolean", "description": "Insert dynamic page numbers into the footer." },
                    "different_first_page": { "type": "boolean" }
                },
                "additionalProperties": false
            }),
        ),
        (
            "office_edit_word_table",
            "Insert, populate, style (`grid` or academic `three_line` 三线表, repeating header row, shading, column widths), merge cells, or add/delete rows & columns in a Word (.docx) table.",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": { "type": "integer" },
                    "action": {
                        "type": "string",
                        "enum": ["insert", "set_cells", "style_table", "merge_cells", "add_row", "delete_row", "add_column", "delete_column", "delete_table"],
                        "description": "Table operation (default `insert`)."
                    },
                    "insert_after": { "type": "integer", "description": "For `insert`: 1-based paragraph index after which to place the table (omit to append at end)." },
                    "rows": { "type": "integer" },
                    "cols": { "type": "integer" },
                    "cells": {
                        "type": "array",
                        "description": "For `insert`: 2D array of cell strings `[[\"维度\", \"数值\"], [\"A\", \"100\"]]`.",
                        "items": { "type": "array", "items": { "type": "string" } }
                    },
                    "table_id": { "type": "integer", "description": "Target `table_id` from `office_inspect_document` (defaults to first/active table)." },
                    "header_row": { "type": "boolean", "description": "Format row 1 as bold repeating header row (default true)." },
                    "header_fill_hex": { "type": "string", "description": "Optional header background hex e.g. `#F1F5F9`." },
                    "border_style": { "type": "string", "enum": ["grid", "three_line", "none"], "description": "`grid` (full thin borders), `three_line` (WPS academic/official 三线表), or `none`." },
                    "align": { "type": "string", "enum": ["left", "center", "right"] },
                    "col_widths_cm": { "type": "array", "items": { "type": "number" } },
                    "edits": {
                        "type": "array",
                        "description": "For `set_cells`: `[{ row: 1-based, col: 1-based, text, bold?, align?, color?, fill_hex? }]`.",
                        "items": { "type": "object" }
                    },
                    "row": { "type": "integer", "description": "1-based target row for `add_row` / `delete_row`." },
                    "col": { "type": "integer", "description": "1-based target column for `add_column` / `delete_column`." },
                    "start_row": { "type": "integer" },
                    "start_col": { "type": "integer" },
                    "end_row": { "type": "integer" },
                    "end_col": { "type": "integer" }
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
            "Save an open Word, Excel, or PowerPoint tab to disk. Supports saving in place, Save-As to a new `path` (binding that path to the open tab), or `prompt_user: true` to open Kwork's native Save-As dialog. Returns structured `NEEDS_SAVE_AS` + `suggested_path` when an untitled tab has no path yet.",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": { "type": "integer", "description": "Optional tab_id; defaults to active tab." },
                    "path": { "type": "string", "description": "Optional workspace-relative or absolute path for Save-As (required when saving an untitled document unless `prompt_user: true`)." },
                    "overwrite": { "type": "boolean", "description": "Whether to overwrite if `path` already exists (default true)." },
                    "prompt_user": { "type": "boolean", "description": "Open Kwork's native macOS Save-As file dialog for the user." }
                },
                "additionalProperties": false
            }),
        ),
        (
            "office_render_preview",
            "Render a 1-based page/slide of an open Word (.docx), Excel (.xlsx), PowerPoint (.pptx), or PDF (.pdf) tab into a high-DPI PNG image for visual typography & pagination verification. Returns both multimodal PNG image content and layout diagnostics (`page_count`, `layout_warnings` for orphaned headings or 1-line trailing pages).",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": { "type": "integer", "description": "Optional tab_id; defaults to active Office/PDF tab." },
                    "page": { "type": "integer", "description": "1-based page or slide number to preview (default 1)." },
                    "dpi": { "type": "integer", "description": "Render resolution in DPI (72..=216, default 144)." }
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
        (
            "browser_open",
            "Open or navigate Kwork's built-in system WebView browser to a URL, `:port` shorthand (e.g. `:5173`), or workspace HTML file.",
            json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Target URL (`http://localhost:5173`), `:port` shorthand (`:3000`), or workspace HTML path." },
                    "new_tab": { "type": "boolean", "description": "Open in a new browser sub-tab instead of navigating the active tab (default false)." }
                },
                "required": ["url"],
                "additionalProperties": false
            }),
        ),
        (
            "browser_status",
            "Inspect open tabs, active URL/title, loading state, viewport preset, and console error counts in Kwork's built-in browser.",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        ),
        (
            "browser_snapshot",
            "Read a structured DOM snapshot (headings, visible interactive links/buttons/inputs with CSS selectors & bounding boxes, and body text) from the active built-in browser tab.",
            json!({
                "type": "object",
                "properties": {
                    "max_items": { "type": "integer", "description": "Maximum interactive DOM elements to include (default 80)." }
                },
                "additionalProperties": false
            }),
        ),
        (
            "browser_act",
            "Interact with an element or scroll in the active built-in browser tab (`click`, `fill`, `select`, `press`, `hover`, `scroll`). Works with React/Vue controlled inputs.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["click", "fill", "select", "press", "hover", "scroll"],
                        "description": "Interaction kind."
                    },
                    "selector": { "type": "string", "description": "CSS selector of the target element (from `browser_snapshot`)." },
                    "text": { "type": "string", "description": "Fallback visible button/link text when `selector` is omitted." },
                    "value": { "type": "string", "description": "Input/select value for `fill` or `select`." },
                    "key": { "type": "string", "description": "Keyboard key name for `press` (e.g. `Enter`, `Escape`)." },
                    "dx": { "type": "number", "description": "Horizontal scroll delta in px for `scroll`." },
                    "dy": { "type": "number", "description": "Vertical scroll delta in px for `scroll` (default 400)." }
                },
                "required": ["action"],
                "additionalProperties": false
            }),
        ),
        (
            "browser_eval",
            "Evaluate a JavaScript expression inside the active built-in browser tab and return its JSON-serialized result.",
            json!({
                "type": "object",
                "properties": {
                    "expression": { "type": "string", "description": "JavaScript expression to evaluate in the page context." }
                },
                "required": ["expression"],
                "additionalProperties": false
            }),
        ),
        (
            "browser_console",
            "Read captured console logs, uncaught JS exceptions, unhandled Promise rejections, and failed fetch calls from the active built-in browser tab.",
            json!({
                "type": "object",
                "properties": {
                    "errors_only": { "type": "boolean", "description": "Only return warnings and errors (default false)." },
                    "clear": { "type": "boolean", "description": "Clear the console buffer after reading (default false)." },
                    "limit": { "type": "integer", "description": "Maximum log entries to return (default 50)." }
                },
                "additionalProperties": false
            }),
        ),
        (
            "browser_screenshot",
            "Capture a Retina PNG screenshot of the active built-in browser tab for visual UI inspection (returns both multimodal PNG image content and the saved file path).",
            json!({
                "type": "object",
                "properties": {},
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
    pub fn handle_office_mcp_call(
        &mut self,
        call: OfficeMcpCall,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.try_dispatch_browser_mcp_call(&call, window, cx) {
            cx.notify();
            return;
        }
        let name = call.tool.clone();
        let args = call.args.clone();
        if name == "office_render_preview" {
            let out = self
                .resolve_mcp_tab(&args, None)
                .and_then(|tab| self.exec_render_preview(&tab, &args, window, cx));
            call.respond_output(out);
            cx.notify();
            return;
        }
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
            "office_create_document" => self.exec_create_document(args, window, cx),
            "office_open_document" => self.exec_open_document(args, window, cx),
            "office_inspect_document" => {
                let tab = self.resolve_mcp_tab(args, None)?;
                self.exec_inspect_document(&tab, cx)
            }
            "office_format_word" => {
                let tab = self.resolve_mcp_tab(args, Some(crate::types::OfficeKind::Word))?;
                self.exec_format_word(&tab, args, window, cx)
            }
            "office_page_setup" => {
                let tab = self.resolve_mcp_tab(args, Some(crate::types::OfficeKind::Word))?;
                self.exec_page_setup(&tab, args, window, cx)
            }
            "office_edit_word_table" => {
                let tab = self.resolve_mcp_tab(args, Some(crate::types::OfficeKind::Word))?;
                self.exec_edit_word_table(&tab, args, window, cx)
            }
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
                        "needs_save_as": t.needs_save_as(cx),
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
                    .unwrap_or(60)
                    .clamp(1, 200) as usize;
                let payload = e.read(cx).agent_read(|doc, _| {
                    let total = doc.len();
                    let start_0 = (start_1 - 1).min(total);
                    let end_0 = (start_0 + limit).min(total);
                    let paras: Vec<Value> = (start_0..end_0)
                        .map(|idx| crate::wps_office::format_word_paragraph_entry(doc, idx))
                        .collect();
                    json!({
                        "tab_id": tab.id,
                        "document": clean_display_doc_name(&tab.doc_name(cx)),
                        "path": doc.path().map(|p| p.display().to_string()),
                        "needs_save_as": doc.needs_save_as(),
                        "total_paragraphs": total,
                        "available_styles": doc.paragraph_style_names(),
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

                if let Some(del) = args.get("delete_range").and_then(Value::as_object) {
                    let start_1 = del
                        .get("start_paragraph")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| "缺少 `start_paragraph`".to_owned())?
                        .max(1) as usize;
                    let count = del
                        .get("count")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| "缺少 `count`".to_owned())?
                        .max(1) as usize;
                    let OfficeEditorView::Word(ref e) = tab.view else {
                        return Err("不是 Word 文档".into());
                    };
                    e.update(cx, |v, cx| {
                        v.agent_edit("智能体删除段落", window, cx, |doc, sel| {
                            doc.delete_paragraphs(start_1 - 1, count, sel)
                                .map_err(anyhow::Error::msg)?;
                            Ok(((), doc.clamp_selection(sel)))
                        })
                    })
                    .map_err(|e| e.to_string())?;
                    return Ok(json!({
                        "status": "deleted",
                        "tab_id": tab.id,
                        "start_paragraph": start_1,
                        "deleted_count": count,
                    })
                    .to_string());
                }

                if let Some(fr) = args.get("find_replace").and_then(Value::as_object) {
                    let find = fr
                        .get("find")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| "`find` 不能为空".to_owned())?
                        .to_owned();
                    let replace = fr
                        .get("replace")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    let match_case = fr
                        .get("match_case")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let whole_word = fr
                        .get("whole_word")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let OfficeEditorView::Word(ref e) = tab.view else {
                        return Err("不是 Word 文档".into());
                    };
                    let replaced_count = e
                        .update(cx, |v, cx| {
                            v.agent_edit("智能体查找替换", window, cx, |doc, sel| {
                                let opts = kantor_core::find::FindOptions {
                                    match_case,
                                    whole_word,
                                };
                                let ranges: Vec<_> = doc
                                    .paragraphs()
                                    .iter()
                                    .enumerate()
                                    .flat_map(|(i, p)| {
                                        kantor_core::find::find_all(&p.text, &find, opts)
                                            .into_iter()
                                            .map(move |r| (i, r))
                                    })
                                    .collect();
                                if ranges.is_empty() {
                                    return Ok((0usize, sel));
                                }
                                let n = ranges.len();
                                let after = doc.replace_ranges(&ranges, &replace, sel);
                                Ok((n, after))
                            })
                        })
                        .map_err(|e| e.to_string())?;
                    return Ok(json!({
                        "status": "replaced",
                        "tab_id": tab.id,
                        "matches_replaced": replaced_count,
                    })
                    .to_string());
                }

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
                    if paras_arr.is_empty() {
                        return Err("`paragraphs` 不能为空".into());
                    }
                    let replace_all = args
                        .get("replace_all")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let after_arg = args.get("insert_after").and_then(Value::as_u64);
                    let OfficeEditorView::Word(ref e) = tab.view else {
                        return Err("不是 Word 文档".into());
                    };
                    let range = e
                        .update(cx, |v, cx| {
                            v.agent_edit("智能体写入排版段落", window, cx, |doc, sel| {
                                let mut rich_items = Vec::with_capacity(paras_arr.len());
                                for item in paras_arr {
                                    rich_items.push(
                                        crate::wps_office::build_rich_paragraph(doc, item)
                                            .map_err(anyhow::Error::msg)?,
                                    );
                                }
                                let after = match after_arg {
                                    Some(0) => None,
                                    Some(p1) => {
                                        Some(((p1 - 1) as usize).min(doc.len().saturating_sub(1)))
                                    }
                                    None => Some(doc.len().saturating_sub(1)),
                                };
                                let r = doc
                                    .insert_rich_paragraphs(after, replace_all, &rich_items, sel)
                                    .map_err(anyhow::Error::msg)?;
                                let end = doc.paragraph(r.end - 1).text.len();
                                Ok((
                                    r.clone(),
                                    DocSel {
                                        anchor: DocPos::new(r.start, 0),
                                        head: DocPos::new(r.end - 1, end),
                                    },
                                ))
                            })
                        })
                        .map_err(|e| e.to_string())?;
                    return Ok(json!({
                        "status": if replace_all { "replaced_all" } else { "inserted" },
                        "tab_id": tab.id,
                        "start_paragraph": range.start + 1,
                        "end_paragraph": range.end,
                        "inserted_count": range.len(),
                    })
                    .to_string());
                }
                Err("请提供 (`paragraph` + `text`)、(`paragraphs` 数组)、`delete_range` 或 `find_replace`".into())
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
                self.exec_save_tab_upgraded(&tab, args, window, cx)
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

    fn try_dispatch_browser_mcp_call(
        &mut self,
        call: &OfficeMcpCall,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let args = &call.args;
        match call.tool.as_str() {
            "browser_open" => {
                let Some(raw_url) = args
                    .get("url")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                else {
                    call.respond(Err("缺少非空的 `url` 参数".into()));
                    return true;
                };
                let new_tab = args
                    .get("new_tab")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                self.open_url_in_browser(raw_url, new_tab, window, cx);
                window.refresh();
                let target_tab_id = self.browser.active_tab().map(|t| t.id).unwrap_or(1);
                let win_handle = window.window_handle();
                let call_clone = call.clone();
                cx.spawn_in(window, async move |weak, cx| {
                    for step in 0..45u32 {
                        cx.background_executor()
                            .timer(Duration::from_millis(80))
                            .await;
                        let mut ready_payload: Option<String> = None;
                        let mut needs_draw = false;
                        let alive = weak.update_in(cx, |wb, _, _| {
                            if let Some(tab) = wb.browser.tab_by_id_mut(target_tab_id) {
                                if tab.webview.is_none() {
                                    needs_draw = true;
                                } else if !tab.loading && (step >= 2 || tab.page_load_failed) {
                                    ready_payload = serde_json::to_string_pretty(&json!({
                                        "status": if tab.page_load_failed { "load_failed" } else { "opened" },
                                        "tab_id": tab.id,
                                        "url": tab.url,
                                        "title": tab.title,
                                        "loading": tab.loading,
                                        "page_load_failed": tab.page_load_failed,
                                        "load_error": tab.last_error_banner,
                                        "error_count": tab.error_count,
                                    }))
                                    .ok();
                                }
                            }
                        });
                        if alive.is_err() {
                            call_clone.respond(Err("桌面窗口已关闭".into()));
                            return;
                        }
                        if needs_draw {
                            let _ = win_handle.update(cx, |_, win, cx| {
                                win.refresh();
                                win.draw(cx).clear(cx);
                            });
                        }
                        if let Some(payload) = ready_payload {
                            call_clone.respond(Ok(payload));
                            return;
                        }
                    }
                    let _ = weak.update_in(cx, |wb, _, _| {
                        let out = if let Some(tab) = wb.browser.tab_by_id_mut(target_tab_id) {
                            serde_json::to_string_pretty(&json!({
                                "status": "opened",
                                "tab_id": tab.id,
                                "url": tab.url,
                                "title": tab.title,
                                "loading": tab.loading,
                                "error_count": tab.error_count,
                            }))
                            .map_err(|e| e.to_string())
                        } else {
                            Err("浏览器标签页已关闭".into())
                        };
                        call_clone.respond(out);
                    });
                })
                .detach();
                true
            }
            "browser_status" => {
                let zh = self.ui_locale == crate::ui_primitives::UiLocale::Zh;
                let active_id = self.browser.active_tab().map(|t| t.id);
                let tabs: Vec<Value> = self
                    .browser
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(idx, t)| {
                        json!({
                            "tab_id": t.id,
                            "is_active": idx == self.browser.active_idx,
                            "url": t.url,
                            "title": t.display_title(zh),
                            "loading": t.loading,
                            "viewport": t.viewport.label_en(),
                            "error_count": t.error_count,
                            "warn_count": t.warn_count,
                            "webview_mounted": t.webview.is_some(),
                        })
                    })
                    .collect();
                let payload = json!({
                    "browser_pane_open": self.browser_tab_open && self.right_surface == RightPaneSurface::Browser,
                    "active_tab_id": active_id,
                    "detected_dev_ports": self.detected_dev_ports,
                    "tabs": tabs,
                });
                call.respond(serde_json::to_string_pretty(&payload).map_err(|e| e.to_string()));
                true
            }
            "browser_snapshot" => {
                let max_items = args
                    .get("max_items")
                    .and_then(Value::as_u64)
                    .unwrap_or(80)
                    .clamp(1, 250);
                let Some(tab) = self.browser.active_tab() else {
                    call.respond(Err("内置浏览器当前没有打开任何标签页".into()));
                    return true;
                };
                let Some(ref wv) = tab.webview else {
                    call.respond(Err(
                        "当前浏览器标签尚未加载网页，请先调用 `browser_open` 打开页面".into(),
                    ));
                    return true;
                };
                let script = format!(
                    "JSON.stringify(window.__kwrokSnapshotDom ? window.__kwrokSnapshotDom({max_items}) : {{ url: location.href, title: document.title, readyState: document.readyState, bodyText: document.body ? document.body.innerText : '' }})"
                );
                let call_clone = call.clone();
                if let Err(err) = wv.eval(&script, move |raw| {
                    call_clone.respond(Ok(format_webkit_json_reply(&raw)));
                }) {
                    call.respond(Err(err));
                }
                true
            }
            "browser_act" => {
                let Some(action) = args.get("action").and_then(Value::as_str) else {
                    call.respond(Err("缺少 `action` 参数".into()));
                    return true;
                };
                let Some(tab) = self.browser.active_tab() else {
                    call.respond(Err("内置浏览器当前没有打开任何标签页".into()));
                    return true;
                };
                let Some(ref wv) = tab.webview else {
                    call.respond(Err(
                        "当前浏览器标签尚未加载网页，请先调用 `browser_open` 打开页面".into(),
                    ));
                    return true;
                };
                let cmd_json = json!({
                    "action": action,
                    "selector": args.get("selector").and_then(Value::as_str),
                    "text": args.get("text").and_then(Value::as_str),
                    "value": args.get("value").and_then(Value::as_str),
                    "key": args.get("key").and_then(Value::as_str),
                    "dx": args.get("dx").and_then(Value::as_f64),
                    "dy": args.get("dy").and_then(Value::as_f64),
                });
                let script = format!(
                    "JSON.stringify(window.__kwrokAct ? window.__kwrokAct({cmd_json}) : {{ ok: false, error: 'Bridge not initialized' }})"
                );
                let call_clone = call.clone();
                if let Err(err) = wv.eval(&script, move |raw| {
                    call_clone.respond(Ok(format_webkit_json_reply(&raw)));
                }) {
                    call.respond(Err(err));
                }
                true
            }
            "browser_eval" => {
                let Some(expr) = args
                    .get("expression")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                else {
                    call.respond(Err("缺少非空的 `expression` 参数".into()));
                    return true;
                };
                let Some(tab) = self.browser.active_tab() else {
                    call.respond(Err("内置浏览器当前没有打开任何标签页".into()));
                    return true;
                };
                let Some(ref wv) = tab.webview else {
                    call.respond(Err(
                        "当前浏览器标签尚未加载网页，请先调用 `browser_open` 打开页面".into(),
                    ));
                    return true;
                };
                let script = format!(
                    "(function() {{ try {{ const r = ({expr}); return JSON.stringify({{ ok: true, result: r, url: location.href, title: document.title }}); }} catch (e) {{ return JSON.stringify({{ ok: false, error: String(e && e.stack ? e.stack : e) }}); }} }})()"
                );
                let call_clone = call.clone();
                if let Err(err) = wv.eval(&script, move |raw| {
                    call_clone.respond(Ok(format_webkit_json_reply(&raw)));
                }) {
                    call.respond(Err(err));
                }
                true
            }
            "browser_console" => {
                let errors_only = args
                    .get("errors_only")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let clear = args.get("clear").and_then(Value::as_bool).unwrap_or(false);
                let limit = args
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(50)
                    .clamp(1, 200) as usize;
                let Some(tab) = self.browser.active_tab_mut() else {
                    call.respond(Err("内置浏览器当前没有打开任何标签页".into()));
                    return true;
                };
                let messages: Vec<Value> = tab
                    .console
                    .iter()
                    .filter(|c| !errors_only || c.level.is_error() || c.level.is_warn())
                    .rev()
                    .take(limit)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .map(|c| {
                        json!({
                            "level": c.level.label(),
                            "message": c.message,
                            "source": c.source,
                            "timestamp_ms": c.timestamp_ms,
                        })
                    })
                    .collect();
                let payload = json!({
                    "tab_id": tab.id,
                    "url": tab.url,
                    "error_count": tab.error_count,
                    "warn_count": tab.warn_count,
                    "messages": messages,
                });
                if clear {
                    tab.console.clear();
                    tab.error_count = 0;
                    tab.warn_count = 0;
                }
                call.respond(serde_json::to_string_pretty(&payload).map_err(|e| e.to_string()));
                true
            }
            "browser_screenshot" => {
                let Some(tab) = self.browser.active_tab() else {
                    call.respond(Err("内置浏览器当前没有打开任何标签页".into()));
                    return true;
                };
                let Some(ref wv) = tab.webview else {
                    call.respond(Err(
                        "当前浏览器标签尚未加载网页，请先调用 `browser_open` 打开页面".into(),
                    ));
                    return true;
                };
                let tab_id = tab.id;
                let url = tab.url.clone();
                let snap_tx = self.browser.snapshot_tx.clone();
                let call_clone = call.clone();
                wv.capture_snapshot_png(move |res| match res {
                    Ok(captured) => {
                        let _ = snap_tx.try_send((
                            tab_id,
                            SnapshotPurpose::CacheOnly,
                            Ok(captured.clone()),
                        ));
                        let dir = std::env::temp_dir().join("kwrok-browser-captures");
                        let _ = std::fs::create_dir_all(&dir);
                        let saved_path = dir.join(format!(
                            "mcp-browser-{}-{}.png",
                            tab_id,
                            crate::remote_link::now_epoch_ms()
                        ));
                        let saved_str = std::fs::write(&saved_path, &captured.png_bytes)
                            .ok()
                            .map(|()| saved_path.display().to_string());
                        let b64 = base64::engine::general_purpose::STANDARD
                            .encode(&captured.png_bytes);
                        let text = serde_json::to_string_pretty(&json!({
                            "status": "captured",
                            "tab_id": tab_id,
                            "url": url,
                            "width": captured.width,
                            "height": captured.height,
                            "saved_path": saved_str,
                        }))
                        .unwrap_or_default();
                        call_clone.respond_output(Ok(McpToolOutput {
                            text,
                            image_png_base64: Some(b64),
                        }));
                    }
                    Err(err) => call_clone.respond(Err(err)),
                });
                true
            }
            _ => false,
        }
    }
}

/// Unwrap the outer JSON string returned by `WKWebView`/`WebView2` `evaluateJavaScript` when the
/// script returns `JSON.stringify(...)`, and format the inner JSON cleanly.
pub(crate) fn format_webkit_json_reply(raw: &str) -> String {
    let candidate = serde_json::from_str::<String>(raw).unwrap_or_else(|_| raw.to_owned());
    if let Ok(val) = serde_json::from_str::<Value>(&candidate) {
        serde_json::to_string_pretty(&val).unwrap_or(candidate)
    } else {
        candidate
    }
}

/// Locate the executable capable of running `--mcp-stdio-bridge` (`Kwork` or `kar-desktop`) so
/// stdio-only ACP runtimes (including Kwork's built-in harness) can connect to `ServedMcpTools`.
pub fn resolve_stdio_bridge_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    if matches!(stem, "Kwork" | "Kwrok" | "kar-desktop") {
        return Some(exe);
    }
    let bin_name = if cfg!(windows) {
        "kar-desktop.exe"
    } else {
        "kar-desktop"
    };
    for ancestor in exe.ancestors().skip(1).take(6) {
        let candidate = ancestor.join(bin_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Lightweight stdio-to-HTTP JSON-RPC bridge invoked via `kar-desktop --mcp-stdio-bridge`.
/// Forwards MCP messages between `stdin`/`stdout` and `KAR_MCP_HTTP_URL` (`KAR_MCP_AUTH`).
pub fn run_stdio_bridge() -> ! {
    let Ok(url) = std::env::var("KAR_MCP_HTTP_URL") else {
        eprintln!("kar-desktop --mcp-stdio-bridge: missing KAR_MCP_HTTP_URL");
        std::process::exit(2);
    };
    let auth = std::env::var("KAR_MCP_AUTH").unwrap_or_default();
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        std::process::exit(2);
    };
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
    else {
        std::process::exit(2);
    };

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    let mut session_id: Option<String> = None;

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Err(_) => break,
            Ok(_) => {}
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let (payload, content_length_framed) =
            if let Some(rest) = trimmed
                .strip_prefix("Content-Length:")
                .or_else(|| trimmed.strip_prefix("content-length:"))
                && let Ok(len) = rest.trim().parse::<usize>()
            {
                loop {
                    let mut hdr = String::new();
                    if reader.read_line(&mut hdr).unwrap_or(0) == 0 || hdr.trim().is_empty() {
                        break;
                    }
                }
                let mut buf = vec![0u8; len];
                if reader.read_exact(&mut buf).is_err() {
                    break;
                }
                (buf, true)
            } else {
                (trimmed.as_bytes().to_vec(), false)
            };

        let reply_bytes = rt.block_on(async {
            let mut req = client
                .post(&url)
                .header("Content-Type", "application/json")
                .body(payload);
            if !auth.is_empty() {
                req = req.header("Authorization", &auth);
            }
            if let Some(ref sid) = session_id {
                req = req.header("mcp-session-id", sid);
            }
            let resp = req.send().await.ok()?;
            if let Some(sid) = resp
                .headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
            {
                session_id = Some(sid.to_owned());
            }
            if resp.status() == reqwest::StatusCode::ACCEPTED {
                return None;
            }
            let bytes = resp.bytes().await.ok()?;
            if bytes.is_empty() {
                return None;
            }
            let single_line = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|v| serde_json::to_vec(&v).ok())
                .unwrap_or_else(|| bytes.to_vec());
            Some(single_line)
        });

        if let Some(out) = reply_bytes {
            if content_length_framed {
                let _ = write!(writer, "Content-Length: {}\r\n\r\n", out.len());
                let _ = writer.write_all(&out);
            } else {
                let _ = writer.write_all(&out);
                let _ = writer.write_all(b"\n");
            }
            let _ = writer.flush();
        }
    }
    std::process::exit(0);
}

#[cfg(test)]
mod format_tests {
    use super::format_webkit_json_reply;

    #[test]
    fn unwraps_webkit_double_encoded_json_string() {
        let raw = r#""{\"ok\":true,\"title\":\"Hello\"}""#;
        let formatted = format_webkit_json_reply(raw);
        assert!(formatted.contains("\"ok\": true"));
        assert!(formatted.contains("\"title\": \"Hello\""));
    }
}
