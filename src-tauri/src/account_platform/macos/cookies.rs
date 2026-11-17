//! Scope deployment cookies to a single Platform WebKit view and origin.

use block2::RcBlock;
use objc2::{MainThreadMarker, rc::Retained};
use objc2_foundation::{
    NSDictionary, NSError, NSHTTPCookie, NSHTTPCookieDiscard, NSHTTPCookiePort,
    NSHTTPCookieVersion, NSMutableCopying, NSString, NSURL, ns_string,
};
use objc2_web_kit::{WKContentRuleList, WKContentRuleListStore, WKWebView, WKWebViewConfiguration};
use serde_json::json;
use std::{cell::Cell, time::Duration};
use tauri::{Runtime, Webview, http::HeaderName};
use tokio::sync::oneshot;
use url::Url;

const ERROR: &str = "未能配置平台页面的部署 Cookie。";

pub(in crate::account_platform) async fn configure_cookies<R: Runtime>(
    view: &Webview<R>,
    address: Url,
    header: String,
) -> Result<(), String> {
    let merged = super::super::cookies::merge("", &header);
    if merged.is_empty() {
        return Ok(());
    }
    let port = address.port_or_known_default().ok_or(ERROR)?;
    let rules = content_rules(&address, port).ok_or(ERROR)?;
    let directory = tempfile::Builder::new()
        .prefix("kwork-platform-rules-")
        .tempdir()
        .map_err(|_| ERROR)?;
    let (ready, done) = oneshot::channel();
    view.with_webview(move |view| {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(directory_path) = directory.path().to_str() else {
            return;
        };
        let directory_url =
            NSURL::fileURLWithPath_isDirectory(&NSString::from_str(directory_path), true);
        // SAFETY: The directory is a live local file URL and WebKit runs on its main thread.
        let Some(store) =
            (unsafe { WKContentRuleListStore::storeWithURL(Some(&directory_url), mtm) })
        else {
            return;
        };
        // SAFETY: Tauri exposes the live WKWebView on its owning main thread.
        let webview: &WKWebView = unsafe { &*view.inner().cast() };
        // SAFETY: The configuration is retrieved and used on the main thread.
        let configuration = unsafe { webview.configuration() };
        let ready = Cell::new(Some(ready));
        let complete = RcBlock::new(move |rules: *mut WKContentRuleList, _: *mut NSError| {
            // Keep the compiled file alive until WebKit maps and installs the rules.
            let _directory = &directory;
            let Some(ready) = ready.take().filter(|sender| !sender.is_closed()) else {
                return;
            };
            // SAFETY: A non-null rule list is retained by WebKit for this main-thread callback.
            if let Some(rules) = unsafe { rules.as_ref() } {
                install(&configuration, rules, &address, port, &merged, ready);
            }
        });
        // SAFETY: The store copies the completion block. Arguments are used on the main thread.
        unsafe {
            store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
                Some(ns_string!("platform-cookie-origin")),
                Some(&NSString::from_str(&rules)),
                Some(&complete),
            );
        }
    })
    .map_err(|_| ERROR)?;
    match tokio::time::timeout(Duration::from_secs(12), done).await {
        Ok(Ok(())) => Ok(()),
        _ => Err(ERROR.into()),
    }
}

fn content_rules(address: &Url, port: u16) -> Option<String> {
    let host = escape_url_filter(address.host_str()?);
    let authority = format!("^[a-z]+://([^/]*@)?{host}\\.?");
    let block = |filter: String| json!({ "trigger": { "url-filter": filter }, "action": { "type": "block-cookies" } });
    let mut rules = vec![block(format!("{authority}:{port}/"))];
    // Cookies can be sent on a WebSocket upgrade using the HTTP(S) default port.
    let default_schemes = match port {
        80 => ["http", "ws"].as_slice(),
        443 => ["https", "wss"].as_slice(),
        _ => &[],
    };
    rules.extend(
        default_schemes
            .iter()
            .map(|scheme| block(format!("^{scheme}://([^/]*@)?{host}\\.?/"))),
    );
    rules.push(json!({
        "trigger": { "url-filter": format!("^{}/", escape_url_filter(&address.origin().ascii_serialization())) },
        "action": { "type": "ignore-previous-rules" }
    }));
    Some(serde_json::Value::Array(rules).to_string())
}

