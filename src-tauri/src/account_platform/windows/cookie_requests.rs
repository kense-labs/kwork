//! Apply deployment cookies after Chromium has assembled its cookie header.
//!
//! WebResourceRequested precedes the native cookie store and cannot override a
//! Cookie header once the server has set a cookie. Fetch interception operates
//! on the assembled request and also exposes each redirect separately. Never
//! put deployment credentials in CookieManager: cookies do not isolate ports.
use super::{
    super::{cookies, uncredentialed},
    Completion,
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
use url::{Origin, Url};
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, CoTaskMemPWSTR,
    DevToolsProtocolEventReceivedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2_11, ICoreWebView2DevToolsProtocolEventReceivedEventArgs,
        ICoreWebView2DevToolsProtocolEventReceivedEventArgs2,
    },
};
use windows_core::{Error, HRESULT, HSTRING, Interface, PWSTR, Result as WinResult};

type Done = Rc<RefCell<Option<Box<dyn FnOnce(WinResult<()>)>>>>;
type Commands = VecDeque<(&'static str, Value)>;
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
fn session(args: &ICoreWebView2DevToolsProtocolEventReceivedEventArgs) -> WinResult<String> {
    let args = args.cast::<ICoreWebView2DevToolsProtocolEventReceivedEventArgs2>()?;
    let mut text = PWSTR::null();
    // SAFETY: This writable slot receives an owned COM string on this apartment.
    let result = unsafe { args.SessionId(&mut text) };
    let text = CoTaskMemPWSTR::from(text);
    result?;
    Ok(text.to_string())
}
fn call(
    core: &ICoreWebView2,
    session: &str,
    method: &str,
    params: Value,
    done: impl FnOnce(WinResult<()>) + 'static,
) -> WinResult<()> {
    let callback =
        CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, _| {
            done(result);
            Ok(())
        }));
    // SAFETY: Commands and callbacks stay on the WebView's creating apartment.
    // WebView2 copies the strings and retains the completion callback.
    unsafe {
        core.cast::<ICoreWebView2_11>()?
            .CallDevToolsProtocolMethodForSession(
                &HSTRING::from(session),
                &HSTRING::from(method),
                &HSTRING::from(params.to_string()),
                &callback,
            )
    }
}
fn finish(done: &Done, result: WinResult<()>) {
    if let Some(callback) = done.borrow_mut().take() {
        callback(result);
    }
}
fn configure_session(core: ICoreWebView2, session: String, mut commands: Commands, done: Done) {
    let Some((method, params)) = commands.pop_front() else {
        finish(&done, Ok(()));
        return;
    };
    let next_core = core.clone();
    let next_session = session.clone();
    let next_done = done.clone();
    let result = call(
        &core,
        &session,
        method,
        params,
        move |result| match result {
            Ok(()) => configure_session(next_core, next_session, commands, next_done),
            Err(error) => finish(&next_done, Err(error)),
        },
    );
    if let Err(error) = result {
        finish(&done, Err(error));
    }
}
fn commands(worker: bool) -> Commands {
    let mut commands = VecDeque::from([
        (
            "Fetch.enable",
            json!({ "patterns": [
            { "urlPattern": "http://*", "requestStage": "Request" },
            { "urlPattern": "https://*", "requestStage": "Request" },
        ] }),
        ),
        (
            "Target.setAutoAttach",
            json!({ "autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true }),
        ),
    ]);
    if worker {
        commands.push_back(("Runtime.runIfWaitingForDebugger", json!({})));
    }
    commands
}

pub(super) fn install(
    core: &ICoreWebView2,
    origin: Origin,
    cookie: String,
    ready: Completion,
) -> WinResult<()> {
    // The WebView owns these receivers. Callbacks use the supplied sender, so
    // they do not hold a permanent COM reference cycle back to the WebView.
    let attached =
        DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |sender, args| {
            let (Some(core), Some(args)) = (sender, args) else {
                return Ok(());
            };
            let event = parameters(&args)?;
            let session = event["sessionId"]
                .as_str()
                .ok_or_else(invalid_event)?
                .to_owned();
            let done: Done = Rc::new(RefCell::new(Some(Box::new(|result| {
                if result.is_err() && std::env::var_os("KWORK_PLATFORM_TRACE").is_some() {
                    eprintln!(
                        "platform child request interception failed; target remains suspended"
                    );
                }
            }))));
            configure_session(core, session, commands(true), done);
            Ok(())
        }));
    // SAFETY: Receiver registration borrows the live WebView on this apartment.
    let receiver = unsafe {
        core.GetDevToolsProtocolEventReceiver(&HSTRING::from("Target.attachedToTarget"))
    }?;
    let mut subscription = 0;
    unsafe { receiver.add_DevToolsProtocolEventReceived(&attached, &mut subscription) }?;

    let paused =
        DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |sender, args| {
            let (Some(core), Some(args)) = (sender, args) else {
                return Ok(());
            };
            let event = parameters(&args)?;
            let request_id = event["requestId"].as_str().ok_or_else(invalid_event)?;
            let session = session(&args)?;
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
            let mut headers: Vec<_> = headers
                .iter()
                .filter(|(name, _)| !name.eq_ignore_ascii_case("cookie"))
                .filter_map(|(name, value)| {
                    value
                        .as_str()
                        .map(|value| json!({"name": name, "value": value}))
                })
                .collect();
            headers.push(json!({"name": "Cookie", "value": cookie}));
            call(
                &core,
                &session,
                "Fetch.continueRequest",
                json!({"requestId": request_id, "headers": headers}),
                |result| {
                    if result.is_err() && std::env::var_os("KWORK_PLATFORM_TRACE").is_some() {
                        eprintln!("platform paused request could not continue");
                    }
                },
            )
        }));
    // SAFETY: The receiver delivers events for the root and flattened child sessions.
    let receiver =
        unsafe { core.GetDevToolsProtocolEventReceiver(&HSTRING::from("Fetch.requestPaused")) }?;
    unsafe { receiver.add_DevToolsProtocolEventReceived(&paused, &mut subscription) }?;
    let done: Done = Rc::new(RefCell::new(Some(Box::new(move |result| {
        let _ = ready.send(result);
    }))));
    configure_session(core.clone(), String::new(), commands(false), done);
    Ok(())
}
