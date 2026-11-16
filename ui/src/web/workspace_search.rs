//! Expanding workspace search with coordinated width and opacity motion.

use super::*;
use gpui::svg;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[wasm_bindgen(raw_module = "../session-metadata-search.js")]
extern "C" {
    type SessionMetadataIndex;

    #[wasm_bindgen(constructor)]
    fn new() -> SessionMetadataIndex;

    #[wasm_bindgen(method)]
    fn update(this: &SessionMetadataIndex, entries: JsValue, newest_first: js_sys::Uint32Array);

    #[wasm_bindgen(method)]
    fn search(this: &SessionMetadataIndex, query: &str) -> js_sys::Uint32Array;
}

const SEARCH_LIMIT: usize = 20;
const SEARCH_PENDING: &str = "正在搜索会话历史…";
const SEARCH_EMPTY: &str = "无匹配会话";
const SEARCH_REFINE: &str = "仅显示前 20 条结果，请缩小搜索范围。";
pub(super) const ARCHIVED_RESULT: &str = "已归档对话暂时无法查看，请取消归档后查看";

#[derive(Default)]
pub(super) struct WorkspaceSearch {
    pub(super) query: String,
    pub(super) pending: bool,
    pub(super) scroll: ScrollHandle,
    pub(super) durable_blanks: std::collections::HashSet<String>,
    pub(super) reveal: Option<String>,
    generation: u64,
    in_flight: bool,
    metadata: MetadataSearch,
    content: ContentResults,
}

#[derive(PartialEq, Eq, Serialize)]
struct SessionMetadata {
    id: String,
    title: String,
    workspace: String,
    #[serde(skip)]
    recency: u64,
}

#[derive(Default)]
struct MetadataSearch {
    index: Option<SessionMetadataIndex>,
    entries: Vec<SessionMetadata>,
    query: String,
    ranked_positions: Vec<u32>,
}

