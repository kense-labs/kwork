//! Paragraph-level edits for programs driving Kantor (the MCP server):
//! insert, replace and delete whole paragraphs. Each call is one undo step
//! and leaves tables and pictures alone (it refuses rather than break them).

use std::ops::Range;

use crate::document::{Document, Kind};
use crate::model::{CellRef, NumRef, ParaProps, Paragraph, Position, RunProps, Selection};
use crate::numbering::ListKind;
use crate::object::OBJECT;
use crate::style::StyleKind;
use crate::table::strip_element;
use crate::table_merge::set_tc_child;
use crate::table_props::set_tbl_pr_child;

/// A styled text run inside a [`RichParagraph`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichRun {
    pub text: String,
    pub props: RunProps,
}

/// A paragraph with full typography (`ParaProps`, mixed inline `RichRun`s, list level).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichParagraph {
    pub runs: Vec<RichRun>,
    pub props: ParaProps,
    pub list: Option<ListKind>,
    pub level: u8,
}

/// Common WPS / Word table border styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableBorderPreset {
    /// Full 0.5pt grid borders on all sides and inside rules.
    Grid,
    /// Academic / official three-line table (1.5pt top & bottom, 0.75pt header rule, no vertical rules).
    ThreeLine,
    /// Borderless layout table.
    None,
}

/// A paragraph to insert.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewParagraph {
    pub text: String,
    /// Paragraph style id (`Heading1`); see [`Document::find_style`].
    pub style: Option<String>,
    pub list: Option<ListKind>,
    /// List level, 0–8 (only with `list`).
    pub level: u8,
}

/// Text as a paragraph can hold it: no control characters but tabs, no
/// object placeholders. Line breaks are the caller's to split on.
fn clean(text: &str) -> String {
    text.chars()
        .filter(|&c| c == '\t' || (!c.is_control() && c != OBJECT))
        .collect()
}

/// The lines of `text` (`\n`, `\r\n` or `\r` end a paragraph).
fn lines(text: &str) -> Vec<String> {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(clean)
        .collect()
}

impl Document {
    /// The id of the paragraph style called `name`: an id (`Heading1`) or a
    /// display name (`heading 1`), case-insensitive; `Normal` is `None`.
    pub fn find_style(&self, name: &str) -> Result<Option<String>, String> {
        let name = name.trim();
        if name.is_empty() || name.eq_ignore_ascii_case("normal") {
            return Ok(None);
        }
        let squash = |s: &str| s.replace(' ', "").to_ascii_lowercase();
        let wanted = squash(name);
        self.styles
            .styles
            .iter()
            .filter(|s| s.kind == StyleKind::Paragraph)
            .find(|s| squash(&s.id) == wanted || squash(&s.name) == wanted)
            .map(|s| Some(s.id.clone()))
            .ok_or_else(|| {
                format!(
                    "no paragraph style \"{name}\"; this document has: {}",
                    self.paragraph_style_names().join(", ")
                )
            })
    }

    /// Display names of the document's paragraph styles.
    pub fn paragraph_style_names(&self) -> Vec<String> {
        self.styles
            .styles
            .iter()
            .filter(|s| s.kind == StyleKind::Paragraph)
            .map(|s| self.styles.display_name(&s.id))
            .collect()
    }

    /// Whether a new body paragraph can go at index `at` (not inside a table).
    fn body_gap(&self, at: usize) -> bool {
        let before = at.checked_sub(1).and_then(|i| self.paragraphs.get(i));
        let after = self.paragraphs.get(at);
        match (before.and_then(|p| p.cell), after.and_then(|p| p.cell)) {
            (Some(a), Some(b)) => a.table != b.table,
            _ => true,
        }
    }

