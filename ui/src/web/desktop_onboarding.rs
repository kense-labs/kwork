//! First-run guidance shown after credential setup.

#[path = "../../../shared/desktop_onboarding.rs"]
mod protocol;

use protocol::{Action, Completion, Process, Progress, Purpose, Reply, Snapshot, Step, Usage};
use serde::Serialize;

use super::*;

#[wasm_bindgen(module = "/desktop-onboarding.js")]
extern "C" {
    fn update_desktop_onboarding(value: JsValue);
    pub(super) fn listen_desktop_onboarding(callback: &js_sys::Function);
}

#[derive(PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct View {
    dark: bool,
    progress: Progress,
    loading: bool,
    saving: bool,
    error: Option<String>,
    can_retry: bool,
    must_read: bool,
    can_recharge: bool,
    funded_at_entry: bool,
    balance_funded: Option<bool>,
}

#[derive(Default)]
pub(super) struct DesktopOnboarding {
    snapshot: Snapshot,
    pending: Option<Action>,
    retry: Option<Action>,
    error: Option<String>,
    funded_at_entry: Option<bool>,
    view: Option<View>,
}

impl DesktopOnboarding {
    pub(super) fn revision(&self) -> u64 {
        self.snapshot.revision
    }
}

impl DshApp {
    pub(super) fn load_desktop_onboarding(&mut self, reply: &Result<Value, String>) {
        let onboarding = &mut self.desktop_onboarding;
        match reply {
            Ok(settings) => {
                onboarding.snapshot =
                    serde_json::from_value(settings["onboarding"].clone()).unwrap_or_default();
            }
            Err(_) => {
                onboarding.retry = Some(Action::Read);
                onboarding.error = Some("未能读取首次引导进度，请重试。".into());
            }
        }
    }

    pub(super) fn desktop_onboarding_visible(&self) -> bool {
        self.connected()
            && self.deepseek_account.signed_in()
            && !self.deepseek_account.welcome_open
            && !self.show_welcome
            && self.desktop_onboarding.snapshot.progress.step != Step::Done
    }

    pub(super) fn reconcile_desktop_onboarding(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if !self.deepseek_account.signed_in() {
            self.desktop_onboarding.funded_at_entry = None;
            if self.connected()
                && !self.service_starting
                && !self.developer_tools_loading
                && self.workspace_preferences_loaded
                && self.welcome.read(cx).configured()
                && self.desktop_onboarding.snapshot.progress.step != Step::Done
                && self.desktop_onboarding.error.is_none()
            {
                self.request_onboarding(
                    Action::Finish {
                        completion: Completion::ApiKey,
                    },
                    w,
                    cx,
                );
            }
        }
    }

    pub(super) fn publish_desktop_onboarding(&mut self, cx: &App) {
        let next = if self.desktop_onboarding_visible() {
            let balance_funded = self.deepseek_account.has_positive_balance();
            let onboarding = &mut self.desktop_onboarding;
            if onboarding.snapshot.progress.step == Step::Credit
                && self.workspace_preferences_loaded
            {
                onboarding
                    .funded_at_entry
                    .get_or_insert(balance_funded.unwrap_or_default());
            }
            Some(View {
                dark: workspace::dark(cx),
                progress: onboarding.snapshot.progress,
                loading: !self.workspace_preferences_loaded || self.developer_tools_loading,
                saving: onboarding.pending.is_some(),
                error: onboarding.error.clone(),
                can_retry: onboarding.retry.is_some(),
                must_read: onboarding.retry == Some(Action::Read),
                can_recharge: self.deepseek_account.can_top_up(),
                funded_at_entry: onboarding.funded_at_entry.unwrap_or_default(),
                balance_funded,
            })
        } else {
            None
        };
        let onboarding = &mut self.desktop_onboarding;
        if onboarding.view != next
            && let Ok(value) = serde_wasm_bindgen::to_value(&next)
        {
            onboarding.view = next;
            update_desktop_onboarding(value);
        }
    }

