//! Expose the canvas model pickers through the webview's accessibility tree.

use super::permission_dialog::AccessiblePermissionRisk;
use super::team_mode::AccessibleTeamDisable;
use super::*;
use serde::Serialize;

#[wasm_bindgen(module = "/model-accessibility.js")]
extern "C" {
    fn update_model_selector(selector: JsValue, team_disable: JsValue, permission_risk: JsValue);
    pub(super) fn listen_model_selector(callback: &js_sys::Function);
}

#[derive(Default)]
pub(super) struct ModelAccessibility {
    pub(super) rendered_label: Option<String>,
    pub(super) rendered_composer: bool,
    pub(super) input_label: Option<&'static str>,
    visible: Option<Selector>,
    team_disable: Option<AccessibleTeamDisable>,
    permission_risk: Option<AccessiblePermissionRisk>,
}

#[derive(PartialEq, Eq, Serialize)]
struct Selector {
    label: Option<String>,
    input_label: Option<&'static str>,
    loading: bool,
    busy: bool,
    permission_label: Option<String>,
    permission_busy: bool,
    pane: Option<Pane>,
}

#[derive(PartialEq, Eq, Serialize)]
struct Pane {
    kind: &'static str,
    busy: bool,
    query: Option<String>,
    active: Option<usize>,
    groups: Vec<Group>,
    status: Option<&'static str>,
    errors: Vec<String>,
    retry: bool,
}

#[derive(PartialEq, Eq, Serialize)]
struct Group {
    label: String,
    choices: Vec<Choice>,
}

#[derive(PartialEq, Eq, Serialize)]
struct Choice {
    label: String,
    value: String,
    checked: Option<bool>,
    disabled: bool,
}

fn pane_kind(pane: ComposerMenu) -> Option<&'static str> {
    match pane {
        ComposerMenu::Model => Some("root"),
        ComposerMenu::ModelChoices => Some("model"),
        ComposerMenu::Commands => Some("commands"),
        ComposerMenu::CommandModels => Some("command"),
        ComposerMenu::ReasoningChoices => Some("effort"),
        ComposerMenu::Team => Some("team"),
        ComposerMenu::Permission => Some("permission"),
        ComposerMenu::PermissionChoices => Some("permission-command"),
        _ => None,
    }
}

impl DshApp {
    pub(super) fn composer_tools_available(&self) -> bool {
        self.model_accessibility.rendered_composer
            && self.workspace_tree_available()
            && self.subagent_view.is_none()
    }

    pub(super) fn publish_accessible_model_selector(&mut self, cx: &App) {
        let selector = self.composer_tools_available().then(|| Selector {
            label: self.model_accessibility.rendered_label.clone(),
            input_label: self.model_accessibility.input_label,
            loading: self.model_catalog.loading(),
            busy: self.model_menu_busy(),
            permission_label: self
                .conversation
                .permission_mode
                .state
                .as_ref()
                .map(|state| format!("访问模式，当前：{}", state.current_label())),
            permission_busy: self.conversation.permission_mode.changing(),
            pane: self
                .composer_menu
                .and_then(|pane| self.model_selector_pane(pane, cx)),
        });
        let team_disable = self.accessible_team_disable();
        let permission_risk = self.accessible_permission_risk();
        if (selector != self.model_accessibility.visible
            || team_disable != self.model_accessibility.team_disable
            || permission_risk != self.model_accessibility.permission_risk)
            && let (Ok(value), Ok(dialog), Ok(risk)) = (
                serde_wasm_bindgen::to_value(&selector),
                serde_wasm_bindgen::to_value(&team_disable),
                serde_wasm_bindgen::to_value(&permission_risk),
            )
        {
            self.model_accessibility.visible = selector;
            self.model_accessibility.team_disable = team_disable;
            self.model_accessibility.permission_risk = permission_risk;
            update_model_selector(value, dialog, risk);
        }
    }

