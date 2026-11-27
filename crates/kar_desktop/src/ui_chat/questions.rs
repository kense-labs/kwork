//! Field values belong to the specific request that owns them.
use super::decision_question;
use crate::{
    kar_bridge::KarCommand,
    ui_motion::{primary_button, soft_button, text_button},
    ui_primitives::{FS_META, FS_UI, R_CTRL, StudioPalette, UiLocale},
    workbench::KarWorkbench,
};
use gpui::{prelude::*, *};
use kar::{FieldKind, PendingRequest, QuestionField, QuestionForm, QuestionReply};
use kui::primitives::editor::{EditEvent, Editor, InputMode};
use serde_json::{Map, Value};

pub(crate) struct QuestionDraft {
    choices: Vec<Option<Value>>,
    editors: Vec<Option<Entity<Editor>>>,
    _events: Vec<Subscription>,
}

impl QuestionDraft {
    fn new(
        form: &QuestionForm,
        loc: UiLocale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut editors = Vec::new();
        let mut subscriptions = Vec::new();
        for field in &form.fields {
            let needs_editor = match field.kind {
                FieldKind::Boolean => false,
                FieldKind::Select | FieldKind::MultiSelect => {
                    field.allow_other || field.options.is_empty()
                }
                _ => true,
            };
            let editor = needs_editor.then(|| {
                let mode = if field.secret {
                    InputMode::Password
                } else if matches!(field.kind, FieldKind::Editor | FieldKind::MultiSelect) {
                    InputMode::Multi
                } else {
                    InputMode::Single
                };
                let handle = cx.new(|cx| {
                    let mut e = Editor::new(field.label.clone(), mode, window, cx);
                    e.font_size = FS_UI;
                    let placeholder = match field.kind {
                        FieldKind::Select => loc.pick("其他回答", "Other answer"),
                        FieldKind::MultiSelect => {
                            loc.pick("其他回答，每行一项", "Other answers, one per line")
                        }
                        FieldKind::Number | FieldKind::Integer => {
                            loc.pick("输入数字", "Enter a number")
                        }
                        _ => loc.pick("输入回答", "Enter your answer"),
                    };
                    e.set_placeholder(placeholder);
                    if let Some(value) = &field.default {
                        if !matches!(field.kind, FieldKind::Select | FieldKind::MultiSelect) {
                            let initial = value
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| value.to_string());
                            e.set_value(initial, window, cx);
                        }
                    }
                    e
                });
                subscriptions.push(cx.subscribe(&handle, |_, _, ev: &EditEvent, cx| {
                    if *ev == EditEvent::Changed {
                        cx.notify();
                    }
                }));
                handle
            });
            editors.push(editor);
        }
        Self {
            choices: form.fields.iter().map(|f| f.default.clone()).collect(),
            editors,
            _events: subscriptions,
        }
    }

    fn answers(&self, fields: &[QuestionField], cx: &App) -> Option<Map<String, Value>> {
        let mut values = Map::new();
        for (i, f) in fields.iter().enumerate() {
            let input = self.editors[i].as_ref().map(|e| e.read(cx).value());
            if let Some(value) = field_value(f, self.choices[i].as_ref(), input).ok()? {
                values.insert(f.id.clone(), value);
            }
        }
        Some(values)
    }
}

/// Empty optional fields are omitted. Text, numbers, booleans and lists retain their native types.
fn field_value(
    field: &QuestionField,
    choice: Option<&Value>,
    input: Option<&str>,
) -> Result<Option<Value>, ()> {
    let schema = field.schema.as_ref().unwrap_or(&Value::Null);
    let text = input.unwrap_or("");
    let bounded = |size: usize, min: &str, max: &str| {
        schema[min].as_u64().is_none_or(|v| size as u64 >= v)
            && schema[max].as_u64().is_none_or(|v| size as u64 <= v)
    };
    let value = match field.kind {
        FieldKind::Select if text.trim().is_empty() => choice.cloned(),
        FieldKind::Boolean => choice.cloned(),
        FieldKind::MultiSelect => {
            let mut items = choice
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for item in text.lines().map(str::trim).filter(|s| !s.is_empty()) {
                let item = Value::from(item);
                if !items.contains(&item) {
                    items.push(item);
                }
            }
            if items.is_empty() && choice.is_none() {
                None
            } else {
                if !bounded(items.len(), "minItems", "maxItems") {
                    return Err(());
                }
                Some(items.into())
            }
        }
        _ if text.is_empty() => None,
        FieldKind::Number | FieldKind::Integer => {
            let number = if field.kind == FieldKind::Integer {
                serde_json::Number::from(text.trim().parse::<i64>().map_err(|_| ())?)
            } else {
                serde_json::Number::from_f64(text.trim().parse().map_err(|_| ())?).ok_or(())?
            };
            let n = number.as_f64().ok_or(())?;
            if schema["minimum"].as_f64().is_some_and(|v| n < v)
                || schema["maximum"].as_f64().is_some_and(|v| n > v)
            {
                return Err(());
            }
            Some(number.into())
        }
        _ => {
            if !bounded(text.chars().count(), "minLength", "maxLength") {
                return Err(());
            }
            Some(Value::from(text))
        }
    };
    if value.is_none() && field.required {
        Err(())
    } else {
        Ok(value)
    }
}

