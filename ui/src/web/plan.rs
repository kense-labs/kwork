//! Plan-mode controls and the Host's blocking plan review.

use super::{
    workspace::{TurnActionHint, strip_md_markers},
    *,
};
use gpui::svg;

const PLAN_MODE_HINT: &str = "计划模式已开启 — 点击关闭（/plan off）";

use crate::submitted_plan::PlanDocument;

pub(super) enum PlanPreview {
    Loading,
    Ready(PlanDocument),
    Missing,
    Failed(String),
}

impl PlanPreview {
    pub(super) fn title(&self) -> Option<&str> {
        match self {
            Self::Ready(plan) => Some(&plan.title),
            _ => None,
        }
    }
}

pub(super) struct PlanReview {
    id: String,
    call_id: Option<String>,
    question: String,
    document: PlanDocument,
    approval_description: Option<String>,
    preview: Option<u64>,
    auto_opened: bool,
}

impl PlanReview {
    fn from_update(id: &str, update: &Value) -> Option<Self> {
        let markdown = update["markdown"].as_str()?;
        let blocks = parse_markdown(markdown);
        let title = match blocks.first() {
            Some(Block::Heading(_, text)) => strip_md_markers(text),
            _ => markdown
                .trim()
                .lines()
                .next()
                .map(|line| strip_md_markers(line.trim_start_matches('#').trim()))
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| "计划".into()),
        };
        let call_id = update["callId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(str::to_owned);
        let document = if call_id.is_some() {
            PlanDocument::from_logged_markdown(markdown.to_owned())
        } else {
            None
        }
        .unwrap_or_else(|| PlanDocument {
            title: title.clone(),
            markdown: markdown.to_owned(),
        });
        Some(Self {
            id: id.to_owned(),
            call_id,
            question: update["question"].as_str()?.to_owned(),
            document,
            approval_description: update["approvalDescription"].as_str().map(str::to_owned),
            preview: None,
            auto_opened: false,
        })
    }
}

impl DshApp {
    pub(super) fn handle_plan_review(
        &mut self,
        session: &str,
        update: &Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = update["requestId"].as_str() else {
            return;
        };
        let Some(conversation) = self.conversation_mut(session) else {
            return;
        };
        let reviews = &mut conversation.plan_reviews;
        match update["phase"].as_str() {
            Some("open") if !reviews.iter().any(|review| review.id == id) => {
                if let Some(review) = PlanReview::from_update(id, update) {
                    reviews.push(review);
                }
            }
            Some("closed") => reviews.retain(|review| review.id != id),
            _ => return,
        }
        if self.active_session.as_deref() == Some(session)
            && self.page == WorkspacePage::Conversation
        {
            if self.conversation.plan_reviews.is_empty()
                && self.conversation.approvals.is_empty()
                && update["phase"] == "closed"
                && self.focused_right_pane(w, cx).is_none()
            {
                self.focus_composer(w, cx);
            }
            self.maybe_follow_tail();
        }
    }

    pub(super) fn auto_open_plan_preview(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if !self.show_welcome
            && self.page == WorkspacePage::Conversation
            && self.conversation.approvals.is_empty()
            && self
                .conversation
                .plan_reviews
                .last()
                .is_some_and(|review| !review.auto_opened)
        {
            self.open_plan_preview(false, w, cx);
        }
    }

    fn open_plan_preview(&mut self, focus: bool, w: &mut Window, cx: &mut Context<Self>) {
        let Some(review) = self.conversation.plan_reviews.last_mut() else {
            return;
        };
        review.auto_opened = true;
        let call_id = review.call_id.clone();
        let document = review.document.clone();
        let temporary_key = review.preview;
        let key = self.mount_plan_preview(call_id.as_deref(), temporary_key, document);
        self.conversation.plan_reviews.last_mut().unwrap().preview = Some(key);
        let layout = &mut self.sidebar.surface.layout;
        let tab = RightPanelTab::Plan(key);
        let pane = layout.focused();
        if focus {
            self.open_right_tab(pane, tab, w, cx);
        } else {
            layout.open(pane, tab);
            self.sidebar.surface.open |= !layout.focused().is_floating();
            cx.notify();
        }
    }

