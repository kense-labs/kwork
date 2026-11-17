//! Model button menu navigation across its root, model, and effort panes.

use super::menu_groups::MenuViewport;
use super::*;

#[wasm_bindgen(raw_module = "../model-metadata-search.js")]
extern "C" {
    pub(super) fn rank_model_choices(models: JsValue, query: &str) -> js_sys::Uint32Array;
}

pub(super) struct ModelMenu {
    pub(super) scope: gpui::FocusHandle,
    pub(super) trigger: gpui::FocusHandle,
    pub(super) row: gpui::FocusHandle,
    pub(super) active: Option<usize>,
    pub(super) query: String,
    searchable: bool,
    pub(super) viewport: MenuViewport,
    pub(super) chevron: DockSlide,
    pub(super) selection_focus: bool,
    pub(super) error: Option<String>,
    _focus_subscriptions: [Subscription; 2],
}

impl ModelMenu {
    pub(super) fn new(w: &mut Window, cx: &mut Context<DshApp>) -> Self {
        let scope = cx.focus_handle();
        let trigger = cx.focus_handle();
        let leave_menu = cx.on_focus_out(&scope, w, |app, _, _, cx| {
            if app.composer_menu.is_some_and(ComposerMenu::is_model) {
                app.composer_menu = None;
                app.model_menu.active = None;
                cx.notify();
            }
        });
        let leave_trigger = cx.on_blur(&trigger, w, |app, _, cx| {
            if std::mem::take(&mut app.model_menu.selection_focus) {
                cx.notify();
            }
        });
        Self {
            scope,
            trigger,
            row: cx.focus_handle(),
            active: None,
            query: String::new(),
            searchable: false,
            viewport: MenuViewport::default(),
            chevron: DockSlide::with_curve(120., [0.25, 0.1, 0.25, 1.]),
            selection_focus: false,
            error: None,
            _focus_subscriptions: [leave_menu, leave_trigger],
        }
    }
}

impl DshApp {
    pub(super) fn model_menu_width(
        &self,
        pane: ComposerMenu,
        model: &str,
        w: &Window,
        cx: &App,
    ) -> gpui::Pixels {
        let style = super::document_fonts::interface_style(w, cx);
        let width = |label: &str, font_size: f32| {
            w.text_system()
                .shape_line(
                    label.to_owned().into(),
                    px(font_size),
                    &[style.to_run(label.len())],
                    None,
                )
                .width()
        };
        let content = if pane == ComposerMenu::Model {
            let model = width("模型", 13.) + width(model, 13.);
            let effort = self
                .current_config_label("reasoning_effort")
                .map_or(px(0.), |label| width("推理等级", 13.) + width(&label, 13.));
            model.max(effort) + px(40.)
        } else if pane == ComposerMenu::Team {
            px(372.)
        } else {
            self.model_menu_groups(pane, cx)
                .into_iter()
                .map(|(group, choices)| {
                    let group_width = width(&group, 11.) + px(14.);
                    choices
                        .into_iter()
                        .map(|choice| width(&choice.label, 13.) + px(34.))
                        .fold(group_width, gpui::Pixels::max)
                })
                .fold(px(0.), gpui::Pixels::max)
        };
        content + px(8.)
    }

    pub(super) fn config_option(&self, id: &str) -> Option<&Value> {
        self.conversation
            .config_options
            .iter()
            .find(|option| option["id"].as_str() == Some(id))
    }

    pub(super) fn model_menu_searchable(&self) -> bool {
        self.model_choice_groups("model")
            .iter()
            .map(|(_, choices)| choices.len())
            .sum::<usize>()
            > 4
    }

    pub(super) fn filtered_model_groups(&self, query: &str) -> Vec<(String, Vec<PickerOption>)> {
        self.config_choice_groups("model", query, true)
    }

    pub(super) fn command_model_groups(&self, query: &str) -> Vec<(String, Vec<PickerOption>)> {
        self.config_choice_groups("model", query, false)
    }

