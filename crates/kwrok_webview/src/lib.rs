//! Native system WebView integration (`WKWebView` on macOS, `WebView2` on Windows)
//! with live DOM element picking, console/network error telemetry, agent DOM snapshot/action
//! execution, crash-proof Objective-C delegate proxies, safe downloads, and Retina PNG snapshot
//! capture for zero Z-order bleed in GPUI.

use std::path::{Path, PathBuf};
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
    CloseRequested,
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
    HistoryStateChanged {
        url: String,
        can_go_back: bool,
        can_go_forward: bool,
    },
    NewWindowRequested(String),
    CloseRequested,
    DownloadStarted {
        url: String,
        file_name: String,
    },
    DownloadCompleted {
        url: String,
        path: Option<PathBuf>,
        success: bool,
    },
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

static DOWNLOAD_DIR_OVERRIDE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Override the target directory for browser downloads (used by sandboxed E2E tests or preferences).
pub fn set_download_dir_override(dir: Option<PathBuf>) {
    if let Ok(mut guard) = DOWNLOAD_DIR_OVERRIDE.lock() {
        *guard = dir;
    }
}

const MAX_SEARCH_QUERY_CHARS: usize = 1024;

fn strip_ascii_controls(raw: &str) -> String {
    raw.chars().filter(|c| !c.is_ascii_control()).collect()
}

fn is_dangerous_script_scheme(s: &str) -> bool {
    let lower = s.trim_start().to_ascii_lowercase();
    lower.starts_with("javascript:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("javascript：")
        || lower.starts_with("vbscript：")
}

/// Percent-encode unsafe characters in `data:` / `blob:` / `about:` URIs so Foundation's
/// `NSURL::URLWithString` never returns `nil` on `<`, `>`, spaces, quotes, or raw UTF-8.
pub fn encode_rfc3986_uri_preserving_escapes(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(bytes.len() + 16);
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            out.push('%');
            out.push(bytes[i + 1] as char);
            out.push(bytes[i + 2] as char);
            i += 3;
            continue;
        }
        let safe_ascii = b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'.'
                    | b'_'
                    | b'~'
                    | b':'
                    | b'/'
                    | b'?'
                    | b'#'
                    | b'['
                    | b']'
                    | b'@'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
            );
        if safe_ascii {
            out.push(b as char);
        } else {
            use std::fmt::Write;
            let _ = write!(&mut out, "%{b:02X}");
        }
        i += 1;
    }
    out
}

fn canonicalize_http_or_file_url(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw).ok()?;
    match parsed.scheme() {
        "http" | "https" => {
            let host = parsed.host_str()?;
            if host.is_empty() {
                return None;
            }
            Some(parsed.into())
        }
        "file" => Some(parsed.into()),
        _ => None,
    }
}

/// Sanitize a suggested download filename so malicious `Content-Disposition` headers
/// (`../../.ssh/authorized_keys`, `.bashrc`, NUL/control bytes) can never escape the target folder.
pub fn sanitize_download_filename(raw: &str) -> String {
    let cleaned: String = raw.chars().filter(|c| !c.is_control()).collect();
    let leaf = cleaned
        .rsplit(['/', '\\'])
        .find(|seg| !seg.trim().is_empty())
        .unwrap_or("");
    let safe_chars: String = leaf
        .chars()
        .map(|c| if c == ':' { '_' } else { c })
        .collect();
    let trimmed = safe_chars.trim().trim_start_matches('.').trim();
    if trimmed.is_empty() {
        return "download.bin".into();
    }
    let clamped: String = trimmed.chars().take(180).collect();
    if clamped.is_empty() {
        "download.bin".into()
    } else {
        clamped
    }
}

/// Pick a non-colliding file path in `dir` (`report.pdf`, `report (1).pdf`, ...).
pub fn unique_download_destination(dir: &Path, suggested_name: &str) -> PathBuf {
    let safe_name = sanitize_download_filename(suggested_name);
    let first = dir.join(&safe_name);
    if !first.exists() {
        return first;
    }
    let path_obj = Path::new(&safe_name);
    let stem = path_obj
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("download");
    let ext = path_obj.extension().and_then(|e| e.to_str()).unwrap_or("");
    for n in 1..10_000u32 {
        let candidate_name = if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        let candidate = dir.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}", std::process::id()))
}

fn host_prefix_looks_like_authority(prefix: &str) -> bool {
    if prefix.is_empty() || prefix.contains(char::is_whitespace) {
        return false;
    }
    let had_fullwidth_dot = prefix.contains('。');
    let mapped = prefix.replace('。', ".").replace('：', ":");
    let host_only = if let Some(rest) = mapped.strip_prefix('[') {
        let Some((inside, after)) = rest.split_once(']') else {
            return false;
        };
        if !after.is_empty() && !after.starts_with(':') {
            return false;
        }
        inside
    } else {
        mapped.split(':').next().unwrap_or(&mapped)
    };
    if let Some((_, port_part)) = mapped.rsplit_once(':')
        && !mapped.ends_with(']')
        && (!mapped.starts_with('[') || mapped.contains("]:"))
    {
        let Ok(p) = port_part.parse::<u16>() else {
            return false;
        };
        if p == 0 {
            return false;
        }
    }
    if host_only.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if host_only.contains(':') {
        return host_only.chars().all(|c| c.is_ascii_hexdigit() || c == ':');
    }
    let parts: Vec<&str> = host_only.split('.').collect();
    if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
        return false;
    }
    let is_ipv4 = parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok());
    if is_ipv4 {
        return true;
    }
    if parts.len() == 4 && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) {
        // Reject invalid IPv4 octets like `256.256.256.256`
        return false;
    }
    let tld = parts.last().copied().unwrap_or("");
    let ascii_tld = tld.len() >= 2
        && tld.len() <= 24
        && (tld.starts_with("xn--") || tld.chars().all(|c| c.is_ascii_alphabetic()));
    let known_cjk_idn_tld = !had_fullwidth_dot
        && matches!(
            tld,
            "中国" | "公司" | "网络" | "商城" | "网址" | "在线" | "信息" | "集团" | "中文网"
        );
    if !ascii_tld && !known_cjk_idn_tld {
        return false;
    }
    parts.iter().all(|label| {
        !label.starts_with('-')
            && !label.ends_with('-')
            && if had_fullwidth_dot {
                label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            } else {
                label
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            }
    })
}

/// Normalize Chinese IME fullwidth URL punctuation (`www。baidu。com/s？wd=人工智能`,
/// `localhost：5173/看板`, `：3000`, `https：//...`) without altering natural-language sentences.
fn normalize_ime_url_punctuation(trimmed: &str) -> String {
    if let Some(rest) = trimmed.strip_prefix('：')
        && rest.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        return format!(
            ":{}",
            rest.replace('／', "/")
                .replace('？', "?")
                .replace('＆', "&")
                .replace('＝', "=")
        );
    }

    let mut s = trimmed.to_owned();
    for (bad, good) in [
        ("https：//", "https://"),
        ("HTTPS：//", "https://"),
        ("http：//", "http://"),
        ("HTTP：//", "http://"),
        ("file：//", "file://"),
        ("https：／／", "https://"),
        ("http：／／", "http://"),
    ] {
        if let Some(rest) = s.strip_prefix(bad) {
            s = format!("{good}{rest}");
            break;
        }
    }

    if s.contains(char::is_whitespace) {
        return s;
    }

    let (scheme_prefix, after_scheme) = if let Some(rest) = s.strip_prefix("https://") {
        ("https://", rest)
    } else if let Some(rest) = s.strip_prefix("http://") {
        ("http://", rest)
    } else {
        ("", s.as_str())
    };

    let split_idx = after_scheme
        .char_indices()
        .find(|&(_, c)| matches!(c, '/' | '／' | '?' | '？' | '#' | '＃'))
        .map_or(after_scheme.len(), |(idx, _)| idx);
    let authority = &after_scheme[..split_idx];
    let tail = &after_scheme[split_idx..];

    if host_prefix_looks_like_authority(authority) {
        let fixed_auth = authority.replace('。', ".").replace('：', ":");
        let fixed_tail = tail
            .replace('／', "/")
            .replace('？', "?")
            .replace('＆', "&")
            .replace('＝', "=")
            .replace('＃', "#");
        return format!("{scheme_prefix}{fixed_auth}{fixed_tail}");
    }

    s
}

fn parse_port_shorthand(rest: &str) -> Option<String> {
    let split_at = rest
        .find(['/', '?', '#'])
        .unwrap_or(rest.len());
    let port_str = &rest[..split_at];
    let tail = &rest[split_at..];
    let port: u16 = port_str.parse().ok()?;
    if port == 0 {
        return None;
    }
    canonicalize_http_or_file_url(&format!("http://localhost:{port}{tail}"))
}

