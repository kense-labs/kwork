//! Shared settings chrome, measured by GPUI and exposed as native web controls.

use super::*;
use serde::Serialize;

#[wasm_bindgen(module = "/settings-shell.js")]
extern "C" {
    fn update_settings_shell(value: JsValue);
    fn place_settings_shell(left: f32, top: f32, width: f32, height: f32);
    fn place_settings_theme(left: f32, top: f32, width: f32);
    fn place_settings_plugins(left: f32, top: f32, width: f32, height: f32);
    fn place_settings_agent_presets(left: f32, top: f32, width: f32, height: f32);
    fn settings_theme_height() -> f32;
    fn place_settings_upload(left: f32, top: f32, width: f32);
    fn settings_upload_height() -> f32;
    fn place_settings_permission(left: f32, top: f32, width: f32);
    fn settings_permission_height() -> f32;
    fn place_settings_developer(left: f32, top: f32, width: f32);
    fn settings_developer_height() -> f32;
    pub(super) fn listen_settings_shell(callback: &js_sys::Function);
    fn save_client_theme(changes: JsValue);
}

#[derive(Default)]
pub(super) struct SettingsShell {
    pub(super) generation: u32,
    view: Option<View>,
    pub(super) theme_loaded: bool,
    pub(super) developer_loaded: bool,
    pub(super) theme_writable: bool,
    developer_writable: bool,
}

#[derive(PartialEq, Eq, Serialize)]
struct View {
    visible: bool,
    connected: bool,
    dark: bool,
    generation: u32,
    locked: bool,
    navigation_locked: bool,
    page: usize,
    account: bool,
    theme: &'static str,
    font_size: i32,
    fonts: crate::font_preferences::FontPreferences,
    developer_tools: bool,
    developer_loading: bool,
    developer_loaded: bool,
    theme_writable: bool,
    developer_writable: bool,
    blank_preset_session: Option<String>,
}

pub(super) const PAGES: [SettingsPage; 9] = [
    SettingsPage::Account,
    SettingsPage::Credentials,
    SettingsPage::General,
    SettingsPage::WebSearch,
    SettingsPage::Model,
    SettingsPage::Providers,
    SettingsPage::Plugins,
    SettingsPage::AgentPresets,
    SettingsPage::SessionLog,
];

impl DshApp {
    pub(super) fn publish_settings_shell(&mut self, cx: &App) {
        let next = View {
            visible: self.show_settings && !self.show_welcome,
            connected: self.connected() && !self.service_starting,
            dark: workspace::dark(cx),
            generation: self.settings_shell.generation,
            locked: self.provider_modal_open()
                || self.account_confirmation_open()
                || self.shortcuts.open,
            navigation_locked: self.provider_navigation_locked()
                || self.account_confirmation_open()
                || self.shortcuts.open,
            page: PAGES
                .iter()
                .position(|page| *page == self.settings_page.for_platform())
                .unwrap_or(0),
            account: self.deepseek_account.available,
            theme: self.theme_preference.id(),
            font_size: 14 + self.content_font_delta,
            fonts: document_fonts::preferences(cx).clone(),
            developer_tools: self.developer_tools,
            developer_loading: self.developer_tools_loading,
            developer_loaded: self.settings_shell.developer_loaded,
            theme_writable: self.settings_shell.theme_writable,
            developer_writable: self.settings_shell.developer_writable,
            blank_preset_session: (self.conversation.has_agent_preset
                && self.conversation_header_is_blank()
                && self.conversation.acp_open)
                .then(|| self.active_session.clone())
                .flatten(),
        };
        if self.settings_shell.view.as_ref() != Some(&next)
            && let Ok(value) = serde_wasm_bindgen::to_value(&next)
        {
            self.settings_shell.view = Some(next);
            update_settings_shell(value);
        }
    }

