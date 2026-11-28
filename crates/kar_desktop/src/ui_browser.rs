//! Native in-workbench System WebView Browser (`WKWebView` / `WebView2`) with Codex-style
//! visual DOM element picking into Chat, live console/network error capture, responsive device
//! viewports, Retina snapshot compositing under GPUI overlays, and live MCP agent control.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{prelude::*, *};
use kui::primitives::{
    editor::{EditEvent, Editor, InputMode},
    pro_icons::{ProIcon, pro_icon, pro_symbol},
};
use kwrok_webview::{
    BrowserIpcEvent, BrowserWebView, CapturedSnapshot, ConsoleLevel, LogicalRect, WebViewEvent,
    WebViewInit, normalize_omnibox_input,
};

use crate::{
    types::{OfficeKind, SelectionLocator, SelectionTarget},
    ui_motion::{
        Tactile, close_button, icon_button_sm, primary_button_sm, soft_button_sm, spinner,
        text_button_sm,
    },
    ui_primitives::{
        CTRL_H, CTRL_SM_H, FS_CONTROL, FS_META, FS_SECTION, FS_TAG, FS_UI,
        R_CARD, R_CTRL, R_PILL, R_SM, StudioPalette, card_box, mono_font, sub_card_box,
        tnum_features,
    },
    workbench::{KarWorkbench, MainStageView, RightPaneSurface, WorkspaceLayoutMode},
};

const MAX_CONSOLE_ITEMS: usize = 200;
const MAX_RECENT_URLS: usize = 10;

const SVG_CROSSHAIR_PICKER: &str = r#"<circle cx="12" cy="12" r="7.5" stroke="currentColor" stroke-width="1.8"/><circle cx="12" cy="12" r="2.2" fill="currentColor"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3" stroke="currentColor" stroke-linecap="round" stroke-width="1.8"/>"#;
const SVG_CAMERA_SHOT: &str = r#"<path d="M4 8.5A2.5 2.5 0 0 1 6.5 6h1.38a1.5 1.5 0 0 0 1.25-.67l.74-1.11A1.5 1.5 0 0 1 11.12 3.5h1.76a1.5 1.5 0 0 1 1.25.67l.74 1.11a1.5 1.5 0 0 0 1.25.67h1.38A2.5 2.5 0 0 1 20 8.5v8A2.5 2.5 0 0 1 17.5 19h-11A2.5 2.5 0 0 1 4 16.5v-8Z" stroke="currentColor" stroke-width="1.8"/><circle cx="12" cy="12.2" r="3.2" stroke="currentColor" stroke-width="1.8"/>"#;
const SVG_DEVICE_MOBILE: &str = r#"<rect x="6.5" y="2.5" width="11" height="19" rx="2.4" stroke="currentColor" stroke-width="1.8"/><path d="M10.5 18.5h3" stroke="currentColor" stroke-linecap="round" stroke-width="1.8"/>"#;
const SVG_DEVICE_TABLET: &str = r#"<rect x="4.5" y="2.5" width="15" height="19" rx="2.4" stroke="currentColor" stroke-width="1.8"/><circle cx="12" cy="18.5" r="0.9" fill="currentColor"/>"#;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BrowserViewportPreset {
    #[default]
    Responsive,
    Mobile,
    Tablet,
}

impl BrowserViewportPreset {
    pub fn next(self) -> Self {
        match self {
            Self::Responsive => Self::Mobile,
            Self::Mobile => Self::Tablet,
            Self::Tablet => Self::Responsive,
        }
    }

    pub fn constrained_size(self) -> Option<(f32, f32)> {
        match self {
            Self::Responsive => None,
            Self::Mobile => Some((390., 844.)),
            Self::Tablet => Some((820., 1180.)),
        }
    }

    pub fn label_zh(self) -> &'static str {
        match self {
            Self::Responsive => "自适应",
            Self::Mobile => "手机 390px",
            Self::Tablet => "平板 820px",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::Responsive => "Responsive",
            Self::Mobile => "Mobile 390px",
            Self::Tablet => "Tablet 820px",
        }
    }
}

#[derive(Clone, Debug)]
pub struct BrowserConsoleItem {
    pub level: ConsoleLevel,
    pub message: String,
    pub source: String,
    pub timestamp_ms: u64,
}

pub struct BrowserTab {
    pub id: u64,
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub zoom: f64,
    pub viewport: BrowserViewportPreset,
    pub inspect_mode: bool,
    pub console: VecDeque<BrowserConsoleItem>,
    pub error_count: usize,
    pub warn_count: usize,
    pub webview: Option<BrowserWebView>,
    pub pending_html: Option<String>,
    pub last_bounds: LogicalRect,
    pub last_visible: bool,
    pub snapshot: Option<Arc<Image>>,
    pub snapshot_dims: Option<(u32, u32)>,
    pub last_error_banner: Option<String>,
    pub page_load_failed: bool,
}

impl BrowserTab {
    fn new(id: u64) -> Self {
        Self {
            id,
            url: String::new(),
            title: String::new(),
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            zoom: 1.0,
            viewport: BrowserViewportPreset::Responsive,
            inspect_mode: false,
            console: VecDeque::new(),
            error_count: 0,
            warn_count: 0,
            webview: None,
            pending_html: None,
            last_bounds: LogicalRect::default(),
            last_visible: false,
            snapshot: None,
            snapshot_dims: None,
            last_error_banner: None,
            page_load_failed: false,
        }
    }

    pub fn display_title(&self, zh: bool) -> String {
        let t = self.title.trim();
        if !t.is_empty() {
            return t.to_owned();
        }
        if !self.url.is_empty() {
            return compact_url_display(&self.url);
        }
        if zh {
            "新标签页".into()
        } else {
            "New Tab".into()
        }
    }

    fn push_console(&mut self, item: BrowserConsoleItem) {
        match item.level {
            ConsoleLevel::Error => self.error_count = self.error_count.saturating_add(1),
            ConsoleLevel::Warn => self.warn_count = self.warn_count.saturating_add(1),
            _ => {}
        }
        if self.console.len() >= MAX_CONSOLE_ITEMS
            && let Some(removed) = self.console.pop_front()
        {
            match removed.level {
                ConsoleLevel::Error => self.error_count = self.error_count.saturating_sub(1),
                ConsoleLevel::Warn => self.warn_count = self.warn_count.saturating_sub(1),
                _ => {}
            }
        }
        self.console.push_back(item);
    }

    fn clear_console(&mut self) {
        self.console.clear();
        self.error_count = 0;
        self.warn_count = 0;
    }
}

pub(crate) enum SnapshotPurpose {
    CacheOnly,
    AttachToChat,
}

pub struct BrowserHost {
    pub tabs: Vec<BrowserTab>,
    pub active_idx: usize,
    next_id: u64,
    pub address_editor: Entity<Editor>,
    pub eval_editor: Entity<Editor>,
    pub console_open: bool,
    pub console_errors_only: bool,
    pub recent_urls: Vec<String>,
    pub(crate) event_tx: async_channel::Sender<(u64, WebViewEvent)>,
    pub(crate) snapshot_tx:
        async_channel::Sender<(u64, SnapshotPurpose, Result<CapturedSnapshot, String>)>,
    pub(crate) was_occluded: bool,
    pub(crate) webview_suppressed: bool,
    _subscriptions: Vec<Subscription>,
}

impl BrowserHost {
    pub fn new(window: &mut Window, cx: &mut Context<KarWorkbench>) -> Self {
        let (event_tx, event_rx) = async_channel::bounded::<(u64, WebViewEvent)>(256);
        let (snapshot_tx, snapshot_rx) =
            async_channel::bounded::<(u64, SnapshotPurpose, Result<CapturedSnapshot, String>)>(32);

        let address_editor = cx.new(|cx| {
            let mut ed = Editor::new(
                "输入网址、:端口号 或搜索词…",
                InputMode::Single,
                window,
                cx,
            );
            ed.set_placeholder("输入网址、localhost:5173 或搜索词…");
            ed.font_size = FS_CONTROL;
            ed
        });

        let eval_editor = cx.new(|cx| {
            let mut ed = Editor::new("执行 JavaScript 表达式…", InputMode::Single, window, cx);
            ed.set_placeholder("输入 JS 表达式回车执行，例如 document.title");
            ed.font_size = FS_META;
            ed
        });

        let sub_addr = cx.subscribe_in(
            &address_editor,
            window,
            |wb, ed_ent, ev: &EditEvent, win, cx| match ev {
                EditEvent::Submitted => {
                    let raw = ed_ent.read(cx).value().to_owned();
                    wb.navigate_active_browser(&raw, win, cx);
                }
                EditEvent::Escape => {
                    if let Some(tab) = wb.browser.active_tab() {
                        let cur = tab.url.clone();
                        ed_ent.update(cx, |ed, cx| ed.set_value(cur, win, cx));
                    }
                    wb.focus_handle.focus(win, cx);
                }
                _ => {}
            },
        );

        let sub_eval =
            cx.subscribe_in(
                &eval_editor,
                window,
                |wb, ed_ent, ev: &EditEvent, win, cx| {
                    if *ev == EditEvent::Submitted {
                        let expr = ed_ent.read(cx).value().trim().to_owned();
                        if !expr.is_empty() {
                            ed_ent.update(cx, |ed, cx| ed.set_value("", win, cx));
                            wb.eval_in_active_browser(&expr, cx);
                        }
                    }
                },
            );

        cx.spawn_in(window, async move |weak, cx| {
            while let Ok((tab_id, event)) = event_rx.recv().await {
                let Ok(()) = weak.update_in(cx, |wb, window, cx| {
                    wb.handle_browser_webview_event(tab_id, event, window, cx);
                }) else {
                    break;
                };
            }
        })
        .detach();

        cx.spawn_in(window, async move |weak, cx| {
            while let Ok((tab_id, purpose, result)) = snapshot_rx.recv().await {
                let Ok(()) = weak.update_in(cx, |wb, window, cx| {
                    wb.handle_browser_snapshot_result(tab_id, purpose, result, window, cx);
                }) else {
                    break;
                };
            }
        })
        .detach();

        Self {
            tabs: vec![BrowserTab::new(1)],
            active_idx: 0,
            next_id: 2,
            address_editor,
            eval_editor,
            console_open: false,
            console_errors_only: false,
            recent_urls: Vec::new(),
            event_tx,
            snapshot_tx,
            was_occluded: false,
            webview_suppressed: false,
            _subscriptions: vec![sub_addr, sub_eval],
        }
    }

