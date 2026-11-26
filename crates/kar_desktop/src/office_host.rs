//! Kantor GPUI Office host (`DocsView`, `SheetsView`, `SlidesView`), selection extraction,
//! target navigation across tabs, and real `.docx` / `.xlsx` / `.pptx` roundtrip verification.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use docs_engine::{Document, OBJECT, Paragraph, Position as DocPos, Selection as DocSel};
use docs_ui::DocsView;
use gpui::{prelude::*, *};
use sheets_engine::{CellRange, Workbook, cell_name, formula::parse_range};
use sheets_ui::SheetsView;
use slides_engine::{Focus as SlideFocus, LINE_BREAK, Presentation, ShapeKind};
use slides_ui::SlidesView;

use crate::types::{OfficeKind, SelectionLocator, SelectionTarget};

const EMU_PER_CM: f64 = 360_000.0;

#[derive(Clone)]
pub enum OfficeEditorView {
    Word(Entity<DocsView>),
    Excel(Entity<SheetsView>),
    Ppt(Entity<SlidesView>),
}

#[derive(Clone)]
pub struct OfficeTab {
    pub id: u64,
    pub kind: OfficeKind,
    pub view: OfficeEditorView,
    /// True only for isolated Demo Co-Editing tabs; blocks saving to user files.
    pub is_demo: bool,
    pub demo_scenario_key: Option<&'static str>,
}

impl OfficeTab {
    pub fn open_file(
        id: u64,
        path: PathBuf,
        is_demo: bool,
        demo_scenario_key: Option<&'static str>,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<Self> {
        let kind = OfficeKind::for_path(&path)
            .with_context(|| format!("不支持的 Office 格式: {}", path.display()))?;
        let view = match kind {
            OfficeKind::Word => {
                OfficeEditorView::Word(cx.new(|cx| DocsView::new(Some(path), window, cx)))
            }
            OfficeKind::Excel => {
                OfficeEditorView::Excel(cx.new(|cx| SheetsView::new(Some(path), window, cx)))
            }
            OfficeKind::Ppt => {
                OfficeEditorView::Ppt(cx.new(|cx| SlidesView::new(Some(path), window, cx)))
            }
        };
        Ok(Self {
            id,
            kind,
            view,
            is_demo,
            demo_scenario_key,
        })
    }

    pub fn new_blank(id: u64, kind: OfficeKind, window: &mut Window, cx: &mut App) -> Self {
        let view = match kind {
            OfficeKind::Word => OfficeEditorView::Word(cx.new(|cx| DocsView::new(None, window, cx))),
            OfficeKind::Excel => {
                OfficeEditorView::Excel(cx.new(|cx| SheetsView::new(None, window, cx)))
            }
            OfficeKind::Ppt => {
                OfficeEditorView::Ppt(cx.new(|cx| SlidesView::new(None, window, cx)))
            }
        };
        Self {
            id,
            kind,
            view,
            is_demo: false,
            demo_scenario_key: None,
        }
    }

    pub fn any_view(&self) -> AnyView {
        match &self.view {
            OfficeEditorView::Word(e) => e.clone().into(),
            OfficeEditorView::Excel(e) => e.clone().into(),
            OfficeEditorView::Ppt(e) => e.clone().into(),
        }
    }

    pub fn doc_name(&self, cx: &App) -> String {
        let raw = match &self.view {
            OfficeEditorView::Word(e) => e.read(cx).doc_name(cx),
            OfficeEditorView::Excel(e) => e.read(cx).doc_name(cx),
            OfficeEditorView::Ppt(e) => e.read(cx).doc_name(cx),
        };
        if self.is_demo && !raw.starts_with("[演示]") {
            format!("[演示] {raw}")
        } else {
            raw
        }
    }