    pub(super) fn handle_settings_shell(
        &mut self,
        action: &str,
        value: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action == "settings-client-preferences" {
            if let Ok(snapshot) = serde_json::from_str::<Value>(value) {
                let available = snapshot["available"].as_array();
                let writable = snapshot["writable"] == true;
                self.settings_shell.theme_writable = writable
                    && available.is_some_and(|rows| rows.iter().any(|row| row == "ui-theme"));
                self.settings_shell.developer_writable = writable
                    && available.is_some_and(|rows| rows.iter().any(|row| row == "ui-settings"));
                let mut cache_changed = false;
                for row in snapshot["namespaces"].as_array().into_iter().flatten() {
                    let data = &row["value"];
                    match row["ns"].as_str() {
                        Some("ui-theme") => {
                            self.settings_shell.theme_loaded = true;
                            if let Some(theme) = data["preference"].as_str() {
                                self.restore_theme_preference(ThemePreference::from_id(theme), cx);
                            }
                            if let Some(size) = data["fontSize"].as_i64() {
                                self.content_font_delta = (size - 14).clamp(-4, 8) as i32;
                            }
                            if let Ok(fonts) = serde_json::from_value(data.clone()) {
                                document_fonts::apply_preferences(fonts, cx);
                            }
                            cache_changed = true;
                        }
                        Some("ui-settings") => {
                            self.settings_shell.developer_loaded = true;
                            self.developer_tools_loading = false;
                            if let Some(enabled) = data["enabled"].as_bool() {
                                self.update_html_preview_mode(enabled);
                                invoke("save_developer_tools", json!({"enabled":enabled}));
                            }
                        }
                        _ => {}
                    }
                }
                if cache_changed && self.workspace_preferences_loaded {
                    self.save_workspace_preferences(cx);
                }
                self.reconcile_desktop_onboarding(w, cx);
                cx.notify();
            }
            return;
        }
        if action == "settings-client-preferences-error" {
            self.push_toast(value, cx);
            return;
        }
        // Accepted installation settings still apply if the panel closed during the save.
        if action == "settings-developer-accepted" && matches!(value, "true" | "false") {
            self.update_html_preview_mode(value == "true");
            cx.notify();
            return;
        }
        if !self.show_settings || self.show_welcome {
            return;
        }
        if action == "settings-close" {
            self.close_settings(cx);
            return;
        }
        if action == "settings-agent-preset-creator"
            && self.settings_page == SettingsPage::AgentPresets
        {
            self.start_creator_preset(w, cx);
            return;
        }
        if self.provider_navigation_locked() || self.account_confirmation_open() {
            return;
        }
        if self.settings_page == SettingsPage::General {
            match action {
                "settings-theme-layout"
                | "settings-upload-layout"
                | "settings-permission-layout"
                | "settings-developer-layout" => {
                    cx.notify();
                    return;
                }
                "settings-theme" if matches!(value, "light" | "dark" | "system") => {
                    self.set_theme_preference(ThemePreference::from_id(value), cx);
                    cx.notify();
                    return;
                }
                "settings-font-step" => {
                    if let Ok(step @ (-1 | 1)) = value.parse::<i32>() {
                        self.set_content_font_delta(self.content_font_delta + step, cx);
                    }
                    return;
                }
                "settings-font-role-step" => {
                    if bridge::is_mac() && !self.settings_shell.theme_writable {
                        return;
                    }
                    if let Ok((role, step @ (-1 | 1))) =
                        serde_json::from_str::<(String, i32)>(value)
                    {
                        if role == "text" {
                            self.set_content_font_delta(self.content_font_delta + step, cx);
                        } else {
                            let mut fonts = document_fonts::preferences(cx).clone();
                            match role.as_str() {
                                "code" => fonts.code_font_size += step,
                                "terminal" => fonts.terminal_font_size += step,
                                _ => return,
                            }
                            document_fonts::apply_preferences(fonts, cx);
                            let fonts = document_fonts::preferences(cx);
                            self.persist_host_theme(match role.as_str() {
                                "code" => json!({"codeFontSize": fonts.code_font_size}),
                                _ => json!({"terminalFontSize": fonts.terminal_font_size}),
                            });
                            self.save_workspace_preferences(cx);
                        }
                    }
                    return;
                }
                "settings-font-family" => {
                    if bridge::is_mac() && !self.settings_shell.theme_writable {
                        return;
                    }
                    if let Ok((role, family)) = serde_json::from_str::<(String, String)>(value) {
                        let mut fonts = document_fonts::preferences(cx).clone();
                        match role.as_str() {
                            "text" => fonts.text_font_family = family,
                            "code" => fonts.code_font_family = family,
                            "terminal" => fonts.terminal_font_family = family,
                            _ => return,
                        }
                        document_fonts::apply_preferences(fonts, cx);
                        let fonts = document_fonts::preferences(cx);
                        self.persist_host_theme(match role.as_str() {
                            "text" => json!({"textFontFamily": fonts.text_font_family}),
                            "code" => json!({"codeFontFamily": fonts.code_font_family}),
                            _ => json!({"terminalFontFamily": fonts.terminal_font_family}),
                        });
                        self.save_workspace_preferences(cx);
                    }
                    return;
                }
                _ => {}
            }
        }
        let Some(target) = value
            .parse::<usize>()
            .ok()
            .and_then(|index| PAGES.get(index))
            .copied()
            .map(SettingsPage::for_platform)
        else {
            return;
        };
        if action != "settings-page"
            || target == SettingsPage::Account && !self.deepseek_account.available
        {
            return;
        }
        self.transcript_preferences.clear_menu();
        if target == SettingsPage::Providers {
            self.open_model_providers(w, cx);
        } else if target == SettingsPage::Account {
            self.open_account_settings(w, cx);
        } else {
            self.settings_page = target;
            if target == SettingsPage::WebSearch {
                self.prepare_web_search_settings(w, cx);
            }
            cx.notify();
        }
    }
}

