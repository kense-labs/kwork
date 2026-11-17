use super::*;
use std::time::Duration;

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(super) struct LoginMethod {
    pub(super) id: String,
    label: String,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
struct LoginPrompt {
    id: String,
    kind: String,
    message: String,
    placeholder: Option<String>,
    #[serde(default)]
    options: Vec<PromptOption>,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
struct PromptOption {
    id: String,
    label: String,
    description: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum LoginStatus {
    Running,
    Authorized,
    Cancelled,
    Failed,
    Expired,
    SignedOut,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginAttempt {
    id: Option<String>,
    revision: u64,
    status: LoginStatus,
    message: Option<String>,
    instruction: Option<String>,
    url: Option<String>,
    code: Option<String>,
    prompt: Option<LoginPrompt>,
    credential_kind: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LoginDialog {
    #[serde(skip)]
    generation: u32,
    #[serde(skip)]
    provider: String,
    pub(super) name: String,
    methods: Vec<LoginMethod>,
    logout: bool,
    attempt: Option<LoginAttempt>,
    busy: bool,
    polling: bool,
    error: Option<String>,
    copied: bool,
}

impl LoginDialog {
    fn running(&self) -> Option<&LoginAttempt> {
        self.attempt
            .as_ref()
            .filter(|attempt| attempt.status == LoginStatus::Running)
    }

    fn accept(&mut self, reply: LoginAttempt) -> Option<(bool, bool)> {
        if self
            .attempt
            .as_ref()
            .is_some_and(|current| current.id != reply.id || current.revision > reply.revision)
        {
            return None;
        }
        self.copied &= self
            .attempt
            .as_ref()
            .is_some_and(|attempt| attempt.code == reply.code)
            && reply.code.is_some();
        let credentials = match reply.status {
            LoginStatus::Authorized => {
                Some((true, reply.credential_kind.as_deref() == Some("grant")))
            }
            LoginStatus::SignedOut => Some((false, false)),
            _ => None,
        };
        self.attempt = Some(reply);
        credentials
    }

    fn request(&self, action: &str, value: &str) -> Option<Value> {
        let mut draft = json!({"id": self.provider, "action": action});
        match action {
            "start"
                if !self.logout
                    && self.attempt.is_none()
                    && self.methods.iter().any(|method| method.id == value) =>
            {
                draft["method"] = value.into();
            }
            "logout" if self.logout && self.attempt.is_none() => {}
            "answer" | "cancel" => {
                let attempt = self.running()?;
                draft["attempt"] = attempt.id.as_deref()?.into();
                if action == "answer" {
                    let (prompt, answer) = serde_json::from_str::<(String, String)>(value).ok()?;
                    if attempt.prompt.as_ref()?.id != prompt || answer.trim().is_empty() {
                        return None;
                    }
                    draft["prompt"] = prompt.into();
                    draft["value"] = answer.into();
                }
            }
            _ => return None,
        }
        Some(draft)
    }
}

async fn provider_auth_call(draft: Value) -> Result<LoginAttempt, String> {
    let reply = bridge::invoke_json(
        "model_providers",
        json!({"action":"authorize", "draft":draft}),
    )
    .await?;
    serde_json::from_value(reply).map_err(|_| "无法读取登录状态，请重试。".into())
}

impl ProviderSettings {
    fn update_login_record(&mut self, id: &str, signed_in: bool, oauth_configured: bool) {
        if let Some(catalog) = self
            .providers
            .iter_mut()
            .chain(&mut self.catalog_providers)
            .find(|provider| provider.id == id)
            .and_then(|provider| provider.catalog.as_mut())
        {
            catalog.signed_in = signed_in;
            catalog.oauth_configured = oauth_configured;
            catalog.login_running = false;
        }
    }

    fn accept_login_reply(&mut self, generation: u32, reply: LoginAttempt) {
        let Some(dialog) = self
            .login
            .as_mut()
            .filter(|dialog| dialog.generation == generation)
        else {
            return;
        };
        if let Some((signed_in, oauth_configured)) = dialog.accept(reply) {
            let id = dialog.provider.clone();
            self.update_login_record(&id, signed_in, oauth_configured);
        }
    }
}

impl DshApp {
    pub(super) fn handle_provider_login(
        &mut self,
        action: &str,
        value: &str,
        cx: &mut Context<Self>,
    ) {
        match action {
            "open" | "request-logout" => self.open_provider_login(action == "request-logout", cx),
            "close" => self.close_provider_login(cx),
            "start" | "answer" | "logout" => self.request_provider_login(action, value, cx),
            "refresh" => self.watch_provider_login(cx),
            "retry" => {
                let settings = &mut self.model_providers;
                if let Some(dialog) = settings
                    .login
                    .as_mut()
                    .filter(|dialog| !dialog.busy && dialog.running().is_none())
                {
                    settings.login_generation = settings.login_generation.wrapping_add(1);
                    dialog.generation = settings.login_generation;
                    dialog.attempt = None;
                    dialog.error = None;
                    dialog.polling = false;
                    dialog.copied = false;
                    cx.notify();
                }
            }
            "open-link" | "copy-code" => {
                self.provider_login_notice_action(action == "copy-code", cx)
            }
            _ => {}
        }
    }

    fn open_provider_login(&mut self, logout: bool, cx: &mut Context<Self>) {
        let settings = &mut self.model_providers;
        if settings.login.is_some()
            || settings.busy.is_some()
            || settings.confirm_remove
            || settings.picker.is_some()
        {
            return;
        }
        let Some(provider) = settings.selected_provider() else {
            return;
        };
        let Some(catalog) = provider.catalog.as_ref() else {
            return;
        };
        if catalog.login_running
            || (logout && !catalog.signed_in)
            || (!logout && catalog.login_methods.is_empty())
        {
            return;
        }
        let generation = settings.login_generation.wrapping_add(1);
        settings.login = Some(LoginDialog {
            generation,
            provider: provider.id.clone(),
            name: provider.name.clone(),
            methods: catalog.login_methods.clone(),
            logout,
            attempt: None,
            error: None,
            busy: false,
            polling: false,
            copied: false,
        });
        settings.login_generation = generation;
        cx.notify();
    }

    pub(super) fn close_provider_login(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.model_providers.login.as_ref() else {
            return;
        };
        if dialog.busy {
            return;
        }
        if dialog.running().is_some() {
            self.request_provider_login("cancel", "", cx);
        } else {
            self.model_providers.login = None;
            cx.notify();
        }
    }

    fn request_provider_login(&mut self, action: &str, value: &str, cx: &mut Context<Self>) {
        let Some(dialog) = self
            .model_providers
            .login
            .as_mut()
            .filter(|dialog| !dialog.busy)
        else {
            return;
        };
        let Some(draft) = dialog.request(action, value) else {
            return;
        };
        let generation = dialog.generation;
        dialog.error = None;
        dialog.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = provider_auth_call(draft).await;
            this.update(cx, |app, cx| {
                let Some(dialog) = app
                    .model_providers
                    .login
                    .as_mut()
                    .filter(|dialog| dialog.generation == generation)
                else {
                    return;
                };
                dialog.busy = false;
                match result {
                    Ok(reply) => app.model_providers.accept_login_reply(generation, reply),
                    Err(error) => dialog.error = Some(error),
                }
                app.watch_provider_login(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn watch_provider_login(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self
            .model_providers
            .login
            .as_mut()
            .filter(|dialog| !dialog.polling)
        else {
            return;
        };
        let Some(id) = dialog.running().and_then(|attempt| attempt.id.clone()) else {
            return;
        };
        let generation = dialog.generation;
        let draft = json!({"id": dialog.provider, "action": "status", "attempt": id});
        dialog.polling = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(400))
                    .await;
                let running = this
                    .update(cx, |app, cx| {
                        let Some(dialog) = app
                            .model_providers
                            .login
                            .as_mut()
                            .filter(|dialog| dialog.generation == generation)
                        else {
                            return false;
                        };
                        let running = dialog.running().is_some();
                        if !running {
                            dialog.polling = false;
                            cx.notify();
                        }
                        running
                    })
                    .unwrap_or(false);
                if !running {
                    return;
                }
                let result = provider_auth_call(draft.clone()).await;
                let keep = this
                    .update(cx, |app, cx| {
                        let Some(dialog) = app
                            .model_providers
                            .login
                            .as_mut()
                            .filter(|dialog| dialog.generation == generation)
                        else {
                            return false;
                        };
                        match result {
                            Ok(reply) => app.model_providers.accept_login_reply(generation, reply),
                            Err(error) => {
                                dialog.polling = false;
                                dialog.error = Some(error);
                                cx.notify();
                                return false;
                            }
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    return;
                }
            }
        })
        .detach();
    }

    fn provider_login_notice_action(&mut self, copy: bool, cx: &mut Context<Self>) {
        let Some(dialog) = self.model_providers.login.as_mut() else {
            return;
        };
        let Some(attempt) = dialog.running() else {
            return;
        };
        let generation = dialog.generation;
        let content = if copy {
            attempt.code.clone()
        } else {
            attempt.url.clone()
        };
        let Some(content) = content else { return };
        dialog.error = None;
        let clipboard = copy.then(|| bridge::copy_text_checked(&content));
        cx.spawn(async move |this, cx| {
            let result = if let Some(clipboard) = clipboard {
                match wasm_bindgen_futures::JsFuture::from(clipboard).await {
                    Ok(value) if value.as_bool() == Some(true) => Ok(()),
                    _ => Err("复制失败，请手动复制设备码。".into()),
                }
            } else {
                bridge::invoke("open_browser_url", json!({"url":content}))
                    .await
                    .map(|_| ())
            };
            this.update(cx, |app, cx| {
                let Some(dialog) =
                    app.model_providers.login.as_mut().filter(|dialog| {
                        dialog.generation == generation && dialog.running().is_some()
                    })
                else {
                    return;
                };
                match result {
                    Ok(()) if copy => dialog.copied = true,
                    Ok(()) => {}
                    Err(error) => dialog.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
