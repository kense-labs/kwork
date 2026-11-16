//! Shared busy-Enter preference and the accessible composer Send control.

use super::*;
use crate::submit::{DEFAULT_BUSY_ENTER, SubmitGesture, SubmitMode};
use serde::Serialize;

#[wasm_bindgen(module = "/composer-accessibility.js")]
extern "C" {
    fn update_composer_accessibility(navigation: JsValue, preference: JsValue, submit: JsValue);
    pub(super) fn listen_composer_accessibility(callback: &js_sys::Function);
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize)]
struct AccessibleSettingsNav {
    settings_open: bool,
    providers_open: bool,
    account_available: bool,
    account_open: bool,
    more_open: Option<bool>,
}

#[derive(PartialEq, Eq, Serialize)]
struct AccessiblePreference {
    mode: &'static str,
    busy: bool,
    error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
pub(super) struct PrimaryComposerAction {
    pub(super) label: &'static str,
    pub(super) enabled: bool,
    pub(super) stop: bool,
}

pub(super) struct ComposerPreference {
    pub(super) mode: SubmitMode,
    pub(super) busy: bool,
    pub(super) error: Option<String>,
    refresh: bool,
    accessible_settings_nav: AccessibleSettingsNav,
    accessible: Option<AccessiblePreference>,
    accessible_submit: Option<PrimaryComposerAction>,
}

impl Default for ComposerPreference {
    fn default() -> Self {
        Self {
            mode: DEFAULT_BUSY_ENTER,
            busy: false,
            error: None,
            refresh: false,
            accessible_settings_nav: AccessibleSettingsNav::default(),
            accessible: None,
            accessible_submit: None,
        }
    }
}

impl DshApp {
    pub(super) fn request_composer_preference(
        &mut self,
        mode: Option<SubmitMode>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let preference = &mut self.composer_preference;
        if preference.busy {
            preference.refresh |= mode.is_none();
            return;
        }
        let previous = preference.mode;
        if mode == Some(previous) && preference.error.is_none() {
            return;
        }
        preference.busy = true;
        preference.error = None;
        if let Some(mode) = mode {
            preference.mode = mode;
        }
        cx.spawn_in(w, async move |app, cx| {
            let reply = bridge::invoke(
                "composer_preference",
                json!({"mode": mode.map(SubmitMode::as_str)}),
            )
            .await;
            let _ = app.update_in(cx, |app, w, cx| {
                let preference = &mut app.composer_preference;
                preference.busy = false;
                match reply.and_then(|value| match value["mode"].as_str() {
                    Some("queue") => Ok(SubmitMode::Queue),
                    Some("steer") => Ok(SubmitMode::Steer),
                    _ => Err("服务返回了无效的发送设置。".into()),
                }) {
                    Ok(mode) => preference.mode = mode,
                    Err(error) => {
                        preference.mode = previous;
                        preference.error = Some(error);
                    }
                }
                let refresh = std::mem::take(&mut preference.refresh);
                if refresh {
                    app.request_composer_preference(None, w, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn submit_mode(&self, gesture: SubmitGesture) -> SubmitMode {
        crate::submit::resolve_submit_mode(
            self.composer_preference.mode,
            self.conversation.running,
            gesture,
            self.conversation.acp_open,
        )
    }

    pub(super) fn primary_composer_action(&self, empty: bool) -> PrimaryComposerAction {
        let stop = self.conversation.running && empty;
        let enabled = stop
            || (!empty
                && self.connected()
                && self.conversation.acp_open
                && self.active_session.is_some()
                && !self.creating_session
                && !self.conversation.permission_mode.changing()
                && self.confirm_permission.is_none()
                && !self.conversation.command_busy
                && self.conversation.inbox_submission.is_none()
                && (!self.conversation.history_loading
                    || self.conversation.history_prepend_pending
                    || self.conversation.running));
        let label = if stop {
            "停止生成"
        } else if self.conversation.inbox_submission.is_some() {
            "正在发送消息"
        } else if self.conversation.running {
            match self.submit_mode(SubmitGesture::Enter) {
                SubmitMode::Queue => "排队发送",
                SubmitMode::Steer => "插话发送",
            }
        } else {
            "发送"
        };
        PrimaryComposerAction {
            label,
            enabled,
            stop,
        }
    }

    pub(super) fn publish_accessible_composer(&mut self, cx: &App) {
        let preference =
            (self.show_settings && self.settings_page == SettingsPage::General).then(|| {
                AccessiblePreference {
                    mode: self.composer_preference.mode.as_str(),
                    busy: self.composer_preference.busy,
                    error: self.composer_preference.error.clone(),
                }
            });
        let submit = self.composer_tools_available().then(|| {
            let empty = self.composer.read(cx).value().trim().is_empty()
                && self.conversation.draft_attachments.is_empty();
            self.primary_composer_action(empty)
        });
        let navigation = AccessibleSettingsNav {
            settings_open: self.show_settings
                && !self.provider_navigation_locked()
                && !self.account_confirmation_open(),
            providers_open: self.show_settings && self.settings_page == SettingsPage::Providers,
            account_available: self.deepseek_account.available,
            account_open: self.show_settings && self.settings_page == SettingsPage::Account,
            more_open: (self.workspace_tree_available()
                && self.deepseek_account.launcher_compact.is_none())
            .then_some(self.show_more_menu),
        };
        if (navigation != self.composer_preference.accessible_settings_nav
            || preference != self.composer_preference.accessible
            || submit != self.composer_preference.accessible_submit)
            && let (Ok(nav), Ok(settings), Ok(action)) = (
                serde_wasm_bindgen::to_value(&navigation),
                serde_wasm_bindgen::to_value(&preference),
                serde_wasm_bindgen::to_value(&submit),
            )
        {
            self.composer_preference.accessible_settings_nav = navigation;
            self.composer_preference.accessible = preference;
            self.composer_preference.accessible_submit = submit;
            update_composer_accessibility(nav, settings, action);
        }
    }

    pub(super) fn accessible_composer_action(
        &mut self,
        action: &str,
        value: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            "composer-more-settings" if self.workspace_tree_available() => {
                self.toggle_more_settings(cx);
            }
            "composer-open-settings" if self.show_more_menu && self.workspace_tree_available() => {
                self.open_account_menu_settings(w, cx);
            }
            "composer-model-providers"
                if self.show_settings
                    && !self.provider_navigation_locked()
                    && !self.account_confirmation_open() =>
            {
                self.transcript_preferences.clear_menu();
                self.open_model_providers(w, cx);
            }
            "composer-account-settings"
                if self.show_settings
                    && !self.provider_navigation_locked()
                    && !self.account_confirmation_open() =>
            {
                self.transcript_preferences.clear_menu();
                self.open_account_settings(w, cx);
            }
            "composer-general-settings"
                if self.show_settings
                    && !self.provider_navigation_locked()
                    && !self.account_confirmation_open() =>
            {
                self.transcript_preferences.clear_menu();
                self.settings_page = SettingsPage::General;
                cx.notify();
            }
            "composer-submit" if self.composer_tools_available() => {
                let empty = self.composer.read(cx).value().trim().is_empty()
                    && self.conversation.draft_attachments.is_empty();
                let current = self.primary_composer_action(empty);
                if current.enabled && current.label == value {
                    if current.stop {
                        self.cancel(cx);
                    } else {
                        self.submit_prompt(w, cx);
                    }
                }
            }
            "composer-preference"
                if self.show_settings && self.settings_page == SettingsPage::General =>
            {
                let mode = match value {
                    "queue" => Some(SubmitMode::Queue),
                    "steer" => Some(SubmitMode::Steer),
                    "" => None,
                    _ => return,
                };
                self.request_composer_preference(mode, w, cx);
            }
            "composer-preference-close"
                if self.show_settings && self.settings_page == SettingsPage::General =>
            {
                self.close_settings(cx);
            }
            _ => {}
        }
    }
}
