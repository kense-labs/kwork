//! Custom provider configuration over the official Host adapter.

use super::provider_retry::{RetryDraft, RetryForm, RetryPolicy, RetryPolicySpec};
use super::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

mod provider_authorization;
use provider_authorization::{LoginDialog, LoginMethod};

#[wasm_bindgen(module = "/model-provider-form.js")]
extern "C" {
    fn update_model_provider_form(value: JsValue);
    fn place_model_provider_form(left: f32, top: f32, width: f32, height: f32);
    fn model_provider_logo(bytes: js_sys::Uint8Array) -> String;
    fn provider_endpoint_origin(value: &str, protocol: &str) -> Option<String>;
    pub(super) fn listen_model_provider_form(callback: &js_sys::Function);
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
enum Protocol {
    #[default]
    #[serde(rename = "openai-completions")]
    Chat,
    #[serde(rename = "openai-responses")]
    Responses,
    #[serde(rename = "anthropic-messages")]
    Anthropic,
    #[serde(rename = "google-generative-ai")]
    Gemini,
}

impl Protocol {
    fn wire(self) -> &'static str {
        match self {
            Self::Chat => "openai-completions",
            Self::Responses => "openai-responses",
            Self::Anthropic => "anthropic-messages",
            Self::Gemini => "google-generative-ai",
        }
    }

    fn levels(self) -> &'static [&'static str] {
        if self == Self::Gemini {
            &["minimal", "low", "medium", "high"]
        } else {
            REASONING_LEVELS
        }
    }
}

const LEGACY_PROTOCOLS: &[Protocol] = &[Protocol::Chat, Protocol::Responses];

struct Preset {
    name: &'static str,
    keywords: &'static str,
    url: &'static str,
    protocol: Protocol,
}

const PRESETS: &[Preset] = &[
    Preset {
        name: "硅基流动",
        keywords: "siliconflow",
        url: "https://api.siliconflow.cn/v1",
        protocol: Protocol::Chat,
    },
    Preset {
        name: "深度求索",
        keywords: "deepseek",
        url: "https://api.deepseek.com",
        protocol: Protocol::Chat,
    },
    Preset {
        name: "OpenRouter",
        keywords: "",
        url: "https://openrouter.ai/api/v1",
        protocol: Protocol::Chat,
    },
    Preset {
        name: "OpenAI",
        keywords: "",
        url: "https://api.openai.com/v1",
        protocol: Protocol::Responses,
    },
    Preset {
        name: "Anthropic",
        keywords: "Claude",
        url: "https://api.anthropic.com",
        protocol: Protocol::Anthropic,
    },
    Preset {
        name: "Gemini",
        keywords: "Google",
        url: "https://generativelanguage.googleapis.com/v1beta",
        protocol: Protocol::Gemini,
    },
    Preset {
        name: "Ollama",
        keywords: "本地",
        url: "http://localhost:11434/v1",
        protocol: Protocol::Chat,
    },
];

thread_local! {
    static PROVIDER_LOGOS: RefCell<HashMap<usize, String>> = RefCell::new(HashMap::new());
}

fn provider_logo(name: &str, base_url: &str) -> Option<String> {
    let png = crate::provider_marks::provider_logo(name, base_url)?;
    Some(PROVIDER_LOGOS.with(|logos| {
        logos
            .borrow_mut()
            .entry(png.as_ptr() as usize)
            .or_insert_with(|| model_provider_logo(js_sys::Uint8Array::from(png)))
            .clone()
    }))
}

const MAX_MODELS: usize = 512;
const SAFE_CAPACITY: u64 = 9_007_199_254_740_991;
const REASONING_LEVELS: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh", "max"];
const INITIAL_REASONING_LEVELS: &[&str] = &["off", "low", "medium", "high"];

type ReasoningLevel = (String, Option<String>);

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum ReasoningEfforts<T> {
    Flag(bool),
    Levels(Vec<T>),
}

#[derive(Clone, PartialEq, Eq, Serialize)]
struct ReasoningRow {
    key: u32,
    level: String,
    wire: Option<String>,
}

impl ReasoningRow {
    fn new(counter: &mut u32, level: String, wire: Option<String>) -> Self {
        let key = *counter;
        *counter = counter.wrapping_add(1);
        Self { key, level, wire }
    }

    fn default_level(counter: &mut u32, level: &str) -> Self {
        Self::new(counter, level.into(), default_reasoning_wire(level))
    }
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
enum ReasoningEdit {
    Toggle { enabled: bool },
    Add,
    Remove { key: u32 },
    Level { key: u32, level: String },
    Wire { key: u32, value: String },
}

fn default_reasoning_wire(level: &str) -> Option<String> {
    (level != "off").then(|| level.to_owned())
}

impl<T> ReasoningEfforts<T> {
    fn levels(&self) -> Option<&[T]> {
        match self {
            Self::Levels(levels) => Some(levels),
            Self::Flag(_) => None,
        }
    }
}

#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderModel {
    id: String,
    name: Option<String>,
    context_window: Option<u64>,
    max_tokens: Option<u64>,
    #[serde(alias = "input")]
    input_modalities: Option<Vec<String>>,
    reasoning_efforts: Option<ReasoningEfforts<ReasoningLevel>>,
}

struct ModelDraft {
    key: u32,
    source_id: Option<String>,
    id: String,
    name: String,
    context_window: String,
    max_tokens: String,
    input_modalities: Option<Vec<String>>,
    reasoning_efforts: Option<ReasoningEfforts<ReasoningRow>>,
    next_reasoning_key: u32,
}

impl ModelDraft {
    fn new(key: u32, model: ProviderModel, saved: bool) -> Self {
        let mut next_reasoning_key = 0;
        let reasoning_efforts = model.reasoning_efforts.map(|efforts| match efforts {
            ReasoningEfforts::Flag(flag) => ReasoningEfforts::Flag(flag),
            ReasoningEfforts::Levels(levels) => ReasoningEfforts::Levels(
                levels
                    .into_iter()
                    .map(|(level, wire)| ReasoningRow::new(&mut next_reasoning_key, level, wire))
                    .collect(),
            ),
        });
        Self {
            key,
            source_id: saved.then(|| model.id.clone()),
            id: model.id,
            name: model.name.unwrap_or_default(),
            context_window: format_capacity(model.context_window),
            max_tokens: format_capacity(model.max_tokens),
            input_modalities: model.input_modalities,
            reasoning_efforts,
            next_reasoning_key,
        }
    }