    fn mount_plan_preview(
        &mut self,
        call_id: Option<&str>,
        temporary_key: Option<u64>,
        document: PlanDocument,
    ) -> u64 {
        let saved = call_id.and_then(|call_id| {
            self.sidebar
                .surface
                .plans
                .iter()
                .find_map(|(&key, stored)| (stored == call_id).then_some(key))
        });
        let key = saved
            .or_else(|| temporary_key.filter(|key| self.sidebar.plans.contains_key(key)))
            .unwrap_or_else(|| {
                self.next_file_request += 1;
                let key = (self.next_file_request..)
                    .find(|id| {
                        !self.sidebar.surface.plans.contains_key(id)
                            && !self.sidebar.plans.contains_key(id)
                    })
                    .expect("there are unclaimed plan keys");
                self.next_file_request = key;
                key
            });
        if let Some(call_id) = call_id {
            self.sidebar
                .surface
                .plans
                .entry(key)
                .or_insert_with(|| call_id.to_owned());
        }
        self.sidebar.plans.insert(key, PlanPreview::Ready(document));
        key
    }

    pub(super) fn open_submitted_plan(
        &mut self,
        call_id: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.conversation.entries.iter().find(|entry| {
            entry.role == EntryRole::Plan && entry.external_id.as_deref() == Some(call_id)
        }) else {
            return;
        };
        let document = PlanDocument {
            title: entry.title.clone(),
            markdown: entry.text.clone(),
        };
        let key = self.mount_plan_preview(Some(call_id), None, document);
        self.open_right_tab(
            self.sidebar.surface.layout.focused(),
            RightPanelTab::Plan(key),
            w,
            cx,
        );
    }

