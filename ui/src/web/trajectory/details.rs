//! Inspector structure and tab selection mirror upstream TrajectoryTable.

use super::super::workspace::{MarkdownOptions, render_markdown};
use super::*;

impl DshApp {
    fn trajectory_program_input(&self, record: &Record, cx: &Context<Self>) -> gpui::Div {
        self.trajectory_json(
            record,
            &json!({
                "kind": if record.kind == Kind::Subtool { "subtool" } else { "tool" },
                "toolName": record.name,
                "inputDetail": record.entry.input,
                "schemaDetail": record.detail.schema.as_ref().map(Value::to_string),
            })
            .to_string(),
            "program-input",
            cx,
        )
    }

    fn trajectory_program_output(&self, record: &Record, cx: &Context<Self>) -> gpui::Div {
        let output = (!record.pending() || !record.output.is_empty()).then_some(&record.output);
        self.trajectory_json(
            record,
            &json!({
                "cell": { "outputDetail": output, "isError": record.failed() },
                "running": record.pending(),
            })
            .to_string(),
            "program-output",
            cx,
        )
    }

    fn trajectory_thinking_expanded(&self, record: &Record) -> bool {
        self.trajectory
            .thinking_disclosure
            .as_ref()
            .filter(|(key, _)| key == &record.key)
            .map_or(
                record.kind == Kind::Message && !record.thinking.trim().is_empty(),
                |(_, expanded)| *expanded,
            )
    }

    pub(super) fn toggle_trajectory_thinking(&mut self, cx: &mut Context<Self>) {
        if let Some(record) = self
            .conversation
            .trajectory
            .records
            .iter()
            .find(|record| Some(&record.key) == self.trajectory.selected.as_ref())
        {
            self.trajectory.thinking_disclosure = Some((
                record.key.clone(),
                !self.trajectory_thinking_expanded(record),
            ));
            cx.notify();
        }
    }

    pub(super) fn open_trajectory_summary(
        &mut self,
        key: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .conversation
            .trajectory
            .records
            .iter()
            .chain(&self.conversation.trajectory.requests)
            .any(|record| record.key == key)
        {
            return;
        }
        crate::trajectory::hierarchy::unfold_for_navigation(&mut self.conversation.trajectory, key);
        self.select_trajectory_record(key.to_owned(), w, cx);
        self.activate_trajectory_detail(DetailTab::Overview, cx);
        self.trajectory.focused_record = Some(key.to_owned());
    }

    fn trajectory_detail_links(&self, record: &Record, cx: &mut Context<Self>) -> Vec<gpui::Div> {
        use crate::trajectory::hierarchy::{parents, source_label};
        let mut groups = Vec::new();
        if matches!(record.kind, Kind::User | Kind::Context)
            && let Some(source) = &record.detail.source
        {
            groups.push((
                "来源",
                vec![("section", "Source".to_owned(), source_label(source))],
            ));
        }
        if record.kind == Kind::Message {
            if let Some((index, request)) = self
                .conversation
                .trajectory
                .requests
                .iter()
                .enumerate()
                .find(|(_, item)| item.turn == record.turn && item.step == record.step)
            {
                groups.push((
                    "来源",
                    vec![(
                        "hierarchy",
                        request.key.clone(),
                        format!("请求 #{}", index + 1),
                    )],
                ));
            }
        } else {
            let (message, tool) = parents(&self.conversation.trajectory.records, record);
            let mut links = Vec::new();
            if let Some(message) = message {
                links.push(("hierarchy", message.key.clone(), "助手消息".to_owned()));
            }
            if let Some(tool) = tool {
                links.push(("hierarchy", tool.key.clone(), "工具调用".to_owned()));
            }
            if !links.is_empty() {
                groups.push(("层级", links));
            }
        }
        let p = dsh(cx);
        groups
            .into_iter()
            .map(|(label, links)| {
                row()
                    .gap_0()
                    .w_full()
                    .min_h(px(22.))
                    .px(px(14.))
                    .items_start()
                    .text_size(px(13.))
                    .line_height(px(22.))
                    .child(
                        div()
                            .w(px(94.))
                            .flex_none()
                            .text_color(p.faint)
                            .child(label),
                    )
                    .child(
                        row()
                            .gap(px(14.))
                            .flex_1()
                            .min_w_0()
                            .children(links.into_iter().map(|(action, key, label)| {
                                let keyboard_key = key.clone();
                                row()
                                    .relative()
                                    .gap(px(1.))
                                    .id(format!("trajectory-link-{action}-{key}"))
                                    .role(gpui::Role::Button)
                                    .aria_label(label.clone())
                                    .track_focus(&self.trajectory_action_focus(action, &key, cx))
                                    .tab_index(0)
                                    .cursor_pointer()
                                    .text_color(p.secondary)
                                    .hover(|link| link.text_color(p.label_primary))
                                    .focus_visible(|link| {
                                        link.shadow(vec![
                                            gpui::BoxShadow::new(
                                                px(0.),
                                                px(0.),
                                                p.state_business_primary,
                                            )
                                            .spread_radius(px(1.)),
                                        ])
                                    })
                                    .child(label.clone())
                                    .child(
                                        gpui::svg()
                                            .data(include_bytes!(
                                                "../../../assets/header-chevron-right.svg"
                                            ))
                                            .size(px(11.))
                                            .text_color(p.faint),
                                    )
                                    .child(self.trajectory_accessible(
                                        action,
                                        key.clone(),
                                        label,
                                        None,
                                        None,
                                        false,
                                    ))
                                    .on_click(cx.listener(move |this, _, w, cx| {
                                        if action == "section" {
                                            this.activate_trajectory_detail(DetailTab::Source, cx);
                                        } else {
                                            this.open_trajectory_summary(&key, w, cx);
                                        }
                                    }))
                                    .on_key_down(cx.listener(
                                        move |this, event: &gpui::KeyDownEvent, w, cx| {
                                            if matches!(
                                                event.keystroke.key.as_str(),
                                                "enter" | "space"
                                            ) && !event.keystroke.modifiers.modified()
                                            {
                                                if action == "section" {
                                                    this.activate_trajectory_detail(
                                                        DetailTab::Source,
                                                        cx,
                                                    );
                                                } else {
                                                    this.open_trajectory_summary(
                                                        &keyboard_key,
                                                        w,
                                                        cx,
                                                    );
                                                }
                                                w.prevent_default();
                                                cx.stop_propagation();
                                            }
                                        },
                                    ))
                            })),
                    )
            })
            .collect()
    }

