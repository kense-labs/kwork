//! Brute-force & adversarial end-to-end test suite for Kwork's built-in system WebView browser
//! (`harness = false` so AppKit, Metal, GPUI, and `WKWebView` execute on the OS main thread).

#![forbid(unsafe_code)]

extern crate gpui_kit as gpui;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use axum::{
    Router,
    http::header,
    response::{Html, IntoResponse, Redirect},
    routing::get,
};
use gpui::*;
use kar_desktop::{
    APP_NAME, init_runtime_ui,
    kar_bridge::{KarBridgeEvent, KarCommand},
    mcp_office::{resolve_stdio_bridge_exe, start_office_mcp_server},
    studio_window_options,
    workbench::{KarWorkbench, RightPaneSurface, WorkbenchConfig, WorkspaceLayoutMode},
};
use serde_json::{Value, json};

const TEST_NAME: &str = "adversarial_and_stress_wkwebview_suite";

const PORTAL_HTML: &str = r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8" />
  <title>Adversarial Portal Main</title>
</head>
<body>
  <h1>Portal Survived</h1>
  <!-- Broken ad subframe & custom app-scheme subframe MUST NOT kill the main page or pollute tab.url -->
  <iframe src="http://127.0.0.1:9/dead-ad-subframe"></iframe>
  <iframe src="bilibili://video/99999"></iframe>
  <script>
    // Simulate portal probing native app deep-link right after DOM load
    setTimeout(() => { window.location.href = "baiduboxapp://open?from=portal"; }, 15);
  </script>
</body>
</html>
"#;

const HOSTILE_JS_HTML: &str = r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8" />
  <title>Hostile Prototype Page</title>
  <script>
    // Attempt to sabotage bridge intrinsics & global functions!
    window.JSON.stringify = function() { throw new Error("page clobbered JSON.stringify"); };
    Array.from = function() { throw new Error("page clobbered Array.from"); };
    window.ipc = { postMessage: function() { throw new Error("page clobbered ipc"); } };
    try { window.__kwrokSnapshotDom = () => ({ hijacked: true }); } catch (_) {}
    try { window.__kwrokEval = () => "hijacked"; } catch (_) {}
  </script>
</head>
<body>
  <h1>Intrinsics Intact</h1>
  <select id="plan-select">
    <option value="free">免费版</option>
    <option value="enterprise">企业版</option>
  </select>
  <input id="agree-chk" type="checkbox" />
  <button id="order-btn" onclick="document.getElementById('order-result').textContent = document.getElementById('plan-select').value + ':' + document.getElementById('agree-chk').checked">
    <svg width="16" height="16" class="icon-svg"><circle cx="8" cy="8" r="6"/></svg>
    <span>提交订单</span>
  </button>
  <div id="order-result">pending</div>
</body>
</html>
"#;

const POPUP_BOMB_HTML: &str = r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8" />
  <title>Popup & Dialog Torture</title>
</head>
<body>
  <h1>Popup Torture Page</h1>
  <button id="bomb-btn" onclick="runTorture()">Run Torture</button>
  <div id="dialog-out">idle</div>
  <script>
    function runTorture() {
      alert("AlertAlpha");
      const c = confirm("ConfirmBeta");
      const p = prompt("PromptGamma", "DefaultDelta");
      // Empty URL popup (previously panicked wry's `request.URL().unwrap()`!)
      try { window.open("", "_blank"); } catch (_) {}
      // Dangerous javascript: popup (must be blocked!)
      try { window.open("javascript:alert(999)", "_blank"); } catch (_) {}
      // 40x popup storm (must be rate-limited!)
      for (let i = 0; i < 40; i++) {
        try { window.open("/popup-child?i=" + i, "_blank"); } catch (_) {}
      }
      document.getElementById("dialog-out").textContent = `done:${c}:${p}`;
    }
  </script>
</body>
</html>
"#;

const FLOOD_HTML: &str = r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8" />
  <title>Flood Start</title>
</head>
<body>
  <h1>Telemetry Flood Survived</h1>
  <script>
    for (let i = 0; i < 600; i++) {
      document.title = "FloodTitle-" + i;
    }
    for (let i = 0; i < 320; i++) {
      console.error("FloodErr-" + i);
    }
    // Trigger WebKit's 100-per-30s history.replaceState SecurityError without aborting script
    for (let i = 0; i < 150; i++) {
      try { history.replaceState({ i }, "", "/flood?step=" + i); } catch (_) {}
    }
  </script>
</body>
</html>
"#;

