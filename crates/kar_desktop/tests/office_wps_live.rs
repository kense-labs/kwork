//! Live GPUI Metal window + `ServedMcpTools` + `--mcp-stdio-bridge` end-to-end test
//! verifying Kwrok's WPS-grade Office document lifecycle closure, rich typography,
//! three-line academic tables, GB/T 9704 page setup, deep structural inspection,
//! `NEEDS_SAVE_AS` self-recovery, and multimodal `office_render_preview` PNG output.

#![forbid(unsafe_code)]

extern crate gpui_kit as gpui;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use base64::Engine as _;
use gpui::*;
use kar_desktop::{
    APP_NAME, init_runtime_ui,
    kar_bridge::{KarBridgeEvent, KarCommand},
    mcp_office::{resolve_stdio_bridge_exe, start_office_mcp_server},
    studio_window_options,
    workbench::{KarWorkbench, WorkbenchConfig},
};
use serde_json::{Value, json};

const TEST_NAME: &str = "live_wps_office_lifecycle_formatting_and_visual_preview";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        println!("{TEST_NAME}: test\n\n1 test, 0 benchmarks");
        return;
    }
    if args.iter().any(|a| a == "--ignored") {
        println!(
            "\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out\n"
        );
        return;
    }
    if let Some(filter) = args.iter().find(|a| !a.starts_with('-'))
        && !TEST_NAME.contains(filter.as_str())
        && !"office_wps_live".contains(filter.as_str())
    {
        println!(
            "\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out\n"
        );
        return;
    }

    println!("\nrunning 1 test");

    #[cfg(not(target_os = "macos"))]
    {
        println!("test {TEST_NAME} ... ok");
        println!(
            "\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
        );
        return;
    }

    #[cfg(target_os = "macos")]
    run_macos_e2e();
}

#[cfg(target_os = "macos")]
fn run_macos_e2e() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let chat_dir = tmp.path().join("chats");
    let workspace_dir = tmp.path().join("workspace");
    let docs_dir = tmp.path().join("docs");
    std::fs::create_dir_all(&chat_dir).unwrap();
    std::fs::create_dir_all(&workspace_dir).unwrap();
    std::fs::create_dir_all(&docs_dir).unwrap();

    let ws_for_verify = workspace_dir.clone();
    let (mcp_evt_tx, mcp_evt_rx) = async_channel::bounded::<KarBridgeEvent>(64);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<(String, String)>();
    let (done_tx, done_rx) = async_channel::bounded::<Result<(), String>>(1);

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async move {
            let served = start_office_mcp_server(mcp_evt_tx)
                .await
                .expect("start_office_mcp_server");
            let (_, server) = served.entry();
            let (mcp_url, mcp_auth) = match server {
                kar::McpServer::HostedHttp { url, headers }
                | kar::McpServer::Http { url, headers } => (
                    url,
                    headers.get("Authorization").cloned().unwrap_or_default(),
                ),
                other => panic!("unexpected MCP server variant: {other:?}"),
            };
            ready_tx.send((mcp_url.clone(), mcp_auth.clone())).unwrap();
            tokio::time::sleep(Duration::from_millis(350)).await;
            let result = verify_wps_office_mcp(&ws_for_verify, &mcp_url, &mcp_auth).await;
            served.close().await;
            let _ = done_tx.send(result).await;
        });
    });

    let (_mcp_url, _mcp_auth) = ready_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("MCP server ready");

    let app = gpui_kit::application().with_assets(kantor_ui::KantorAssets);
    app.run(move |cx| {
        init_runtime_ui(cx);
        let mut window_options = studio_window_options(
            APP_NAME,
            size(px(1360.), px(860.)),
            size(px(400.), px(360.)),
            cx,
        );
        window_options.focus = false;

        let wb_cfg = WorkbenchConfig {
            chat_dir: chat_dir.clone(),
            workspace_dir: workspace_dir.clone(),
            docs_dir: docs_dir.clone(),
            resume_last_chat: false,
            start_demo_chat: true,
            ..WorkbenchConfig::default()
        };

        let (window, entity) = gpui_kit::open_window(window_options, cx, move |window, cx| {
            cx.new(|cx| KarWorkbench::new(wb_cfg, window, cx))
        })
        .expect("open GPUI window");

        let bridge_tx = entity.read(cx).bridge.tx.clone();

        let wb_for_mcp = entity.clone();
        cx.spawn(async move |cx| {
            while let Ok(KarBridgeEvent::OfficeMcpCall(call)) = mcp_evt_rx.recv().await {
                let _ = window.update(cx, |_, win, cx| {
                    wb_for_mcp.update(cx, |wb, cx| {
                        wb.handle_office_mcp_call(call, win, cx);
                    });
                });
            }
        })
        .detach();

        cx.spawn(async move |cx| {
            let res = done_rx.recv().await.unwrap_or_else(|e| Err(e.to_string()));
            if let Ok(shot_path) = std::env::var("KWROK_SAVE_WINDOW_PNG")
                && !shot_path.is_empty()
            {
                let _ = window.update(cx, |_, win, cx| {
                    win.refresh();
                    win.draw(cx).clear(cx);
                });
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
            }
            let _ = bridge_tx.send(KarCommand::Shutdown).await;
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            match res {
                Ok(()) => {
                    println!("test {TEST_NAME} ... ok");
                    println!(
                        "\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
                    );
                    cx.update(|cx| cx.quit());
                }
                Err(err) => {
                    eprintln!("test {TEST_NAME} ... FAILED\nerror: {err}");
                    std::process::exit(1);
                }
            }
        })
        .detach();
    });
}