    pub(super) fn model_choice_groups(&self, config_id: &str) -> Vec<(String, Vec<PickerOption>)> {
        self.config_choice_groups(config_id, "", false)
    }

    fn config_choice_groups(
        &self,
        config_id: &str,
        query: &str,
        include_model_metadata: bool,
    ) -> Vec<(String, Vec<PickerOption>)> {
        let catalog = (config_id == "model")
            .then(|| self.model_catalog.groups())
            .flatten();
        let Some(items) = catalog.or_else(|| {
            self.config_option(config_id)
                .and_then(|option| option["options"].as_array().map(Vec::as_slice))
        }) else {
            return Vec::new();
        };
        let mut groups: Vec<(u8, String, Vec<&Value>)> = Vec::new();
        let mut ungrouped = None;
        for item in items {
            if let Some(nested) = item["options"].as_array() {
                let provider = item["group"].as_str();
                let name = if config_id == "model" && provider == Some("deepseek-account") {
                    "DeepSeek 账号"
                } else {
                    item["name"].as_str().or(provider).unwrap_or("模型")
                };
                let priority = match (config_id, provider) {
                    ("model", Some("deepseek-account")) => 0,
                    ("model", Some("deepseek-official")) => 1,
                    _ => 2,
                };
                groups.push((priority, name.to_owned(), nested.iter().collect()));
            } else {
                let index = *ungrouped.get_or_insert_with(|| {
                    groups.push((2, "模型".to_owned(), Vec::new()));
                    groups.len() - 1
                });
                groups[index].2.push(item);
            }
        }
        groups.sort_by_key(|group| group.0);
        groups
            .into_iter()
            .filter_map(|(_, name, source)| {
                let rows: Vec<_> = source
                    .into_iter()
                    .filter_map(|item| picker_choice(item).map(|choice| (choice, item)))
                    .collect();
                let selected = if config_id == "model" && !query.is_empty() && !rows.is_empty() {
                    let keys: Vec<_> = rows
                        .iter()
                        .map(|(choice, item)| {
                            if include_model_metadata {
                                let id = serde_json::from_str::<[String; 2]>(&choice.value)
                                    .map_or_else(|_| choice.value.to_string(), |[_, id]| id);
                                (
                                    choice.label.as_ref(),
                                    Some(id),
                                    item["description"].as_str(),
                                )
                            } else {
                                (choice.label.as_ref(), None, None)
                            }
                        })
                        .collect();
                    let keys =
                        serde_wasm_bindgen::to_value(&keys).expect("serialize model search keys");
                    rank_model_choices(keys, query)
                        .to_vec()
                        .into_iter()
                        .map(|position| rows[position as usize].0.clone())
                        .collect::<Vec<_>>()
                } else {
                    rows.into_iter().map(|(choice, _)| choice).collect()
                };
                (!selected.is_empty()).then_some((name, selected))
            })
            .collect()
    }

    pub(super) fn model_menu_groups(
        &self,
        pane: ComposerMenu,
        cx: &App,
    ) -> Vec<(String, Vec<PickerOption>)> {
        if pane == ComposerMenu::Team {
            return Vec::new();
        }
        if pane == ComposerMenu::ModelChoices {
            let input = self.model_filter_editor.read(cx);
            let query = if self.model_menu_searchable() {
                input.value()
            } else {
                ""
            };
            self.filtered_model_groups(query)
        } else {
            self.model_choice_groups("reasoning_effort")
        }
    }