    /// Insert paragraphs after paragraph `after` (`None`: at the start;
    /// after a paragraph in a table: after that table). Text with line
    /// breaks becomes several paragraphs of the same kind. A blank new
    /// document's only paragraph is replaced. Returns the new paragraphs'
    /// indices and selects them for undo/redo; `sel` is restored on undo.
    pub fn insert_paragraphs(
        &mut self,
        after: Option<usize>,
        items: &[NewParagraph],
        sel: Selection,
    ) -> Result<Range<usize>, String> {
        let mut at = match after {
            None => 0,
            Some(i) if i >= self.paragraphs.len() => {
                return Err(format!(
                    "paragraph {i} doesn't exist (the document has {})",
                    self.paragraphs.len()
                ));
            }
            Some(i) => match self.paragraphs[i].cell {
                Some(c) => self.table_range(c.table).end,
                None => i + 1,
            },
        };
        if !self.body_gap(at) {
            return Err("can't insert inside a table".into());
        }
        let blank = self.paragraphs.len() == 1
            && self.paragraphs[0].text.is_empty()
            && self.paragraphs[0].cell.is_none()
            && self.paragraphs[0].props.num.is_none();
        let replace = usize::from(blank);
        if blank {
            at = 0;
        }

        // Lists continue the one just before them when it's the same kind.
        let mut prev_list = at.checked_sub(1).and_then(|i| {
            Some((
                self.list_kind(i)?,
                self.styles.effective_num(&self.paragraphs[i])?.id,
            ))
        });
        let mut new = Vec::new();
        for item in items {
            let num = match item.list {
                None => None,
                Some(kind) => {
                    let id = match prev_list {
                        Some((k, id)) if k == kind => id,
                        _ => self.numbering.new_list(kind),
                    };
                    prev_list = Some((kind, id));
                    Some(NumRef {
                        id,
                        level: item.level.min(8),
                    })
                }
            };
            if num.is_none() {
                prev_list = None;
            }
            for line in lines(&item.text) {
                let mut p = Paragraph::new(line);
                p.props.style = item.style.clone();
                p.props.num = num;
                new.push(p);
            }
        }
        if new.is_empty() {
            return Err("no paragraphs to insert".into());
        }
        let range = at..at + new.len();
        let last = new.last().map_or(0, |p| p.text.len());
        let after_sel = Selection {
            anchor: Position::new(range.start, 0),
            head: Position::new(range.end - 1, last),
        };
        self.replace(at, replace, new, (sel, after_sel), Kind::Other, true);
        Ok(range)
    }

    /// Replace the text of paragraph `i`, keeping its style, list and the
    /// formatting of its first character. Line breaks in `text` add
    /// paragraphs of the same kind. Refuses paragraphs holding pictures or
    /// other objects. Returns the indices of the resulting paragraphs.
    pub fn replace_paragraph_text(
        &mut self,
        i: usize,
        text: &str,
        sel: Selection,
    ) -> Result<Range<usize>, String> {
        let Some(p) = self.paragraphs.get(i) else {
            return Err(format!(
                "paragraph {i} doesn't exist (the document has {})",
                self.paragraphs.len()
            ));
        };
        if p.objects().any(|(_, o)| !o.is_marker()) {
            return Err(format!(
                "paragraph {i} holds a picture or another object Kantor would lose; edit its text with find_replace instead"
            ));
        }
        let props = p.props_at(0);
        let mut template = p.clone();
        template.delete(0..template.text.len());
        let new: Vec<Paragraph> = lines(text)
            .into_iter()
            .map(|line| {
                let mut q = template.clone();
                q.insert(0, &line, props.clone());
                q
            })
            .collect();
        let range = i..i + new.len();
        let last = new.last().map_or(0, |p| p.text.len());
        let after_sel = Selection {
            anchor: Position::new(i, 0),
            head: Position::new(range.end - 1, last),
        };
        self.replace(i, 1, new, (sel, after_sel), Kind::Other, true);
        Ok(range)
    }

