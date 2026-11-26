//! Document library for open Kantor editors, workspace files and editable samples.
use gpui::{prelude::*, *};
use kui::primitives::pro_icons::{ProIcon, pro_icon};
use std::{collections::HashSet, path::PathBuf};

use crate::{
    studio_hub::WorkspaceTreeItem,
    types::OfficeKind,
    ui_motion::{Tactile, text_button},
    ui_primitives::{
        FS_META, FS_UI, MiniDocPreviewKind, R_CARD, StudioPalette, clean_display_doc_name,
        render_doc_outline_icon, render_mini_doc_artwork, section_heading, workspace_page,
    },
    workbench::KarWorkbench,
};

fn collect_documents<'a>(
    items: &'a [WorkspaceTreeItem],
    result: &mut Vec<(&'a WorkspaceTreeItem, OfficeKind)>,
) {
    for item in items {
        if item.is_dir {
            collect_documents(&item.children, result);
        } else if let Some(kind) = OfficeKind::for_path(&item.rel_path) {
            result.push((item, kind));
        }
    }
}

impl KarWorkbench {
    pub(crate) fn render_office_gallery_screen(
        &mut self,
        avail_w: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let s = StudioPalette::get(cx);
        let loc = self.ui_locale;
        let workspace = self.effective_workspace_dir();
        let open = text_button("gallery-open", loc.pick("打开文件…", "Open file…"), &s)
            .on_press(cx, |this, win, cx| this.pick_documents(win, cx));
        let toolbar = self.workspace_toolbar(
            loc.pick("文档", "Documents"),
            Some(open.into_any_element()),
            &s,
            window,
        );
        let query = self.documents_search.read(cx).value().trim().to_lowercase();
        let mut content = workspace_page()
            .px(px(if avail_w < 600. { 16. } else { 28. }))
            .pt(px(16.))
            .gap(px(24.))
            .child(self.collection_search_field(self.documents_search.clone(), &s, cx));

        let mut list = div().flex().flex_col();
        let mut open_paths = HashSet::new();
        let mut count = 0;
        for (idx, tab) in self.office_tabs.iter().enumerate() {
            if let Some(path) = tab.path(cx) {
                open_paths.insert(path);
            }
            let name = clean_display_doc_name(&tab.doc_name(cx));
            if !name.to_lowercase().contains(&query) {
                continue;
            }
            let status = if tab.is_dirty(cx) {
                loc.pick("有未保存的更改", "Unsaved changes")
            } else {
                loc.pick("已打开", "Open")
            };
            list = list.child(
                document_row(("library-tab", idx), tab.kind, name, status.to_owned(), &s)
                    .on_press(cx, move |this, _, cx| this.activate_office_tab(idx, cx)),
            );
            count += 1;
        }
        let mut files = Vec::new();
        collect_documents(&self.workspace_tree, &mut files);
        for (n, (item, kind)) in files.into_iter().enumerate() {
            if open_paths.contains(&workspace.join(&item.rel_path))
                || !item
                    .rel_path
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&query)
            {
                continue;
            }
            let path: PathBuf = item.rel_path.clone();
            let directory = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| loc.pick("工作区", "Workspace").to_owned());
            list = list.child(
                document_row(("library-file", n), kind, item.name.clone(), directory, &s)
                    .on_press(cx, move |this, win, cx| {
                        this.open_workspace_any_file(path.clone(), win, cx)
                    }),
            );
            count += 1;
        }
        content = content.child(
            div()
                .flex()
                .flex_col()
                .child(section_heading(
                    loc.pick("工作区文档", "Workspace documents"),
                    Some(count),
                    &s,
                ))
                .child(list)
                .when(count == 0, |d| {
                    d.child(
                        div()
                            .py(px(20.))
                            .text_size(px(FS_UI))
                            .text_color(s.muted)
                            .child(if query.is_empty() {
                                loc.pick(
                            "这里会列出已打开的文档，以及文件树中的办公文件。",
                            "Open documents and Office files from the workspace tree appear here.",
                        )
                            } else {
                                loc.pick("没有匹配的文档", "No matching documents")
                            }),
                    )
                }),
        );

        let formats = [
            (
                OfficeKind::Word,
                loc.pick("文本文档", "Document"),
                MiniDocPreviewKind::Document,
                0x3168bc,
            ),
            (
                OfficeKind::Excel,
                loc.pick("电子表格", "Spreadsheet"),
                MiniDocPreviewKind::Spreadsheet,
                0x287351,
            ),
            (
                OfficeKind::Ppt,
                loc.pick("演示文稿", "Presentation"),
                MiniDocPreviewKind::Presentation,
                0xb45535,
            ),
            (
                OfficeKind::Pdf,
                loc.pick("PDF 阅读与签章", "PDF Viewer & Sign"),
                MiniDocPreviewKind::Pdf,
                0xc23b3b,
            ),
        ];
        let mut cards = div()
            .flex()
            .gap(px(12.))
            .when(avail_w < 680., |d| d.flex_col());
        for (i, (kind, label, preview, color)) in formats.into_iter().enumerate() {
            let compact = avail_w < 680.;
            let is_pdf = kind == OfficeKind::Pdf;
            cards = cards.child(
                Tactile::ghost(("new-document", i), &s)
                    .fills(s.card, kui::components::pro_chrome::pro_mix_color(s.card, s.ink, 0.045))
                    .outlined(s.line_soft, s.line_strong)
                    .flex_1()
                    .min_w_0()
                    .h(px(if compact { 52. } else { 112. }))
                    .px(px(16.))
                    .gap(px(12.))
                    .radius(R_CARD)
                    .when(!compact, |b| {
                        b.child(render_mini_doc_artwork(
                            kind.short_tag(),
                            rgb(color).into(),
                            preview,
                            &s,
                        ))
                    })
                    .when(compact, |b| {
                        b.child(render_doc_outline_icon(kind, 18., s.text))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(FS_UI))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child(label),
                    )
                    .child(pro_icon(
                        if is_pdf { ProIcon::Folder } else { ProIcon::Plus },
                        14.,
                        s.muted,
                    ))
                    .on_press(cx, move |this, win, cx| {
                        if is_pdf {
                            this.pick_documents(win, cx);
                        } else {
                            this.open_blank_office_tab(kind, win, cx);
                        }
                    }),
            );
        }
        content = content.child(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(section_heading(
                    loc.pick("新建空白文档", "Create a blank file"),
                    None,
                    &s,
                ))
                .child(cards),
        );
        let mut samples = div().flex().flex_col().child(section_heading(
            loc.pick("从示例起步", "Start from a sample"),
            None,
            &s,
        ));
        for (i, (kind, file, title, prompt)) in [
            (
                OfficeKind::Word,
                "sample_minutes.docx",
                loc.pick("会议纪要", "Meeting minutes"),
                loc.pick(
                    "请整理这份会议纪要中的决定和待办事项。",
                    "Summarize the decisions and action items in these minutes.",
                ),
            ),
            (
                OfficeKind::Excel,
                "sample_formulas.xlsx",
                loc.pick("预算与公式", "Budget workbook"),
                loc.pick(
                    "请核对这份预算表里的公式，指出需要复查的项目。",
                    "Check the formulas in this budget and flag entries to review.",
                ),
            ),
            (
                OfficeKind::Ppt,
                "sample_deck.pptx",
                loc.pick("产品演示", "Product presentation"),
                loc.pick(
                    "请梳理这份演示文稿的结构，给出逐页修改建议。",
                    "Review the structure of this deck and suggest edits slide by slide.",
                ),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            samples = samples.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        document_row(
                            ("sample-open", i),
                            kind,
                            title.to_owned(),
                            loc.pick("打开示例副本", "Open sample copy").to_owned(),
                            &s,
                        )
                        .on_press(cx, move |this, win, cx| {
                            this.open_working_sample(file, win, cx);
                        }),
                    )
                    .child(
                        text_button(
                            ("sample-draft", i),
                            loc.pick("起草提问", "Draft question"),
                            &s,
                        )
                        .on_press(cx, move |this, win, cx| {
                            if this.open_working_sample(file, win, cx) {
                                this.fill_starter_prompt(prompt, win, cx);
                            }
                        }),
                    ),
            );
        }
        content = content.child(samples);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(
                div()
                    .id("document-library-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(content),
            )
            .into_any_element()
    }
}

fn document_row(
    id: impl Into<ElementId>,
    kind: OfficeKind,
    title: String,
    detail: String,
    s: &StudioPalette,
) -> Tactile {
    Tactile::ghost(id, s)
        .flex_1()
        .min_w_0()
        .min_h(px(52.))
        .px(px(8.))
        .gap(px(12.))
        .child(render_doc_outline_icon(kind, 18., s.text))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(FS_UI))
                .child(title),
        )
        .child(
            div()
                .min_w_0()
                .max_w(px(200.))
                .truncate()
                .text_size(px(FS_META))
                .text_color(s.muted)
                .child(detail),
        )
        .child(pro_icon(ProIcon::ChevronRight, 14., s.muted))
}
