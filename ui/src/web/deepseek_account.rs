//! DeepSeek account settings and browser authorization backed by the ACP Host.

use super::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[wasm_bindgen(module = "/deepseek-account-form.js")]
extern "C" {
    fn update_deepseek_account_form(value: JsValue);
    fn place_deepseek_account_form(left: f32, top: f32, width: f32, height: f32);
    fn update_account_launcher(value: JsValue);
    fn update_account_notices(value: JsValue);
    fn update_account_quota_notice(id: &str, kind: &str, dark: bool);
    pub(super) fn place_account_notice_cover(x: f32, y: f32, width: f32, height: f32);
    fn update_account_avatar(url: &str, retry: u32);
    fn place_account_launcher(left: f32, top: f32, width: f32, height: f32);
    fn focus_account_launcher();
    fn account_contact_url(id: &str, version: &str) -> String;
    fn account_authorization_url(url: &str, dark: bool) -> String;
    pub(super) fn listen_deepseek_account_form(callback: &js_sys::Function);
}

#[wasm_bindgen(module = "/account-platform-view.js")]
extern "C" {
    fn update_account_platform_view(page: &str, dark: bool, shell: f64);
    fn close_account_platform_view();
    fn account_platform_status(id: u32, status: &str, shell: f64);
    pub(super) fn listen_account_platform_view(callback: &js_sys::Function);
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountState {
    status: String,
    links: AccountLinks,
    attempt: Option<SignInAttempt>,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountLinks {
    usage_url: Option<String>,
    top_up_url: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SignInAttempt {
    id: String,
    phase: String,
    authorize_url: Option<String>,
    error_code: Option<String>,
}

impl SignInAttempt {
    fn running(&self) -> bool {
        matches!(
            self.phase.as_str(),
            "initializing" | "waiting-browser" | "exchanging" | "committing"
        )
    }
}

#[derive(Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
struct AccountDetails {
    profile: Option<AccountProfile>,
    balance: Option<AccountBalance>,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
struct AccountProfile {
    value: Option<AccountIdentity>,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountIdentity {
    id: Option<String>,
    name: Option<String>,
    contact: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountBalance {
    value: Option<Vec<Wallet>>,
    #[serde(default)]
    bonus_wallets: Vec<Wallet>,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
struct Wallet {
    currency: String,
    balance: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountReply {
    available: bool,
    notifications: Option<NotificationFeed>,
    #[serde(default)]
    platform_embedded: bool,
    #[serde(default)]
    model_ready: bool,
    state: Option<AccountState>,
    details: Option<AccountDetails>,
    #[serde(default)]
    running_tasks: bool,
    #[serde(default)]
    needs_confirmation: bool,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct NotificationFeed {
    scope: String,
    mode: String,
    poll_interval_ms: u32,
}

#[derive(PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct NotificationView {
    feed: Option<NotificationFeed>,
    visible: bool,
    interactive: bool,
    settings_open: bool,
    dark: bool,
    refresh: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlatformPage {
    Usage,
    TopUp,
}

impl PlatformPage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::TopUp => "top-up",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum LauncherDialog {
    SignIn,
    SignOut,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SignOutImpact {
    Idle,
    Running,
    Unknown,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountForm {
    dark: bool,
    onboarding: bool,
    launcher_dialog: Option<LauncherDialog>,
    state: Option<AccountState>,
    details: Option<AccountDetails>,
    busy: Option<&'static str>,
    confirm_signout: Option<SignOutImpact>,
    error: Option<String>,
}

#[derive(PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountLauncher {
    dark: bool,
    compact: bool,
    open: bool,
    signed_in: bool,
    label: Option<String>,
}

#[derive(Default)]
pub(super) struct AccountPanel {
    pub(super) available: bool,
    pub(super) welcome_open: bool,
    platform_embedded: bool,
    platform_page: Option<PlatformPage>,
    account_status_loaded: bool,
    quota_revision: u32,
    quota_notice: Option<QuotaFailure>,
    quota_top_up: bool,
    state: Option<AccountState>,
    details: Option<AccountDetails>,
    loading_details: bool,
    refresh_on_completion: bool,
    details_revision: u32,
    busy: Option<&'static str>,
    confirm_signout: Option<SignOutImpact>,
    launcher_dialog: Option<LauncherDialog>,
    pub(super) launcher_compact: Option<bool>,
    launcher_view: Option<AccountLauncher>,
    notification_feed: Option<NotificationFeed>,
    notification_view: Option<NotificationView>,
    notification_refresh: u32,
    avatar_retry: u32,
    error: Option<String>,
    generation: u32,
    poll_scheduled: bool,
    launch_browser: bool,
    cancel_on_start: bool,
    refresh_queued: bool,
    launcher_action_queued: Option<&'static str>,
    view: Option<AccountForm>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum QuotaFailure {
    Provider,
    Account,
}

impl AccountPanel {
    pub(super) fn signed_in(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(|state| state.status == "credential-stored")
    }

    fn active_attempt(&self) -> Option<&SignInAttempt> {
        self.state
            .as_ref()?
            .attempt
            .as_ref()
            .filter(|attempt| attempt.running())
    }

    fn accept(&mut self, reply: AccountReply) {
        let previous = self.signed_in();
        self.account_status_loaded = true;
        self.available = reply.available;
        self.notification_feed = reply.notifications;
        self.platform_embedded = reply.platform_embedded;
        self.state = reply.state;
        if !self.signed_in() {
            self.platform_page = None;
            if previous {
                self.clear_account_quota();
            }
        }
        if previous != self.signed_in() {
            self.invalidate_details();
        }
    }

    fn clear_account_quota(&mut self) {
        if self.quota_notice == Some(QuotaFailure::Account) {
            self.quota_notice = None;
            self.quota_top_up = false;
        }
    }

    fn invalidate_details(&mut self) {
        self.details = None;
        self.loading_details = false;
        self.refresh_on_completion = false;
        self.details_revision = self.details_revision.wrapping_add(1);
    }

    fn profile(&self) -> Option<&AccountIdentity> {
        self.details.as_ref()?.profile.as_ref()?.value.as_ref()
    }

    pub(super) fn has_positive_balance(&self) -> Option<bool> {
        let balance = self.details.as_ref()?.balance.as_ref()?;
        Some(
            balance
                .value
                .as_ref()?
                .iter()
                .chain(&balance.bonus_wallets)
                .any(|wallet| {
                    let amount = wallet.balance.trim();
                    !amount.starts_with('-')
                        && amount.split(['e', 'E']).next().is_some_and(|mantissa| {
                            mantissa.bytes().any(|digit| matches!(digit, b'1'..=b'9'))
                        })
                }),
        )
    }

    pub(super) fn can_top_up(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(|state| state.links.top_up_url.is_some())
    }
}

impl DshApp {
    pub(super) fn show_quota_failure(&mut self, code: &str) {
        let panel = &mut self.deepseek_account;
        if panel.quota_top_up {
            return;
        }
        let failure = match code {
            "QUOTA" => QuotaFailure::Provider,
            "ACCOUNT_QUOTA" => QuotaFailure::Account,
            _ => return,
        };
        panel.quota_revision = panel.quota_revision.wrapping_add(1);
        panel.quota_notice = Some(failure);
    }

    fn quota_notice_kind(&self) -> &'static str {
        let panel = &self.deepseek_account;
        match panel.quota_notice {
            None => "",
            Some(QuotaFailure::Provider) => "toast",
            Some(QuotaFailure::Account) if self.browser_shell.is_some() => {
                if !panel.account_status_loaded || panel.quota_top_up {
                    ""
                } else if !panel.platform_embedded || !panel.signed_in() {
                    "toast"
                } else if panel.platform_page.is_none() {
                    "balance"
                } else {
                    ""
                }
            }
            Some(QuotaFailure::Account) => "toast",
        }
    }

    pub(super) fn account_quota_modal_open(&self) -> bool {
        self.quota_notice_kind() == "balance"
    }

    pub(super) fn handle_quota_notice(&mut self, action: &str, id: &str, cx: &mut Context<Self>) {
        let kind = self.quota_notice_kind();
        if kind.is_empty() || id.parse::<u32>().ok() != Some(self.deepseek_account.quota_revision) {
            return;
        }
        if action == "account-quota-top-up" {
            if kind != "balance" {
                return;
            }
            self.deepseek_account.quota_top_up = true;
            self.open_account_top_up(cx);
        } else {
            self.deepseek_account.quota_notice = None;
        }
        cx.notify();
    }

    pub(super) fn account_platform_open(&self) -> bool {
        self.deepseek_account.platform_page.is_some()
    }

    pub(super) fn close_account_platform(&self) {
        close_account_platform_view();
    }

    pub(super) fn handle_account_platform_event(&self, event: &Value) {
        if self.account_platform_open()
            && let Some(id) = event["id"].as_u64().and_then(|id| u32::try_from(id).ok())
            && let Some(status) = event["status"].as_str()
            && let Some(shell) = event["shell"].as_u64()
        {
            account_platform_status(id, status, shell as f64);
        }
    }

    pub(super) fn reset_account_connection(&mut self) {
        let panel = &mut self.deepseek_account;
        *panel = AccountPanel {
            generation: panel.generation.wrapping_add(1),
            quota_revision: panel.quota_revision,
            view: panel.view.take(),
            launcher_view: panel.launcher_view.take(),
            notification_view: panel.notification_view.take(),
            ..Default::default()
        };
        self.show_more_menu = false;
        if self.settings_page == SettingsPage::Account {
            self.settings_page = SettingsPage::Credentials;
        }
    }

    pub(super) fn load_startup_account(
        &mut self,
        reply: &Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reset_account_connection();
        self.deepseek_account.account_status_loaded = true;
        if let Ok(account) =
            serde_json::from_value::<AccountReply>(reply["deepseekAccount"].clone())
        {
            let ready = account.model_ready;
            self.deepseek_account.accept(account);
            if ready {
                self.welcome
                    .update(cx, |welcome, cx| welcome.use_saved_models(w, cx));
            }
            self.refresh_account_details(w, cx);
        }
    }

    pub(super) fn render_account_launcher(
        &mut self,
        compact: bool,
        width: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        self.deepseek_account.launcher_compact = Some(compact);
        let height = if compact {
            36.
        } else if self.deepseek_account.signed_in() {
            44.
        } else {
            32.
        };
        div()
            .id(if compact {
                "rail-account-launcher"
            } else {
                "sidebar-account-launcher"
            })
            .w(px(width))
            .h(px(height))
            .flex_none()
            .relative()
            .child(
                gpui::canvas(
                    |bounds, _, _| {
                        place_account_launcher(
                            bounds.left().as_f32(),
                            bounds.top().as_f32(),
                            bounds.size.width.as_f32(),
                            bounds.size.height.as_f32(),
                        );
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .when(!compact && self.sidebar_drawer, |div| {
                div.track_focus(&self.sidebar_drawer_focus.more)
                    .tab_index(0)
                    .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, w, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            focus_account_launcher();
                            w.prevent_default();
                            cx.stop_propagation();
                        }
                    }))
            })
    }

    pub(super) fn render_account_panel(&self) -> gpui::Div {
        div().size_full().relative().child(
            gpui::canvas(
                |bounds, _, _| {
                    place_deepseek_account_form(
                        bounds.left().as_f32(),
                        bounds.top().as_f32(),
                        bounds.size.width.as_f32(),
                        bounds.size.height.as_f32(),
                    )
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
    }

    pub(super) fn account_confirmation_open(&self) -> bool {
        self.settings_page == SettingsPage::Account
            && self.deepseek_account.confirm_signout.is_some()
    }

    pub(super) fn account_launcher_modal(&self) -> bool {
        self.deepseek_account.launcher_dialog.is_some()
    }

    pub(super) fn open_account_menu_settings(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        self.open_settings_from_more(cx);
        self.deepseek_account.avatar_retry = self.deepseek_account.avatar_retry.wrapping_add(1);
        self.refresh_account_details(w, cx);
    }

    pub(super) fn handle_account_launcher(
        &mut self,
        action: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.deepseek_account.available || !self.workspace_tree_available() {
            return;
        }
        if action == "account-menu-toggle" {
            self.toggle_more_settings(cx);
            return;
        }
        if !self.show_more_menu {
            return;
        }
        self.show_more_menu = false;
        match action {
            "account-menu-settings" => self.open_account_menu_settings(w, cx),
            "account-menu-contact" => {
                let id = self
                    .deepseek_account
                    .profile()
                    .and_then(|profile| profile.id.as_deref());
                self.open_account_url(
                    account_contact_url(id.unwrap_or_default(), env!("CARGO_PKG_VERSION")),
                    cx,
                );
            }
            "account-menu-sign-in" if !self.deepseek_account.signed_in() => {
                self.deepseek_account.launcher_dialog = Some(LauncherDialog::SignIn);
                if self.deepseek_account.active_attempt().is_none() {
                    self.request_account("start", w, cx);
                }
            }
            "account-menu-signout" if self.deepseek_account.signed_in() => {
                self.deepseek_account.launcher_dialog = Some(LauncherDialog::SignOut);
                self.request_account("check-signout", w, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn publish_account_panel(&mut self, cx: &App) {
        let interactive = self.workspace_tree_available();
        let notification_visible = self.deepseek_account.launcher_compact == Some(false)
            && !self.account_platform_open()
            && !self.show_welcome
            && !self.desktop_onboarding_visible();
        let launcher = self
            .deepseek_account
            .launcher_compact
            .filter(|_| interactive);
        let dark = workspace::dark(cx);
        let quota_kind = self.quota_notice_kind();
        let quota_id = self.deepseek_account.quota_revision.to_string();
        let platform_open = self.account_platform_open();
        // The prior dialog must release focus before the native view opens.
        // On return the native view must release focus before a deferred dialog.
        if platform_open {
            update_account_quota_notice(&quota_id, quota_kind, dark);
        }
        let panel = &mut self.deepseek_account;
        update_account_platform_view(
            panel
                .platform_page
                .map(PlatformPage::as_str)
                .unwrap_or_default(),
            dark,
            self.browser_shell.unwrap_or_default() as f64,
        );
        if !platform_open {
            update_account_quota_notice(&quota_id, quota_kind, dark);
        }
        update_account_avatar(
            panel
                .profile()
                .and_then(|profile| profile.avatar_url.as_deref())
                .unwrap_or_default(),
            panel.avatar_retry,
        );
        let launcher = launcher.map(|compact| {
            let profile = panel.profile();
            AccountLauncher {
                dark,
                compact,
                open: self.show_more_menu,
                signed_in: panel.signed_in(),
                label: panel.details.as_ref().map(|_| {
                    profile
                        .and_then(|profile| {
                            profile.name.clone().or_else(|| profile.contact.clone())
                        })
                        .unwrap_or_else(|| "已登录".into())
                }),
            }
        });
        if panel.launcher_view != launcher
            && let Ok(value) = serde_wasm_bindgen::to_value(&launcher)
        {
            panel.launcher_view = launcher;
            update_account_launcher(value);
        }
        let notices = NotificationView {
            feed: panel.notification_feed.clone(),
            visible: notification_visible,
            interactive,
            settings_open: self.show_settings,
            dark,
            refresh: panel.notification_refresh,
        };
        if panel.notification_view.as_ref() != Some(&notices)
            && let Ok(value) = serde_wasm_bindgen::to_value(&notices)
        {
            panel.notification_view = Some(notices);
            update_account_notices(value);
        }
        let visible = panel.welcome_open
            || panel.launcher_dialog.is_some()
            || self.show_settings && self.settings_page == SettingsPage::Account;
        let next = visible.then(|| AccountForm {
            dark,
            onboarding: panel.welcome_open,
            launcher_dialog: panel.launcher_dialog,
            state: panel.state.clone(),
            details: panel.details.clone(),
            busy: panel
                .busy
                .filter(|action| *action != "status")
                .or(panel.loading_details.then_some("details")),
            confirm_signout: panel.confirm_signout,
            error: panel.error.clone(),
        });
        if panel.view != next
            && let Ok(value) = serde_wasm_bindgen::to_value(&next)
        {
            panel.view = next;
            update_deepseek_account_form(value);
        }
    }

    pub(super) fn open_account_settings(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if !self.deepseek_account.available {
            return;
        }
        self.settings_page = SettingsPage::Account;
        if self.deepseek_account.signed_in() {
            if self.deepseek_account.details.is_none() {
                self.refresh_account_details(w, cx);
            }
        } else {
            self.request_account("status", w, cx);
        }
        cx.notify();
    }

    pub(super) fn open_welcome_account(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.show_welcome && self.deepseek_account.available {
            self.deepseek_account.welcome_open = true;
            cx.notify();
            if self.deepseek_account.active_attempt().is_none()
                && !self.deepseek_account.signed_in()
            {
                self.request_account("start", w, cx);
            }
        }
    }

    pub(super) fn handle_account_form(
        &mut self,
        action: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.account_platform_open() {
            if action == "account-platform-revoked" {
                self.deepseek_account.platform_page = None;
                if self.deepseek_account.quota_top_up {
                    self.deepseek_account.clear_account_quota();
                }
                cx.notify();
            }
            if action == "account-platform-return" {
                let page = self.deepseek_account.platform_page.take();
                if self.deepseek_account.quota_top_up {
                    self.deepseek_account.clear_account_quota();
                }
                if page == Some(PlatformPage::TopUp) {
                    if self.show_settings {
                        self.refresh_account_notices();
                    }
                    if self.deepseek_account.loading_details {
                        self.deepseek_account.refresh_on_completion = true;
                    } else {
                        self.refresh_account_details(w, cx);
                    }
                }
                cx.notify();
            }
            return;
        }
        if !self.account_launcher_modal()
            && !self.deepseek_account.welcome_open
            && !(self.show_settings && self.settings_page == SettingsPage::Account)
        {
            return;
        }
        match action {
            "account-back" if self.deepseek_account.welcome_open => {
                self.deepseek_account.welcome_open = false;
                self.deepseek_account.launch_browser = false;
                self.deepseek_account.cancel_on_start = self.deepseek_account.busy == Some("start");
                self.request_account("cancel", w, cx);
            }
            "account-continue"
                if self.deepseek_account.welcome_open && self.deepseek_account.signed_in() =>
            {
                self.deepseek_account.welcome_open = false;
                self.welcome
                    .update(cx, |welcome, cx| welcome.continue_with_account(w, cx));
            }
            "account-close" if self.account_launcher_modal() => self.close_account_launcher(w, cx),
            "account-close" => self.close_settings(cx),
            "account-api-key"
                if self.deepseek_account.launcher_dialog == Some(LauncherDialog::SignIn) =>
            {
                self.close_account_launcher(w, cx);
                if !self.account_launcher_modal() {
                    self.open_settings_from_more(cx);
                }
            }
            "account-sign-in" if !self.deepseek_account.signed_in() => {
                self.request_account("start", w, cx)
            }
            "account-cancel" if self.account_launcher_modal() => self.close_account_launcher(w, cx),
            "account-cancel" => self.request_account("cancel", w, cx),
            "account-refresh" if self.deepseek_account.signed_in() => {
                self.refresh_account_notices();
                self.deepseek_account.avatar_retry =
                    self.deepseek_account.avatar_retry.wrapping_add(1);
                self.refresh_account_details(w, cx);
            }
            "account-refresh" => self.request_account("status", w, cx),
            "account-check-signout" if self.deepseek_account.signed_in() => {
                self.request_account("check-signout", w, cx)
            }
            "account-signout" if self.deepseek_account.confirm_signout.is_some() => {
                self.request_account("signout", w, cx)
            }
            "account-cancel-signout" => {
                if self.account_launcher_modal() {
                    self.close_account_launcher(w, cx);
                } else {
                    self.cancel_account_confirmation(cx);
                }
            }
            "account-usage" => self.open_account_page(PlatformPage::Usage, cx),
            "account-top-up" => self.open_account_top_up(cx),
            "account-open-browser" => {
                if let Some(url) = self
                    .deepseek_account
                    .active_attempt()
                    .and_then(|attempt| attempt.authorize_url.as_deref())
                {
                    self.open_account_url(account_authorization_url(url, workspace::dark(cx)), cx);
                }
            }
            _ => return,
        }
        cx.notify();
    }

    pub(super) fn open_account_from_notice(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.workspace_tree_available() && self.deepseek_account.notification_feed.is_some() {
            self.show_more_menu = false;
            self.show_settings = true;
            self.open_account_settings(w, cx);
        }
    }

    pub(super) fn has_account_notice_feed(&self) -> bool {
        self.deepseek_account.notification_feed.is_some()
    }

    fn refresh_account_notices(&mut self) {
        let panel = &mut self.deepseek_account;
        panel.notification_refresh = panel.notification_refresh.wrapping_add(1);
    }

    pub(super) fn open_account_top_up(&mut self, cx: &mut Context<Self>) {
        self.open_account_page(PlatformPage::TopUp, cx);
    }

    fn open_account_page(&mut self, page: PlatformPage, cx: &mut Context<Self>) {
        let panel = &mut self.deepseek_account;
        if !panel.signed_in() {
            return;
        }
        if panel.platform_embedded && self.browser_shell.is_some() {
            panel.platform_page = Some(page);
            cx.notify();
        } else if let Some(state) = &panel.state {
            let link = match page {
                PlatformPage::Usage => state.links.usage_url.clone(),
                PlatformPage::TopUp => state.links.top_up_url.clone(),
            };
            if let Some(link) = link {
                self.open_account_url(link, cx);
            }
        }
    }

    pub(super) fn close_account_launcher(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let panel = &mut self.deepseek_account;
        if matches!(
            panel.busy,
            Some("start" | "cancel" | "check-signout" | "signout")
        ) || panel
            .active_attempt()
            .is_some_and(|attempt| attempt.phase == "committing")
        {
            return;
        }
        panel.confirm_signout = None;
        panel.launcher_dialog = None;
        panel.launcher_action_queued = None;
        if panel.active_attempt().is_some() {
            panel.launch_browser = false;
            self.request_account("cancel", w, cx);
        }
        cx.notify();
    }

    pub(super) fn cancel_account_confirmation(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.account_confirmation_open() {
            return false;
        }
        if self.deepseek_account.busy != Some("signout") {
            self.deepseek_account.confirm_signout = None;
            cx.notify();
        }
        true
    }

    pub(super) fn account_changed(&mut self, reason: &str, w: &mut Window, cx: &mut Context<Self>) {
        if reason == "session-expired" {
            self.push_toast("DeepSeek 登录已失效，请重新登录。", cx);
        } else if reason == "model-sign-in-required" {
            self.push_toast("请在设置中登录 DeepSeek 账号。", cx);
        }
        if matches!(reason, "changed" | "session-expired") {
            if self.deepseek_account.signed_in() && !self.deepseek_account.quota_top_up {
                self.deepseek_account.clear_account_quota();
            }
            self.deepseek_account.notification_feed = None;
            self.deepseek_account.invalidate_details();
        }
        self.request_account("status", w, cx);
        self.model_catalog_changed(w, cx);
    }

    fn request_account(&mut self, action: &'static str, w: &mut Window, cx: &mut Context<Self>) {
        let connected = self.connected();
        let panel = &mut self.deepseek_account;
        if !connected {
            return;
        }
        if panel.busy.is_some() {
            panel.refresh_queued |= action == "status";
            panel.cancel_on_start |= action == "cancel";
            if panel.launcher_dialog.is_some() && matches!(action, "start" | "check-signout") {
                panel.launcher_action_queued = Some(action);
            }
            return;
        }
        let mut request = json!({"action": action});
        if action == "cancel" {
            let Some(attempt) = panel.active_attempt() else {
                return;
            };
            request["attempt"] = attempt.id.as_str().into();
        } else if action == "signout" {
            request["approveRunning"] =
                (panel.confirm_signout == Some(SignOutImpact::Running)).into();
        } else if action == "start" {
            panel.launch_browser = true;
            panel.cancel_on_start = false;
        } else if action == "status"
            && self.conversation.acp_open
            && !self.conversation.running
            && self.conversation.entries.is_empty()
            && let Some(session) = &self.active_session
        {
            request["sessionId"] = session.as_str().into();
        }
        panel.busy = Some(action);
        if action != "status" {
            panel.error = None;
        }
        let generation = panel.generation;
        cx.notify();
        cx.spawn_in(w, async move |this, cx| {
            let result = bridge::invoke_json("deepseek_account", json!({"request":request}))
                .await
                .and_then(|value| {
                    serde_json::from_value::<AccountReply>(value)
                        .map_err(|_| "无法读取 DeepSeek 账号状态。".into())
                });
            let _ = this.update_in(cx, |app, w, cx| {
                app.finish_account_request(generation, action, result, w, cx)
            });
        })
        .detach();
    }

    fn finish_account_request(
        &mut self,
        generation: u32,
        action: &str,
        result: Result<AccountReply, String>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = &mut self.deepseek_account;
        if panel.generation != generation {
            return;
        }
        panel.busy = None;
        let Ok(reply) = result else {
            panel.account_status_loaded |= action == "status";
            if action == "check-signout" && panel.signed_in() {
                panel.confirm_signout = Some(SignOutImpact::Unknown);
            } else {
                panel.error = result.err();
            }
            panel.launch_browser &= action != "start";
            panel.refresh_queued = false;
            if let Some(queued) = panel.launcher_action_queued.take() {
                self.request_account(queued, w, cx);
            }
            cx.notify();
            return;
        };
        let before = panel.signed_in();
        let ready = reply.model_ready;
        if (action == "check-signout" || reply.needs_confirmation)
            && reply
                .state
                .as_ref()
                .is_some_and(|state| state.status == "credential-stored")
        {
            panel.confirm_signout = Some(if reply.running_tasks {
                SignOutImpact::Running
            } else {
                SignOutImpact::Idle
            });
        } else if action == "signout" {
            panel.confirm_signout = None;
        }
        let retry_details = panel.details.is_none();
        panel.accept(reply);
        if (panel.launcher_dialog == Some(LauncherDialog::SignIn) && panel.signed_in())
            || (panel.launcher_dialog == Some(LauncherDialog::SignOut) && !panel.signed_in())
        {
            panel.launcher_dialog = None;
            panel.confirm_signout = None;
        }
        if !panel.signed_in() {
            panel.confirm_signout = None;
        }
        let running = panel.active_attempt().is_some();
        let changed = before != panel.signed_in();
        let refresh = std::mem::take(&mut panel.refresh_queued);
        let cancel = std::mem::take(&mut panel.cancel_on_start);
        let queued = panel.launcher_action_queued.take();
        let launch = panel
            .state
            .as_ref()
            .and_then(|state| state.attempt.as_ref())
            .and_then(|attempt| attempt.authorize_url.clone());
        if cancel && running {
            self.request_account("cancel", w, cx);
            return;
        }
        if panel.launch_browser
            && let Some(url) = launch
        {
            panel.launch_browser = false;
            self.open_account_url(account_authorization_url(&url, workspace::dark(cx)), cx);
        }
        if changed {
            self.model_catalog_changed(w, cx);
        }
        if ready && !self.deepseek_account.welcome_open {
            self.welcome
                .update(cx, |welcome, cx| welcome.use_saved_models(w, cx));
        }
        if self.deepseek_account.signed_in() && (changed || retry_details) {
            self.refresh_account_details(w, cx);
        }
        if let Some(queued) = queued
            && self.account_launcher_modal()
        {
            self.request_account(queued, w, cx);
        } else if running {
            self.schedule_account_poll(w, cx);
        } else if refresh {
            self.request_account("status", w, cx);
        }
        self.reconcile_desktop_onboarding(w, cx);
        cx.notify();
    }

    fn refresh_account_details(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let panel = &mut self.deepseek_account;
        if !panel.signed_in() || panel.loading_details {
            return;
        }
        panel.loading_details = true;
        let generation = panel.generation;
        let revision = panel.details_revision;
        cx.notify();
        cx.spawn_in(w, async move |this, cx| {
            let result =
                bridge::invoke_json("deepseek_account", json!({"request":{"action":"details"}}))
                    .await
                    .ok()
                    .and_then(|value| serde_json::from_value::<AccountReply>(value).ok());
            let _ = this.update_in(cx, |app, w, cx| {
                let panel = &mut app.deepseek_account;
                if panel.generation != generation || panel.details_revision != revision {
                    return;
                }
                panel.loading_details = false;
                let retry = std::mem::take(&mut panel.refresh_on_completion);
                if let Some(reply) = result {
                    if reply
                        .state
                        .is_some_and(|state| state.status == "credential-stored")
                    {
                        panel.details = Some(reply.details.unwrap_or_default());
                    } else {
                        app.request_account("status", w, cx);
                    }
                } else {
                    panel.details.get_or_insert_with(AccountDetails::default);
                }
                if retry {
                    app.refresh_account_details(w, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn schedule_account_poll(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.deepseek_account.poll_scheduled {
            return;
        }
        self.deepseek_account.poll_scheduled = true;
        let generation = self.deepseek_account.generation;
        cx.spawn_in(w, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            let _ = this.update_in(cx, |app, w, cx| {
                if app.deepseek_account.generation != generation {
                    return;
                }
                app.deepseek_account.poll_scheduled = false;
                if app.deepseek_account.active_attempt().is_some() {
                    app.request_account("status", w, cx);
                }
            });
        })
        .detach();
    }

    fn open_account_url(&mut self, url: String, cx: &mut Context<Self>) {
        let generation = self.deepseek_account.generation;
        cx.spawn(async move |this, cx| {
            if let Err(error) = bridge::invoke("open_browser_url", json!({"url": url})).await {
                this.update(cx, |app, cx| {
                    if app.deepseek_account.generation == generation {
                        app.deepseek_account.error = Some(error);
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();
    }
}
