//! Isolated WebView2 profiles for the embedded account pages.

use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use tauri::{AppHandle, Manager, Runtime, Webview};
use tokio::sync::oneshot;
use webview2_com::{
    ClearBrowsingDataCompletedHandler, CoTaskMemPWSTR,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_BROWSING_DATA_KINDS, COREWEBVIEW2_BROWSING_DATA_KINDS_CACHE_STORAGE,
        COREWEBVIEW2_BROWSING_DATA_KINDS_COOKIES, COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE,
        COREWEBVIEW2_BROWSING_DATA_KINDS_FILE_SYSTEMS, COREWEBVIEW2_BROWSING_DATA_KINDS_INDEXED_DB,
        COREWEBVIEW2_BROWSING_DATA_KINDS_SERVICE_WORKERS, COREWEBVIEW2_BROWSING_DATA_KINDS_WEB_SQL,
        ICoreWebView2, ICoreWebView2_13, ICoreWebView2Profile2,
    },
    NavigationCompletedEventHandler,
};
use windows_core::{Interface, PWSTR, Result as WinResult, w};

mod cookie_requests;
mod navigation;
mod network;

pub(super) struct ProfileDirectory {
    pub path: PathBuf,
    pub temporary: bool,
}

pub(super) fn profile_directory<R: Runtime>(
    app: &AppHandle<R>,
    account: Option<[u8; 16]>,
    temporary: &mut Option<tempfile::TempDir>,
) -> Result<ProfileDirectory, String> {
    const ERROR: &str = "未能创建平台页面的独立存储。";
    if let Some(id) = account {
        let path = app
            .path()
            .app_local_data_dir()
            .map_err(|_| ERROR)?
            .join("platform-accounts")
            .join(uuid::Uuid::from_bytes(id).to_string());
        std::fs::create_dir_all(&path).map_err(|_| ERROR)?;
        return Ok(ProfileDirectory {
            path,
            temporary: false,
        });
    }
    let profile = tempfile::Builder::new()
        .prefix("kwork-platform-")
        .tempdir()
        .map_err(|_| ERROR)?;
    let path = profile.path().to_owned();
    *temporary = Some(profile);
    Ok(ProfileDirectory {
        path,
        temporary: true,
    })
}

type Completion = oneshot::Sender<WinResult<()>>;

async fn on_webview<R: Runtime>(
    view: &Webview<R>,
    error: &str,
    action: impl FnOnce(ICoreWebView2, Completion) -> WinResult<()> + Send + 'static,
) -> Result<(), String> {
    let (send, receive) = oneshot::channel();
    view.with_webview(move |view| {
        // SAFETY: Tauri hands over the live controller on its COM apartment.
        let core = unsafe { view.controller().CoreWebView2() };
        match core {
            Ok(core) => {
                let _ = action(core, send);
            }
            Err(error) => {
                let _ = send.send(Err(error));
            }
        }
    })
    .map_err(|_| error.to_owned())?;
    match tokio::time::timeout(Duration::from_secs(12), receive).await {
        Ok(Ok(Ok(()))) => Ok(()),
        _ => Err(error.into()),
    }
}

pub(super) async fn configure<R: Runtime>(
    view: &Webview<R>,
    origin: url::Origin,
    headers: BTreeMap<String, String>,
) -> Result<oneshot::Receiver<bool>, String> {
    let (report, outcome) = oneshot::channel();
    on_webview(
        view,
        "未能配置平台页面的请求。",
        move |core, send| {
            let with_cookie = headers.contains_key("cookie");
            let installed = navigation::install(&core, origin.clone(), report)
                .and_then(|()| network::install(&core, origin.clone(), headers.clone()));
            if installed.is_ok() && with_cookie {
                cookie_requests::install(&core, origin, headers, send)
            } else {
                let _ = send.send(installed);
                Ok(())
            }
        },
    )
    .await?;
    Ok(outcome)
}

pub(super) async fn sanitize<R: Runtime>(view: &Webview<R>) -> Result<(), String> {
    on_webview(
        view,
        "未能清理平台页面的登录数据。",
        |core, send| {
            let core = core.cast::<ICoreWebView2_13>()?;
            // SAFETY: The core is live on the creating apartment and the returned
            // profile refers only to this WebView's dedicated user data folder.
            let profile = unsafe { core.Profile() }?.cast::<ICoreWebView2Profile2>()?;
            let types = COREWEBVIEW2_BROWSING_DATA_KINDS(
                COREWEBVIEW2_BROWSING_DATA_KINDS_FILE_SYSTEMS.0
                    | COREWEBVIEW2_BROWSING_DATA_KINDS_INDEXED_DB.0
                    | COREWEBVIEW2_BROWSING_DATA_KINDS_WEB_SQL.0
                    | COREWEBVIEW2_BROWSING_DATA_KINDS_CACHE_STORAGE.0
                    | COREWEBVIEW2_BROWSING_DATA_KINDS_COOKIES.0
                    | COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE.0
                    | COREWEBVIEW2_BROWSING_DATA_KINDS_SERVICE_WORKERS.0,
            );
            let callback = ClearBrowsingDataCompletedHandler::create(Box::new(move |result| {
                let _ = send.send(result);
                Ok(())
            }));
            // SAFETY: WebView2 retains the callback on this apartment until the
            // requested data has been cleared. LocalStorage is outside the mask.
            unsafe { profile.ClearBrowsingData(types, &callback) }
        },
    )
    .await
}

pub(super) async fn retire<R: Runtime>(view: &Webview<R>) -> Result<(), String> {
    // Keep the profile alive long enough for the cleanup to finish, after the
    // prior document and its workers have been taken out of the page.
    on_webview(view, "未能卸载平台页面。", |core, send| {
        // SAFETY: `core` is a live WebView2 interface on this apartment.
        unsafe { core.Stop() }?;
        let mut send = Some(send);
        let finished = NavigationCompletedEventHandler::create(Box::new(move |core, _| {
            let (Some(core), Some(_)) = (core, send.as_ref()) else {
                return Ok(());
            };
            let mut source = PWSTR::null();
            // SAFETY: `source` provides an initialized writable PWSTR and frees
            // the returned CoTaskMem string through its guard below.
            let result = unsafe { core.Source(&mut source) };
            let source = CoTaskMemPWSTR::from(source);
            result?;
            if source.to_string() == "about:blank"
                && let Some(send) = send.take()
            {
                let _ = send.send(Ok(()));
            }
            Ok(())
        }));
        let mut subscription = 0;
        // SAFETY: The runtime retains the registered callback until this closing
        // view is destroyed. The writable event token lives through registration.
        unsafe { core.add_NavigationCompleted(&finished, &mut subscription) }?;
        // SAFETY: Static, null-terminated empty HTML never executes script or
        // issues a network request. Navigation is dispatched on the apartment.
        unsafe { core.NavigateToString(w!("<!doctype html>")) }
    })
    .await?;
    sanitize(view).await
}