impl MetadataSearch {
    fn refresh(&mut self, entries: Vec<SessionMetadata>, query: &str) {
        let changed = self.entries != entries;
        if !changed && self.query == query {
            return;
        }
        let index = self.index.get_or_insert_with(SessionMetadataIndex::new);
        if changed {
            let mut newest_first = (0..entries.len() as u32).collect::<Vec<_>>();
            newest_first.sort_unstable_by(|&a, &b| {
                let a = &entries[a as usize];
                let b = &entries[b as usize];
                b.recency
                    .cmp(&a.recency)
                    .then_with(|| a.id.encode_utf16().cmp(b.id.encode_utf16()))
            });
            index.update(
                serde_wasm_bindgen::to_value(&entries).expect("serialize session search metadata"),
                js_sys::Uint32Array::from(newest_first.as_slice()),
            );
            self.entries = entries;
        }
        self.query = query.to_owned();
        self.ranked_positions = index.search(query).to_vec();
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContentResults {
    items: Vec<ContentHit>,
    has_more: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContentHit {
    session_id: String,
    snippet: String,
}

struct SearchResult {
    session: SessionSummary,
    workspace: String,
    snippet: Option<String>,
}

fn sanitize_search(value: &str) -> String {
    let mut capacity = 500_usize;
    value
        .chars()
        .filter(|character| *character != '\0')
        .take_while(|character| {
            let Some(remaining) = capacity.checked_sub(character.len_utf16()) else {
                return false;
            };
            capacity = remaining;
            true
        })
        .collect()
}

fn trim_search_query(query: &str) -> &str {
    // Match String.trim() on the sidebar and in the ACP search controller.
    query.trim_matches(|c: char| c == '\u{feff}' || (c.is_whitespace() && c != '\u{0085}'))
}

impl DshApp {
    pub(super) fn session_visible_in_sidebar(&self, session: &SessionSummary) -> bool {
        match self.workspace_archived_filter {
            WorkspaceArchivedFilter::Hide => !self.archived_sessions.contains(&session.id),
            WorkspaceArchivedFilter::Only => self.archived_sessions.contains(&session.id),
            WorkspaceArchivedFilter::Show => true,
        }
    }

    fn workspace_search_results(&mut self) -> (Vec<SearchResult>, bool) {
        let candidates = self
            .sessions
            .iter()
            .filter(|session| {
                !session.blank
                    && !self.workspace_search.durable_blanks.contains(&session.id)
                    && self.session_visible_in_sidebar(session)
            })
            .collect::<Vec<_>>();
        let entries = candidates
            .iter()
            .map(|session| {
                let title = if session.title.is_some() || self.session_title_name(session).is_some()
                {
                    self.session_display_title(session)
                } else {
                    String::new()
                };
                SessionMetadata {
                    id: session.id.clone(),
                    title,
                    workspace: self.search_workspace_label(session),
                    recency: self.session_recency(session),
                }
            })
            .collect();
        let search = &mut self.workspace_search;
        search.metadata.refresh(entries, &search.query);

        let mut snippets = HashMap::new();
        let mut ranked_content = Vec::new();
        if !search.content.items.is_empty() {
            let positions = candidates
                .iter()
                .enumerate()
                .map(|(index, session)| (session.id.as_str(), index))
                .collect::<HashMap<_, _>>();
            for hit in &search.content.items {
                if let Some(&position) = positions.get(hit.session_id.as_str())
                    && let std::collections::hash_map::Entry::Vacant(entry) =
                        snippets.entry(position)
                {
                    entry.insert(&hit.snippet);
                    ranked_content.push(position);
                }
            }
        }
        let mut included = HashSet::new();
        let mut ordered = search
            .metadata
            .ranked_positions
            .iter()
            .map(|&position| position as usize)
            .chain(ranked_content)
            .filter(|&position| included.insert(position))
            .peekable();
        let rows = ordered
            .by_ref()
            .take(SEARCH_LIMIT)
            .map(|position| SearchResult {
                session: candidates[position].clone(),
                workspace: search.metadata.entries[position].workspace.clone(),
                snippet: snippets.get(&position).map(|&snippet| snippet.clone()),
            })
            .collect();
        let has_more = ordered.peek().is_some() || search.content.has_more;
        (rows, has_more)
    }

    pub(super) fn render_search_results(
        &mut self,
        w: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let (results, has_more) = self.workspace_search_results();
        let pending = self.workspace_search.pending;
        let empty = results.is_empty();
        self.workspace_accessibility.search_status = if pending {
            Some(SEARCH_PENDING)
        } else if empty {
            Some(SEARCH_EMPTY)
        } else if has_more {
            Some(SEARCH_REFINE)
        } else {
            None
        };
        let mut list = column().w_full().gap(px(2.));
        for result in results {
            let workspace = if result.workspace.is_empty() {
                "未分组".to_owned()
            } else {
                result.workspace
            };
            list = list.child(self.render_session_row(
                result.session,
                0,
                0,
                Some((workspace, result.snippet)),
                w,
                cx,
            ));
        }
        let p = dsh(cx);
        let status = |label: &'static str| {
            div()
                .px(px(12.))
                .py(px(10.))
                .text_size(px(12.))
                .line_height(px(18.))
                .text_color(p.label_tertiary)
                .child(label)
        };
        if pending {
            let skeleton = |width: f32, height: f32| {
                div().w(px(width)).h(px(height)).rounded_full().bg(p.hover)
            };
            let mut loading = column()
                .id("workspace-search-pending")
                .role(gpui::Role::Status)
                .aria_label(SEARCH_PENDING);
            for _ in 0..if empty { 2 } else { 1 } {
                loading = loading.child(
                    row()
                        .h(px(48.))
                        .items_start()
                        .gap(px(6.))
                        .px(px(9.))
                        .pt(px(6.))
                        .child(skeleton(16., 16.))
                        .child(
                            column()
                                .gap(px(6.))
                                .child(skeleton(92., 14.))
                                .child(skeleton(146., 12.)),
                        ),
                );
            }
            list = list.child(loading);
        } else if empty {
            list = list.child(status(SEARCH_EMPTY));
        }
        list.when(has_more, |list| list.child(status(SEARCH_REFINE)))
    }

    pub(super) fn render_search_result_metadata(
        &self,
        workspace: String,
        snippet: Option<String>,
        archived: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let p = dsh(cx);
        let muted = if archived {
            p.label_caption
        } else {
            p.label_tertiary
        };
        let has_snippet = snippet.is_some();
        row()
            .min_w_0()
            .gap(px(6.))
            .text_size(px(12.))
            .line_height(px(17.))
            .child(
                div()
                    .truncate()
                    .text_color(muted)
                    .when(has_snippet, |label| {
                        label.flex_none().max_w(gpui::relative(0.4))
                    })
                    .child(workspace),
            )
            .when_some(snippet, |meta, snippet| {
                meta.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(if archived { muted } else { p.label_secondary })
                        .child(snippet),
                )
            })
    }

    fn search_workspace_label(&self, session: &SessionSummary) -> String {
        if let Some(path) = self.workspace_registry.session_path(&session.id) {
            return self.workspace_name(path);
        }
        if !self.workspace_registry.available()
            && let Some(alias) = self.workspace_aliases.get(&session.cwd)
        {
            return alias.clone();
        }
        session
            .cwd
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or(&session.cwd)
            .to_owned()
    }

    pub(super) fn sidebar_session_scroll(&self) -> &ScrollHandle {
        if self.workspace_search.query.is_empty() {
            &self.sessions_scroll
        } else {
            &self.workspace_search.scroll
        }
    }

    pub(super) fn scroll_sidebar_row_into_view(&self, bounds: gpui::Bounds<gpui::Pixels>) -> bool {
        let scroll = self.sidebar_session_scroll();
        let viewport = scroll.bounds();
        let before = scroll.offset();
        let mut offset = before;
        if bounds.top() < viewport.top() {
            offset.y += viewport.top() - bounds.top();
        } else if bounds.bottom() > viewport.bottom() {
            offset.y -= bounds.bottom() - viewport.bottom();
        }
        offset.y = offset.y.clamp(-scroll.max_offset().y, px(0.));
        if offset == before {
            return false;
        }
        scroll.set_offset(offset);
        true
    }

    pub(super) fn update_workspace_search(&mut self, w: &Window, cx: &mut Context<Self>) {
        if !self.search_expanded {
            return;
        }
        let value = self.search_editor.read(cx).value();
        let sanitized = sanitize_search(value);
        if value != sanitized {
            replace(&self.search_editor, &sanitized, w, cx);
        }
        self.set_workspace_search_query(trim_search_query(&sanitized), cx);
    }

    fn set_workspace_search_query(&mut self, query: &str, cx: &mut Context<Self>) {
        if self.workspace_search.query == query {
            return;
        }
        let search = &mut self.workspace_search;
        search.query = query.to_owned();
        // A reloaded webview can share the running ACP process. Seed its
        // sequence above the previous document's requests in JS-safe units.
        search.generation = (search.generation + 2).max((bridge::now_ms() as u64) * 1024);
        if !query.is_empty() {
            search.reveal = None;
        }
        let generation = search.generation;
        search.content = ContentResults::default();
        search.pending = !query.is_empty();
        search.scroll.set_offset(gpui::point(px(0.), px(0.)));
        if std::mem::take(&mut search.in_flight) {
            cx.spawn(async move |_, _| {
                let _ = bridge::invoke(
                    "search_session_history",
                    json!({"query": null, "generation": generation - 1}),
                )
                .await;
            })
            .detach();
        }
        if !query.is_empty() {
            let query = query.to_owned();
            cx.spawn(async move |this, cx| {
                gloo_timers::future::TimeoutFuture::new(250).await;
                let current = this
                    .update(cx, |this, _| {
                        let search = &mut this.workspace_search;
                        if search.generation != generation {
                            return false;
                        }
                        search.in_flight = true;
                        true
                    })
                    .unwrap_or(false);
                if !current {
                    return;
                }
                let result = bridge::invoke(
                    "search_session_history",
                    json!({"query": query, "generation": generation}),
                )
                .await;
                let _ = this.update(cx, |this, cx| {
                    let search = &mut this.workspace_search;
                    if search.generation == generation {
                        search.in_flight = false;
                        search.pending = false;
                        search.content = result
                            .ok()
                            .and_then(|reply| serde_json::from_value(reply).ok())
                            .unwrap_or_default();
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    pub(super) fn render_workspace_search(
        &mut self,
        content_width: f32,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let p = dsh(cx);
        self.search_slide
            .duration(if self.search_expanded { 1. } else { 0. }, 0.18, cx);
        let expanded = self.search_slide.get(w, cx).clamp(0., 1.);
        let collapsed = 1. - expanded;
        let fade_in = (expanded * 1.5).min(1.);
        let fade_out = (collapsed * 1.5).min(1.);
        let label = if self.workspace_group_by == WorkspaceGroupBy::Flat {
            "会话".to_string()
        } else {
            "工作区".to_string()
        };
        let label_width = 48.;
        let spacer_width = (content_width - label_width - 112.).max(0.) * collapsed;
        let search_width = 28. + (content_width - 36.).max(0.) * expanded;

        row()
            .w(px(content_width))
            .h(px(36.))
            .flex_none()
            .mt(px(10.))
            .pl(px(4. + 8. * collapsed))
            .pr(px(4.))
            .gap(px(4. * collapsed))
            .overflow_hidden()
            .child(
                div()
                    .w(px(label_width * collapsed))
                    .flex_none()
                    .overflow_hidden()
                    .opacity(fade_out)
                    .child(
                        div()
                            .w(px(label_width))
                            .truncate()
                            .text_size(px(12.))
                            .line_height(px(20.))
                            .text_color(p.faint)
                            .child(label),
                    ),
            )
            .child(div().w(px(spacer_width)).flex_none())
            .child(
                row()
                    .id("session-search")
                    .w(px(search_width))
                    .h(px(28. + 4. * expanded))
                    .flex_none()
                    .overflow_hidden()
                    .rounded(px(8. + 4. * expanded))
                    .pr(px(4. * expanded))
                    .border(px(0.5))
                    .border_color(p.border_l4.opacity(expanded))
                    .when(self.search_expanded, |search| {
                        search.on_mouse_down_out(cx.listener(|this, _, w, cx| {
                            if this.workspace_search.query.is_empty() {
                                this.close_workspace_search(false, w, cx);
                            } else if this.search_editor.read(cx).focus.is_focused(w) {
                                w.blur(cx);
                            }
                        }))
                    })
                    .child(super::dock_action_button(
                        div()
                            .id("expand-session-search")
                            .role(gpui::Role::Button)
                            .aria_label("搜索会话")
                            .aria_expanded(self.search_expanded)
                            .track_focus(&self.workspace_search_focus)
                            .tab_index(0)
                            .tab_stop(true)
                            .w(px(28.))
                            .h(px(28. + 4. * expanded))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(8.))
                            .cursor_pointer()
                            .when(!self.search_expanded, |element| {
                                element.hover(|style| style.bg(p.hover))
                            })
                            .child(
                                svg()
                                    .data(include_bytes!("../../assets/workspace-search.svg"))
                                    .size(px(14. - 3. * expanded))
                                    .text_color(if self.search_expanded {
                                        p.label_caption
                                    } else {
                                        p.label_tertiary
                                    }),
                            ),
                        cx.listener(|this, _, w, cx| this.open_workspace_search(w, cx)),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(32.))
                            .ml(px(-2. * expanded))
                            .overflow_hidden()
                            .opacity(fade_in)
                            .child(self.search_editor.clone()),
                    )
                    .child(super::dock_action_button(
                        div()
                            .id("close-session-search")
                            .role(gpui::Role::Button)
                            .aria_label("清除搜索")
                            .tab_index(0)
                            .tab_stop(self.search_expanded)
                            .w(px(24. * expanded))
                            .h(px(24.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .overflow_hidden()
                            .rounded(px(8.))
                            .opacity(fade_in)
                            .cursor_pointer()
                            .hover(|style| style.bg(p.hover))
                            .when(expanded > 0., |button| {
                                button.child(
                                    svg()
                                        .data(include_bytes!("../../assets/dock-tab-close.svg"))
                                        .size(px(14.))
                                        .flex_none()
                                        .text_color(p.label_secondary),
                                )
                            }),
                        cx.listener(|this, _, w, cx| this.close_workspace_search(true, w, cx)),
                    )),
            )
            .child(
                row()
                    .w(px(60. * collapsed))
                    .h(px(28.))
                    .flex_none()
                    .gap(px(4.))
                    .overflow_hidden()
                    .opacity(fade_out)
                    .child(
                        div()
                            .relative()
                            .size(px(28.))
                            .flex_none()
                            .child(dock_action_button(
                                div()
                                    .id("workspace-view-options")
                                    .role(gpui::Role::Button)
                                    .aria_label("视图选项")
                                    .aria_expanded(self.show_workspace_view_options)
                                    .tab_index(0)
                                    .tab_stop(!self.search_expanded)
                                    .size_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .when(self.show_workspace_view_options, |element| {
                                        element.bg(p.hover)
                                    })
                                    .hover(|style| style.bg(p.hover))
                                    .child(
                                        svg()
                                            .data(include_bytes!("../../assets/view-options.svg"))
                                            .size(px(16.))
                                            .text_color(p.secondary),
                                    ),
                                cx.listener(|this, _, _, cx| {
                                    if this.search_expanded {
                                        return;
                                    }
                                    this.show_workspace_view_options =
                                        !this.show_workspace_view_options;
                                    this.show_workspace_menu = false;
                                    this.show_more_menu = false;
                                    this.composer_menu = None;
                                    cx.notify();
                                }),
                            ))
                            .when(self.show_workspace_view_options, |element| {
                                element.child(self.render_workspace_view_options(cx))
                            }),
                    )
                    .child(
                        div()
                            .id("add-workspace")
                            .role(gpui::Role::Button)
                            .aria_label("添加工作区")
                            .tab_index(0)
                            .tab_stop(!self.search_expanded)
                            .size(px(28.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(|style| style.bg(p.hover))
                            .child(
                                svg()
                                    .data(include_bytes!("../../assets/workspace-add.svg"))
                                    .size(px(16.))
                                    .text_color(p.secondary),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.search_expanded {
                                    return;
                                }
                                this.show_workspace_view_options = false;
                                this.show_workspace_menu = false;
                                this.composer_menu = None;
                                this.request_workspace_picker();
                                cx.notify();
                            })),
                    ),
            )
    }

    pub(super) fn open_workspace_search(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.rail_active(w) {
            self.set_left_sidebar_collapsed(false, w, cx);
        }
        self.search_expanded = true;
        self.show_workspace_view_options = false;
        self.show_workspace_menu = false;
        self.search_editor.update(cx, |editor, cx| {
            editor.disabled = false;
            editor.focus(w, cx);
        });
        cx.notify();
    }

    pub(super) fn close_workspace_search(
        &mut self,
        restore_focus: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.search_expanded {
            return;
        }
        self.search_expanded = false;
        self.set_workspace_search_query("", cx);
        self.search_editor.update(cx, |editor, cx| {
            if editor.focus.is_focused(w) {
                w.blur(cx);
            }
            editor.disabled = true;
            editor.set_value("", w, cx);
        });
        if restore_focus {
            w.focus(&self.workspace_search_focus, cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_search;

    #[test]
    fn search_limit_uses_utf16_units_without_splitting_emoji() {
        assert_eq!(sanitize_search("a\0b"), "ab");
        let prefix = "x".repeat(499);
        assert_eq!(sanitize_search(&format!("{prefix}🐋z")), prefix);
        assert_eq!(sanitize_search(&"🐋".repeat(251)), "🐋".repeat(250));
    }
}