impl KarWorkbench {
    pub(super) fn render_question_fields(
        &mut self,
        mut body: Div,
        request: PendingRequest,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let loc = self.ui_locale;
        let form = request.question_form();
        if let Some(form) = &form {
            if let Some(message) = &form.message {
                body = body.child(decision_question(message.clone(), s));
            }
            if let Some(url) = form.url.as_ref().filter(|url| !url.is_empty()) {
                let url = url.clone();
                body = body.child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_color(s.muted)
                                .child(url.clone()),
                        )
                        .child(
                            text_button("question-copy-url", loc.pick("复制链接", "Copy link"), s)
                                .on_press(cx, move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(url.clone()))
                                }),
                        ),
                );
            }
        }
        let confirms_tool = request.method == "mcpServer/elicitation/request"
            && request
                .body
                .pointer("/_meta/codex_approval_kind")
                .and_then(Value::as_str)
                == Some("mcp_tool_call")
            && form
                .as_ref()
                .is_some_and(|f| f.url.is_none() && f.fields.is_empty());
        let decline_label = if confirms_tool {
            loc.pick("拒绝", "Decline")
        } else {
            loc.pick("跳过", "Skip")
        };
        let confirm_label = if confirms_tool {
            loc.pick("允许", "Allow")
        } else if form.as_ref().is_some_and(|f| f.fields.is_empty()) {
            loc.pick("继续", "Continue")
        } else {
            loc.pick("提交答复", "Submit answers")
        };
        let skip = text_button("decision-skip", decline_label, s).on_press(cx, {
            let request = request.clone();
            move |this, _, _| {
                this.bridge.send(KarCommand::AnswerQuestion {
                    request: request.clone(),
                    reply: QuestionReply::Decline,
                })
            }
        });
        let Some(form) = form else {
            return body.child(skip);
        };
        let draft = if let Some((_, draft)) = self
            .question_draft
            .as_ref()
            .filter(|(cursor, _)| *cursor == request.cursor)
        {
            draft.clone()
        } else {
            let draft = cx.new(|cx| QuestionDraft::new(&form, loc, window, cx));
            cx.observe(&draft, |_, _, cx| cx.notify()).detach();
            self.question_draft = Some((request.cursor.clone(), draft.clone()));
            draft
        };
        let quick = form.fields.len() == 1 && {
            let field = &form.fields[0];
            field.kind == FieldKind::Boolean
                || (field.kind == FieldKind::Select
                    && !field.allow_other
                    && !field.options.is_empty())
        };
        for (i, field) in form.fields.iter().enumerate() {
            let selected = &draft.read(cx).choices[i];
            let mut row = div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(5.))
                .child(
                    div()
                        .text_size(s.font_size(FS_UI))
                        .text_color(s.ink)
                        .child(field.label.clone()),
                )
                .when_some(field.description.clone(), |row, desc| {
                    row.child(div().text_size(s.font_size(FS_META)).text_color(s.muted).child(desc))
                });
            let options: Vec<_> = if field.kind == FieldKind::Boolean {
                vec![
                    (loc.pick("是", "Yes").to_owned(), Value::Bool(true), None),
                    (loc.pick("否", "No").to_owned(), Value::Bool(false), None),
                ]
            } else {
                field
                    .options
                    .iter()
                    .map(|o| {
                        (
                            o.label.clone(),
                            Value::from(o.value.clone()),
                            o.description.clone(),
                        )
                    })
                    .collect()
            };
            if !options.is_empty() {
                let mut buttons = div().w_full().flex().flex_wrap().gap(px(5.));
                for (j, (label, value, description)) in options.into_iter().enumerate() {
                    let selected = !(field.kind == FieldKind::Select
                        && draft.read(cx).editors[i]
                            .as_ref()
                            .is_some_and(|e| !e.read(cx).value().is_empty()))
                        && selected.as_ref().is_some_and(|x| {
                            x == &value || x.as_array().is_some_and(|items| items.contains(&value))
                        });
                    let draft = draft.clone();
                    let kind = field.kind;
                    let request = request.clone();
                    let field_id = field.id.clone();
                    buttons = buttons.child(
                        soft_button(
                            SharedString::from(format!("question-choice-{i}-{j}")),
                            label,
                            s,
                        )
                        .lit(selected)
                        .when_some(description, |b, hint| b.tip(hint))
                        .on_press(cx, move |this, win, cx| {
                            if quick {
                                let mut values = Map::new();
                                values.insert(field_id.clone(), value.clone());
                                this.bridge.send(KarCommand::AnswerQuestion {
                                    request: request.clone(),
                                    reply: QuestionReply::Submit { values },
                                });
                            } else {
                                draft.update(cx, |draft, cx| {
                                    if kind == FieldKind::MultiSelect {
                                        let mut items = draft.choices[i]
                                            .take()
                                            .and_then(|v| v.as_array().cloned())
                                            .unwrap_or_default();
                                        if let Some(index) = items.iter().position(|v| v == &value)
                                        {
                                            items.remove(index);
                                        } else {
                                            items.push(value.clone());
                                        }
                                        draft.choices[i] = Some(items.into());
                                    } else {
                                        draft.choices[i] = Some(value.clone());
                                        if let Some(e) = &draft.editors[i] {
                                            e.update(cx, |e, cx| e.set_value("", win, cx));
                                        }
                                    }
                                    cx.notify();
                                });
                            }
                        }),
                    );
                }
                row = row.child(buttons);
            }
            if let Some(editor) = &draft.read(cx).editors[i] {
                let multi = editor.read(cx).mode == InputMode::Multi;
                row = row.child(
                    div()
                        .w_full()
                        .h(px(if multi { 58. } else { 32. }))
                        .px(px(8.))
                        .rounded(px(R_CTRL))
                        .bg(s.soft)
                        .child(editor.clone()),
                );
            }
            body = body.child(row);
        }
        let ready = draft.read(cx).answers(&form.fields, cx).is_some();
        body.child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(6.))
                .child(skip)
                .when(!quick, |d| {
                    d.child(
                        primary_button("decision-submit", confirm_label, s)
                            .disabled(!ready)
                            .on_press(cx, move |this, _, cx| {
                                if let Some(values) = draft.read(cx).answers(&form.fields, cx) {
                                    this.bridge.send(KarCommand::AnswerQuestion {
                                        request: request.clone(),
                                        reply: QuestionReply::Submit { values },
                                    });
                                }
                            }),
                    )
                }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::field_value;
    use kar::{FieldKind, QuestionField};
    use serde_json::{Value, json};

    fn field(kind: FieldKind, schema: Value) -> QuestionField {
        QuestionField {
            id: "id".into(),
            label: "Field".into(),
            kind,
            header: None,
            description: None,
            options: Vec::new(),
            required: true,
            allow_other: false,
            secret: false,
            default: None,
            schema: Some(schema),
        }
    }

    #[test]
    fn only_explicit_answers_satisfy_required_fields() {
        let mut selector = field(FieldKind::Select, Value::Null);
        assert!(field_value(&selector, None, None).is_err());
        assert_eq!(
            field_value(&selector, Some(&json!("mint")), None),
            Ok(Some(json!("mint")))
        );
        selector.required = false;
        assert_eq!(field_value(&selector, None, None), Ok(None));
        assert_eq!(
            field_value(
                &field(FieldKind::Boolean, Value::Null),
                Some(&json!(false)),
                None
            ),
            Ok(Some(json!(false)))
        );
    }

    #[test]
    fn validates_provider_limits_and_submits_typed_values() {
        let numeric = field(FieldKind::Integer, json!({"minimum":2,"maximum":4}));
        for invalid in ["", "text", "2.3", "5"] {
            assert!(field_value(&numeric, None, Some(invalid)).is_err());
        }
        assert_eq!(field_value(&numeric, None, Some("3")), Ok(Some(json!(3))));
        let answer = field(FieldKind::Text, json!({"minLength":2}));
        assert!(field_value(&answer, None, Some("文")).is_err());
        assert_eq!(
            field_value(&answer, None, Some("文字")),
            Ok(Some(json!("文字")))
        );
        let list = field(FieldKind::MultiSelect, json!({"minItems":2,"maxItems":3}));
        assert!(field_value(&list, Some(&json!(["one"])), None).is_err());
        assert_eq!(
            field_value(&list, Some(&json!(["one"])), Some("two\none")),
            Ok(Some(json!(["one", "two"])))
        );
    }
}