impl DshApp {
    pub(super) fn persist_host_theme(&self, changes: Value) {
        if bridge::is_mac()
            && self.settings_shell.theme_writable
            && let Ok(value) = changes.serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        {
            save_client_theme(value);
        }
    }
}

pub(super) fn theme_rows() -> impl IntoElement {
    div()
        .w_full()
        .h(px(settings_theme_height()))
        .flex_none()
        .child(
            gpui::canvas(
                |bounds, _, _| {
                    place_settings_theme(
                        bounds.left().as_f32(),
                        bounds.top().as_f32(),
                        bounds.size.width.as_f32(),
                    );
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
        .relative()
}

pub(super) fn plugin_inventory() -> gpui::Div {
    div().size_full().relative().child(
        gpui::canvas(
            |bounds, _, _| {
                place_settings_plugins(
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
}

pub(super) fn agent_presets() -> gpui::Div {
    div().size_full().relative().child(
        gpui::canvas(
            |bounds, _, _| {
                place_settings_agent_presets(
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

pub(super) fn upload_row() -> impl IntoElement {
    div()
        .w_full()
        .h(px(settings_upload_height()))
        .flex_none()
        .relative()
        .child(
            gpui::canvas(
                |bounds, _, _| {
                    place_settings_upload(
                        bounds.left().as_f32(),
                        bounds.top().as_f32(),
                        bounds.size.width.as_f32(),
                    )
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
}

pub(super) fn permission_row() -> impl IntoElement {
    div()
        .w_full()
        .h(px(settings_permission_height()))
        .flex_none()
        .relative()
        .child(
            gpui::canvas(
                |bounds, _, _| {
                    place_settings_permission(
                        bounds.left().as_f32(),
                        bounds.top().as_f32(),
                        bounds.size.width.as_f32(),
                    )
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
}

pub(super) fn measure() -> impl IntoElement {
    gpui::canvas(
        |bounds, _, _| {
            place_settings_shell(
                bounds.left().as_f32(),
                bounds.top().as_f32(),
                bounds.size.width.as_f32(),
                bounds.size.height.as_f32(),
            );
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

pub(super) fn developer_row() -> impl IntoElement {
    div()
        .w_full()
        .h(px(settings_developer_height()))
        .flex_none()
        .relative()
        .child(
            gpui::canvas(
                |bounds, _, _| {
                    place_settings_developer(
                        bounds.left().as_f32(),
                        bounds.top().as_f32(),
                        bounds.size.width.as_f32(),
                    )
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
}
