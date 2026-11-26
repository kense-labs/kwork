//! Activity from Kar's persisted session list and the currently attached live feed.
use gpui::{prelude::*, *};
use kar::{RequestKind, TaskStatus, chat::TurnStatus};
use kui::primitives::pro_icons::{ProIcon, pro_icon};
use uuid::Uuid;

use crate::{
    remote_link::now_epoch_ms,
    types::{format_relative_age, runtime_display_name},
    ui_motion::Tactile,
    ui_primitives::{FS_META, FS_UI, StudioPalette, UiLocale},
    workbench::{InboxScopeFilter as Scope, KarWorkbench, MainStageView},
};

struct Activity {
    key: String,
    chat: Option<Uuid>,
    scope: Scope,
    title: String,
    detail: String,
    status: &'static str,
}

impl KarWorkbench {
    fn inbox_activity(&self) -> Vec<Activity> {
        let loc = self.ui_locale;
        let mut rows = Vec::new();
        let current = self.current_chat_id();
        let live = &self.projected;
        let agent = runtime_display_name(self.active_or_selected_runtime_id()).to_string();

        for request in &live.pending_requests {
            let status = match request.kind {
                RequestKind::Question => loc.pick("等待答复", "Question"),
                RequestKind::Permission => loc.pick("等待授权", "Permission"),
                _ => loc.pick("等待处理", "Pending request"),
            };
            let title = ["question", "reason", "command"]
                .iter()
                .find_map(|key| request.body[*key].as_str().filter(|s| !s.trim().is_empty()))
                .unwrap_or(&request.method);
            rows.push(Activity {
                key: format!("request-{:?}", request.cursor),
                chat: current,
                scope: Scope::Waiting,
                title: title.to_owned(),
                detail: agent.clone(),
                status,
            });
        }
        for task in &live.tasks {
            let (scope, status) = match task.status {
                TaskStatus::Running => (Scope::Running, loc.pick("运行中", "Running")),
                TaskStatus::Pending => (Scope::Running, loc.pick("排队中", "Queued")),
                TaskStatus::Paused => (Scope::Waiting, loc.pick("已暂停", "Paused")),
                _ => continue,
            };
            rows.push(Activity {
                key: format!("task-{}", task.id),
                chat: current,
                scope,
                status,
                title: task
                    .summary
                    .clone()
                    .or_else(|| task.details.description.clone())
                    .unwrap_or_else(|| task.id.to_string()),
                detail: agent.clone(),
            });
        }
        for bg in self
            .client_reducer
            .background_commands
            .values()
            .filter(|bg| live.turns.iter().any(|t| t.id == bg.turn_id))
        {
            let (scope, status) = if !bg.completed {
                (Scope::Running, loc.pick("后台执行", "In background"))
            } else if bg.exit_code.is_some_and(|c| c != 0) {
                (Scope::Waiting, loc.pick("命令失败", "Command failed"))
            } else {
                (Scope::Done, loc.pick("已结束", "Finished"))
            };
            rows.push(Activity {
                key: format!("command-{}", bg.tool_id),
                chat: current,
                scope,
                status,
                title: bg.command_summary.clone(),
                detail: agent.clone(),
            });
        }
        for entry in &self.chat_list {
            if self
                .deferred_delete_chat
                .as_ref()
                .is_some_and(|(id, _, _)| *id == entry.info.id)
            {
                continue;
            }
            let active = current == Some(entry.info.id);
            if active && !live.pending_requests.is_empty() {
                continue;
            }
            let last = if active {
                live.turns.last().map(|t| t.status).or(entry.last_status)
            } else {
                entry.last_status
            };
            let Some((scope, status)) = last.map(|status| match status {
                TurnStatus::Running => (Scope::Running, loc.pick("执行中", "Running")),
                TurnStatus::Failed => (Scope::Waiting, loc.pick("执行失败", "Failed")),
                TurnStatus::Interrupted => (Scope::Waiting, loc.pick("已中断", "Interrupted")),
                TurnStatus::Aborted => (Scope::Done, loc.pick("已停止", "Stopped")),
                TurnStatus::Completed => (Scope::Done, loc.pick("已完成", "Completed")),
            }) else {
                continue;
            };
            let project = entry
                .info
                .workspace
                .file_name()
                .unwrap_or(entry.info.workspace.as_os_str())
                .to_string_lossy();
            let age = format_relative_age(entry.last_activity_ms, now_epoch_ms());
            rows.push(Activity {
                key: entry.info.id.to_string(),
                chat: Some(entry.info.id),
                scope,
                title: entry.title.clone(),
                detail: format!(
                    "{} · {project} · {age}",
                    runtime_display_name(&entry.info.runtime)
                ),
                status,
            });
        }
        rows
    }