fn escape_url_filter(url: &str) -> String {
    // The caller supplies only the canonical scheme, host and port from a URL.
    url.replace('.', "\\.")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn install(
    config: &WKWebViewConfiguration,
    rules: &WKContentRuleList,
    address: &Url,
    port: u16,
    header: &str,
    ready: oneshot::Sender<()>,
) {
    let Some(url) = NSURL::URLWithString(&NSString::from_str(address.as_str())) else {
        return;
    };
    let secure = address.scheme() == "https";
    let cookies: Option<Vec<_>> = super::super::cookies::pairs(header)
        .filter(|(name, value)| {
            HeaderName::from_bytes(name.as_bytes()).is_ok()
                && value
                    .strip_prefix('"')
                    .and_then(|rest| rest.strip_suffix('"'))
                    .unwrap_or(value)
                    .bytes()
                    .all(|byte| matches!(byte, 0x21 | 0x23..=0x2b | 0x2d..=0x3a | 0x3c..=0x5b | 0x5d..=0x7e))
        })
        .map(|(name, value)| scoped_cookie(&url, port, secure, name, value))
        .collect();
    let Some(cookies) = cookies else {
        return;
    };
    if cookies.is_empty() {
        let _ = ready.send(());
        return;
    }
    // SAFETY: The configuration and compiled rules are live on the main thread.
    let controller = unsafe { config.userContentController() };
    // SAFETY: Install the rules before any cookie can be stored or page can navigate.
    unsafe { controller.addContentRuleList(rules) };
    // SAFETY: The configuration and website store belong to this WebKit view.
    let store = unsafe { config.websiteDataStore() };
    // SAFETY: All cookie store operations run on WebKit's main thread.
    let store = unsafe { store.httpCookieStore() };
    let remaining = Cell::new(cookies.len());
    let ready = Cell::new(Some(ready));
    let completion = RcBlock::new(move || {
        let count = remaining.get() - 1;
        remaining.set(count);
        if count == 0
            && let Some(ready) = ready.take()
        {
            let _ = ready.send(());
        }
    });
    for cookie in cookies {
        // SAFETY: The store retains the cookie and copies the completion handler.
        unsafe { store.setCookie_completionHandler(&cookie, Some(&completion)) };
    }
}

fn scoped_cookie(
    url: &NSURL,
    port: u16,
    secure: bool,
    name: &str,
    value: &str,
) -> Option<Retained<NSHTTPCookie>> {
    let security = if secure { "; Secure" } else { "" };
    let header = NSString::from_str(&format!("{name}={value}; Path=/; HttpOnly{security}"));
    let headers = NSDictionary::from_slices(&[ns_string!("Set-Cookie")], &[&*header]);
    let parsed =
        NSHTTPCookie::cookiesWithResponseHeaderFields_forURL(&headers, url).firstObject()?;
    let properties = parsed.properties()?.mutableCopy();
    // SAFETY: Foundation publishes these immutable property keys for the process lifetime.
    let port_key = unsafe { NSHTTPCookiePort };
    // SAFETY: Foundation publishes this immutable property key for the process lifetime.
    let version_key = unsafe { NSHTTPCookieVersion };
    // SAFETY: Foundation publishes this immutable property key for the process lifetime.
    let discard_key = unsafe { NSHTTPCookieDiscard };
    properties.insert(port_key, &NSString::from_str(&port.to_string()));
    properties.insert(version_key, ns_string!("1"));
    properties.insert(discard_key, ns_string!("TRUE"));
    // SAFETY: The dictionary was made by Foundation. The added values use documented types.
    let cookie = unsafe { NSHTTPCookie::cookieWithProperties(&properties) }?;
    let ports = cookie.portList()?;
    (cookie.isSessionOnly()
        && cookie.isHTTPOnly()
        && cookie.isSecure() == secure
        && ports.len() == 1
        && ports.firstObject()?.unsignedShortValue() == port)
        .then_some(cookie)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_cookies_are_http_only_session_cookies_with_a_single_port() {
        for (origin, port, secure) in [
            ("https://billing.example.invalid/", 443, true),
            ("https://billing.example.invalid:8443/", 8443, true),
            ("http://[::1]:3521/", 3521, false),
        ] {
            let url = NSURL::URLWithString(&NSString::from_str(origin)).unwrap();
            let cookie = scoped_cookie(&url, port, secure, "Route", "\"dummy==\"").unwrap();
            assert_eq!(cookie.name().to_string(), "Route");
            assert_eq!(cookie.value().to_string(), "\"dummy==\"");
            assert!(!cookie.domain().to_string().starts_with('.'));
        }
    }
}