#[cfg(target_os = "macos")]
async fn verify_wps_office_mcp(
    workspace_dir: &Path,
    mcp_url: &str,
    mcp_auth: &str,
) -> Result<(), String> {
    if let Some(bridge_exe) = resolve_stdio_bridge_exe() {
        verify_over_stdio(&bridge_exe, workspace_dir, mcp_url, mcp_auth)
    } else {
        verify_over_http(workspace_dir, mcp_url, mcp_auth).await
    }
}

#[cfg(target_os = "macos")]
fn verify_over_stdio(
    bridge_exe: &Path,
    workspace_dir: &Path,
    mcp_url: &str,
    mcp_auth: &str,
) -> Result<(), String> {
    let mut child = Command::new(bridge_exe)
        .arg("--mcp-stdio-bridge")
        .env("KAR_MCP_HTTP_URL", mcp_url)
        .env("KAR_MCP_AUTH", mcp_auth)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn stdio bridge: {e}"))?;

    let mut stdin = child.stdin.take().ok_or("stdin")?;
    let mut reader = BufReader::new(child.stdout.take().ok_or("stdout")?);

    fn send_rpc(
        stdin: &mut std::process::ChildStdin,
        reader: &mut BufReader<std::process::ChildStdout>,
        req: Value,
    ) -> Result<Value, String> {
        let line = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        writeln!(stdin, "{line}").map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;
        let mut out = String::new();
        reader.read_line(&mut out).map_err(|e| e.to_string())?;
        serde_json::from_str(out.trim()).map_err(|e| format!("json parse ({out:?}): {e}"))
    }

    let init = send_rpc(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "wps-live-test", "version": "0.1.0" }
            }
        }),
    )?;
    let instructions = init["result"]["instructions"].as_str().unwrap_or_default();
    if !instructions.contains("office_create_document")
        || !instructions.contains("office_render_preview")
    {
        return Err(format!("MCP instructions missing proactive Office workflow: {instructions}"));
    }

    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())?;

    let result = run_wps_assertions(workspace_dir, &mut |id, name, arguments| {
        send_rpc(
            &mut stdin,
            &mut reader,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": { "name": name, "arguments": arguments }
            }),
        )
    });

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    result
}

