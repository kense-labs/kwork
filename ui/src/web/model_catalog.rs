//! The Host model directory is shared by the composer button and /model.

use super::*;
use serde::Deserialize;

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Loading,
    Ready,
    Unsupported,
    Error(String),
}

#[derive(Deserialize)]
struct CatalogSnapshot {
    groups: Vec<Value>,
    failures: Vec<ProviderFailure>,
}

#[derive(Deserialize)]
struct ProviderFailure {
    id: String,
    name: String,
    message: String,
}

impl ProviderFailure {
    fn label(&self) -> String {
        format!("{} 加载失败：{}", self.name, self.message)
    }
}

pub(super) struct ModelCatalog {
    generation: u64,
    dirty: bool,
    phase: Phase,
    snapshot: Option<CatalogSnapshot>,
    retry_error: gpui::FocusHandle,
    retry_providers: HashMap<String, gpui::FocusHandle>,
}

impl ModelCatalog {
    pub(super) fn new(cx: &mut Context<DshApp>) -> Self {
        Self {
            generation: 0,
            dirty: false,
            phase: Phase::Idle,
            snapshot: None,
            retry_error: cx.focus_handle(),
            retry_providers: HashMap::new(),
        }
    }

    pub(super) fn reset(&mut self) {
        self.generation += 1;
        self.dirty = false;
        self.phase = Phase::Idle;
        self.snapshot = None;
        self.retry_providers.clear();
    }

    pub(super) fn groups(&self) -> Option<&[Value]> {
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.groups.as_slice())
    }

    fn failures(&self) -> &[ProviderFailure] {
        self.snapshot
            .as_ref()
            .map_or(&[], |snapshot| snapshot.failures.as_slice())
    }

    pub(super) fn loading(&self) -> bool {
        matches!(self.phase, Phase::Loading)
    }

    pub(super) fn can_retry(&self) -> bool {
        matches!(self.phase, Phase::Error(_)) || !self.failures().is_empty()
    }

    fn error_label(&self) -> Option<String> {
        if let Phase::Error(error) = &self.phase {
            Some(format!("模型操作失败：{error}"))
        } else {
            None
        }
    }

    pub(super) fn warnings(&self) -> Vec<String> {
        self.error_label()
            .into_iter()
            .chain(self.failures().iter().map(ProviderFailure::label))
            .collect()
    }
}

