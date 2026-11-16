//! Harness Welcome flow backed by write-only Host credential commands.

use crate::{
    brand, bridge,
    state::validate_api_key,
    tokens::{Tokens, dsh},
    web::invoke,
};
use gpui::{Context, Subscription, Window, div, prelude::*, px};
use kui::components::button::Button;
use kui::primitives::editor::{EditEvent, Editor, InputMode};
use kui::primitives::glyph::{Glyph, icon};
use kui::primitives::ops_surface::{column, editor, icon_btn, input, note, replace, row};
use serde::Serialize;
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/welcome-form.js")]
extern "C" {
    fn update_welcome_form(value: JsValue);
    fn welcome_has_draft() -> bool;
    pub(crate) fn listen_welcome_form(callback: &js_sys::Function);
}

#[derive(PartialEq, Eq, Serialize)]
struct WelcomeForm {
    dark: bool,
    checking: bool,
    saving: bool,
    account_available: bool,
    account_open: bool,
    error: Option<String>,
}

#[derive(Clone, Copy)]
pub enum WelcomeEvent {
    Changed,
    OpenModelProviders,
}

pub struct Welcome {
    configured: bool,
    editing_key: bool,
    busy: bool,
    checking_key: bool,
    error: Option<String>,
    skipped: bool,
    api_key: gpui::Entity<Editor>,
    web_form: Option<WelcomeForm>,
    _subscriptions: Vec<Subscription>,
}

impl gpui::EventEmitter<WelcomeEvent> for Welcome {}

impl Welcome {
    pub fn new(w: &mut Window, cx: &mut Context<Self>) -> Self {
        let api_key = editor("输入 API 密钥".into(), InputMode::Password, w, cx);
        let submit = cx.subscribe_in(
            &api_key,
            w,
            |this, _, event: &EditEvent, w, cx| match event {
                EditEvent::Submitted => this.save_api_key(cx),
                EditEvent::Changed => {
                    this.error = None;
                    this.refresh(cx);
                }
                EditEvent::Focused | EditEvent::Blurred => cx.notify(),
                EditEvent::Escape if !this.busy && this.editing_key => {
                    this.clear_api_key(w, cx);
                    this.refresh(cx);
                }
                _ => {}
            },
        );
        Self {
            configured: false,
            editing_key: false,
            busy: true,
            checking_key: true,
            error: None,
            skipped: false,
            api_key,
            web_form: None,
            _subscriptions: vec![
                submit,
                cx.observe_window_appearance(w, |_, _, cx| cx.notify()),
            ],
        }
    }

    pub fn configured(&self) -> bool {
        self.configured
    }

    pub fn should_show(&self) -> bool {
        !self.skipped && !self.configured()
    }

    pub fn use_saved_models(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.should_show() && !welcome_has_draft() && self.api_key.read(cx).value().is_empty() {
            self.skip_key(w, cx);
        }
    }

    pub(crate) fn continue_with_account(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        self.skip_key(w, cx);
    }

    fn skip_key(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if !self.busy || self.checking_key {
            self.clear_api_key(w, cx);
            self.skipped = true;
            self.refresh(cx);
        }
    }

    fn clear_api_key(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        replace(&self.api_key, "", w, cx);
        self.editing_key = false;
        self.error = None;
    }

    /// Official ApiKeyInput reveal control: only the unsaved draft can be
    /// revealed; stored keys stay write-only on the Host.
    fn toggle_key_reveal(&mut self, cx: &mut Context<Self>) {
        let revealed = !self.api_key.read(cx).revealed;
        self.api_key.update(cx, |editor, cx| {
            editor.revealed = revealed;
            cx.notify();
        });
        cx.notify();
    }

    fn refresh(&self, cx: &mut Context<Self>) {
        cx.emit(WelcomeEvent::Changed);
        cx.notify();
    }

    pub fn handle_reply(
        &mut self,
        command: &'static str,
        result: Result<Value, String>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.busy = false;
        self.checking_key = false;
        match result {
            Ok(status) => {
                self.configured = status.pointer("/apiKey/configured") == Some(&Value::Bool(true));
                self.error = None;
                if command == "save_api_key" || command == "remove_api_key" {
                    self.clear_api_key(w, cx);
                }
            }
            Err(error) => self.error = Some(error),
        }
        self.refresh(cx);
    }

    pub fn handle_event(&mut self, event: &Value, cx: &mut Context<Self>) {
        self.busy = false;
        self.checking_key = false;
        self.error = Some(
            event["detail"]
                .as_str()
                .unwrap_or("Harness 凭据服务不可用")
                .to_owned(),
        );
        cx.notify();
    }

    fn save_api_key(&mut self, cx: &mut Context<Self>) {
        let value = self.api_key.read(cx).value().to_owned();
        self.save_key_value(&value, cx);
    }

    fn save_key_value(&mut self, value: &str, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let value = value.trim();
        if let Err(error) = validate_api_key(value) {
            self.error = Some(error.to_string());
            self.refresh(cx);
            return;
        }
        self.busy = true;
        self.error = None;
        invoke("save_api_key", json!({"value": value}));
        self.refresh(cx);
    }