    pub(super) fn handle_desktop_onboarding(
        &mut self,
        action: &str,
        value: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.desktop_onboarding_visible()
            || self.account_platform_open()
            || !self.workspace_preferences_loaded
            || self.developer_tools_loading
            || self.desktop_onboarding.pending.is_some()
        {
            return;
        }
        let progress = self.desktop_onboarding.snapshot.progress;
        let action = match action {
            "onboarding-recharge" if progress.step == Step::Credit => {
                self.open_account_top_up(cx);
                return;
            }
            "onboarding-next"
                if progress.step == Step::Purpose && progress.purpose == Some(Purpose::Office) =>
            {
                Action::Finish {
                    completion: Completion::Completed,
                }
            }
            "onboarding-next" => Action::Next,
            "onboarding-back" => Action::Back,
            "onboarding-finish" => Action::Finish {
                completion: Completion::Completed,
            },
            "onboarding-skip" => Action::Finish {
                completion: Completion::Skipped,
            },
            "onboarding-purpose" => Action::SelectPurpose {
                purpose: match value {
                    "" => None,
                    "office" => Some(Purpose::Office),
                    "development" => Some(Purpose::Development),
                    "both" => Some(Purpose::Both),
                    _ => return,
                },
            },
            "onboarding-process" => Action::SelectProcess {
                process: match value {
                    "compact" => Process::Compact,
                    "standard" => Process::Standard,
                    "detailed" => Process::Detailed,
                    _ => return,
                },
            },
            "onboarding-retry" => {
                let Some(action) = self.desktop_onboarding.retry else {
                    return;
                };
                action
            }
            _ => return,
        };
        if self.desktop_onboarding.retry == Some(Action::Read) && action != Action::Read {
            return;
        }
        self.request_onboarding(action, w, cx);
    }

    fn request_onboarding(&mut self, action: Action, w: &mut Window, cx: &mut Context<Self>) {
        let onboarding = &mut self.desktop_onboarding;
        if onboarding.pending.is_some() {
            return;
        }
        onboarding.pending = Some(action);
        onboarding.error = None;
        onboarding.retry = None;
        let revision = onboarding.snapshot.revision;
        cx.notify();
        cx.spawn_in(w, async move |this, cx| {
            let result = bridge::invoke_json(
                "desktop_onboarding",
                json!({"action":action,"revision":revision}),
            )
            .await
            .and_then(|value| {
                serde_json::from_value::<Reply>(value)
                    .map_err(|_| "无法读取首次引导状态。".to_owned())
            });
            let _ = this.update_in(cx, |app, w, cx| {
                app.finish_onboarding(action, result, w, cx)
            });
        })
        .detach();
    }

    fn finish_onboarding(
        &mut self,
        action: Action,
        result: Result<Reply, String>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let onboarding = &mut self.desktop_onboarding;
        onboarding.pending = None;
        match result {
            Ok(reply) => {
                onboarding.snapshot = reply.snapshot;
                if reply.conflict {
                    onboarding.error = Some("首次引导已在另一窗口更新。请从当前步骤继续。".into());
                }
                if reply.snapshot.progress.step == Step::Done
                    && (reply.conflict || action == Action::Read)
                {
                    invoke("load_workspace_preferences", json!({}));
                    invoke("load_developer_tools", json!({}));
                } else if reply.snapshot.progress.step == Step::Done {
                    let progress = reply.snapshot.progress;
                    self.work_details = match progress.process {
                        Some(Process::Compact) => WorkDetailsMode::Compact,
                        Some(Process::Detailed) => WorkDetailsMode::Detailed,
                        _ => WorkDetailsMode::Standard,
                    };
                    self.performance_compact = progress.usage == Usage::Compact;
                    self.update_html_preview_mode(progress.developer_tools);
                    if self.page == WorkspacePage::Conversation && self.active_session.is_some() {
                        cx.on_next_frame(w, |app, w, cx| {
                            if !app.show_settings && !app.show_welcome {
                                app.focus_composer(w, cx);
                            }
                        });
                    }
                }
            }
            Err(error) => {
                onboarding.error = Some(error);
                onboarding.retry = Some(action);
            }
        }
        cx.notify();
    }
}