impl DshApp {
    pub(super) fn load_model_catalog(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.model_catalog.phase, Phase::Idle | Phase::Error(_)) {
            self.request_model_catalog(w, cx);
        }
    }

    pub(super) fn model_catalog_changed(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        self.refresh_visible_model_providers(w, cx);
        if self.model_catalog.loading() {
            // A topology change invalidates the in-flight reply. Wait for its
            // settlement so a burst does not start several provider reads.
            self.model_catalog.dirty = true;
        } else {
            self.request_model_catalog(w, cx);
        }
    }

    fn request_model_catalog(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if !self.connected()
            || self.service_starting
            || matches!(
                self.model_catalog.phase,
                Phase::Loading | Phase::Unsupported
            )
        {
            return;
        }
        self.model_catalog.phase = Phase::Loading;
        let generation = self.model_catalog.generation;
        cx.notify();
        cx.spawn_in(w, async move |this, cx| {
            let reply = bridge::invoke("model_catalog", json!({})).await;
            let _ = this.update_in(cx, |app, w, cx| {
                if app.model_catalog.generation != generation {
                    return;
                }
                if std::mem::take(&mut app.model_catalog.dirty) {
                    app.model_catalog.phase = Phase::Idle;
                    app.request_model_catalog(w, cx);
                    return;
                }
                match reply {
                    Ok(value) if value.is_null() || value["available"] == false => {
                        app.model_catalog.phase = Phase::Unsupported;
                        app.model_catalog.snapshot = None;
                        app.model_catalog.retry_providers.clear();
                    }
                    Ok(value) => match serde_json::from_value::<CatalogSnapshot>(value) {
                        Ok(snapshot) => {
                            let previous = &app.model_catalog.retry_providers;
                            app.model_catalog.retry_providers = snapshot
                                .failures
                                .iter()
                                .map(|failure| {
                                    (
                                        failure.id.clone(),
                                        previous
                                            .get(&failure.id)
                                            .cloned()
                                            .unwrap_or_else(|| cx.focus_handle()),
                                    )
                                })
                                .collect();
                            app.model_catalog.snapshot = Some(snapshot);
                            app.model_catalog.phase = Phase::Ready;
                        }
                        Err(_) => {
                            app.model_catalog.phase = Phase::Error("无法读取模型目录。".into())
                        }
                    },
                    Err(error) => app.model_catalog.phase = Phase::Error(error),
                }
                app.refresh_model_menu(w, cx);
                if app.composer_menu == Some(ComposerMenu::CommandModels) {
                    app.refresh_command_model_highlight(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn retry_model_catalog(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if !self.model_catalog.can_retry() || self.model_catalog.loading() {
            return;
        }
        if self.composer_menu == Some(ComposerMenu::CommandModels) {
            self.focus_command_picker(w, cx);
        } else if self.model_menu_searchable() && !self.model_menu_busy() {
            self.model_filter_editor
                .update(cx, |editor, cx| editor.focus(w, cx));
        } else {
            self.model_menu.trigger.focus(w, cx);
        }
        self.request_model_catalog(w, cx);
    }

    pub(super) fn handle_model_catalog_retry_key(
        &mut self,
        combo: &str,
        empty: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.model_catalog.can_retry() || self.model_catalog.loading() {
            return false;
        }
        let catalog = &self.model_catalog;
        if matches!(combo, "Enter" | "Tab")
            && (catalog.retry_error.is_focused(w)
                || catalog
                    .retry_providers
                    .values()
                    .any(|focus| focus.is_focused(w)))
        {
            self.retry_model_catalog(w, cx);
            return true;
        }
        if combo == "Tab" && empty {
            let focus = if matches!(catalog.phase, Phase::Error(_)) {
                Some(&catalog.retry_error)
            } else {
                catalog
                    .failures()
                    .first()
                    .and_then(|failure| catalog.retry_providers.get(&failure.id))
            };
            if let Some(focus) = focus {
                focus.focus(w, cx);
                return true;
            }
        }
        false
    }

    pub(super) fn render_model_catalog_notices(&self, cx: &mut Context<Self>) -> gpui::Div {
        let p = dsh(cx);
        let catalog = &self.model_catalog;
        let enabled = !catalog.loading();
        let retry = |id: String, focus: &gpui::FocusHandle| {
            dock_action_button(
                div()
                    .id(gpui::ElementId::Name(id.into()))
                    .role(gpui::Role::Button)
                    .aria_label("重试加载模型")
                    .track_focus(focus)
                    .tab_stop(enabled)
                    .px(px(3.))
                    .flex_none()
                    .rounded(px(4.))
                    .when(enabled, |button| {
                        button
                            .cursor_pointer()
                            .hover(|style| style.bg(p.hover))
                            .focus_visible(|style| style.bg(p.hover))
                    })
                    .when(!enabled, |button| button.opacity(0.5))
                    .child("重试"),
                cx.listener(|this, _, w, cx| this.retry_model_catalog(w, cx)),
            )
        };
        let notice = |id: String, text: String, color, background, focus: &gpui::FocusHandle| {
            row()
                .id(gpui::ElementId::Name(id.clone().into()))
                .role(gpui::Role::Status)
                .w_full()
                .items_start()
                .gap(px(6.))
                .px(px(7.))
                .py(px(5.))
                .rounded(px(6.))
                .bg(background)
                .text_size(px(11.))
                .line_height(px(16.))
                .text_color(color)
                .child(div().flex_1().min_w_0().child(text))
                .child(retry(format!("{id}-retry"), focus))
        };
        let mut list = column()
            .id("model-catalog-notices")
            .w_full()
            .max_h(px(108.))
            .overflow_y_scroll()
            .gap(px(3.));
        if catalog.loading() {
            list = list.child(
                div()
                    .id("model-catalog-loading")
                    .role(gpui::Role::Status)
                    .px(px(7.))
                    .text_size(px(12.))
                    .line_height(px(17.))
                    .text_color(p.faint)
                    .child("正在刷新模型列表…"),
            );
        }
        if let Some(message) = catalog.error_label() {
            list = list.child(notice(
                "model-catalog-error".into(),
                message,
                p.danger,
                p.danger.opacity(0.12),
                &catalog.retry_error,
            ));
        }
        for failure in catalog.failures() {
            if let Some(focus) = catalog.retry_providers.get(&failure.id) {
                list = list.child(notice(
                    format!("model-catalog-warning-{}", failure.id),
                    failure.label(),
                    p.state_warn_label,
                    p.state_warn_tertiary.opacity(0.28),
                    focus,
                ));
            }
        }
        column()
            .w_full()
            .flex_none()
            .when(catalog.loading() || catalog.can_retry(), |panel| {
                panel.px(px(4.)).py(px(3.)).child(list)
            })
    }
}