    fn model_selector_pane(&self, pane: ComposerMenu, cx: &App) -> Option<Pane> {
        let kind = pane_kind(pane)?;
        let mut view = Pane {
            kind,
            busy: self.model_menu_busy(),
            query: None,
            active: self.model_menu.active,
            groups: Vec::new(),
            status: None,
            errors: Vec::new(),
            retry: false,
        };
        match pane {
            ComposerMenu::Permission | ComposerMenu::PermissionChoices => {
                let mode = &self.conversation.permission_mode;
                let command = pane == ComposerMenu::PermissionChoices;
                view.busy = mode.changing() || mode.loading();
                let choices = if command {
                    view.query = Some(self.command_picker.input.read(cx).value().to_owned());
                    view.errors.extend(mode.error.iter().cloned());
                    view.retry = mode.can_retry();
                    let choices = self.command_options(pane, cx);
                    view.status = if mode.loading() {
                        Some("正在加载选项…")
                    } else if mode.changing() {
                        Some("正在应用…")
                    } else if choices.is_empty() && mode.error.is_none() {
                        Some("无选项")
                    } else {
                        None
                    };
                    view.active = choices
                        .len()
                        .checked_sub(1)
                        .map(|last| self.command_picker.active.min(last));
                    choices
                        .into_iter()
                        .map(|option| {
                            let mut label = option.label;
                            if option.experimental {
                                label.push_str(" EXP");
                            }
                            if let Some(detail) = option.detail {
                                label.push_str(&format!("，{detail}"));
                            }
                            Choice {
                                label,
                                value: option.value,
                                checked: Some(option.current),
                                disabled: false,
                            }
                        })
                        .collect()
                } else {
                    let state = mode.state.as_ref()?;
                    view.active = self.permission_menu.active;
                    state
                        .options
                        .iter()
                        .map(|option| Choice {
                            label: if option.value == "auto" {
                                format!("{} EXP", option.label())
                            } else {
                                option.label().into()
                            },
                            value: option.value.clone(),
                            checked: Some(option.value == state.current_value),
                            disabled: false,
                        })
                        .collect()
                };
                view.groups.push(Group {
                    label: String::new(),
                    choices,
                });
            }
            ComposerMenu::Commands => {
                view.busy = false;
                let catalog = self.command_catalog.catalog();
                let choices = visible_commands(&catalog, &self.command_filter)
                    .into_iter()
                    .map(|spec| Choice {
                        label: format!("{} /{}，{}", spec.title, spec.key, spec.description),
                        value: spec.key.into(),
                        checked: None,
                        disabled: !spec.enabled,
                    })
                    .collect::<Vec<_>>();
                view.active = choices
                    .len()
                    .checked_sub(1)
                    .map(|last| self.command_selected.min(last));
                view.groups.push(Group {
                    label: String::new(),
                    choices,
                });
            }
            ComposerMenu::Model => {
                let model = self
                    .model_menu_current_label()
                    .unwrap_or_else(|| self.model_menu_placeholder().into());
                let mut choices = vec![root_choice("模型", model, "model")];
                if let Some(effort) = self.current_config_label("reasoning_effort") {
                    choices.push(root_choice("推理等级", effort, "effort"));
                }
                if self.conversation.acp_open
                    && let Some(team) = &self.conversation.team
                {
                    let label = if team.enabled { "开启" } else { "关闭" };
                    choices.push(root_choice("智能体团队协作", label.into(), "team"));
                }
                view.groups.push(Group {
                    label: String::new(),
                    choices,
                });
            }
            ComposerMenu::Team => {
                let team = self.conversation.team.as_ref()?;
                view.groups.push(Group {
                    label: String::new(),
                    choices: vec![Choice {
                        label: "开启智能体团队协作".into(),
                        value: "team".into(),
                        checked: Some(team.enabled),
                        disabled: team.has_used_model.is_none(),
                    }],
                });
            }
            _ => {
                let catalog = pane != ComposerMenu::ReasoningChoices;
                let command = pane == ComposerMenu::CommandModels;
                let query = if command {
                    Some(self.command_picker.input.read(cx).value())
                } else if pane == ComposerMenu::ModelChoices && self.model_menu_searchable() {
                    Some(self.model_filter_editor.read(cx).value())
                } else {
                    None
                };
                view.query = query.map(str::to_owned);
                let groups = if command {
                    self.command_model_groups(query.unwrap_or_default())
                } else {
                    self.model_menu_groups(pane, cx)
                };
                let count = groups
                    .iter()
                    .map(|(_, choices)| choices.len())
                    .sum::<usize>();
                let active = if command {
                    self.command_picker.active
                } else {
                    view.active.unwrap_or(0)
                };
                view.active = (count > 0).then(|| active.min(count - 1));
                let config = if command {
                    "model"
                } else {
                    model_menu::config_id(pane)
                };
                let current = self
                    .config_option(config)
                    .and_then(|option| option["currentValue"].as_str());
                view.groups = groups
                    .into_iter()
                    .map(|(label, choices)| Group {
                        label,
                        choices: choices
                            .into_iter()
                            .map(|choice| Choice {
                                label: choice.label.to_string(),
                                checked: Some(current == Some(choice.value.as_ref())),
                                value: choice.value.to_string(),
                                disabled: false,
                            })
                            .collect(),
                    })
                    .collect();
                if catalog {
                    view.errors = self.model_catalog.warnings();
                    view.retry = self.model_catalog.can_retry() && !self.model_catalog.loading();
                }
                let error = if command {
                    &self.command_picker.model_error
                } else {
                    &self.model_menu.error
                };
                view.errors.extend(error.iter().cloned());
                view.status = if self.model_menu_busy() {
                    Some("正在应用…")
                } else if catalog && self.model_catalog.loading() {
                    Some("正在刷新模型列表…")
                } else if count > 0 || (command && error.is_some()) {
                    None
                } else if !catalog {
                    Some("当前模型未提供推理等级。")
                } else if query.is_some_and(|q| !q.trim().is_empty())
                    && (!command || !self.model_choice_groups("model").is_empty())
                {
                    Some("没有匹配的模型。")
                } else {
                    Some("暂无可用模型")
                };
            }
        }
        Some(view)
    }