    pub(super) fn sync_plan_previews(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.active_session.clone() else {
            return;
        };
        let reads: Vec<_> = self
            .sidebar
            .surface
            .layout
            .all_panes()
            .filter(|pane| self.dock_pane_on_screen(pane.id, cx))
            .flat_map(|pane| pane.tabs().iter())
            .filter_map(|tab| {
                let RightPanelTab::Plan(key) = *tab else {
                    return None;
                };
                if self.sidebar.plans.contains_key(&key) {
                    return None;
                }
                Some((key, self.sidebar.surface.plans.get(&key)?.clone()))
            })
            .collect();
        if reads.is_empty() {
            return;
        }
        for (key, _) in &reads {
            self.sidebar.plans.insert(*key, PlanPreview::Loading);
        }
        let cwd = self.sidebar.workspace.clone();
        cx.spawn(async move |this, cx| {
            let ids: Vec<_> = reads.iter().map(|(_, id)| id).collect();
            let mut result = bridge::invoke(
                "read_session_plans",
                json!({"sessionId": session, "cwd": cwd, "callIds": ids}),
            )
            .await
            .and_then(|value| {
                serde_json::from_value::<HashMap<String, PlanDocument>>(value)
                    .map_err(|error| error.to_string())
            });
            let _ = this.update(cx, |app, cx| {
                let Some(sidebar) = app.sidebar_for_session(&session) else {
                    return;
                };
                for (key, call_id) in reads {
                    if sidebar.surface.plans.get(&key) == Some(&call_id)
                        && let Some(preview @ PlanPreview::Loading) = sidebar.plans.get_mut(&key)
                    {
                        *preview = match &mut result {
                            Ok(plans) => plans
                                .remove(&call_id)
                                .map_or(PlanPreview::Missing, PlanPreview::Ready),
                            Err(error) => PlanPreview::Failed(error.clone()),
                        };
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn plan_review_preview_action(
        &mut self,
        value: &Value,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .conversation
            .plan_reviews
            .last()
            .is_some_and(|review| value["requestId"].as_str() == Some(review.id.as_str()))
        {
            self.open_plan_preview(true, w, cx);
        }
    }

    fn exit_plan_mode(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.active_session.clone() else {
            return;
        };
        if !self.connected()
            || !self.conversation.acp_open
            || self.conversation.plan_mode_busy
            || self.conversation.command_busy
        {
            return;
        }
        self.conversation.plan_mode_busy = true;
        self.conversation.plan_mode_error = None;
        cx.notify();
        cx.spawn_in(w, async move |this, cx| {
            let result =
                bridge::invoke("plan_mode", json!({"sessionId": session, "active": false})).await;
            let _ = this.update_in(cx, |this, _, cx| {
                let Some(conversation) = this.conversation_mut(&session) else {
                    return;
                };
                if !std::mem::take(&mut conversation.plan_mode_busy) || !conversation.acp_open {
                    return;
                }
                match result {
                    Ok(value) => {
                        conversation.plan_active = value["active"].as_bool();
                    }
                    Err(error) => conversation.plan_mode_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn plan_chip(&self, cx: &mut Context<Self>) -> Option<gpui::Div> {
        if self.conversation.plan_active != Some(true) {
            return None;
        }
        let p = dsh(cx);
        let locked =
            !self.connected() || self.conversation.plan_mode_busy || self.conversation.command_busy;
        let group = "plan-mode-chip";
        let chip = row()
            .id("plan-mode-chip")
            .role(gpui::Role::Button)
            .aria_label("计划模式已开启，按下关闭")
            .tab_index(0)
            .tab_stop(!locked)
            .group(group)
            .flex_none()
            .h(px(28.))
            .min_w(px(34.))
            .px(px(8.))
            .gap(px(4.))
            .rounded(px(8.))
            .bg(p.state_business_tertiary)
            .text_color(p.state_business_primary)
            .text_size(px(13.))
            .line_height(px(20.))
            .font_weight(FontWeight::MEDIUM)
            .tooltip(|_, cx| cx.new(|_| TurnActionHint(PLAN_MODE_HINT.into())).into())
            .child(
                div()
                    .relative()
                    .flex_none()
                    .size(px(14.))
                    .child(
                        plan_icon(
                            include_bytes!("../../assets/plan-mode.svg"),
                            p.state_business_primary,
                        )
                        .absolute()
                        .inset_0()
                        .when(!locked, |icon| {
                            icon.group_hover(group, |style| style.opacity(0.))
                        }),
                    )
                    .when(!locked, |glyph| {
                        glyph.child(
                            plan_icon(
                                include_bytes!("../../assets/plan-mode-close.svg"),
                                p.state_business_primary,
                            )
                            .absolute()
                            .inset_0()
                            .opacity(0.)
                            .group_hover(group, |style| style.opacity(1.)),
                        )
                    }),
            )
            .child("计划");
        let chip = if locked {
            chip.opacity(0.6)
        } else {
            dock_action_button(
                chip.cursor_pointer().hover(move |style| {
                    style.bg(p
                        .state_business_tertiary
                        .blend(p.state_business_primary.opacity(0.06)))
                }),
                cx.listener(|this, _, w, cx| this.exit_plan_mode(w, cx)),
            )
        };
        Some(row().flex_none().gap(px(6.)).child(chip).children(
            self.conversation.plan_mode_error.as_ref().map(|error| {
                let error = gpui::SharedString::from(error.clone());
                div()
                    .id("plan-mode-error")
                    .role(gpui::Role::Status)
                    .text_color(p.state_error_primary)
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .tooltip(move |_, cx| cx.new(|_| TurnActionHint(error.clone())).into())
                    .child("退出计划模式失败")
            }),
        ))
    }

    pub(super) fn render_plan_review(&self, width: f32, cx: &Context<Self>) -> Option<gpui::Div> {
        let review = self.conversation.plan_reviews.last()?;
        let session = self.active_session.as_deref()?;
        let request = &review.id;
        let mut approve = json!({"label":"approve"});
        if let Some(description) = &review.approval_description {
            approve["description"] = json!(description);
        }
        let payload = json!({
            "kind":"plan", "requestId":request, "canHide":false, "liveKeys":[],
            "questions":[{
                "id":request, "question":review.question, "detail":review.document.markdown,
                "options":[approve], "intent":{"kind":"plan-review","approve":"approve"},
            }],
            "snapshot":{"channel":"waterfall","closed":false,"waitState":"counting","state":"open"}
        });
        Some(super::chat_markdown::interaction(
            session,
            request,
            width,
            &payload,
            self.trajectory_controls_hidden(),
            cx,
        ))
    }

    pub(super) fn render_plan_document(
        &self,
        pane: DockPaneId,
        key: u64,
        w: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let call_id = self.sidebar.surface.plans.get(&key);
        let (status, document, failure) = match self.sidebar.plans.get(&key) {
            Some(PlanPreview::Ready(plan)) => ("ready", Some(plan), None),
            Some(PlanPreview::Missing) => (
                "failed",
                None,
                Some(json!({"code": "plan/not-found", "message": ""})),
            ),
            Some(PlanPreview::Failed(error)) => (
                "failed",
                None,
                Some(json!({"code": "plan/read-failed", "message": error})),
            ),
            _ if call_id.is_some() => ("loading", None, None),
            _ => ("none", None, None),
        };
        let session = self.active_session.as_deref().unwrap_or("");
        let source = json!({
            "session": session,
            "key": key.to_string(),
            "callId": call_id,
            "document": document,
            "status": status,
            "failure": failure,
        });
        div().relative().size_full().child(self.plan_document_frame(
            pane,
            format!("{session}:{key}"),
            source.to_string(),
            w,
            cx,
        ))
    }
}

fn plan_icon(artwork: &'static [u8], color: gpui::Hsla) -> gpui::Svg {
    svg()
        .data(artwork)
        .size(px(14.))
        .flex_none()
        .text_color(color)
}