    /// Delete paragraphs `i..i + count` from the body. Refuses ranges that
    /// touch a table. Deleting everything leaves one empty paragraph.
    pub fn delete_paragraphs(
        &mut self,
        i: usize,
        count: usize,
        sel: Selection,
    ) -> Result<(), String> {
        let end = i.saturating_add(count);
        if count == 0 || end > self.paragraphs.len() {
            return Err(format!(
                "paragraphs {i}..{end} don't exist (the document has {})",
                self.paragraphs.len()
            ));
        }
        if self.paragraphs[i..end].iter().any(|p| p.cell.is_some()) {
            return Err("those paragraphs include a table, which can't be deleted this way".into());
        }
        let new = if count == self.paragraphs.len() {
            vec![Paragraph::default()]
        } else {
            Vec::new()
        };
        let left = (self.paragraphs.len() - count).max(1);
        let caret = Selection::caret(Position::new(i.min(left - 1), 0));
        self.replace(i, count, new, (sel, caret), Kind::Other, true);
        Ok(())
    }

    /// Sorted table IDs present in the document.
    pub fn table_ids(&self) -> Vec<u32> {
        self.tables.keys().copied().collect()
    }

    /// Insert or replace body paragraphs with full paragraph & character typography in one undo step.
    pub fn insert_rich_paragraphs(
        &mut self,
        after: Option<usize>,
        replace_all: bool,
        items: &[RichParagraph],
        sel: Selection,
    ) -> Result<Range<usize>, String> {
        if items.is_empty() {
            return Err("no paragraphs to insert".into());
        }
        let (mut at, mut replace_count) = if replace_all {
            (0, self.paragraphs.len())
        } else {
            let at = match after {
                None => 0,
                Some(i) if i >= self.paragraphs.len() => {
                    return Err(format!(
                        "paragraph {i} doesn't exist (the document has {})",
                        self.paragraphs.len()
                    ));
                }
                Some(i) => match self.paragraphs[i].cell {
                    Some(c) => self.table_range(c.table).end,
                    None => i + 1,
                },
            };
            if !self.body_gap(at) {
                return Err("can't insert inside a table".into());
            }
            let blank = self.paragraphs.len() == 1
                && self.paragraphs[0].text.is_empty()
                && self.paragraphs[0].cell.is_none()
                && self.paragraphs[0].props.num.is_none();
            if blank {
                (0, 1)
            } else {
                (at, 0)
            }
        };

        let mut prev_list = (!replace_all)
            .then(|| at.checked_sub(1))
            .flatten()
            .and_then(|i| {
                Some((
                    self.list_kind(i)?,
                    self.styles.effective_num(&self.paragraphs[i])?.id,
                ))
            });

        let mut new = Vec::with_capacity(items.len());
        for item in items {
            let num = match item.list {
                None => item.props.num,
                Some(kind) => {
                    let id = match prev_list {
                        Some((k, id)) if k == kind => id,
                        _ => self.numbering.new_list(kind),
                    };
                    prev_list = Some((kind, id));
                    Some(NumRef {
                        id,
                        level: item.level.min(8),
                    })
                }
            };
            if num.is_none() {
                prev_list = None;
            }
            let mut base_props = item.props.clone();
            base_props.num = num;

            if item.runs.is_empty() {
                let mut p = Paragraph::styled("", RunProps::default());
                p.props = base_props;
                new.push(p);
                continue;
            }

            // Split multi-line runs cleanly into paragraphs preserving run styling.
            let default_run_props = item.runs[0].props.clone();
            let mut cur = Paragraph::styled("", default_run_props.clone());
            cur.props = base_props.clone();
            for run in &item.runs {
                let parts = lines(&run.text);
                for (part_ix, piece) in parts.into_iter().enumerate() {
                    if part_ix > 0 {
                        new.push(cur);
                        cur = Paragraph::styled("", run.props.clone());
                        cur.props = base_props.clone();
                    }
                    if !piece.is_empty() {
                        cur.push(&piece, run.props.clone());
                    } else if cur.text.is_empty() {
                        cur.runs = vec![crate::model::Run::text(0, run.props.clone())];
                    }
                }
            }
            new.push(cur);
        }

        if new.is_empty() {
            return Err("no paragraphs to insert".into());
        }
        if replace_all {
            at = 0;
            replace_count = self.paragraphs.len();
        }
        let range = at..at + new.len();
        let last = new.last().map_or(0, |p| p.text.len());
        let after_sel = Selection {
            anchor: Position::new(range.start, 0),
            head: Position::new(range.end - 1, last),
        };
        let tables_override = replace_all.then(std::collections::BTreeMap::new);
        self.replace_with_tables(
            at,
            replace_count,
            new,
            tables_override,
            (sel, after_sel),
            Kind::Other,
            true,
        );
        Ok(range)
    }