    pub(super) fn toggle_model_menu(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        self.model_menu.selection_focus = false;
        self.model_menu.error = None;
        if !self.composer_menu.is_some_and(ComposerMenu::is_model) {
            self.load_model_catalog(w, cx);
        }
        if !self.composer_menu.is_some_and(ComposerMenu::is_model)
            && !self.model_menu_has_current_choice()
        {
            self.drill_model_menu(ComposerMenu::ModelChoices, w, cx);
            return;
        }
        self.composer_menu = if self.composer_menu.is_some_and(ComposerMenu::is_model) {
            None
        } else {
            Some(ComposerMenu::Model)
        };
        self.model_menu.active = None;
        self.model_menu.trigger.focus(w, cx);
        cx.notify();
    }

    pub(super) fn drill_model_menu(
        &mut self,
        pane: ComposerMenu,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composer_menu != Some(pane) {
            self.model_menu.error = None;
        }
        if pane == ComposerMenu::ModelChoices {
            self.model_menu.searchable = self.model_menu_searchable();
            self.model_menu.query.clear();
            let busy = self.model_menu_busy();
            self.model_filter_editor
                .update(cx, |editor, _| editor.disabled = busy);
            replace(&self.model_filter_editor, "", w, cx);
        }
        self.composer_menu = Some(pane);
        self.model_menu.viewport.reset();
        let groups = self.model_menu_groups(pane, cx);
        let index = self.current_model_menu_index(pane, &groups);
        self.highlight_model_menu(pane, &groups, index, w, cx);
    }

    pub(super) fn model_menu_has_current_choice(&self) -> bool {
        self.model_menu_current_label().is_some()
    }

