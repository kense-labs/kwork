//! Select one file from an immutable Kar turn checkpoint, with human-readable
//! text extraction for binary Office / PDF documents (`.docx`, `.xlsx`, `.pptx`, `.pdf`).

use crate::types::OfficeKind;
use kar::chat::ChatStore;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewKey {
    pub chat: Uuid,
    pub turn: Uuid,
    pub index: usize,
    pub path: PathBuf,
}

pub(crate) struct FileReview {
    pub key: ReviewKey,
    pub patch: Option<Result<Arc<Vec<String>>, String>>,
}

fn path_matches(token: &str, prefix: &[u8], path: &Path) -> bool {
    let token = token.trim_end_matches('\t');
    let Ok((decoded, consumed)) = gix_quote::ansi_c::undo(token.as_bytes().into()) else {
        return false;
    };
    let bytes: &[u8] = decoded.as_ref().as_ref();
    #[cfg(windows)]
    let normalized = path.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    let expected = normalized.as_bytes();
    #[cfg(not(windows))]
    let expected = path.as_os_str().as_encoded_bytes();
    consumed == token.len() && bytes.strip_prefix(prefix) == Some(expected)
}

fn patch_for_path<'a>(diff: &'a str, path: &Path) -> Option<&'a str> {
    let mut starts = diff
        .match_indices("\ndiff --git ")
        .map(|(offset, _)| offset + 1)
        .chain(std::iter::once(diff.len()));
    let mut start = 0;
    while start < diff.len() {
        let end = starts.next()?;
        let section = &diff[start..end];
        start = end;
        let mut lines = section.lines();
        let Some(header) = lines.next().and_then(|l| l.strip_prefix("diff --git ")) else {
            continue;
        };
        if lines
            .take_while(|l| !l.starts_with("@@") && !l.starts_with("GIT binary patch"))
            .any(|line| {
                [
                    ("--- ", b"a/".as_slice()),
                    ("+++ ", b"b/"),
                    ("rename from ", b""),
                    ("rename to ", b""),
                    ("copy from ", b""),
                    ("copy to ", b""),
                ]
                .iter()
                .any(|(marker, prefix)| {
                    line.strip_prefix(marker)
                        .is_some_and(|name| path_matches(name, prefix, path))
                })
            })
        {
            return Some(section);
        }
        // Binary files and mode-only changes can have neither --- nor +++.
        // Match both sides so spaces in a Git filename remain unambiguous.
        if header.match_indices(' ').any(|(i, _)| {
            path_matches(&header[..i], b"a/", path) && path_matches(&header[i + 1..], b"b/", path)
        }) {
            return Some(section);
        }
    }
    None
}

/// Extract a human-readable structured preview of an Office / PDF file on disk so users
/// never stare at `Binary files /dev/null and "b/\351\230\205..."` in turn diffs.
pub(crate) fn extract_office_readable_lines(full_path: &Path, is_new: bool) -> Option<Vec<String>> {
    let kind = OfficeKind::for_path(full_path)?;
    if !full_path.is_file() {
        return Some(vec![format!(
            "- [已删除 {} 文档] {}",
            kind.short_tag(),
            full_path.file_name().and_then(|s| s.to_str()).unwrap_or_default()
        )]);
    }
    let prefix = if is_new { "+" } else { " " };
    match kind {
        OfficeKind::Word => {
            let doc = docs_engine::Document::open(full_path).ok()?;
            let mut out = Vec::new();
            out.push(format!(
                "@@ {} 文档内容预览（共 {} 段 · 点击右上角可在内置编辑器中打开） @@",
                if is_new { "新建 Word" } else { "更新后 Word" },
                doc.len()
            ));
            for i in 0..doc.len().min(200) {
                let p = doc.paragraph(i);
                let text = crate::office_host::clean_doc_para_text(&doc, i);
                if text.trim().is_empty() && p.cell.is_none() {
                    continue;
                }
                let style = p
                    .props
                    .style
                    .as_deref()
                    .map(|s| doc.styles.display_name(s))
                    .unwrap_or_else(|| "正文".to_owned());
                if let Some(c) = p.cell {
                    out.push(format!(
                        "{prefix}[表格#{} R{}C{}] {}",
                        c.table,
                        c.row + 1,
                        c.cell + 1,
                        text
                    ));
                } else if style.eq_ignore_ascii_case("normal") || style == "正文" {
                    out.push(format!("{prefix}{text}"));
                } else {
                    out.push(format!("{prefix}[{style}] {text}"));
                }
            }
            Some(out)
        }
        OfficeKind::Excel => {
            let book = sheets_engine::Workbook::open(full_path).ok()?;
            let names = book.sheet_names();
            let mut out = vec![format!(
                "@@ {} 工作簿预览（共 {} 个工作表） @@",
                if is_new { "新建 Excel" } else { "更新后 Excel" },
                names.len()
            )];
            for (s_ix, name) in names.iter().enumerate().take(4) {
                out.push(format!("@@ Sheet {}: {name} @@", s_ix + 1));
                for r in 0..30u32 {
                    let mut cells = Vec::new();
                    for c in 0..12u32 {
                        let val = book.display_value(s_ix as u32, r, c);
                        if !val.is_empty() {
                            cells.push(format!("{}: {val}", sheets_engine::cell_name(r, c)));
                        }
                    }
                    if !cells.is_empty() {
                        out.push(format!("{prefix}{}", cells.join("  |  ")));
                    }
                }
            }
            Some(out)
        }
        OfficeKind::Ppt => {
            let pres = slides_engine::Presentation::open(full_path).ok()?;
            let mut out = vec![format!(
                "@@ {} 演示文稿预览（共 {} 页幻灯片） @@",
                if is_new { "新建 PPT" } else { "更新后 PPT" },
                pres.len()
            )];
            for s in 0..pres.len().min(30) {
                out.push(format!("@@ 第 {} 页幻灯片 @@", s + 1));
                for sh in &pres.slide(s).shapes {
                    if let Some(txt) = sh.text.as_ref().map(|t| t.plain_text())
                        && !txt.trim().is_empty()
                    {
                        for line in txt.lines() {
                            out.push(format!("{prefix}{line}"));
                        }
                    }
                }
            }
            Some(out)
        }
        OfficeKind::Pdf => {
            let bytes = std::fs::read(full_path).ok()?;
            let pdf = pdf_engine::PdfDoc::open(Arc::new(bytes), "").ok()?;
            let mut out = vec![format!(
                "@@ PDF 文档预览（共 {} 页） @@",
                pdf.page_count()
            )];
            for p in 0..pdf.page_count().min(10) {
                let page_txt = pdf.text(p);
                if !page_txt.text.trim().is_empty() {
                    out.push(format!("@@ 第 {} 页 @@", p + 1));
                    for line in page_txt.text.lines().take(40) {
                        if !line.trim().is_empty() {
                            out.push(format!("{prefix}{line}"));
                        }
                    }
                }
            }
            Some(out)
        }
    }
}