    /// Replace the text and optional formatting of a specific table cell `(row, col)` (0-based)
    /// as one undo step.
    #[allow(clippy::too_many_arguments)]
    pub fn set_table_cell_content(
        &mut self,
        table_id: u32,
        row: usize,
        col: usize,
        text: &str,
        run_props: Option<RunProps>,
        align: Option<crate::model::Align>,
        sel: Selection,
    ) -> Result<Selection, String> {
        let t = self
            .tables
            .get(&table_id)
            .ok_or_else(|| format!("table #{table_id} doesn't exist"))?;
        if row >= t.rows.len() {
            return Err(format!(
                "row {row} out of bounds (table #{table_id} has {} rows)",
                t.rows.len()
            ));
        }
        let cols_in_row = t.rows[row].cells.len();
        if col >= cols_in_row {
            return Err(format!(
                "col {col} out of bounds (row {row} has {cols_in_row} cells)"
            ));
        }
        let target_cell = CellRef {
            table: table_id,
            row: row as u32,
            cell: col as u32,
        };
        let range = self.table_range(table_id);
        let mut cell_start = None;
        let mut cell_count = 0usize;
        for idx in range {
            if self.paragraphs[idx].cell == Some(target_cell) {
                if cell_start.is_none() {
                    cell_start = Some(idx);
                }
                cell_count += 1;
            } else if cell_start.is_some() {
                break;
            }
        }
        let start = cell_start.ok_or_else(|| "target table cell not found".to_owned())?;
        let template = &self.paragraphs[start];
        let mut base_run = template.props_at(0);
        if let Some(ref over) = run_props {
            base_run.overlay(over);
        }
        let mut base_para = template.props.clone();
        if let Some(a) = align {
            base_para.align = Some(a);
        }
        let new_paras: Vec<Paragraph> = lines(text)
            .into_iter()
            .map(|line| {
                let mut p = Paragraph::styled(line, base_run.clone());
                p.props = base_para.clone();
                p.cell = Some(target_cell);
                p
            })
            .collect();
        let last_idx = start + new_paras.len() - 1;
        let last_len = new_paras.last().map_or(0, |p| p.text.len());
        let after_sel = Selection {
            anchor: Position::new(start, 0),
            head: Position::new(last_idx, last_len),
        };
        self.replace(start, cell_count, new_paras, (sel, after_sel), Kind::Other, true);
        Ok(after_sel)
    }