    fn wire(&self, include_reasoning: bool) -> Value {
        let optional_name = self.name.trim();
        let efforts = match &self.reasoning_efforts {
            None => Value::Null,
            Some(ReasoningEfforts::Flag(flag)) => Value::Bool(*flag),
            Some(ReasoningEfforts::Levels(levels)) => json!(
                levels
                    .iter()
                    .map(|row| (&row.level, &row.wire))
                    .collect::<Vec<_>>()
            ),
        };
        let mut model = json!({
            "sourceId": self.source_id,
            "id": self.id.trim(),
            "name": (!optional_name.is_empty()).then_some(optional_name),
            "contextWindow": parse_capacity(&self.context_window).ok().flatten(),
            "maxTokens": parse_capacity(&self.max_tokens).ok().flatten(),
            "inputModalities": self.input_modalities,
        });
        if include_reasoning {
            model["reasoningEfforts"] = efforts;
        }
        model
    }

    fn reasoning_issue(&self, protocol: Protocol) -> Option<&'static str> {
        let levels = self.reasoning_efforts.as_ref()?.levels()?;
        if protocol == Protocol::Gemini
            && levels.iter().any(|row| {
                !protocol.levels().contains(&row.level.as_str())
                    || row.wire.as_deref() != Some(row.level.as_str())
            })
        {
            Some("Gemini API 仅接受 minimal、low、medium 和 high 档位，参数值须与档位名相同。")
        } else if levels.iter().any(|row| match &row.wire {
            Some(wire) => wire.is_empty(),
            None => row.level != "off",
        }) {
            Some("思考档位的 API 参数值不能为空。")
        } else if !levels.iter().any(|row| row.level != "off") {
            Some("至少添加一个 Off 以外的档位，或关闭推理等级。")
        } else {
            None
        }
    }

    fn edit_reasoning(&mut self, edit: ReasoningEdit, protocol: Protocol) {
        let allowed = protocol.levels();
        if let ReasoningEdit::Toggle { enabled } = edit {
            self.reasoning_efforts = if enabled {
                Some(ReasoningEfforts::Levels(
                    INITIAL_REASONING_LEVELS
                        .iter()
                        .filter(|level| allowed.contains(level))
                        .map(|&level| {
                            ReasoningRow::default_level(&mut self.next_reasoning_key, level)
                        })
                        .collect(),
                ))
            } else {
                None
            };
            return;
        }
        let Some(ReasoningEfforts::Levels(levels)) = &mut self.reasoning_efforts else {
            return;
        };
        match edit {
            ReasoningEdit::Add => {
                let used = |candidate: &str| levels.iter().any(|row| row.level == candidate);
                let next = allowed
                    .iter()
                    .rposition(|level| used(level))
                    .map_or(0, |index| index + 1);
                if let Some(&level) = allowed[next..]
                    .iter()
                    .find(|level| !used(level))
                    .or_else(|| allowed.iter().find(|level| !used(level)))
                {
                    levels.push(ReasoningRow::default_level(
                        &mut self.next_reasoning_key,
                        level,
                    ));
                }
            }
            ReasoningEdit::Remove { key } => {
                levels.retain(|row| row.key != key);
            }
            ReasoningEdit::Level { key, level }
                if allowed.contains(&level.as_str())
                    && !levels.iter().any(|row| row.level == level) =>
            {
                if let Some(row) = levels.iter_mut().find(|row| row.key == key) {
                    let carry = protocol != Protocol::Gemini
                        && row.level != "off"
                        && level != "off"
                        && row
                            .wire
                            .as_ref()
                            .is_some_and(|value| !value.is_empty() && value != &row.level);
                    if !carry {
                        row.wire = default_reasoning_wire(&level);
                    }
                    row.level = level;
                }
            }
            ReasoningEdit::Wire { key, value } if protocol != Protocol::Gemini => {
                if let Some(row) = levels.iter_mut().find(|row| row.key == key)
                    && row.level != "off"
                {
                    let value = value.trim();
                    row.wire = Some(if value.is_empty() {
                        row.level.clone()
                    } else {
                        value.into()
                    });
                }
            }
            _ => {}
        }
    }
}

fn format_capacity(value: Option<u64>) -> String {
    match value {
        None => String::new(),
        Some(value) if value > 0 && value.is_multiple_of(1_000_000) => {
            format!("{}M", value / 1_000_000)
        }
        Some(value) if value > 0 && value.is_multiple_of(1000) => format!("{}K", value / 1000),
        Some(value) => value.to_string(),
    }
}