    pub(super) fn model_menu_placeholder(&self) -> &'static str {
        if self.model_catalog.loading() {
            "正在加载模型…"
        } else {
            "请选择模型"
        }
    }

    pub(super) fn model_menu_current_label(&self) -> Option<String> {
        let option = self.config_option("model")?;
        let current = option["currentValue"]
            .as_str()
            .filter(|value| !value.is_empty())?;
        if self.model_catalog.groups().is_none() && !option["options"].is_array() {
            return self.current_config_label("model");
        }
        self.model_choice_groups("model")
            .into_iter()
            .flat_map(|(_, options)| options)
            .find(|choice| choice.value.as_ref() == current)
            .map(|choice| choice.label.to_string())
    }

    pub(super) fn refresh_model_menu(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let busy = self.model_menu_busy();
        self.model_filter_editor
            .update(cx, |editor, _| editor.disabled = busy);
        if busy || self.composer_menu != Some(ComposerMenu::ModelChoices) {
            return;
        }
        if self.model_menu.searchable != self.model_menu_searchable() {
            // The focused search field can unmount while the directory is
            // open. Hand focus to the checked row or to the new search field.
            self.drill_model_menu(ComposerMenu::ModelChoices, w, cx);
        }
    }

    fn current_model_menu_index(
        &self,
        pane: ComposerMenu,
        groups: &[(String, Vec<PickerOption>)],
    ) -> usize {
        if pane == ComposerMenu::Model {
            return 0;
        }
        let current = self
            .config_option(config_id(pane))
            .and_then(|option| option["currentValue"].as_str());
        groups
            .iter()
            .flat_map(|(_, choices)| choices)
            .position(|choice| Some(choice.value.as_ref()) == current)
            .unwrap_or(0)
    }

    pub(super) fn back_model_menu(
        &mut self,
        pane: ComposerMenu,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model_menu.error = None;
        if pane == ComposerMenu::Model
            || (pane == ComposerMenu::ModelChoices && !self.model_menu_has_current_choice())
        {
            self.composer_menu = None;
            self.model_menu.active = None;
            self.model_menu.trigger.focus(w, cx);
        } else {
            self.composer_menu = Some(ComposerMenu::Model);
            self.model_menu.active = Some(match pane {
                ComposerMenu::ReasoningChoices => 1,
                ComposerMenu::Team => {
                    1 + usize::from(self.current_config_label("reasoning_effort").is_some())
                }
                _ => 0,
            });
            self.model_menu.row.focus(w, cx);
        }
        cx.notify();
    }

    fn highlight_model_menu(
        &mut self,
        pane: ComposerMenu,
        groups: &[(String, Vec<PickerOption>)],
        index: usize,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model_menu.active = if pane == ComposerMenu::Model || pane == ComposerMenu::Team {
            Some(index)
        } else {
            let count: usize = groups.iter().map(|(_, choices)| choices.len()).sum();
            (count > 0).then(|| index.min(count - 1))
        };
        if pane != ComposerMenu::Model {
            let mut skipped = 0;
            let mut headers = 0;
            for (name, choices) in groups {
                headers += usize::from(groups.len() > 1 || name != "模型");
                skipped += choices.len();
                if index < skipped {
                    self.model_menu.viewport.reveal(index + headers);
                    break;
                }
            }
        }
        if self.model_menu_busy() {
            self.model_menu.trigger.focus(w, cx);
        } else if pane == ComposerMenu::ModelChoices && self.model_menu_searchable() {
            self.model_filter_editor
                .update(cx, |editor, cx| editor.focus(w, cx));
        } else if self.model_menu.active.is_some() {
            self.model_menu.row.focus(w, cx);
        } else {
            self.model_menu.trigger.focus(w, cx);
        }
        cx.notify();
    }

    pub(super) fn hover_model_choice(
        &mut self,
        pane: ComposerMenu,
        index: usize,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.model_menu_busy() && self.model_menu.active != Some(index) {
            self.model_menu.active = Some(index);
            if pane != ComposerMenu::ModelChoices || !self.model_menu_searchable() {
                self.model_menu.row.focus(w, cx);
            }
            cx.notify();
        }
    }

    pub(super) fn choose_model_menu(
        &mut self,
        pane: ComposerMenu,
        value: String,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model_menu_busy() {
            return;
        }
        self.model_menu.error = None;
        let id = config_id(pane);
        let current = self
            .config_option(id)
            .and_then(|option| option["currentValue"].as_str());
        if current == Some(value.as_str()) {
            self.model_menu.selection_focus = true;
            self.back_model_menu(ComposerMenu::Model, w, cx);
        } else if let Some(session) = self.active_session.as_ref() {
            let args = json!({"sessionId": session, "configId": id, "value": value});
            self.conversation.model_menu_pending = Some((pane, value));
            self.model_menu.selection_focus = true;
            self.model_filter_editor
                .update(cx, |editor, _| editor.disabled = true);
            self.model_menu.trigger.focus(w, cx);
            invoke_with_reply("set_config_option", "button_model", args);
            cx.notify();
        }
    }

    pub(super) fn model_menu_busy(&self) -> bool {
        self.conversation.model_menu_pending.is_some()
            || self.conversation.command_model_pending
            || self.conversation.team_pending
    }

    pub(super) fn finish_model_menu(
        &mut self,
        owner: Option<&str>,
        result: Result<Value, String>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = owner else {
            return;
        };
        let selected = self.active_session.as_deref() == Some(session);
        let Some(conversation) = self.conversation_mut(session) else {
            return;
        };
        let Some((pane, _)) = conversation.model_menu_pending.take() else {
            return;
        };
        match result.and_then(|value| {
            value["configOptions"]
                .as_array()
                .cloned()
                .ok_or_else(|| "无法读取模型选项。".into())
        }) {
            Ok(options) => {
                conversation.config_options = options;
                if selected {
                    self.rebuild_selectors(w, cx);
                    if self.composer_menu.is_some_and(ComposerMenu::is_model) {
                        self.back_model_menu(ComposerMenu::Model, w, cx);
                    }
                }
            }
            Err(error) if selected => {
                self.model_filter_editor
                    .update(cx, |editor, _| editor.disabled = false);
                if self.composer_menu == Some(pane)
                    && pane == ComposerMenu::ModelChoices
                    && self.model_menu_searchable()
                {
                    self.model_filter_editor
                        .update(cx, |editor, cx| editor.focus(w, cx));
                }
                let message = format!("模型操作失败：{error}");
                if self.composer_menu == Some(pane) {
                    self.model_menu.error = Some(message.clone());
                }
                self.push_toast(message, cx);
            }
            Err(error) => self.push_toast(format!("模型操作失败：{error}"), cx),
        }
    }

    pub(super) fn handle_model_menu_key(
        &mut self,
        combo: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.model_menu.scope.contains_focused(w, cx) {
            return false;
        }
        self.navigate_model_menu(combo, false, w, cx)
    }

    pub(super) fn navigate_model_menu(
        &mut self,
        combo: &str,
        native: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(pane) = self.composer_menu.filter(|pane| pane.is_model()) else {
            return false;
        };
        if matches!(combo, "Escape" | "shift+Tab") {
            self.back_model_menu(pane, w, cx);
            return true;
        }
        if !matches!(combo, "ArrowDown" | "ArrowUp" | "Enter" | "Tab") {
            return false;
        }
        if self.model_menu_busy() {
            return true;
        }
        let groups = if pane == ComposerMenu::Model {
            Vec::new()
        } else {
            self.model_menu_groups(pane, cx)
        };
        let count = if pane == ComposerMenu::Model {
            1 + usize::from(self.current_config_label("reasoning_effort").is_some())
                + usize::from(self.conversation.acp_open && self.conversation.team.is_some())
        } else if pane == ComposerMenu::Team {
            1
        } else {
            groups.iter().map(|(_, choices)| choices.len()).sum()
        };
        if !native
            && pane == ComposerMenu::ModelChoices
            && self.handle_model_catalog_retry_key(combo, count == 0, w, cx)
        {
            return true;
        }
        let in_pane = (native && self.model_menu.active.is_some())
            || self.model_menu.row.is_focused(w)
            || (pane == ComposerMenu::ModelChoices
                && self.model_filter_editor.read(cx).focus.is_focused(w));
        let active = if in_pane && count > 0 {
            Some(self.model_menu.active.unwrap_or(0).min(count - 1))
        } else {
            None
        };
        match combo {
            "ArrowDown" | "ArrowUp" if count > 0 => {
                let next = if combo == "ArrowDown" {
                    active.map_or(0, |index| (index + 1) % count)
                } else {
                    active.map_or(count - 1, |index| (index + count - 1) % count)
                };
                self.highlight_model_menu(pane, &groups, next, w, cx);
            }
            "Tab" if active.is_none() && count > 0 => {
                let index = self.current_model_menu_index(pane, &groups);
                self.highlight_model_menu(pane, &groups, index, w, cx);
            }
            "Enter" | "Tab" if pane == ComposerMenu::Model && active.is_some() => {
                let target = match active {
                    Some(0) => ComposerMenu::ModelChoices,
                    Some(1) if self.current_config_label("reasoning_effort").is_some() => {
                        ComposerMenu::ReasoningChoices
                    }
                    _ => ComposerMenu::Team,
                };
                self.drill_model_menu(target, w, cx);
            }
            "Enter" | "Tab" if pane == ComposerMenu::Team => {
                let enabled = self
                    .conversation
                    .team
                    .as_ref()
                    .is_some_and(|team| team.enabled);
                self.choose_team_mode(!enabled, w, cx);
            }
            "Enter" | "Tab" if active.is_some() => {
                if let Some(choice) = groups
                    .into_iter()
                    .flat_map(|(_, choices)| choices)
                    .nth(active.unwrap())
                {
                    self.choose_model_menu(pane, choice.value.to_string(), w, cx);
                }
            }
            "ArrowDown" | "ArrowUp" => {}
            "Enter" if pane != ComposerMenu::Model => {}
            _ => return false,
        }
        true
    }
}

pub(super) fn config_id(pane: ComposerMenu) -> &'static str {
    if pane == ComposerMenu::ModelChoices {
        "model"
    } else {
        "reasoning_effort"
    }
}