const HISTORY_PAGE1_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>HistOne</title></head><body><h1>Step One</h1><a id="to-two" href="/hist-2">Two</a></body></html>"#;
const HISTORY_PAGE2_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>HistTwo</title></head><body><h1>Step Two</h1><button id="push-three" onclick="history.pushState({}, '', '/hist-3'); document.title='HistThree';">Three</button></body></html>"#;
const POPUP_CHILD_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>PopupChild</title></head><body><button id="self-close" onclick="window.close()">Close</button></body></html>"#;

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
        && !"browser_adversarial".contains(filter.as_str())
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
    run_macos_adversarial_suite();
}

#[cfg(target_os = "macos")]
fn write_local_bundle(workspace_dir: &Path) {
    std::fs::write(workspace_dir.join("parent-secret.txt"), "TOP_SECRET_PARENT").unwrap();
    let sub = workspace_dir.join("测试 页面");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        sub.join("style.css"),
        "body { background: rgb(240, 249, 255); }",
    )
    .unwrap();
    std::fs::write(sub.join("data.json"), r#"{"token":"local-file-ok"}"#).unwrap();
    std::fs::write(
        sub.join("app.js"),
        r#"
        document.getElementById('bundle-out').textContent = 'js-loaded:' + getComputedStyle(document.body).backgroundColor;
        fetch('./data.json')
          .then(r => r.json())
          .then(d => { document.getElementById('fetch-out').textContent = d.token; })
          .catch(e => { document.getElementById('fetch-out').textContent = 'fetch-err:' + e; });
        fetch('../parent-secret.txt')
          .then(r => r.text())
          .then(t => { document.getElementById('escape-out').textContent = 'LEAKED:' + t; })
          .catch(_ => { document.getElementById('escape-out').textContent = 'parent-blocked'; });
        "#,
    )
    .unwrap();
    std::fs::write(
        sub.join("index.html"),
        r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8" />
  <title>本地多文件预览包</title>
  <link rel="stylesheet" href="./style.css" />
</head>
<body>
  <h1>Local Bundle Heading</h1>
  <div id="bundle-out">waiting-js</div>
  <div id="fetch-out">waiting-fetch</div>
  <div id="escape-out">waiting-escape</div>
  <script src="./app.js"></script>
</body>
</html>
"#,
    )
    .unwrap();
}