fn parse_capacity(text: &str) -> Result<Option<u64>, ()> {
    let text = text.trim();
    let (mantissa, scale) = match text.as_bytes().last() {
        None => return Ok(None),
        Some(b'k' | b'K') => (&text[..text.len() - 1], 1000.),
        Some(b'm' | b'M') => (&text[..text.len() - 1], 1_000_000.),
        Some(_) => (text, 1.),
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    let valid = mantissa.split_once('.').map_or_else(
        || digits(mantissa),
        |(whole, fraction)| digits(whole) && digits(fraction),
    );
    if !valid {
        return Err(());
    }
    let value = mantissa.parse::<f64>().map_err(|_| ())? * scale;
    let count = value.round();
    if count >= 1. && count <= SAFE_CAPACITY as f64 && (value - count).abs() < 1e-6 {
        Ok(Some(count as u64))
    } else {
        Err(())
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Provider {
    id: String,
    name: String,
    base_url: String,
    protocol: Option<Protocol>,
    catalog: Option<CatalogProvider>,
    models: Vec<ProviderModel>,
    default_input: Option<Vec<String>>,
    retry_policy: Option<RetryPolicy>,
    inherited_retry_policy: Option<RetryPolicy>,
    key_configured: bool,
    available: Option<bool>,
    editable: Option<bool>,
    removable: Option<bool>,
    can_clear_key: Option<bool>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogProvider {
    default_base_url: String,
    endpoint_env: Vec<String>,
    auth_env: Vec<String>,
    auth_detected: bool,
    oauth: bool,
    oauth_configured: bool,
    #[serde(default)]
    signed_in: bool,
    #[serde(default)]
    login_methods: Vec<LoginMethod>,
    #[serde(default)]
    login_running: bool,
    api_key_allowed: bool,
    diagnostic: Option<String>,
    inherited_models: Vec<ProviderModel>,
    models_overridden: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedProviders {
    id: String,
    providers: Vec<Provider>,
    revision: Option<u64>,
    #[serde(default)]
    catalog_providers: Vec<Provider>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ProviderCatalog {
    Detailed {
        providers: Vec<Provider>,
        #[serde(default, rename = "catalogProviders")]
        catalog_providers: Vec<Provider>,
        revision: Option<u64>,
        protocols: Option<Vec<Protocol>>,
        #[serde(rename = "retryPolicySpec")]
        retry_policy_spec: Option<Box<RetryPolicySpec>>,
    },
    Legacy(Vec<Provider>),
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum CatalogModelsChange {
    #[default]
    Keep,
    Override {
        from_default: bool,
    },
    Reset,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Operation {
    List,
    Discover,
    Save,
    Remove,
}

#[derive(Default)]
pub(super) struct ProviderSettings {
    provider_search: String,
    name: String,
    url: String,
    key: String,
    providers: Vec<Provider>,
    catalog_providers: Vec<Provider>,
    revision: Option<u64>,
    draft_revision: Option<u64>,
    selected: Option<String>,
    preset: Option<usize>,
    protocol: Protocol,
    protocols: Option<Vec<Protocol>>,
    models: Vec<ModelDraft>,
    catalog_models: CatalogModelsChange,
    model_key: u32,
    default_input: Option<Vec<String>>,
    retry_spec: Option<RetryPolicySpec>,
    retry: RetryDraft,
    retry_dirty: bool,
    picker: Option<ModelPicker>,
    discovery_generation: u32,
    key_configured: bool,
    clear_key: bool,
    key_revision: u32,
    confirm_remove: bool,
    loaded: bool,
    dirty: bool,
    busy: Option<Operation>,
    feedback: Option<String>,
    login: Option<LoginDialog>,
    login_generation: u32,
    view: Option<ProviderForm>,
}

#[derive(PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderChoice {
    id: String,
    name: String,
    logo: Option<String>,
    selected: bool,
}

#[derive(PartialEq, Eq, Serialize)]
struct ModelChoice {
    id: String,
    name: String,
    selected: bool,
}

struct ModelPicker {
    candidates: Vec<ProviderModel>,
    positions: HashMap<String, usize>,
    selected: HashSet<String>,
    query: String,
    visible: Vec<usize>,
}

impl ModelPicker {
    fn new(found: Vec<ProviderModel>, configured: &[ModelDraft]) -> Self {
        let mut positions = HashMap::new();
        let mut candidates = Vec::new();
        for model in found {
            if let Some(&position) = positions.get(&model.id) {
                candidates[position] = model;
            } else {
                positions.insert(model.id.clone(), candidates.len());
                candidates.push(model);
            }
        }
        let configured_ids: HashSet<_> = configured
            .iter()
            .map(|model| model.id.trim())
            .filter(|id| !id.is_empty())
            .collect();
        let selected = candidates
            .iter()
            .filter(|model| configured_ids.is_empty() || configured_ids.contains(model.id.as_str()))
            .map(|model| model.id.clone())
            .collect();
        let visible = (0..candidates.len()).collect();
        Self {
            candidates,
            positions,
            selected,
            query: String::new(),
            visible,
        }
    }

    fn search(&mut self, query: String) {
        if self.query == query {
            return;
        }
        self.visible = if query.trim().is_empty() {
            (0..self.candidates.len()).collect()
        } else {
            let keys: Vec<_> = self
                .candidates
                .iter()
                .map(|model| (model.id.as_str(), model.name.as_deref(), None::<&str>))
                .collect();
            let keys =
                serde_wasm_bindgen::to_value(&keys).expect("serialize candidate search keys");
            super::model_menu::rank_model_choices(keys, &query)
                .to_vec()
                .into_iter()
                .map(|index| index as usize)
                .collect()
        };
        self.query = query;
    }

    fn keep(&self, model: &ModelDraft) -> bool {
        let id = model.id.trim();
        !self.positions.contains_key(id) || self.selected.contains(id)
    }

    fn issue(&self, configured: &[ModelDraft]) -> Option<&'static str> {
        let mut size = self.selected.len();
        let mut seen = HashSet::new();
        for model in configured.iter().filter(|model| self.keep(model)) {
            let id = model.id.trim();
            if !self.selected.contains(id) || !seen.insert(id) {
                size += 1;
            }
        }
        if !configured.is_empty() && size == 0 {
            Some("请至少保留一个模型。")
        } else if size > MAX_MODELS {
            Some("最多可添加 512 个模型。")
        } else {
            None
        }
    }

    fn all_visible_selected(&self) -> bool {
        !self.visible.is_empty()
            && self
                .visible
                .iter()
                .all(|&index| self.selected.contains(&self.candidates[index].id))
    }

    fn toggle_visible(&mut self) {
        let clear = self.all_visible_selected();
        for &index in &self.visible {
            let id = &self.candidates[index].id;
            if clear {
                self.selected.remove(id);
            } else {
                self.selected.insert(id.clone());
            }
        }
    }

    fn form(&self, configured: &[ModelDraft]) -> ModelPickerForm {
        ModelPickerForm {
            query: self.query.clone(),
            issue: self.issue(configured),
            all_visible_selected: self.all_visible_selected(),
            candidates: self
                .visible
                .iter()
                .map(|&index| {
                    let model = &self.candidates[index];
                    ModelChoice {
                        id: model.id.clone(),
                        name: model.name.clone().unwrap_or_else(|| model.id.clone()),
                        selected: self.selected.contains(&model.id),
                    }
                })
                .collect(),
        }
    }
}

#[derive(PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelPickerForm {
    query: String,
    issue: Option<&'static str>,
    all_visible_selected: bool,
    candidates: Vec<ModelChoice>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelIssue {
    field: &'static str,
    message: &'static str,
}

#[derive(PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelForm {
    key: u32,
    id: String,
    name: String,
    context_window: String,
    max_tokens: String,
    text: bool,
    image: bool,
    reasoning: Option<Vec<ReasoningRow>>,
    issue: Option<ModelIssue>,
}

#[derive(PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderForm {
    dark: bool,
    provider_search: String,
    name: String,
    url: String,
    protocol: Protocol,
    protocols: Vec<Protocol>,
    missing_api_key: bool,
    key_revision: u32,
    key_configured: bool,
    clear_key: bool,
    saved: Vec<ProviderChoice>,
    presets: Vec<ProviderChoice>,
    directory: Vec<ProviderChoice>,
    catalog: Option<CatalogForm>,
    custom: bool,
    read_only: bool,
    removable: bool,
    can_clear_key: bool,
    refreshable: bool,
    models: Vec<ModelForm>,
    reasoning_levels: &'static [&'static str],
    retry: Option<RetryForm>,
    can_add_model: bool,
    picker: Option<ModelPickerForm>,
    invalid_url: bool,
    can_discover: bool,
    can_save: bool,
    loaded: bool,
    busy: Option<Operation>,
    confirm_remove: bool,
    feedback: Option<String>,
    login: Option<LoginDialog>,
}

#[derive(PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogForm {
    default_base_url: String,
    endpoint_env: Vec<String>,
    auth_env: Vec<String>,
    auth_detected: bool,
    oauth: bool,
    oauth_configured: bool,
    signed_in: bool,
    login_available: bool,
    login_running: bool,
    api_key_allowed: bool,
    diagnostic: Option<String>,
    can_reset_models: bool,
}

impl ProviderSettings {
    fn install_reply(&mut self, catalog: ProviderCatalog) {
        let (providers, directory) = match catalog {
            ProviderCatalog::Detailed {
                providers,
                catalog_providers,
                revision,
                protocols,
                retry_policy_spec,
            } => {
                self.retry_spec = retry_policy_spec.map(|spec| *spec);
                self.protocols = protocols;
                self.revision = revision;
                (providers, catalog_providers)
            }
            ProviderCatalog::Legacy(providers) => {
                self.revision = None;
                (providers, Vec::new())
            }
        };
        self.install_catalog(providers, directory);
    }

    fn install_catalog(&mut self, providers: Vec<Provider>, directory: Vec<Provider>) {
        self.providers = providers;
        self.catalog_providers = directory;
        self.loaded = true;
        if self.dirty {
            self.feedback = None;
        } else {
            let selected = self
                .selected
                .clone()
                .filter(|id| self.provider(id).is_some())
                .or_else(|| self.providers.first().map(|provider| provider.id.clone()));
            self.load(selected);
        }
    }

    fn reset_key(&mut self) {
        self.key.clear();
        self.key_revision = self.key_revision.wrapping_add(1);
    }

    fn load(&mut self, id: Option<String>) {
        let saved = id.as_deref().and_then(|id| {
            self.providers
                .iter()
                .chain(&self.catalog_providers)
                .find(|provider| provider.id == id)
        });
        self.name = saved.map_or_else(String::new, |provider| provider.name.clone());
        self.url = saved.map_or_else(String::new, |provider| provider.base_url.clone());
        self.protocol = saved
            .and_then(|provider| provider.protocol)
            .unwrap_or_default();
        let models = saved.map_or_else(Vec::new, |provider| provider.models.clone());
        self.default_input = saved.and_then(|provider| provider.default_input.clone());
        self.retry = RetryDraft::load(
            saved.and_then(|provider| provider.retry_policy.clone()),
            saved.and_then(|provider| provider.inherited_retry_policy.clone()),
        );
        self.retry_dirty = false;
        self.key_configured = saved.is_some_and(|provider| provider.key_configured);
        self.models.clear();
        for model in models {
            self.append_model(model, true);
        }
        self.catalog_models = CatalogModelsChange::Keep;
        self.selected = id;
        self.draft_revision = self.revision;
        self.reset_key();
        self.preset = None;
        self.picker = None;
        self.feedback = None;
        self.clear_key = false;
        self.confirm_remove = false;
        self.dirty = false;
    }

    fn load_preset(&mut self, index: usize) {
        let Some(preset) = PRESETS.get(index) else {
            return;
        };
        if !self.supported_protocols().contains(&preset.protocol) {
            return;
        }
        self.load(None);
        self.name = preset.name.to_owned();
        self.url = preset.url.to_owned();
        self.protocol = preset.protocol;
        self.preset = Some(index);
        self.dirty = true;
    }

    fn append_model(&mut self, model: ProviderModel, saved: bool) {
        self.model_key = self.model_key.wrapping_add(1);
        self.models
            .push(ModelDraft::new(self.model_key, model, saved));
    }

    fn apply_picker(&mut self) {
        let Some(picker) = &self.picker else { return };
        if picker.issue(&self.models).is_some() {
            return;
        }
        let picker = self.picker.take().expect("validated picker");
        let before = self.models.len();
        self.models.retain(|model| picker.keep(model));
        let retained = self.models.len();
        let ids: HashSet<_> = self
            .models
            .iter()
            .map(|model| model.id.trim().to_owned())
            .collect();
        for model in picker.candidates {
            if picker.selected.contains(&model.id) && !ids.contains(&model.id) {
                self.append_model(model, false);
            }
        }
        if retained != before || self.models.len() != retained {
            self.models_changed();
        }
    }

    fn models_changed(&mut self) {
        self.catalog_models = CatalogModelsChange::Override {
            from_default: matches!(
                self.catalog_models,
                CatalogModelsChange::Reset | CatalogModelsChange::Override { from_default: true }
            ),
        };
        self.feedback = None;
        self.dirty = true;
    }

    fn reset_catalog_models(&mut self) {
        let Some(catalog) = self.catalog_provider() else {
            return;
        };
        let inherited = catalog.inherited_models.clone();
        self.models.clear();
        for model in inherited {
            self.append_model(model, true);
        }
        self.catalog_models = CatalogModelsChange::Reset;
        self.feedback = None;
        self.dirty = true;
    }

    fn edit_model(&mut self, key: u32, field: &str, value: String) {
        let Some(model) = self.models.iter_mut().find(|model| model.key == key) else {
            return;
        };
        match field {
            "id" => model.id = value,
            "name" => model.name = value,
            "contextWindow" => model.context_window = value,
            "maxTokens" => model.max_tokens = value,
            "text" | "image" => {
                let mut input = model
                    .input_modalities
                    .as_deref()
                    .filter(|input| !input.is_empty())
                    .or(self.default_input.as_deref())
                    .filter(|input| !input.is_empty())
                    .map_or_else(|| vec!["text".into()], <[String]>::to_vec);
                if value == "true" {
                    if !input.iter().any(|kind| kind == field) {
                        input.push(field.into());
                    }
                } else {
                    input.retain(|kind| kind != field);
                }
                if input.is_empty() {
                    return;
                }
                model.input_modalities = Some(input);
            }
            _ => return,
        }
        self.models_changed();
    }

    fn move_model(&mut self, key: u32, target: u32, after: bool) {
        let Some(from) = self.models.iter().position(|model| model.key == key) else {
            return;
        };
        let Some(to) = self.models.iter().position(|model| model.key == target) else {
            return;
        };
        if from != to {
            let insertion = to + usize::from(after) - usize::from(from < to);
            if from != insertion {
                let model = self.models.remove(from);
                self.models.insert(insertion, model);
                self.models_changed();
            }
        }
    }

    fn model_issues(&self) -> impl Iterator<Item = Option<ModelIssue>> + '_ {
        let mut seen = HashSet::new();
        let catalog = self.catalog_provider().is_some();
        self.models.iter().map(move |model| {
            let id = model.id.trim();
            let (field, message) = if id.is_empty() {
                ("id", "请输入模型 ID。")
            } else if id.encode_utf16().count() > 512
                || id
                    .chars()
                    .any(|ch| ch.is_whitespace() || ch.is_ascii_control())
            {
                ("id", "模型 ID 最长 512 个字符，且不能包含空格。")
            } else if !seen.insert(id) {
                ("id", "模型 ID 必须唯一。")
            } else if model.name.trim().encode_utf16().count() > 512 {
                ("name", "模型名称最长 512 个字符。")
            } else if parse_capacity(&model.context_window).is_err() {
                ("contextWindow", "请输入有效的上下文窗口，例如 128K 或 1M。")
            } else if parse_capacity(&model.max_tokens).is_err() {
                ("maxTokens", "请输入有效的最大输出令牌数，例如 32K。")
            } else if !catalog {
                ("reasoning", model.reasoning_issue(self.protocol)?)
            } else {
                return None;
            };
            Some(ModelIssue { field, message })
        })
    }

    fn set_field(&mut self, field: &str, value: &str) {
        if field.starts_with("retry-") {
            if self.retry_spec.is_some()
                && self.retry.set_field(field, value)
                && field != "retry-custom-code"
            {
                self.retry_changed();
            }
            return;
        }
        let current = match field {
            "provider-search" => &mut self.provider_search,
            "name" => &mut self.name,
            "url" => &mut self.url,
            "key" => &mut self.key,
            _ => return,
        };
        if current == value {
            return;
        }
        *current = value.to_owned();
        if field == "key" && !value.is_empty() {
            self.clear_key = false;
        }
        if field != "provider-search" {
            self.feedback = None;
            self.dirty = true;
        }
    }

    fn valid_url(&self) -> bool {
        crate::validation::valid_provider_url(self.url.trim())
            && provider_endpoint_origin(self.url.trim(), self.protocol.wire()).is_some()
    }

    fn supported_protocols(&self) -> &[Protocol] {
        self.protocols.as_deref().unwrap_or(LEGACY_PROTOCOLS)
    }

    fn provider(&self, id: &str) -> Option<&Provider> {
        self.providers
            .iter()
            .chain(&self.catalog_providers)
            .find(|provider| provider.id == id)
    }

    fn selected_provider(&self) -> Option<&Provider> {
        self.provider(self.selected.as_deref()?)
    }

    fn catalog_provider(&self) -> Option<&CatalogProvider> {
        self.selected_provider()?.catalog.as_ref()
    }

    fn can_reset_catalog_models(&self) -> bool {
        self.catalog_provider()
            .is_some_and(|catalog| match self.catalog_models {
                CatalogModelsChange::Keep => catalog.models_overridden,
                CatalogModelsChange::Override { .. } => true,
                CatalogModelsChange::Reset => false,
            })
    }

    fn stale_draft(&self) -> bool {
        self.selected.is_some()
            && self.draft_revision.is_some()
            && self.revision != self.draft_revision
    }

    fn can_remove(&self) -> bool {
        !self.stale_draft()
            && self
                .selected_provider()
                .is_some_and(|provider| provider.removable.unwrap_or(true))
    }

    fn read_only(&self) -> bool {
        self.selected_provider()
            .is_some_and(|provider| provider.editable == Some(false))
    }

    fn can_clear_key(&self) -> bool {
        self.key_configured
            && self
                .selected_provider()
                .is_some_and(|provider| provider.can_clear_key.unwrap_or(true))
    }

    fn missing_api_key(&self) -> bool {
        if self.protocol != Protocol::Gemini || !self.key.trim().is_empty() {
            return false;
        }
        let saved = self.selected_provider();
        !(!self.clear_key
            && saved.is_some_and(|provider| {
                provider.key_configured
                    && provider_endpoint_origin(self.url.trim(), self.protocol.wire()).is_some_and(
                        |origin| {
                            provider_endpoint_origin(
                                &provider.base_url,
                                provider.protocol.unwrap_or_default().wire(),
                            )
                            .as_deref()
                                == Some(origin.as_str())
                        },
                    )
            }))
    }

    fn can_discover(&self) -> bool {
        !self.read_only()
            && !self.stale_draft()
            && !self.clear_key
            && (self.catalog_provider().is_some()
                || self.valid_url()
                    && !self.missing_api_key()
                    && self.supported_protocols().contains(&self.protocol))
    }

    fn can_save(&self) -> bool {
        let catalog = self.catalog_provider().is_some();
        let valid_route = if catalog {
            self.url.trim().is_empty() || self.valid_url()
        } else {
            self.valid_url()
                && !self.missing_api_key()
                && self.supported_protocols().contains(&self.protocol)
                && !self.name.trim().is_empty()
                && !self.models.is_empty()
        };
        valid_route
            && !self.read_only()
            && !self.stale_draft()
            && self.models.len() <= MAX_MODELS
            && self.model_issues().all(|issue| issue.is_none())
            && self
                .retry_spec
                .as_ref()
                .is_none_or(|spec| self.retry.policy(spec).is_ok())
    }

    fn draft(&self) -> Value {
        let catalog = self.catalog_provider().is_some();
        let mut draft = json!({
            "baseUrl": self.url.trim(),
            "clearKey": self.clear_key,
        });
        let models = || {
            self.models
                .iter()
                .map(|model| model.wire(!catalog))
                .collect::<Vec<_>>()
        };
        if catalog {
            draft["catalog"] = true.into();
            match self.catalog_models {
                CatalogModelsChange::Keep => {}
                CatalogModelsChange::Reset => draft["models"] = Value::Null,
                CatalogModelsChange::Override { from_default } => {
                    draft["models"] = json!(models());
                    if from_default {
                        draft["modelsFromDefault"] = true.into();
                    }
                }
            }
        } else {
            draft["name"] = self.name.trim().into();
            draft["protocol"] = json!(self.protocol);
            draft["models"] = json!(models());
        }
        if let Some(id) = &self.selected {
            draft["id"] = id.as_str().into();
        }
        if !self.key.trim().is_empty() {
            draft["apiKey"] = self.key.trim().into();
        }
        draft
    }

    fn retry_changed(&mut self) {
        self.retry_dirty = true;
        self.feedback = None;
        self.dirty = true;
    }

    fn form(&self, dark: bool) -> ProviderForm {
        let provider_query = self.provider_search.trim().to_lowercase();
        let choices = |providers: &[Provider]| {
            providers
                .iter()
                .filter(|provider| {
                    matches_query(
                        &provider_query,
                        &[
                            &provider.name,
                            &provider.base_url,
                            &provider.id,
                            provider
                                .catalog
                                .as_ref()
                                .map_or("", |catalog| catalog.default_base_url.as_str()),
                        ],
                    )
                })
                .map(|provider| ProviderChoice {
                    id: provider.id.clone(),
                    name: provider.name.clone(),
                    logo: provider_logo(&provider.name, &provider.base_url),
                    selected: self.selected.as_ref() == Some(&provider.id),
                })
                .collect::<Vec<_>>()
        };
        let catalog = self.catalog_provider().map(|catalog| CatalogForm {
            default_base_url: catalog.default_base_url.clone(),
            endpoint_env: catalog.endpoint_env.clone(),
            auth_env: catalog.auth_env.clone(),
            auth_detected: catalog.auth_detected,
            oauth: catalog.oauth,
            oauth_configured: catalog.oauth_configured,
            signed_in: catalog.signed_in,
            login_available: !catalog.login_methods.is_empty(),
            login_running: catalog.login_running,
            api_key_allowed: catalog.api_key_allowed,
            diagnostic: catalog.diagnostic.clone(),
            can_reset_models: self.can_reset_catalog_models(),
        });
        let presets = PRESETS
            .iter()
            .enumerate()
            .filter(|(_, preset)| self.supported_protocols().contains(&preset.protocol))
            .filter(|(_, preset)| {
                matches_query(&provider_query, &[preset.name, preset.url, preset.keywords])
            })
            .map(|(index, preset)| ProviderChoice {
                id: index.to_string(),
                name: preset.name.to_owned(),
                logo: provider_logo(preset.name, preset.url),
                selected: self.preset == Some(index),
            })
            .collect();
        ProviderForm {
            dark,
            provider_search: self.provider_search.clone(),
            name: self.name.clone(),
            url: self.url.clone(),
            protocol: self.protocol,
            protocols: self.supported_protocols().to_vec(),
            missing_api_key: self.missing_api_key(),
            key_revision: self.key_revision,
            key_configured: self.key_configured,
            clear_key: self.clear_key,
            saved: choices(&self.providers),
            presets,
            directory: choices(&self.catalog_providers),
            catalog,
            custom: self.selected.is_none() && self.preset.is_none(),
            read_only: self.read_only(),
            removable: self.can_remove(),
            can_clear_key: self.can_clear_key(),
            refreshable: self.revision.is_some(),
            models: self
                .models
                .iter()
                .zip(self.model_issues())
                .map(|(model, issue)| {
                    let input = model
                        .input_modalities
                        .as_deref()
                        .filter(|input| !input.is_empty())
                        .or(self.default_input.as_deref())
                        .filter(|input| !input.is_empty());
                    ModelForm {
                        key: model.key,
                        id: model.id.clone(),
                        name: model.name.clone(),
                        context_window: model.context_window.clone(),
                        max_tokens: model.max_tokens.clone(),
                        text: input.is_none_or(|input| input.iter().any(|kind| kind == "text")),
                        image: input.is_some_and(|input| input.iter().any(|kind| kind == "image")),
                        reasoning: model
                            .reasoning_efforts
                            .as_ref()
                            .and_then(ReasoningEfforts::levels)
                            .map(<[ReasoningRow]>::to_vec),
                        issue,
                    }
                })
                .collect(),
            reasoning_levels: self.protocol.levels(),
            retry: self.retry_spec.as_ref().map(|spec| self.retry.form(spec)),
            can_add_model: !self.read_only() && self.models.len() < MAX_MODELS,
            picker: self.picker.as_ref().map(|picker| picker.form(&self.models)),
            invalid_url: !self.url.trim().is_empty() && !self.valid_url(),
            can_discover: self.can_discover(),
            can_save: self.can_save(),
            loaded: self.loaded,
            busy: self.busy,
            confirm_remove: self.confirm_remove,
            feedback: self.feedback.clone().or_else(|| {
                self.stale_draft()
                    .then(|| "模型服务配置已更新。请重新选择服务商后再编辑。".into())
            }),
            login: self.login.clone(),
        }
    }
}

fn matches_query(query: &str, values: &[&str]) -> bool {
    query.is_empty()
        || values
            .iter()
            .any(|value| value.to_lowercase().contains(query))
}

impl DshApp {
    pub(super) fn refresh_visible_model_providers(
        &mut self,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings = &self.model_providers;
        if self.show_settings
            && !self.show_welcome
            && self.settings_page == SettingsPage::Providers
            && settings.revision.is_some()
            && settings.busy.is_none()
            && !settings.dirty
            && settings.login.is_none()
            && !settings.confirm_remove
            && settings.picker.is_none()
            && settings.feedback.is_none()
        {
            self.request_model_providers(Operation::List, w, cx);
        }
    }

    pub(super) fn load_startup_model_providers(
        &mut self,
        reply: &Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(value) = reply.get("modelProviders")
            && let Ok(catalog) = serde_json::from_value::<ProviderCatalog>(value.clone())
        {
            self.model_providers.install_reply(catalog);
            let available = self.model_providers.providers.iter().any(|provider| {
                provider.available.unwrap_or(true)
                    && (provider.catalog.is_some() || !provider.models.is_empty())
            });
            if available {
                self.welcome
                    .update(cx, |welcome, cx| welcome.use_saved_models(w, cx));
            }
        } else if self.show_settings && self.settings_page == SettingsPage::Providers {
            self.open_model_providers(w, cx);
        }
    }

    pub(super) fn open_model_providers(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        self.settings_page = SettingsPage::Providers;
        if !self.model_providers.loaded || self.model_providers.revision.is_some() {
            if self.connected() {
                self.request_model_providers(Operation::List, w, cx);
            } else {
                self.model_providers.feedback = Some(
                    self.error
                        .clone()
                        .unwrap_or_else(|| "正在连接模型服务…".into()),
                );
            }
        }
        cx.notify();
    }

    fn request_model_providers(
        &mut self,
        action: Operation,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let connected = self.connected();
        let blank_session = if action == Operation::Save
            && self.conversation.acp_open
            && !self.conversation.running
            && self.conversation.entries.is_empty()
        {
            self.active_session.clone()
        } else {
            None
        };
        let settings = &mut self.model_providers;
        if settings.busy.is_some() {
            return;
        }
        if !connected {
            settings.feedback = Some(
                self.error
                    .clone()
                    .unwrap_or_else(|| "正在连接模型服务…".into()),
            );
            cx.notify();
            return;
        }
        let mut draft = match action {
            Operation::List => json!({"includeRetrySettings": true}),
            Operation::Remove => {
                let Some(id) = &settings.selected else { return };
                if !settings.confirm_remove || !settings.can_remove() {
                    return;
                }
                if settings.catalog_provider().is_some() {
                    json!({ "id": id, "catalog": true })
                } else {
                    json!({ "id": id })
                }
            }
            Operation::Save if settings.can_save() => {
                let mut draft = settings.draft();
                if settings.retry_dirty
                    && let Some(spec) = &settings.retry_spec
                    && let Ok(policy) = settings.retry.policy(spec)
                {
                    draft["retryPolicy"] = json!(policy);
                }
                draft
            }
            Operation::Discover if settings.can_discover() => {
                if settings.catalog_provider().is_some() {
                    json!({ "id": settings.selected, "catalog": true })
                } else {
                    settings.draft()
                }
            }
            _ => return,
        };
        if matches!(action, Operation::Save | Operation::Remove)
            && settings.selected.is_some()
            && let Some(revision) = settings.draft_revision
        {
            draft["expectedRevision"] = revision.into();
        }
        if let Some(session) = blank_session {
            draft["sessionId"] = session.into();
        }
        let discovery = (action == Operation::Discover).then(|| {
            settings.discovery_generation = settings.discovery_generation.wrapping_add(1);
            settings.discovery_generation
        });
        settings.busy = Some(action);
        settings.feedback = None;
        cx.notify();

        cx.spawn_in(w, async move |this, cx| {
            // Null model fields explicitly remove saved overrides.
            let result =
                bridge::invoke_json("model_providers", json!({"action":action,"draft":draft}))
                    .await;
            let _ = this.update_in(cx, |app, w, cx| {
                let provider_visible = app.show_settings
                    && !app.show_welcome
                    && app.settings_page == SettingsPage::Providers;
                let settings = &mut app.model_providers;
                if discovery.is_some_and(|generation| generation != settings.discovery_generation) {
                    return;
                }
                settings.busy = None;
                if discovery.is_some() && !provider_visible {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(value) => {
                        if settings.apply_reply(action, value).is_err() {
                            settings.feedback = Some("模型服务返回的数据无法读取。".into());
                        } else if action == Operation::Save && app.active_session.is_none() {
                            app.new_session(w, cx);
                        }
                    }
                    Err(error) => settings.feedback = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_model_providers(&self) -> gpui::Div {
        div().size_full().relative().child(
            gpui::canvas(
                |bounds, _, _| {
                    place_model_provider_form(
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

    pub(super) fn publish_model_provider_form(&mut self, cx: &App) {
        let visible = self.show_settings
            && !self.show_welcome
            && self.settings_page == SettingsPage::Providers;
        let next = visible.then(|| self.model_providers.form(workspace::dark(cx)));
        if next != self.model_providers.view
            && let Ok(value) = serde_wasm_bindgen::to_value(&next)
        {
            self.model_providers.view = next;
            update_model_provider_form(value);
        }
    }

    pub(super) fn provider_navigation_locked(&self) -> bool {
        self.settings_page == SettingsPage::Providers
            && (self.model_providers.confirm_remove
                || self.model_providers.login.is_some()
                || self.model_providers.picker.is_some()
                || self.model_providers.busy == Some(Operation::Discover))
    }

    pub(super) fn cancel_provider_modal(&mut self, cx: &mut Context<Self>) -> bool {
        if self.settings_page != SettingsPage::Providers {
            return false;
        }
        if self.model_providers.login.is_some() {
            self.close_provider_login(cx);
            return true;
        }
        let settings = &mut self.model_providers;
        if settings.picker.take().is_some() {
            cx.notify();
            return true;
        }
        if !settings.confirm_remove {
            if settings.busy == Some(Operation::Discover) {
                settings.busy = None;
                settings.discovery_generation = settings.discovery_generation.wrapping_add(1);
            }
            return false;
        }
        if settings.busy.is_none() {
            settings.confirm_remove = false;
            settings.feedback = None;
            cx.notify();
        }
        true
    }

    pub(super) fn handle_model_provider_form(
        &mut self,
        action: &str,
        value: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.show_welcome || !self.show_settings || self.settings_page != SettingsPage::Providers
        {
            return;
        }
        if let Some(action) = action.strip_prefix("provider-auth-") {
            self.handle_provider_login(action, value, cx);
            return;
        }
        if self.model_providers.login.is_some() && action != "provider-close" {
            return;
        }
        if action == "provider-close" {
            self.close_settings(cx);
            if !self.show_settings
                && self.page == WorkspacePage::Conversation
                && self.active_session.is_some()
            {
                self.focus_composer(w, cx);
            }
            return;
        }
        let settings = &mut self.model_providers;
        if settings.busy.is_some() {
            return;
        }
        if let Some(picker) = &mut settings.picker {
            match action {
                "provider-picker-cancel" => settings.picker = None,
                "provider-picker-search" => picker.search(value.into()),
                "provider-picker-bulk" => picker.toggle_visible(),
                "provider-picker-toggle" if picker.positions.contains_key(value) => {
                    if !picker.selected.remove(value) {
                        picker.selected.insert(value.into());
                    }
                }
                "provider-picker-apply" => settings.apply_picker(),
                _ => return,
            }
            cx.notify();
            return;
        }
        if settings.confirm_remove
            && !matches!(action, "provider-remove" | "provider-cancel-remove")
        {
            return;
        }
        match action {
            "provider-reload" => self.request_model_providers(Operation::List, w, cx),
            "provider-discover" => self.request_model_providers(Operation::Discover, w, cx),
            "provider-save" => self.request_model_providers(Operation::Save, w, cx),
            "provider-remove" => self.request_model_providers(Operation::Remove, w, cx),
            "provider-cancel-remove" => {
                self.cancel_provider_modal(cx);
            }
            "provider-ask-remove" if settings.can_remove() => {
                settings.feedback = None;
                settings.confirm_remove = true;
            }
            "provider-select" if settings.provider(value).is_some() => {
                settings.load(Some(value.to_owned()));
            }
            "provider-new" => settings.load(None),
            "provider-preset" => {
                if let Ok(index) = value.parse() {
                    settings.load_preset(index);
                }
            }
            "provider-protocol" if settings.catalog_provider().is_none() => {
                if let Ok(protocol) =
                    serde_json::from_value::<Protocol>(Value::String(value.to_owned()))
                    && settings.protocol != protocol
                    && settings.supported_protocols().contains(&protocol)
                {
                    settings.protocol = protocol;
                    settings.feedback = None;
                    settings.dirty = true;
                }
            }
            "provider-field" => {
                if let Ok([field, value]) = serde_json::from_str::<[String; 2]>(value) {
                    settings.set_field(&field, &value);
                }
            }
            "provider-retry-mode" if settings.retry_spec.is_some() => {
                if settings.retry.set_mode(value) {
                    settings.retry_changed();
                }
            }
            "provider-retry-code" => {
                if let Some(spec) = &settings.retry_spec
                    && let Ok((code, selected)) = serde_json::from_str::<(String, bool)>(value)
                    && settings.retry.edit_code(spec, &code, selected)
                {
                    settings.retry_changed();
                }
            }
            "provider-retry-add-code" => {
                if let Some(spec) = &settings.retry_spec
                    && settings.retry.add_code(spec)
                {
                    settings.retry_changed();
                }
            }
            "provider-clear-key" if settings.can_clear_key() => {
                settings.clear_key = !settings.clear_key;
                settings.feedback = None;
                settings.dirty = true;
                if settings.clear_key {
                    settings.reset_key();
                }
            }
            "provider-add-model" if settings.models.len() < MAX_MODELS => {
                settings.append_model(ProviderModel::default(), false);
                settings.models_changed();
            }
            "provider-reset-models" if settings.can_reset_catalog_models() => {
                settings.reset_catalog_models();
            }
            "provider-model-field" => {
                if let Ok((key, field, value)) =
                    serde_json::from_str::<(u32, String, String)>(value)
                {
                    settings.edit_model(key, &field, value);
                }
            }
            "provider-model-reasoning" if settings.catalog_provider().is_none() => {
                if let Ok((key, edit)) = serde_json::from_str::<(u32, ReasoningEdit)>(value)
                    && let Some(model) = settings.models.iter_mut().find(|model| model.key == key)
                {
                    model.edit_reasoning(edit, settings.protocol);
                    settings.feedback = None;
                    settings.dirty = true;
                }
            }
            "provider-model-order" => {
                if let Ok((key, target, after)) = serde_json::from_str::<(u32, u32, bool)>(value) {
                    settings.move_model(key, target, after);
                }
            }
            "provider-remove-model" => {
                if let Ok(key) = value.parse::<u32>() {
                    settings.models.retain(|model| model.key != key);
                    settings.models_changed();
                }
            }
            _ => {}
        }
        cx.notify();
    }
}

impl ProviderSettings {
    fn apply_reply(&mut self, action: Operation, value: Value) -> Result<(), serde_json::Error> {
        match action {
            Operation::List => self.install_reply(serde_json::from_value(value)?),
            Operation::Discover => {
                let found: Vec<ProviderModel> = serde_json::from_value(value)?;
                if found.is_empty() {
                    self.feedback = Some("该服务商没有列出任何模型，请手动添加。".into());
                } else {
                    self.picker = Some(ModelPicker::new(found, &self.models));
                }
            }
            Operation::Save | Operation::Remove => {
                let reply: SavedProviders = serde_json::from_value(value)?;
                self.providers = reply.providers;
                self.catalog_providers = reply.catalog_providers;
                self.revision = reply.revision;
                self.loaded = true;
                let id = if action == Operation::Save {
                    self.provider_search.clear();
                    Some(reply.id)
                } else {
                    self.providers.first().map(|provider| provider.id.clone())
                };
                self.load(id);
                self.feedback = Some(
                    if action == Operation::Save {
                        "模型服务已保存"
                    } else {
                        "模型服务已删除"
                    }
                    .into(),
                );
            }
        }
        Ok(())
    }
}
