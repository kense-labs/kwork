use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Normal,
    Always,
}

#[derive(Clone, Copy, Deserialize)]
struct NumberSpec {
    value: f64,
    min: f64,
    max: f64,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackoffSpec {
    initial_delay_ms: NumberSpec,
    max_delay_ms: NumberSpec,
    jitter_ratio: NumberSpec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RetryPolicySpec {
    max_retries: NumberSpec,
    normal_backoff: BackoffSpec,
    always_backoff: BackoffSpec,
    choices: Vec<String>,
    default_codes: Vec<String>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RetryPolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<Mode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_retries: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retryable_codes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    backoff: Option<Backoff>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Backoff {
    #[serde(skip_serializing_if = "Option::is_none")]
    initial_delay_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_delay_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    jitter_ratio: Option<f64>,
}

#[derive(Default)]
pub(super) struct RetryDraft {
    inherited: RetryPolicy,
    mode: Option<Mode>,
    max_retries: String,
    initial_delay_ms: String,
    max_delay_ms: String,
    jitter_ratio: String,
    codes: Option<Vec<String>>,
    added_codes: Vec<String>,
    custom_code: String,
}

#[derive(PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RetryIssue {
    field: &'static str,
    message: String,
}

#[derive(PartialEq, Eq, Serialize)]
struct RetryNumber {
    text: String,
    placeholder: String,
}

#[derive(PartialEq, Eq, Serialize)]
struct RetryCode {
    code: String,
    selected: bool,
}

#[derive(PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RetryForm {
    mode: Option<Mode>,
    inherited_mode: Mode,
    max_retries: RetryNumber,
    initial_delay_ms: RetryNumber,
    max_delay_ms: RetryNumber,
    jitter_ratio: RetryNumber,
    codes: Vec<RetryCode>,
    custom_code: String,
    has_custom_backoff: bool,
    issue: Option<RetryIssue>,
}

impl RetryDraft {
    pub(super) fn load(user: Option<RetryPolicy>, inherited: Option<RetryPolicy>) -> Self {
        let inherited = inherited.unwrap_or_default();
        let Some(user) = user else {
            return Self {
                inherited,
                ..Self::default()
            };
        };
        let mode = user.mode.or(inherited.mode).or(Some(Mode::Normal));
        let backoff = user.backoff.unwrap_or_default();
        Self {
            inherited,
            mode,
            max_retries: display(user.max_retries),
            initial_delay_ms: display(backoff.initial_delay_ms),
            max_delay_ms: display(backoff.max_delay_ms),
            jitter_ratio: display(backoff.jitter_ratio),
            codes: user.retryable_codes,
            ..Self::default()
        }
    }

    pub(super) fn set_mode(&mut self, value: &str) -> bool {
        let mode = match value {
            "" => None,
            "normal" => Some(Mode::Normal),
            "always" => Some(Mode::Always),
            _ => return false,
        };
        if mode == self.mode {
            return false;
        }
        self.mode = mode;
        self.custom_code.clear();
        if mode != Some(Mode::Normal) {
            self.codes = None;
            self.max_retries.clear();
        }
        if mode.is_none() {
            self.initial_delay_ms.clear();
            self.max_delay_ms.clear();
            self.jitter_ratio.clear();
        }
        true
    }

    pub(super) fn set_field(&mut self, field: &str, value: &str) -> bool {
        let text = match (self.mode, field) {
            (None, _) => return false,
            (Some(Mode::Normal), "retry-max-retries") => &mut self.max_retries,
            (Some(Mode::Normal), "retry-custom-code") => &mut self.custom_code,
            (_, "retry-initial-delay") => &mut self.initial_delay_ms,
            (_, "retry-max-delay") => &mut self.max_delay_ms,
            (_, "retry-jitter") => &mut self.jitter_ratio,
            _ => return false,
        };
        if *text == value {
            return false;
        }
        *text = value.into();
        true
    }

    fn inherited_codes<'a>(&'a self, spec: &'a RetryPolicySpec) -> &'a [String] {
        self.inherited
            .retryable_codes
            .as_deref()
            .unwrap_or(&spec.default_codes)
    }

    fn selected_codes<'a>(&'a self, spec: &'a RetryPolicySpec) -> &'a [String] {
        self.codes
            .as_deref()
            .unwrap_or_else(|| self.inherited_codes(spec))
    }

    pub(super) fn edit_code(&mut self, spec: &RetryPolicySpec, code: &str, selected: bool) -> bool {
        if self.mode != Some(Mode::Normal) || code.is_empty() {
            return false;
        }
        let inherited = self.inherited_codes(spec);
        let mut codes = self.selected_codes(spec).to_vec();
        let present = codes.iter().any(|item| item == code);
        if present == selected || !selected && codes.len() == 1 {
            return false;
        }
        if selected {
            codes.push(code.into());
        } else {
            codes.retain(|item| item != code);
        }
        let inherits =
            codes.len() == inherited.len() && codes.iter().all(|item| inherited.contains(item));
        self.codes = (!inherits).then_some(codes);
        true
    }

    pub(super) fn add_code(&mut self, spec: &RetryPolicySpec) -> bool {
        let code = self.custom_code.trim().to_string();
        if self.mode != Some(Mode::Normal) || code.is_empty() {
            return false;
        }
        self.custom_code.clear();
        if !self.added_codes.contains(&code) {
            self.added_codes.push(code.clone());
        }
        self.edit_code(spec, &code, true);
        true
    }