    pub fn active_tab(&self) -> Option<&BrowserTab> {
        self.tabs.get(self.active_idx)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut BrowserTab> {
        self.tabs.get_mut(self.active_idx)
    }

    pub fn tab_by_id_mut(&mut self, id: u64) -> Option<&mut BrowserTab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    fn record_recent(&mut self, url: &str) {
        if url.is_empty() || url.starts_with("about:") || url.starts_with("data:") {
            return;
        }
        self.recent_urls.retain(|u| u != url);
        self.recent_urls.insert(0, url.to_owned());
        if self.recent_urls.len() > MAX_RECENT_URLS {
            self.recent_urls.truncate(MAX_RECENT_URLS);
        }
    }
}

pub fn compact_url_display(url: &str) -> String {
    let stripped = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    stripped.trim_end_matches('/').to_owned()
}

fn epoch_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn format_browser_load_error(code: i64, url: &str, raw_msg: &str, zh: bool) -> String {
    let host = compact_url_display(url);
    let is_local = url.starts_with("http://localhost")
        || url.starts_with("http://127.0.0.1")
        || url.starts_with("http://[::1]");
    match code {
        -1004 if is_local => {
            if zh {
                format!("无法连接到本地服务 {host} — 请先在终端启动对应的开发服务器（如 npm run dev / vite / cargo run）")
            } else {
                format!("Cannot connect to {host} — start your local dev server first")
            }
        }
        -1004 => {
            if zh {
                format!("无法连接到服务器 {host}（连接被拒绝或端口未开放）")
            } else {
                format!("Could not connect to server {host}")
            }
        }
        -1003 => {
            if zh {
                format!("无法解析域名 {host} — 请检查网址拼写或当前网络连接")
            } else {
                format!("DNS lookup failed for {host} — check the address or network connection")
            }
        }
        -1009 => {
            if zh {
                "当前未连接到互联网，请检查系统网络设置后重试".into()
            } else {
                "The Internet connection appears to be offline".into()
            }
        }
        -1001 => {
            if zh {
                format!("打开 {host} 请求超时 — 目标网站响应缓慢或当前网络不可达")
            } else {
                format!("Timed out while loading {host}")
            }
        }
        -1022 => {
            if zh {
                format!("安全传输策略拦截了明文连接 ({host})")
            } else {
                format!("App Transport Security blocked cleartext connection ({host})")
            }
        }
        -1206..=-1200 => {
            if zh {
                format!("HTTPS 安全连接或证书校验失败 ({host})：{raw_msg}")
            } else {
                format!("TLS/SSL handshake failed ({host}): {raw_msg}")
            }
        }
        _ => {
            if zh {
                if raw_msg.trim().is_empty() {
                    format!("页面加载失败 ({host})")
                } else {
                    format!("页面加载失败：{raw_msg}")
                }
            } else if raw_msg.trim().is_empty() {
                format!("Failed to load {host}")
            } else {
                format!("Failed to load page: {raw_msg}")
            }
        }
    }
}

fn discover_workspace_html_files(root: &std::path::Path, max_files: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if found.len() >= max_files || depth > 2 {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with('.')
                || matches!(
                    name,
                    "node_modules" | "target" | "dist" | "build" | "vendor" | ".git"
                )
            {
                continue;
            }
            if path.is_dir() {
                stack.push((path, depth + 1));
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "html" | "htm"))
            {
                if let Ok(rel) = path.strip_prefix(root) {
                    found.push(rel.to_path_buf());
                }
                if found.len() >= max_files {
                    break;
                }
            }
        }
    }
    found.sort();
    found
}

impl KarWorkbench {
    /// Hide every live child WebView immediately (called when overlays open, layout collapses,
    /// or non-browser surfaces are active).
    pub(crate) fn hide_all_browser_webviews(&mut self) {
        for tab in &mut self.browser.tabs {
            if tab.last_visible {
                if let Some(ref wv) = tab.webview {
                    wv.set_visible(false);
                }
                tab.last_visible = false;
            }
        }
    }

    /// Reconcile native WebView visibility at the root of `KarWorkbench::render` so hidden/occluded
    /// tabs never float over GPUI modals, menus, drawers, or other right-pane tabs.
    pub(crate) fn sync_browser_webview_visibility(
        &mut self,
        browser_pane_mounted: bool,
        occluded: bool,
        window: &mut Window,
    ) {
        if occluded && !self.browser.was_occluded {
            // Refresh the active tab's PNG backing frame right as an overlay opens.
            if let Some(tab) = self.browser.active_tab()
                && let Some(ref wv) = tab.webview
            {
                let tab_id = tab.id;
                let tx = self.browser.snapshot_tx.clone();
                wv.capture_snapshot_png(move |res| {
                    let _ = tx.try_send((tab_id, SnapshotPurpose::CacheOnly, res));
                });
            }
        }
        self.browser.was_occluded = occluded;

        let suppressed = !browser_pane_mounted || occluded;
        let was_suppressed = self.browser.webview_suppressed;
        self.browser.webview_suppressed = suppressed;
        if was_suppressed && !suppressed {
            window.request_animation_frame();
        }

        let active_id = if !suppressed {
            self.browser.active_tab().map(|t| t.id)
        } else {
            None
        };
        for tab in &mut self.browser.tabs {
            let should_show =
                Some(tab.id) == active_id && !tab.url.is_empty() && !tab.page_load_failed;
            if !should_show && tab.last_visible {
                if let Some(ref wv) = tab.webview {
                    wv.set_visible(false);
                }
                tab.last_visible = false;
            }
        }
    }

    pub fn open_browser_dock_tab(&mut self, cx: &mut Context<Self>) {
        self.dismiss_compact_sidebar(cx);
        self.close_all_overlays(cx);
        self.main_stage_view = MainStageView::Conversation;
        self.browser_tab_open = true;
        self.right_surface = RightPaneSurface::Browser;
        if self.layout_mode == WorkspaceLayoutMode::ChatFocus {
            self.set_layout_mode(WorkspaceLayoutMode::Split, cx);
        }
        if self.detected_dev_ports.is_empty() {
            self.detected_dev_ports = crate::forge_devices_voice::probe_local_dev_ports(&[]);
        }
        cx.notify();
    }

    pub fn open_url_in_browser(
        &mut self,
        raw_input: &str,
        new_tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.effective_workspace_dir();
        let Some(target_url) = normalize_omnibox_input(raw_input, Some(&workspace)) else {
            self.open_browser_dock_tab(cx);
            return;
        };

        self.open_browser_dock_tab(cx);
        if self.browser.tabs.is_empty() {
            let id = self.browser.next_id;
            self.browser.next_id += 1;
            self.browser.tabs.push(BrowserTab::new(id));
            self.browser.active_idx = 0;
        } else if new_tab {
            let id = self.browser.next_id;
            self.browser.next_id += 1;
            self.browser.tabs.push(BrowserTab::new(id));
            self.browser.active_idx = self.browser.tabs.len() - 1;
        }

        self.navigate_active_browser_resolved(target_url, window, cx);
    }

    pub fn open_html_in_browser(
        &mut self,
        title: &str,
        html: String,
        new_tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_browser_dock_tab(cx);
        if self.browser.tabs.is_empty() || new_tab {
            let id = self.browser.next_id;
            self.browser.next_id += 1;
            self.browser.tabs.push(BrowserTab::new(id));
            self.browser.active_idx = self.browser.tabs.len() - 1;
        }
        let synthetic_url = "about:preview".to_owned();
        self.browser
            .address_editor
            .update(cx, |ed, cx| ed.set_value(synthetic_url.clone(), window, cx));
        if let Some(tab) = self.browser.active_tab_mut() {
            tab.url = synthetic_url;
            tab.title = title.to_owned();
            tab.loading = false;
            tab.inspect_mode = false;
            tab.last_error_banner = None;
            tab.page_load_failed = false;
            if let Some(ref wv) = tab.webview {
                let _ = wv.load_html(&html);
            } else {
                tab.pending_html = Some(html);
            }
        }
        cx.notify();
    }

    pub fn navigate_active_browser(
        &mut self,
        raw_input: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.effective_workspace_dir();
        let Some(url) = normalize_omnibox_input(raw_input, Some(&workspace)) else {
            return;
        };
        self.navigate_active_browser_resolved(url, window, cx);
    }