pub(crate) fn file_patch(store: &ChatStore, key: &ReviewKey) -> Result<Vec<String>, String> {
    let history = store.history(key.chat).map_err(|err| err.to_string())?;
    if history
        .turns
        .get(key.index)
        .is_none_or(|turn| turn.id != key.turn)
    {
        return Err("这轮对话的历史记录已改变，请重新选择文件。".into());
    }
    let diff = store
        .turn_diff(key.chat, key.index, key.index + 1, false)
        .map_err(|err| err.to_string())?;
    let mut section = patch_for_path(&diff.diff, &key.path);
    if section.is_none() {
        section = diff.repositories.iter().flatten().find_map(|repo| {
            let relative = key.path.strip_prefix(&repo.relative_path).ok()?;
            patch_for_path(&repo.diff, relative)
        });
    }
    let section = section.ok_or_else(|| {
        if diff.diff.contains("\n[truncated]") {
            "本轮差异超出检查点输出上限，此文件的差异不可用。"
        } else {
            "检查点未提供此文件的差异。"
        }
        .to_owned()
    })?;
    let is_binary = section
        .lines()
        .any(|l| l.starts_with("Binary files ") || l.starts_with("GIT binary patch"));
    if is_binary && OfficeKind::for_path(&key.path).is_some() {
        let is_new = section.lines().any(|l| {
            l.starts_with("new file mode") || l.starts_with("Binary files /dev/null")
        });
        let full = history.chat.workspace.join(&key.path);
        if let Some(readable) = extract_office_readable_lines(&full, is_new) {
            return Ok(readable);
        }
    }

    // File identity is already visible in the header. Retain binary / rename / mode metadata.
    let mut lines = section.lines().skip(1).peekable();
    let mut result = Vec::new();
    while lines.peek().is_some_and(|l| !l.starts_with("@@")) {
        let line = lines.next().unwrap();
        if !line.starts_with("index ") && !line.starts_with("--- ") && !line.starts_with("+++ ") {
            if line.starts_with("Binary files ") {
                result.push(format!(
                    "二进制文件已变更：{}",
                    key.path.display()
                ));
            } else {
                result.push(line.to_owned());
            }
        }
    }
    result.extend(lines.map(str::to_owned));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use docs_engine::{Document, Paragraph};

    #[test]
    fn finds_only_the_named_file_including_deletions_and_quoted_names() {
        let patch = "diff --git a/one b/one\n+++ b/one\n@@ -0 +1 @@\n+one\ndiff --git \"a/a\\tfile\" \"b/a\\tfile\"\n--- \"a/a\\tfile\"\n+++ /dev/null\n@@ -1 +0 @@\n-deleted\ndiff --git a/my data b/my data\nBinary files a/my data and b/my data differ\n";
        let deleted = patch_for_path(patch, Path::new("a\tfile")).unwrap();
        assert!(
            deleted.contains("-deleted")
                && !deleted.contains("+one")
                && !deleted.contains("Binary files")
        );
        assert!(
            patch_for_path(patch, Path::new("my data"))
                .unwrap()
                .contains("Binary files")
        );
        assert!(patch_for_path(patch, Path::new("absent")).is_none());
    }

    #[test]
    fn extracts_human_readable_chinese_word_preview_instead_of_binary_diff() {
        let dir = tempfile::tempdir().unwrap();
        let docx_path = dir.path().join("阅读让生活更丰盈.docx");
        let mut doc = Document::from_paragraphs(vec![
            Paragraph::new("阅读让生活更丰盈").with_style("Title"),
            Paragraph::new("一、阅读拓宽精神世界").with_style("Heading1"),
            Paragraph::new("每一本书都是一扇通往广阔天地的窗口。"),
        ]);
        doc.save_as(&docx_path).unwrap();

        let lines = extract_office_readable_lines(&docx_path, true).unwrap();
        let joined = lines.join("\n");
        assert!(joined.contains("阅读让生活更丰盈"));
        assert!(joined.contains("一、阅读拓宽精神世界"));
        assert!(joined.contains("+每一本书都是一扇通往广阔天地的窗口。"));
        assert!(!joined.contains("Binary files"));
    }
}
