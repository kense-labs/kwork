//! The authenticated Platform document belongs to one main renderer and one grant.

use crate::{acp, browser::current_browser_shell};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};
#[cfg(not(windows))]
use tauri::webview::PageLoadEvent;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Runtime, State, Webview, WebviewUrl,
    http::{HeaderName, HeaderValue},
    webview::{NewWindowResponse, PermissionResponse, WebviewBuilder},
};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Mutex;
use url::{Host, Url};

#[cfg(any(target_os = "macos", windows, test))]
mod cookies;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(any(target_os = "macos", windows))]
use std::sync::atomic::AtomicU8;

#[derive(Default)]
pub(crate) struct AccountPlatform {
    epoch: AtomicU64,
    state: Mutex<PlatformState>,
    #[cfg(any(target_os = "macos", windows))]
    exit: AtomicU8,
}

impl AccountPlatform {
    fn accepts_visits(&self) -> bool {
        #[cfg(any(target_os = "macos", windows))]
        {
            self.exit.load(Ordering::Acquire) == EXIT_RUNNING
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        true
    }
}

#[cfg(any(target_os = "macos", windows))]
const EXIT_RUNNING: u8 = 0;
#[cfg(any(target_os = "macos", windows))]
const EXIT_CLEANING: u8 = 1;
#[cfg(any(target_os = "macos", windows))]
const EXIT_CLEANED: u8 = 2;

#[derive(Default)]
struct PlatformState {
    shell: u64,
    last_id: u32,
    active: Option<Visit>,
}

struct Visit {
    id: u32,
    epoch: u64,
    bounds: Bounds,
    loaded: bool,
    #[cfg(any(target_os = "macos", windows))]
    storage: Option<[u8; 16]>,
    #[cfg(windows)]
    temporary_profile: Option<tempfile::TempDir>,
}

#[derive(Clone, Copy, Deserialize)]
pub(crate) struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Bounds {
    fn validate(self) -> Result<Self, String> {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| (0. ..=100_000.).contains(value))
            .then_some(self)
            .ok_or_else(|| "平台页面布局无效。".into())
    }