#[cfg(target_os = "macos")]
async fn verify_over_http(
    workspace_dir: &Path,
    mcp_url: &str,
    mcp_auth: &str,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let mut session_id: Option<String> = None;

    let send = |client: &reqwest::Client, sid: &mut Option<String>, body: Value| {
        let client = client.clone();
        let url = mcp_url.to_owned();
        let auth = mcp_auth.to_owned();
        let cur_sid = sid.clone();
        async move {
            let mut req = client.post(&url).json(&body);
            if !auth.is_empty() {
                req = req.header("Authorization", auth);
            }
            if let Some(s) = cur_sid {
                req = req.header("mcp-session-id", s);
            }
            let resp = req.send().await.map_err(|e| e.to_string())?;
            let new_sid = resp
                .headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let status = resp.status();
            let text = resp.text().await.map_err(|e| e.to_string())?;
            Ok::<(reqwest::StatusCode, Option<String>, String), String>((status, new_sid, text))
        }
    };

    let (_, sid, init_txt) = send(
        &client,
        &mut session_id,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "wps-live-http", "version": "0.1.0" }
            }
        }),
    )
    .await?;
    if sid.is_some() {
        session_id = sid;
    }
    let init: Value = serde_json::from_str(&init_txt).map_err(|e| e.to_string())?;
    if !init["result"]["instructions"]
        .as_str()
        .unwrap_or_default()
        .contains("office_create_document")
    {
        return Err("missing office_create_document in MCP instructions".into());
    }
    let _ = send(
        &client,
        &mut session_id,
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    )
    .await?;

    let handle = tokio::runtime::Handle::current();
    run_wps_assertions(workspace_dir, &mut |id, name, arguments| {
        tokio::task::block_in_place(|| {
            handle.block_on(async {
                let (_, _, txt) = send(
                    &client,
                    &mut session_id,
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "method": "tools/call",
                        "params": { "name": name, "arguments": arguments }
                    }),
                )
                .await?;
                serde_json::from_str(&txt).map_err(|e| format!("parse ({txt}): {e}"))
            })
        })
    })
}

