//! Live macOS `WKWebView` + GPUI Metal window + `ServedMcpTools` + `--mcp-stdio-bridge`
//! end-to-end integration test (`harness = false` so AppKit & WebKit run on OS main thread).

#![forbid(unsafe_code)]

extern crate gpui_kit as gpui;

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use axum::{Router, response::Html, routing::get};
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

const TEST_NAME: &str = "live_wkwebview_mcp_and_zero_bleed";

const LIVE_PAGE_HTML: &str = r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8" />
  <title>Kwrok Live Browser Test</title>
</head>
<body style="font-family:sans-serif;padding:24px;background:#f8fafc;color:#0f172a">
  <h1>Edge Gateway Ready</h1>
  <input id="filter-box" placeholder="Region" value="" />
  <button id="apply-btn" onclick="document.getElementById('status-out').textContent = 'applied:' + document.getElementById('filter-box').value">Apply</button>
  <div id="status-out">idle</div>
  <script>
    console.warn("SlowQueryWarning: 142ms");
    console.error("QuotaMismatchError: synthetic test error");
  </script>
</body>
</html>
"#;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        println!("{TEST_NAME}: test\n\n1 test, 0 benchmarks");
        return;
    }
    if args.iter().any(|a| a == "--ignored") {
        println!("\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out\n");
        return;
    }
    if let Some(filter) = args.iter().find(|a| !a.starts_with('-'))
        && !TEST_NAME.contains(filter.as_str())
        && !"browser_live".contains(filter.as_str())
    {
        println!("\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out\n");
        return;
    }

    println!("\nrunning 1 test");

    #[cfg(not(target_os = "macos"))]
    {
        println!("test {TEST_NAME} ... ok");
        println!("\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n");
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

    let (mcp_evt_tx, mcp_evt_rx) = async_channel::bounded::<KarBridgeEvent>(64);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<(std::net::SocketAddr, String, String)>();
    let (done_tx, done_rx) = async_channel::bounded::<Result<(), String>>(1);

    // Spawn Tokio runtime hosting the live HTTP test server + ServedMcpTools + MCP client verifier.
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async move {
            let app_router = Router::new().route("/", get(|| async { Html(LIVE_PAGE_HTML) }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind test http");
            let http_addr = listener.local_addr().unwrap();
            let http_task = tokio::spawn(async move {
                let _ = axum::serve(listener, app_router).await;
            });

            let served = start_office_mcp_server(mcp_evt_tx)
                .await
                .expect("start office/browser MCP server");
            let (mcp_url, mcp_auth) = match &served.server {
                kar::McpServer::HostedHttp { url, headers }
                | kar::McpServer::Http { url, headers } => (
                    url.clone(),
                    headers.get("Authorization").cloned().unwrap_or_default(),
                ),
                other => panic!("unexpected MCP server variant: {other:?}"),
            };
            let _ = ready_tx.send((http_addr, mcp_url.clone(), mcp_auth.clone()));

            // Give the GPUI window 350ms to open and settle initial frame.
            tokio::time::sleep(Duration::from_millis(350)).await;

            let outcome = verify_browser_mcp_tools(http_addr, &mcp_url, &mcp_auth).await;
            served.close().await;
            http_task.abort();
            let _ = done_tx.send(outcome).await;
        });
    });

    let (_http_addr, _mcp_url, _mcp_auth) = ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("test servers ready");

    let app = gpui_kit::application().with_assets(kantor_ui::KantorAssets);
    app.run(move |cx| {
        init_runtime_ui(cx);
        let mut window_options = studio_window_options(
            APP_NAME,
            size(px(1160.), px(780.)),
            size(px(400.), px(360.)),
            cx,
        );
        window_options.focus = false;

        let wb_cfg = WorkbenchConfig {
            chat_dir: chat_dir.clone(),
            workspace_dir: workspace_dir.clone(),
            docs_dir: docs_dir.clone(),
            resume_last_chat: false,
            open_browser_tab: true,
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

        let wb_for_occlusion = entity;
        cx.spawn(async move |cx| {
            let res = done_rx.recv().await.unwrap_or_else(|e| Err(e.to_string()));
            let final_res = res.and_then(|()| {
                // Verify Zero Z-Order Bleed: opening the Command Palette immediately hides the
                // child WKWebView while keeping its Retina PNG backing snapshot cached!
                let mut check = Ok(());
                let _ = window.update(cx, |_, win, cx| {
                    wb_for_occlusion.update(cx, |wb, cx| {
                        wb.toggle_command_palette(win, cx);
                    });
                    win.refresh();
                    win.draw(cx).clear(cx);
                    let wb = wb_for_occlusion.read(cx);
                    let tab = wb.browser.active_tab().expect("active browser tab");
                    if tab.last_visible {
                        check = Err("WKWebView remained visible while Command Palette modal was open".into());
                    } else if tab.snapshot.is_none() {
                        check = Err("Retina PNG snapshot was not cached underneath Command Palette modal".into());
                    }
                });
                check
            });

            let _ = bridge_tx.send(KarCommand::Shutdown).await;
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
            match final_res {
                Ok(()) => {
                    println!("test {TEST_NAME} ... ok");
                    println!("\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n");
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
async fn verify_browser_mcp_tools(
    http_addr: std::net::SocketAddr,
    mcp_url: &str,
    mcp_auth: &str,
) -> Result<(), String> {
    // Exercise via `--mcp-stdio-bridge` subprocess if kar-desktop binary is present (testing
    // BOTH the stdio bridge used by Kwrok's default harness AND ServedMcpTools!), otherwise HTTP.
    if let Some(bridge_exe) = resolve_stdio_bridge_exe() {
        verify_over_stdio_bridge(&bridge_exe, http_addr, mcp_url, mcp_auth)
    } else {
        verify_over_http(http_addr, mcp_url, mcp_auth).await
    }
}

#[cfg(target_os = "macos")]
fn verify_over_stdio_bridge(
    bridge_exe: &std::path::Path,
    http_addr: std::net::SocketAddr,
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
        .map_err(|e| format!("spawn --mcp-stdio-bridge failed: {e}"))?;

    let mut stdin = child.stdin.take().ok_or("bridge stdin")?;
    let mut reader = BufReader::new(child.stdout.take().ok_or("bridge stdout")?);

    fn send_rpc(
        stdin: &mut std::process::ChildStdin,
        reader: &mut BufReader<std::process::ChildStdout>,
        req: Value,
    ) -> Result<Value, String> {
        let line = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        writeln!(stdin, "{line}").map_err(|e| format!("write stdin: {e}"))?;
        stdin.flush().map_err(|e| format!("flush stdin: {e}"))?;
        let mut out = String::new();
        reader
            .read_line(&mut out)
            .map_err(|e| format!("read stdout: {e}"))?;
        serde_json::from_str(out.trim())
            .map_err(|e| format!("parse bridge reply ({out:?}): {e}"))
    }

    // 1. initialize + notifications/initialized (notification produces 202 Accepted -> no stdout line!)
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
                "clientInfo": { "name": "browser-live-test", "version": "0.1.0" }
            }
        }),
    )?;
    if init["result"]["serverInfo"]["name"] != "kantor" {
        return Err(format!("unexpected initialize reply: {init}"));
    }
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())?;

    let mut rpc = |req: Value| send_rpc(&mut stdin, &mut reader, req);

    // 2. tools/list -> verify all 7 browser_* tools exist
    let list = rpc(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}))?;
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .ok_or("tools array")?
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for expected in [
        "browser_open",
        "browser_status",
        "browser_snapshot",
        "browser_act",
        "browser_eval",
        "browser_console",
        "browser_screenshot",
    ] {
        if !names.contains(&expected) {
            return Err(format!("missing MCP tool `{expected}` in {names:?}"));
        }
    }

    // 3. browser_open -> navigate live WKWebView to loopback server
    let target_url = format!("http://{http_addr}/");
    let opened = rpc(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "browser_open",
            "arguments": { "url": target_url }
        }
    }))?;
    let opened_txt = opened["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    if !opened_txt.contains("Kwrok Live Browser Test") {
        return Err(format!("browser_open did not load title: {opened}"));
    }

    // 4. browser_snapshot -> verify DOM headings & interactive selectors
    let snap = rpc(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": { "name": "browser_snapshot", "arguments": {} }
    }))?;
    let snap_txt = snap["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    if !snap_txt.contains("Edge Gateway Ready")
        || !snap_txt.contains("#filter-box")
        || !snap_txt.contains("#apply-btn")
    {
        return Err(format!("browser_snapshot missing expected DOM nodes: {snap_txt}"));
    }

    // 5. browser_console -> verify real console.error & console.warn captured via window.ipc
    let con = rpc(json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": { "name": "browser_console", "arguments": { "errors_only": true } }
    }))?;
    let con_txt = con["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    if !con_txt.contains("QuotaMismatchError: synthetic test error") {
        return Err(format!("browser_console did not capture page error: {con_txt}"));
    }

    // 6. browser_act (fill + click) + browser_eval -> verify live DOM interaction
    let fill = rpc(json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "tools/call",
        "params": {
            "name": "browser_act",
            "arguments": { "action": "fill", "selector": "#filter-box", "value": "ap-east-1" }
        }
    }))?;
    if !fill["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("\"ok\": true")
    {
        return Err(format!("browser_act fill failed: {fill}"));
    }

    let click = rpc(json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": {
            "name": "browser_act",
            "arguments": { "action": "click", "selector": "#apply-btn" }
        }
    }))?;
    if !click["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("\"ok\": true")
    {
        return Err(format!("browser_act click failed: {click}"));
    }

    let eval_res = rpc(json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "tools/call",
        "params": {
            "name": "browser_eval",
            "arguments": { "expression": "document.getElementById('status-out').textContent" }
        }
    }))?;
    if !eval_res["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("applied:ap-east-1")
    {
        return Err(format!("browser_eval did not reflect click result: {eval_res}"));
    }

    // 7. browser_screenshot -> verify multimodal image/png + saved Retina PNG file
    let shot = rpc(json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "tools/call",
        "params": { "name": "browser_screenshot", "arguments": {} }
    }))?;
    let content = shot["result"]["content"]
        .as_array()
        .ok_or_else(|| format!("screenshot content array missing: {shot}"))?;
    if content.len() < 2 {
        return Err(format!("expected multimodal [Text, Image] content, got: {shot}"));
    }
    let meta: Value = serde_json::from_str(content[0]["text"].as_str().unwrap_or("{}"))
        .map_err(|e| e.to_string())?;
    let saved_path = meta["saved_path"].as_str().ok_or("saved_path missing")?;
    if !std::path::Path::new(saved_path).is_file() {
        return Err(format!("saved_path does not exist: {saved_path}"));
    }
    let _ = std::fs::remove_file(saved_path);

    if content[1]["type"] != "image" || content[1]["mimeType"] != "image/png" {
        return Err(format!("unexpected image content block: {}", content[1]));
    }
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(content[1]["data"].as_str().unwrap_or_default())
        .map_err(|e| format!("decode png base64: {e}"))?;
    if png_bytes.len() < 64 || &png_bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("browser_screenshot did not return valid PNG bytes".into());
    }

    drop(stdin);
    let _ = child.wait();
    Ok(())
}

#[cfg(target_os = "macos")]
async fn verify_over_http(
    http_addr: std::net::SocketAddr,
    mcp_url: &str,
    mcp_auth: &str,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(mcp_url)
        .header("Authorization", mcp_auth)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "browser_open",
                "arguments": { "url": format!("http://{http_addr}/") }
            }
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<Value>()
        .await
        .map_err(|e| e.to_string())?;
    if !resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("Kwrok Live Browser Test")
    {
        return Err(format!("HTTP MCP browser_open failed: {resp}"));
    }
    Ok(())
}