    pub fn path(&self, cx: &App) -> Option<PathBuf> {
        match &self.view {
            OfficeEditorView::Word(e) => e.read(cx).path(cx),
            OfficeEditorView::Excel(e) => e.read(cx).path(cx),
            OfficeEditorView::Ppt(e) => e.read(cx).path(cx),
        }
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        match &self.view {
            OfficeEditorView::Word(e) => e.read(cx).is_dirty(cx),
            OfficeEditorView::Excel(e) => e.read(cx).is_dirty(cx),
            OfficeEditorView::Ppt(e) => e.read(cx).is_dirty(cx),
        }
    }

    pub fn revision(&self, cx: &App) -> Option<u64> {
        match &self.view {
            OfficeEditorView::Word(e) => e.read(cx).revision(cx),
            OfficeEditorView::Excel(e) => e.read(cx).revision(cx),
            OfficeEditorView::Ppt(e) => e.read(cx).revision(cx),
        }
    }

    pub fn is_busy(&self, cx: &App) -> bool {
        match &self.view {
            OfficeEditorView::Excel(e) => e.read(cx).is_busy(),
            OfficeEditorView::Word(_) | OfficeEditorView::Ppt(_) => false,
        }
    }

    pub fn needs_save_as(&self, cx: &App) -> bool {
        match &self.view {
            OfficeEditorView::Word(e) => e.read(cx).agent_read(|doc, _| doc.needs_save_as()),
            OfficeEditorView::Excel(e) => e.read(cx).agent_read(cx, |book, _| book.path().is_none()),
            OfficeEditorView::Ppt(e) => e.read(cx).agent_read(|pres, _| pres.needs_save_as()),
        }
    }

    pub fn default_save_filename(&self, cx: &App) -> String {
        let raw = self.doc_name(cx);
        let ext = match self.kind {
            OfficeKind::Word => "docx",
            OfficeKind::Excel => "xlsx",
            OfficeKind::Ppt => "pptx",
        };
        let stem = std::path::Path::new(&raw)
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("未命名文档");
        format!("{stem}.{ext}")
    }

