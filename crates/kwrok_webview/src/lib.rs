//! Native system WebView integration (`WKWebView` on macOS, `WebView2` on Windows)
//! with live DOM element picking, console/network error telemetry, agent DOM snapshot/action
//! execution, and Retina PNG snapshot capture for zero Z-order bleed in GPUI.

use std::path::Path;
use std::sync::Arc;

use raw_window_handle::HasWindowHandle;
use serde::{Deserialize, Serialize};

/// Logical rectangle in window coordinates (top-left origin, logical points/pixels).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LogicalRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Captured PNG frame from the live system WebView.
#[derive(Clone, Debug)]
pub struct CapturedSnapshot {
    pub png_bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Console / uncaught error / network failure severity reported by the injected bridge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConsoleLevel {
    Log,
    Info,
    Warn,
    Error,
}

impl ConsoleLevel {
    pub fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }

    pub fn is_warn(self) -> bool {
        matches!(self, Self::Warn)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Log => "log",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// Structured message posted from the injected page script over `window.ipc.postMessage`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserIpcEvent {
    State {
        url: String,
        title: String,
        #[serde(default)]
        ready_state: String,
        #[serde(default)]
        can_go_back: bool,
        #[serde(default)]
        can_go_forward: bool,
    },
    Console {
        level: ConsoleLevel,
        message: String,
        #[serde(default)]
        source: String,
        #[serde(default)]
        timestamp_ms: u64,
    },
    ElementPicked {
        url: String,
        title: String,
        selector: String,
        tag_name: String,
        #[serde(default)]
        text_excerpt: String,
        #[serde(default)]
        html_snippet: String,
        #[serde(default)]
        bounds_summary: String,
        #[serde(default)]
        computed_styles: String,
    },
    InspectCancelled,
}

/// Lifecycle & bridge events emitted by a `BrowserWebView`.
#[derive(Clone, Debug)]
pub enum WebViewEvent {
    Navigated(String),
    TitleChanged(String),
    LoadStarted(String),
    LoadFinished(String),
    LoadFailed {
        url: String,
        code: i64,
        message: String,
    },
    NewWindowRequested(String),
    Ipc(BrowserIpcEvent),
}

pub type WebViewEventCallback = Arc<dyn Fn(WebViewEvent) + Send + Sync + 'static>;

/// Initialization parameters for mounting a child `BrowserWebView` inside a GPUI window.
pub struct WebViewInit {
    pub initial_url: Option<String>,
    pub initial_html: Option<String>,
    pub bounds: LogicalRect,
    pub visible: bool,
    pub on_event: WebViewEventCallback,
}

fn canonicalize_http_or_special_url(raw: &str) -> Option<String> {
    url::Url::parse(raw).ok().map(String::from)
}

/// Normalize Chinese IME fullwidth URL punctuation (`www。baidu。com`, `localhost：5173`, `：3000`, `https：//...`)
/// when typed without whitespace in URL/host/port position.
fn normalize_ime_url_punctuation(trimmed: &str) -> String {
    if let Some(rest) = trimmed.strip_prefix('：')
        && rest.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        return format!(":{rest}");
    }
    let mut s = trimmed
        .replace("https：//", "https://")
        .replace("HTTPS：//", "https://")
        .replace("http：//", "http://")
        .replace("HTTP：//", "http://")
        .replace("file：//", "file://");
    if !s.contains(char::is_whitespace)
        && (s.contains('。') || s.contains('：') || s.contains('／'))
    {
        let ascii_url_like = s.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(
                    c,
                    '.' | '-' | '_' | ':' | '/' | '?' | '&' | '=' | '%' | '#' | '+' | '~' | '。' | '：' | '／'
                )
        });
        if ascii_url_like {
            s = s.replace('。', ".").replace('：', ":").replace('／', "/");
        }
    }
    s
}

/// Normalize an omnibox entry (`:5173`, `localhost:3000`, `example.com`, workspace `.html`, or search query)
/// into a canonical, RFC 3986 percent-encoded navigable URL.
pub fn normalize_omnibox_input(raw: &str, workspace: Option<&Path>) -> Option<String> {
    let raw_trimmed = raw.trim();
    if raw_trimmed.is_empty() {
        return None;
    }
    let normalized = normalize_ime_url_punctuation(raw_trimmed);
    let trimmed = normalized.as_str();

    // `:3000` or `:5173/dashboard` shorthand -> `http://localhost:...`
    if let Some(rest) = trimmed.strip_prefix(':')
        && rest.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        return canonicalize_http_or_special_url(&format!("http://localhost:{rest}"));
    }

    // Already has a known scheme
    let lower = trimmed.to_ascii_lowercase();
    for scheme in ["http://", "https://", "file://", "data:", "about:"] {
        if lower.starts_with(scheme) {
            if lower.starts_with("http://0.0.0.0:") {
                let suffix = &trimmed["http://0.0.0.0:".len()..];
                return canonicalize_http_or_special_url(&format!("http://localhost:{suffix}"));
            }
            if matches!(scheme, "data:" | "about:") {
                return Some(trimmed.to_owned());
            }
            return canonicalize_http_or_special_url(trimmed).or_else(|| Some(trimmed.to_owned()));
        }
    }

    // Localhost / loopback without scheme (`localhost:5173`, `127.0.0.1:8080`, `[::1]:3000`)
    if lower == "localhost"
        || lower.starts_with("localhost:")
        || lower.starts_with("localhost/")
        || lower.starts_with("127.0.0.1")
        || lower.starts_with("[::1]")
    {
        return canonicalize_http_or_special_url(&format!("http://{trimmed}"));
    }
    if let Some(suffix) = trimmed.strip_prefix("0.0.0.0:") {
        return canonicalize_http_or_special_url(&format!("http://localhost:{suffix}"));
    }

    // Existing local or workspace HTML/SVG/PDF file path
    let candidate = Path::new(trimmed);
    if candidate.is_absolute()
        && candidate.is_file()
        && let Ok(u) = url::Url::from_file_path(candidate)
    {
        return Some(u.into());
    }
    if let Some(ws) = workspace {
        let joined = ws.join(trimmed);
        if joined.is_file()
            && let Ok(canon) = joined.canonicalize()
            && let Ok(u) = url::Url::from_file_path(canon)
        {
            return Some(u.into());
        }
    }

    // Bare domain or IPv4 (`vercel.com`, `docs.rs/gpui`, `192.168.1.10:8080`, `baike.baidu.com/item/人工智能`)
    if !trimmed.contains(char::is_whitespace) {
        let host_port = trimmed.split('/').next().unwrap_or(trimmed);
        let host = host_port.split(':').next().unwrap_or(host_port);
        let is_ipv4 = host.split('.').count() == 4
            && host.split('.').all(|part| part.parse::<u8>().is_ok());
        if is_ipv4 {
            return canonicalize_http_or_special_url(&format!("http://{trimmed}"));
        }
        if host.contains('.') && !host.starts_with('.') && !host.ends_with('.') {
            let scheme = if host.ends_with(".local") || host_port.contains(':') {
                "http"
            } else {
                "https"
            };
            if let Some(valid) = canonicalize_http_or_special_url(&format!("{scheme}://{trimmed}"))
            {
                return Some(valid);
            }
        }
    }

    // Fallback: Bing web search (accessible with low latency in Mainland China & globally)
    let mut search = url::Url::parse("https://cn.bing.com/search").ok()?;
    search.query_pairs_mut().append_pair("q", raw_trimmed);
    Some(search.into())
}

