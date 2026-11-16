//! Resolve the initial Platform document from WebView2's top-level navigation.

use std::{cell::RefCell, rc::Rc};
use tokio::sync::oneshot;
use url::{Origin, Url};
use webview2_com::{
    CoTaskMemPWSTR,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_WEB_ERROR_STATUS, COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED,
        ICoreWebView2,
    },
    NavigationCompletedEventHandler, NavigationStartingEventHandler,
};
use windows_core::{BOOL, PWSTR, Result as WinResult};

struct Observation {
    navigation: Option<u64>,
    report: Option<oneshot::Sender<bool>>,
}

impl Observation {
    fn finish(&mut self, loaded: bool) {
        if let Some(report) = self.report.take() {
            let _ = report.send(loaded);
        }
    }
}

pub(super) fn install(
    core: &ICoreWebView2,
    origin: Origin,
    report: oneshot::Sender<bool>,
) -> WinResult<()> {
    let observation = Rc::new(RefCell::new(Observation {
        navigation: None,
        report: Some(report),
    }));
    let starting = NavigationStartingEventHandler::create(Box::new({
        let observation = observation.clone();
        move |_, args| {
            let Some(args) = args else {
                return Ok(());
            };
            if observation.borrow().report.is_none() {
                return Ok(());
            }
            let mut id = 0;
            // SAFETY: The event arguments are live and the output slot is writable.
            unsafe { args.NavigationId(&mut id) }?;
            let mut uri = PWSTR::null();
            // SAFETY: The slot receives an owned string, freed by the guard below.
            let result = unsafe { args.Uri(&mut uri) };
            let uri = CoTaskMemPWSTR::from(uri);
            result?;
            let trusted = Url::parse(&uri.to_string()).is_ok_and(|target| {
                target.origin() == origin && super::super::uncredentialed(&target)
            });
            if trusted {
                observation.borrow_mut().navigation = Some(id);
            } else if observation.borrow().navigation == Some(id) {
                // Redirects keep their navigation ID. Reject this one before
                // the network can send any part of the initial request again.
                // SAFETY: This is the cancellable native navigation event.
                unsafe { args.SetCancel(true) }?;
                observation.borrow_mut().finish(false);
            }
            Ok(())
        }
    }));
    let completed = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        if observation.borrow().report.is_none() {
            return Ok(());
        }
        let mut id = 0;
        // SAFETY: The event arguments are live and the output slot is writable.
        unsafe { args.NavigationId(&mut id) }?;
        if observation.borrow().navigation != Some(id) {
            return Ok(());
        }
        let mut loaded = BOOL::default();
        // SAFETY: The live completion event writes its success flag to `loaded`.
        unsafe { args.IsSuccess(&mut loaded) }?;
        if !loaded.as_bool() {
            let mut error = COREWEBVIEW2_WEB_ERROR_STATUS::default();
            // SAFETY: The native completion writes its error status to this slot.
            unsafe { args.WebErrorStatus(&mut error) }?;
            if std::env::var_os("KWORK_PLATFORM_TRACE").is_some() {
                eprintln!(
                    "platform navigation {id} failed with WebView2 status {}",
                    error.0
                );
            }
            if error == COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED {
                // The next trusted navigation may be replacing this document.
                observation.borrow_mut().navigation = None;
                return Ok(());
            }
        }
        observation.borrow_mut().finish(loaded.as_bool());
        Ok(())
    }));
    let mut token = 0;
    // SAFETY: The live core retains both handlers on this WebView's apartment.
    unsafe { core.add_NavigationStarting(&starting, &mut token) }?;
    // SAFETY: The token is writable through registration and the view owns the handler.
    unsafe { core.add_NavigationCompleted(&completed, &mut token) }
}