    fn navigate_active_browser_resolved(
        &mut self,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.browser.record_recent(&url);
        self.browser
            .address_editor
            .update(cx, |ed, cx| ed.set_value(url.clone(), window, cx));

        if self.browser.tabs.is_empty() {
            let id = self.browser.next_id;
            self.browser.next_id += 1;
            self.browser.tabs.push(BrowserTab::new(id));
            self.browser.active_idx = 0;
        }

        let mut watch_tab_id = None;
        if let Some(tab) = self.browser.active_tab_mut() {
            watch_tab_id = Some(tab.id);
            tab.url.clone_from(&url);
            tab.pending_html = None;
            tab.loading = true;
            tab.inspect_mode = false;
            tab.last_error_banner = None;
            tab.page_load_failed = false;
            if let Some(ref wv) = tab.webview {
                if let Err(err) = wv.load_url(&url) {
                    tab.loading = false;
                    tab.page_load_failed = true;
                    tab.last_error_banner = Some(err);
                } else {
                    wv.focus();
                }
            }
        }
        if let Some(tid) = watch_tab_id {
            self.spawn_browser_loading_watchdog(tid, window, cx);
        }
        cx.notify();
    }

    fn spawn_browser_loading_watchdog(
        &self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |weak, cx| {
            for delay_ms in [4_000u64, 10_000, 18_000] {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(delay_ms))
                    .await;
                let mut keep_polling = false;
                let _ = weak.update_in(cx, |wb, _, cx| {
                    if let Some(tab) = wb.browser.tab_by_id_mut(tab_id)
                        && tab.loading
                    {
                        if let Some(ref wv) = tab.webview {
                            if !wv.is_loading() {
                                tab.loading = false;
                                tab.can_go_back = wv.can_go_back();
                                tab.can_go_forward = wv.can_go_forward();
                                cx.notify();
                            } else {
                                keep_polling = true;
                            }
                        }
                    }
                });
                if !keep_polling {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn new_browser_subtab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_all_browser_webviews();
        let id = self.browser.next_id;
        self.browser.next_id += 1;
        self.browser.tabs.push(BrowserTab::new(id));
        self.browser.active_idx = self.browser.tabs.len() - 1;
        self.browser.address_editor.update(cx, |ed, cx| {
            ed.set_value("", window, cx);
            ed.focus(window, cx);
        });
        cx.notify();
    }

    pub fn select_browser_subtab(
        &mut self,
        idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if idx >= self.browser.tabs.len() {
            return;
        }
        self.hide_all_browser_webviews();
        self.browser.active_idx = idx;
        let url = self.browser.tabs[idx].url.clone();
        self.browser
            .address_editor
            .update(cx, |ed, cx| ed.set_value(url, window, cx));
        cx.notify();
    }

    pub fn close_browser_subtab(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pos) = self.browser.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        if let Some(ref wv) = self.browser.tabs[pos].webview {
            wv.set_visible(false);
        }
        self.browser.tabs.remove(pos);
        if self.browser.tabs.is_empty() {
            let id = self.browser.next_id;
            self.browser.next_id += 1;
            self.browser.tabs.push(BrowserTab::new(id));
            self.browser.active_idx = 0;
        } else if self.browser.active_idx >= self.browser.tabs.len() {
            self.browser.active_idx = self.browser.tabs.len() - 1;
        } else if pos < self.browser.active_idx {
            self.browser.active_idx -= 1;
        }
        let url = self
            .browser
            .active_tab()
            .map_or_else(String::new, |t| t.url.clone());
        self.browser
            .address_editor
            .update(cx, |ed, cx| ed.set_value(url, window, cx));
        cx.notify();
    }

    pub fn toggle_browser_inspect_mode(&mut self, cx: &mut Context<Self>) {
        let loc = self.ui_locale;
        if let Some(tab) = self.browser.active_tab_mut() {
            if tab.webview.is_none() || tab.url.is_empty() {
                return;
            }
            tab.inspect_mode = !tab.inspect_mode;
            let active = tab.inspect_mode;
            if let Some(ref wv) = tab.webview {
                wv.set_inspect_mode(active);
                if active {
                    wv.focus();
                }
            }
            if active {
                self.notify_toast(
                    loc.pick(
                        "点选模式已开启：在网页中点击任意元素即可引用到对话（Esc 取消）",
                        "Inspect mode: click any element on the page to add it to chat (Esc to cancel)",
                    ),
                    cx,
                );
            }
        }
        cx.notify();
    }

    pub fn capture_browser_to_chat(&mut self, cx: &mut Context<Self>) {
        let loc = self.ui_locale;
        let Some(tab) = self.browser.active_tab() else {
            return;
        };
        let Some(ref wv) = tab.webview else {
            self.notify_warning(
                loc.pick("请先打开一个网页再截图", "Open a web page before capturing"),
                cx,
            );
            return;
        };
        let tab_id = tab.id;
        let tx = self.browser.snapshot_tx.clone();
        wv.capture_snapshot_png(move |res| {
            let _ = tx.try_send((tab_id, SnapshotPurpose::AttachToChat, res));
        });
    }

    pub fn refresh_active_browser_snapshot(&self) {
        if let Some(tab) = self.browser.active_tab()
            && let Some(ref wv) = tab.webview
        {
            let tab_id = tab.id;
            let tx = self.browser.snapshot_tx.clone();
            wv.capture_snapshot_png(move |res| {
                let _ = tx.try_send((tab_id, SnapshotPurpose::CacheOnly, res));
            });
        }
    }

    pub fn highlight_selector_in_browser(&mut self, url: &str, selector: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_browser_dock_tab(cx);
        let matches_existing = self
            .browser
            .tabs
            .iter()
            .position(|t| t.url == url || compact_url_display(&t.url) == compact_url_display(url));
        if let Some(idx) = matches_existing {
            self.select_browser_subtab(idx, window, cx);
        } else if !url.is_empty() {
            self.open_url_in_browser(url, false, window, cx);
        }
        if let Some(tab) = self.browser.active_tab()
            && let Some(ref wv) = tab.webview
        {
            let sel_json = serde_json::to_string(selector).unwrap_or_else(|_| "\"body\"".into());
            let js = format!(
                r#"(function() {{
                    const el = document.querySelector({sel_json});
                    if (!el) return;
                    el.scrollIntoView({{ block: 'center', inline: 'nearest', behavior: 'smooth' }});
                    const prevOutline = el.style.outline;
                    const prevOffset = el.style.outlineOffset;
                    el.style.outline = '2.5px solid #3b82f6';
                    el.style.outlineOffset = '3px';
                    setTimeout(() => {{ el.style.outline = prevOutline; el.style.outlineOffset = prevOffset; }}, 1600);
                }})()"#
            );
            let _ = wv.eval_fire_and_forget(&js);
        }
    }

    pub fn send_browser_errors_to_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let loc = self.ui_locale;
        let Some(tab) = self.browser.active_tab() else {
            return;
        };
        let errors: Vec<_> = tab
            .console
            .iter()
            .filter(|c| c.level.is_error() || c.level.is_warn())
            .rev()
            .take(8)
            .collect();
        if errors.is_empty() {
            self.notify_toast(
                loc.pick("当前页面暂无控制台报错", "No console errors on this page"),
                cx,
            );
            return;
        }
        let mut summary = String::new();
        for item in errors.into_iter().rev() {
            if !summary.is_empty() {
                summary.push('\n');
            }
            if item.source.is_empty() {
                summary.push_str(&format!("[{}] {}", item.level.label(), item.message));
            } else {
                summary.push_str(&format!(
                    "[{} @ {}] {}",
                    item.level.label(),
                    item.source,
                    item.message
                ));
            }
        }
        let tab_id = tab.id;
        let url = tab.url.clone();
        let title = tab.display_title(loc == crate::ui_primitives::UiLocale::Zh);
        let host_label = compact_url_display(&url);
        if let Some(ref wv) = tab.webview {
            wv.focus_parent();
        }
        self.pinned_target = Some(SelectionTarget {
            tab_id,
            file_path: None,
            doc_name: if host_label.is_empty() {
                title.clone()
            } else {
                host_label
            },
            kind: OfficeKind::Word,
            base_revision: None,
            locator: SelectionLocator::WebElement {
                url,
                page_title: title,
                selector: "window.console".into(),
                tag_name: "console.error".into(),
                bounds_summary: String::new(),
                computed_styles: String::new(),
                html_snippet: summary.clone(),
            },
            excerpt: summary.lines().next().unwrap_or_default().to_owned(),
        });
        self.reveal_chat_composer(window, cx);
        self.composer_editor
            .update(cx, |ed, cx| ed.focus(window, cx));
        self.notify_toast(
            loc.pick(
                "已将网页控制台报错引用到对话输入框",
                "Attached console errors to chat composer",
            ),
            cx,
        );
        cx.notify();
    }

    fn eval_in_active_browser(&mut self, expr: &str, cx: &mut Context<Self>) {
        let tx = self.browser.event_tx.clone();
        let Some(tab) = self.browser.active_tab_mut() else {
            return;
        };
        let tab_id = tab.id;
        tab.push_console(BrowserConsoleItem {
            level: ConsoleLevel::Info,
            message: format!("> {expr}"),
            source: "eval".into(),
            timestamp_ms: epoch_ms_now(),
        });
        if let Some(ref wv) = tab.webview {
            let wrapped = format!(
                "(function() {{ try {{ const r = ({expr}); return typeof r === 'string' ? r : JSON.stringify(r); }} catch (e) {{ return 'Error: ' + (e && e.message ? e.message : String(e)); }} }})()"
            );
            let _ = wv.eval(&wrapped, move |reply| {
                let level = if reply.contains("\"Error:") || reply.starts_with("Error:") {
                    ConsoleLevel::Error
                } else {
                    ConsoleLevel::Log
                };
                let cleaned = serde_json::from_str::<String>(&reply).unwrap_or(reply);
                let _ = tx.try_send((
                    tab_id,
                    WebViewEvent::Ipc(BrowserIpcEvent::Console {
                        level,
                        message: format!("← {cleaned}"),
                        source: "eval".into(),
                        timestamp_ms: epoch_ms_now(),
                    }),
                ));
            });
        }
        cx.notify();
    }

    fn handle_browser_webview_event(
        &mut self,
        tab_id: u64,
        event: WebViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_active = self.browser.active_tab().is_some_and(|t| t.id == tab_id);
        match event {
            WebViewEvent::Navigated(url) => {
                if !url.is_empty() && !url.starts_with("about:blank") {
                    self.browser.record_recent(&url);
                    if let Some(tab) = self.browser.tab_by_id_mut(tab_id) {
                        tab.url.clone_from(&url);
                    }
                    if is_active
                        && !self
                            .browser
                            .address_editor
                            .read(cx)
                            .focus
                            .is_focused(window)
                    {
                        self.browser
                            .address_editor
                            .update(cx, |ed, cx| ed.set_value(url, window, cx));
                    }
                }
            }
            WebViewEvent::TitleChanged(title) => {
                if let Some(tab) = self.browser.tab_by_id_mut(tab_id) {
                    tab.title = title;
                }
            }
            WebViewEvent::LoadStarted(url) => {
                if let Some(tab) = self.browser.tab_by_id_mut(tab_id) {
                    tab.loading = true;
                    tab.inspect_mode = false;
                    tab.page_load_failed = false;
                    tab.last_error_banner = None;
                    if !url.is_empty() && !url.starts_with("about:blank") {
                        tab.url.clone_from(&url);
                    }
                }
                if is_active
                    && !url.is_empty()
                    && !url.starts_with("about:blank")
                    && !self
                        .browser
                        .address_editor
                        .read(cx)
                        .focus
                        .is_focused(window)
                {
                    self.browser
                        .address_editor
                        .update(cx, |ed, cx| ed.set_value(url, window, cx));
                }
            }
            WebViewEvent::LoadFinished(url) => {
                let tx = self.browser.snapshot_tx.clone();
                if let Some(tab) = self.browser.tab_by_id_mut(tab_id) {
                    tab.loading = false;
                    tab.page_load_failed = false;
                    if !url.is_empty() && !url.starts_with("about:blank") {
                        tab.url = url;
                    }
                    if let Some(ref wv) = tab.webview {
                        tab.can_go_back = wv.can_go_back();
                        tab.can_go_forward = wv.can_go_forward();
                        wv.capture_snapshot_png(move |res| {
                            let _ = tx.try_send((tab_id, SnapshotPurpose::CacheOnly, res));
                        });
                    }
                }
            }
            WebViewEvent::LoadFailed { url, code, message } => {
                let zh = self.ui_locale == crate::ui_primitives::UiLocale::Zh;
                if let Some(tab) = self.browser.tab_by_id_mut(tab_id) {
                    let effective_url = if url.is_empty() {
                        tab.url.clone()
                    } else {
                        url
                    };
                    let friendly = format_browser_load_error(code, &effective_url, &message, zh);
                    tab.loading = false;
                    tab.inspect_mode = false;
                    tab.page_load_failed = true;
                    tab.last_error_banner = Some(friendly.clone());
                    if let Some(ref wv) = tab.webview {
                        wv.set_visible(false);
                    }
                    tab.last_visible = false;
                    tab.push_console(BrowserConsoleItem {
                        level: ConsoleLevel::Error,
                        message: friendly,
                        source: format!("navigation ({code})"),
                        timestamp_ms: epoch_ms_now(),
                    });
                }
            }
            WebViewEvent::NewWindowRequested(url) => {
                if !url.is_empty() {
                    self.open_url_in_browser(&url, true, window, cx);
                }
            }
            WebViewEvent::Ipc(ipc) => match ipc {
                BrowserIpcEvent::State {
                    url,
                    title,
                    ready_state,
                    can_go_back,
                    can_go_forward,
                } => {
                    if let Some(tab) = self.browser.tab_by_id_mut(tab_id) {
                        if !url.is_empty() && !url.starts_with("about:blank") {
                            tab.url.clone_from(&url);
                        }
                        if !title.is_empty() {
                            tab.title = title;
                        }
                        if matches!(ready_state.as_str(), "interactive" | "complete") {
                            tab.loading = false;
                            tab.page_load_failed = false;
                        }
                        if let Some(ref wv) = tab.webview {
                            tab.can_go_back = wv.can_go_back() || can_go_back;
                            tab.can_go_forward = wv.can_go_forward() || can_go_forward;
                        }
                    }
                    if is_active
                        && !url.is_empty()
                        && !url.starts_with("about:blank")
                        && !self
                            .browser
                            .address_editor
                            .read(cx)
                            .focus
                            .is_focused(window)
                    {
                        self.browser
                            .address_editor
                            .update(cx, |ed, cx| ed.set_value(url, window, cx));
                    }
                }
                BrowserIpcEvent::Console {
                    level,
                    message,
                    source,
                    timestamp_ms,
                } => {
                    if let Some(tab) = self.browser.tab_by_id_mut(tab_id) {
                        tab.push_console(BrowserConsoleItem {
                            level,
                            message,
                            source,
                            timestamp_ms,
                        });
                    }
                }
                BrowserIpcEvent::InspectCancelled => {
                    if let Some(tab) = self.browser.tab_by_id_mut(tab_id) {
                        tab.inspect_mode = false;
                    }
                }
                BrowserIpcEvent::ElementPicked {
                    url,
                    title,
                    selector,
                    tag_name,
                    text_excerpt,
                    html_snippet,
                    bounds_summary,
                    computed_styles,
                } => {
                    if let Some(tab) = self.browser.tab_by_id_mut(tab_id) {
                        tab.inspect_mode = false;
                        if let Some(ref wv) = tab.webview {
                            wv.focus_parent();
                        }
                    }
                    let host_label = compact_url_display(&url);
                    let doc_name = if !host_label.is_empty() {
                        host_label
                    } else if !title.is_empty() {
                        title.clone()
                    } else {
                        "Web Page".into()
                    };
                    let excerpt = if !text_excerpt.is_empty() {
                        text_excerpt
                    } else {
                        selector.clone()
                    };
                    self.pinned_target = Some(SelectionTarget {
                        tab_id,
                        file_path: None,
                        doc_name,
                        kind: OfficeKind::Word,
                        base_revision: None,
                        locator: SelectionLocator::WebElement {
                            url,
                            page_title: title,
                            selector,
                            tag_name: tag_name.clone(),
                            bounds_summary,
                            computed_styles,
                            html_snippet,
                        },
                        excerpt,
                    });
                    self.reveal_chat_composer(window, cx);
                    self.composer_editor
                        .update(cx, |ed, cx| ed.focus(window, cx));
                    let msg = match self.ui_locale {
                        crate::ui_primitives::UiLocale::Zh => {
                            format!("已引用网页元素 <{tag_name}> 到对话")
                        }
                        crate::ui_primitives::UiLocale::En => {
                            format!("Added <{tag_name}> to chat composer")
                        }
                    };
                    self.notify_toast(msg, cx);
                }
            },
        }
        cx.notify();
    }

    fn handle_browser_snapshot_result(
        &mut self,
        tab_id: u64,
        purpose: SnapshotPurpose,
        result: Result<CapturedSnapshot, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let loc = self.ui_locale;
        match result {
            Ok(captured) => {
                let img = Arc::new(Image::from_bytes(
                    ImageFormat::Png,
                    captured.png_bytes.clone(),
                ));
                if let Some(tab) = self.browser.tab_by_id_mut(tab_id) {
                    tab.snapshot = Some(img);
                    tab.snapshot_dims = Some((captured.width, captured.height));
                    if let SnapshotPurpose::AttachToChat = purpose
                        && let Some(ref wv) = tab.webview
                    {
                        wv.focus_parent();
                    }
                }
                if matches!(purpose, SnapshotPurpose::AttachToChat) {
                    let dir = std::env::temp_dir().join("kwrok-browser-captures");
                    let _ = std::fs::create_dir_all(&dir);
                    let path = dir.join(format!("browser-{}.png", epoch_ms_now()));
                    match std::fs::write(&path, &captured.png_bytes) {
                        Ok(()) => {
                            if !self.pending_attachments.contains(&path) {
                                self.pending_attachments.push(path);
                            }
                            self.reveal_chat_composer(window, cx);
                            self.composer_editor
                                .update(cx, |ed, cx| ed.focus(window, cx));
                            self.notify_toast(
                                loc.pick(
                                    "已将网页高清截图添加至对话附件",
                                    "Attached Retina page screenshot to chat",
                                ),
                                cx,
                            );
                        }
                        Err(err) => {
                            self.notify_warning(format!("保存截图失败：{err}"), cx);
                        }
                    }
                }
            }
            Err(err) => {
                if matches!(purpose, SnapshotPurpose::AttachToChat) {
                    self.notify_warning(err, cx);
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn render_browser_dock_body(
        &mut self,
        pane_w: f32,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let zh = loc == crate::ui_primitives::UiLocale::Zh;
        if self.browser.tabs.is_empty() {
            let id = self.browser.next_id;
            self.browser.next_id += 1;
            self.browser.tabs.push(BrowserTab::new(id));
            self.browser.active_idx = 0;
        }
        let active_idx = self
            .browser
            .active_idx
            .min(self.browser.tabs.len().saturating_sub(1));
        self.browser.active_idx = active_idx;

        let tab_count = self.browser.tabs.len();
        let active_tab_id = self.browser.tabs[active_idx].id;
        let active_url = self.browser.tabs[active_idx].url.clone();
        let active_loading = self.browser.tabs[active_idx].loading;
        let can_back = self.browser.tabs[active_idx].can_go_back;
        let can_fwd = self.browser.tabs[active_idx].can_go_forward;
        let inspect_on = self.browser.tabs[active_idx].inspect_mode;
        let viewport = self.browser.tabs[active_idx].viewport;
        let zoom = self.browser.tabs[active_idx].zoom;
        let err_cnt = self.browser.tabs[active_idx].error_count;
        let warn_cnt = self.browser.tabs[active_idx].warn_count;
        let err_banner = self.browser.tabs[active_idx].last_error_banner.clone();
        let active_failed = self.browser.tabs[active_idx].page_load_failed;
        let cached_snapshot = self.browser.tabs[active_idx].snapshot.clone();
        let has_live_page = !active_url.is_empty();
        let compact_toolbar = pane_w < 540.;

        // ── 1. Multi-Tab Strip (shown when 2+ browser tabs exist) ─────────────
        let subtab_bar = (tab_count > 1).then(|| {
            let mut row = div()
                .id("browser-subtabs-scroll")
                .h(px(33.))
                .px(px(8.))
                .border_b_1()
                .border_color(s.line_soft)
                .bg(s.tray.opacity(0.65))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.))
                .overflow_x_scroll();

            for (idx, t) in self.browser.tabs.iter().enumerate() {
                let selected = idx == active_idx;
                let tid = t.id;
                let title = t.display_title(zh);
                let loading = t.loading;
                let pill = div()
                    .h(px(25.))
                    .pl(px(8.))
                    .pr(px(4.))
                    .rounded(px(R_SM))
                    .bg(if selected { s.card } else { gpui::transparent_black() })
                    .border_1()
                    .border_color(if selected {
                        s.line_strong
                    } else {
                        gpui::transparent_black()
                    })
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .child(
                        Tactile::ghost(("browser-subtab", tid as usize), s)
                            .h(px(22.))
                            .px(px(2.))
                            .gap(px(5.))
                            .child(if loading {
                                spinner(11., s.pro.accent_info, window, cx).into_any_element()
                            } else {
                                pro_icon(ProIcon::Globe, 12., if selected { s.ink } else { s.muted })
                            })
                            .child(
                                div()
                                    .max_w(px(136.))
                                    .truncate()
                                    .text_size(s.font_size(FS_META))
                                    .font_weight(if selected {
                                        FontWeight::MEDIUM
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(if selected { s.ink } else { s.muted })
                                    .child(title.clone()),
                            )
                            .on_press(cx, move |wb, win, cx| {
                                wb.select_browser_subtab(idx, win, cx);
                            }),
                    )
                    .child(
                        close_button(
                            SharedString::from(format!("browser-subtab-close-{tid}")),
                            format!("{} {title}", loc.pick("关闭", "Close")),
                            s,
                        )
                        .on_press(cx, move |wb, win, cx| {
                            wb.close_browser_subtab(tid, win, cx);
                        }),
                    );
                row = row.child(pill);
            }

            row.child(
                icon_button_sm(
                    "browser-new-subtab-strip",
                    ProIcon::Plus,
                    loc.pick("新建浏览器标签页", "New browser tab"),
                    s,
                )
                .on_press(cx, |wb, win, cx| wb.new_browser_subtab(win, cx)),
            )
        });

        // ── 2. Unified Browser Navigation & Developer Toolbar ─────────────────
        let addr_focused = self.browser.address_editor.read(cx).focus.is_focused(window);
        let addr_text = self.browser.address_editor.read(cx).value().trim().to_owned();
        let addr_ent = self.browser.address_editor.clone();
        let bench_for_addr = cx.weak_entity();
        let is_localhost = active_url.starts_with("http://localhost")
            || active_url.starts_with("http://127.0.0.1")
            || active_url.starts_with("http://[::1]");
        let is_https = active_url.starts_with("https://");

        let origin_badge = if is_localhost {
            div()
                .flex_none()
                .px(px(5.5))
                .py(px(1.5))
                .rounded(px(4.))
                .bg(s.pro.green_soft)
                .font_family(mono_font(cx))
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(s.pro.green_ink)
                .child("LOCAL")
                .into_any_element()
        } else if is_https {
            pro_icon(ProIcon::Lock, 12., s.muted)
        } else {
            pro_icon(ProIcon::Globe, 12., s.muted)
        };

        let show_go_btn = !addr_text.is_empty() && (addr_focused || !has_live_page || active_failed);
        let omnibox = div()
            .id("browser-omnibox")
            .flex_1()
            .min_w(px(110.))
            .h(px(30.))
            .pl(px(8.))
            .pr(px(5.))
            .rounded(px(R_CTRL))
            .bg(s.card)
            .border_1()
            .border_color(if addr_focused {
                s.pro.accent_info
            } else {
                s.line
            })
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_text()
            .on_mouse_down(MouseButton::Left, move |_, win, cx| {
                let _ = bench_for_addr.update(cx, |wb, _| {
                    if let Some(tab) = wb.browser.active_tab()
                        && let Some(ref wv) = tab.webview
                    {
                        wv.focus_parent();
                    }
                });
                addr_ent.update(cx, |ed, cx| ed.focus(win, cx));
            })
            .child(origin_badge)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(20.))
                    .child(self.browser.address_editor.clone()),
            )
            .when(show_go_btn, |bar| {
                bar.child(
                    Tactile::soft("browser-omnibox-go", s)
                        .h(px(22.))
                        .px(px(7.))
                        .radius(4.)
                        .gap(px(3.))
                        .tip(loc.pick("打开网址或搜索（回车）", "Navigate or search (Enter)"))
                        .child(pro_icon(ProIcon::ArrowRight, 11.5, s.pro.accent_info))
                        .on_press(cx, |wb, win, cx| {
                            let raw = wb.browser.address_editor.read(cx).value().to_owned();
                            wb.navigate_active_browser(&raw, win, cx);
                        }),
                )
            })
            .when(has_live_page && !show_go_btn, |bar| {
                let copy_url = active_url.clone();
                bar.child(
                    Tactile::ghost("browser-copy-url", s)
                        .size(px(20.))
                        .justify_center()
                        .tip(loc.pick("复制当前网址", "Copy URL"))
                        .child(pro_icon(ProIcon::Copy, 11.5, s.muted))
                        .on_press(cx, move |wb, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_url.clone()));
                            wb.notify_toast(loc.pick("已复制网址", "URL copied"), cx);
                        }),
                )
            });

        let inspect_color = if inspect_on {
            s.pro.accent_info
        } else {
            s.text
        };
        let inspect_btn = Tactile::ghost("browser-inspect-btn", s)
            .h(px(CTRL_SM_H))
            .px(px(if compact_toolbar { 6. } else { 8. }))
            .radius(R_CTRL)
            .gap(px(4.))
            .lit(inspect_on)
            .disabled(!has_live_page)
            .tip(loc.pick(
                "点选网页元素并引用到对话输入框（像 Codex 一样精准指改 UI）",
                "Pick any page element to reference in chat",
            ))
            .child(pro_symbol("0 0 24 24", SVG_CROSSHAIR_PICKER, 14., inspect_color))
            .when(!compact_toolbar, |b| {
                b.child(
                    div()
                        .text_size(s.font_size(FS_META))
                        .text_color(inspect_color)
                        .child(loc.pick("点选元素", "Inspect")),
                )
            })
            .on_press(cx, |wb, _, cx| wb.toggle_browser_inspect_mode(cx));

        let shot_btn = Tactile::ghost("browser-capture-shot-btn", s)
            .size(px(CTRL_SM_H))
            .justify_center()
            .radius(R_CTRL)
            .disabled(!has_live_page)
            .tip(loc.pick("截取当前网页画面添加到对话附件", "Attach page screenshot to chat"))
            .child(pro_symbol("0 0 24 24", SVG_CAMERA_SHOT, 14., s.text))
            .on_press(cx, |wb, _, cx| wb.capture_browser_to_chat(cx));

        let viewport_icon = match viewport {
            BrowserViewportPreset::Responsive => {
                pro_icon(ProIcon::Expand, 13., s.muted)
            }
            BrowserViewportPreset::Mobile => {
                pro_symbol("0 0 24 24", SVG_DEVICE_MOBILE, 14., s.pro.accent_info)
            }
            BrowserViewportPreset::Tablet => {
                pro_symbol("0 0 24 24", SVG_DEVICE_TABLET, 14., s.pro.accent_info)
            }
        };
        let viewport_tip = format!(
            "{} ({})",
            loc.pick("切换响应式视口尺寸", "Cycle device viewport"),
            if zh {
                viewport.label_zh()
            } else {
                viewport.label_en()
            }
        );
        let viewport_btn = Tactile::ghost("browser-viewport-btn", s)
            .size(px(CTRL_SM_H))
            .justify_center()
            .radius(R_CTRL)
            .lit(viewport != BrowserViewportPreset::Responsive)
            .tip(viewport_tip)
            .child(viewport_icon)
            .on_press(cx, |wb, _, cx| {
                if let Some(tab) = wb.browser.active_tab_mut() {
                    tab.viewport = tab.viewport.next();
                }
                cx.notify();
            });

        let console_open = self.browser.console_open;
        let console_btn = Tactile::ghost("browser-console-toggle", s)
            .h(px(CTRL_SM_H))
            .px(px(6.))
            .radius(R_CTRL)
            .gap(px(4.))
            .lit(console_open)
            .tip(loc.pick("打开网页控制台与报错捕获", "Toggle page console & errors"))
            .child(pro_icon(
                ProIcon::Terminal,
                13.,
                if err_cnt > 0 {
                    s.pro.red_ink
                } else if warn_cnt > 0 {
                    s.pro.amber_ink
                } else {
                    s.muted
                },
            ))
            .when(err_cnt > 0, |b| {
                b.child(
                    div()
                        .px(px(4.5))
                        .py(px(0.5))
                        .rounded(px(R_PILL))
                        .bg(s.pro.red_soft)
                        .font_features(tnum_features())
                        .text_size(px(10.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(s.pro.red_ink)
                        .child(err_cnt.to_string()),
                )
            })
            .on_press(cx, |wb, _, cx| {
                wb.browser.console_open = !wb.browser.console_open;
                cx.notify();
            });

        let toolbar = div()
            .h(px(40.))
            .px(px(8.))
            .border_b_1()
            .border_color(s.line_soft)
            .bg(s.stage.opacity(0.55))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                icon_button_sm("browser-back", ProIcon::ArrowLeft, loc.pick("后退", "Back"), s)
                    .disabled(!can_back)
                    .on_press(cx, |wb, _, cx| {
                        if let Some(tab) = wb.browser.active_tab()
                            && let Some(ref wv) = tab.webview
                        {
                            let _ = wv.go_back();
                        }
                        cx.notify();
                    }),
            )
            .child(
                icon_button_sm(
                    "browser-forward",
                    ProIcon::ArrowRight,
                    loc.pick("前进", "Forward"),
                    s,
                )
                .disabled(!can_fwd)
                .on_press(cx, |wb, _, cx| {
                    if let Some(tab) = wb.browser.active_tab()
                        && let Some(ref wv) = tab.webview
                    {
                        let _ = wv.go_forward();
                    }
                    cx.notify();
                }),
            )
            .child(if active_loading {
                Tactile::ghost("browser-loading-spin", s)
                    .size(px(CTRL_SM_H))
                    .justify_center()
                    .tip(loc.pick("正在加载页面…", "Loading page…"))
                    .child(spinner(13., s.pro.accent_info, window, cx))
                    .on_press(cx, |wb, _, cx| {
                        if let Some(tab) = wb.browser.active_tab_mut() {
                            tab.loading = false;
                            if let Some(ref wv) = tab.webview {
                                let _ = wv.eval_fire_and_forget("window.stop && window.stop();");
                            }
                        }
                        cx.notify();
                    })
                    .into_any_element()
            } else {
                icon_button_sm(
                    "browser-reload",
                    ProIcon::Refresh,
                    loc.pick("刷新页面", "Reload"),
                    s,
                )
                .disabled(!has_live_page)
                .on_press(cx, |wb, win, cx| {
                    let mut retry_url = None;
                    if let Some(tab) = wb.browser.active_tab_mut() {
                        if tab.page_load_failed || tab.webview.is_none() {
                            retry_url = Some(tab.url.clone());
                        } else if let Some(ref wv) = tab.webview {
                            tab.loading = true;
                            tab.page_load_failed = false;
                            tab.last_error_banner = None;
                            let _ = wv.reload();
                        }
                    }
                    if let Some(url) = retry_url {
                        wb.navigate_active_browser_resolved(url, win, cx);
                    } else if let Some(tid) = wb.browser.active_tab().map(|t| t.id) {
                        wb.spawn_browser_loading_watchdog(tid, win, cx);
                        cx.notify();
                    }
                })
                .into_any_element()
            })
            .when(has_live_page, |bar| {
                bar.child(
                    icon_button_sm(
                        "browser-home",
                        ProIcon::Home,
                        loc.pick("返回起始页", "Browser start page"),
                        s,
                    )
                    .on_press(cx, |wb, win, cx| {
                        wb.hide_all_browser_webviews();
                        if let Some(tab) = wb.browser.active_tab_mut() {
                            tab.url.clear();
                            tab.title.clear();
                            tab.loading = false;
                            tab.inspect_mode = false;
                            tab.page_load_failed = false;
                            tab.last_error_banner = None;
                        }
                        wb.browser
                            .address_editor
                            .update(cx, |ed, cx| ed.set_value("", win, cx));
                        cx.notify();
                    }),
                )
            })
            .child(omnibox)
            .child(inspect_btn)
            .child(shot_btn)
            .child(viewport_btn)
            .child(console_btn)
            .when(!compact_toolbar && has_live_page, |bar| {
                bar.child(
                    icon_button_sm(
                        "browser-devtools",
                        ProIcon::Code,
                        loc.pick("打开系统 WebKit 检查器", "Open WebKit Inspector"),
                        s,
                    )
                    .on_press(cx, |wb, _, _| {
                        if let Some(tab) = wb.browser.active_tab()
                            && let Some(ref wv) = tab.webview
                        {
                            if wv.is_devtools_open() {
                                wv.close_devtools();
                            } else {
                                wv.open_devtools();
                            }
                        }
                    }),
                )
            })
            .when(tab_count <= 1, |bar| {
                bar.child(
                    icon_button_sm(
                        "browser-new-subtab-inline",
                        ProIcon::Plus,
                        loc.pick("新建浏览器标签页", "New browser tab"),
                        s,
                    )
                    .on_press(cx, |wb, win, cx| wb.new_browser_subtab(win, cx)),
                )
            })
            .when(has_live_page, |bar| {
                let ext_url = active_url.clone();
                bar.child(
                    icon_button_sm(
                        "browser-open-external",
                        ProIcon::ExternalArrow,
                        loc.pick("在系统默认浏览器中打开", "Open in external browser"),
                        s,
                    )
                    .on_press(cx, move |_, _, cx| {
                        cx.open_url(&ext_url);
                    }),
                )
            });

        // ── 3. Active Inspect / Zoom / Error Banner Strip ─────────────────────
        let inspect_banner = inspect_on.then(|| {
            div()
                .h(px(28.))
                .px(px(12.))
                .bg(s.pro.accent_soft)
                .border_b_1()
                .border_color(s.pro.accent_info.opacity(0.28))
                .flex_none()
                .flex()
                .items_center()
                .justify_between()
                .text_size(s.font_size(FS_META))
                .text_color(s.pro.accent_info)
                .child(loc.pick(
                    "点选模式：移动鼠标高亮网页元素，点击即可将其选择器与样式引用到对话",
                    "Inspect active: hover elements and click to attach selector & styles to chat",
                ))
                .child(
                    text_button_sm("browser-cancel-inspect", loc.pick("取消", "Cancel"), s)
                        .on_press(cx, |wb, _, cx| wb.toggle_browser_inspect_mode(cx)),
                )
        });

        let error_strip = err_banner.clone().filter(|_| !active_failed).map(|err| {
            div()
                .px(px(12.))
                .py(px(6.))
                .bg(s.pro.red_soft)
                .border_b_1()
                .border_color(s.pro.red_ink.opacity(0.25))
                .flex_none()
                .text_size(s.font_size(FS_META))
                .text_color(s.pro.red_ink)
                .child(err)
        });

        // ── 4. Main Content Area (Start Page, Error Recovery Card, OR Live System WebView) ──
        let stage_area: AnyElement = if !has_live_page {
            self.render_browser_start_page(s, cx)
        } else if active_failed {
            let retry_url = active_url.clone();
            let detail = err_banner.unwrap_or_else(|| {
                loc.pick("无法打开该网址，请检查地址或网络连接。", "Failed to load this URL.")
                    .to_owned()
            });
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .p(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    card_box(s)
                        .w_full()
                        .max_w(px(460.))
                        .p(px(22.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(pro_icon(ProIcon::AlertTriangle, 18., s.pro.amber_ink))
                                .child(
                                    div()
                                        .text_size(s.font_size(FS_SECTION))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(s.ink)
                                        .child(loc.pick("网页暂时无法打开", "Page Could Not Be Loaded")),
                                ),
                        )
                        .child(
                            div()
                                .text_size(s.font_size(FS_UI))
                                .line_height(px(21.))
                                .text_color(s.text)
                                .child(detail),
                        )
                        .child(
                            div()
                                .px(px(8.))
                                .py(px(5.))
                                .rounded(px(R_SM))
                                .bg(s.tray)
                                .font_family(mono_font(cx))
                                .text_size(px(FS_TAG))
                                .text_color(s.muted)
                                .truncate()
                                .child(active_url.clone()),
                        )
                        .child(
                            div()
                                .pt(px(4.))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    primary_button_sm(
                                        "browser-err-retry",
                                        loc.pick("重新加载", "Retry"),
                                        s,
                                    )
                                    .on_press(cx, move |wb, win, cx| {
                                        wb.navigate_active_browser_resolved(retry_url.clone(), win, cx);
                                    }),
                                )
                                .child(
                                    soft_button_sm(
                                        "browser-err-home",
                                        loc.pick("返回起始页", "Start page"),
                                        s,
                                    )
                                    .on_press(cx, |wb, win, cx| {
                                        wb.hide_all_browser_webviews();
                                        if let Some(tab) = wb.browser.active_tab_mut() {
                                            tab.url.clear();
                                            tab.title.clear();
                                            tab.loading = false;
                                            tab.page_load_failed = false;
                                            tab.last_error_banner = None;
                                        }
                                        wb.browser
                                            .address_editor
                                            .update(cx, |ed, cx| ed.set_value("", win, cx));
                                        cx.notify();
                                    }),
                                ),
                        ),
                )
                .into_any_element()
        } else {
            let bench = cx.weak_entity();
            let suppressed = self.browser.webview_suppressed;
            let constrained = viewport.constrained_size();
            let zoom_level = zoom;

            let webview_slot = div()
                .relative()
                .map(|d| {
                    if let Some((cw, ch)) = constrained {
                        d.w(px(cw))
                            .max_w_full()
                            .h(px(ch))
                            .max_h_full()
                            .rounded(px(12.))
                            .border_1()
                            .border_color(s.line_strong)
                            .overflow_hidden()
                            .bg(s.card)
                    } else {
                        d.size_full().bg(s.card)
                    }
                })
                .when_some(cached_snapshot, |d, snap| {
                    d.child(
                        img(snap)
                            .size_full()
                            .object_fit(ObjectFit::Cover),
                    )
                })
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, win, cx| {
                            let x = f32::from(bounds.origin.x);
                            let y = f32::from(bounds.origin.y);
                            let w = f32::from(bounds.size.width);
                            let h = f32::from(bounds.size.height);
                            if w < 4.0 || h < 4.0 {
                                return;
                            }
                            let rect = LogicalRect {
                                x,
                                y,
                                width: w,
                                height: h,
                            };
                            let _ = bench.update(cx, |wb, cx| {
                                let tx = wb.browser.event_tx.clone();
                                let hide_now = wb.browser.webview_suppressed || suppressed;
                                let Some(tab) = wb.browser.tab_by_id_mut(active_tab_id) else {
                                    return;
                                };
                                if hide_now || tab.url.is_empty() || tab.page_load_failed {
                                    if tab.last_visible {
                                        if let Some(ref wv) = tab.webview {
                                            wv.set_visible(false);
                                        }
                                        tab.last_visible = false;
                                    }
                                    return;
                                }
                                if tab.webview.is_none() {
                                    let tid = tab.id;
                                    let init = WebViewInit {
                                        initial_url: tab
                                            .pending_html
                                            .is_none()
                                            .then(|| tab.url.clone()),
                                        initial_html: tab.pending_html.take(),
                                        bounds: rect,
                                        visible: true,
                                        on_event: Arc::new(move |ev| {
                                            let _ = tx.try_send((tid, ev));
                                        }),
                                    };
                                    match BrowserWebView::build(win, init) {
                                        Ok(wv) => {
                                            if (zoom_level - 1.0).abs() > 0.01 {
                                                wv.set_zoom(zoom_level);
                                            }
                                            tab.last_bounds = rect;
                                            tab.last_visible = true;
                                            tab.webview = Some(wv);
                                        }
                                        Err(err) => {
                                            tab.loading = false;
                                            tab.page_load_failed = true;
                                            tab.last_error_banner = Some(err);
                                            cx.notify();
                                        }
                                    }
                                } else if let Some(ref wv) = tab.webview {
                                    let moved = (tab.last_bounds.x - rect.x).abs() > 0.5
                                        || (tab.last_bounds.y - rect.y).abs() > 0.5
                                        || (tab.last_bounds.width - rect.width).abs() > 0.5
                                        || (tab.last_bounds.height - rect.height).abs() > 0.5;
                                    if moved {
                                        wv.set_bounds(rect);
                                        tab.last_bounds = rect;
                                    }
                                    if !tab.last_visible {
                                        wv.set_visible(true);
                                        tab.last_visible = true;
                                    }
                                }
                            });
                        },
                    )
                    .size_full(),
                );

            if constrained.is_some() {
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .p(px(14.))
                    .bg(s.tray)
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .child(webview_slot)
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .child(webview_slot)
                    .into_any_element()
            }
        };

        // ── 5. Collapsible Console & Runtime Error Drawer ─────────────────────
        let console_drawer = self
            .browser
            .console_open
            .then(|| self.render_browser_console_drawer(s, cx));

        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .when_some(subtab_bar, |d, bar| d.child(bar))
            .child(toolbar)
            .when_some(inspect_banner, |d, b| d.child(b))
            .when_some(error_strip, |d, b| d.child(b))
            .child(stage_area)
            .when_some(console_drawer, |d, drawer| d.child(drawer))
            .into_any_element()
    }

    fn render_browser_start_page(
        &self,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let workspace = self.effective_workspace_dir();
        let html_files = discover_workspace_html_files(&workspace, 6);

        let mut ports_section = card_box(s)
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .child(div().size(px(7.)).rounded_full().bg(
                                if self.detected_dev_ports.is_empty() {
                                    s.faint
                                } else {
                                    s.pro.green
                                },
                            ))
                            .child(
                                div()
                                    .text_size(s.font_size(FS_UI))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(s.ink)
                                    .child(loc.pick("本地开发预览服务", "Local Dev Servers")),
                            ),
                    )
                    .child(
                        soft_button_sm(
                            "browser-scan-ports",
                            loc.pick("重新扫描端口", "Rescan ports"),
                            s,
                        )
                        .on_press(cx, |wb, _, cx| {
                            wb.detected_dev_ports =
                                crate::forge_devices_voice::probe_local_dev_ports(&[]);
                            cx.notify();
                        }),
                    ),
            );

        if self.detected_dev_ports.is_empty() {
            ports_section = ports_section.child(
                div()
                    .text_size(s.font_size(FS_META))
                    .text_color(s.muted)
                    .child(loc.pick(
                        "在终端启动 npm run dev / vite / cargo run 后，监听中的 localhost 端口会自动显示在这里，点击即可并排预览并点选元素让智能体修改。",
                        "Start a local dev server (Vite, Next.js, Cargo, Python) to preview it side-by-side and click any DOM element to edit in chat.",
                    )),
            );
            let mut presets = div().flex().flex_wrap().gap(px(6.));
            for port in [3000u16, 5173, 8080, 4173] {
                let url = format!("http://localhost:{port}");
                presets = presets.child(
                    soft_button_sm(
                        ("browser-preset-port", port as usize),
                        format!(":{port}"),
                        s,
                    )
                    .on_press(cx, move |wb, win, cx| {
                        wb.open_url_in_browser(&url, false, win, cx);
                    }),
                );
            }
            ports_section = ports_section.child(presets);
        } else {
            let mut grid = div().flex().flex_wrap().gap(px(8.));
            for &port in &self.detected_dev_ports {
                let url = format!("http://localhost:{port}");
                grid = grid.child(
                    Tactile::soft(("browser-live-port", port as usize), s)
                        .h(px(36.))
                        .px(px(12.))
                        .radius(R_CTRL)
                        .gap(px(7.))
                        .fills(s.pro.green_soft, s.pro.green_soft)
                        .font_family(mono_font(cx))
                        .font_features(tnum_features())
                        .text_size(s.font_size(FS_UI))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(s.pro.green_ink)
                        .child(div().size(px(6.)).rounded_full().bg(s.pro.green))
                        .child(format!("localhost:{port}"))
                        .child(pro_icon(ProIcon::ArrowRight, 12., s.pro.green_ink))
                        .on_press(cx, move |wb, win, cx| {
                            wb.open_url_in_browser(&url, false, win, cx);
                        }),
                );
            }
            ports_section = ports_section.child(grid);
        }

        let html_section = (!html_files.is_empty()).then(|| {
            let list = card_box(s)
                .p(px(16.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(s.font_size(FS_UI))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(s.ink)
                        .child(loc.pick("工作区网页文件", "Workspace HTML Files")),
                );
            let mut chips = div().flex().flex_wrap().gap(px(6.));
            for (ix, rel) in html_files.into_iter().enumerate() {
                let rel_str = rel.display().to_string();
                let open_arg = rel_str.clone();
                chips = chips.child(
                    soft_button_sm(("browser-ws-html", ix), rel_str, s).on_press(
                        cx,
                        move |wb, win, cx| {
                            wb.open_url_in_browser(&open_arg, false, win, cx);
                        },
                    ),
                );
            }
            list.child(chips)
        });

        let recent_section = (!self.browser.recent_urls.is_empty()).then(|| {
            let mut card = card_box(s)
                .p(px(16.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(s.font_size(FS_UI))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(s.ink)
                        .child(loc.pick("最近访问", "Recent Pages")),
                );
            for (ix, url) in self.browser.recent_urls.iter().cloned().enumerate() {
                let go = url.clone();
                card = card.child(
                    Tactile::ghost(("browser-recent-url", ix), s)
                        .w_full()
                        .h(px(30.))
                        .px(px(8.))
                        .radius(R_SM)
                        .gap(px(8.))
                        .child(pro_icon(ProIcon::Globe, 13., s.muted))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(mono_font(cx))
                                .text_size(s.font_size(FS_META))
                                .text_color(s.text)
                                .child(compact_url_display(&url)),
                        )
                        .on_press(cx, move |wb, win, cx| {
                            wb.open_url_in_browser(&go, false, win, cx);
                        }),
                );
            }
            card
        });

        let tips_card = sub_card_box(s)
            .p(px(14.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(s.font_size(FS_META))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(s.ink)
                    .child(pro_symbol(
                        "0 0 24 24",
                        SVG_CROSSHAIR_PICKER,
                        13.5,
                        s.pro.accent_info,
                    ))
                    .child(loc.pick(
                        "与对话深度协同的内置系统浏览器",
                        "Agent-Integrated System WebView",
                    )),
            )
            .child(
                div()
                    .text_size(s.font_size(FS_META))
                    .line_height(px(19.))
                    .text_color(s.muted)
                    .child(loc.pick(
                        "• 点击工具栏「点选元素」可在真实网页上悬停高亮并点选任意组件，自动附带 CSS 选择器与计算样式到对话输入框。\n• 自动捕获页面 Console 报错与失败请求，支持一键把报错堆栈发给智能体修复。\n• 智能体也可通过内置浏览器工具自动打开、读取 DOM、点击表单并截图验收。",
                        "• Click Inspect in the toolbar to hover & pick any live DOM element straight into your chat prompt.\n• Captures console errors, uncaught exceptions, and failed fetch calls with one-click Fix in Chat.\n• Agents can also navigate, inspect DOM, interact, and capture screenshots directly.",
                    )),
            );

        let quick_sites_section = {
            let sites = [
                (loc.pick("必应搜索", "Bing"), "https://cn.bing.com"),
                (loc.pick("百度", "Baidu"), "https://www.baidu.com"),
                ("GitHub", "https://github.com"),
                ("MDN Docs", "https://developer.mozilla.org/zh-CN/"),
            ];
            let mut chips = div().flex().flex_wrap().gap(px(8.));
            for (ix, (label, site_url)) in sites.into_iter().enumerate() {
                chips = chips.child(
                    Tactile::soft(("browser-quick-site", ix), s)
                        .h(px(32.))
                        .px(px(11.))
                        .radius(R_CTRL)
                        .gap(px(6.))
                        .child(pro_icon(ProIcon::Globe, 12.5, s.pro.accent_info))
                        .child(
                            div()
                                .text_size(s.font_size(FS_META))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(s.ink)
                                .child(label),
                        )
                        .on_press(cx, move |wb, win, cx| {
                            wb.open_url_in_browser(site_url, false, win, cx);
                        }),
                );
            }
            card_box(s)
                .p(px(16.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .text_size(s.font_size(FS_UI))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(s.ink)
                        .child(loc.pick("常用网站快速访问", "Quick Access")),
                )
                .child(chips)
        };

        div()
            .id("browser-start-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .p(px(20.))
            .child(
                div()
                    .w_full()
                    .max_w(px(640.))
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(
                        div()
                            .pb(px(2.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(pro_icon(ProIcon::Globe, 18., s.pro.accent_info))
                            .child(
                                div()
                                    .text_size(s.font_size(FS_SECTION))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(s.ink)
                                    .child(loc.pick("内置浏览器与实时预览", "Browser & Live Preview")),
                            ),
                    )
                    .child(quick_sites_section)
                    .child(ports_section)
                    .when_some(html_section, |d, c| d.child(c))
                    .when_some(recent_section, |d, c| d.child(c))
                    .child(tips_card),
            )
            .into_any_element()
    }

    fn render_browser_console_drawer(
        &self,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let errors_only = self.browser.console_errors_only;
        let items: Vec<_> = self
            .browser
            .active_tab()
            .map(|t| {
                t.console
                    .iter()
                    .filter(|c| !errors_only || c.level.is_error() || c.level.is_warn())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let has_errors = self
            .browser
            .active_tab()
            .is_some_and(|t| t.error_count > 0 || t.warn_count > 0);

        let header = div()
            .h(px(30.))
            .px(px(10.))
            .border_b_1()
            .border_color(s.line_soft)
            .bg(s.tray)
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(s.font_size(FS_META))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(s.ink)
                            .child(loc.pick("网页控制台", "Console")),
                    )
                    .child(
                        text_button_sm(
                            "browser-console-filter",
                            if errors_only {
                                loc.pick("仅报错/警告", "Errors & warns")
                            } else {
                                loc.pick("全部日志", "All logs")
                            },
                            s,
                        )
                        .on_press(cx, |wb, _, cx| {
                            wb.browser.console_errors_only = !wb.browser.console_errors_only;
                            cx.notify();
                        }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .when(has_errors, |row| {
                        row.child(
                            primary_button_sm(
                                "browser-send-errors-chat",
                                loc.pick("发送报错到对话", "Fix in chat"),
                                s,
                            )
                            .on_press(cx, |wb, win, cx| wb.send_browser_errors_to_chat(win, cx)),
                        )
                    })
                    .child(
                        text_button_sm("browser-console-clear", loc.pick("清空", "Clear"), s)
                            .on_press(cx, |wb, _, cx| {
                                if let Some(tab) = wb.browser.active_tab_mut() {
                                    tab.clear_console();
                                }
                                cx.notify();
                            }),
                    )
                    .child(
                        icon_button_sm(
                            "browser-console-close",
                            ProIcon::Close,
                            loc.pick("收起控制台", "Close console"),
                            s,
                        )
                        .on_press(cx, |wb, _, cx| {
                            wb.browser.console_open = false;
                            cx.notify();
                        }),
                    ),
            );

        let mut log_list = div()
            .id("browser-console-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(10.))
            .py(px(6.))
            .font_family(mono_font(cx))
            .text_size(px(FS_TAG))
            .flex()
            .flex_col()
            .gap(px(3.));

        if items.is_empty() {
            log_list = log_list.child(
                div()
                    .py(px(8.))
                    .text_color(s.muted)
                    .child(loc.pick("暂无控制台输出", "No console messages yet")),
            );
        } else {
            for (ix, item) in items.into_iter().enumerate() {
                let (fg, bg) = match item.level {
                    ConsoleLevel::Error => (s.pro.red_ink, s.pro.red_soft.opacity(0.45)),
                    ConsoleLevel::Warn => (s.pro.amber_ink, s.pro.amber_soft.opacity(0.45)),
                    ConsoleLevel::Info => (s.pro.accent_info, gpui::transparent_black()),
                    ConsoleLevel::Log => (s.text, gpui::transparent_black()),
                };
                log_list = log_list.child(
                    div()
                        .id(("browser-con-row", ix))
                        .w_full()
                        .px(px(6.))
                        .py(px(2.5))
                        .rounded(px(4.))
                        .bg(bg)
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_color(fg)
                                .child(item.message),
                        )
                        .when(!item.source.is_empty(), |r| {
                            r.child(
                                div()
                                    .flex_none()
                                    .max_w(px(140.))
                                    .truncate()
                                    .text_color(s.faint)
                                    .child(item.source),
                            )
                        }),
                );
            }
        }

        let eval_ent = self.browser.eval_editor.clone();
        let eval_bar = div()
            .h(px(28.))
            .px(px(10.))
            .border_t_1()
            .border_color(s.line_soft)
            .bg(s.card)
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_text()
            .on_mouse_down(MouseButton::Left, move |_, win, cx| {
                eval_ent.update(cx, |ed, cx| ed.focus(win, cx));
            })
            .child(
                div()
                    .font_family(mono_font(cx))
                    .text_size(px(11.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(s.pro.accent_info)
                    .child(">"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(19.))
                    .child(self.browser.eval_editor.clone()),
            );

        div()
            .h(px(192.))
            .w_full()
            .flex_none()
            .border_t_1()
            .border_color(s.line)
            .bg(s.surface)
            .flex()
            .flex_col()
            .child(header)
            .child(log_list)
            .child(eval_bar)
    }
}

#[allow(dead_code)]
const _: f32 = CTRL_H + R_CARD;

#[cfg(test)]
mod tests {
    use super::{BrowserConsoleItem, BrowserTab, BrowserViewportPreset, MAX_CONSOLE_ITEMS, compact_url_display};
    use kwrok_webview::ConsoleLevel;

    #[test]
    fn viewport_cycles_and_url_display_compacts() {
        let v = BrowserViewportPreset::Responsive;
        assert_eq!(v.constrained_size(), None);
        let m = v.next();
        assert_eq!(m, BrowserViewportPreset::Mobile);
        assert_eq!(m.constrained_size(), Some((390., 844.)));
        let t = m.next();
        assert_eq!(t, BrowserViewportPreset::Tablet);
        assert_eq!(t.next(), BrowserViewportPreset::Responsive);
        assert_eq!(compact_url_display("http://localhost:5173/"), "localhost:5173");
        assert_eq!(compact_url_display("https://docs.rs/gpui/"), "docs.rs/gpui");
    }

    #[test]
    fn console_ring_buffer_tracks_error_and_warning_counts() {
        let mut tab = BrowserTab::new(1);
        for i in 0..(MAX_CONSOLE_ITEMS + 5) {
            tab.push_console(BrowserConsoleItem {
                level: if i == 0 { ConsoleLevel::Error } else { ConsoleLevel::Log },
                message: format!("msg {i}"),
                source: "test".into(),
                timestamp_ms: i as u64,
            });
        }
        // The initial error at index 0 rolled out once MAX_CONSOLE_ITEMS was exceeded.
        assert_eq!(tab.console.len(), MAX_CONSOLE_ITEMS);
        assert_eq!(tab.error_count, 0);

        tab.push_console(BrowserConsoleItem {
            level: ConsoleLevel::Error,
            message: "boom".into(),
            source: "app.js:1".into(),
            timestamp_ms: 999,
        });
        tab.push_console(BrowserConsoleItem {
            level: ConsoleLevel::Warn,
            message: "slow".into(),
            source: "app.js:2".into(),
            timestamp_ms: 1000,
        });
        assert_eq!(tab.error_count, 1);
        assert_eq!(tab.warn_count, 1);
        tab.clear_console();
        assert_eq!(tab.error_count, 0);
        assert_eq!(tab.warn_count, 0);
        assert!(tab.console.is_empty());
    }
}
