//! WebKit account stores keep page preferences and clear authentication data.

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_foundation::{
    NSDate, NSMutableURLRequest, NSProcessInfo, NSSet, NSString, NSTimeZone, NSURL, NSUUID,
    ns_string,
};
use objc2_web_kit::{WKWebView, WKWebsiteDataStore, WKWebsiteDataTypeLocalStorage};
use std::{cell::Cell, collections::BTreeMap, time::Duration};
use tauri::{AppHandle, Runtime, Webview};
use tokio::sync::oneshot;
use url::Url;

mod cookies;
mod navigation;

pub(super) use cookies::configure_cookies;

pub(super) async fn navigate<R: Runtime>(
    view: &Webview<R>,
    address: Url,
    mut headers: BTreeMap<String, String>,
    is_current: impl FnOnce() -> bool + Send + 'static,
) -> Result<oneshot::Receiver<()>, String> {
    const ERROR: &str = "未能加载平台页面。";
    // Cookie routing is handled by the WebKit store. On a redirect, the
    // view's navigation policy must check the destination before it is sent.
    headers.remove("cookie");
    headers.extend(
        [
            ("x-client-bundle-id", ""),
            ("x-client-platform", "desktop-mac"),
            ("x-client-version", env!("CARGO_PKG_VERSION")),
            ("x-client-locale", "zh_CN"),
        ]
        .map(|(name, value)| (name.into(), value.into())),
    );
    let (send, done) = oneshot::channel();
    let (report_failure, failed) = oneshot::channel();
    view.with_webview(move |view| {
        if send.is_closed() || !is_current() {
            let _ = send.send(true);
            return;
        }
        let Some(url) = NSURL::URLWithString(&NSString::from_str(address.as_str())) else {
            return;
        };
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let request = NSMutableURLRequest::requestWithURL(&url);
        for (name, value) in headers {
            request.setValue_forHTTPHeaderField(
                Some(&NSString::from_str(&value)),
                &NSString::from_str(&name),
            );
        }
        let offset = NSTimeZone::localTimeZone().secondsFromGMT().to_string();
        request.setValue_forHTTPHeaderField(
            Some(&NSString::from_str(&offset)),
            ns_string!("x-client-timezone-offset"),
        );
        // SAFETY: Tauri exposes the live WKWebView on its owning main thread.
        let webview: &WKWebView = unsafe { &*view.inner().cast() };
        let started = navigation::load(webview, &request, report_failure, mtm);
        let _ = send.send(started);
    })
    .map_err(|_| ERROR)?;
    match tokio::time::timeout(Duration::from_secs(12), done).await {
        Ok(Ok(true)) => Ok(failed),
        _ => Err(ERROR.into()),
    }
}

pub(super) fn supports_account_stores() -> bool {
    NSProcessInfo::processInfo()
        .operatingSystemVersion()
        .majorVersion
        >= 14
}

pub(super) async fn sanitize<R: Runtime>(app: &AppHandle<R>, id: [u8; 16]) -> Result<(), String> {
    const ERROR: &str = "未能清理平台页面的登录数据。";
    let (sender, done) = oneshot::channel();
    app.run_on_main_thread(move || {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let sender = Cell::new(Some(sender));
        let complete = RcBlock::new(move || {
            if let Some(sender) = sender.take() {
                let _ = sender.send(());
            }
        });
        // SAFETY: WebKit requires the main thread, proven by `mtm`. The caller
        // only supplies a nonzero identifier on macOS 14 or later.
        let store =
            unsafe { WKWebsiteDataStore::dataStoreForIdentifier(&NSUUID::from_bytes(id), mtm) };
        // SAFETY: The retained set is returned from WebKit on the main thread.
        let mut types = unsafe { WKWebsiteDataStore::allWebsiteDataTypes(mtm) }.to_vec();
        // SAFETY: WebKit publishes this immutable constant for the lifetime of the process.
        let local_storage = unsafe { WKWebsiteDataTypeLocalStorage };
        types.retain(|kind| &**kind != local_storage);
        // SAFETY: The cleanup API copies the completion block and retains its
        // arguments as needed. All Objective-C objects stay on this thread.
        unsafe {
            store.removeDataOfTypes_modifiedSince_completionHandler(
                &NSSet::from_retained_slice(&types),
                &NSDate::distantPast(),
                &complete,
            );
        }
    })
    .map_err(|_| ERROR.to_owned())?;
    done.await.map_err(|_| ERROR.to_owned())
}