    pub(super) fn activate_trajectory_detail(&mut self, tab: DetailTab, cx: &mut Context<Self>) {
        self.trajectory
            .detail_tab_history
            .retain(|item| *item != tab);
        self.trajectory.detail_tab_history.push(tab);
        self.trajectory.detail_tab = tab;
        self.trajectory.unix_timestamp = false;
        self.trajectory
            .inspector_scroll
            .set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    }

    fn trajectory_detail_markdown(
        &self,
        record: &Record,
        source: &str,
        suffix: &str,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        render_markdown(
            source,
            &format!(
                "trajectory-{}-{}-{suffix}",
                record.key,
                record.source_seq.unwrap_or(0.)
            ),
            MarkdownOptions {
                dom: None,
                copied_mark: self.copied_mark.as_deref(),
                wrapped_code: &self.conversation.wrapped_code,
                font_delta: -1,
                selection: None,
                mentions: self.conversation.mention_vocab.get(&record.entry.key),
                settled: true,
                cache: super::super::conversation::MarkdownCache {
                    highlights: &self.conversation.markdown_highlights,
                    math: &self.conversation.markdown_math,
                    expanded_diffs: &self.conversation.expanded_diffs,
                },
                active_session: self.active_session.as_deref(),
            },
            cx,
        )
    }

    fn trajectory_detail_payload(
        &self,
        record: &Record,
        source: &str,
        suffix: &str,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        if serde_json::from_str::<Value>(source)
            .is_ok_and(|value| value.is_object() || value.is_array())
        {
            return self.trajectory_json(record, source, suffix, cx);
        }
        self.trajectory_json(
            record,
            &json!({"value":source}).to_string(),
            &format!("literal-{suffix}"),
            cx,
        )
    }

    fn trajectory_tool_output(&self, record: &Record, cx: &Context<Self>) -> gpui::Div {
        self.trajectory_json(
            record,
            &json!({"value":record.output,"error":record.failed(),"blocks":record.detail.blocks})
                .to_string(),
            "tool-output",
            cx,
        )
    }