/// Normalize an omnibox entry (`:5173`, `localhost:3000`, `example.com`, workspace `.html`, or search query)
/// into a canonical, RFC 3986 percent-encoded navigable URL.
/// Returns `None` for empty inputs or blocked schemes (`javascript:`, `vbscript:`).
pub fn normalize_omnibox_input(raw: &str, workspace: Option<&Path>) -> Option<String> {
    let cleaned = strip_ascii_controls(raw);
    let raw_trimmed = cleaned.trim();
    if raw_trimmed.is_empty() || is_dangerous_script_scheme(raw_trimmed) {
        return None;
    }
    let normalized = normalize_ime_url_punctuation(raw_trimmed);
    let trimmed = normalized.as_str();
    if is_dangerous_script_scheme(trimmed) {
        return None;
    }

    // `:3000` or `:5173/dashboard` shorthand -> `http://localhost:...`
    if let Some(rest) = trimmed.strip_prefix(':')
        && rest.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        return parse_port_shorthand(rest);
    }

    // Already has a known scheme
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("file://")
    {
        if lower.starts_with("http://0.0.0.0:") {
            let suffix = &trimmed["http://0.0.0.0:".len()..];
            return canonicalize_http_or_file_url(&format!("http://localhost:{suffix}"));
        }
        return canonicalize_http_or_file_url(trimmed);
    }
    if lower.starts_with("data:") || lower.starts_with("blob:") {
        return Some(encode_rfc3986_uri_preserving_escapes(trimmed));
    }
    if lower.starts_with("about:") {
        let rest = &trimmed["about:".len()..];
        if rest.is_empty() || rest.contains(char::is_whitespace) {
            return None;
        }
        return Some(encode_rfc3986_uri_preserving_escapes(trimmed));
    }

    // Localhost / loopback without scheme (`localhost:5173`, `127.0.0.1:8080`, `[::1]:3000`)
    if lower == "localhost"
        || lower.starts_with("localhost:")
        || lower.starts_with("localhost/")
        || lower.starts_with("localhost?")
        || lower.starts_with("127.0.0.1")
        || lower.starts_with("[::1]")
    {
        return canonicalize_http_or_file_url(&format!("http://{trimmed}"));
    }
    if let Some(suffix) = trimmed.strip_prefix("0.0.0.0:") {
        return canonicalize_http_or_file_url(&format!("http://localhost:{suffix}"));
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
        let split_idx = trimmed
            .find(['/', '?', '#'])
            .unwrap_or(trimmed.len());
        let host_port = &trimmed[..split_idx];
        if host_prefix_looks_like_authority(host_port) {
            let host = if let Some(rest) = host_port.strip_prefix('[') {
                rest.split(']').next().unwrap_or("")
            } else {
                host_port.split(':').next().unwrap_or(host_port)
            };
            let is_ipv4 = host.split('.').count() == 4
                && host.split('.').all(|part| part.parse::<u8>().is_ok());
            let is_ipv6 = host_port.starts_with('[');
            if is_ipv4 || is_ipv6 {
                return canonicalize_http_or_file_url(&format!("http://{trimmed}"));
            }
            let scheme = if host.ends_with(".local") || host_port.contains(':') {
                "http"
            } else {
                "https"
            };
            if let Some(valid) = canonicalize_http_or_file_url(&format!("{scheme}://{trimmed}")) {
                return Some(valid);
            }
        }
    }

    // Fallback: Bing web search (accessible with low latency in Mainland China & globally)
    let clamped_query: String = raw_trimmed.chars().take(MAX_SEARCH_QUERY_CHARS).collect();
    let mut search = url::Url::parse("https://cn.bing.com/search").ok()?;
    search.query_pairs_mut().append_pair("q", &clamped_query);
    Some(search.into())
}

