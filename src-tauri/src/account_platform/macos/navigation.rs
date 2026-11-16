//! Observe the native result of opening a Platform document.

use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class,
    ffi::{OBJC_ASSOCIATION_RETAIN_NONATOMIC, objc_setAssociatedObject},
    msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel},
    sel,
};
use objc2_foundation::{NSError, NSURLErrorCancelled, NSURLRequest, ns_string};
use objc2_web_kit::{WKNavigation, WKNavigationDelegate, WKWebView};
use std::{cell::RefCell, ptr};
use tokio::sync::oneshot;

struct Observation {
    navigation: Retained<WKNavigation>,
    failed: oneshot::Sender<()>,
}

struct ObserverIvars {
    downstream: Retained<ProtocolObject<dyn WKNavigationDelegate>>,
    initial: RefCell<Option<Observation>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "KWorkPlatformNavigationObserver"]
    #[ivars = ObserverIvars]
    struct Observer;

    impl Observer {
        #[unsafe(method_id(forwardingTargetForSelector:))]
        fn forwarding_target(&self, selector: Sel) -> Option<Retained<AnyObject>> {
            self.downstream(selector).map(AsRef::<AnyObject>::as_ref).map(Message::retain)
        }
    }

    // This NSObject implements optional delegate callbacks and participates in message forwarding.
    unsafe impl NSObjectProtocol for Observer {
        #[unsafe(method(respondsToSelector:))]
        fn responds_to_selector(&self, selector: Sel) -> bool {
            // SAFETY: NSObject implements this query for any selector.
            let own: bool = unsafe { msg_send![super(self), respondsToSelector: selector] };
            own || self.downstream(selector).is_some()
        }
    }

    // WebKit calls the observers on the owning main thread.
    unsafe impl WKNavigationDelegate for Observer {
        #[unsafe(method(webView:didFinishNavigation:))]
        fn finished(&self, view: &WKWebView, navigation: Option<&WKNavigation>) {
            self.take_observation(navigation);
            if let Some(delegate) = self.downstream(sel!(webView:didFinishNavigation:)) {
                // SAFETY: The downstream implements this selector. WebKit supplied the arguments.
                unsafe { delegate.webView_didFinishNavigation(view, navigation) };
            }
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn provisional_failure(&self, view: &WKWebView, navigation: Option<&WKNavigation>, error: &NSError) {
            if let Some(delegate) = self.downstream(sel!(webView:didFailProvisionalNavigation:withError:)) {
                // SAFETY: The downstream implements this selector. WebKit supplied the arguments.
                unsafe { delegate.webView_didFailProvisionalNavigation_withError(view, navigation, error) };
            }
            self.failed(navigation, error);
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn committed_failure(&self, view: &WKWebView, navigation: Option<&WKNavigation>, error: &NSError) {
            if let Some(delegate) = self.downstream(sel!(webView:didFailNavigation:withError:)) {
                // SAFETY: The downstream implements this selector. WebKit supplied the arguments.
                unsafe { delegate.webView_didFailNavigation_withError(view, navigation, error) };
            }
            self.failed(navigation, error);
        }
    }
);

impl Observer {
    fn downstream(&self, selector: Sel) -> Option<&ProtocolObject<dyn WKNavigationDelegate>> {
        let delegate = &*self.ivars().downstream;
        delegate.respondsToSelector(selector).then_some(delegate)
    }

    fn take_observation(&self, navigation: Option<&WKNavigation>) -> Option<Observation> {
        let mut initial = self.ivars().initial.borrow_mut();
        if let (Some(active), Some(navigation)) = (initial.as_ref(), navigation)
            && ptr::eq(&*active.navigation, navigation)
        {
            return initial.take();
        }
        None
    }

    fn failed(&self, navigation: Option<&WKNavigation>, error: &NSError) {
        let Some(observation) = self.take_observation(navigation) else {
            return;
        };
        // A page may replace its own URL while its first navigation is still loading.
        if error.code() != NSURLErrorCancelled || &*error.domain() != ns_string!("NSURLErrorDomain")
        {
            let _ = observation.failed.send(());
        }
    }
}

pub(super) fn load(
    view: &WKWebView,
    request: &NSURLRequest,
    failed: oneshot::Sender<()>,
    mtm: MainThreadMarker,
) -> bool {
    static OBSERVER_KEY: u8 = 0;
    // SAFETY: The view belongs to the current main thread and has Wry's navigation policy.
    let Some(downstream) = (unsafe { view.navigationDelegate() }) else {
        return false;
    };
    let allocated = Observer::alloc(mtm).set_ivars(ObserverIvars {
        downstream,
        initial: RefCell::default(),
    });
    // SAFETY: The superclass initializes this NSObject with its ivars already set.
    let observer: Retained<Observer> = unsafe { msg_send![super(allocated), init] };
    // SAFETY: Both objects are live on the same thread. The key is static; the
    // association retains the observer for the weak delegate's lifetime.
    unsafe {
        objc_setAssociatedObject(
            ptr::from_ref(view).cast_mut().cast(),
            ptr::from_ref(&OBSERVER_KEY).cast(),
            Retained::as_ptr(&observer).cast_mut().cast(),
            OBJC_ASSOCIATION_RETAIN_NONATOMIC,
        );
    }
    // SAFETY: The observer implements this protocol and the associated object retains it.
    unsafe { view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*observer))) };
    // SAFETY: The request and view are live for this main-thread call.
    let Some(navigation) = (unsafe { view.loadRequest(request) }) else {
        return false;
    };
    observer
        .ivars()
        .initial
        .replace(Some(Observation { navigation, failed }));
    true
}