    fn trajectory_detail_preview(&self, record: &Record, cx: &mut Context<Self>) -> gpui::Div {
        let p = dsh(cx);
        let mut content = column().gap_0().w_full().gap(px(10.));
        if !record.thinking.is_empty() {
            let expanded = self.trajectory_thinking_expanded(record);
            let thinking =
                self.trajectory_detail_markdown(record, &record.thinking, "thinking", cx);
            content = content.child(
                column()
                    .gap_0()
                    .w_full()
                    .border_l(px(2.))
                    .border_color(p.border_l2)
                    .pl(px(10.))
                    .child(
                        row()
                            .gap_0()
                            .id("trajectory-thinking-toggle")
                            .relative()
                            .role(gpui::Role::Button)
                            .aria_label("思考")
                            .aria_expanded(expanded)
                            .track_focus(&self.trajectory_action_focus("thinking", "", cx))
                            .tab_index(0)
                            .cursor_pointer()
                            .text_size(px(12.))
                            .text_color(p.secondary)
                            .gap(px(4.))
                            .child("思考")
                            .child(if expanded { "⌄" } else { "›" })
                            .child(self.trajectory_accessible(
                                "thinking",
                                "",
                                "思考",
                                None,
                                Some(expanded),
                                false,
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_trajectory_thinking(cx);
                            }))
                            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, w, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.toggle_trajectory_thinking(cx);
                                    w.prevent_default();
                                    cx.stop_propagation();
                                }
                            })),
                    )
                    .when(expanded, |body| body.child(thinking)),
            );
        }
        if !record.output.is_empty() {
            content = content.child(self.trajectory_detail_markdown(
                record,
                &record.output,
                "preview",
                cx,
            ));
        }
        content
    }

    fn trajectory_detail_section(
        &self,
        record: &Record,
        tab: DetailTab,
        body: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let p = dsh(cx);
        column()
            .gap_0()
            .w_full()
            .min_w_0()
            .child(
                row()
                    .gap_0()
                    .id(format!("trajectory-section-{tab:?}"))
                    .relative()
                    .role(gpui::Role::Button)
                    .aria_label(format!("打开{}", tab.label(record)))
                    .track_focus(&self.trajectory_action_focus("section", &format!("{tab:?}"), cx))
                    .tab_index(0)
                    .w_full()
                    .h(px(24.))
                    .px(px(14.))
                    .gap(px(4.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(p.secondary)
                    .child(tab.label(record))
                    .child("›")
                    .child(self.trajectory_accessible(
                        "section",
                        format!("{tab:?}"),
                        format!("打开{}", tab.label(record)),
                        None,
                        None,
                        false,
                    ))
                    .hover(|row| row.text_color(p.label_primary))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.activate_trajectory_detail(tab, cx)),
                    )
                    .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, w, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.activate_trajectory_detail(tab, cx);
                            w.prevent_default();
                            cx.stop_propagation();
                        }
                    })),
            )
            .child(
                if matches!(
                    tab,
                    DetailTab::Usage
                        | DetailTab::Timing
                        | DetailTab::Schema
                        | DetailTab::Input
                        | DetailTab::Options
                        | DetailTab::Source
                        | DetailTab::Tools
                        | DetailTab::Output
                ) {
                    body
                } else {
                    div().w_full().px(px(14.)).py(px(8.)).child(body)
                },
            )
    }

    fn trajectory_detail_metadata(
        &self,
        values: Vec<(&'static str, String)>,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let p = dsh(cx);
        column()
            .gap_0()
            .w_full()
            .py(px(8.))
            .children(values.into_iter().map(|(label, value)| {
                row()
                    .gap_0()
                    .w_full()
                    .min_h(px(22.))
                    .px(px(14.))
                    .items_start()
                    .text_size(px(13.))
                    .line_height(px(22.))
                    .child(
                        div()
                            .w(px(94.))
                            .flex_none()
                            .text_color(p.faint)
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(p.label_primary)
                            .child(value),
                    )
            }))
    }

    fn trajectory_detail_timing(&self, record: &Record, cx: &mut Context<Self>) -> gpui::Div {
        let p = dsh(cx);
        let timestamp = record.timing.started_at.filter(|time| time.is_finite());
        let started = timestamp
            .map(|time| {
                if self.trajectory.unix_timestamp {
                    return format!("{:.3}", time / 1000.);
                }
                #[cfg(target_family = "wasm")]
                {
                    let date = js_sys::Date::new(&time.into());
                    format!(
                        "{}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
                        date.get_full_year(),
                        date.get_month() + 1,
                        date.get_date(),
                        date.get_hours(),
                        date.get_minutes(),
                        date.get_seconds(),
                        date.get_milliseconds()
                    )
                }
                #[cfg(not(target_family = "wasm"))]
                {
                    format!("{:.3}", time / 1000.)
                }
            })
            .unwrap_or_else(|| "—".into());
        let mut values = Vec::new();
        if record.kind == Kind::Message && record.timing.completed_at.is_some() {
            values.push(("总时长", format_duration(record.timing.duration_ms())));
            values.push((
                "首 token 延迟",
                record
                    .timing
                    .ttft_ms()
                    .map(|time| format_duration(Some(time)))
                    .unwrap_or_else(|| "首 token 时间不可用".into()),
            ));
            values.push((
                "生成",
                record
                    .timing
                    .decoding_ms()
                    .map(|time| format_duration(Some(time)))
                    .unwrap_or_else(|| "首 token 时间不可用".into()),
            ));
            let throughput = match (record.usage, record.timing.decoding_ms()) {
                (None, _) => "用量不可用".into(),
                (Some(usage), Some(ms)) if ms > 0. => usage
                    .output
                    .map(|count| format!("{:.1} tok/s", count as f64 / ms * 1000.))
                    .unwrap_or_else(|| "输出 token 数不可用".into()),
                (_, Some(_)) => "时长过短".into(),
                _ => "首 token 时间不可用".into(),
            };
            values.push(("吞吐量", throughput));
        } else {
            values.push(("时长", format_duration(record.timing.duration_ms())));
            values.push((
                "计时来源",
                if record.pending() {
                    "会话时间戳（运行中）"
                } else {
                    "会话时间戳"
                }
                .into(),
            ));
        }
        column()
            .gap_0()
            .w_full()
            .pt(px(8.))
            .child(
                row()
                    .gap_0()
                    .w_full()
                    .min_h(px(22.))
                    .px(px(14.))
                    .text_size(px(13.))
                    .line_height(px(22.))
                    .child(
                        div()
                            .w(px(94.))
                            .flex_none()
                            .text_color(p.faint)
                            .child("开始时间"),
                    )
                    .child(
                        div()
                            .id("trajectory-started-at")
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .child(started)
                            .when(timestamp.is_some(), |value| {
                                value
                                    .role(gpui::Role::Button)
                                    .aria_label(if self.trajectory.unix_timestamp {
                                        "显示本地时间"
                                    } else {
                                        "显示 Unix 时间戳"
                                    })
                                    .tab_index(0)
                                    .track_focus(&self.trajectory_action_focus("timestamp", "", cx))
                                    .cursor_pointer()
                                    .child(self.trajectory_accessible(
                                        "timestamp",
                                        "",
                                        if self.trajectory.unix_timestamp {
                                            "显示本地时间"
                                        } else {
                                            "显示 Unix 时间戳"
                                        },
                                        None,
                                        None,
                                        false,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.trajectory.unix_timestamp =
                                            !this.trajectory.unix_timestamp;
                                        cx.notify();
                                    }))
                                    .on_key_down(cx.listener(
                                        |this, event: &gpui::KeyDownEvent, w, cx| {
                                            if matches!(
                                                event.keystroke.key.as_str(),
                                                "enter" | "space"
                                            ) {
                                                this.trajectory.unix_timestamp =
                                                    !this.trajectory.unix_timestamp;
                                                w.prevent_default();
                                                cx.stop_propagation();
                                                cx.notify();
                                            }
                                        },
                                    ))
                            }),
                    ),
            )
            .child(self.trajectory_detail_metadata(values, cx).pt_0())
    }

    fn trajectory_detail_usage(&self, record: &Record, cx: &Context<Self>) -> gpui::Div {
        let p = dsh(cx);
        let Some(usage) = record.usage else {
            return div()
                .px(px(14.))
                .py(px(8.))
                .text_size(px(13.))
                .text_color(p.faint)
                .child("未报告用量");
        };
        let mut values = Vec::new();
        if usage.input.is_some() || usage.cache_read.is_some() || usage.cache_write.is_some() {
            values.push((
                "输入",
                format!(
                    "{} tok",
                    usage.input.unwrap_or(0)
                        + usage.cache_read.unwrap_or(0)
                        + usage.cache_write.unwrap_or(0)
                ),
            ));
        }
        for (label, value) in [
            ("缓存读取", usage.cache_read),
            ("缓存写入", usage.cache_write),
            ("其他", usage.input),
            ("输出", usage.output),
            ("推理", usage.thinking),
        ] {
            if let Some(value) = value {
                values.push((label, format!("{value} tok")));
            }
        }
        if let (Some(output), Some(thinking)) = (usage.output, usage.thinking) {
            values.push(("内容", format!("{} tok", output.saturating_sub(thinking))));
        }
        self.trajectory_detail_metadata(values, cx)
    }

    fn trajectory_detail_content(
        &self,
        record: &Record,
        request: bool,
        tab: DetailTab,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let p = dsh(cx);
        let payload = |source: &str, suffix: &str, cx: &mut Context<Self>| {
            self.trajectory_detail_payload(record, source, suffix, cx)
        };
        match tab {
            DetailTab::Overview => {
                let status = if record.failed() {
                    "失败"
                } else if record.pending() {
                    "等待中"
                } else {
                    "已完成"
                };
                let mut metadata = vec![("状态", status.to_owned())];
                if request {
                    if let Some(provider) = &record.provider {
                        metadata.push(("提供方", provider.clone()));
                    }
                    if let Some(model) = &record.model {
                        metadata.push(("模型", model.clone()));
                    }
                    let calls = self
                        .conversation
                        .trajectory
                        .records
                        .iter()
                        .filter(|item| item.turn == record.turn && item.step == record.step);
                    metadata.push((
                        "工具调用",
                        calls
                            .clone()
                            .filter(|item| item.kind == Kind::Tool)
                            .count()
                            .to_string(),
                    ));
                    let subcalls = calls.filter(|item| item.kind == Kind::Subtool).count();
                    if subcalls > 0 {
                        metadata.push(("子工具调用", subcalls.to_string()));
                    }
                } else if matches!(record.kind, Kind::User | Kind::Context | Kind::Compacted) {
                    metadata.push(("总时长", format_duration(record.timing.duration_ms())));
                }
                let mut body = column()
                    .gap_0()
                    .w_full()
                    .when_some(record.program(), |body, program| {
                        body.when(!program.description.is_empty(), |body| {
                            body.child(
                                div()
                                    .px(px(14.))
                                    .pt(px(8.))
                                    .pb(px(2.))
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(p.label_primary)
                                    .child(program.description),
                            )
                        })
                    })
                    .child(
                        column()
                            .gap_0()
                            .w_full()
                            .py(px(8.))
                            .children(if request {
                                Vec::new()
                            } else {
                                self.trajectory_detail_links(record, cx)
                            })
                            .child(self.trajectory_detail_metadata(metadata, cx).py_0()),
                    );
                if request {
                    if let Some(options) = &record.options {
                        let content = self.trajectory_detail_payload(
                            record,
                            &options.to_string(),
                            "options",
                            cx,
                        );
                        body = body.child(self.trajectory_detail_section(
                            record,
                            DetailTab::Options,
                            content,
                            cx,
                        ));
                    }
                    body = body
                        .child(self.trajectory_detail_section(
                            record,
                            DetailTab::Usage,
                            self.trajectory_detail_usage(record, cx),
                            cx,
                        ))
                        .child(self.trajectory_detail_section(
                            record,
                            DetailTab::Timing,
                            self.trajectory_detail_timing(record, cx),
                            cx,
                        ));
                } else if matches!(
                    record.kind,
                    Kind::User | Kind::Context | Kind::Message | Kind::Compacted
                ) {
                    if record.kind == Kind::Message {
                        let mut tokens = vec![(
                            "Token",
                            record
                                .usage
                                .and_then(|usage| usage.output)
                                .map(|value| format!("{value} tok"))
                                .unwrap_or_else(|| "—".into()),
                        )];
                        if let Some(usage) = record.usage {
                            if let Some(thinking) = usage.thinking {
                                tokens.push(("推理", format!("{thinking} tok")));
                            }
                            if let (Some(output), Some(thinking)) = (usage.output, usage.thinking) {
                                tokens.push((
                                    "内容",
                                    format!("{} tok", output.saturating_sub(thinking)),
                                ));
                            }
                        }
                        body = body.child(self.trajectory_detail_metadata(tokens, cx));
                    }
                    let preview = self.trajectory_detail_preview(record, cx);
                    body = if record.kind == Kind::Compacted {
                        body.child(div().p(px(14.)).child(preview))
                    } else {
                        body.child(self.trajectory_detail_section(
                            record,
                            DetailTab::Rendered,
                            preview,
                            cx,
                        ))
                    };
                } else {
                    if record.program().is_some() {
                        body = body
                            .child(self.trajectory_program_input(record, cx))
                            .child(self.trajectory_program_output(record, cx));
                    } else if record.detail_tabs(false).contains(&DetailTab::Input) {
                        let content = self.trajectory_detail_payload(
                            record,
                            &record.entry.input,
                            "input",
                            cx,
                        );
                        body = body.child(self.trajectory_detail_section(
                            record,
                            DetailTab::Input,
                            content,
                            cx,
                        ));
                    }
                    if record.program().is_none() && !record.output.is_empty() {
                        let content = self.trajectory_tool_output(record, cx);
                        body = body.child(self.trajectory_detail_section(
                            record,
                            DetailTab::Output,
                            content,
                            cx,
                        ));
                    }
                    let schema =
                        self.trajectory_detail_content(record, false, DetailTab::Schema, cx);
                    body = body
                        .child(self.trajectory_detail_section(
                            record,
                            DetailTab::Schema,
                            schema,
                            cx,
                        ))
                        .child(self.trajectory_detail_section(
                            record,
                            DetailTab::Timing,
                            self.trajectory_detail_timing(record, cx),
                            cx,
                        ));
                }
                body
            }
            DetailTab::Rendered => div()
                .w_full()
                .p(px(14.))
                .child(self.trajectory_detail_preview(record, cx)),
            DetailTab::Raw => {
                if record.detail.blocks.is_empty() {
                    let source = [&record.thinking, &record.output]
                        .into_iter()
                        .filter(|text| !text.is_empty())
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    div()
                        .w_full()
                        .p(px(14.))
                        .text_size(px(12.))
                        .line_height(px(19.))
                        .font_family(crate::web::document_fonts::code_family(cx))
                        .map(|mut element| {
                            element.text_style().font_fallbacks =
                                Some(crate::web::document_fonts::code_fallbacks(cx));
                            element
                        })
                        .bg(p.md_code_block)
                        .child(source)
                } else {
                    column().gap_0().w_full().px(px(14.)).py(px(12.)).gap(px(14.))
                        .children(record.detail.blocks.iter().enumerate().map(|(index, block)| {
                            let kind = block["type"].as_str().unwrap_or("text");
                            let kind = if kind == "reasoning" { "thinking" } else { kind };
                            let source = block["text"].as_str().map(str::to_owned)
                                .unwrap_or_else(|| serde_json::to_string_pretty(block).unwrap_or_default());
                            let call = block.get("id").or_else(|| block.get("callId")).and_then(Value::as_str)
                                .filter(|_| matches!(kind, "tool-call" | "tool_call"))
                                .and_then(|id| self.conversation.trajectory.records.iter().find(|record| record.entry.external_id.as_deref() == Some(id)))
                                .map(|record| record.key.clone());
                            column().gap_0().w_full().gap(px(3.))
                                .child(row().gap_0().relative().id(format!("trajectory-source-block-{index}"))
                                    .text_color(p.faint).text_size(px(11.)).line_height(px(16.)).font_family(crate::web::document_fonts::code_family(cx)).map(|mut element| { element.text_style().font_fallbacks = Some(crate::web::document_fonts::code_fallbacks(cx)); element })
                                    .child(format!("块 #{} {kind}", index + 1))
                                    .when_some(call, |header, key| {
                                        let keyboard_key = key.clone();
                                        header.role(gpui::Role::Button).aria_label(format!("打开块 #{} 的概要", index + 1))
                                            .track_focus(&self.trajectory_action_focus("hierarchy", &key, cx))
                                            .tab_index(0).cursor_pointer().child(" ›")
                                            .child(self.trajectory_accessible("hierarchy", key.clone(), format!("打开块 #{} 的概要", index + 1), None, None, false))
                                            .on_click(cx.listener(move |this, _, w, cx| this.open_trajectory_summary(&key, w, cx)))
                                            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, w, cx| {
                                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                                    this.open_trajectory_summary(&keyboard_key, w, cx);
                                                    w.prevent_default(); cx.stop_propagation();
                                                }
                                            }))
                                    }))
                                .child(div().w_full().text_size(px(12.)).line_height(px(19.)).font_family(crate::web::document_fonts::code_family(cx)).map(|mut element| { element.text_style().font_fallbacks = Some(crate::web::document_fonts::code_fallbacks(cx)); element }).child(source))
                        }))
                }
            }
            DetailTab::Source => payload(
                &record
                    .detail
                    .source
                    .as_ref()
                    .map(Value::to_string)
                    .unwrap_or_default(),
                "source",
                cx,
            ),
            DetailTab::Input if record.program().is_some() => {
                self.trajectory_program_input(record, cx)
            }
            DetailTab::Input => payload(&record.entry.input, "input", cx),
            DetailTab::Output => {
                let heading = |label| {
                    row()
                        .gap_0()
                        .w_full()
                        .h(px(24.))
                        .items_end()
                        .pl(px(14.))
                        .pb(px(3.))
                        .text_size(px(13.))
                        .line_height(px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.secondary)
                        .child(label)
                };
                let raw = self.trajectory_raw_result(record);
                let raw_section = column().gap_0().w_full().child(heading("原始结果"));
                let raw_section = if let Some(raw) = raw {
                    raw_section.child(self.trajectory_json(
                        record,
                        &raw.to_string(),
                        if raw.is_object() || raw.is_array() {
                            "raw-result"
                        } else {
                            "raw-result-scalar"
                        },
                        cx,
                    ))
                } else {
                    raw_section.child(
                        div()
                            .px(px(14.))
                            .pt(px(2.))
                            .pb(px(8.))
                            .text_size(px(13.))
                            .text_color(p.faint)
                            .child(if record.pending() {
                                "正在运行…"
                            } else {
                                "原始结果仅在实时工具调用中可用"
                            }),
                    )
                };
                column()
                    .gap_0()
                    .w_full()
                    .gap(px(2.))
                    .pt(px(8.))
                    .pb(px(12.))
                    .child(raw_section)
                    .child(column().gap_0().w_full().child(heading("渲染结果")).child(
                        if record.program().is_some() {
                            self.trajectory_program_output(record, cx)
                        } else {
                            self.trajectory_tool_output(record, cx)
                        },
                    ))
            }
            DetailTab::Schema => {
                if let Some(schema) = &record.detail.schema {
                    let parameters = schema
                        .get("parameters")
                        .or_else(|| schema.get("inputSchema"));
                    column()
                        .gap_0()
                        .w_full()
                        .p(px(14.))
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(schema["name"].as_str().unwrap_or(&record.name).to_owned()),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(p.secondary)
                                .child(schema["description"].as_str().unwrap_or("").to_owned()),
                        )
                        .child(self.trajectory_detail_payload(
                            record,
                            &parameters.unwrap_or(schema).to_string(),
                            "schema",
                            cx,
                        ))
                } else {
                    div()
                        .p(px(14.))
                        .text_size(px(13.))
                        .text_color(p.faint)
                        .child("Schema 不可用")
                }
            }
            DetailTab::Timing => self.trajectory_detail_timing(record, cx),
            DetailTab::Usage => self.trajectory_detail_usage(record, cx),
            DetailTab::Options => payload(
                &record
                    .options
                    .as_ref()
                    .map(Value::to_string)
                    .unwrap_or_default(),
                "options",
                cx,
            ),
            DetailTab::SystemPrompt => {
                let text = record.system_prompt.as_deref().unwrap_or(&record.output);
                if text.is_empty() {
                    div()
                        .px(px(14.))
                        .py(px(18.))
                        .text_size(px(13.))
                        .text_color(p.label_tertiary)
                        .child("本次请求没有系统提示词")
                } else {
                    div()
                        .w_full()
                        .p(px(14.))
                        .child(self.trajectory_detail_markdown(record, text, "system", cx))
                }
            }
            DetailTab::Tools => self.trajectory_json(
                record,
                record.tool_schemas.as_deref().unwrap_or("[]"),
                "prompt-tools",
                cx,
            ),
            DetailTab::Diff => self.trajectory_json(
                record,
                &serde_json::json!({
                    "before": record.detail.previous_prompt,
                    "after": record.detail.prompt,
                })
                .to_string(),
                "prompt-diff",
                cx,
            ),
        }
    }

    pub(super) fn render_trajectory_inspector(
        &self,
        record: &Record,
        narrow: bool,
        width: f32,
        composer_clearance: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = dsh(cx);
        let request_index = self
            .conversation
            .trajectory
            .requests
            .iter()
            .position(|request| request.key == record.key);
        let request = request_index.is_some();
        let tabs = record.detail_tabs(request);
        let active = if tabs.contains(&self.trajectory.detail_tab) {
            self.trajectory.detail_tab
        } else {
            tabs[0]
        };
        let content = self.trajectory_detail_content(record, request, active, cx);
        let last_tab = tabs.last().copied();
        let mut title = row().gap_0().flex_1().min_w_0().gap(px(8.));
        if let Some(index) = request_index {
            title = title
                .child(div().size(px(5.)).rounded_full().bg(p.secondary))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_family(crate::web::document_fonts::code_family(cx))
                        .map(|mut element| {
                            element.text_style().font_fallbacks =
                                Some(crate::web::document_fonts::code_fallbacks(cx));
                            element
                        })
                        .font_weight(FontWeight::MEDIUM)
                        .child(format!("请求 #{}", index + 1)),
                );
        } else {
            title = title.child(self.trajectory_kind_badge(record, false, cx));
        }
        let location = if record.kind == Kind::System && !request {
            record.name.clone()
        } else if request || record.kind == Kind::Compacted {
            format!("第 {} 轮", record.turn)
        } else {
            format!("第 {} 轮 · 第 {} 步", record.turn, record.step)
        };
        title = title.child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(11.))
                .font_family(crate::web::document_fonts::code_family(cx))
                .map(|mut element| {
                    element.text_style().font_fallbacks =
                        Some(crate::web::document_fonts::code_fallbacks(cx));
                    element
                })
                .text_color(p.faint)
                .child(location),
        );
        let tab_bar = row()
            .gap_0()
            .id("trajectory-detail-tabs")
            .role(gpui::Role::TabList)
            .aria_label("事件详情")
            .w_full()
            .min_w_0()
            .h(px(34.))
            .flex_none()
            .px(px(8.))
            .gap(px(1.))
            .overflow_x_scroll()
            .border_b(px(0.5))
            .border_color(p.border_l2)
            .children(tabs.into_iter().map(|tab| {
                let focus = self
                    .trajectory
                    .detail_tab_focus
                    .borrow_mut()
                    .entry(tab)
                    .or_insert_with(|| cx.focus_handle().tab_stop(true))
                    .clone();
                row()
                    .gap_0()
                    .id(format!("trajectory-detail-{tab:?}"))
                    .role(gpui::Role::Tab)
                    .aria_label(tab.label(record))
                    .aria_selected(active == tab)
                    .track_focus(&focus)
                    .tab_index(0)
                    .relative()
                    .h_full()
                    .flex_none()
                    .px(px(8.))
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus_visible(|tab| tab.border_color(p.state_business_primary))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .text_color(if active == tab {
                        p.state_business_primary
                    } else {
                        p.faint
                    })
                    .hover(|tab| tab.bg(p.hover).text_color(p.label_primary))
                    .child(tab.label(record))
                    .child(self.trajectory_accessible(
                        "detail-tab",
                        format!("{tab:?}"),
                        tab.label(record),
                        Some(active == tab),
                        None,
                        false,
                    ))
                    .when(active == tab, |tab| {
                        tab.child(
                            div()
                                .absolute()
                                .bottom(px(-1.))
                                .left(px(8.))
                                .right(px(8.))
                                .h(px(2.))
                                .bg(p.state_business_primary),
                        )
                    })
                    .on_click(cx.listener(move |this, _, w, cx| {
                        if let Some(focus) = this.trajectory.detail_tab_focus.borrow().get(&tab) {
                            focus.focus(w, cx);
                        }
                        this.activate_trajectory_detail(tab, cx);
                    }))
                    .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, w, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space")
                            && !event.keystroke.modifiers.modified()
                        {
                            this.activate_trajectory_detail(tab, cx);
                            w.prevent_default();
                            cx.stop_propagation();
                        }
                    }))
            }));
        column()
            .gap_0()
            .id("trajectory-inspector")
            .tab_group()
            .relative()
            .role(gpui::Role::Complementary)
            .aria_label("事件详情")
            .track_focus(&self.trajectory.inspector_focus)
            .tab_index(0)
            .capture_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, w, cx| {
                if event.keystroke.key == "tab"
                    && !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.alt
                    && !event.keystroke.modifiers.platform
                {
                    if event.keystroke.modifiers.shift {
                        w.focus_prev(cx);
                    } else if this.trajectory.inspector_focus.is_focused(w) {
                        if !super::json::focus_separator() {
                            this.trajectory.close_focus.focus(w, cx);
                        }
                    } else {
                        let last = last_tab
                            .and_then(|tab| {
                                this.trajectory.detail_tab_focus.borrow().get(&tab).cloned()
                            })
                            .is_some_and(|focus| focus.is_focused(w));
                        if !last || !super::json::focus(false) {
                            w.focus_next(cx);
                        }
                    }
                    w.prevent_default();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape"
                    && !event.keystroke.modifiers.modified()
                    && !w.default_prevented()
                {
                    this.close_trajectory_inspector(w, cx);
                }
            }))
            .when(narrow, |panel| {
                panel
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .shadow(crate::tokens::elevation_panel())
            })
            .w(px(width))
            .h_full()
            .min_h_0()
            .flex_none()
            .border_l(px(0.5))
            .border_color(if narrow { p.border_l3 } else { p.border_l2 })
            .bg(p.bg_layer_1)
            .child(self.trajectory_resize_handle(narrow, width))
            .child(
                row()
                    .gap_0()
                    .w_full()
                    .h(px(42.))
                    .flex_none()
                    .pl(px(12.))
                    .pr(px(8.))
                    .gap(px(8.))
                    .border_b(px(0.5))
                    .border_color(p.border_l2)
                    .child(title)
                    .child(
                        row()
                            .gap_0()
                            .id("trajectory-inspector-close")
                            .relative()
                            .role(gpui::Role::Button)
                            .aria_label("关闭详情")
                            .track_focus(&self.trajectory.close_focus)
                            .tab_index(0)
                            .size(px(28.))
                            .border_1()
                            .border_color(gpui::transparent_black())
                            .focus_visible(|button| button.border_color(p.state_business_primary))
                            .flex_none()
                            .justify_center()
                            .rounded(px(6.))
                            .text_size(px(18.))
                            .text_color(p.secondary)
                            .cursor_pointer()
                            .hover(|row| row.bg(p.hover))
                            .child("×")
                            .child(self.trajectory_accessible(
                                "close",
                                "",
                                "关闭详情",
                                None,
                                None,
                                false,
                            ))
                            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, w, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                    && !event.keystroke.modifiers.modified()
                                    && !w.default_prevented()
                                {
                                    this.close_trajectory_inspector(w, cx);
                                }
                            }))
                            .on_click(
                                cx.listener(|this, _, w, cx| {
                                    this.close_trajectory_inspector(w, cx)
                                }),
                            ),
                    ),
            )
            .child(tab_bar)
            .child(
                column()
                    .gap_0()
                    .id("trajectory-inspector-scroll")
                    .role(gpui::Role::TabPanel)
                    .aria_label(active.label(record))
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.trajectory.inspector_scroll)
                    .pb(px(composer_clearance
                        + if active == DetailTab::Overview {
                            12.
                        } else {
                            0.
                        }))
                    .child(content),
            )
    }
}
