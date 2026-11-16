//! Platform deployment and client headers for the real WebView2 request.

use super::super::{cookies, uncredentialed};
use std::collections::BTreeMap;
use url::{Origin, Url};
use webview2_com::{
    CoTaskMemPWSTR,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL, COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
        ICoreWebView2, ICoreWebView2_2, ICoreWebView2_22, ICoreWebView2HttpRequestHeaders,
        ICoreWebView2Settings4, ICoreWebView2WebResourceRequestedEventArgs,
    },
    WebResourceRequestedEventHandler,
};
use windows::Win32::System::{
    Com::IStream,
    SystemServices::{TIME_ZONE_ID_DAYLIGHT, TIME_ZONE_ID_STANDARD, TIME_ZONE_ID_UNKNOWN},
    Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION},
};
use windows_core::{BOOL, HSTRING, Interface, PWSTR, Result as WinResult, w};

const IDENTITY: [(&str, &str); 4] = [
    ("x-client-bundle-id", ""),
    ("x-client-platform", "desktop-win"),
    ("x-client-version", env!("CARGO_PKG_VERSION")),
    ("x-client-locale", "zh_CN"),
];
const TIME_ZONE: &str = "x-client-timezone-offset";

pub(super) fn install(
    core: &ICoreWebView2,
    origin: Origin,
    mut deployment: BTreeMap<String, String>,
) -> WinResult<()> {
    let deployment_cookie = deployment.remove("cookie");
    deployment.remove(TIME_ZONE);
    deployment.extend(IDENTITY.map(|(name, value)| (name.into(), value.into())));
    let environment = core.cast::<ICoreWebView2_2>()?;
    // SAFETY: The live core is on its creating COM apartment. This environment
    // creates a synthetic rejection if editing a request fails.
    let environment = unsafe { environment.Environment() }?;
    let handler = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        if rewrite(&args, &origin, &deployment, deployment_cookie.as_deref()).is_err() {
            // SAFETY: A body is optional for this synthetic empty response.
            // The strings are static and the environment is on its owning apartment.
            let rejection = unsafe {
                environment.CreateWebResourceResponse(
                    None::<&IStream>,
                    403,
                    w!("Forbidden"),
                    w!(""),
                )
            }?;
            // SAFETY: Substituting this response prevents a request with partially
            // edited private headers from reaching the network.
            unsafe { args.SetResponse(&rejection) }?;
        }
        Ok(())
    }));
    let filter = core.cast::<ICoreWebView2_22>()?;
    // SAFETY: Observe all HTTP(S) contexts through the same handler, including
    // worker requests and external redirect destinations. No foreign origin is trusted by this filter.
    unsafe {
        filter.AddWebResourceRequestedFilterWithRequestSourceKinds(
            w!("*"),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
            COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
        )
    }?;
    let mut subscription = 0;
    // SAFETY: The core owns the handler for the lifetime of this dedicated child.
    unsafe { core.add_WebResourceRequested(&handler, &mut subscription) }?;

    // SAFETY: The settings belong to this live COM core.
    let settings = unsafe { core.Settings() }?.cast::<ICoreWebView2Settings4>()?;
    // SAFETY: The dedicated platform profile never saves form passwords.
    unsafe { settings.SetIsPasswordAutosaveEnabled(false) }
}

fn rewrite(
    args: &ICoreWebView2WebResourceRequestedEventArgs,
    origin: &Origin,
    deployment: &BTreeMap<String, String>,
    cookie: Option<&str>,
) -> WinResult<()> {
    // SAFETY: This callback owns the live request arguments through the whole call.
    let request = unsafe { args.Request() }?;
    let mut uri = PWSTR::null();
    // SAFETY: `uri` is a writable result slot, with a CoTaskMem guard below.
    let result = unsafe { request.Uri(&mut uri) };
    let uri = CoTaskMemPWSTR::from(uri);
    result?;
    let trusted = Url::parse(&uri.to_string())
        .is_ok_and(|url| url.origin() == *origin && uncredentialed(&url));
    // SAFETY: Header edits occur inside the live WebResourceRequested event.
    let headers = unsafe { request.Headers() }?;
    if trusted {
        for (name, value) in deployment {
            set_header(&headers, name, value)?;
        }
        set_header(&headers, TIME_ZONE, &timezone_seconds().to_string())?;
    } else {
        for (name, value) in deployment {
            if get_header(&headers, name)?.is_some_and(|actual| actual.trim() == value.trim()) {
                remove_header(&headers, name)?;
            }
        }
        remove_header(&headers, TIME_ZONE)?;
        remove_header(&headers, "x-dsh-auth-token")?;
    }
    if let Some(cookie) = cookie {
        let base = get_header(&headers, "cookie")?;
        let updated = if trusted {
            cookies::merge(base.as_deref().unwrap_or_default(), cookie)
        } else if let Some(base) = &base {
            cookies::strip_deployment(base, cookie)
        } else {
            return Ok(());
        };
        if base.as_deref() != Some(&updated) {
            if updated.is_empty() {
                remove_header(&headers, "cookie")?;
            } else {
                set_header(&headers, "cookie", &updated)?;
            }
        }
    }
    Ok(())
}

fn has_header(headers: &ICoreWebView2HttpRequestHeaders, name: &HSTRING) -> WinResult<bool> {
    let mut present = BOOL::default();
    // SAFETY: The live collection borrows the header name and writes to our BOOL.
    unsafe { headers.Contains(name, &mut present) }?;
    Ok(present.as_bool())
}

fn get_header(headers: &ICoreWebView2HttpRequestHeaders, name: &str) -> WinResult<Option<String>> {
    let name = HSTRING::from(name);
    if !has_header(headers, &name)? {
        return Ok(None);
    }
    let mut value = PWSTR::null();
    // SAFETY: `value` is a writable result slot. The guard frees the returned string.
    let result = unsafe { headers.GetHeader(&name, &mut value) };
    let guard = CoTaskMemPWSTR::from(value);
    result?;
    Ok(Some(guard.to_string()))
}

fn set_header(headers: &ICoreWebView2HttpRequestHeaders, name: &str, value: &str) -> WinResult<()> {
    // SAFETY: Both strings are owned UTF-16 values held through the synchronous edit.
    unsafe { headers.SetHeader(&HSTRING::from(name), &HSTRING::from(value)) }
}

fn remove_header(headers: &ICoreWebView2HttpRequestHeaders, name: &str) -> WinResult<()> {
    let name = HSTRING::from(name);
    if has_header(headers, &name)? {
        // SAFETY: The live collection borrows the known header name for this call.
        unsafe { headers.RemoveHeader(&name) }?;
    }
    Ok(())
}

fn timezone_seconds() -> i32 {
    let mut zone = TIME_ZONE_INFORMATION::default();
    // SAFETY: This Win32 API fills our writable result structure synchronously.
    let season = unsafe { GetTimeZoneInformation(&mut zone) };
    let seasonal_bias = match season {
        TIME_ZONE_ID_STANDARD => zone.StandardBias,
        TIME_ZONE_ID_DAYLIGHT => zone.DaylightBias,
        TIME_ZONE_ID_UNKNOWN => 0,
        _ => return 0,
    };
    zone.Bias.saturating_add(seasonal_bias).saturating_mul(-60)
}
