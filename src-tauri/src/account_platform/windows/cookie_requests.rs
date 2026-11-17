//! Single owner for platform request headers when deployment cookies are configured.
//!
//! WebView2 validation 43966893018 still fails after the server sets a cookie:
//! its cookie store can overwrite even this Fetch override. This implementation
//! is not accepted as cookie parity; see docs/parity-rc3.md. Never move deployment
//! credentials into CookieManager to mask the failure: cookies do not isolate ports.
use super::{
    super::{cookies, uncredentialed},
    Completion, network,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use url::{Origin, Url};
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, CoTaskMemPWSTR,
    DevToolsProtocolEventReceivedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2DevToolsProtocolEventReceivedEventArgs,
    },
};
use windows_core::{Error, HRESULT, HSTRING, PWSTR, Result as WinResult};

fn invalid_event() -> Error {
    Error::from_hresult(HRESULT(0x80004005u32 as i32))
}

fn parameters(args: &ICoreWebView2DevToolsProtocolEventReceivedEventArgs) -> WinResult<Value> {
    let mut text = PWSTR::null();
    // SAFETY: The event owns its parameters until this callback returns. The
    // CoTaskMem guard frees the returned string, including on a failed call.
    let result = unsafe { args.ParameterObjectAsJson(&mut text) };
    let text = CoTaskMemPWSTR::from(text);
    result?;
    serde_json::from_str(&text.to_string()).map_err(|_| invalid_event())
}
fn call(
    core: &ICoreWebView2,
    method: &'static str,
    params: Value,
    done: impl FnOnce(WinResult<()>) + 'static,
) -> WinResult<()> {
    let callback =
        CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, _| {
            if let Err(error) = &result
                && std::env::var_os("KWORK_PLATFORM_TRACE").is_some()
            {
                // Method names and HRESULTs cannot expose request URLs or credentials.
                eprintln!("platform CDP {method} failed: {:?}", error.code());
            }
            done(result);
            Ok(())
        }));
    // SAFETY: Commands and callbacks stay on the WebView's creating apartment.
    // WebView2 copies the strings and retains the completion callback.
    unsafe {
        core.CallDevToolsProtocolMethod(
            &HSTRING::from(method),
            &HSTRING::from(params.to_string()),
            &callback,
        )
    }
}

pub(super) fn install(
    core: &ICoreWebView2,
    origin: Origin,
    mut deployment: BTreeMap<String, String>,
    ready: Completion,
) -> WinResult<()> {
    let cookie = deployment.remove("cookie").unwrap_or_default();
    network::add_identity_headers(&mut deployment);
    // The root Fetch handler also covers its workers' requests. Enabling Fetch
    // in a worker target is unsupported and suspending that target deadlocks it.
    // Do not additionally edit headers through WebResourceRequested: WebView2
    // assembles that collection separately and can replace the CDP Cookie edit.
    let paused =
        DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |sender, args| {
            let (Some(core), Some(args)) = (sender, args) else {
                return Ok(());
            };
            let event = parameters(&args)?;
            let request_id = event["requestId"].as_str().ok_or_else(invalid_event)?;
            let request = &event["request"];
            let trusted = request["url"]
                .as_str()
                .and_then(|url| Url::parse(url).ok())
                .is_some_and(|url| url.origin() == origin && uncredentialed(&url));
            let headers = request["headers"].as_object().ok_or_else(invalid_event)?;
            let base = headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("cookie"))
                .and_then(|(_, value)| value.as_str())
                .unwrap_or_default();
            let cookie = if trusted {
                cookies::merge(base, &cookie)
            } else {
                cookies::strip_deployment(base, &cookie)
            };
            let mut headers: BTreeMap<String, String> = headers
                .iter()
                .filter(|(name, _)| !name.eq_ignore_ascii_case("cookie"))
                .filter_map(|(name, value)| {
                    value
                        .as_str()
                        .map(|value| (name.to_lowercase(), value.into()))
                })
                .collect();
            if trusted {
                headers.extend(deployment.clone());
                headers.insert(
                    "x-client-timezone-offset".into(),
                    network::timezone_seconds().to_string(),
                );
            } else {
                for (name, value) in &deployment {
                    if headers
                        .get(name)
                        .is_some_and(|actual| actual.trim() == value.trim())
                    {
                        headers.remove(name);
                    }
                }
                headers.remove("x-client-timezone-offset");
                headers.remove("x-dsh-auth-token");
            }
            headers.insert("cookie".into(), cookie);
            let headers: Vec<_> = headers
                .into_iter()
                .map(|(name, value)| json!({"name":name,"value":value}))
                .collect();
            call(
                &core,
                "Fetch.continueRequest",
                json!({"requestId": request_id, "headers": headers}),
                |_| {},
            )
        }));
    // SAFETY: The root receiver is owned by this dedicated platform WebView.
    let receiver =
        unsafe { core.GetDevToolsProtocolEventReceiver(&HSTRING::from("Fetch.requestPaused")) }?;
    let mut subscription = 0;
    unsafe { receiver.add_DevToolsProtocolEventReceived(&paused, &mut subscription) }?;
    call(
        core,
        "Fetch.enable",
        json!({ "patterns": [
        { "urlPattern": "http://*", "requestStage": "Request" },
        { "urlPattern": "https://*", "requestStage": "Request" },
    ] }),
        move |result| {
            let _ = ready.send(result);
        },
    )
}