    fn present<R: Runtime>(self, view: &Webview<R>) -> tauri::Result<()> {
        if self.width < 1. || self.height < 1. {
            return view.hide();
        }
        view.set_bounds(tauri::Rect {
            position: LogicalPosition::new(self.x, self.y).into(),
            size: LogicalSize::new(self.width, self.height).into(),
        })?;
        view.show()
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Page {
    Usage,
    TopUp,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub(crate) enum Request {
    Open { id: u32, page: Page, bounds: Bounds },
    Bounds { id: u32, bounds: Bounds },
    Close { id: u32 },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlatformGrant {
    origin: String,
    token: String,
    embedded_page_dist: Option<String>,
    #[serde(default)]
    request_headers: BTreeMap<String, String>,
    #[cfg(any(target_os = "macos", windows))]
    user_id: Option<String>,
}

impl PlatformGrant {
    fn address(&self, page: Page) -> Option<Url> {
        let origin = Url::parse(&self.origin).ok()?;
        let supported = origin.scheme() == "https"
            || origin.scheme() == "http"
                && match origin.host() {
                    Some(Host::Domain("localhost")) => true,
                    Some(Host::Ipv4(address)) => address.is_loopback(),
                    Some(Host::Ipv6(address)) => address.is_loopback(),
                    _ => false,
                };
        if !supported
            || !uncredentialed(&origin)
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
            || self.token.is_empty()
        {
            return None;
        }
        let mut address = origin
            .join(match page {
                Page::Usage => "/usage",
                Page::TopUp => "/top_up",
            })
            .ok()?;
        if let Some(dist) = self
            .embedded_page_dist
            .as_deref()
            .filter(|dist| !dist.is_empty())
        {
            address.query_pairs_mut().append_pair("dist", dist);
        }
        Some(address)
    }

    #[cfg(any(target_os = "macos", windows))]
    fn storage_id(&self, address: &Url) -> Option<[u8; 16]> {
        use sha2::{Digest, Sha256};

        let user_id = self.user_id.as_deref().filter(|id| !id.is_empty())?;
        let scope = json!([address.origin().ascii_serialization(), user_id]);
        let mut digest = Sha256::new();
        digest.update(b"kwork-account-platform\0");
        digest.update(scope.to_string().as_bytes());
        let bytes = digest.finalize();
        Some(
            uuid::Builder::from_custom_bytes(bytes[..16].try_into().unwrap())
                .into_uuid()
                .into_bytes(),
        )
    }
}

fn uncredentialed(url: &Url) -> bool {
    url.username().is_empty() && url.password().is_none()
}

fn label(shell: u64, id: u32) -> String {
    format!("dsh-account-platform-{shell}-{id}")
}

impl PlatformState {
    async fn clear<R: Runtime>(&mut self, app: &AppHandle<R>) -> Result<(), String> {
        let Some(visit) = self.active.take() else {
            return Ok(());
        };
        #[cfg(windows)]
        let mut cleanup = Ok(());
        if let Some(view) = app.get_webview(&label(self.shell, visit.id)) {
            let _ = view.hide();
            #[cfg(windows)]
            if visit.storage.is_some() {
                cleanup = windows::retire(&view).await;
            }
            if view.close().is_err() {
                self.active = Some(visit);
                return Err("未能关闭平台页面。".into());
            }
        }
        #[cfg(windows)]
        drop(visit.temporary_profile);
        #[cfg(windows)]
        cleanup?;
        #[cfg(target_os = "macos")]
        if let Some(storage) = visit.storage {
            macos::sanitize(app, storage).await?;
        }
        Ok(())
    }

    async fn set_shell<R: Runtime>(
        &mut self,
        app: &AppHandle<R>,
        shell: u64,
    ) -> Result<(), String> {
        if self.shell != shell {
            self.clear(app).await?;
            self.shell = shell;
            self.last_id = 0;
        }
        Ok(())
    }
}

pub(crate) fn retire_shell<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let service = app.state::<AccountPlatform>();
        let mut state = service.state.lock().await;
        let _ = state.set_shell(&app, current_browser_shell(&app)).await;
    });
}

#[cfg(any(target_os = "macos", windows))]
pub(crate) fn defer_exit<R: Runtime>(
    app: &AppHandle<R>,
    code: Option<i32>,
    api: &tauri::ExitRequestApi,
) -> bool {
    if code == Some(tauri::RESTART_EXIT_CODE) {
        return false;
    }
    defer_shutdown(
        app,
        || api.prevent_exit(),
        move |app| app.exit(code.unwrap_or_default()),
    )
}

#[cfg(windows)]
pub(crate) fn window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    if window.label() == "main"
        && let tauri::WindowEvent::CloseRequested { api, .. } = event
    {
        let main = window.clone();
        defer_shutdown(
            window.app_handle(),
            || api.prevent_close(),
            move |_| {
                let _ = main.close();
            },
        );
    }
}

#[cfg(any(target_os = "macos", windows))]
fn defer_shutdown<R: Runtime>(
    app: &AppHandle<R>,
    prevent: impl FnOnce(),
    finish: impl FnOnce(AppHandle<R>) + Send + 'static,
) -> bool {
    let platform = app.state::<AccountPlatform>();
    match platform.exit.load(Ordering::Acquire) {
        EXIT_CLEANED => return false,
        EXIT_RUNNING
            if platform.state.try_lock().is_ok_and(|state| {
                state
                    .active
                    .as_ref()
                    .is_none_or(|visit| visit.storage.is_none())
            }) =>
        {
            return false;
        }
        _ => {}
    }
    prevent();
    if platform
        .exit
        .compare_exchange(
            EXIT_RUNNING,
            EXIT_CLEANING,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let platform = app.state::<AccountPlatform>();
            let mut state = platform.state.lock().await;
            if let Err(error) = state.clear(&app).await {
                eprintln!("{error}");
            }
            drop(state);
            platform.exit.store(EXIT_CLEANED, Ordering::Release);
            finish(app);
        });
    }
    true
}

pub(crate) fn invalidate_grant<R: Runtime>(app: &AppHandle<R>) {
    app.state::<AccountPlatform>()
        .epoch
        .fetch_add(1, Ordering::AcqRel);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let service = app.state::<AccountPlatform>();
        let mut state = service.state.lock().await;
        if let Some(id) = state
            .active
            .as_ref()
            .filter(|visit| visit.epoch != service.epoch.load(Ordering::Acquire))
            .map(|visit| visit.id)
        {
            state.revoke(&app, id).await;
        }
    });
}

impl PlatformState {
    async fn revoke<R: Runtime>(&mut self, app: &AppHandle<R>, id: u32) {
        let _ = self.clear(app).await;
        surface_status(app, self.shell, id, "revoked");
    }
}