    pub(crate) fn render_tasks_inbox_screen(
        &mut self,
        avail_w: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let s = StudioPalette::get(cx);
        let toolbar = self.workspace_toolbar(loc.pick("收件箱", "Inbox"), None, &s, window, cx);
        let query = self.inbox_search.read(cx).value().trim().to_lowercase();
        let rows = self.inbox_activity();
        let mut content = crate::ui_primitives::workspace_page()
            .px(px(if avail_w < 600. { 16. } else { 28. }))
            .pt(px(16.))
            .gap(px(20.));

        let scopes = [
            (Scope::All, loc.pick("全部", "All")),
            (Scope::Waiting, loc.pick("待处理", "Needs attention")),
            (Scope::Running, loc.pick("进行中", "In progress")),
            (Scope::Done, loc.pick("已结束", "Finished")),
        ];
        let mut filters = div().flex().items_center().flex_wrap().gap(px(2.));
        for (i, (scope, label)) in scopes.into_iter().enumerate() {
            let active = self.inbox_scope == scope;
            filters = filters.child(
                Tactile::ghost(("inbox-filter", i), &s)
                    .lit(active)
                    .h(px(32.))
                    .px(px(8.))
                    .text_size(px(FS_META))
                    .child(label)
                    .on_press(cx, move |this, _, cx| {
                        this.inbox_scope = scope;
                        cx.notify();
                    }),
            );
        }
        let controls = div()
            .w_full()
            .flex()
            .gap(px(12.))
            .when(avail_w < 720., |d| d.flex_col())
            .child(self.collection_search_field(self.inbox_search.clone(), &s, cx))
            .child(filters);
        content = content.child(controls);
        let mut has_items = false;
        for (scope, label) in scopes.into_iter().skip(1) {
            if self.inbox_scope != Scope::All && self.inbox_scope != scope {
                continue;
            }
            let group = rows
                .iter()
                .filter(|row| {
                    row.scope == scope
                        && (query.is_empty()
                            || row.title.to_lowercase().contains(&query)
                            || row.detail.to_lowercase().contains(&query)
                            || row.status.to_lowercase().contains(&query))
                })
                .collect::<Vec<_>>();
            if group.is_empty() {
                continue;
            }
            has_items = true;
            let mut list = div()
                .flex()
                .flex_col()
                .child(crate::ui_primitives::section_heading(
                    label,
                    Some(group.len()),
                    &s,
                ));
            for row in group {
                let chat = row.chat;
                let ink = match scope {
                    Scope::Waiting => s.pro.amber_ink,
                    Scope::Running => s.ink,
                    _ => s.muted,
                };
                list = list.child(
                    Tactile::ghost(SharedString::from(format!("activity-{}", row.key)), &s)
                        .w_full()
                        .min_h(px(54.))
                        .px(px(8.))
                        .gap(px(14.))
                        .child(div().size(px(7.)).flex_none().rounded_full().bg(ink))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(row.title.clone()),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(FS_META))
                                        .text_color(s.muted)
                                        .child(row.detail.clone()),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(FS_META))
                                .text_color(ink)
                                .child(row.status),
                        )
                        .child(pro_icon(ProIcon::ChevronRight, 14., s.muted))
                        .on_press(cx, move |this, win, cx| {
                            if let Some(id) = chat.filter(|id| Some(*id) != this.current_chat_id())
                            {
                                this.open_chat(id, win, cx);
                            } else {
                                this.main_stage_view = MainStageView::Conversation;
                                cx.notify();
                            }
                        }),
                );
            }
            content = content.child(list);
        }
        if !has_items {
            let (title, detail) = if query.is_empty() {
                empty_copy(self.inbox_scope, loc)
            } else {
                (
                    loc.pick("没有匹配的活动", "No matching activity"),
                    loc.pick(
                        "试试其他搜索词或筛选条件。",
                        "Try a different search or filter.",
                    ),
                )
            };
            content = content.min_h_full().child(
                div()
                    .flex_1()
                    .min_h(px(150.))
                    .justify_center()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .size(px(38.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(s.line)
                            .bg(s.card)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(pro_icon(ProIcon::Inbox, 18., s.muted)),
                    )
                    .child(div().text_size(px(18.)).text_color(s.ink).child(title))
                    .child(
                        div()
                            .text_center()
                            .text_size(px(FS_UI))
                            .text_color(s.muted)
                            .child(detail),
                    ),
            );
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(
                div()
                    .id("inbox-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(content),
            )
            .into_any_element()
    }
}

fn empty_copy(scope: Scope, loc: UiLocale) -> (&'static str, &'static str) {
    match scope {
        Scope::All => (
            loc.pick("收件箱是空的", "Your inbox is empty"),
            loc.pick(
                "开始对话后，在这里跟进请求和执行进度。",
                "Start a chat to see requests and session activity here.",
            ),
        ),
        Scope::Waiting => (
            loc.pick("没有待处理事项", "Nothing needs attention"),
            loc.pick(
                "授权请求、待答问题和异常会话会出现在这里。",
                "Permissions, questions and interrupted sessions appear here.",
            ),
        ),
        Scope::Running => (
            loc.pick("没有正在执行的任务", "Nothing is running"),
            loc.pick(
                "执行中的对话和后台任务会出现在这里。",
                "Active sessions and background work appear here.",
            ),
        ),
        Scope::Done => (
            loc.pick("还没有已结束的事项", "No finished activity"),
            loc.pick(
                "已完成和已停止的会话会出现在这里。",
                "Completed and stopped sessions appear here.",
            ),
        ),
    }
}