    pub fn status(&self, cx: &App) -> SharedString {
        match &self.view {
            OfficeEditorView::Word(e) => e.read(cx).status(),
            OfficeEditorView::Excel(e) => e.read(cx).status(),
            OfficeEditorView::Ppt(e) => e.read(cx).status(),
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        match &self.view {
            OfficeEditorView::Word(e) => e.update(cx, |v, cx| v.focus(window, cx)),
            OfficeEditorView::Excel(e) => e.update(cx, |v, cx| v.focus(window, cx)),
            OfficeEditorView::Ppt(e) => e.update(cx, |v, cx| v.focus(window, cx)),
        }
    }

    pub fn save_in_place(&self, window: &mut Window, cx: &mut App) -> Result<String> {
        if self.is_demo {
            bail!("演示模式文档仅用于人机双写演示，不会写回真实用户文件");
        }
        match &self.view {
            OfficeEditorView::Word(e) => e.update(cx, |v, cx| v.agent_save(None, window, cx)),
            OfficeEditorView::Excel(e) => e.update(cx, |v, cx| v.agent_save(None, window, cx)),
            OfficeEditorView::Ppt(e) => e.update(cx, |v, cx| v.agent_save(None, window, cx)),
        }
    }

    pub fn save_copy_to(&self, dest: &Path, window: &mut Window, cx: &mut App) -> Result<String> {
        if self.is_demo {
            bail!("演示模式文档禁止另存为用户文件");
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match &self.view {
            OfficeEditorView::Word(e) => e.update(cx, |v, cx| v.agent_save(Some(dest), window, cx)),
            OfficeEditorView::Excel(e) => {
                e.update(cx, |v, cx| v.agent_save(Some(dest), window, cx))
            }
            OfficeEditorView::Ppt(e) => e.update(cx, |v, cx| v.agent_save(Some(dest), window, cx)),
        }
    }

    /// Capture the current user selection in this Office tab as a structured `SelectionTarget`.
    pub fn capture_selection(&self, cx: &App) -> SelectionTarget {
        let doc_name = self.doc_name(cx);
        let file_path = if self.is_demo { None } else { self.path(cx) };
        let base_revision = self.revision(cx);

        match &self.view {
            OfficeEditorView::Word(e) => e.read(cx).agent_read(|doc, sel| {
                let (start, end) = sel.ordered();
                let start_para = start.para.min(doc.len().saturating_sub(1));
                let end_para = end.para.min(doc.len().saturating_sub(1));
                let style = doc
                    .paragraph(start_para)
                    .props
                    .style
                    .as_deref()
                    .map_or_else(|| "Normal".to_owned(), |s| doc.styles.display_name(s));
                let excerpt = (start_para..=end_para)
                    .map(|idx| clean_doc_para_text(doc, idx))
                    .collect::<Vec<_>>()
                    .join("\n");
                SelectionTarget {
                    tab_id: self.id,
                    file_path,
                    doc_name,
                    kind: OfficeKind::Word,
                    base_revision,
                    locator: SelectionLocator::WordParagraph {
                        start_para,
                        end_para,
                        total_paras: doc.len(),
                        style,
                    },
                    excerpt,
                }
            }),
            OfficeEditorView::Excel(e) => {
                let v = e.read(cx);
                let (sheet_index, range, (active_row, active_col)) = v.agent_selection(cx);
                let (sheet_name, raw_input, computed_value) = v.agent_read(cx, |book, _| {
                    let sname = book
                        .sheet_names()
                        .get(sheet_index as usize)
                        .cloned()
                        .unwrap_or_else(|| format!("Sheet{}", sheet_index + 1));
                    let raw = book.input(sheet_index, active_row, active_col);
                    let val = book.display_value(sheet_index, active_row, active_col);
                    (sname, raw, val)
                });
                let active_cell = cell_name(active_row, active_col);
                let excerpt = if raw_input.starts_with('=') {
                    format!("{raw_input} → {computed_value}")
                } else if !computed_value.is_empty() {
                    computed_value.clone()
                } else {
                    raw_input.clone()
                };
                SelectionTarget {
                    tab_id: self.id,
                    file_path,
                    doc_name,
                    kind: OfficeKind::Excel,
                    base_revision,
                    locator: SelectionLocator::ExcelRange {
                        sheet_index,
                        sheet_name,
                        range: range.name(),
                        active_cell,
                        raw_input,
                        computed_value,
                    },
                    excerpt,
                }
            }
            OfficeEditorView::Ppt(e) => {
                let v = e.read(cx);
                let (slide_idx, mut selected_ix) = v.agent_selected_shape();
                v.agent_read(|pres, _| {
                    let slide = pres.slide(slide_idx);
                    if selected_ix.is_none() && !slide.shapes.is_empty() {
                        selected_ix = Some(0);
                    }
                    let (shape_id, shape_name, shape_kind, bounds_cm, excerpt) = match selected_ix
                        .and_then(|ix| slide.shapes.get(ix).map(|s| (ix, s)))
                    {
                        Some((ix, s)) => {
                            let xfrm = s
                                .xfrm
                                .or_else(|| pres.placed((slide_idx, ix)).map(|p| p.xfrm));
                            let bounds = xfrm.map(|x| {
                                (
                                    x.x as f64 / EMU_PER_CM,
                                    x.y as f64 / EMU_PER_CM,
                                    x.cx as f64 / EMU_PER_CM,
                                    x.cy as f64 / EMU_PER_CM,
                                )
                            });
                            let text = s
                                .text
                                .as_ref()
                                .map(|t| t.plain_text().replace(LINE_BREAK, " / "))
                                .unwrap_or_default();
                            (
                                Some(s.id),
                                s.name.clone(),
                                describe_shape_kind(s),
                                bounds,
                                text,
                            )
                        }
                        None => (None, String::new(), "空白幻灯片".into(), None, String::new()),
                    };
                    SelectionTarget {
                        tab_id: self.id,
                        file_path,
                        doc_name,
                        kind: OfficeKind::Ppt,
                        base_revision,
                        locator: SelectionLocator::PptObject {
                            slide_index: slide_idx,
                            slide_count: pres.len(),
                            shape_id,
                            shape_index: selected_ix,
                            shape_name,
                            shape_kind,
                            bounds_cm,
                        },
                        excerpt,
                    }
                })
            }
        }
    }

    /// Navigate this tab to a previously captured `SelectionTarget`.
    pub fn reveal_locator(
        &self,
        locator: &SelectionLocator,
        window: &mut Window,
        cx: &mut App,
    ) {
        match (&self.view, locator) {
            (
                OfficeEditorView::Word(e),
                SelectionLocator::WordParagraph {
                    start_para,
                    end_para,
                    ..
                },
            ) => {
                e.update(cx, |v, cx| {
                    let sel = v.agent_read(|doc, _| {
                        let s = (*start_para).min(doc.len().saturating_sub(1));
                        let e_idx = (*end_para).min(doc.len().saturating_sub(1));
                        let end_off = doc.paragraph(e_idx).text.len();
                        DocSel {
                            anchor: DocPos::new(s, 0),
                            head: DocPos::new(e_idx, end_off),
                        }
                    });
                    v.agent_select(sel, window, cx);
                    v.focus(window, cx);
                });
            }
            (
                OfficeEditorView::Excel(e),
                SelectionLocator::ExcelRange {
                    sheet_index, range, ..
                },
            ) => {
                if let Some(parsed) = parse_range(range) {
                    e.update(cx, |v, cx| {
                        let _ = v.agent_select(*sheet_index, parsed, window, cx);
                        v.focus(window, cx);
                    });
                }
            }
            (
                OfficeEditorView::Ppt(e),
                SelectionLocator::PptObject {
                    slide_index,
                    shape_id,
                    shape_index,
                    ..
                },
            ) => {
                e.update(cx, |v, cx| {
                    let resolved_ix = v.agent_read(|pres, _| {
                        if *slide_index >= pres.len() {
                            return None;
                        }
                        shape_id
                            .and_then(|id| pres.shape_index(*slide_index, id))
                            .or(*shape_index)
                    });
                    v.agent_select_shape(*slide_index, resolved_ix, cx);
                    v.focus(window, cx);
                });
            }
            _ => {}
        }
    }

    pub fn replace_word_paragraph(
        &self,
        para_idx: usize,
        new_text: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<()> {
        let OfficeEditorView::Word(e) = &self.view else {
            bail!("当前标签不是 Word 文档");
        };
        let text = new_text.to_owned();
        e.update(cx, |v, cx| {
            v.agent_edit("修改段落文本", window, cx, |doc, sel| {
                let after_sel = replace_doc_paragraph_preserving_anchors(doc, para_idx, &text, sel)?;
                Ok(((), after_sel))
            })
        })
    }

    pub fn write_excel_cell(
        &self,
        sheet: u32,
        cell_a1: &str,
        input_or_formula: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<String> {
        let OfficeEditorView::Excel(e) = &self.view else {
            bail!("当前标签不是 Excel 工作簿");
        };
        let range = parse_range(cell_a1)
            .with_context(|| format!("无效的单元格地址 `{cell_a1}`（示例：B4）"))?;
        let row = range.top;
        let col = range.left;
        let value = input_or_formula.to_owned();
        let show = CellRange::from_corners((row, col), (row, col));
        e.update(cx, |v, cx| {
            v.agent_edit(
                "修改单元格",
                Some((sheet, show)),
                window,
                cx,
                move |book| book.set_inputs(sheet, &[(row, col, value)]),
            )
        })?;
        Ok(e.read(cx)
            .agent_read(cx, |book, _| book.display_value(sheet, row, col)))
    }

    pub fn update_ppt_shape(
        &self,
        slide: usize,
        shape_id: u32,
        new_text: Option<&str>,
        new_xy_cm: Option<(f64, f64)>,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<()> {
        let OfficeEditorView::Ppt(e) = &self.view else {
            bail!("当前标签不是 PPT 演示文稿");
        };
        let text_owned = new_text.map(str::to_owned);
        e.update(cx, |v, cx| {
            v.agent_edit("修改幻灯片对象", window, cx, move |pres, _| {
                let ix = pres
                    .shape_index(slide, shape_id)
                    .with_context(|| format!("第 {} 页未找到对象 #{shape_id}（可能已被删除）", slide + 1))?;
                if let Some((x_cm, y_cm)) = new_xy_cm {
                    let mut xfrm = pres.slide(slide).shapes[ix]
                        .xfrm
                        .or_else(|| pres.placed((slide, ix)).map(|p| p.xfrm))
                        .context("该对象无独立坐标")?;
                    xfrm.x = (x_cm * EMU_PER_CM).round() as i64;
                    xfrm.y = (y_cm * EMU_PER_CM).round() as i64;
                    pres.set_xfrm((slide, ix), xfrm, false)?;
                }
                if let Some(ref txt) = text_owned {
                    pres.set_shape_text((slide, ix), txt)?;
                }
                Ok(((), SlideFocus::shape(slide, ix)))
            })
        })
    }

    pub fn delete_ppt_shape(
        &self,
        slide: usize,
        shape_id: u32,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<()> {
        let OfficeEditorView::Ppt(e) = &self.view else {
            bail!("当前标签不是 PPT 演示文稿");
        };
        e.update(cx, |v, cx| {
            v.agent_edit("删除幻灯片对象", window, cx, move |pres, _| {
                let ix = pres
                    .shape_index(slide, shape_id)
                    .with_context(|| format!("第 {} 页未找到对象 #{shape_id}", slide + 1))?;
                pres.delete_shape((slide, ix))?;
                Ok(((), SlideFocus::slide(slide)))
            })
        })
    }
}

pub fn clean_doc_para_text(doc: &Document, idx: usize) -> String {
    doc.paragraph(idx)
        .text
        .chars()
        .filter(|&c| c != OBJECT)
        .collect()
}

/// Replace paragraph `para_idx` text cleanly. If the paragraph contains zero-width bookmark
/// or comment markers (`OBJECT`), replace the contiguous non-object text range so inline
/// bookmarks/comments are preserved rather than rejected.
pub fn replace_doc_paragraph_preserving_anchors(
    doc: &mut Document,
    para_idx: usize,
    text: &str,
    sel: DocSel,
) -> Result<DocSel> {
    if let Ok(r) = doc.replace_paragraph_text(para_idx, text, sel) {
        let end = doc.paragraph(r.end - 1).text.len();
        return Ok(DocSel {
            anchor: DocPos::new(r.start, 0),
            head: DocPos::new(r.end - 1, end),
        });
    }
    anyhow::ensure!(para_idx < doc.len(), "段落索引越界: {para_idx}");
    let raw = &doc.paragraph(para_idx).text;
    let first_non_obj = raw.char_indices().find(|&(_, c)| c != OBJECT).map(|(i, _)| i);
    let last_non_obj_end = raw
        .char_indices()
        .rev()
        .find(|&(_, c)| c != OBJECT)
        .map(|(i, c)| i + c.len_utf8());
    match (first_non_obj, last_non_obj_end) {
        (Some(start), Some(end)) if start <= end && !raw[start..end].contains(OBJECT) => {
            Ok(doc.replace_ranges(&[(para_idx, start..end)], text, sel))
        }
        _ => doc
            .replace_paragraph_text(para_idx, text, sel)
            .map(|r| {
                let end = doc.paragraph(r.end - 1).text.len();
                DocSel {
                    anchor: DocPos::new(r.start, 0),
                    head: DocPos::new(r.end - 1, end),
                }
            })
            .map_err(anyhow::Error::msg),
    }
}

fn describe_shape_kind(s: &slides_engine::Shape) -> String {
    match &s.kind {
        ShapeKind::Shape { text_box: true, .. } => "文本框".into(),
        ShapeKind::Shape { .. } => "形状".into(),
        ShapeKind::Picture { .. } => "图片".into(),
        ShapeKind::Other { .. } if s.table().is_some() => "表格".into(),
        ShapeKind::Other { .. } if s.is_group() => "组合".into(),
        ShapeKind::Other { .. } if s.is_chart() => "图表".into(),
        ShapeKind::Other { what, .. } => what.clone(),
    }
}

/// Build isolated demo `.docx`, `.xlsx`, `.pptx` files in `demo_dir` for the three Co-Editing
/// scenarios. Never touches user files.
pub fn materialize_coedit_demo_files(demo_dir: &Path) -> Result<(PathBuf, PathBuf, PathBuf)> {
    std::fs::create_dir_all(demo_dir)?;

    // 1. Word demo: release schedule with date + owner in the same paragraph.
    let word_path = demo_dir.join("demo_coedit_schedule.docx");
    let mut doc = Document::from_paragraphs(vec![
        Paragraph::new("Kar Desktop 发布排期纪要（演示数据）"),
        Paragraph::new("发布排期：日期 2026-11-24，负责人：待定，状态：评审中"),
        Paragraph::new("回归范围：聊天时间线、Office 本地保存重开、人机双写裁决。"),
    ]);
    doc.save_as(&word_path)?;

    // 2. Excel demo: Q4 budget with formula dependent on B3 and comfortable column widths.
    let excel_path = demo_dir.join("demo_coedit_budget.xlsx");
    let mut book = Workbook::new_empty()?;
    book.set_inputs(
        0,
        &[
            (0, 0, "科目".into()),
            (0, 1, "金额 (元)".into()),
            (1, 0, "基础算力预算".into()),
            (1, 1, "80000".into()),
            (2, 0, "Q4 研发专项预算 (双写单元格 B3)".into()),
            (2, 1, "120000".into()),
            (3, 0, "合计 (=B2+B3)".into()),
            (3, 1, "=B2+B3".into()),
        ],
    )?;
    let _ = book.set_column_widths(0, &[(0, 224.0), (1, 124.0)]);
    book.save_as(&excel_path)?;

    // 3. PPT demo: slide with symmetric centered headline & subtitle text boxes.
    let ppt_path = demo_dir.join("demo_coedit_launch.pptx");
    let mut pres = Presentation::new();
    if !pres.slide(0).shapes.is_empty() {
        let _ = pres.set_shape_text((0, 0), "初版标题：Q4 增长复盘");
        if let Some(mut xfrm) = pres
            .slide(0)
            .shapes
            .first()
            .and_then(|s| s.xfrm)
            .or_else(|| pres.placed((0, 0)).map(|p| p.xfrm))
        {
            xfrm.x = (2.0 * EMU_PER_CM) as i64;
            xfrm.y = (3.0 * EMU_PER_CM) as i64;
            xfrm.cx = (29.8 * EMU_PER_CM) as i64;
            xfrm.cy = (4.8 * EMU_PER_CM) as i64;
            let _ = pres.set_xfrm((0, 0), xfrm, false);
        }
    }
    if pres.slide(0).shapes.len() >= 2 {
        let _ = pres.set_shape_text((0, 1), "纯 Rust · GPUI · Kantor 双写协议演练（演示数据）");
        if let Some(mut xfrm) = pres
            .slide(0)
            .shapes
            .get(1)
            .and_then(|s| s.xfrm)
            .or_else(|| pres.placed((0, 1)).map(|p| p.xfrm))
        {
            xfrm.x = (2.0 * EMU_PER_CM) as i64;
            xfrm.y = (9.2 * EMU_PER_CM) as i64;
            xfrm.cx = (29.8 * EMU_PER_CM) as i64;
            xfrm.cy = (2.8 * EMU_PER_CM) as i64;
            let _ = pres.set_xfrm((0, 1), xfrm, false);
        }
    }
    pres.save_as(&ppt_path)?;

    Ok((word_path, excel_path, ppt_path))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoundtripReport {
    pub kind: OfficeKind,
    pub saved_copy: PathBuf,
    pub edited_summary: String,
    pub unedited_check: String,
}

/// Real Kantor `.docx` open -> edit paragraph -> save copy -> close -> reopen verification.
pub fn verify_docx_roundtrip(
    source_copy: &Path,
    saved_copy: &Path,
    para_idx: usize,
    replacement_text: &str,
) -> Result<RoundtripReport> {
    let mut doc = Document::open_with_password(source_copy, None)?;
    anyhow::ensure!(
        doc.len() >= 2 && para_idx < doc.len(),
        "docx sample needs at least 2 paragraphs"
    );
    let untouched_idx = if para_idx == 0 { 1 } else { 0 };
    let untouched_before = clean_doc_para_text(&doc, untouched_idx);

    let sel = DocSel::caret(DocPos::new(para_idx, 0));
    replace_doc_paragraph_preserving_anchors(&mut doc, para_idx, replacement_text, sel)?;
    doc.save_as(saved_copy)?;
    drop(doc); // Close document

    // Reopen from disk and verify edited + unedited paragraphs
    let reopened = Document::open_with_password(saved_copy, None)?;
    let edited_after = clean_doc_para_text(&reopened, para_idx);
    let untouched_after = clean_doc_para_text(&reopened, untouched_idx);
    anyhow::ensure!(
        edited_after == replacement_text,
        "reopened docx paragraph #{para_idx} mismatch: got `{edited_after}`, expected `{replacement_text}`"
    );
    anyhow::ensure!(
        untouched_after == untouched_before,
        "reopened docx unedited paragraph #{untouched_idx} changed: `{untouched_after}` != `{untouched_before}`"
    );

    Ok(RoundtripReport {
        kind: OfficeKind::Word,
        saved_copy: saved_copy.to_path_buf(),
        edited_summary: format!("段落 #{} 已持久更新为「{}」", para_idx + 1, edited_after),
        unedited_check: format!(
            "未编辑段落 #{} 保持原样：「{}」",
            untouched_idx + 1,
            untouched_after
        ),
    })
}

/// Real Kantor `.xlsx` open -> edit value & formula -> verify calculated value -> save copy -> close -> reopen.
pub fn verify_xlsx_roundtrip(
    source_copy: &Path,
    saved_copy: &Path,
) -> Result<RoundtripReport> {
    let mut book = Workbook::open_with_password(source_copy, None)?;
    let untouched_a1_before = book.display_value(0, 0, 0);

    // Write numeric inputs in H2=125, H3=275 and formula in H4 =SUM(H2:H3)*2 (= 800)
    book.set_inputs(
        0,
        &[
            (1, 7, "125".into()),
            (2, 7, "275".into()),
            (3, 7, "=SUM(H2:H3)*2".into()),
        ],
    )?;
    let calc_before_save = book.display_value(0, 3, 7);
    anyhow::ensure!(
        calc_before_save == "800",
        "expected H4 formula result 800 before save, got `{calc_before_save}`"
    );
    book.save_as(saved_copy)?;
    drop(book); // Close workbook

    // Reopen saved copy and verify formula + computed value + unedited A1
    let reopened = Workbook::open_with_password(saved_copy, None)?;
    let formula_after = reopened.input(0, 3, 7);
    let calc_after = reopened.display_value(0, 3, 7);
    let untouched_a1_after = reopened.display_value(0, 0, 0);
    anyhow::ensure!(
        formula_after == "=SUM(H2:H3)*2",
        "reopened xlsx formula mismatch: `{formula_after}`"
    );
    anyhow::ensure!(
        calc_after == "800",
        "reopened xlsx computed value mismatch: `{calc_after}`"
    );
    anyhow::ensure!(
        untouched_a1_after == untouched_a1_before,
        "reopened xlsx unedited A1 changed: `{untouched_a1_after}` != `{untouched_a1_before}`"
    );

    Ok(RoundtripReport {
        kind: OfficeKind::Excel,
        saved_copy: saved_copy.to_path_buf(),
        edited_summary: format!("H4 公式 `{formula_after}` 重开后计算结果为 `{calc_after}`"),
        unedited_check: format!("未编辑单元格 A1 保持原值 `{untouched_a1_after}`"),
    })
}

/// Real Kantor `.pptx` open -> edit object text & coordinates -> save copy -> close -> reopen.
pub fn verify_pptx_roundtrip(
    source_copy: &Path,
    saved_copy: &Path,
    new_text: &str,
    new_xy_cm: (f64, f64),
) -> Result<RoundtripReport> {
    let mut pres = Presentation::open_with_password(source_copy, None)?;
    anyhow::ensure!(!pres.is_empty(), "pptx sample has no slides");
    let slide_count_before = pres.len();
    let untouched_slide_check = if pres.len() >= 2 {
        pres.slide(1)
            .shapes
            .first()
            .and_then(|s| s.text.as_ref().map(|t| t.plain_text()))
            .unwrap_or_default()
    } else {
        String::new()
    };

    anyhow::ensure!(
        !pres.slide(0).shapes.is_empty(),
        "slide 1 has no shapes to edit"
    );
    let shape_id = pres.slide(0).shapes[0].id;
    let mut xfrm = pres.slide(0).shapes[0]
        .xfrm
        .or_else(|| pres.placed((0, 0)).map(|p| p.xfrm))
        .context("slide 1 shape 0 has no xfrm")?;
    let target_x = (new_xy_cm.0 * EMU_PER_CM).round() as i64;
    let target_y = (new_xy_cm.1 * EMU_PER_CM).round() as i64;
    xfrm.x = target_x;
    xfrm.y = target_y;
    pres.set_xfrm((0, 0), xfrm, false)?;
    pres.set_shape_text((0, 0), new_text)?;
    pres.save_as(saved_copy)?;
    drop(pres); // Close presentation

    // Reopen saved copy and verify shape text, coordinates, and unedited slide
    let reopened = Presentation::open_with_password(saved_copy, None)?;
    anyhow::ensure!(reopened.len() == slide_count_before, "slide count changed");
    let ix = reopened
        .shape_index(0, shape_id)
        .context("edited shape id missing after reopen")?;
    let after_shape = &reopened.slide(0).shapes[ix];
    let after_text = after_shape
        .text
        .as_ref()
        .map(|t| t.plain_text().replace(LINE_BREAK, "\n"))
        .unwrap_or_default();
    let after_xfrm = after_shape
        .xfrm
        .or_else(|| reopened.placed((0, ix)).map(|p| p.xfrm))
        .context("reopened shape missing xfrm")?;
    anyhow::ensure!(
        after_text == new_text,
        "reopened pptx text mismatch: `{after_text}` != `{new_text}`"
    );
    anyhow::ensure!(
        after_xfrm.x == target_x && after_xfrm.y == target_y,
        "reopened pptx coords mismatch: ({}, {}) != ({target_x}, {target_y})",
        after_xfrm.x,
        after_xfrm.y
    );
    if reopened.len() >= 2 {
        let untouched_after = reopened
            .slide(1)
            .shapes
            .first()
            .and_then(|s| s.text.as_ref().map(|t| t.plain_text()))
            .unwrap_or_default();
        anyhow::ensure!(
            untouched_after == untouched_slide_check,
            "unedited slide 2 text changed"
        );
    }

    Ok(RoundtripReport {
        kind: OfficeKind::Ppt,
        saved_copy: saved_copy.to_path_buf(),
        edited_summary: format!(
            "第 1 页对象 #{shape_id} 重开后文字为「{after_text}」，坐标为 ({:.2}cm, {:.2}cm)",
            after_xfrm.x as f64 / EMU_PER_CM,
            after_xfrm.y as f64 / EMU_PER_CM
        ),
        unedited_check: format!(
            "总页数保持 {} 页，第 2 页未编辑对象保持原样",
            reopened.len()
        ),
    })
}