/// Early-document script injected into every Kwrok WebView tab:
/// 1. Console (`log`/`info`/`warn`/`error`), uncaught errors, unhandled rejections & failed `fetch` telemetry.
/// 2. SPA route & title observer (`pushState`/`replaceState`/`popstate`/`MutationObserver`).
/// 3. Codex-style interactive DOM Element Picker (`window.__kwrokSetInspectMode(bool)`).
/// 4. Agent DOM accessibility/interactive snapshot (`window.__kwrokSnapshotDom`) & action executor (`window.__kwrokAct`).
pub const BRIDGE_INIT_JS: &str = r#"
(function() {
  if (window.__kwrokBridgeInstalled) return;
  window.__kwrokBridgeInstalled = true;

  const post = (payload) => {
    try {
      if (window.ipc && typeof window.ipc.postMessage === 'function') {
        window.ipc.postMessage(JSON.stringify(payload));
      }
    } catch (_) {}
  };

  const clampStr = (s, maxLen) => {
    const str = String(s == null ? '' : s);
    return str.length > maxLen ? str.slice(0, maxLen) + '…' : str;
  };

  const formatArg = (arg) => {
    if (arg instanceof Error) {
      return arg.stack || (arg.name + ': ' + arg.message);
    }
    if (typeof arg === 'object' && arg !== null) {
      try { return JSON.stringify(arg); } catch (_) { return Object.prototype.toString.call(arg); }
    }
    return String(arg);
  };

  // 1. Console + Runtime Error + Failed Fetch Telemetry (rate-limited)
  let windowStart = Date.now();
  let sentInWindow = 0;
  const emitConsole = (level, message, source) => {
    const now = Date.now();
    if (now - windowStart > 1000) {
      windowStart = now;
      sentInWindow = 0;
    }
    if (sentInWindow >= 40) return;
    sentInWindow += 1;
    post({
      kind: 'console',
      level,
      message: clampStr(message, 1200),
      source: clampStr(source || '', 240),
      timestamp_ms: now
    });
  };

  ['log', 'info', 'warn', 'error'].forEach((lvl) => {
    const orig = console[lvl];
    if (typeof orig === 'function') {
      console[lvl] = function(...args) {
        try {
          emitConsole(lvl, args.map(formatArg).join(' '), 'console.' + lvl);
        } catch (_) {}
        return orig.apply(this, args);
      };
    }
  });

  window.addEventListener('error', (ev) => {
    const loc = ev.filename ? (ev.filename + ':' + (ev.lineno || 0) + ':' + (ev.colno || 0)) : 'window.onerror';
    const msg = (ev.error && ev.error.stack) ? ev.error.stack : (ev.message || 'Uncaught error');
    emitConsole('error', msg, loc);
  });

  window.addEventListener('unhandledrejection', (ev) => {
    const reason = formatArg(ev.reason);
    emitConsole('error', 'Unhandled Promise Rejection: ' + reason, 'Promise');
  });

  if (typeof window.fetch === 'function') {
    const origFetch = window.fetch;
    window.fetch = function(...args) {
      const reqUrl = typeof args[0] === 'string' ? args[0] : (args[0] && args[0].url) || '';
      return origFetch.apply(this, args).then((res) => {
        if (res && res.status >= 400) {
          emitConsole(res.status >= 500 ? 'error' : 'warn', `HTTP ${res.status} ${res.statusText || ''} — ${reqUrl}`, 'fetch');
        }
        return res;
      }).catch((err) => {
        emitConsole('error', `Fetch failed (${reqUrl}): ${formatArg(err)}`, 'fetch');
        throw err;
      });
    };
  }

  // 2. Page State & SPA Navigation Observer
  const reportState = () => {
    post({
      kind: 'state',
      url: String(location.href || ''),
      title: String(document.title || ''),
      ready_state: String(document.readyState || ''),
      can_go_back: history.length > 1,
      can_go_forward: false
    });
  };

  ['pushState', 'replaceState'].forEach((fn) => {
    const orig = history[fn];
    if (typeof orig === 'function') {
      history[fn] = function(...args) {
        const out = orig.apply(this, args);
        setTimeout(reportState, 0);
        return out;
      };
    }
  });
  window.addEventListener('popstate', reportState);
  window.addEventListener('hashchange', reportState);
  window.addEventListener('DOMContentLoaded', () => {
    reportState();
    const titleEl = document.querySelector('title') || document.head;
    if (titleEl && typeof MutationObserver === 'function') {
      new MutationObserver(reportState).observe(titleEl, { subtree: true, childList: true, characterData: true });
    }
  });
  window.addEventListener('load', reportState);
  if (document.readyState === 'interactive' || document.readyState === 'complete') {
    setTimeout(reportState, 0);
  }

  // Helper: Shortest reliable CSS selector for an element
  const cssSelectorFor = (el) => {
    if (!el || el.nodeType !== 1) return 'body';
    if (el.id && /^[a-zA-Z][\w-]*$/.test(el.id)) {
      return '#' + CSS.escape(el.id);
    }
    const testId = el.getAttribute('data-testid') || el.getAttribute('data-test');
    if (testId) {
      return `${el.tagName.toLowerCase()}[data-testid="${CSS.escape(testId)}"]`;
    }
    const parts = [];
    let cur = el;
    let depth = 0;
    while (cur && cur.nodeType === 1 && cur !== document.documentElement && depth < 5) {
      let part = cur.tagName.toLowerCase();
      if (cur.id && /^[a-zA-Z][\w-]*$/.test(cur.id)) {
        parts.unshift('#' + CSS.escape(cur.id));
        break;
      }
      const classes = Array.from(cur.classList || [])
        .filter((c) => c && c.length < 32 && !c.startsWith('__kwrok'))
        .slice(0, 2);
      if (classes.length > 0) {
        part += '.' + classes.map((c) => CSS.escape(c)).join('.');
      }
      const parent = cur.parentElement;
      if (parent) {
        const sameTag = Array.from(parent.children).filter((sib) => sib.tagName === cur.tagName);
        if (sameTag.length > 1) {
          const idx = sameTag.indexOf(cur) + 1;
          part += `:nth-of-type(${idx})`;
        }
      }
      parts.unshift(part);
      if (document.querySelectorAll(parts.join(' > ')).length === 1) break;
      cur = parent;
      depth += 1;
    }
    return parts.join(' > ') || el.tagName.toLowerCase();
  };

  // 3. Codex-Style Interactive Visual Element Picker
  let inspectActive = false;
  let overlayHost = null;
  let boxEl = null;
  let badgeEl = null;
  let hoveredEl = null;

  const ensureOverlay = () => {
    if (overlayHost && overlayHost.isConnected) return;
    overlayHost = document.createElement('div');
    overlayHost.id = '__kwrok_inspect_host__';
    overlayHost.style.cssText = 'position:fixed;inset:0;pointer-events:none;z-index:2147483647;';
    boxEl = document.createElement('div');
    boxEl.style.cssText = 'position:fixed;border:2px solid #3b82f6;background:rgba(59,130,246,0.12);border-radius:4px;box-sizing:border-box;pointer-events:none;transition:all 45ms ease-out;display:none;box-shadow:0 0 0 1px rgba(255,255,255,0.35);';
    badgeEl = document.createElement('div');
    badgeEl.style.cssText = 'position:fixed;padding:3px 7px;border-radius:5px;background:#0f172a;color:#f8fafc;font:600 11px/1.35 ui-monospace,SFMono-Regular,Menlo,monospace;pointer-events:none;white-space:nowrap;box-shadow:0 4px 12px rgba(0,0,0,0.28);border:1px solid rgba(96,165,250,0.45);display:none;';
    overlayHost.appendChild(boxEl);
    overlayHost.appendChild(badgeEl);
    (document.documentElement || document.body).appendChild(overlayHost);
  };

  const teardownOverlay = () => {
    hoveredEl = null;
    if (overlayHost && overlayHost.parentNode) {
      overlayHost.parentNode.removeChild(overlayHost);
    }
    overlayHost = null;
    boxEl = null;
    badgeEl = null;
    if (document.documentElement) {
      document.documentElement.style.cursor = '';
    }
  };

  const updateHover = (target) => {
    if (!inspectActive || !target || target === overlayHost || (overlayHost && overlayHost.contains(target))) return;
    hoveredEl = target;
    ensureOverlay();
    const r = target.getBoundingClientRect();
    boxEl.style.display = 'block';
    boxEl.style.left = r.left + 'px';
    boxEl.style.top = r.top + 'px';
    boxEl.style.width = Math.max(r.width, 2) + 'px';
    boxEl.style.height = Math.max(r.height, 2) + 'px';

    const tag = target.tagName.toLowerCase();
    const idPart = target.id ? ('#' + target.id) : '';
    const clsPart = target.classList && target.classList.length
      ? ('.' + Array.from(target.classList).slice(0, 2).join('.'))
      : '';
    badgeEl.textContent = `${tag}${idPart}${clsPart}  ${Math.round(r.width)}×${Math.round(r.height)}`;
    badgeEl.style.display = 'block';
    const badgeTop = r.top >= 28 ? (r.top - 26) : Math.min(window.innerHeight - 26, r.bottom + 6);
    const badgeLeft = Math.max(6, Math.min(window.innerWidth - 220, r.left));
    badgeEl.style.top = badgeTop + 'px';
    badgeEl.style.left = badgeLeft + 'px';
  };

  const onMove = (ev) => {
    if (!inspectActive) return;
    const el = document.elementFromPoint(ev.clientX, ev.clientY);
    if (el) updateHover(el);
  };

  const onClick = (ev) => {
    if (!inspectActive) return;
    ev.preventDefault();
    ev.stopPropagation();
    ev.stopImmediatePropagation();
    const el = hoveredEl || document.elementFromPoint(ev.clientX, ev.clientY) || ev.target;
    window.__kwrokSetInspectMode(false);
    if (!el || el.nodeType !== 1) return;

    const r = el.getBoundingClientRect();
    const cs = window.getComputedStyle(el);
    const stylePairs = [
      ['display', cs.display],
      ['position', cs.position],
      ['color', cs.color],
      ['background', cs.backgroundColor],
      ['font', `${cs.fontWeight} ${cs.fontSize}/${cs.lineHeight}`],
      ['padding', cs.padding],
      ['margin', cs.margin],
      ['radius', cs.borderRadius],
      ['gap', cs.gap]
    ].filter(([, v]) => v && v !== 'none' && v !== 'normal' && v !== '0px' && v !== 'rgba(0, 0, 0, 0)')
     .map(([k, v]) => `${k}: ${v}`)
     .join('; ');

    const text = clampStr((el.innerText || el.value || el.getAttribute('aria-label') || '').replace(/\s+/g, ' ').trim(), 280);
    const clone = el.cloneNode(true);
    const html = clampStr(clone.outerHTML || '', 640);
    const tagLabel = el.tagName.toLowerCase()
      + (el.id ? '#' + el.id : '')
      + (el.classList && el.classList.length ? '.' + Array.from(el.classList).slice(0, 2).join('.') : '');

    post({
      kind: 'element_picked',
      url: String(location.href || ''),
      title: String(document.title || ''),
      selector: cssSelectorFor(el),
      tag_name: tagLabel,
      text_excerpt: text,
      html_snippet: html,
      bounds_summary: `x=${Math.round(r.left)}, y=${Math.round(r.top)}, w=${Math.round(r.width)}px, h=${Math.round(r.height)}px`,
      computed_styles: clampStr(stylePairs, 420)
    });
  };

  const onKeyDown = (ev) => {
    if (!inspectActive) return;
    if (ev.key === 'Escape') {
      ev.preventDefault();
      ev.stopPropagation();
      window.__kwrokSetInspectMode(false);
      post({ kind: 'inspect_cancelled' });
    }
  };

  window.__kwrokSetInspectMode = function(active) {
    inspectActive = !!active;
    if (inspectActive) {
      ensureOverlay();
      if (document.documentElement) document.documentElement.style.cursor = 'crosshair';
      window.addEventListener('mousemove', onMove, true);
      window.addEventListener('click', onClick, true);
      window.addEventListener('keydown', onKeyDown, true);
    } else {
      window.removeEventListener('mousemove', onMove, true);
      window.removeEventListener('click', onClick, true);
      window.removeEventListener('keydown', onKeyDown, true);
      teardownOverlay();
    }
    return inspectActive;
  };

  // 4. Agent DOM Snapshot & Action Executor
  window.__kwrokSnapshotDom = function(maxItems) {
    const limit = typeof maxItems === 'number' && maxItems > 0 ? maxItems : 80;
    const nodes = document.querySelectorAll('a[href], button, input, textarea, select, summary, [role="button"], [role="link"], [role="tab"], [role="menuitem"], [contenteditable="true"]');
    const interactive = [];
    for (const el of nodes) {
      if (interactive.length >= limit) break;
      if (overlayHost && overlayHost.contains(el)) continue;
      const r = el.getBoundingClientRect();
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden' || (r.width === 0 && r.height === 0)) continue;
      interactive.push({
        index: interactive.length,
        tag: el.tagName.toLowerCase(),
        type: el.getAttribute('type') || undefined,
        role: el.getAttribute('role') || undefined,
        selector: cssSelectorFor(el),
        text: clampStr((el.innerText || el.getAttribute('aria-label') || el.getAttribute('title') || '').replace(/\s+/g, ' ').trim(), 120),
        placeholder: el.getAttribute('placeholder') || undefined,
        value: ('value' in el && el.type !== 'password') ? clampStr(el.value, 120) : undefined,
        href: el.getAttribute('href') || undefined,
        disabled: !!el.disabled,
        rect: { x: Math.round(r.left), y: Math.round(r.top), w: Math.round(r.width), h: Math.round(r.height) }
      });
    }
    const headings = Array.from(document.querySelectorAll('h1, h2, h3'))
      .slice(0, 25)
      .map((h) => ({ level: h.tagName.toLowerCase(), text: clampStr((h.innerText || '').trim(), 140) }))
      .filter((h) => h.text.length > 0);
    const bodyText = clampStr((document.body ? document.body.innerText : '').replace(/\n{3,}/g, '\n\n').trim(), 3200);
    return {
      url: String(location.href || ''),
      title: String(document.title || ''),
      readyState: String(document.readyState || ''),
      viewport: { width: window.innerWidth, height: window.innerHeight, scrollX: Math.round(window.scrollX), scrollY: Math.round(window.scrollY) },
      headings,
      interactive,
      bodyText
    };
  };

  const setNativeValue = (el, val) => {
    const proto = el instanceof HTMLTextAreaElement
      ? HTMLTextAreaElement.prototype
      : (el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype);
    const desc = Object.getOwnPropertyDescriptor(proto, 'value');
    if (desc && typeof desc.set === 'function') {
      desc.set.call(el, val);
    } else {
      el.value = val;
    }
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  };

  window.__kwrokAct = function(cmd) {
    const action = cmd && cmd.action;
    if (action === 'scroll') {
      window.scrollBy({ left: Number(cmd.dx || 0), top: Number(cmd.dy || 400), behavior: 'instant' });
      return { ok: true, scrollX: Math.round(window.scrollX), scrollY: Math.round(window.scrollY) };
    }
    let el = null;
    if (cmd && cmd.selector) {
      el = document.querySelector(cmd.selector);
    }
    if (!el && cmd && cmd.text) {
      const needle = String(cmd.text).toLowerCase();
      const candidates = document.querySelectorAll('button, a, [role="button"], label, summary, input[type="submit"]');
      for (const c of candidates) {
        if (((c.innerText || c.value || c.getAttribute('aria-label') || '')).toLowerCase().includes(needle)) {
          el = c;
          break;
        }
      }
    }
    if (!el) {
      return { ok: false, error: 'Target element not found for selector/text' };
    }
    el.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'instant' });
    if (action === 'click') {
      el.focus();
      el.click();
      return { ok: true, selector: cssSelectorFor(el), url: location.href, title: document.title };
    }
    if (action === 'fill') {
      el.focus();
      if (el.isContentEditable) {
        el.textContent = String(cmd.value == null ? '' : cmd.value);
        el.dispatchEvent(new InputEvent('input', { bubbles: true }));
      } else {
        setNativeValue(el, String(cmd.value == null ? '' : cmd.value));
      }
      return { ok: true, selector: cssSelectorFor(el) };
    }
    if (action === 'select') {
      setNativeValue(el, String(cmd.value == null ? '' : cmd.value));
      return { ok: true, selector: cssSelectorFor(el) };
    }
    if (action === 'press') {
      const key = String(cmd.key || 'Enter');
      el.focus();
      el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
      if (key === 'Enter' && el.form && typeof el.form.requestSubmit === 'function') {
        try { el.form.requestSubmit(); } catch (_) {}
      }
      el.dispatchEvent(new KeyboardEvent('keyup', { key, bubbles: true }));
      return { ok: true, selector: cssSelectorFor(el), key };
    }
    if (action === 'hover') {
      el.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
      el.dispatchEvent(new MouseEvent('mouseenter', { bubbles: false }));
      return { ok: true, selector: cssSelectorFor(el) };
    }
    return { ok: false, error: 'Unsupported action: ' + action };
  };
})();
"#;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native {
    use super::*;
    use wry::{
        BackgroundThrottlingPolicy, PageLoadEvent, Rect, WebView, WebViewBuilder,
        dpi::{LogicalPosition, LogicalSize},
    };

    const SAFARI_DESKTOP_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.2 Safari/605.1.15";

    pub struct BrowserWebView {
        inner: WebView,
        #[cfg(target_os = "macos")]
        _nav_proxy: objc2::rc::Retained<macos_nav::KwrokNavDelegateProxy>,
    }

    fn sanitize_url_for_load(raw: &str) -> Result<String, String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err("网址不能为空".into());
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("about:") || lower.starts_with("data:") || lower.starts_with("blob:") {
            return Ok(trimmed.to_owned());
        }
        url::Url::parse(trimmed)
            .map(String::from)
            .map_err(|e| format!("无效的网址格式 ({trimmed})：{e}"))
    }

    impl BrowserWebView {
        pub fn build(window: &impl HasWindowHandle, init: WebViewInit) -> Result<Self, String> {
            let cb_nav = init.on_event.clone();
            let cb_title = init.on_event.clone();
            let cb_load = init.on_event.clone();
            let cb_new_win = init.on_event.clone();
            let cb_ipc = init.on_event.clone();

            let rect = to_wry_rect(init.bounds);
            let mut builder = WebViewBuilder::new()
                .with_bounds(rect)
                .with_visible(init.visible)
                .with_user_agent(SAFARI_DESKTOP_USER_AGENT)
                .with_background_throttling(BackgroundThrottlingPolicy::Disabled)
                .with_devtools(true)
                .with_hotkeys_zoom(true)
                .with_clipboard(true)
                .with_accept_first_mouse(true)
                .with_back_forward_navigation_gestures(true)
                .with_initialization_script(BRIDGE_INIT_JS)
                .with_document_title_changed_handler(move |title| {
                    cb_title(WebViewEvent::TitleChanged(title));
                })
                .with_on_page_load_handler(move |event, url| match event {
                    PageLoadEvent::Started => cb_load(WebViewEvent::LoadStarted(url)),
                    PageLoadEvent::Finished => cb_load(WebViewEvent::LoadFinished(url)),
                })
                .with_new_window_req_handler(move |url, _features| {
                    cb_new_win(WebViewEvent::NewWindowRequested(url));
                    wry::NewWindowResponse::Deny
                })
                .with_ipc_handler(move |req| {
                    if let Ok(parsed) = serde_json::from_str::<BrowserIpcEvent>(req.body()) {
                        cb_ipc(WebViewEvent::Ipc(parsed));
                    }
                });

            #[cfg(not(target_os = "macos"))]
            {
                let cb_win_nav = cb_nav.clone();
                builder = builder.with_navigation_handler(move |url| {
                    cb_win_nav(WebViewEvent::Navigated(url));
                    true
                });
            }

            let pending_initial_url = if let Some(html) = init.initial_html {
                builder = builder.with_html(html);
                None
            } else {
                init.initial_url
            };

            let inner = builder
                .build_as_child(window)
                .map_err(|err| format!("初始化系统浏览器组件失败：{err}"))?;

            #[cfg(target_os = "macos")]
            let nav_proxy = macos_nav::install_navigation_proxy(&inner, cb_nav);

            let view = Self {
                inner,
                #[cfg(target_os = "macos")]
                _nav_proxy: nav_proxy,
            };

            if let Some(url) = pending_initial_url {
                view.load_url(&url)?;
            }

            Ok(view)
        }

        pub fn load_url(&self, url: &str) -> Result<(), String> {
            let valid = sanitize_url_for_load(url)?;
            self.inner.load_url(&valid).map_err(|e| e.to_string())
        }

        pub fn load_html(&self, html: &str) -> Result<(), String> {
            self.inner.load_html(html).map_err(|e| e.to_string())
        }

        pub fn reload(&self) -> Result<(), String> {
            self.inner.reload().map_err(|e| e.to_string())
        }

        pub fn go_back(&self) -> Result<(), String> {
            self.inner.go_back().map_err(|e| e.to_string())
        }

        pub fn go_forward(&self) -> Result<(), String> {
            self.inner.go_forward().map_err(|e| e.to_string())
        }

        pub fn can_go_back(&self) -> bool {
            self.inner.can_go_back().unwrap_or(false)
        }

        pub fn can_go_forward(&self) -> bool {
            self.inner.can_go_forward().unwrap_or(false)
        }

        pub fn is_loading(&self) -> bool {
            #[cfg(target_os = "macos")]
            {
                use wry::WebViewExtMacOS;
                unsafe { self.inner.webview().isLoading() }
            }
            #[cfg(not(target_os = "macos"))]
            {
                false
            }
        }

        pub fn current_url(&self) -> Option<String> {
            self.inner.url().ok().filter(|s| !s.is_empty())
        }

        pub fn set_bounds(&self, rect: LogicalRect) {
            let _ = self.inner.set_bounds(to_wry_rect(rect));
        }

        pub fn set_visible(&self, visible: bool) {
            let _ = self.inner.set_visible(visible);
        }

        pub fn set_zoom(&self, factor: f64) {
            let _ = self.inner.zoom(factor.clamp(0.25, 3.0));
        }

        pub fn focus(&self) {
            let _ = self.inner.focus();
        }

        pub fn focus_parent(&self) {
            let _ = self.inner.focus_parent();
        }

        pub fn open_devtools(&self) {
            self.inner.open_devtools();
        }

        pub fn close_devtools(&self) {
            self.inner.close_devtools();
        }

        pub fn is_devtools_open(&self) -> bool {
            self.inner.is_devtools_open()
        }

        pub fn clear_browsing_data(&self) -> Result<(), String> {
            self.inner
                .clear_all_browsing_data()
                .map_err(|e| e.to_string())
        }

        pub fn eval<F>(&self, js: &str, callback: F) -> Result<(), String>
        where
            F: FnOnce(String) + Send + 'static,
        {
            let slot = std::sync::Mutex::new(Some(callback));
            self.inner
                .evaluate_script_with_callback(js, move |reply| {
                    if let Ok(mut guard) = slot.lock()
                        && let Some(cb) = guard.take()
                    {
                        cb(reply);
                    }
                })
                .map_err(|e| e.to_string())
        }

        pub fn eval_fire_and_forget(&self, js: &str) -> Result<(), String> {
            self.inner.evaluate_script(js).map_err(|e| e.to_string())
        }

        pub fn set_inspect_mode(&self, active: bool) {
            let script = format!(
                "if (window.__kwrokSetInspectMode) {{ window.__kwrokSetInspectMode({}); }}",
                if active { "true" } else { "false" }
            );
            let _ = self.eval_fire_and_forget(&script);
        }

        pub fn capture_snapshot_png<F>(&self, callback: F)
        where
            F: FnOnce(Result<CapturedSnapshot, String>) + Send + 'static,
        {
            #[cfg(target_os = "macos")]
            {
                capture_macos_wkwebview_png(&self.inner, callback);
            }
            #[cfg(not(target_os = "macos"))]
            {
                callback(Err(
                    "Native WebView snapshot capture is currently supported on macOS".into(),
                ));
            }
        }
    }

    fn to_wry_rect(rect: LogicalRect) -> Rect {
        Rect {
            position: LogicalPosition::new(f64::from(rect.x), f64::from(rect.y)).into(),
            size: LogicalSize::new(
                f64::from(rect.width.max(1.0)),
                f64::from(rect.height.max(1.0)),
            )
            .into(),
        }
    }

    #[cfg(target_os = "macos")]
    mod macos_nav {
        use super::{WebViewEvent, WebViewEventCallback};
        use objc2::{
            DeclaredClass, MainThreadOnly, define_class, msg_send,
            rc::Retained,
            runtime::{NSObject, ProtocolObject},
        };
        use objc2_foundation::{MainThreadMarker, NSError, NSObjectProtocol, NSString, ns_string};
        use objc2_web_kit::{
            WKDownload, WKNavigation, WKNavigationAction, WKNavigationActionPolicy,
            WKNavigationDelegate, WKNavigationResponse, WKNavigationResponsePolicy, WKWebView,
        };
        use wry::{WebView, WebViewExtMacOS};

        pub struct KwrokNavDelegateIvars {
            orig: Option<Retained<ProtocolObject<dyn WKNavigationDelegate>>>,
            on_event: WebViewEventCallback,
        }

        fn is_supported_web_scheme(url: &str) -> bool {
            let lower = url.trim_start().to_ascii_lowercase();
            lower.starts_with("http://")
                || lower.starts_with("https://")
                || lower.starts_with("file://")
                || lower.starts_with("about:")
                || lower.starts_with("data:")
                || lower.starts_with("blob:")
                || lower.starts_with("javascript:")
        }

        fn extract_failing_url(webview: &WKWebView, error: &NSError) -> String {
            let info = error.userInfo();
            if let Some(val) = info.objectForKey(ns_string!("NSErrorFailingURLStringKey"))
                && let Ok(ns_str) = val.downcast::<NSString>()
            {
                let s = ns_str.to_string();
                if !s.is_empty() {
                    return s;
                }
            }
            unsafe {
                webview
                    .URL()
                    .and_then(|u| u.absoluteString())
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            }
        }

        fn handle_navigation_failure(
            ivars: &KwrokNavDelegateIvars,
            webview: &WKWebView,
            error: &NSError,
        ) {
            let code = error.code() as i64;
            let url = extract_failing_url(webview, error);
            // -999 = NSURLErrorCancelled (normal when redirects or newer navigations supersede a load)
            // 204 = WebKitErrorPlugInWillHandleLoad, 102 = WebKitErrorFrameLoadInterruptedByPolicyChange
            if matches!(code, -999 | 102 | 204) {
                let still_loading = unsafe { webview.isLoading() };
                if !still_loading {
                    (ivars.on_event)(WebViewEvent::LoadFinished(url));
                }
                return;
            }
            let message = error.localizedDescription().to_string();
            (ivars.on_event)(WebViewEvent::LoadFailed { url, code, message });
        }

        define_class!(
            #[unsafe(super(NSObject))]
            #[thread_kind = MainThreadOnly]
            #[ivars = KwrokNavDelegateIvars]
            pub struct KwrokNavDelegateProxy;

            unsafe impl NSObjectProtocol for KwrokNavDelegateProxy {}

            unsafe impl WKNavigationDelegate for KwrokNavDelegateProxy {
                #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
                fn decide_action(
                    &self,
                    webview: &WKWebView,
                    action: &WKNavigationAction,
                    handler: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy)>,
                ) {
                    let url = unsafe {
                        action
                            .request()
                            .URL()
                            .and_then(|u| u.absoluteString())
                            .map(|s| s.to_string())
                    };
                    let Some(url_str) = url else {
                        handler.call((WKNavigationActionPolicy::Cancel,));
                        return;
                    };

                    // Block custom native-app deep-link schemes (`baiduboxapp://`, `bilibili://`, `weixin://`, etc.)
                    // so portal websites probing installed apps never fail provisional navigation with -1002.
                    if !is_supported_web_scheme(&url_str) {
                        handler.call((WKNavigationActionPolicy::Cancel,));
                        return;
                    }

                    let is_main_frame = unsafe { action.targetFrame() }
                        .is_some_and(|frame| unsafe { frame.isMainFrame() });
                    if is_main_frame
                        && !url_str.starts_with("about:blank")
                        && !url_str.to_ascii_lowercase().starts_with("javascript:")
                    {
                        (self.ivars().on_event)(WebViewEvent::Navigated(url_str));
                    }

                    if let Some(ref orig) = self.ivars().orig {
                        unsafe {
                            orig.webView_decidePolicyForNavigationAction_decisionHandler(
                                webview, action, handler,
                            );
                        }
                    } else {
                        handler.call((WKNavigationActionPolicy::Allow,));
                    }
                }

                #[unsafe(method(webView:decidePolicyForNavigationResponse:decisionHandler:))]
                fn decide_response(
                    &self,
                    webview: &WKWebView,
                    response: &WKNavigationResponse,
                    handler: &block2::DynBlock<dyn Fn(WKNavigationResponsePolicy)>,
                ) {
                    if let Some(ref orig) = self.ivars().orig {
                        unsafe {
                            orig.webView_decidePolicyForNavigationResponse_decisionHandler(
                                webview, response, handler,
                            );
                        }
                    } else {
                        handler.call((WKNavigationResponsePolicy::Allow,));
                    }
                }

                #[unsafe(method(webView:didCommitNavigation:))]
                fn did_commit(&self, webview: &WKWebView, navigation: Option<&WKNavigation>) {
                    if let Some(ref orig) = self.ivars().orig {
                        unsafe {
                            orig.webView_didCommitNavigation(webview, navigation);
                        }
                    }
                }

                #[unsafe(method(webView:didFinishNavigation:))]
                fn did_finish(&self, webview: &WKWebView, navigation: Option<&WKNavigation>) {
                    if let Some(ref orig) = self.ivars().orig {
                        unsafe {
                            orig.webView_didFinishNavigation(webview, navigation);
                        }
                    }
                }

                #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
                fn did_fail_provisional(
                    &self,
                    webview: &WKWebView,
                    _navigation: Option<&WKNavigation>,
                    error: &NSError,
                ) {
                    handle_navigation_failure(self.ivars(), webview, error);
                }

                #[unsafe(method(webView:didFailNavigation:withError:))]
                fn did_fail_navigation(
                    &self,
                    webview: &WKWebView,
                    _navigation: Option<&WKNavigation>,
                    error: &NSError,
                ) {
                    handle_navigation_failure(self.ivars(), webview, error);
                }

                #[unsafe(method(webView:navigationAction:didBecomeDownload:))]
                fn action_download(
                    &self,
                    webview: &WKWebView,
                    action: &WKNavigationAction,
                    download: &WKDownload,
                ) {
                    if let Some(ref orig) = self.ivars().orig {
                        unsafe {
                            orig.webView_navigationAction_didBecomeDownload(
                                webview, action, download,
                            );
                        }
                    }
                }

                #[unsafe(method(webView:navigationResponse:didBecomeDownload:))]
                fn response_download(
                    &self,
                    webview: &WKWebView,
                    response: &WKNavigationResponse,
                    download: &WKDownload,
                ) {
                    if let Some(ref orig) = self.ivars().orig {
                        unsafe {
                            orig.webView_navigationResponse_didBecomeDownload(
                                webview, response, download,
                            );
                        }
                    }
                }

                #[unsafe(method(webViewWebContentProcessDidTerminate:))]
                fn process_terminated(&self, webview: &WKWebView) {
                    if let Some(ref orig) = self.ivars().orig {
                        unsafe {
                            orig.webViewWebContentProcessDidTerminate(webview);
                        }
                    }
                    let url = unsafe {
                        webview
                            .URL()
                            .and_then(|u| u.absoluteString())
                            .map(|s| s.to_string())
                            .unwrap_or_default()
                    };
                    (self.ivars().on_event)(WebViewEvent::LoadFailed {
                        url,
                        code: -1,
                        message: "网页渲染进程意外退出，请点击刷新重试".into(),
                    });
                }
            }
        );

        pub fn install_navigation_proxy(
            webview: &WebView,
            on_event: WebViewEventCallback,
        ) -> Retained<KwrokNavDelegateProxy> {
            let wk = webview.webview();
            let mtm = MainThreadMarker::new().expect("WKWebView must be created on the main thread");
            let orig = unsafe { wk.navigationDelegate() };
            let alloc = mtm
                .alloc::<KwrokNavDelegateProxy>()
                .set_ivars(KwrokNavDelegateIvars { orig, on_event });
            let proxy: Retained<KwrokNavDelegateProxy> = unsafe { msg_send![super(alloc), init] };
            unsafe {
                wk.setNavigationDelegate(Some(ProtocolObject::from_ref(&*proxy)));
            }
            proxy
        }
    }

    #[cfg(target_os = "macos")]
    fn capture_macos_wkwebview_png<F>(webview: &WebView, callback: F)
    where
        F: FnOnce(Result<CapturedSnapshot, String>) + Send + 'static,
    {
        use block2::RcBlock;
        use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
        use objc2_foundation::{NSDictionary, NSError};
        use wry::WebViewExtMacOS;

        let wk = webview.webview();
        let slot = std::sync::Mutex::new(Some(callback));
        let block = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
            let Some(done) = slot.lock().ok().and_then(|mut g| g.take()) else {
                return;
            };
            if !error.is_null() {
                let msg = unsafe { (*error).localizedDescription().to_string() };
                done(Err(format!("WKWebView snapshot failed: {msg}")));
                return;
            }
            if image.is_null() {
                done(Err("WKWebView returned an empty snapshot image".into()));
                return;
            }
            let encoded = unsafe {
                let ns_image = &*image;
                let Some(tiff) = ns_image.TIFFRepresentation() else {
                    done(Err("Failed to read NSImage TIFF representation".into()));
                    return;
                };
                let Some(rep) = NSBitmapImageRep::imageRepWithData(&tiff) else {
                    done(Err("Failed to decode NSBitmapImageRep".into()));
                    return;
                };
                let width = rep.pixelsWide().max(1) as u32;
                let height = rep.pixelsHigh().max(1) as u32;
                let props = NSDictionary::new();
                let Some(png_data) = rep
                    .representationUsingType_properties(NSBitmapImageFileType::PNG, &props)
                else {
                    done(Err("Failed to encode WKWebView snapshot as PNG".into()));
                    return;
                };
                CapturedSnapshot {
                    png_bytes: png_data.to_vec(),
                    width,
                    height,
                }
            };
            done(Ok(encoded));
        });

        unsafe {
            wk.takeSnapshotWithConfiguration_completionHandler(None, &block);
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod native {
    use super::*;

    pub struct BrowserWebView {
        url: std::sync::Mutex<Option<String>>,
    }

    impl BrowserWebView {
        pub fn build(_window: &impl HasWindowHandle, init: WebViewInit) -> Result<Self, String> {
            Ok(Self {
                url: std::sync::Mutex::new(init.initial_url),
            })
        }

        pub fn load_url(&self, url: &str) -> Result<(), String> {
            if let Ok(mut g) = self.url.lock() {
                *g = Some(url.to_owned());
            }
            Ok(())
        }

        pub fn load_html(&self, _html: &str) -> Result<(), String> {
            Ok(())
        }

        pub fn reload(&self) -> Result<(), String> {
            Ok(())
        }

        pub fn go_back(&self) -> Result<(), String> {
            Ok(())
        }

        pub fn go_forward(&self) -> Result<(), String> {
            Ok(())
        }

        pub fn can_go_back(&self) -> bool {
            false
        }

        pub fn can_go_forward(&self) -> bool {
            false
        }

        pub fn is_loading(&self) -> bool {
            false
        }

        pub fn current_url(&self) -> Option<String> {
            self.url.lock().ok().and_then(|g| g.clone())
        }

        pub fn set_bounds(&self, _rect: LogicalRect) {}
        pub fn set_visible(&self, _visible: bool) {}
        pub fn set_zoom(&self, _factor: f64) {}
        pub fn focus(&self) {}
        pub fn focus_parent(&self) {}
        pub fn open_devtools(&self) {}
        pub fn close_devtools(&self) {}
        pub fn is_devtools_open(&self) -> bool {
            false
        }
        pub fn clear_browsing_data(&self) -> Result<(), String> {
            Ok(())
        }
        pub fn eval<F>(&self, _js: &str, callback: F) -> Result<(), String>
        where
            F: FnOnce(String) + Send + 'static,
        {
            callback("null".into());
            Ok(())
        }
        pub fn eval_fire_and_forget(&self, _js: &str) -> Result<(), String> {
            Ok(())
        }
        pub fn set_inspect_mode(&self, _active: bool) {}
        pub fn capture_snapshot_png<F>(&self, callback: F)
        where
            F: FnOnce(Result<CapturedSnapshot, String>) + Send + 'static,
        {
            callback(Err(
                "System WebView snapshot is unavailable on this platform".into(),
            ));
        }
    }
}

pub use native::BrowserWebView;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omnibox_normalizes_ports_localhost_domains_ime_unicode_and_bing_search() {
        assert_eq!(
            normalize_omnibox_input(":5173", None).as_deref(),
            Some("http://localhost:5173/")
        );
        assert_eq!(
            normalize_omnibox_input("：3000", None).as_deref(),
            Some("http://localhost:3000/")
        );
        assert_eq!(
            normalize_omnibox_input(":3000/login?next=1", None).as_deref(),
            Some("http://localhost:3000/login?next=1")
        );
        assert_eq!(
            normalize_omnibox_input("localhost:8080/api", None).as_deref(),
            Some("http://localhost:8080/api")
        );
        assert_eq!(
            normalize_omnibox_input("0.0.0.0:4173", None).as_deref(),
            Some("http://localhost:4173/")
        );
        assert_eq!(
            normalize_omnibox_input("http://0.0.0.0:4173/app", None).as_deref(),
            Some("http://localhost:4173/app")
        );
        assert_eq!(
            normalize_omnibox_input("127.0.0.1:9000", None).as_deref(),
            Some("http://127.0.0.1:9000/")
        );
        assert_eq!(
            normalize_omnibox_input("192.168.1.42:8000", None).as_deref(),
            Some("http://192.168.1.42:8000/")
        );
        assert_eq!(
            normalize_omnibox_input("openai.com/codex", None).as_deref(),
            Some("https://openai.com/codex")
        );
        assert_eq!(
            normalize_omnibox_input("www。baidu。com", None).as_deref(),
            Some("https://www.baidu.com/")
        );
        assert_eq!(
            normalize_omnibox_input("https://baike.baidu.com/item/人工智能", None).as_deref(),
            Some("https://baike.baidu.com/item/%E4%BA%BA%E5%B7%A5%E6%99%BA%E8%83%BD")
        );
        assert_eq!(
            normalize_omnibox_input("gpui rust layout guide", None).as_deref(),
            Some("https://cn.bing.com/search?q=gpui+rust+layout+guide")
        );
        assert_eq!(
            normalize_omnibox_input("百度热搜", None).as_deref(),
            Some("https://cn.bing.com/search?q=%E7%99%BE%E5%BA%A6%E7%83%AD%E6%90%9C")
        );
        assert!(normalize_omnibox_input("   ", None).is_none());

        let tmp = std::env::temp_dir().join("kwrok_omnibox_test.html");
        std::fs::write(&tmp, "<!doctype html><title>ok</title>").unwrap();
        let file_url = normalize_omnibox_input(
            "kwrok_omnibox_test.html",
            tmp.parent(),
        )
        .unwrap();
        assert!(file_url.starts_with("file:///"), "{file_url}");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn ipc_events_deserialize_state_console_and_picked_elements() {
        let state: BrowserIpcEvent = serde_json::from_str(
            r#"{"kind":"state","url":"http://localhost:5173/","title":"App","ready_state":"complete","can_go_back":true,"can_go_forward":false}"#,
        )
        .unwrap();
        assert!(matches!(state, BrowserIpcEvent::State { can_go_back: true, .. }));

        let err: BrowserIpcEvent = serde_json::from_str(
            r#"{"kind":"console","level":"error","message":"ReferenceError: foo is not defined","source":"app.tsx:14","timestamp_ms":100}"#,
        )
        .unwrap();
        assert!(matches!(
            err,
            BrowserIpcEvent::Console {
                level: ConsoleLevel::Error,
                ..
            }
        ));

        let picked: BrowserIpcEvent = serde_json::from_str(
            r##"{"kind":"element_picked","url":"http://localhost:5173","title":"Demo","selector":"#hero > button.cta","tag_name":"button.cta","text_excerpt":"Start","html_snippet":"<button class=\"cta\">Start</button>","bounds_summary":"x=10, y=20, w=120px, h=40px","computed_styles":"display: flex"}"##,
        )
        .unwrap();
        assert!(matches!(picked, BrowserIpcEvent::ElementPicked { .. }));
    }
}