fn surface_status<R: Runtime>(app: &AppHandle<R>, shell: u64, id: u32, status: &str) {
    let _ = app.emit_to(
        "main",
        "acp-event",
        json!({
            "kind": "account_platform_status", "shell": shell, "id": id, "status": status,
        }),
    );
}

#[cfg(target_os = "macos")]
pub(crate) fn process_terminated<R: Runtime>(view: &Webview<R>) -> bool {
    let Some((shell, id)) = view
        .label()
        .strip_prefix("dsh-account-platform-")
        .and_then(|suffix| suffix.split_once('-'))
        .and_then(|(shell, id)| Some((shell.parse::<u64>().ok()?, id.parse::<u32>().ok()?)))
    else {
        return false;
    };
    let app = view.app_handle().clone();
    tauri::async_runtime::spawn(async move {
        let service = app.state::<AccountPlatform>();
        let mut state = service.state.lock().await;
        if state.shell == shell && state.active.as_ref().is_some_and(|visit| visit.id == id) {
            let _ = state.clear(&app).await;
            surface_status(&app, shell, id, "failed");
        }
    });
    true
}

fn page_loaded<R: Runtime>(app: &AppHandle<R>, shell: u64, id: u32) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let service = app.state::<AccountPlatform>();
        let mut state = service.state.lock().await;
        if state.shell != shell || current_browser_shell(&app) != shell {
            return;
        }
        let epoch = service.epoch.load(Ordering::Acquire);
        let Some(current) = state
            .active
            .as_mut()
            .filter(|visit| visit.id == id && visit.epoch == epoch)
        else {
            return;
        };
        if let Some(view) = app.get_webview(&label(shell, id))
            && current.bounds.present(&view).is_ok()
        {
            current.loaded = true;
            surface_status(&app, shell, id, "loaded");
        } else {
            let _ = state.clear(&app).await;
            surface_status(&app, shell, id, "failed");
        }
    });
}

#[cfg(any(target_os = "macos", windows))]
async fn page_failed<R: Runtime>(app: &AppHandle<R>, shell: u64, id: u32) {
    let service = app.state::<AccountPlatform>();
    let mut state = service.state.lock().await;
    let epoch = service.epoch.load(Ordering::Acquire);
    if state.shell == shell
        && current_browser_shell(app) == shell
        && state
            .active
            .as_ref()
            .is_some_and(|visit| visit.id == id && visit.epoch == epoch && !visit.loaded)
    {
        let _ = state.clear(app).await;
        surface_status(app, shell, id, "failed");
    }
}