/// Early-document script injected into every Kwork WebView tab:
/// 1. Intrinsics snapshot (immune to page prototype pollution / `JSON.stringify` overrides).
/// 2. Console (`log`/`info`/`warn`/`error`), uncaught errors, unhandled rejections & failed `fetch` telemetry.
/// 3. Coalesced SPA route, `pageshow` (bfcache) & title observer (`pushState`/`replaceState`/`popstate`/`MutationObserver`).
/// 4. Codex-style interactive DOM Element Picker (`window.__kwrokSetInspectMode(bool)`).
/// 5. Agent DOM snapshot (`window.__kwrokSnapshotDom`), resilient action executor (`window.__kwrokAct`),
///    and crash-proof arbitrary JS evaluator (`window.__kwrokEval`).
pub const BRIDGE_INIT_JS: &str = r#"
(function() {
  if (window.__kwrokBridgeInstalled) return;
  const _defProp = Object.defineProperty;
  const _stringify = JSON.stringify;
  const _ArrayFrom = Array.from;
  const _cssEscape = (typeof CSS !== 'undefined' && typeof CSS.escape === 'function')
    ? CSS.escape.bind(CSS)
    : (s) => String(s).replace(/([^\w-])/g, '\\$1');
  const _rawPost = (window.ipc && typeof window.ipc.postMessage === 'function')
    ? window.ipc.postMessage.bind(window.ipc)
    : null;

  try {
    _defProp(window, '__kwrokBridgeInstalled', { value: true, configurable: false, enumerable: false, writable: false });
  } catch (_) {
    window.__kwrokBridgeInstalled = true;
  }

  const safeWellFormed = (s) => {
    const str = String(s == null ? '' : s);
    return typeof str.toWellFormed === 'function' ? str.toWellFormed() : str;
  };

  const post = (payload) => {
    try {
      if (_rawPost) {
        _rawPost(_stringify(payload));
      } else if (window.ipc && typeof window.ipc.postMessage === 'function') {
        window.ipc.postMessage(_stringify(payload));
      }
    } catch (_) {}
  };

  const clampStr = (s, maxLen) => {
    const str = safeWellFormed(s);
    return str.length > maxLen ? str.slice(0, maxLen) + '…' : str;
  };

  const safeSerializeValue = (val, maxDepth) => {
    const seen = new WeakSet();
    const walk = (v, depth) => {
      if (v === null || typeof v === 'boolean') return v;
      if (typeof v === 'undefined') return null;
      if (typeof v === 'number') {
        if (Number.isNaN(v)) return '[NaN]';
        if (!Number.isFinite(v)) return v > 0 ? '[Infinity]' : '[-Infinity]';
        return v;
      }
      if (typeof v === 'bigint') return v.toString() + 'n';
      if (typeof v === 'string') return safeWellFormed(v);
      if (typeof v === 'symbol') return String(v);
      if (typeof v === 'function') return `[Function: ${v.name || 'anonymous'}]`;
      if (v instanceof Error) {
        return { name: v.name, message: safeWellFormed(v.message), stack: clampStr(v.stack || '', 1200) };
      }
      if (v instanceof RegExp || v instanceof Date) return String(v);
      if (typeof Node !== 'undefined' && v instanceof Node) {
        if (v.nodeType === 1) {
          const id = v.id ? '#' + v.id : '';
          return `<${String(v.tagName || 'node').toLowerCase()}${id}>`;
        }
        return `[Node ${v.nodeName}]`;
      }
      if (typeof v === 'object') {
        if (seen.has(v)) return '[Circular]';
        if (depth >= maxDepth) return Object.prototype.toString.call(v);
        seen.add(v);
        if (Array.isArray(v)) {
          return v.slice(0, 200).map((item) => walk(item, depth + 1));
        }
        if (v instanceof Map) {
          const obj = {};
          let n = 0;
          for (const [k, mv] of v.entries()) {
            if (n++ >= 100) break;
            obj[String(k)] = walk(mv, depth + 1);
          }
          return obj;
        }
        if (v instanceof Set) {
          return _ArrayFrom(v).slice(0, 100).map((item) => walk(item, depth + 1));
        }
        const out = {};
        const keys = Object.keys(v).slice(0, 150);
        for (const k of keys) {
          try {
            out[safeWellFormed(k)] = walk(v[k], depth + 1);
          } catch (err) {
            out[safeWellFormed(k)] = `[Getter Error: ${err && err.message ? err.message : err}]`;
          }
        }
        return out;
      }
      return safeWellFormed(v);
    };
    return walk(val, 0);
  };

  const formatError = (err) => {
    if (!err) return 'Unknown error';
    if (typeof err !== 'object') return safeWellFormed(err);
    const header = (err.name || 'Error') + (err.message ? ': ' + err.message : '');
    const stack = err.stack ? String(err.stack) : '';
    if (!stack) return safeWellFormed(header);
    return safeWellFormed(stack.includes(header) ? stack : (header + '\n' + stack));
  };

  const formatArg = (arg) => {
    if (arg instanceof Error) {
      return formatError(arg);
    }
    if (typeof arg === 'object' && arg !== null) {
      try { return _stringify(safeSerializeValue(arg, 4)); } catch (_) { return Object.prototype.toString.call(arg); }
    }
    return safeWellFormed(arg);
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
    if (sentInWindow >= 50) return;
    sentInWindow += 1;
    post({
      kind: 'console',
      level,
      message: clampStr(message, 1200),
      source: clampStr(source || '', 240),
      timestamp_ms: now
    });
  };

  let lastBridgeCaughtAt = { ts: 0 };

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
    if (!ev.error && ev.message === 'Script error.' && (Date.now() - lastBridgeCaughtAt.ts) < 25) {
      return;
    }
    const loc = ev.filename ? (ev.filename + ':' + (ev.lineno || 0) + ':' + (ev.colno || 0)) : 'window.onerror';
    const msg = ev.error ? formatError(ev.error) : (ev.message || 'Uncaught error');
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
      let p;
      try {
        p = origFetch.apply(this, args);
      } catch (syncErr) {
        lastBridgeCaughtAt.ts = Date.now();
        emitConsole('error', `Fetch error (${reqUrl}): ${formatError(syncErr)}`, 'fetch');
        throw syncErr;
      }
      return p.then((res) => {
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

  const origWinClose = typeof window.close === 'function' ? window.close.bind(window) : null;
  window.close = function() {
    post({ kind: 'close_requested' });
    if (origWinClose) {
      try { origWinClose(); } catch (_) {}
    }
  };

  // 2. Coalesced Page State, SPA Router & BFCache (`pageshow`) Observer
  let stateScheduled = false;
  const reportStateNow = () => {
    stateScheduled = false;
    const navApi = window.navigation;
    post({
      kind: 'state',
      url: safeWellFormed(location.href || ''),
      title: safeWellFormed(document.title || ''),
      ready_state: safeWellFormed(document.readyState || ''),
      can_go_back: !!(navApi && navApi.canGoBack),
      can_go_forward: !!(navApi && navApi.canGoForward)
    });
  };
  const scheduleReportState = () => {
    if (stateScheduled) return;
    stateScheduled = true;
    setTimeout(reportStateNow, 12);
  };

  ['pushState', 'replaceState'].forEach((fn) => {
    const orig = history[fn];
    if (typeof orig === 'function') {
      history[fn] = function(...args) {
        try {
          const out = orig.apply(this, args);
          scheduleReportState();
          return out;
        } catch (err) {
          lastBridgeCaughtAt.ts = Date.now();
          emitConsole('error', formatError(err), 'history.' + fn);
          throw err;
        }
      };
    }
  });
  window.addEventListener('popstate', scheduleReportState);
  window.addEventListener('hashchange', scheduleReportState);
  window.addEventListener('pageshow', reportStateNow);
  window.addEventListener('DOMContentLoaded', () => {
    reportStateNow();
    const titleEl = document.querySelector('title') || document.head;
    if (titleEl && typeof MutationObserver === 'function') {
      new MutationObserver(scheduleReportState).observe(titleEl, { subtree: true, childList: true, characterData: true });
    }
  });
  window.addEventListener('load', reportStateNow);
  if (document.readyState === 'interactive' || document.readyState === 'complete') {
    scheduleReportState();
  }

  // Helper: Safe element class list (works for both HTMLElement and SVGElement)
  const safeClassesOf = (el) => {
    if (!el || !el.classList) return [];
    try {
      return _ArrayFrom(el.classList).filter((c) => c && c.length < 36 && !c.startsWith('__kwrok'));
    } catch (_) {
      return [];
    }
  };

  // Helper: Shortest reliable CSS selector for an element (exception-safe)
  const cssSelectorFor = (el) => {
    if (!el || el.nodeType !== 1) return 'body';
    const root = (typeof el.getRootNode === 'function' && el.getRootNode() instanceof ShadowRoot)
      ? el.getRootNode()
      : document;
    const rawId = typeof el.getAttribute === 'function' ? el.getAttribute('id') : '';
    if (rawId && /^[a-zA-Z][\w-]*$/.test(rawId)) {
      const idSel = '#' + _cssEscape(rawId);
      try {
        if (root.querySelectorAll(idSel).length === 1) return idSel;
      } catch (_) {}
    }
    const testId = el.getAttribute && (el.getAttribute('data-testid') || el.getAttribute('data-test'));
    if (testId) {
      const candidate = `${el.tagName.toLowerCase()}[data-testid="${_cssEscape(testId)}"]`;
      try {
        if (root.querySelectorAll(candidate).length === 1) return candidate;
      } catch (_) {}
    }
    const parts = [];
    let cur = el;
    let depth = 0;
    while (cur && cur.nodeType === 1 && cur !== document.documentElement && depth < 5) {
      let part = cur.tagName.toLowerCase();
      const cid = cur.getAttribute ? cur.getAttribute('id') : '';
      if (cid && /^[a-zA-Z][\w-]*$/.test(cid)) {
        parts.unshift('#' + _cssEscape(cid));
        break;
      }
      const classes = safeClassesOf(cur).slice(0, 2);
      if (classes.length > 0) {
        part += '.' + classes.map((c) => _cssEscape(c)).join('.');
      }
      const parent = cur.parentElement;
      if (parent) {
        const sameTag = _ArrayFrom(parent.children).filter((sib) => sib.tagName === cur.tagName);
        if (sameTag.length > 1) {
          const idx = sameTag.indexOf(cur) + 1;
          part += `:nth-of-type(${idx})`;
        }
      }
      parts.unshift(part);
      const joined = parts.join(' > ');
      try {
        if (root.querySelectorAll(joined).length === 1) break;
      } catch (_) {}
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
    const mountRoot = document.documentElement || document.body;
    if (!mountRoot) return;
    overlayHost = document.createElement('div');
    overlayHost.id = '__kwrok_inspect_host__';
    overlayHost.style.cssText = 'position:fixed;inset:0;pointer-events:none;z-index:2147483647;';
    boxEl = document.createElement('div');
    boxEl.style.cssText = 'position:fixed;border:2px solid #3b82f6;background:rgba(59,130,246,0.12);border-radius:4px;box-sizing:border-box;pointer-events:none;transition:all 45ms ease-out;display:none;box-shadow:0 0 0 1px rgba(255,255,255,0.35);';
    badgeEl = document.createElement('div');
    badgeEl.style.cssText = 'position:fixed;padding:3px 7px;border-radius:5px;background:#0f172a;color:#f8fafc;font:600 11px/1.35 ui-monospace,SFMono-Regular,Menlo,monospace;pointer-events:none;white-space:nowrap;box-shadow:0 4px 12px rgba(0,0,0,0.28);border:1px solid rgba(96,165,250,0.45);display:none;';
    overlayHost.appendChild(boxEl);
    overlayHost.appendChild(badgeEl);
    mountRoot.appendChild(overlayHost);
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
    if (!inspectActive || !target || target.nodeType !== 1 || target === overlayHost || (overlayHost && overlayHost.contains(target))) return;
    hoveredEl = target;
    ensureOverlay();
    if (!boxEl || !badgeEl) return;
    const r = target.getBoundingClientRect();
    boxEl.style.display = 'block';
    boxEl.style.left = r.left + 'px';
    boxEl.style.top = r.top + 'px';
    boxEl.style.width = Math.max(r.width, 2) + 'px';
    boxEl.style.height = Math.max(r.height, 2) + 'px';

    const tag = target.tagName.toLowerCase();
    const tid = target.getAttribute ? target.getAttribute('id') : '';
    const idPart = tid ? ('#' + tid) : '';
    const classes = safeClassesOf(target).slice(0, 2);
    const clsPart = classes.length ? ('.' + classes.join('.')) : '';
    badgeEl.textContent = `${tag}${idPart}${clsPart}  ${Math.round(r.width)}×${Math.round(r.height)}`;
    badgeEl.style.display = 'block';
    const badgeTop = r.top >= 28 ? (r.top - 26) : Math.min(window.innerHeight - 26, r.bottom + 6);
    const badgeLeft = Math.max(6, Math.min(window.innerWidth - 220, r.left));
    badgeEl.style.top = badgeTop + 'px';
    badgeEl.style.left = badgeLeft + 'px';
  };

  const resolveDeepTarget = (ev) => {
    if (typeof ev.composedPath === 'function') {
      const path = ev.composedPath();
      for (const n of path) {
        if (n && n.nodeType === 1 && n !== overlayHost && !(overlayHost && overlayHost.contains(n))) {
          return n;
        }
      }
    }
    return document.elementFromPoint(ev.clientX, ev.clientY);
  };

  const onMove = (ev) => {
    if (!inspectActive) return;
    const el = resolveDeepTarget(ev);
    if (el) updateHover(el);
  };

  const onClick = (ev) => {
    if (!inspectActive) return;
    ev.preventDefault();
    ev.stopPropagation();
    ev.stopImmediatePropagation();
    const el = hoveredEl || resolveDeepTarget(ev) || ev.target;
    setInspectMode(false);
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

    const rawText = el.innerText || el.textContent || el.value || (el.getAttribute && el.getAttribute('aria-label')) || '';
    const text = clampStr(String(rawText).replace(/\s+/g, ' ').trim(), 280);
    const clone = el.cloneNode(true);
    const html = clampStr(clone.outerHTML || '', 640);
    const tid = el.getAttribute ? el.getAttribute('id') : '';
    const classes = safeClassesOf(el).slice(0, 2);
    const tagLabel = el.tagName.toLowerCase()
      + (tid ? '#' + tid : '')
      + (classes.length ? '.' + classes.join('.') : '');

    post({
      kind: 'element_picked',
      url: safeWellFormed(location.href || ''),
      title: safeWellFormed(document.title || ''),
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
      setInspectMode(false);
      post({ kind: 'inspect_cancelled' });
    }
  };

  const setInspectMode = function(active) {
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

  // 4. Agent DOM Snapshot, Resilient Action Executor & Safe JS Evaluator
  const snapshotDom = function(maxItems) {
    const limit = typeof maxItems === 'number' && maxItems > 0 ? Math.min(maxItems, 250) : 80;
    const nodes = document.querySelectorAll('a[href], button, input, textarea, select, summary, [role="button"], [role="link"], [role="tab"], [role="menuitem"], [contenteditable="true"]');
    const interactive = [];
    for (const el of nodes) {
      if (interactive.length >= limit) break;
      if (overlayHost && overlayHost.contains(el)) continue;
      const r = el.getBoundingClientRect();
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden' || (r.width === 0 && r.height === 0)) continue;
      const rawTxt = el.innerText || el.textContent || el.getAttribute('aria-label') || el.getAttribute('title') || '';
      interactive.push({
        index: interactive.length,
        tag: el.tagName.toLowerCase(),
        type: el.getAttribute('type') || undefined,
        role: el.getAttribute('role') || undefined,
        selector: cssSelectorFor(el),
        text: clampStr(String(rawTxt).replace(/\s+/g, ' ').trim(), 120),
        placeholder: el.getAttribute('placeholder') || undefined,
        value: ('value' in el && el.type !== 'password') ? clampStr(el.value, 120) : undefined,
        checked: (el.type === 'checkbox' || el.type === 'radio') ? !!el.checked : undefined,
        href: el.getAttribute('href') || undefined,
        disabled: !!el.disabled,
        rect: { x: Math.round(r.left), y: Math.round(r.top), w: Math.round(r.width), h: Math.round(r.height) }
      });
    }
    const headings = _ArrayFrom(document.querySelectorAll('h1, h2, h3'))
      .slice(0, 25)
      .map((h) => ({ level: h.tagName.toLowerCase(), text: clampStr((h.innerText || h.textContent || '').trim(), 140) }))
      .filter((h) => h.text.length > 0);
    const bodyText = clampStr((document.body ? (document.body.innerText || document.body.textContent || '') : '').replace(/\n{3,}/g, '\n\n').trim(), 3200);
    return {
      url: safeWellFormed(location.href || ''),
      title: safeWellFormed(document.title || ''),
      readyState: safeWellFormed(document.readyState || ''),
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

  const actOnDom = function(cmd) {
    try {
      const action = cmd && cmd.action;
      if (action === 'scroll') {
        const dx = Number.isFinite(Number(cmd.dx)) ? Number(cmd.dx) : 0;
        const dy = Number.isFinite(Number(cmd.dy)) ? Number(cmd.dy) : 400;
        window.scrollBy({ left: dx, top: dy, behavior: 'instant' });
        return { ok: true, scrollX: Math.round(window.scrollX), scrollY: Math.round(window.scrollY) };
      }
      if (action === 'back') {
        history.back();
        return { ok: true, action: 'back' };
      }
      if (action === 'forward') {
        history.forward();
        return { ok: true, action: 'forward' };
      }
      let el = null;
      let selectorErr = null;
      if (cmd && cmd.selector) {
        try {
          el = document.querySelector(cmd.selector);
        } catch (err) {
          selectorErr = err && err.message ? err.message : String(err);
        }
      }
      if (!el && cmd && cmd.text) {
        const needle = String(cmd.text).toLowerCase();
        const candidates = document.querySelectorAll('button, a, [role="button"], [role="link"], [role="tab"], [role="menuitem"], label, summary, input[type="submit"], input[type="button"]');
        for (const c of candidates) {
          const label = (c.innerText || c.textContent || c.value || c.getAttribute('aria-label') || '').toLowerCase();
          if (label.includes(needle)) {
            el = c;
            break;
          }
        }
      }
      if (!el) {
        return {
          ok: false,
          error: selectorErr
            ? `Invalid CSS selector (${selectorErr}) and no text match found`
            : 'Target element not found for selector/text'
        };
      }
      if (el.disabled) {
        return { ok: false, error: 'Target element is disabled', selector: cssSelectorFor(el) };
      }
      try {
        el.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'instant' });
      } catch (_) {}
      if (action === 'click') {
        if (typeof el.focus === 'function') el.focus();
        if (typeof el.click === 'function') {
          el.click();
        } else {
          el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, view: window }));
        }
        return { ok: true, selector: cssSelectorFor(el), url: safeWellFormed(location.href), title: safeWellFormed(document.title) };
      }
      if (action === 'fill') {
        if (typeof el.focus === 'function') el.focus();
        const strVal = String(cmd.value == null ? '' : cmd.value);
        if (el.isContentEditable) {
          el.textContent = strVal;
          el.dispatchEvent(new InputEvent('input', { bubbles: true, data: strVal }));
        } else if (el.tagName === 'SELECT') {
          let matched = strVal;
          const opts = _ArrayFrom(el.options || []);
          const byVal = opts.find((o) => o.value === strVal);
          const byTxt = !byVal && opts.find((o) => (o.text || '').trim().toLowerCase() === strVal.trim().toLowerCase());
          if (byTxt) matched = byTxt.value;
          setNativeValue(el, matched);
        } else if (el.type === 'checkbox' || el.type === 'radio') {
          const want = !['false', '0', 'off', 'no'].includes(strVal.trim().toLowerCase());
          if (el.checked !== want) el.click();
        } else {
          setNativeValue(el, strVal);
        }
        return { ok: true, selector: cssSelectorFor(el) };
      }
      if (action === 'select') {
        const strVal = String(cmd.value == null ? '' : cmd.value);
        let matched = strVal;
        if (el.tagName === 'SELECT') {
          const opts = _ArrayFrom(el.options || []);
          const byVal = opts.find((o) => o.value === strVal);
          const byTxt = !byVal && opts.find((o) => (o.text || '').trim().toLowerCase() === strVal.trim().toLowerCase());
          if (byTxt) matched = byTxt.value;
        }
        setNativeValue(el, matched);
        return { ok: true, selector: cssSelectorFor(el) };
      }
      if (action === 'press') {
        const key = String(cmd.key || 'Enter');
        if (typeof el.focus === 'function') el.focus();
        el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
        if (key === 'Enter' && el.form && typeof el.form.requestSubmit === 'function') {
          try { el.form.requestSubmit(); } catch (_) {}
        }
        el.dispatchEvent(new KeyboardEvent('keyup', { key, bubbles: true, cancelable: true }));
        return { ok: true, selector: cssSelectorFor(el), key };
      }
      if (action === 'hover') {
        el.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
        el.dispatchEvent(new MouseEvent('mouseenter', { bubbles: false }));
        return { ok: true, selector: cssSelectorFor(el) };
      }
      return { ok: false, error: 'Unsupported action: ' + action };
    } catch (err) {
      return { ok: false, error: formatError(err) };
    }
  };

  const safeEval = function(source) {
    try {
      const code = String(source == null ? '' : source);
      let out;
      try {
        out = (0, eval)('(\n' + code + '\n)');
      } catch (synErr) {
        if (synErr instanceof SyntaxError) {
          out = (0, eval)(code);
        } else {
          throw synErr;
        }
      }
      return _stringify({
        ok: true,
        result: safeSerializeValue(out, 6),
        url: safeWellFormed(location.href || ''),
        title: safeWellFormed(document.title || '')
      });
    } catch (err) {
      return _stringify({
        ok: false,
        error: formatError(err),
        url: safeWellFormed(location.href || ''),
        title: safeWellFormed(document.title || '')
      });
    }
  };

  for (const [name, fn] of [
    ['__kwrokSetInspectMode', setInspectMode],
    ['__kwrokSnapshotDom', snapshotDom],
    ['__kwrokSnapshotDomJson', (maxItems) => _stringify(snapshotDom(maxItems))],
    ['__kwrokAct', actOnDom],
    ['__kwrokActJson', (cmd) => _stringify(actOnDom(cmd))],
    ['__kwrokEval', safeEval],
  ]) {
    try {
      _defProp(window, name, { value: fn, configurable: false, enumerable: false, writable: false });
    } catch (_) {
      window[name] = fn;
    }
  }
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
        _nav_proxy: objc2::rc::Retained<macos_delegates::KwrokNavDelegateProxy>,
        #[cfg(target_os = "macos")]
        _ui_proxy: objc2::rc::Retained<macos_delegates::KwrokUIDelegateProxy>,
    }

    pub(crate) fn sanitize_url_for_load(raw: &str) -> Result<String, String> {
        let cleaned = strip_ascii_controls(raw);
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            return Err("网址不能为空".into());
        }
        if is_dangerous_script_scheme(trimmed) {
            return Err("出于安全保护，已拦截脚本协议地址".into());
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("about:") || lower.starts_with("data:") || lower.starts_with("blob:") {
            return Ok(encode_rfc3986_uri_preserving_escapes(trimmed));
        }
        let parsed = url::Url::parse(trimmed)
            .map_err(|e| format!("无效的网址格式 ({trimmed})：{e}"))?;
        match parsed.scheme() {
            "http" | "https" => {
                if parsed.host_str().is_none_or(str::is_empty) {
                    return Err(format!("无效的服务器域名 ({trimmed})"));
                }
                Ok(parsed.into())
            }
            "file" => Ok(parsed.into()),
            other => Err(format!("不支持的网址协议：{other}:")),
        }
    }

    pub(crate) fn default_downloads_dir() -> PathBuf {
        let override_dir = super::DOWNLOAD_DIR_OVERRIDE
            .lock()
            .ok()
            .and_then(|g| g.clone());
        let base = override_dir
            .or_else(|| std::env::var_os("KWORK_DOWNLOAD_DIR").map(PathBuf::from))
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|h| h.join("Downloads"))
                    .filter(|d| d.is_dir())
            })
            .unwrap_or_else(std::env::temp_dir);
        let _ = std::fs::create_dir_all(&base);
        base.canonicalize().unwrap_or(base)
    }

    impl BrowserWebView {
        pub fn build(window: &impl HasWindowHandle, init: WebViewInit) -> Result<Self, String> {
            let cb_nav = init.on_event.clone();
            let cb_ui = init.on_event.clone();
            let cb_title = init.on_event.clone();
            let cb_load = init.on_event.clone();
            let cb_new_win = init.on_event.clone();
            let cb_dl_start = init.on_event.clone();
            let cb_dl_done = init.on_event.clone();
            let cb_ipc = init.on_event;

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
                    if !url.is_empty()
                        && !url.starts_with("about:blank")
                        && !is_dangerous_script_scheme(&url)
                    {
                        cb_new_win(WebViewEvent::NewWindowRequested(url));
                    }
                    wry::NewWindowResponse::Deny
                })
                .with_download_started_handler(move |url, dest| {
                    let dl_dir = default_downloads_dir();
                    let _ = std::fs::create_dir_all(&dl_dir);
                    let suggested = dest
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("download.bin");
                    let target = unique_download_destination(&dl_dir, suggested);
                    let file_name = target
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("download.bin")
                        .to_owned();
                    *dest = target;
                    cb_dl_start(WebViewEvent::DownloadStarted { url, file_name });
                    true
                })
                .with_download_completed_handler(move |url, path, success| {
                    cb_dl_done(WebViewEvent::DownloadCompleted { url, path, success });
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
                    if is_dangerous_script_scheme(&url) {
                        return false;
                    }
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
            let nav_proxy = macos_delegates::install_navigation_proxy(&inner, cb_nav);
            #[cfg(target_os = "macos")]
            let ui_proxy = macos_delegates::install_ui_proxy(&inner, cb_ui);

            let view = Self {
                inner,
                #[cfg(target_os = "macos")]
                _nav_proxy: nav_proxy,
                #[cfg(target_os = "macos")]
                _ui_proxy: ui_proxy,
            };

            if let Some(url) = pending_initial_url {
                view.load_url(&url)?;
            }

            Ok(view)
        }

        pub fn load_url(&self, url: &str) -> Result<(), String> {
            let valid = sanitize_url_for_load(url)?;
            #[cfg(target_os = "macos")]
            {
                macos_delegates::safe_load_url(&self.inner, &valid)
            }
            #[cfg(not(target_os = "macos"))]
            {
                self.inner.load_url(&valid).map_err(|e| e.to_string())
            }
        }

        pub fn load_html(&self, html: &str) -> Result<(), String> {
            self.inner.load_html(html).map_err(|e| e.to_string())
        }

        pub fn reload(&self) -> Result<(), String> {
            self.inner.reload().map_err(|e| e.to_string())
        }

        pub fn stop_loading(&self) {
            #[cfg(target_os = "macos")]
            {
                use wry::WebViewExtMacOS;
                unsafe {
                    self.inner.webview().stopLoading();
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = self.eval_fire_and_forget("window.stop && window.stop();");
            }
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
            #[cfg(target_os = "macos")]
            {
                macos_delegates::safe_set_bounds(&self.inner, rect);
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = self.inner.set_bounds(to_wry_rect(rect));
            }
        }

        pub fn set_visible(&self, visible: bool) {
            let _ = self.inner.set_visible(visible);
        }

        pub fn set_zoom(&self, factor: f64) {
            let _ = self.inner.zoom(factor.clamp(0.25, 3.0));
        }

        pub fn is_focused(&self) -> bool {
            #[cfg(target_os = "macos")]
            {
                macos_delegates::is_webview_focused(&self.inner)
            }
            #[cfg(not(target_os = "macos"))]
            {
                false
            }
        }

        pub fn focus(&self) {
            #[cfg(target_os = "macos")]
            {
                macos_delegates::safe_focus(&self.inner);
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = self.inner.focus();
            }
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
            #[cfg(target_os = "macos")]
            {
                macos_delegates::safe_eval_with_callback(&self.inner, js, callback);
                Ok(())
            }
            #[cfg(not(target_os = "macos"))]
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
        }

        pub fn eval_fire_and_forget(&self, js: &str) -> Result<(), String> {
            #[cfg(target_os = "macos")]
            {
                macos_delegates::safe_eval_fire_and_forget(&self.inner, js);
                Ok(())
            }
            #[cfg(not(target_os = "macos"))]
            {
                self.inner.evaluate_script(js).map_err(|e| e.to_string())
            }
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

    impl Drop for BrowserWebView {
        fn drop(&mut self) {
            #[cfg(target_os = "macos")]
            {
                macos_delegates::teardown_before_drop(&self.inner);
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
    mod macos_delegates {
        use std::cell::{Cell, RefCell};
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::time::{Instant, SystemTime, UNIX_EPOCH};

        use super::{
            BrowserIpcEvent, ConsoleLevel, LogicalRect, WebViewEvent, WebViewEventCallback,
            default_downloads_dir, unique_download_destination,
        };
        use block2::RcBlock;
        use objc2::{
            AnyThread, DeclaredClass, MainThreadOnly, define_class, msg_send,
            rc::Retained,
            runtime::{AnyObject, Bool, NSObject, ProtocolObject},
            sel,
        };
        use objc2_app_kit::NSView;
        use objc2_foundation::{
            MainThreadMarker, NSArray, NSData, NSError, NSHTTPURLResponse, NSJSONSerialization,
            NSJSONWritingOptions, NSNumber, NSObjectNSKeyValueCoding, NSObjectProtocol, NSPoint,
            NSRect, NSSize, NSString, NSURL, NSURLRequest, NSURLResponse, NSUTF8StringEncoding,
            ns_string,
        };
        use objc2_web_kit::{
            WKDownload, WKDownloadDelegate, WKDownloadPlaceholderPolicy, WKFrameInfo,
            WKMediaCaptureType, WKNavigation, WKNavigationAction, WKNavigationActionPolicy,
            WKNavigationDelegate, WKNavigationResponse, WKNavigationResponsePolicy,
            WKOpenPanelParameters, WKPermissionDecision, WKSecurityOrigin, WKUIDelegate, WKWebView,
            WKWebViewConfiguration, WKWindowFeatures,
        };
        use wry::{WebView, WebViewExtMacOS};

        pub struct KwrokDownloadDelegateIvars {
            on_event: WebViewEventCallback,
            active_paths: RefCell<HashMap<usize, PathBuf>>,
        }

        pub struct KwrokNavDelegateIvars {
            orig: Option<Retained<ProtocolObject<dyn WKNavigationDelegate>>>,
            download_delegate: Retained<KwrokDownloadDelegate>,
            on_event: WebViewEventCallback,
        }

        pub struct KwrokUIDelegateIvars {
            orig: Option<Retained<ProtocolObject<dyn WKUIDelegate>>>,
            on_event: WebViewEventCallback,
            popup_window_start: Cell<Option<Instant>>,
            popups_in_window: Cell<u32>,
        }

        pub(crate) fn is_supported_web_scheme(url: &str) -> bool {
            let lower = url.trim_start().to_ascii_lowercase();
            lower.starts_with("http://")
                || lower.starts_with("https://")
                || lower.starts_with("file://")
                || lower.starts_with("about:")
                || lower.starts_with("data:")
                || lower.starts_with("blob:")
        }

        fn emit_history_state(webview: &WKWebView, on_event: &WebViewEventCallback) {
            let url = unsafe {
                webview
                    .URL()
                    .and_then(|u| u.absoluteString())
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            };
            let can_go_back = unsafe { webview.canGoBack() };
            let can_go_forward = unsafe { webview.canGoForward() };
            on_event(WebViewEvent::HistoryStateChanged {
                url,
                can_go_back,
                can_go_forward,
            });
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

            // Never fail the main tab when a portal probes a native app deep-link scheme (`bilibili://`, `baiduboxapp://`)
            // or when WebKit cancels an obsolete/superseded/downloaded navigation (`-999`, `102`, `204`, `101` on external schemes).
            if (!url.is_empty() && !is_supported_web_scheme(&url))
                || matches!(code, -999 | 102 | 204)
                || (code == 101 && !is_supported_web_scheme(&url))
            {
                let still_loading = unsafe { webview.isLoading() };
                if !still_loading {
                    let cur_url = unsafe {
                        webview
                            .URL()
                            .and_then(|u| u.absoluteString())
                            .map(|s| s.to_string())
                            .unwrap_or_default()
                    };
                    (ivars.on_event)(WebViewEvent::LoadFinished(cur_url));
                    emit_history_state(webview, &ivars.on_event);
                }
                return;
            }
            let message = error.localizedDescription().to_string();
            (ivars.on_event)(WebViewEvent::LoadFailed { url, code, message });
        }

        fn is_attachment_response(response: &NSURLResponse) -> bool {
            let Some(http_resp) = response.downcast_ref::<NSHTTPURLResponse>() else {
                return false;
            };
            let headers = http_resp.allHeaderFields();
            headers
                .valueForKey(ns_string!("Content-Disposition"))
                .and_then(|val| val.downcast::<NSString>().ok())
                .is_some_and(|disp| {
                    disp.to_string()
                        .split(';')
                        .next()
                        .is_some_and(|k| k.trim().eq_ignore_ascii_case("attachment"))
                })
        }

        fn download_request_url(download: &WKDownload) -> String {
            unsafe {
                download
                    .originalRequest()
                    .and_then(|r| r.URL())
                    .and_then(|u| u.absoluteString())
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            }
        }

        define_class!(
            #[unsafe(super(NSObject))]
            #[thread_kind = MainThreadOnly]
            #[ivars = KwrokDownloadDelegateIvars]
            pub struct KwrokDownloadDelegate;

            unsafe impl NSObjectProtocol for KwrokDownloadDelegate {}

            unsafe impl WKDownloadDelegate for KwrokDownloadDelegate {
                #[unsafe(method(download:decidePlaceholderPolicy:))]
                fn decide_placeholder_policy(
                    &self,
                    _download: &WKDownload,
                    completion_handler: &block2::DynBlock<
                        dyn Fn(WKDownloadPlaceholderPolicy, *mut NSURL),
                    >,
                ) {
                    completion_handler
                        .call((WKDownloadPlaceholderPolicy::Disable, std::ptr::null_mut()));
                }

                #[unsafe(method(download:decideDestinationUsingResponse:suggestedFilename:completionHandler:))]
                fn decide_destination(
                    &self,
                    download: &WKDownload,
                    _response: &NSURLResponse,
                    suggested_filename: &NSString,
                    completion_handler: &block2::DynBlock<dyn Fn(*mut NSURL)>,
                ) {
                    let url = download_request_url(download);
                    let dl_dir = default_downloads_dir();
                    let target = unique_download_destination(&dl_dir, &suggested_filename.to_string());
                    let file_name = target
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("download.bin")
                        .to_owned();
                    let key = (download as *const WKDownload) as usize;
                    self.ivars()
                        .active_paths
                        .borrow_mut()
                        .insert(key, target.clone());
                    (self.ivars().on_event)(WebViewEvent::DownloadStarted { url, file_name });
                    let path_ns = NSString::from_str(&target.to_string_lossy());
                    let ns_url = NSURL::fileURLWithPath_isDirectory(&path_ns, false);
                    completion_handler.call((Retained::as_ptr(&ns_url).cast_mut(),));
                    drop(ns_url);
                }

                #[unsafe(method(downloadDidFinish:))]
                fn did_finish(&self, download: &WKDownload) {
                    let url = download_request_url(download);
                    let key = (download as *const WKDownload) as usize;
                    let path = self.ivars().active_paths.borrow_mut().remove(&key);
                    (self.ivars().on_event)(WebViewEvent::DownloadCompleted {
                        url,
                        path,
                        success: true,
                    });
                }

                #[unsafe(method(download:didFailWithError:resumeData:))]
                fn did_fail(
                    &self,
                    download: &WKDownload,
                    _error: &NSError,
                    _resume_data: Option<&NSData>,
                ) {
                    let url = download_request_url(download);
                    let key = (download as *const WKDownload) as usize;
                    let path = self.ivars().active_paths.borrow_mut().remove(&key);
                    (self.ivars().on_event)(WebViewEvent::DownloadCompleted {
                        url,
                        path,
                        success: false,
                    });
                }
            }
        );

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

                    let lower = url_str.trim_start().to_ascii_lowercase();
                    if !is_supported_web_scheme(&lower) && !lower.starts_with("javascript:") {
                        handler.call((WKNavigationActionPolicy::Cancel,));
                        return;
                    }

                    let should_download = unsafe {
                        action.respondsToSelector(sel!(shouldPerformDownload))
                            && action.shouldPerformDownload()
                    };
                    if should_download {
                        handler.call((WKNavigationActionPolicy::Download,));
                        return;
                    }

                    let is_main_frame = unsafe { action.targetFrame() }
                        .is_some_and(|frame| unsafe { frame.isMainFrame() });
                    if is_main_frame
                        && !url_str.starts_with("about:blank")
                        && !lower.starts_with("javascript:")
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
                    _webview: &WKWebView,
                    response: &WKNavigationResponse,
                    handler: &block2::DynBlock<dyn Fn(WKNavigationResponsePolicy)>,
                ) {
                    let can_show = unsafe { response.canShowMIMEType() };
                    let ns_resp = unsafe { response.response() };
                    if !can_show || is_attachment_response(&ns_resp) {
                        handler.call((WKNavigationResponsePolicy::Download,));
                        return;
                    }
                    handler.call((WKNavigationResponsePolicy::Allow,));
                }

                #[unsafe(method(webView:didCommitNavigation:))]
                fn did_commit(&self, webview: &WKWebView, navigation: Option<&WKNavigation>) {
                    if let Some(ref orig) = self.ivars().orig {
                        unsafe {
                            orig.webView_didCommitNavigation(webview, navigation);
                        }
                    }
                    emit_history_state(webview, &self.ivars().on_event);
                }

                #[unsafe(method(webView:didFinishNavigation:))]
                fn did_finish(&self, webview: &WKWebView, navigation: Option<&WKNavigation>) {
                    if let Some(ref orig) = self.ivars().orig {
                        unsafe {
                            orig.webView_didFinishNavigation(webview, navigation);
                        }
                    }
                    emit_history_state(webview, &self.ivars().on_event);
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
                    _action: &WKNavigationAction,
                    download: &WKDownload,
                ) {
                    unsafe {
                        download.setDelegate(Some(ProtocolObject::from_ref(
                            &*self.ivars().download_delegate,
                        )));
                    }
                    emit_history_state(webview, &self.ivars().on_event);
                }

                #[unsafe(method(webView:navigationResponse:didBecomeDownload:))]
                fn response_download(
                    &self,
                    webview: &WKWebView,
                    _response: &WKNavigationResponse,
                    download: &WKDownload,
                ) {
                    unsafe {
                        download.setDelegate(Some(ProtocolObject::from_ref(
                            &*self.ivars().download_delegate,
                        )));
                    }
                    emit_history_state(webview, &self.ivars().on_event);
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

        define_class!(
            #[unsafe(super(NSObject))]
            #[thread_kind = MainThreadOnly]
            #[ivars = KwrokUIDelegateIvars]
            pub struct KwrokUIDelegateProxy;

            unsafe impl NSObjectProtocol for KwrokUIDelegateProxy {}

            unsafe impl WKUIDelegate for KwrokUIDelegateProxy {
                #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
                fn create_webview(
                    &self,
                    _webview: &WKWebView,
                    _configuration: &WKWebViewConfiguration,
                    action: &WKNavigationAction,
                    _window_features: &WKWindowFeatures,
                ) -> Option<Retained<WKWebView>> {
                    // Safely extract target URL without wry's `.unwrap()` calls on `URL()`, `window()`, or `screen()`.
                    let url = unsafe {
                        action
                            .request()
                            .URL()
                            .and_then(|u| u.absoluteString())
                            .map(|s| s.to_string())
                    };
                    if let Some(url_str) = url
                        && !url_str.is_empty()
                        && !url_str.starts_with("about:blank")
                        && is_supported_web_scheme(&url_str)
                    {
                        let now = Instant::now();
                        let reset = self
                            .ivars()
                            .popup_window_start
                            .get()
                            .is_none_or(|t| now.duration_since(t).as_secs_f32() > 2.0);
                        if reset {
                            self.ivars().popup_window_start.set(Some(now));
                            self.ivars().popups_in_window.set(0);
                        }
                        let count = self.ivars().popups_in_window.get();
                        if count < 4 {
                            self.ivars().popups_in_window.set(count + 1);
                            (self.ivars().on_event)(WebViewEvent::NewWindowRequested(url_str));
                        }
                    }
                    None
                }

                #[unsafe(method(webViewDidClose:))]
                fn did_close(&self, _webview: &WKWebView) {
                    (self.ivars().on_event)(WebViewEvent::CloseRequested);
                }

                #[unsafe(method(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:))]
                fn run_alert(
                    &self,
                    _webview: &WKWebView,
                    message: &NSString,
                    _frame: &WKFrameInfo,
                    completion_handler: &block2::DynBlock<dyn Fn()>,
                ) {
                    let ts = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |d| d.as_millis() as u64);
                    (self.ivars().on_event)(WebViewEvent::Ipc(BrowserIpcEvent::Console {
                        level: ConsoleLevel::Info,
                        message: format!("[alert] {message}"),
                        source: "window.alert".into(),
                        timestamp_ms: ts,
                    }));
                    completion_handler.call(());
                }

                #[unsafe(method(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:))]
                fn run_confirm(
                    &self,
                    _webview: &WKWebView,
                    message: &NSString,
                    _frame: &WKFrameInfo,
                    completion_handler: &block2::DynBlock<dyn Fn(Bool)>,
                ) {
                    let ts = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |d| d.as_millis() as u64);
                    (self.ivars().on_event)(WebViewEvent::Ipc(BrowserIpcEvent::Console {
                        level: ConsoleLevel::Info,
                        message: format!("[confirm → true] {message}"),
                        source: "window.confirm".into(),
                        timestamp_ms: ts,
                    }));
                    completion_handler.call((Bool::YES,));
                }

                #[unsafe(method(webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:))]
                fn run_prompt(
                    &self,
                    _webview: &WKWebView,
                    prompt: &NSString,
                    default_text: Option<&NSString>,
                    _frame: &WKFrameInfo,
                    completion_handler: &block2::DynBlock<dyn Fn(*mut NSString)>,
                ) {
                    let ts = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |d| d.as_millis() as u64);
                    (self.ivars().on_event)(WebViewEvent::Ipc(BrowserIpcEvent::Console {
                        level: ConsoleLevel::Info,
                        message: format!("[prompt] {prompt}"),
                        source: "window.prompt".into(),
                        timestamp_ms: ts,
                    }));
                    let ptr = default_text.map_or(std::ptr::null_mut(), |s| {
                        (s as *const NSString).cast_mut()
                    });
                    completion_handler.call((ptr,));
                }

                #[unsafe(method(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:))]
                fn request_media_permission(
                    &self,
                    webview: &WKWebView,
                    origin: &WKSecurityOrigin,
                    frame: &WKFrameInfo,
                    capture_type: WKMediaCaptureType,
                    decision_handler: &block2::DynBlock<dyn Fn(WKPermissionDecision)>,
                ) {
                    if let Some(ref orig) = self.ivars().orig {
                        unsafe {
                            orig.webView_requestMediaCapturePermissionForOrigin_initiatedByFrame_type_decisionHandler(
                                webview, origin, frame, capture_type, decision_handler,
                            );
                        }
                    } else {
                        decision_handler.call((WKPermissionDecision::Prompt,));
                    }
                }

                #[unsafe(method(webView:runOpenPanelWithParameters:initiatedByFrame:completionHandler:))]
                fn run_open_panel(
                    &self,
                    webview: &WKWebView,
                    parameters: &WKOpenPanelParameters,
                    frame: &WKFrameInfo,
                    completion_handler: &block2::DynBlock<dyn Fn(*mut NSArray<NSURL>)>,
                ) {
                    if let Some(ref orig) = self.ivars().orig {
                        unsafe {
                            orig.webView_runOpenPanelWithParameters_initiatedByFrame_completionHandler(
                                webview, parameters, frame, completion_handler,
                            );
                        }
                    } else {
                        completion_handler.call((std::ptr::null_mut(),));
                    }
                }
            }
        );

        pub fn install_navigation_proxy(
            webview: &WebView,
            on_event: WebViewEventCallback,
        ) -> Retained<KwrokNavDelegateProxy> {
            let wk = webview.webview();
            let mtm = MainThreadMarker::new().expect("WKWebView must be created on the main thread");
            unsafe {
                // Allow local `file://` bundles (e.g. `dist/index.html` with `<script type="module">` or
                // `fetch('./data.json')`) to read sibling assets inside the directory granted to
                // `loadFileURL_allowingReadAccessToURL`.
                let prefs = wk.configuration().preferences();
                let yes = NSNumber::numberWithBool(true);
                prefs.setValue_forKey(Some(&yes), ns_string!("allowFileAccessFromFileURLs"));
            }
            let dl_alloc = mtm
                .alloc::<KwrokDownloadDelegate>()
                .set_ivars(KwrokDownloadDelegateIvars {
                    on_event: on_event.clone(),
                    active_paths: RefCell::new(HashMap::new()),
                });
            let download_delegate: Retained<KwrokDownloadDelegate> =
                unsafe { msg_send![super(dl_alloc), init] };
            let orig = unsafe { wk.navigationDelegate() };
            let alloc = mtm
                .alloc::<KwrokNavDelegateProxy>()
                .set_ivars(KwrokNavDelegateIvars {
                    orig,
                    download_delegate,
                    on_event,
                });
            let proxy: Retained<KwrokNavDelegateProxy> = unsafe { msg_send![super(alloc), init] };
            unsafe {
                wk.setNavigationDelegate(Some(ProtocolObject::from_ref(&*proxy)));
            }
            proxy
        }

        pub fn install_ui_proxy(
            webview: &WebView,
            on_event: WebViewEventCallback,
        ) -> Retained<KwrokUIDelegateProxy> {
            let wk = webview.webview();
            let mtm = MainThreadMarker::new().expect("WKWebView must be created on the main thread");
            let orig = unsafe { wk.UIDelegate() };
            let alloc = mtm
                .alloc::<KwrokUIDelegateProxy>()
                .set_ivars(KwrokUIDelegateIvars {
                    orig,
                    on_event,
                    popup_window_start: Cell::new(None),
                    popups_in_window: Cell::new(0),
                });
            let proxy: Retained<KwrokUIDelegateProxy> = unsafe { msg_send![super(alloc), init] };
            unsafe {
                wk.setUIDelegate(Some(ProtocolObject::from_ref(&*proxy)));
            }
            proxy
        }

        pub fn safe_load_url(webview: &WebView, url: &str) -> Result<(), String> {
            let wk = webview.webview();
            if url.starts_with("file://")
                && let Ok(parsed) = url::Url::parse(url)
                && let Ok(file_path) = parsed.to_file_path()
            {
                let file_str = file_path.to_string_lossy();
                let read_dir = file_path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("/"))
                    .to_string_lossy();
                let file_ns = NSURL::fileURLWithPath(&NSString::from_str(&file_str));
                let dir_ns = NSURL::fileURLWithPath(&NSString::from_str(&read_dir));
                unsafe {
                    wk.loadFileURL_allowingReadAccessToURL(&file_ns, &dir_ns);
                }
                return Ok(());
            }
            let ns_str = NSString::from_str(url);
            let Some(ns_url) = NSURL::URLWithString(&ns_str) else {
                return Err(format!("系统无法解析该网址：{url}"));
            };
            let req = NSURLRequest::requestWithURL(&ns_url);
            unsafe {
                wk.loadRequest(&req);
            }
            Ok(())
        }

        pub fn safe_set_bounds(webview: &WebView, rect: LogicalRect) {
            let wk = webview.webview();
            let Some(parent) = (unsafe { wk.superview() }) else {
                return;
            };
            let x = f64::from(rect.x);
            let y = f64::from(rect.y);
            let w = f64::from(rect.width.max(1.0));
            let h = f64::from(rect.height.max(1.0));
            let is_flipped = parent.isFlipped();
            let origin_y = if is_flipped {
                y
            } else {
                parent.frame().size.height - y - h
            };
            let frame = NSRect::new(NSPoint::new(x, origin_y), NSSize::new(w, h));
            wk.setFrame(frame);
        }

        pub fn is_webview_focused(webview: &WebView) -> bool {
            let wk = webview.webview();
            let Some(window) = wk.window() else {
                return false;
            };
            let Some(resp) = window.firstResponder() else {
                return false;
            };
            let wk_view: &NSView = &wk;
            let resp_ptr: *const objc2_app_kit::NSResponder = &*resp;
            let wk_ptr: *const NSView = wk_view;
            if resp_ptr.cast::<()>() == wk_ptr.cast::<()>() {
                return true;
            }
            if let Some(resp_view) = resp.downcast_ref::<NSView>() {
                return resp_view.isDescendantOf(wk_view);
            }
            false
        }

        pub fn safe_focus(webview: &WebView) {
            let wk = webview.webview();
            if let Some(window) = wk.window() {
                window.makeFirstResponder(Some(&wk));
            }
        }

        /// Evaluate JavaScript directly on `WKWebView`, bypassing `wry`'s `pending_scripts` callback drop
        /// and `NSJSONSerialization::dataWithJSONObject(...).unwrap()` crash on `NaN`/`Infinity`/`Date`.
        pub fn safe_eval_with_callback<F>(webview: &WebView, js: &str, callback: F)
        where
            F: FnOnce(String) + Send + 'static,
        {
            let wk = webview.webview();
            let slot = std::sync::Mutex::new(Some(callback));
            let handler = RcBlock::new(move |val: *mut AnyObject, err: *mut NSError| {
                let Some(done) = slot.lock().ok().and_then(|mut g| g.take()) else {
                    return;
                };
                if !err.is_null() {
                    let msg = unsafe { (*err).localizedDescription().to_string() };
                    let err_json = serde_json::json!({
                        "ok": false,
                        "error": msg,
                    })
                    .to_string();
                    done(err_json);
                    return;
                }
                if val.is_null() {
                    done("null".into());
                    return;
                }
                let serialized = unsafe {
                    let obj = &*val;
                    if let Some(ns_str) = obj.downcast_ref::<NSString>() {
                        serde_json::to_string(&ns_str.to_string())
                            .unwrap_or_else(|_| "null".into())
                    } else if let Ok(data) = NSJSONSerialization::dataWithJSONObject_options_error(
                        obj,
                        NSJSONWritingOptions::FragmentsAllowed,
                    ) && let Some(json_ns) =
                        NSString::initWithData_encoding(NSString::alloc(), &data, NSUTF8StringEncoding)
                    {
                        json_ns.to_string()
                    } else {
                        let desc: Retained<NSString> = msg_send![obj, description];
                        serde_json::to_string(&desc.to_string()).unwrap_or_else(|_| "null".into())
                    }
                };
                done(serialized);
            });
            unsafe {
                wk.evaluateJavaScript_completionHandler(&NSString::from_str(js), Some(&handler));
            }
        }

        pub fn safe_eval_fire_and_forget(webview: &WebView, js: &str) {
            let wk = webview.webview();
            unsafe {
                wk.evaluateJavaScript_completionHandler(&NSString::from_str(js), None);
            }
        }

        pub fn teardown_before_drop(webview: &WebView) {
            if is_webview_focused(webview) {
                let _ = webview.focus_parent();
            }
            let wk = webview.webview();
            wk.setHidden(true);
            unsafe {
                wk.stopLoading();
                wk.setNavigationDelegate(None);
                wk.setUIDelegate(None);
                wk.loadHTMLString_baseURL(ns_string!(""), None);
            }
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

    pub(crate) fn sanitize_url_for_load(raw: &str) -> Result<String, String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || is_dangerous_script_scheme(trimmed) {
            return Err("无效的网址".into());
        }
        Ok(trimmed.to_owned())
    }

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
            let valid = sanitize_url_for_load(url)?;
            if let Ok(mut g) = self.url.lock() {
                *g = Some(valid);
            }
            Ok(())
        }

        pub fn load_html(&self, _html: &str) -> Result<(), String> {
            Ok(())
        }

        pub fn reload(&self) -> Result<(), String> {
            Ok(())
        }

        pub fn stop_loading(&self) {}

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
        pub fn is_focused(&self) -> bool {
            false
        }
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
            normalize_omnibox_input("localhost：5173/看板？tab=1", None).as_deref(),
            Some("http://localhost:5173/%E7%9C%8B%E6%9D%BF?tab=1")
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
            normalize_omnibox_input("[::1]:8080/health", None).as_deref(),
            Some("http://[::1]:8080/health")
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
            normalize_omnibox_input("baike。baidu。com/item/人工智能", None).as_deref(),
            Some("https://baike.baidu.com/item/%E4%BA%BA%E5%B7%A5%E6%99%BA%E8%83%BD")
        );
        assert_eq!(
            normalize_omnibox_input("https://baike.baidu.com/item/人工智能", None).as_deref(),
            Some("https://baike.baidu.com/item/%E4%BA%BA%E5%B7%A5%E6%99%BA%E8%83%BD")
        );
        assert_eq!(
            normalize_omnibox_input("gpui rust layout guide", None).as_deref(),
            Some("https://cn.bing.com/search?q=gpui+rust+layout+guide")
        );
        // Natural Chinese sentences with `。` or `：` must NOT be mangled as hostnames!
        assert_eq!(
            normalize_omnibox_input("你好。今天天气怎么样？", None).as_deref(),
            Some("https://cn.bing.com/search?q=%E4%BD%A0%E5%A5%BD%E3%80%82%E4%BB%8A%E5%A4%A9%E5%A4%A9%E6%B0%94%E6%80%8E%E4%B9%88%E6%A0%B7%EF%BC%9F")
        );
        assert_eq!(
            normalize_omnibox_input("百度热搜", None).as_deref(),
            Some("https://cn.bing.com/search?q=%E7%99%BE%E5%BA%A6%E7%83%AD%E6%90%9C")
        );
        assert!(normalize_omnibox_input("   ", None).is_none());

        let tmp = std::env::temp_dir().join("kwrok_omnibox_test.html");
        std::fs::write(&tmp, "<!doctype html><title>ok</title>").unwrap();
        let file_url = normalize_omnibox_input("kwrok_omnibox_test.html", tmp.parent()).unwrap();
        assert!(file_url.starts_with("file:///"), "{file_url}");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn adversarial_omnibox_and_url_sanitizer_fuzz_matrix() {
        // 1. Script execution schemes (including embedded control-char bypasses) MUST be blocked!
        for dangerous in [
            "javascript:alert(1)",
            "  JaVaScRiPt:alert(document.cookie) ",
            "java\nscript:alert(1)",
            "java\tscript:fetch('https://evil.test')",
            "javascript：alert(1)",
            "vbscript:msgbox(1)",
            "VBSCRIPT:exec()",
        ] {
            assert_eq!(
                normalize_omnibox_input(dangerous, None),
                None,
                "must reject dangerous input: {dangerous:?}"
            );
            assert!(
                native::sanitize_url_for_load(dangerous).is_err(),
                "sanitizer must reject: {dangerous:?}"
            );
        }

        // 2. Malformed scheme URLs must NOT slip through as broken NSURLs
        for malformed in [
            "http://",
            "https://",
            "http://[invalid-ipv6",
            "https://exa mple.com",
            ":0",
            ":99999",
            "about:",
            "about: blank",
        ] {
            assert_eq!(
                normalize_omnibox_input(malformed, None),
                None,
                "malformed URL should return None: {malformed:?}"
            );
        }

        // 3. `data:` URLs with `<`, `>`, spaces, `%`, and Chinese UTF-8 must encode cleanly for NSURL
        let raw_data = "data:text/html;charset=utf-8,<h1>你好 世界 100%</h1>";
        let encoded = normalize_omnibox_input(raw_data, None).expect("data URL should normalize");
        assert!(encoded.starts_with("data:text/html;charset=utf-8,"));
        assert!(!encoded.contains(' ') && !encoded.contains('<') && !encoded.contains('>'));
        assert!(encoded.contains("%25")); // bare `%` encoded safely as `%25`
        #[cfg(target_os = "macos")]
        {
            use objc2_foundation::{NSString, NSURL};
            let ns = NSURL::URLWithString(&NSString::from_str(&encoded));
            assert!(ns.is_some(), "Foundation NSURL must accept encoded data URL: {encoded}");
        }

        // 4. Huge 50KB paste into omnibox must clamp safely on UTF-8 boundary without panicking
        let huge = "测试🔥".repeat(12_000);
        let search_url = normalize_omnibox_input(&huge, None).expect("clamped search URL");
        assert!(search_url.starts_with("https://cn.bing.com/search?q="));
        assert!(search_url.len() < 16_384);

        // 5. Download filename path-traversal & dotfile sanitization
        assert_eq!(
            sanitize_download_filename("../../.ssh/authorized_keys"),
            "authorized_keys"
        );
        assert_eq!(
            sanitize_download_filename("..\\..\\Windows\\System32\\cmd.exe"),
            "cmd.exe"
        );
        assert_eq!(sanitize_download_filename(".zshrc"), "zshrc");
        assert_eq!(sanitize_download_filename("   \0\n  "), "download.bin");
        assert_eq!(
            sanitize_download_filename("季度报告 2026.docx"),
            "季度报告 2026.docx"
        );
        assert_eq!(
            normalize_omnibox_input("256.256.256.256", None).as_deref(),
            Some("https://cn.bing.com/search?q=256.256.256.256")
        );
    }

    #[test]
    fn ipc_events_deserialize_state_console_and_picked_elements() {
        let state: BrowserIpcEvent = serde_json::from_str(
            r#"{"kind":"state","url":"http://localhost:5173/","title":"App","ready_state":"complete","can_go_back":true,"can_go_forward":false}"#,
        )
        .unwrap();
        assert!(matches!(
            state,
            BrowserIpcEvent::State {
                can_go_back: true,
                ..
            }
        ));

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