    /// Apply WPS-grade table styling (border preset, repeating header row, header/cell shading,
    /// column widths, alignment) in one undoable step.
    #[allow(clippy::too_many_arguments)]
    pub fn style_table_wps(
        &mut self,
        table_id: u32,
        border_preset: Option<TableBorderPreset>,
        header_row: Option<bool>,
        header_fill_hex: Option<&str>,
        col_widths_twips: Option<&[u32]>,
        align: Option<crate::table::TableAlign>,
        cell_fills: &[(usize, usize, String)],
        sel: Selection,
    ) -> Result<Selection, String> {
        let mut t = self
            .tables
            .get(&table_id)
            .cloned()
            .ok_or_else(|| format!("table #{table_id} doesn't exist"))?;

        if let Some(widths) = col_widths_twips
            && widths.len() == t.grid.len()
        {
            t.grid = widths.iter().map(|&w| w.max(240)).collect();
            for row in &mut t.rows {
                let mut g = 0usize;
                for cell in &mut row.cells {
                    let span = (cell.span as usize).max(1);
                    let w: u32 = t.grid.iter().skip(g).take(span).sum();
                    g += span;
                    let base = cell
                        .tc_pr
                        .as_deref()
                        .unwrap_or("<w:tcPr></w:tcPr>");
                    let updated = set_tc_child(
                        base,
                        "tcW",
                        Some(format!("<w:tcW w:w=\"{w}\" w:type=\"dxa\"/>")),
                    );
                    cell.tc_pr = Some(updated.into());
                    cell.refresh();
                }
            }
        }

        let mut tbl_pr = t
            .tbl_pr
            .as_deref()
            .unwrap_or("<w:tblPr></w:tblPr>")
            .to_owned();

        if let Some(a) = align {
            tbl_pr = set_tbl_pr_child(
                &tbl_pr,
                "jc",
                Some(format!("<w:jc w:val=\"{}\"/>", a.jc())),
            );
        }

        if let Some(preset) = border_preset {
            let borders_xml = match preset {
                TableBorderPreset::Grid => Some(
                    "<w:tblBorders>\
                     <w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CBD5E1\"/>\
                     <w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CBD5E1\"/>\
                     <w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CBD5E1\"/>\
                     <w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CBD5E1\"/>\
                     <w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CBD5E1\"/>\
                     <w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CBD5E1\"/>\
                     </w:tblBorders>"
                        .to_owned(),
                ),
                TableBorderPreset::ThreeLine => Some(
                    "<w:tblBorders>\
                     <w:top w:val=\"single\" w:sz=\"12\" w:space=\"0\" w:color=\"1E293B\"/>\
                     <w:left w:val=\"none\"/>\
                     <w:bottom w:val=\"single\" w:sz=\"12\" w:space=\"0\" w:color=\"1E293B\"/>\
                     <w:right w:val=\"none\"/>\
                     <w:insideH w:val=\"none\"/>\
                     <w:insideV w:val=\"none\"/>\
                     </w:tblBorders>"
                        .to_owned(),
                ),
                TableBorderPreset::None => Some(
                    "<w:tblBorders>\
                     <w:top w:val=\"none\"/>\
                     <w:left w:val=\"none\"/>\
                     <w:bottom w:val=\"none\"/>\
                     <w:right w:val=\"none\"/>\
                     <w:insideH w:val=\"none\"/>\
                     <w:insideV w:val=\"none\"/>\
                     </w:tblBorders>"
                        .to_owned(),
                ),
            };
            tbl_pr = set_tbl_pr_child(&tbl_pr, "tblBorders", borders_xml);

            // For a three-line academic/official table, draw a crisp 0.75pt bottom border under row 0.
            if preset == TableBorderPreset::ThreeLine
                && let Some(first_row) = t.rows.first_mut()
            {
                for cell in &mut first_row.cells {
                    let base = cell.tc_pr.as_deref().unwrap_or("<w:tcPr></w:tcPr>");
                    let updated = set_tc_child(
                        base,
                        "tcBorders",
                        Some(
                            "<w:tcBorders>\
                             <w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"0\" w:color=\"334155\"/>\
                             </w:tcBorders>"
                                .into(),
                        ),
                    );
                    cell.tc_pr = Some(updated.into());
                    cell.refresh();
                }
            }
        }

        t.tbl_pr = Some(tbl_pr.into());
        t.refresh();

        if let Some(hdr_on) = header_row
            && let Some(first_row) = t.rows.first_mut()
        {
            let cleaned = first_row
                .tr_pr
                .as_deref()
                .map(|x| strip_element(x, "tblHeader"))
                .unwrap_or_else(|| "<w:trPr></w:trPr>".to_owned());
            let updated = if hdr_on {
                cleaned.replacen("<w:trPr>", "<w:trPr><w:tblHeader/>", 1)
            } else {
                cleaned
            };
            first_row.tr_pr = Some(updated.into());
            first_row.refresh();
        }

        if let Some(fill) = header_fill_hex
            && let Some(first_row) = t.rows.first_mut()
        {
            let hex = fill.trim_start_matches('#').to_ascii_uppercase();
            if hex.len() == 6 {
                for cell in &mut first_row.cells {
                    let base = cell.tc_pr.as_deref().unwrap_or("<w:tcPr></w:tcPr>");
                    let shd = format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{hex}\"/>");
                    cell.tc_pr = Some(set_tc_child(base, "shd", Some(shd)).into());
                    cell.refresh();
                }
            }
        }