    pub(crate) fn publish_form(
        &mut self,
        visible: bool,
        checking_models: bool,
        account_available: bool,
        account_open: bool,
        w: &Window,
    ) {
        let next = visible.then(|| WelcomeForm {
            dark: dark_window(w),
            checking: self.checking_key || checking_models,
            saving: self.busy && !self.checking_key,
            account_available,
            account_open,
            error: self.error.clone(),
        });
        if self.web_form != next
            && let Ok(value) = serde_wasm_bindgen::to_value(&next)
        {
            self.web_form = next;
            update_welcome_form(value);
        }
    }

    pub(crate) fn handle_form_action(
        &mut self,
        action: &str,
        key: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.should_show() {
            return;
        }
        match action {
            "welcome-save" => self.save_key_value(key, cx),
            "welcome-edit" if !self.busy && self.error.take().is_some() => self.refresh(cx),
            "welcome-later" => self.skip_key(w, cx),
            "welcome-providers" if !self.busy || self.checking_key => {
                self.skip_key(w, cx);
                cx.emit(WelcomeEvent::OpenModelProviders);
            }
            _ => {}
        }
    }

    pub fn render_inline(&self, cx: &mut Context<Self>) -> gpui::Div {
        let p = dsh(cx);
        let mut content = column()
            .w_full()
            .gap(px(10.))
            .child(
                row()
                    .w_full()
                    .gap(px(8.))
                    .child(icon(Glyph::Lock, 15., p.secondary))
                    .child(div().flex_1().text_size(px(14.)).child("DeepSeek API Key"))
                    .child(note(
                        if self.configured() {
                            "已配置"
                        } else {
                            "未配置"
                        },
                        cx,
                    )),
            )
            .child(note("API Key 存储在本机 Harness 凭据文件中。", cx));
        if self.editing_key {
            let draft = self.api_key.read(cx);
            let revealed = draft.revealed;
            let has_value = !draft.value().is_empty();
            let reveal_label = if revealed {
                "隐藏密钥"
            } else {
                "显示密钥"
            };
            let mut key_row = row()
                .w_full()
                .gap(px(6.))
                .items_center()
                .child(div().flex_1().min_w_0().child(input(&self.api_key, cx)));
            if has_value {
                key_row = key_row.child(icon_btn(
                    "api-key-reveal",
                    reveal_label,
                    Glyph::Eye,
                    cx,
                    |this: &mut Self, _, cx| this.toggle_key_reveal(cx),
                ));
            }
            content = content.child(key_row).child(
                row()
                    .gap(px(6.))
                    .child(
                        Button::new("inline-save-api-key")
                            .label("保存")
                            .primary()
                            .loading(self.busy)
                            .disabled(self.busy || self.api_key.read(cx).value().trim().is_empty())
                            .on_click(cx.listener(|this, _, _, cx| this.save_api_key(cx)))
                            .into_themed(cx),
                    )
                    .child(
                        Button::new("inline-cancel-api-key")
                            .label("取消")
                            .ghost()
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.clear_api_key(w, cx);
                                this.refresh(cx);
                            }))
                            .into_themed(cx),
                    ),
            );
        } else {
            content = content.child(
                row()
                    .gap(px(6.))
                    .child(
                        Button::new("inline-configure-api-key")
                            .label(if self.configured() {
                                "更新 API Key"
                            } else {
                                "添加 API Key"
                            })
                            .secondary()
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.editing_key = true;
                                this.error = None;
                                cx.on_next_frame(w, |this, w, cx| {
                                    if this.editing_key {
                                        this.api_key.read(cx).focus.clone().focus(w, cx);
                                    }
                                });
                                this.refresh(cx);
                            }))
                            .into_themed(cx),
                    )
                    .when(self.configured(), |actions| {
                        actions.child(
                            Button::new("inline-remove-api-key")
                                .label("移除 API Key")
                                .ghost()
                                .disabled(self.busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.busy = true;
                                    invoke("remove_api_key", json!({}));
                                    this.refresh(cx);
                                }))
                                .into_themed(cx),
                        )
                    }),
            );
        }
        content.children(
            self.error
                .clone()
                .map(|error| div().text_size(px(12.)).text_color(p.danger).child(error)),
        )
    }
}

fn dark_window(w: &Window) -> bool {
    matches!(
        w.appearance(),
        gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
    )
}

impl Render for Welcome {
    fn render(&mut self, w: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let dark = dark_window(w);
        let p = if dark {
            Tokens::dark()
        } else {
            Tokens::light()
        };
        let tint = if bridge::has_mac_window_material() {
            match w.appearance() {
                gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark => {
                    gpui::rgb(0x18191c).opacity(0.5).into()
                }
                _ => gpui::white().opacity(0.4),
            }
        } else {
            p.stage
        };
        column()
            .size_full()
            .overflow_hidden()
            .items_center()
            .px(px(16.))
            .pt(px(74.))
            .bg(tint)
            .text_color(p.label_primary)
            .font_family("Montserrat")
            .child(brand::wordmark(40., 34., dark))
    }
}