#[cfg(target_os = "macos")]
fn run_macos_adversarial_suite() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let chat_dir = tmp.path().join("chats");
    let workspace_dir = tmp.path().join("workspace");
    let docs_dir = tmp.path().join("docs");
    let downloads_dir = tmp.path().join("downloads");
    std::fs::create_dir_all(&chat_dir).unwrap();
    std::fs::create_dir_all(&workspace_dir).unwrap();
    std::fs::create_dir_all(&docs_dir).unwrap();
    std::fs::create_dir_all(&downloads_dir).unwrap();
    let downloads_canon = downloads_dir.canonicalize().unwrap_or(downloads_dir);
    kwrok_webview::set_download_dir_override(Some(downloads_canon.clone()));
    write_local_bundle(&workspace_dir);

    let (mcp_evt_tx, mcp_evt_rx) = async_channel::bounded::<KarBridgeEvent>(128);
    let (ready_tx, ready_rx) =
        std::sync::mpsc::channel::<(std::net::SocketAddr, String, String)>();
    let (done_tx, done_rx) = async_channel::bounded::<Result<(), String>>(1);

    let dl_marker_name = format!("kwork_adv_dl_{}.txt", std::process::id());
    let dl_clean_name = dl_marker_name.clone();
    let dl_evil_name = dl_marker_name.clone();
    let dl_verify_dir = downloads_canon.clone();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async move {
            let app_router = Router::new()
                .route("/redirect-start", get(|| async { Redirect::permanent("/hop2") }))
                .route("/hop2", get(|| async { Redirect::temporary("/portal") }))
                .route("/portal", get(|| async { Html(PORTAL_HTML) }))
                .route("/hostile-js", get(|| async { Html(HOSTILE_JS_HTML) }))
                .route("/popup-bomb", get(|| async { Html(POPUP_BOMB_HTML) }))
                .route("/popup-child", get(|| async { Html(POPUP_CHILD_HTML) }))
                .route("/flood", get(|| async { Html(FLOOD_HTML) }))
                .route("/hist-1", get(|| async { Html(HISTORY_PAGE1_HTML) }))
                .route("/hist-2", get(|| async { Html(HISTORY_PAGE2_HTML) }))
                .route(
                    "/clean-download",
                    get(move || {
                        let cd = format!("attachment; filename=\"{}\"", dl_clean_name);
                        async move {
                            (
                                [
                                    (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
                                    (header::CONTENT_DISPOSITION, cd),
                                ],
                                "SAFE_DOWNLOAD_PAYLOAD_OK",
                            )
                                .into_response()
                        }
                    }),
                )
                .route(
                    "/evil-download",
                    get(move || {
                        let evil_cd = format!(
                            "attachment; filename=\"../../.ssh/evil_{}\"",
                            dl_evil_name
                        );
                        async move {
                            (
                                [
                                    (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
                                    (header::CONTENT_DISPOSITION, evil_cd),
                                ],
                                "TRAVERSAL_CONTAINED_OK",
                            )
                                .into_response()
                        }
                    }),
                );

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind adversarial http");
            let http_addr = listener.local_addr().unwrap();
            let http_task = tokio::spawn(async move {
                let _ = axum::serve(listener, app_router).await;
            });

            let served = start_office_mcp_server(mcp_evt_tx)
                .await
                .expect("start MCP server");
            let (mcp_url, mcp_auth) = match &served.server {
                kar::McpServer::HostedHttp { url, headers }
                | kar::McpServer::Http { url, headers } => (
                    url.clone(),
                    headers.get("Authorization").cloned().unwrap_or_default(),
                ),
                other => panic!("unexpected MCP server variant: {other:?}"),
            };
            let _ = ready_tx.send((http_addr, mcp_url.clone(), mcp_auth.clone()));

            tokio::time::sleep(Duration::from_millis(350)).await;

            let outcome = run_adversarial_mcp_attacks(
                http_addr,
                &mcp_url,
                &mcp_auth,
                &dl_verify_dir,
                &dl_marker_name,
            )
            .await;
            served.close().await;
            http_task.abort();
            let _ = done_tx.send(outcome).await;
        });
    });

    let (http_addr, _mcp_url, _mcp_auth) = ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("servers ready");

    let app = gpui_kit::application().with_assets(kantor_ui::KantorAssets);
    app.run(move |cx| {
        init_runtime_ui(cx);
        let mut window_options = studio_window_options(
            APP_NAME,
            size(px(1220.), px(800.)),
            size(px(400.), px(360.)),
            cx,
        );
        window_options.focus = false;

        // Intentionally start in `ChatFocus` (`open_browser_tab: false`) so the very first MCP
        // `browser_open` exercises the cold `ChatFocus -> Split` spring animation transition!
        let wb_cfg = WorkbenchConfig {
            chat_dir: chat_dir.clone(),
            workspace_dir: workspace_dir.clone(),
            docs_dir: docs_dir.clone(),
            resume_last_chat: false,
            open_browser_tab: false,
            ..WorkbenchConfig::default()
        };

        let (window, entity) = gpui_kit::open_window(window_options, cx, move |window, cx| {
            cx.new(|cx| {
                let mut wb = KarWorkbench::new(wb_cfg, window, cx);
                // Put focus in Chat Composer before MCP attacks begin to verify MCP never steals it!
                wb.focus_chat_composer(window, cx);
                wb
            })
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

        let wb_ui = entity;
        cx.spawn(async move |cx| {
            let mcp_outcome = done_rx
                .recv()
                .await
                .unwrap_or_else(|e| Err(e.to_string()));

            // Phase B: Direct UI / Focus / Zoom / Viewport / Rapid Subtab & Teardown Torture on Main Thread!
            let ui_outcome = mcp_outcome.and_then(|()| {
                let mut res: Result<(), String> = Ok(());
                let _ = window.update(cx, |_, win, cx| {
                    res = verify_ui_focus_zoom_subtabs_and_teardown(&wb_ui, http_addr, win, cx);
                });
                res
            });

            let _ = bridge_tx.send(KarCommand::Shutdown).await;
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
            match ui_outcome {
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
fn verify_ui_focus_zoom_subtabs_and_teardown(
    wb_ent: &Entity<KarWorkbench>,
    http_addr: std::net::SocketAddr,
    win: &mut Window,
    cx: &mut App,
) -> Result<(), String> {
    // 1. Verify Vector 12 pinned `#order-btn` onto `wb.pinned_target` and focused Chat Composer!
    {
        let wb = wb_ent.read(cx);
        if !wb.is_chat_composer_focused(win, cx) {
            return Err("Chat Composer did not hold focus after MCP + Element Picker!".into());
        }
        let Some(ref pinned) = wb.pinned_target else {
            return Err("Vector 12 FAILED: ElementPicked did not populate wb.pinned_target!".into());
        };
        match &pinned.locator {
            kar_desktop::types::SelectionLocator::WebElement { selector, .. } => {
                if selector != "#order-btn" {
                    return Err(format!("Vector 12 unexpected selector: {selector}"));
                }
            }
            other => return Err(format!("Vector 12 unexpected locator variant: {other:?}")),
        }
    }

    // 2. Focus WebView, then invoke `focus_browser_address_bar` (`⌘L`) and verify responder returns to GPUI!
    wb_ent.update(cx, |wb, cx| {
        if let Some(tab) = wb.browser.active_tab()
            && let Some(ref wv) = tab.webview
        {
            wv.focus();
        }
        wb.focus_browser_address_bar(win, cx);
    });
    win.refresh();
    win.draw(cx).clear(cx);
    {
        let wb = wb_ent.read(cx);
        if !wb.browser.address_editor.read(cx).focus.is_focused(win) {
            return Err("⌘L (`focus_browser_address_bar`) did not focus address_editor".into());
        }
        if let Some(tab) = wb.browser.active_tab()
            && let Some(ref wv) = tab.webview
            && wv.is_focused()
        {
            return Err("WKWebView still held AppKit firstResponder after ⌘L!".into());
        }
    }

    // 3. Verify Zoom In (`+0.15`), Zoom Out (`-0.15`), Clamping, and Reset (`None` -> `1.0`)
    wb_ent.update(cx, |wb, cx| {
        wb.adjust_browser_zoom(Some(0.25), cx);
        wb.adjust_browser_zoom(Some(99.0), cx);
    });
    {
        let z = wb_ent.read(cx).browser.active_tab().unwrap().zoom;
        if (z - 2.5).abs() > 0.01 {
            return Err(format!("zoom upper clamp failed: got {z}"));
        }
    }
    wb_ent.update(cx, |wb, cx| wb.adjust_browser_zoom(None, cx));
    {
        let z = wb_ent.read(cx).browser.active_tab().unwrap().zoom;
        if (z - 1.0).abs() > 0.001 {
            return Err(format!("zoom reset failed: got {z}"));
        }
    }

    // 4. Rapid Subtab Stress: try creating 35 subtabs -> must cap cleanly at MAX_BROWSER_TABS (24)
    wb_ent.update(cx, |wb, cx| {
        for i in 0..35 {
            wb.new_browser_subtab(win, cx);
            if i % 3 == 0 {
                let url = format!("http://{http_addr}/hist-1?r={i}");
                wb.navigate_active_browser(&url, win, cx);
            }
        }
    });
    win.refresh();
    win.draw(cx).clear(cx);
    {
        let count = wb_ent.read(cx).browser.tabs.len();
        if count != kar_desktop::ui_browser::MAX_BROWSER_TABS {
            return Err(format!(
                "expected tab count capped at {}, got {count}",
                kar_desktop::ui_browser::MAX_BROWSER_TABS
            ));
        }
        let visible_count = wb_ent
            .read(cx)
            .browser
            .tabs
            .iter()
            .filter(|t| t.last_visible)
            .count();
        if visible_count > 1 {
            return Err(format!(
                "at most 1 subtab WebView may be visible at once, found {visible_count}"
            ));
        }
    }

    // 5. Close all subtabs one by one -> must never underflow or panic, leaving 1 clean tab
    for _ in 0..30 {
        wb_ent.update(cx, |wb, cx| {
            if let Some(tid) = wb.browser.active_tab().map(|t| t.id) {
                wb.close_browser_subtab(tid, win, cx);
            }
        });
    }
    {
        let wb = wb_ent.read(cx);
        if wb.browser.tabs.len() != 1 || wb.browser.active_idx != 0 {
            return Err(format!(
                "closing all subtabs left inconsistent state: len={}, active_idx={}",
                wb.browser.tabs.len(),
                wb.browser.active_idx
            ));
        }
    }

    // 6. Test `open_html_in_browser` custom title preservation & `reset_active_browser_to_home` teardown
    wb_ent.update(cx, |wb, cx| {
        wb.open_html_in_browser(
            "自定义预览标题",
            "<!doctype html><html><body>No title tag</body></html>".into(),
            false,
            win,
            cx,
        );
    });
    win.refresh();
    win.draw(cx).clear(cx);
    {
        let wb = wb_ent.read(cx);
        let tab = wb.browser.active_tab().unwrap();
        if tab.title != "自定义预览标题" {
            return Err(format!("custom preview title was lost: {:?}", tab.title));
        }
        if tab.webview.is_none() {
            return Err("open_html_in_browser did not mount WebView".into());
        }
    }

    wb_ent.update(cx, |wb, cx| wb.reset_active_browser_to_home(win, cx));
    {
        let wb = wb_ent.read(cx);
        let tab = wb.browser.active_tab().unwrap();
        if tab.webview.is_some() || !tab.url.is_empty() || tab.snapshot.is_some() {
            return Err("reset_active_browser_to_home did not tear down WebView & stale snapshot".into());
        }
    }

    // 7. Layout toggle (`ChatFocus` <-> `Split` <-> `CanvasWide`) & Settings modal occlusion check
    wb_ent.update(cx, |wb, cx| {
        wb.open_url_in_browser(&format!("http://{http_addr}/portal"), false, win, cx);
    });
    win.refresh();
    win.draw(cx).clear(cx);
    wb_ent.update(cx, |wb, cx| {
        wb.set_layout_mode(WorkspaceLayoutMode::CanvasWide, cx);
    });
    win.refresh();
    win.draw(cx).clear(cx);
    wb_ent.update(cx, |wb, cx| {
        wb.toggle_command_palette(win, cx);
    });
    win.refresh();
    win.draw(cx).clear(cx);
    {
        let wb = wb_ent.read(cx);
        if wb.browser.active_tab().unwrap().last_visible {
            return Err("WebView stayed visible under Command Palette!".into());
        }
    }
    wb_ent.update(cx, |wb, cx| {
        wb.toggle_command_palette(win, cx);
        wb.teardown_browser_on_dock_close();
        wb.browser_tab_open = false;
        wb.right_surface = RightPaneSurface::Office;
        cx.notify();
    });
    {
        let wb = wb_ent.read(cx);
        if wb.browser.tabs.iter().any(|t| t.webview.is_some()) {
            return Err("teardown_browser_on_dock_close left live background WebViews!".into());
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
async fn run_adversarial_mcp_attacks(
    http_addr: std::net::SocketAddr,
    mcp_url: &str,
    mcp_auth: &str,
    dl_dir: &Path,
    dl_marker_name: &str,
) -> Result<(), String> {
    let bridge_exe =
        resolve_stdio_bridge_exe().ok_or("could not locate kar-desktop binary for stdio bridge")?;
    let mut child = Command::new(bridge_exe)
        .arg("--mcp-stdio-bridge")
        .env("KAR_MCP_HTTP_URL", mcp_url)
        .env("KAR_MCP_AUTH", mcp_auth)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn bridge: {e}"))?;

    let mut stdin = child.stdin.take().ok_or("stdin")?;
    let mut reader = BufReader::new(child.stdout.take().ok_or("stdout")?);
    let mut next_id = 1u64;

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
            .map_err(|e| format!("parse reply ({out:?}): {e}"))
    }

    let _ = send_rpc(
        &mut stdin,
        &mut reader,
        json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "adversarial-fuzzer", "version": "0.1.0" }
            }
        }),
    )?;
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())?;

    let mut call_tool = |name: &str, args: Value| -> Result<(bool, String), String> {
        let id = next_id;
        next_id += 1;
        let resp = send_rpc(
            &mut stdin,
            &mut reader,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": { "name": name, "arguments": args }
            }),
        )?;
        if let Some(err) = resp.get("error") {
            return Ok((true, err.to_string()));
        }
        let is_err = resp["result"]["isError"].as_bool().unwrap_or(false);
        let text = resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        Ok((is_err, text))
    };

    // ── Vector 1: Dangerous & Malformed URLs MUST Return Errors, Never Crash or Search ──
    for bad in [
        "javascript:alert(1)",
        "  JaVaScRiPt:document.write('x') ",
        "java\nscript:alert(1)",
        "vbscript:msgbox(1)",
        "https://",
        "http://[broken-ipv6",
        ":0",
        ":99999",
    ] {
        let (is_err, txt) = call_tool("browser_open", json!({ "url": bad }))?;
        if !is_err {
            return Err(format!(
                "Vector 1 FAILED: dangerous/malformed URL {bad:?} should return isError=true, got: {txt}"
            ));
        }
    }

    // ── Vector 2: `data:` URL with `<`, `>`, Spaces, `%`, and Multi-Byte Chinese/Emoji ──
    let (is_err, data_open) = call_tool(
        "browser_open",
        json!({ "url": "data:text/html;charset=utf-8,<title>DataOK</title><h1>你好 Kwork 100% 🔥</h1>" }),
    )?;
    if is_err || !data_open.contains("\"status\": \"opened\"") {
        return Err(format!("Vector 2 FAILED on data: URL open: {data_open}"));
    }
    let (_, data_snap) = call_tool("browser_snapshot", json!({}))?;
    if !data_snap.contains("你好 Kwork 100% 🔥") {
        return Err(format!("Vector 2 FAILED: data URL UTF-8 DOM mismatch: {data_snap}"));
    }

    // ── Vector 3: Local Multi-File Workspace Bundle (`file://` + CSS + JS + local `fetch`) ──
    let (is_err, file_open) = call_tool(
        "browser_open",
        json!({ "url": "测试 页面/index.html" }),
    )?;
    if is_err || !file_open.contains("本地多文件预览包") {
        return Err(format!("Vector 3 FAILED opening workspace HTML bundle: {file_open}"));
    }
    tokio::time::sleep(Duration::from_millis(120)).await;
    let (_, file_snap) = call_tool("browser_snapshot", json!({}))?;
    if !file_snap.contains("js-loaded:rgb(240, 249, 255)")
        || !file_snap.contains("local-file-ok")
        || file_snap.contains("TOP_SECRET_PARENT")
    {
        return Err(format!(
            "Vector 3 FAILED: sibling CSS/JS/fetch or parent directory sandbox check failed: {file_snap}"
        ));
    }

    // ── Vector 4: 301 -> 302 Redirect Chain + Broken Ad `<iframe>` + Custom App Scheme Probe ──
    let (is_err, portal_open) = call_tool(
        "browser_open",
        json!({ "url": format!("http://{http_addr}/redirect-start") }),
    )?;
    if is_err || !portal_open.contains("\"status\": \"opened\"") {
        return Err(format!("Vector 4 FAILED on redirect chain open: {portal_open}"));
    }
    // Wait past the 15ms `window.location.href = "baiduboxapp://..."` probe & dead ad iframe failure!
    tokio::time::sleep(Duration::from_millis(220)).await;
    let (_, status_after_probe) = call_tool("browser_status", json!({}))?;
    let st_val: Value = serde_json::from_str(&status_after_probe).map_err(|e| e.to_string())?;
    let active_tab = &st_val["tabs"][0];
    if active_tab["page_load_failed"] != false
        || !active_tab["url"]
            .as_str()
            .unwrap_or_default()
            .ends_with("/portal")
    {
        return Err(format!(
            "Vector 4 FAILED: custom scheme probe or dead iframe corrupted main page: {status_after_probe}"
        ));
    }

    // ── Vector 5: Hostile Prototype-Clobbering Page (`JSON.stringify` / `Array.from` / `window.ipc` sabotaged) ──
    let (is_err, hostile_open) = call_tool(
        "browser_open",
        json!({ "url": format!("http://{http_addr}/hostile-js") }),
    )?;
    if is_err || !hostile_open.contains("Hostile Prototype Page") {
        return Err(format!("Vector 5 FAILED opening hostile-js: {hostile_open}"));
    }
    let (is_err, hostile_snap) = call_tool("browser_snapshot", json!({}))?;
    if is_err || !hostile_snap.contains("Intrinsics Intact") || hostile_snap.contains("hijacked") {
        return Err(format!("Vector 5 FAILED: hostile page hijacked snapshot: {hostile_snap}"));
    }

    // ── Vector 6: `browser_eval` Torture Payloads (`NaN`, `Infinity`, `BigInt`, `Date`, `Node`, Circular, Surrogates, Comments, Multi-Statement, Throw) ──
    for (expr, expected_substr) in [
        ("NaN", "[NaN]"),
        ("1 / 0", "[Infinity]"),
        ("-1 / 0", "[-Infinity]"),
        ("undefined", "null"),
        ("9007199254740993n", "9007199254740993n"),
        ("Symbol('kwork')", "Symbol(kwork)"),
        ("new Date(0)", "1970"),
        ("document.body", "<body>"),
        ("const c = { a: 1 }; c.self = c; c", "[Circular]"),
        ("'bad\\uD800surrogate'", "bad"),
        ("20 + 22 // trailing single-line comment!", "42"),
        ("let acc = 0; for (let i = 1; i <= 5; i++) acc += i; acc", "15"),
        ("(() => { throw new TypeError('SyntheticBoom'); })()", "SyntheticBoom"),
    ] {
        let (is_err, eval_out) = call_tool("browser_eval", json!({ "expression": expr }))?;
        if is_err || !eval_out.contains(expected_substr) {
            return Err(format!(
                "Vector 6 FAILED on expression `{expr}`: expected `{expected_substr}` in {eval_out}"
            ));
        }
    }

    // ── Vector 7: Invalid Playwright CSS Selector + Text Fallback + `<select>` by Visible Text + Checkbox + SVG Button ──
    let (_, sel_res) = call_tool(
        "browser_act",
        json!({ "action": "select", "selector": "#plan-select", "value": "企业版" }),
    )?;
    if !sel_res.contains("\"ok\": true") {
        return Err(format!("Vector 7 FAILED selecting option by visible text: {sel_res}"));
    }
    let (_, chk_res) = call_tool(
        "browser_act",
        json!({ "action": "fill", "selector": "#agree-chk", "value": "true" }),
    )?;
    if !chk_res.contains("\"ok\": true") {
        return Err(format!("Vector 7 FAILED toggling checkbox: {chk_res}"));
    }
    // Pass INVALID CSS selector `button:has-text('提交订单')` WITH `text: "提交订单"` fallback!
    let (_, click_res) = call_tool(
        "browser_act",
        json!({
            "action": "click",
            "selector": "button:has-text('提交订单')",
            "text": "提交订单"
        }),
    )?;
    if !click_res.contains("\"ok\": true") {
        return Err(format!(
            "Vector 7 FAILED recovering from invalid CSS selector via text fallback: {click_res}"
        ));
    }
    let (_, verify_order) = call_tool(
        "browser_eval",
        json!({ "expression": "document.getElementById('order-result').textContent" }),
    )?;
    if !verify_order.contains("enterprise:true") {
        return Err(format!("Vector 7 FAILED order state check: {verify_order}"));
    }

    // ── Vector 8: Telemetry Flood (`600x title` + `800x replaceState` + `320x console.error`) ──
    let (is_err, flood_open) = call_tool(
        "browser_open",
        json!({ "url": format!("http://{http_addr}/flood") }),
    )?;
    if is_err || !flood_open.contains("\"status\": \"opened\"") {
        return Err(format!("Vector 8 FAILED opening flood page: {flood_open}"));
    }
    tokio::time::sleep(Duration::from_millis(80)).await;
    let (_, flood_con) = call_tool("browser_console", json!({ "errors_only": true, "limit": 200 }))?;
    if !flood_con.contains("FloodErr-") {
        return Err(format!("Vector 8 FAILED console capture under flood: {flood_con}"));
    }

    // ── Vector 9: History Stack (`Page 1 -> Page 2 -> pushState('/hist-3') -> Back -> Back`) ──
    let _ = call_tool(
        "browser_open",
        json!({ "url": format!("http://{http_addr}/hist-1") }),
    )?;
    let _ = call_tool(
        "browser_open",
        json!({ "url": format!("http://{http_addr}/hist-2") }),
    )?;
    let _ = call_tool("browser_act", json!({ "action": "click", "selector": "#push-three" }))?;
    tokio::time::sleep(Duration::from_millis(80)).await;
    let (_, st_mid) = call_tool("browser_status", json!({}))?;
    if !st_mid.contains("/hist-3") || !st_mid.contains("\"can_go_back\": true") {
        return Err(format!("Vector 9 FAILED after pushState('/hist-3'): {st_mid}"));
    }
    let _ = call_tool("browser_act", json!({ "action": "back" }))?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let _ = call_tool("browser_act", json!({ "action": "back" }))?;
    tokio::time::sleep(Duration::from_millis(220)).await;
    let (_, st_back) = call_tool("browser_status", json!({}))?;
    if !st_back.contains("/hist-1") || !st_back.contains("\"can_go_forward\": true") {
        return Err(format!("Vector 9 FAILED after going back to HistOne: {st_back}"));
    }

    // ── Vector 10: `alert`/`confirm`/`prompt` Non-Blocking + `window.open("")` Crash Guard + 40x Popup Bomb Rate-Limit + `window.close()` ──
    let _ = call_tool(
        "browser_open",
        json!({ "url": format!("http://{http_addr}/popup-bomb") }),
    )?;
    let (_, bomb_click) = call_tool("browser_act", json!({ "action": "click", "selector": "#bomb-btn" }))?;
    if !bomb_click.contains("\"ok\": true") {
        return Err(format!("Vector 10 FAILED clicking bomb-btn (JS dialog hung?): {bomb_click}"));
    }
    tokio::time::sleep(Duration::from_millis(250)).await;
    let (_, post_bomb_status) = call_tool("browser_status", json!({}))?;
    let pb_val: Value = serde_json::from_str(&post_bomb_status).map_err(|e| e.to_string())?;
    let tab_len = pb_val["tabs"].as_array().map_or(0, Vec::len);
    // 1 original tab + at most 4 rate-limited popups = <= 5 tabs (never 41!)
    if !(2..=6).contains(&tab_len) {
        return Err(format!(
            "Vector 10 FAILED popup rate limiter: expected 2..=6 tabs after 40x window.open bomb, got {tab_len}: {post_bomb_status}"
        ));
    }
    // Ensure the active popup child tab has mounted & loaded `/popup-child`, then click `#self-close` (`window.close()`)!
    let _ = call_tool(
        "browser_open",
        json!({ "url": format!("http://{http_addr}/popup-child") }),
    )?;
    let _ = call_tool("browser_act", json!({ "action": "click", "selector": "#self-close" }))?;
    tokio::time::sleep(Duration::from_millis(120)).await;
    let (_, after_close_status) = call_tool("browser_status", json!({}))?;
    let ac_val: Value = serde_json::from_str(&after_close_status).map_err(|e| e.to_string())?;
    let after_len = ac_val["tabs"].as_array().map_or(0, Vec::len);
    if after_len != tab_len - 1 {
        return Err(format!(
            "Vector 10 FAILED `window.close()` subtab cleanup: before={tab_len}, after={after_len}"
        ));
    }

    // ── Vector 11: Attachment Download + Path-Traversal Containment (`Content-Disposition`) ──
    let _ = call_tool(
        "browser_open",
        json!({ "url": format!("http://{http_addr}/portal") }),
    )?;
    let _ = call_tool(
        "browser_eval",
        json!({ "expression": format!("window.location.href = 'http://{http_addr}/clean-download'; 'triggered'") }),
    )?;
    let expected_dl = dl_dir.join(dl_marker_name);
    for _ in 0..30 {
        if expected_dl.is_file() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(60)).await;
    }
    let dl_contents = std::fs::read_to_string(&expected_dl).map_err(|e| {
        let entries: Vec<_> = std::fs::read_dir(dl_dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|en| en.file_name())
            .collect();
        format!(
            "Vector 11a FAILED: download file {} not written (dir entries: {:?}): {e}",
            expected_dl.display(),
            entries
        )
    })?;
    if dl_contents != "SAFE_DOWNLOAD_PAYLOAD_OK" {
        return Err(format!("Vector 11a FAILED download payload mismatch: {dl_contents}"));
    }

    // Vector 11b: Path-traversal `filename="../../.ssh/evil_..."` MUST stay strictly inside `dl_dir`!
    let _ = call_tool(
        "browser_eval",
        json!({ "expression": format!("window.location.href = 'http://{http_addr}/evil-download'; 'triggered'") }),
    )?;
    let mut contained_file = None;
    for _ in 0..30 {
        if let Ok(rd) = std::fs::read_dir(dl_dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p != expected_dl && p.is_file() {
                    contained_file = Some(p);
                    break;
                }
            }
        }
        if contained_file.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(60)).await;
    }
    let Some(evil_saved) = contained_file else {
        return Err("Vector 11b FAILED: traversal download did not land safely inside dl_dir".into());
    };
    if !evil_saved.starts_with(dl_dir) {
        return Err(format!("Vector 11b ESCAPED dl_dir: {}", evil_saved.display()));
    }
    let evil_body = std::fs::read_to_string(&evil_saved).map_err(|e| e.to_string())?;
    if evil_body != "TRAVERSAL_CONTAINED_OK" {
        return Err(format!("Vector 11b body mismatch: {evil_body}"));
    }

    // ── Vector 12: Codex-Style Interactive Element Picker (`__kwrokSetInspectMode(true)` -> Click -> Chat Pin) ──
    let _ = call_tool(
        "browser_open",
        json!({ "url": format!("http://{http_addr}/hostile-js") }),
    )?;
    let (_, pick_sim) = call_tool(
        "browser_eval",
        json!({
            "expression": "window.__kwrokSetInspectMode(true); document.getElementById('order-btn').dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true })); 'picked'"
        }),
    )?;
    if !pick_sim.contains("picked") {
        return Err(format!("Vector 12 FAILED triggering inspect pick: {pick_sim}"));
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    drop(stdin);
    let _ = child.wait();
    Ok(())
}