        for (r, c, hex_raw) in cell_fills {
            let hex = hex_raw.trim_start_matches('#').to_ascii_uppercase();
            if hex.len() == 6
                && let Some(cell) = t.rows.get_mut(*r).and_then(|row| row.cells.get_mut(*c))
            {
                let base = cell.tc_pr.as_deref().unwrap_or("<w:tcPr></w:tcPr>");
                let shd = format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{hex}\"/>");
                cell.tc_pr = Some(set_tc_child(base, "shd", Some(shd)).into());
                cell.refresh();
            }
        }

        let range = self.table_range(table_id);
        let paras = self.paragraphs[range.clone()].to_vec();
        let mut tables = self.tables.clone();
        tables.insert(table_id, t);
        let after_sel = Selection::caret(Position::new(range.start, 0));
        self.replace_with_tables(
            range.start,
            range.len(),
            paras,
            Some(tables),
            (sel, after_sel),
            Kind::Other,
            true,
        );
        Ok(after_sel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(doc: &Document) -> Vec<&str> {
        doc.paragraphs().iter().map(|p| p.text.as_str()).collect()
    }

    fn item(text: &str) -> NewParagraph {
        NewParagraph {
            text: text.into(),
            ..Default::default()
        }
    }

    const SEL: Selection = Selection::caret(Position::new(0, 0));

    #[test]
    fn inserts_into_a_blank_document_and_undoes_in_one_step() {
        let mut doc = Document::new();
        let heading = NewParagraph {
            style: doc.find_style("heading 1").unwrap(),
            ..item("Plan")
        };
        let r = doc
            .insert_paragraphs(Some(0), &[heading, item("One\nTwo")], SEL)
            .unwrap();
        assert_eq!(r, 0..3);
        assert_eq!(texts(&doc), ["Plan", "One", "Two"]);
        assert_eq!(doc.paragraph(0).props.style.as_deref(), Some("Heading1"));
        assert!(doc.undo().is_some());
        assert_eq!(texts(&doc), [""]);
        assert!(!doc.is_dirty());
    }

    #[test]
    fn inserts_lists_that_continue() {
        let mut doc = Document::from_paragraphs(vec![Paragraph::new("a"), Paragraph::new("b")]);
        let bullet = |t: &str, level| NewParagraph {
            list: Some(ListKind::Bullet),
            level,
            ..item(t)
        };
        doc.insert_paragraphs(Some(0), &[bullet("x", 0), bullet("y", 1)], SEL)
            .unwrap();
        assert_eq!(texts(&doc), ["a", "x", "y", "b"]);
        assert_eq!(doc.list_kind(1), Some(ListKind::Bullet));
        let (x, y) = (doc.paragraph(1).props.num, doc.paragraph(2).props.num);
        assert_eq!(x.unwrap().id, y.unwrap().id);
        assert_eq!(y.unwrap().level, 1);
        assert_eq!(doc.list_kind(3), None);
        assert!(doc.insert_paragraphs(Some(9), &[item("z")], SEL).is_err());
        assert!(doc.find_style("No Such Style").is_err());
    }

    #[test]
    fn replaces_and_deletes() {
        let mut doc = Document::from_paragraphs(vec![
            Paragraph::new("a").with_style("Heading1"),
            Paragraph::new("b"),
        ]);
        doc.replace_paragraph_text(0, "A\u{7}!\nA2", SEL).unwrap();
        assert_eq!(texts(&doc), ["A!", "A2", "b"]);
        assert_eq!(doc.paragraph(1).props.style.as_deref(), Some("Heading1"));
        doc.delete_paragraphs(1, 2, SEL).unwrap();
        assert_eq!(texts(&doc), ["A!"]);
        doc.delete_paragraphs(0, 1, SEL).unwrap();
        assert_eq!(texts(&doc), [""]);
        assert!(doc.delete_paragraphs(0, 2, SEL).is_err());
        doc.undo();
        doc.undo();
        doc.undo();
        assert_eq!(texts(&doc), ["a", "b"]);
    }
}