#[tauri::command]
pub(crate) async fn account_platform_view(
    webview: Webview,
    platform: State<'_, AccountPlatform>,
    host: State<'_, acp::AcpService>,
    shell: u64,
    request: Request,
) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("平台页面只能由主界面打开。".into());
    }
    if !platform.accepts_visits() {
        return Ok(());
    }
    let app = webview.app_handle();
    let mut state = platform.state.lock().await;
    if current_browser_shell(app) != shell {
        return Ok(());
    }
    state.set_shell(app, shell).await?;
    let (id, page, bounds) = match request {
        Request::Close { id } => {
            if id >= state.last_id {
                state.last_id = id;
                state.clear(app).await?;
            }
            return Ok(());
        }
        Request::Bounds { id, bounds } => {
            if let Some(visit) = state.active.as_mut().filter(|visit| visit.id == id) {
                visit.bounds = bounds.validate()?;
                if visit.loaded
                    && let Some(view) = app.get_webview(&label(shell, id))
                {
                    visit
                        .bounds
                        .present(&view)
                        .map_err(|_| "未能调整平台页面布局。")?;
                }
            }
            return Ok(());
        }
        Request::Open { id, page, bounds } if id > state.last_id => (id, page, bounds.validate()?),
        Request::Open { .. } => return Ok(()),
    };
    state.clear(app).await?;
    state.last_id = id;
    let epoch = platform.epoch.load(Ordering::Acquire);
    state.active = Some(Visit {
        id,
        epoch,
        bounds,
        loaded: false,
        #[cfg(any(target_os = "macos", windows))]
        storage: None,
        #[cfg(windows)]
        temporary_profile: None,
    });
    drop(state);

    let session = acp::private_platform_grant(&host).await;
    let mut state = platform.state.lock().await;
    if state.active.as_ref().is_none_or(|visit| visit.id != id)
        || state.shell != shell
        || current_browser_shell(app) != shell
        || platform.epoch.load(Ordering::Acquire) != epoch
    {
        return Ok(());
    }
    let result = async {
        let mut grant = session?;
        let address = grant.address(page).ok_or("平台页面地址无效。")?;
        if webview
            .url()
            .is_ok_and(|current| current.origin() == address.origin())
        {
            return Err("平台页面不能打开应用本身。".into());
        }
        #[cfg(target_os = "macos")]
        let storage = if macos::supports_account_stores() {
            grant.storage_id(&address)
        } else {
            None
        };
        #[cfg(target_os = "macos")]
        if let Some(storage) = storage {
            // Start with clean authentication even after a prior interrupted exit.
            macos::sanitize(app, storage).await?;
            if current_browser_shell(app) != shell
                || platform.epoch.load(Ordering::Acquire) != epoch
                || !platform.accepts_visits()
            {
                return Ok(());
            }
        }
        #[cfg(windows)]
        let storage = grant.storage_id(&address);
        #[cfg(any(target_os = "macos", windows))]
        let visit = state
            .active
            .as_mut()
            .expect("opening visit retained under lock");
        #[cfg(any(target_os = "macos", windows))]
        {
            visit.storage = storage;
        }
        let headers = platform_headers(
            std::mem::take(&mut grant.request_headers),
            cfg!(any(target_os = "macos", windows)),
        );
        let view = mount(
            &webview,
            shell,
            id,
            &address,
            &grant.token,
            &headers,
            #[cfg(target_os = "macos")]
            storage,
            #[cfg(windows)]
            windows::profile_directory(app, storage, &mut visit.temporary_profile)?,
        )
        .await?;
        #[cfg(windows)]
        let result = windows::configure(&view, address.origin(), headers).await?;
        #[cfg(windows)]
        if storage.is_some() {
            windows::sanitize(&view).await?;
        }
        if current_browser_shell(app) != shell
            || platform.epoch.load(Ordering::Acquire) != epoch
            || !platform.accepts_visits()
        {
            state.revoke(app, id).await;
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        {
            let owner = app.clone();
            let failed = macos::navigate(&view, address, headers, move || {
                let platform = owner.state::<AccountPlatform>();
                current_browser_shell(&owner) == shell
                    && platform.epoch.load(Ordering::Acquire) == epoch
                    && platform.accepts_visits()
            })
            .await?;
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if failed.await.is_ok() {
                    page_failed(&app, shell, id).await;
                }
            });
            Ok(())
        }
        #[cfg(windows)]
        {
            view.navigate(address).map_err(|_| "未能加载平台页面。")?;
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                match result.await {
                    Ok(true) => page_loaded(&app, shell, id),
                    Ok(false) => page_failed(&app, shell, id).await,
                    Err(_) => {}
                }
            });
            Ok(())
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        view.navigate(address)
            .map_err(|_| "未能加载平台页面。".into())
    }
    .await;
    if result.is_err() {
        let _ = state.clear(app).await;
    }
    result
}

async fn mount<R: Runtime>(
    owner: &Webview<R>,
    shell: u64,
    id: u32,
    address: &Url,
    token: &str,
    headers: &BTreeMap<String, String>,
    #[cfg(target_os = "macos")] storage: Option<[u8; 16]>,
    #[cfg(windows)] profile: windows::ProfileDirectory,
) -> Result<Webview<R>, String> {
    let app = owner.app_handle().clone();
    let origin = address.origin();
    let navigation_origin = origin.clone();
    let blank = Url::parse("about:blank").expect("constant blank URL");
    let fetch_headers = (!cfg!(windows)).then(|| {
        headers
            .iter()
            .filter(|(name, _)| name.as_str() != "cookie")
            .collect::<BTreeMap<_, _>>()
    });
    let bootstrap = json!({
        "origin": origin.ascii_serialization(), "token": token,
        "headers": fetch_headers,
        "version": env!("CARGO_PKG_VERSION"),
    });
    let init = format!(
        "({})({bootstrap})",
        include_str!("account_platform_preload.js")
    );
    let builder = WebviewBuilder::new(label(shell, id), WebviewUrl::External(blank))
        .focused(false)
        .incognito(true)
        .devtools(false)
        .initialization_script(init)
        .on_navigation(move |target| {
            target.as_str() == "about:blank"
                || target.origin() == navigation_origin && uncredentialed(target)
        })
        .on_permission_request(|_, _| PermissionResponse::Deny)
        .on_new_window(move |target, _| {
            if target.scheme() == "https" && uncredentialed(&target) {
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let _ = app.opener().open_url(target.as_str(), None::<&str>);
                });
            }
            NewWindowResponse::Deny
        });
    #[cfg(not(windows))]
    let builder = builder.on_page_load(move |view, event| {
        if event.event() == PageLoadEvent::Finished && event.url().origin() == origin {
            page_loaded(view.app_handle(), shell, id);
        }
    });
    #[cfg(target_os = "macos")]
    let builder = if let Some(storage) = storage {
        builder.incognito(false).data_store_identifier(storage)
    } else {
        builder
    };
    #[cfg(windows)]
    let builder = builder
        .incognito(profile.temporary)
        .data_directory(profile.path)
        .general_autofill_enabled(false);
    let view = owner
        .window()
        .add_child(
            builder,
            LogicalPosition::new(-4., -4.),
            LogicalSize::new(1., 1.),
        )
        .map_err(|_| "未能创建平台页面。")?;
    let _ = view.hide();
    #[cfg(target_os = "macos")]
    if let Some(deployment_cookie) = headers.get("cookie") {
        macos::configure_cookies(&view, address.clone(), deployment_cookie.clone()).await?;
    }
    Ok(view)
}