    pub(super) fn handle_accessible_model_action(
        &mut self,
        action: &str,
        value: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            action,
            "composer-submit"
                | "composer-preference"
                | "composer-preference-close"
                | "composer-general-settings"
                | "composer-model-providers"
                | "composer-account-settings"
                | "composer-more-settings"
                | "composer-open-settings"
        ) {
            self.accessible_composer_action(action, value, w, cx);
            return;
        }
        if matches!(action, "team-disable-confirm" | "team-disable-cancel") {
            self.act_on_team_disable(value, action == "team-disable-confirm", w, cx);
            return;
        }
        if matches!(
            action,
            "permission-risk-acknowledge" | "permission-risk-confirm" | "permission-risk-cancel"
        ) {
            self.act_on_permission_risk(action, value, w, cx);
            return;
        }
        if action.starts_with("provider-") {
            self.handle_model_provider_form(action, value, w, cx);
            return;
        }
        if action == "welcome-account" {
            self.open_welcome_account(w, cx);
            return;
        }
        if action == "account-notice-open" {
            self.open_account_from_notice(w, cx);
            return;
        }
        if matches!(action, "account-quota-dismiss" | "account-quota-top-up") {
            self.handle_quota_notice(action, value, cx);
            return;
        }
        if action.starts_with("account-menu-") {
            self.handle_account_launcher(action, w, cx);
            return;
        }
        if action.starts_with("account-") {
            self.handle_account_form(action, w, cx);
            return;
        }
        if action.starts_with("onboarding-") {
            self.handle_desktop_onboarding(action, value, w, cx);
            return;
        }
        if action.starts_with("welcome-") {
            if self.show_welcome {
                self.welcome.update(cx, |welcome, cx| {
                    welcome.handle_form_action(action, value, w, cx)
                });
            }
            return;
        }
        if matches!(action, "attachment-remove" | "attachment-download") {
            self.accessible_attachment_action(action, value, cx);
            return;
        }
        if !self.composer_tools_available() {
            return;
        }
        if action == "toggle-commands" {
            self.toggle_command_menu(w, cx);
            return;
        }
        if action == "toggle-permission" {
            if self.conversation.permission_mode.state.is_some() {
                self.toggle_permission_menu(w, cx);
            }
            return;
        }
        if self.composer_menu == Some(ComposerMenu::Commands) && action != "toggle" {
            match action {
                "key" => {
                    self.navigate_command_menu(value, w, cx);
                }
                "dismiss" => {
                    self.dismiss_command_menu();
                    cx.notify();
                }
                "choose" => {
                    let catalog = self.command_catalog.catalog();
                    if let Some(spec) = visible_commands(&catalog, &self.command_filter)
                        .into_iter()
                        .find(|spec| spec.key == value && spec.enabled)
                    {
                        self.activate_command(spec.key, w, cx);
                    }
                }
                _ => {}
            }
            return;
        }
        if action == "toggle" {
            if self.config_option("model").is_some() {
                self.toggle_model_menu(w, cx);
            }
            return;
        }
        let Some(pane) = self.composer_menu.filter(|pane| pane_kind(*pane).is_some()) else {
            return;
        };
        if action == "dismiss" {
            if pane.is_command_picker() {
                self.dismiss_command_picker(w, cx);
            } else {
                self.composer_menu = None;
                self.model_menu.active = None;
                cx.notify();
            }
            return;
        }
        if action == "key" {
            if pane.is_command_picker() {
                self.navigate_command_picker(pane, value, true, w, cx);
            } else if pane == ComposerMenu::Permission {
                self.navigate_permission_menu(value, true, w, cx);
            } else {
                self.navigate_model_menu(value, true, w, cx);
            }
            return;
        }
        let catalog = matches!(
            pane,
            ComposerMenu::ModelChoices | ComposerMenu::CommandModels
        );
        if action == "retry" && catalog {
            self.retry_model_catalog(w, cx);
            return;
        }
        if action == "retry" && pane == ComposerMenu::PermissionChoices {
            self.retry_permission_mode(w, cx);
            return;
        }
        let permission = matches!(
            pane,
            ComposerMenu::Permission | ComposerMenu::PermissionChoices
        );
        if if permission {
            self.conversation.permission_mode.changing()
                || self.conversation.permission_mode.loading()
        } else {
            self.model_menu_busy()
        } {
            return;
        }
        if action == "query" {
            if pane.is_command_picker() {
                replace(&self.command_picker.input, value, w, cx);
            } else if catalog && self.model_menu_searchable() {
                replace(&self.model_filter_editor, value, w, cx);
            }
            return;
        }
        if action != "choose"
            || !self.model_selector_pane(pane, cx).is_some_and(|view| {
                view.groups
                    .iter()
                    .flat_map(|group| &group.choices)
                    .any(|choice| !choice.disabled && choice.value == value)
            })
        {
            return;
        }
        match pane {
            ComposerMenu::Model => {
                let target = match value {
                    "model" => ComposerMenu::ModelChoices,
                    "effort" => ComposerMenu::ReasoningChoices,
                    _ => ComposerMenu::Team,
                };
                self.drill_model_menu(target, w, cx);
            }
            ComposerMenu::Team => {
                let enabled = self
                    .conversation
                    .team
                    .as_ref()
                    .is_some_and(|team| team.enabled);
                self.choose_team_mode(!enabled, w, cx);
            }
            ComposerMenu::CommandModels | ComposerMenu::PermissionChoices => {
                self.choose_command_option(pane, value.into(), w, cx)
            }
            ComposerMenu::Permission => self.choose_permission(value.into(), w, cx),
            _ => self.choose_model_menu(pane, value.into(), w, cx),
        }
    }
}

fn root_choice(label: &str, current: String, value: &str) -> Choice {
    Choice {
        label: format!("{label}，当前 {current}"),
        value: value.into(),
        checked: None,
        disabled: false,
    }
}