#[cfg(target_os = "macos")]
fn run_wps_assertions(
    workspace_dir: &Path,
    call_tool: &mut dyn FnMut(u64, &str, Value) -> Result<Value, String>,
) -> Result<(), String> {
    // 1. Test untitled scratch creation -> NEEDS_SAVE_AS structured recovery -> Save-As path binding!
    let created_untitled = call_tool(
        10,
        "office_create_document",
        json!({ "kind": "word", "untitled": true }),
    )?;
    let untitled_json: Value = serde_json::from_str(
        created_untitled["result"]["content"][0]["text"]
            .as_str()
            .ok_or("untitled text")?,
    )
    .map_err(|e| e.to_string())?;
    let untitled_tab_id = untitled_json["tab_id"].as_u64().ok_or("untitled tab_id")?;
    if untitled_json["needs_save_as"] != true {
        return Err(format!("expected needs_save_as=true: {untitled_json}"));
    }

    // Write rich WPS-styled essay into the untitled tab
    let edited = call_tool(
        11,
        "office_edit_word",
        json!({
            "tab_id": untitled_tab_id,
            "replace_all": true,
            "paragraphs": [
                {
                    "style": "标题",
                    "align": "center",
                    "font": "黑体",
                    "size_zh": "二号",
                    "bold": true,
                    "text": "阅读让生活更丰盈"
                },
                {
                    "style": "Subtitle",
                    "align": "center",
                    "font": "楷体",
                    "size_zh": "四号",
                    "color": "#475569",
                    "text": "——在快节奏时代守护内心的秩序与辽阔"
                },
                {
                    "style": "一级标题",
                    "font": "黑体",
                    "size_zh": "三号",
                    "bold": true,
                    "text": "一、阅读拓宽认知的边界"
                },
                {
                    "style": "正文",
                    "font": "宋体",
                    "ascii_font": "Times New Roman",
                    "size_zh": "小四",
                    "first_line_indent_chars": 2,
                    "line_spacing": 1.6,
                    "markdown": true,
                    "text": "**真正的阅读**从来不是信息的堆砌，而是一场跨越时间与空间的深度精神对话。当我们翻开书页，有限的个人经验便与人类数千年的智慧结晶汇合，让庸常的日子生出*沉静而恒久的光亮*。"
                },
                {
                    "style": "一级标题",
                    "font": "黑体",
                    "size_zh": "三号",
                    "bold": true,
                    "text": "二、不同阅读方式的长期价值对比"
                }
            ]
        }),
    )?;
    if edited["result"]["isError"] == true {
        return Err(format!("office_edit_word failed: {edited}"));
    }

    // Insert a WPS three-line academic table (`three_line`)
    let tbl = call_tool(
        12,
        "office_edit_word_table",
        json!({
            "tab_id": untitled_tab_id,
            "action": "insert",
            "border_style": "three_line",
            "header_row": true,
            "align": "center",
            "cells": [
                ["阅读形态", "建议频次", "核心收益"],
                ["经典深度精读", "每日 40 分钟", "锤炼逻辑体系与审美定力"],
                ["跨学科主题阅读", "每周 1 组", "打通知识孤岛并激发创造力"]
            ]
        }),
    )?;
    if tbl["result"]["isError"] == true {
        return Err(format!("office_edit_word_table failed: {tbl}"));
    }

    // Apply page setup + header + dynamic page numbers
    let pg = call_tool(
        13,
        "office_page_setup",
        json!({
            "tab_id": untitled_tab_id,
            "paper": "A4",
            "orientation": "portrait",
            "margin_preset": "official_zh",
            "header_text": "Kwrok 内置文档 · 精品排版示例",
            "header_align": "center",
            "page_numbers": true
        }),
    )?;
    if pg["result"]["isError"] == true {
        return Err(format!("office_page_setup failed: {pg}"));
    }

    // Calling office_save_tab WITHOUT path on an untitled document must return structured NEEDS_SAVE_AS!
    let needs_save = call_tool(14, "office_save_tab", json!({ "tab_id": untitled_tab_id }))?;
    if needs_save["result"]["isError"] == true {
        return Err(format!(
            "office_save_tab on untitled should return NEEDS_SAVE_AS, not hard error: {needs_save}"
        ));
    }
    let needs_json: Value = serde_json::from_str(
        needs_save["result"]["content"][0]["text"]
            .as_str()
            .ok_or("needs_save text")?,
    )
    .map_err(|e| e.to_string())?;
    if needs_json["status"] != "NEEDS_SAVE_AS" {
        return Err(format!("expected NEEDS_SAVE_AS, got: {needs_json}"));
    }
    let suggested_path = needs_json["suggested_path"]
        .as_str()
        .ok_or("missing suggested_path")?;
    if !suggested_path.ends_with("阅读让生活更丰盈.docx") {
        return Err(format!(
            "suggested_path did not infer document title: {suggested_path}"
        ));
    }

    // Now save to `suggested_path` via `office_save_tab`!
    let saved = call_tool(
        15,
        "office_save_tab",
        json!({ "tab_id": untitled_tab_id, "path": suggested_path }),
    )?;
    let saved_json: Value = serde_json::from_str(
        saved["result"]["content"][0]["text"]
            .as_str()
            .ok_or("saved text")?,
    )
    .map_err(|e| e.to_string())?;
    if saved_json["status"] != "saved" || !Path::new(suggested_path).is_file() {
        return Err(format!("Save-As did not persist file: {saved_json}"));
    }
    let canon_saved = Path::new(suggested_path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let canon_ws = workspace_dir.canonicalize().map_err(|e| e.to_string())?;
    if !canon_saved.starts_with(&canon_ws) {
        return Err(format!(
            "saved path outside workspace: {} vs {}",
            canon_saved.display(),
            canon_ws.display()
        ));
    }

    // Deep inspection check
    let inspected = call_tool(
        16,
        "office_inspect_document",
        json!({ "tab_id": untitled_tab_id }),
    )?;
    let insp_json: Value = serde_json::from_str(
        inspected["result"]["content"][0]["text"]
            .as_str()
            .ok_or("inspect text")?,
    )
    .map_err(|e| e.to_string())?;
    if insp_json["needs_save_as"] != false
        || insp_json["page_setup"]["margins_cm"]["top"] != 3.7
        || insp_json["tables"][0]["rows"] != 3
    {
        return Err(format!("unexpected inspect payload: {insp_json}"));
    }

    // Visual page preview check (multimodal PNG!)
    let preview = call_tool(
        17,
        "office_render_preview",
        json!({ "tab_id": untitled_tab_id, "page": 1, "dpi": 144 }),
    )?;
    let content_arr = preview["result"]["content"]
        .as_array()
        .ok_or("preview content array")?;
    if content_arr.len() < 2 || content_arr[1]["type"] != "image" {
        return Err(format!(
            "office_render_preview did not return multimodal image content: {preview}"
        ));
    }
    let b64 = content_arr[1]["data"].as_str().ok_or("missing png base64")?;
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| e.to_string())?;
    if !png_bytes.starts_with(b"\x89PNG\r\n\x1a\n") || png_bytes.len() < 8_000 {
        return Err(format!("invalid or tiny rendered page PNG ({} bytes)", png_bytes.len()));
    }
    if let Ok(dest) = std::env::var("KWROK_SAVE_PREVIEW_PNG")
        && !dest.is_empty()
    {
        let _ = std::fs::write(dest, &png_bytes);
    }
    if let Ok(docx_dest) = std::env::var("KWROK_SAVE_DOCX")
        && !docx_dest.is_empty()
    {
        if let Some(parent) = Path::new(&docx_dest).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::copy(suggested_path, docx_dest);
    }

    Ok(())
}