fn platform_headers(
    configured: BTreeMap<String, String>,
    native_requests: bool,
) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    for (name, value) in configured {
        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        if HeaderValue::from_str(&value).is_err() {
            continue;
        }
        // JavaScript fetch cannot set Cookie. Route it only through a native policy.
        if native_requests || name.as_str() != "cookie" {
            headers.insert(name.as_str().to_string(), value);
        }
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_destination_uses_only_its_grant_origin_and_fixed_routes() {
        let mut grant = PlatformGrant {
            origin: "https://platform.example.invalid".into(),
            token: "synthetic-token".into(),
            embedded_page_dist: Some("preview&mode=other".into()),
            request_headers: BTreeMap::new(),
            #[cfg(any(target_os = "macos", windows))]
            user_id: None,
        };
        let usage = grant.address(Page::Usage).unwrap();
        assert_eq!(
            usage.as_str(),
            "https://platform.example.invalid/usage?dist=preview%26mode%3Dother"
        );
        assert_eq!(grant.address(Page::TopUp).unwrap().path(), "/top_up");
        for origin in [
            "http://platform.example.invalid",
            "https://user@platform.example.invalid",
            "https://platform.example.invalid/foreign",
            "https://platform.example.invalid/?other=route",
            "file:///tmp/platform",
        ] {
            grant.origin = origin.to_owned();
            assert!(grant.address(Page::Usage).is_none(), "{origin}");
        }
        grant.origin = "http://127.0.0.1:3521".into();
        assert_eq!(grant.address(Page::Usage).unwrap().port(), Some(3521));
        grant.token.clear();
        assert!(grant.address(Page::Usage).is_none());
    }

    #[test]
    fn platform_headers_exclude_invalid_values_and_host_scoped_cookies() {
        let configured = BTreeMap::from([
            ("Cookie".into(), "secret=synthetic".into()),
            ("X-Deploy".into(), "preview".into()),
            ("X-Bad".into(), "first\r\nsecond".into()),
        ]);
        assert_eq!(
            platform_headers(configured.clone(), false),
            BTreeMap::from([("x-deploy".into(), "preview".into())]),
        );
        assert_eq!(
            platform_headers(configured, true),
            BTreeMap::from([
                ("cookie".into(), "secret=synthetic".into()),
                ("x-deploy".into(), "preview".into()),
            ])
        );
    }

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn platform_store_is_scoped_to_the_verified_identity_and_origin() {
        let mut grant = PlatformGrant {
            origin: "https://platform.example.invalid".into(),
            token: "synthetic-token".into(),
            embedded_page_dist: None,
            request_headers: BTreeMap::new(),
            user_id: None,
        };
        let usage = grant.address(Page::Usage).unwrap();
        let top_up = grant.address(Page::TopUp).unwrap();
        assert_eq!(grant.storage_id(&usage), None);
        grant.user_id = Some(String::new());
        assert_eq!(grant.storage_id(&usage), None);
        grant.user_id = Some("account-a".into());
        let store = grant.storage_id(&usage).expect("known account store");
        grant.token = "new-synthetic-token".into();
        assert_eq!(grant.storage_id(&top_up), Some(store));
        let other_port = Url::parse("https://platform.example.invalid:8443/usage").unwrap();
        assert_ne!(grant.storage_id(&other_port), Some(store));
        grant.user_id = Some("account-b".into());
        assert_ne!(grant.storage_id(&usage), Some(store));
    }
}
