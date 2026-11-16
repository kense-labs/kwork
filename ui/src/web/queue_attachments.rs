//! Attachment previews for messages that the Host has yet to admit.

use super::*;
use crate::state::UserFile;
use serde::Serialize;

#[wasm_bindgen(module = "/composer-accessibility.js")]
extern "C" {
    fn update_attachment_accessibility(value: JsValue);
}

#[derive(Default)]
pub(super) struct AttachmentAccessibility {
    pub(super) rendered: Vec<AttachmentControl>,
    published: Option<AccessibleAttachments>,
}

#[derive(PartialEq, Eq, Serialize)]
struct AccessibleAttachments {
    session: String,
    drafts: Vec<AttachmentControl>,
    files: Vec<AttachmentControl>,
}

#[derive(PartialEq, Eq, Serialize)]
pub(super) struct AttachmentControl {
    id: String,
    label: String,
    size: String,
    #[serde(skip)]
    file: Option<UserFile>,
}

impl AttachmentControl {
    pub(super) fn download(entry: &str, index: usize, file: UserFile) -> Self {
        Self {
            id: format!("{entry}:{index}"),
            label: file_download_label(&file),
            size: format_bytes(file.bytes),
            file: Some(file),
        }
    }
}

fn file_download_label(file: &UserFile) -> String {
    format!("下载附件 {}", file.name)
}

fn download_user_file(file: &UserFile) {
    invoke(
        "download_user_attachment",
        json!({"attachmentId": file.attachment_id, "name": file.name, "bytes": file.bytes}),
    );
}

#[derive(Clone)]
pub(super) enum QueueAttachment {
    Image {
        reference: Option<(String, String)>,
        preview: Option<String>,
    },
    File {
        name: String,
        bytes: Option<u64>,
    },
}

impl QueueAttachment {
    pub(super) fn from_inbox(value: &Value) -> Option<Self> {
        match value["type"].as_str()? {
            "image" => Some(Self::Image {
                reference: Some((
                    value["attachmentId"].as_str()?.to_owned(),
                    value["mediaType"]
                        .as_str()
                        .unwrap_or("image/png")
                        .to_owned(),
                )),
                preview: None,
            }),
            "file" => Some(Self::File {
                name: value["name"].as_str()?.to_owned(),
                bytes: value["bytes"].as_u64(),
            }),
            _ => None,
        }
    }

    pub(super) fn from_draft(item: &DraftAttachment) -> Self {
        match item.kind {
            AttachmentKind::Image => Self::Image {
                reference: None,
                preview: item.preview.clone(),
            },
            AttachmentKind::File => Self::File {
                name: item.name.clone(),
                bytes: Some(item.size),
            },
        }
    }

    pub(super) fn label(&self) -> String {
        match self {
            Self::Image { .. } => "排队消息图片".into(),
            Self::File { name, bytes } => bytes.map_or_else(
                || format!("排队文件 {name}"),
                |size| format!("排队文件 {name} · {}", format_bytes(size)),
            ),
        }
    }
}

impl DshApp {
    pub(super) fn remove_draft_attachment(&mut self, id: &str, cx: &mut Context<Self>) {
        self.conversation
            .draft_attachments
            .retain(|item| item.id != id);
        cx.notify();
    }