    fn backoff_spec(&self, spec: &RetryPolicySpec) -> BackoffSpec {
        let mut limits = match self.mode.or(self.inherited.mode).unwrap_or(Mode::Normal) {
            Mode::Normal => spec.normal_backoff,
            Mode::Always => spec.always_backoff,
        };
        if let Some(inherited) = &self.inherited.backoff {
            limits.initial_delay_ms.value = inherited
                .initial_delay_ms
                .unwrap_or(limits.initial_delay_ms.value);
            limits.max_delay_ms.value = inherited.max_delay_ms.unwrap_or(limits.max_delay_ms.value);
            limits.jitter_ratio.value = inherited.jitter_ratio.unwrap_or(limits.jitter_ratio.value);
        }
        limits
    }

    fn max_retries_spec(&self, spec: &RetryPolicySpec) -> NumberSpec {
        let mut limits = spec.max_retries;
        if let Some(value) = self.inherited.max_retries {
            limits.value = value as f64;
        }
        limits
    }

    pub(super) fn policy(&self, spec: &RetryPolicySpec) -> Result<Option<RetryPolicy>, RetryIssue> {
        let Some(mode) = self.mode else {
            return Ok(None);
        };
        let mut policy = RetryPolicy {
            mode: Some(mode),
            ..RetryPolicy::default()
        };
        if mode == Mode::Normal {
            policy.max_retries = read_number(
                "retry-max-retries",
                &self.max_retries,
                self.max_retries_spec(spec),
                true,
            )?
            .map(|number| number as u64);
            let codes = self.selected_codes(spec);
            if codes.is_empty() || codes.iter().any(|code| code.is_empty()) {
                return Err(issue("retry-codes", "请至少选择一个有效错误码。"));
            }
            policy.retryable_codes = self.codes.clone();
        }
        let limits = self.backoff_spec(spec);
        let initial = read_number(
            "retry-initial-delay",
            &self.initial_delay_ms,
            limits.initial_delay_ms,
            false,
        )?;
        let maximum = read_number(
            "retry-max-delay",
            &self.max_delay_ms,
            limits.max_delay_ms,
            false,
        )?;
        let jitter = read_number(
            "retry-jitter",
            &self.jitter_ratio,
            limits.jitter_ratio,
            false,
        )?;
        if initial.unwrap_or(limits.initial_delay_ms.value)
            > maximum.unwrap_or(limits.max_delay_ms.value)
        {
            return Err(issue("retry-initial-delay", "初始延迟不能大于最大延迟。"));
        }
        if initial.is_some() || maximum.is_some() || jitter.is_some() {
            policy.backoff = Some(Backoff {
                initial_delay_ms: initial,
                max_delay_ms: maximum,
                jitter_ratio: jitter,
            });
        }
        Ok(Some(policy))
    }

    pub(super) fn form(&self, spec: &RetryPolicySpec) -> RetryForm {
        let limits = self.backoff_spec(spec);
        let number = |text: &String, field: NumberSpec| RetryNumber {
            text: text.clone(),
            placeholder: field.value.to_string(),
        };
        let selected = self.selected_codes(spec);
        let mut codes = Vec::new();
        for code in spec
            .choices
            .iter()
            .chain(self.inherited_codes(spec))
            .chain(selected)
            .chain(&self.added_codes)
        {
            if !codes.iter().any(|item: &RetryCode| item.code == *code) {
                codes.push(RetryCode {
                    code: code.clone(),
                    selected: selected.contains(code),
                });
            }
        }
        RetryForm {
            mode: self.mode,
            inherited_mode: self.inherited.mode.unwrap_or(Mode::Normal),
            max_retries: number(&self.max_retries, self.max_retries_spec(spec)),
            initial_delay_ms: number(&self.initial_delay_ms, limits.initial_delay_ms),
            max_delay_ms: number(&self.max_delay_ms, limits.max_delay_ms),
            jitter_ratio: number(&self.jitter_ratio, limits.jitter_ratio),
            codes,
            custom_code: self.custom_code.clone(),
            has_custom_backoff: [
                &self.initial_delay_ms,
                &self.max_delay_ms,
                &self.jitter_ratio,
            ]
            .iter()
            .any(|value| !value.is_empty()),
            issue: self.policy(spec).err(),
        }
    }
}

fn display(number: Option<impl ToString>) -> String {
    number.map_or_else(String::new, |number| number.to_string())
}

fn issue(field: &'static str, message: impl Into<String>) -> RetryIssue {
    RetryIssue {
        field,
        message: message.into(),
    }
}

fn read_number(
    field: &'static str,
    text: &str,
    limits: NumberSpec,
    integer: bool,
) -> Result<Option<f64>, RetryIssue> {
    let parsed = if text.trim().is_empty() {
        None
    } else {
        Some(
            text.trim()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| issue(field, "请输入数字。"))?,
        )
    };
    let value = parsed.unwrap_or(limits.value);
    if integer && value.fract() != 0. {
        return Err(issue(field, "请输入整数。"));
    }
    if value < limits.min {
        let message = if limits.min == f64::from_bits(1) {
            "必须大于 0。".to_string()
        } else {
            format!("不能小于 {}。", limits.min)
        };
        return Err(issue(field, message));
    }
    if value > limits.max {
        return Err(issue(field, format!("不能大于 {}。", limits.max)));
    }
    Ok(parsed)
}