    pub(super) fn publish_accessible_attachments(&mut self) {
        let files = std::mem::take(&mut self.attachment_accessibility.rendered);
        let visible = self.workspace_tree_available() && self.subagent_view.is_none();
        let next = self
            .active_session
            .as_ref()
            .filter(|_| visible)
            .map(|session| {
                let drafts = if self.composer_tools_available() {
                    self.conversation
                        .draft_attachments
                        .iter()
                        .map(|item| AttachmentControl {
                            id: item.id.clone(),
                            label: format!("移除附件 {}", item.name),
                            size: format_bytes(item.size),
                            file: None,
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                AccessibleAttachments {
                    session: session.clone(),
                    drafts,
                    files,
                }
            });
        if next != self.attachment_accessibility.published
            && let Ok(value) = serde_wasm_bindgen::to_value(&next)
        {
            self.attachment_accessibility.published = next;
            update_attachment_accessibility(value);
        }
    }

    pub(super) fn accessible_attachment_action(
        &mut self,
        action: &str,
        value: &str,
        cx: &mut Context<Self>,
    ) {
        let Ok(target) = serde_json::from_str::<Value>(value) else {
            return;
        };
        let Some(view) = &self.attachment_accessibility.published else {
            return;
        };
        if target["session"].as_str() != Some(view.session.as_str())
            || self.active_session.as_deref() != Some(view.session.as_str())
            || !self.workspace_tree_available()
            || self.subagent_view.is_some()
        {
            return;
        }
        let Some(id) = target["id"].as_str() else {
            return;
        };
        match action {
            "attachment-remove"
                if self.composer_tools_available()
                    && view.drafts.iter().any(|item| item.id == id) =>
            {
                self.remove_draft_attachment(id, cx);
            }
            "attachment-download" => {
                if let Some(file) = view
                    .files
                    .iter()
                    .find(|item| item.id == id)
                    .and_then(|item| item.file.as_ref())
                {
                    download_user_file(file);
                }
            }
            _ => {}
        }
    }

    pub(super) fn request_inbox_images(&mut self) {
        let queue_rows = self
            .conversation
            .queued_prompts
            .iter()
            .filter(|row| row.mode == crate::submit::SubmitMode::Queue)
            .count();
        let show_queue = queue_rows <= 1
            || self.conversation.inbox_expanded
            || self.conversation.inbox_edit.is_some()
            || self.conversation.inbox_action.is_some();
        let images: Vec<_> = self
            .conversation
            .queued_prompts
            .iter()
            .filter(|row| show_queue || row.mode == crate::submit::SubmitMode::Steer)
            .flat_map(|row| &row.attachments)
            .filter_map(|attachment| match attachment {
                QueueAttachment::Image {
                    reference: Some(reference),
                    ..
                } => Some(reference.clone()),
                _ => None,
            })
            .collect();
        for (id, mime) in images {
            self.request_attachment(&id, &mime);
        }
    }

    pub(super) fn render_queue_attachments(
        &self,
        attachments: &[QueueAttachment],
        cx: &App,
    ) -> Option<gpui::Div> {
        if attachments.is_empty() {
            return None;
        }
        let p = dsh(cx);
        let mut rail = row()
            .max_w(px(210.))
            .flex_shrink(1.)
            .min_w_0()
            .gap(px(4.))
            .items_center()
            .overflow_hidden();
        for item in attachments {
            match item {
                QueueAttachment::Image { reference, preview } => {
                    let data = preview.as_deref().or_else(|| {
                        let (id, _) = reference.as_ref()?;
                        self.attachment_data.get(id).map(String::as_str)
                    });
                    rail = rail.child(
                        div()
                            .size(px(24.))
                            .flex_none()
                            .rounded(px(5.))
                            .border(px(0.5))
                            .border_color(p.border_l2)
                            .bg(p.bg_base)
                            .overflow_hidden()
                            .children(data.and_then(crate::media::image_source).map(|source| {
                                gpui::img(source)
                                    .size_full()
                                    .object_fit(gpui::ObjectFit::Cover)
                            })),
                    );
                }
                QueueAttachment::File { name, bytes } => {
                    rail = rail.child(
                        row()
                            .h(px(24.))
                            .max_w(px(180.))
                            .min_w(px(60.))
                            .gap(px(4.))
                            .px(px(6.))
                            .items_center()
                            .rounded(px(6.))
                            .border(px(0.5))
                            .border_color(p.border_l2)
                            .bg(p.bg_base)
                            .overflow_hidden()
                            .child(div().flex_none().child(icon(Glyph::FileText, 16., p.faint)))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(13.))
                                    .child(name.clone()),
                            )
                            .children(bytes.map(|size| {
                                div()
                                    .flex_none()
                                    .text_color(p.label_caption)
                                    .text_size(px(10.))
                                    .child(format_bytes(size))
                            })),
                    );
                }
            }
        }
        Some(rail)
    }
}

pub(super) fn render_user_files(entry: &ChatEntry, cx: &mut Context<DshApp>) -> Option<gpui::Div> {
    let files = crate::state::user_files(entry);
    if files.is_empty() {
        return None;
    }
    let p = dsh(cx);
    let mut rail = row().gap(px(8.)).flex_wrap();
    for (index, file) in files.into_iter().enumerate() {
        let label = file_download_label(&file);
        let button = row()
            .id(gpui::ElementId::Name(
                format!("user-file-{}-{index}", entry.key).into(),
            ))
            .role(gpui::Role::Button)
            .aria_label(label)
            .tab_index(0)
            .max_w(px(280.))
            .px(px(10.))
            .py(px(5.))
            .gap(px(6.))
            .items_center()
            .rounded(px(8.))
            .border(px(0.5))
            .border_color(p.border_l2)
            .bg(p.bg_base)
            .cursor_pointer()
            .hover(|style| style.bg(p.hover))
            .child(icon(Glyph::FileText, 16., p.label_caption))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .child(file.name.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(p.label_caption)
                    .text_size(px(11.))
                    .child(format_bytes(file.bytes)),
            );
        rail = rail.child(dock_controls::dock_action_button(
            button,
            cx.listener(move |_, _, _, _| download_user_file(&file)),
        ));
    }
    Some(rail)
}
