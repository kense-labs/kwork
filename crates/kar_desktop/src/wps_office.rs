//! WPS-grade Office lifecycle, typography, complex document inspection, table & page-setup
//! authoring, and 100% pure-Rust multimodal visual page preview (`docs_pdf` / `sheets_pdf` /
//! `slides_pdf` + `pdf_engine::Renderer`).

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine as _;
use docs_engine::{
    Align, CellRef, Document, Fonts, HfKind, HfType, Indent, LineRule, LineSpacing, ListKind,
    MarginPreset, NoteKind, Object, Orientation, PAGE_BREAK, PageGeometry, PaperSize,
    ParaProps, Position as DocPos, ResolvedLine, Rgb, RunProps, Selection as DocSel, TableAlign,
    TableEdit,
    agent::{RichParagraph, RichRun, TableBorderPreset},
    cm_to_twips, twips_to_cm,
};
use gpui::*;
use pdf_engine::{PdfDoc, Renderer as PdfRenderer, Turn as PdfTurn};
use serde_json::{Map, Value, json};

use crate::{
    mcp_office::McpToolOutput,
    office_host::{OfficeEditorView, OfficeTab, clean_doc_para_text},
    types::OfficeKind,
    ui_primitives::clean_display_doc_name,
    workbench::{KarWorkbench, MainStageView, RightPaneSurface},
};

static PDF_LAYOUT_ENGINE: OnceLock<Mutex<docs_pdf::Engine>> = OnceLock::new();

fn with_pdf_engine<R>(f: impl FnOnce(&mut docs_pdf::Engine) -> R) -> R {
    let mutex = PDF_LAYOUT_ENGINE.get_or_init(|| Mutex::new(docs_pdf::Engine::new()));
    let mut guard = mutex.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

/// Parse Chinese traditional Word/WPS font size names (`初号`..`八号`) into points (`pt`).
pub fn parse_zh_font_size_pt(name: &str) -> Option<f64> {
    match name.trim() {
        "初号" => Some(42.0),
        "小初" => Some(36.0),
        "一号" => Some(26.0),
        "小一" => Some(24.0),
        "二号" => Some(22.0),
        "小二" => Some(18.0),
        "三号" => Some(16.0),
        "小三" => Some(15.0),
        "四号" => Some(14.0),
        "小四" => Some(12.0),
        "五号" => Some(10.5),
        "小五" => Some(9.0),
        "六号" => Some(7.5),
        "小六" => Some(6.5),
        "七号" => Some(5.5),
        "八号" => Some(5.0),
        _ => None,
    }
}

fn resolve_size_pt(obj: &Value) -> Option<f64> {
    if let Some(pt) = obj.get("size_pt").and_then(Value::as_f64) {
        return Some(pt.clamp(4.0, 144.0));
    }
    if let Some(zh) = obj.get("size_zh").and_then(Value::as_str) {
        return parse_zh_font_size_pt(zh).or_else(|| zh.trim_end_matches("pt").parse::<f64>().ok());
    }
    if let Some(s) = obj.get("font_size").and_then(Value::as_f64) {
        return Some(s.clamp(4.0, 144.0));
    }
    if let Some(s) = obj.get("font_size").and_then(Value::as_str) {
        return parse_zh_font_size_pt(s).or_else(|| s.trim_end_matches("pt").parse::<f64>().ok());
    }
    None
}

pub fn parse_align_spec(raw: &str) -> Option<Align> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "left" | "start" | "左" | "左对齐" => Some(Align::Left),
        "center" | "centre" | "居中" | "中" => Some(Align::Center),
        "right" | "end" | "右" | "右对齐" => Some(Align::Right),
        "justify" | "both" | "两端对齐" | "分散对齐" => Some(Align::Justify),
        _ => None,
    }
}

fn align_name(a: Align) -> &'static str {
    match a {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
        Align::Justify => "justify",
    }
}

fn parse_rgb_hex(raw: &str) -> Option<Rgb> {
    let clean = raw.trim().trim_start_matches('#');
    Rgb::parse(clean)
}

fn build_fonts(font: Option<&str>, ascii_font: Option<&str>) -> Fonts {
    let mut out = Fonts::default();
    if let Some(f) = font.map(str::trim).filter(|s| !s.is_empty()) {
        let mapped = match f {
            "宋体" => "SimSun",
            "黑体" => "SimHei",
            "楷体" => "KaiTi",
            "仿宋" => "FangSong",
            "微软雅黑" => "Microsoft YaHei",
            "方正小标宋" | "小标宋" => "FZXiaoBiaoSong-B05S",
            other => other,
        };
        out = Fonts::family(mapped);
    }
    if let Some(asc) = ascii_font.map(str::trim).filter(|s| !s.is_empty()) {
        out.ascii = Some(asc.to_owned());
        out.h_ansi = Some(asc.to_owned());
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FallbackStyleHint {
    None,
    Subtitle,
    SubHeading(u8),
    Quote,
}

fn resolve_style_with_hint(
    doc: &Document,
    raw: &str,
) -> Result<(Option<String>, FallbackStyleHint), String> {
    let normalized = match raw.trim() {
        "正文" | "普通" | "normal" | "Normal" => "Normal",
        "标题" | "主标题" | "文档标题" => "Title",
        "副标题" | "subtitle" | "Subtitle" => "Subtitle",
        "一级标题" | "标题1" | "标题 1" | "h1" | "H1" => "Heading 1",
        "二级标题" | "标题2" | "标题 2" | "h2" | "H2" => "Heading 2",
        "三级标题" | "标题3" | "标题 3" | "h3" | "H3" => "Heading 3",
        "四级标题" | "标题4" | "标题 4" | "h4" | "H4" | "Heading4" | "Heading 4" => "Heading 4",
        "五级标题" | "标题5" | "标题 5" | "h5" | "H5" | "Heading5" | "Heading 5" => "Heading 5",
        "六级标题" | "标题6" | "标题 6" | "h6" | "H6" | "Heading6" | "Heading 6" => "Heading 6",
        "引用" | "引文" | "blockquote" | "quote" | "Quote" => "Quote",
        other => other,
    };
    match doc.find_style(normalized) {
        Ok(found) => Ok((found, FallbackStyleHint::None)),
        Err(_) if normalized == "Subtitle" => Ok((None, FallbackStyleHint::Subtitle)),
        Err(_) if normalized == "Quote" => Ok((None, FallbackStyleHint::Quote)),
        Err(_) if normalized == "Heading 4" => {
            Ok((doc.find_style("Heading 3").ok().flatten(), FallbackStyleHint::SubHeading(4)))
        }
        Err(_) if normalized == "Heading 5" => {
            Ok((doc.find_style("Heading 3").ok().flatten(), FallbackStyleHint::SubHeading(5)))
        }
        Err(_) if normalized == "Heading 6" => {
            Ok((doc.find_style("Heading 3").ok().flatten(), FallbackStyleHint::SubHeading(6)))
        }
        Err(e) => Err(e),
    }
}

fn resolve_style_alias(doc: &Document, raw: &str) -> Result<Option<String>, String> {
    resolve_style_with_hint(doc, raw).map(|(s, _)| s)
}

fn build_run_props(spec: &Value, base: Option<&RunProps>) -> RunProps {
    let mut rp = base.cloned().unwrap_or_default();
    let font = spec.get("font").and_then(Value::as_str);
    let ascii_font = spec.get("ascii_font").and_then(Value::as_str);
    let fonts = build_fonts(font, ascii_font);
    if !fonts.is_empty() {
        rp.fonts.overlay(&fonts);
    }
    if let Some(pt) = resolve_size_pt(spec) {
        rp.size = Some((pt * 2.0).round().max(8.0) as u32);
    }
    if let Some(b) = spec.get("bold").and_then(Value::as_bool) {
        rp.bold = Some(b);
    }
    if let Some(i) = spec.get("italic").and_then(Value::as_bool) {
        rp.italic = Some(i);
    }
    if let Some(u) = spec.get("underline").and_then(Value::as_bool) {
        rp.underline = Some(u);
    }
    if let Some(hex) = spec.get("color").and_then(Value::as_str)
        && let Some(rgb) = parse_rgb_hex(hex)
    {
        rp.color = Some(rgb);
    }
    if let Some(hl) = spec.get("highlight").and_then(Value::as_str) {
        let clean = hl.trim().to_ascii_lowercase();
        rp.extras.remove("highlight");
        if matches!(
            clean.as_str(),
            "yellow" | "green" | "cyan" | "magenta" | "blue" | "red" | "lightGray" | "lightgray"
        ) {
            let val = if clean == "lightgray" {
                "lightGray"
            } else {
                clean.as_str()
            };
            rp.extras.push(&format!("<w:highlight w:val=\"{val}\"/>"));
        }
    }
    rp
}

/// Parse simple inline Markdown (`**bold**`, `*italic*`) into styled `RichRun`s when `markdown: true`.
pub fn parse_inline_markdown_runs(text: &str, base: &RunProps) -> Vec<RichRun> {
    let mut out: Vec<RichRun> = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(after_open) = rest.strip_prefix("**")
            && let Some(end_idx) = after_open.find("**")
            && end_idx > 0
        {
            let inner = &after_open[..end_idx];
            let mut p = base.clone();
            p.bold = Some(true);
            out.push(RichRun {
                text: inner.to_owned(),
                props: p,
            });
            rest = &after_open[end_idx + 2..];
            continue;
        }
        if let Some(after_open) = rest.strip_prefix('*')
            && !after_open.starts_with('*')
            && let Some(end_idx) = after_open.find('*')
            && end_idx > 0
        {
            let inner = &after_open[..end_idx];
            let mut p = base.clone();
            p.italic = Some(true);
            out.push(RichRun {
                text: inner.to_owned(),
                props: p,
            });
            rest = &after_open[end_idx + 1..];
            continue;
        }
        let next_special = rest
            .char_indices()
            .skip(1)
            .find(|&(_, c)| c == '*')
            .map_or(rest.len(), |(idx, _)| idx);
        let plain = &rest[..next_special];
        if let Some(last) = out.last_mut()
            && last.props == *base
        {
            last.text.push_str(plain);
        } else {
            out.push(RichRun {
                text: plain.to_owned(),
                props: base.clone(),
            });
        }
        rest = &rest[next_special..];
    }
    if out.is_empty() {
        out.push(RichRun {
            text: String::new(),
            props: base.clone(),
        });
    }
    out
}

pub fn build_rich_paragraph(doc: &Document, item: &Value) -> Result<RichParagraph, String> {
    let obj = match item {
        Value::String(s) => {
            return Ok(RichParagraph {
                runs: vec![RichRun {
                    text: s.clone(),
                    props: RunProps::default(),
                }],
                props: ParaProps::default(),
                list: None,
                level: 0,
            });
        }
        Value::Object(_) => item,
        _ => return Err("段落项必须是字符串或对象".into()),
    };

    let mut pp = ParaProps::default();
    let mut style_hint = FallbackStyleHint::None;
    if let Some(style_raw) = obj.get("style").and_then(Value::as_str) {
        let (found, hint) = resolve_style_with_hint(doc, style_raw)?;
        pp.style = found;
        style_hint = hint;
    }
    match style_hint {
        FallbackStyleHint::Subtitle => {
            pp.align = Some(Align::Center);
            pp.after = Some(200);
            pp.contextual = Some(false);
        }
        FallbackStyleHint::Quote => {
            pp.indent.left = Some(cm_to_twips(0.8) as i32);
            pp.indent.right = Some(cm_to_twips(0.5) as i32);
            pp.before = Some(100);
            pp.after = Some(100);
            pp.contextual = Some(false);
        }
        FallbackStyleHint::SubHeading(_) => {
            pp.before = Some(120);
            pp.after = Some(60);
            pp.contextual = Some(false);
        }
        FallbackStyleHint::None => {}
    }
    if let Some(align_raw) = obj.get("align").and_then(Value::as_str) {
        pp.align = Some(
            parse_align_spec(align_raw)
                .ok_or_else(|| format!("不支持的对齐方式 `{align_raw}`"))?,
        );
    }
    if let Some(mult) = obj.get("line_spacing").and_then(Value::as_f64) {
        pp.line = Some(LineSpacing {
            line: (mult.clamp(0.8, 4.0) * 240.0).round() as i32,
            rule: LineRule::Auto,
        });
    } else if let Some(exact_pt) = obj.get("line_spacing_exact_pt").and_then(Value::as_f64) {
        pp.line = Some(LineSpacing {
            line: (exact_pt.clamp(8.0, 120.0) * 20.0).round() as i32,
            rule: LineRule::Exact,
        });
    } else if let Some(min_pt) = obj.get("line_spacing_at_least_pt").and_then(Value::as_f64) {
        pp.line = Some(LineSpacing {
            line: (min_pt.clamp(8.0, 120.0) * 20.0).round() as i32,
            rule: LineRule::AtLeast,
        });
    }

    if let Some(before_pt) = obj.get("space_before_pt").and_then(Value::as_f64) {
        pp.before = Some((before_pt.max(0.0) * 20.0).round() as u32);
        pp.contextual = Some(false);
    }
    if let Some(after_pt) = obj.get("space_after_pt").and_then(Value::as_f64) {
        pp.after = Some((after_pt.max(0.0) * 20.0).round() as u32);
        pp.contextual = Some(false);
    }

    let mut base_run = build_run_props(obj, None);
    match style_hint {
        FallbackStyleHint::Subtitle => {
            if base_run.size.is_none() {
                base_run.size = Some(26);
            }
            if base_run.color.is_none() {
                base_run.color = Rgb::parse("475569");
            }
        }
        FallbackStyleHint::Quote => {
            if base_run.fonts.is_empty() {
                base_run.fonts = build_fonts(Some("KaiTi"), None);
            }
            if base_run.color.is_none() {
                base_run.color = Rgb::parse("334155");
            }
        }
        FallbackStyleHint::SubHeading(_) => {
            if base_run.bold.is_none() {
                base_run.bold = Some(true);
            }
            if base_run.size.is_none() {
                base_run.size = Some(25);
            }
        }
        FallbackStyleHint::None => {}
    }
    let effective_pt = base_run
        .size
        .map(|half_pt| half_pt as f64 / 2.0)
        .unwrap_or(12.0);

    if let Some(chars) = obj.get("first_line_indent_chars").and_then(Value::as_f64) {
        pp.indent.first = Some((chars * effective_pt * 20.0).round() as i32);
    } else if let Some(cm) = obj.get("first_line_indent_cm").and_then(Value::as_f64) {
        let sign = if cm < 0.0 { -1 } else { 1 };
        pp.indent.first = Some(sign * cm_to_twips(cm.abs()) as i32);
    }
    if let Some(cm) = obj.get("left_indent_cm").and_then(Value::as_f64) {
        pp.indent.left = Some(cm_to_twips(cm.max(0.0)) as i32);
    }
    if let Some(cm) = obj.get("right_indent_cm").and_then(Value::as_f64) {
        pp.indent.right = Some(cm_to_twips(cm.max(0.0)) as i32);
    }
    if let Some(pb) = obj.get("page_break_before").and_then(Value::as_bool) {
        pp.page_break_before = Some(pb);
    }
    let is_heading = pp
        .style
        .as_deref()
        .is_some_and(|s| s.starts_with("Heading") || s == "Title" || s == "Subtitle")
        || matches!(
            style_hint,
            FallbackStyleHint::Subtitle | FallbackStyleHint::SubHeading(_)
        );
    let keep_next = obj
        .get("keep_next")
        .and_then(Value::as_bool)
        .unwrap_or(is_heading);
    if keep_next {
        pp.extras.remove("keepNext");
        pp.extras.push("<w:keepNext/>");
    }

    let list = match obj.get("list").and_then(Value::as_str) {
        None | Some("none") | Some("") => None,
        Some("bullet") | Some("unordered") => Some(ListKind::Bullet),
        Some("number") | Some("numbered") | Some("ordered") => Some(ListKind::Numbered),
        Some(other) => return Err(format!("不支持的列表类型 `{other}`（可选 `bullet` / `number`）")),
    };
    let level = obj
        .get("level")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(8) as u8;

    let parse_md = obj
        .get("markdown")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let runs = if let Some(arr) = obj.get("runs").and_then(Value::as_array) {
        let mut built = Vec::with_capacity(arr.len());
        for r_val in arr {
            match r_val {
                Value::String(s) => built.push(RichRun {
                    text: s.clone(),
                    props: base_run.clone(),
                }),
                Value::Object(_) => {
                    let txt = r_val
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    let rp = build_run_props(r_val, Some(&base_run));
                    built.push(RichRun {
                        text: txt,
                        props: rp,
                    });
                }
                _ => {}
            }
        }
        built
    } else {
        let raw_text = obj
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if parse_md {
            parse_inline_markdown_runs(raw_text, &base_run)
        } else {
            vec![RichRun {
                text: raw_text.to_owned(),
                props: base_run,
            }]
        }
    };

    Ok(RichParagraph {
        runs,
        props: pp,
        list,
        level,
    })
}

/// Apply a cohesive WPS-level typography & page setup preset across a Word document.
pub fn apply_word_preset(
    doc: &mut Document,
    preset: &str,
    sel: DocSel,
) -> Result<&'static str, String> {
    let key = preset.trim().to_ascii_lowercase();
    match key.as_str() {
        "official_zh" | "gongwen" | "公文" => {
            // GB/T 9704 公文标准：A4，上 3.7cm 下 3.5cm 左 2.8cm 右 2.6cm，页码，二号标题居中，三号一级标题，仿宋三号 28磅行距 首行缩进2字符
            let (w, h) = PaperSize::A4.twips();
            doc.set_page(
                PageGeometry {
                    width: w,
                    height: h,
                    margin_top: cm_to_twips(3.7),
                    margin_bottom: cm_to_twips(3.5),
                    margin_left: cm_to_twips(2.8),
                    margin_right: cm_to_twips(2.6),
                },
                sel,
            );
            doc.add_page_numbers(0, "— {PAGE} —", sel);
            style_existing_paragraphs(
                doc,
                ("FZXiaoBiaoSong-B05S", "Times New Roman", 44, true), // 二号 22pt
                ("SimHei", "Times New Roman", 32, false),             // 三号 16pt
                ("KaiTi", "Times New Roman", 32, true),               // 三号楷体加粗
                ("FangSong", "Times New Roman", 32),                  // 三号仿宋 16pt
                LineSpacing {
                    line: 560, // 固定值 28 磅
                    rule: LineRule::Exact,
                },
                640, // 2 chars * 16pt * 20 = 640 twips
                0,
                0,
                sel,
            );
            Ok("official_zh")
        }
        "academic" | "paper" | "论文" => {
            let (w, h) = PaperSize::A4.twips();
            doc.set_page(
                PageGeometry {
                    width: w,
                    height: h,
                    margin_top: cm_to_twips(2.54),
                    margin_bottom: cm_to_twips(2.54),
                    margin_left: cm_to_twips(3.0),
                    margin_right: cm_to_twips(2.5),
                },
                sel,
            );
            doc.add_page_numbers(0, "第 {PAGE} 页 / 共 {PAGES} 页", sel);
            style_existing_paragraphs(
                doc,
                ("SimHei", "Times New Roman", 36, true),  // 小二 18pt
                ("SimHei", "Times New Roman", 30, true),  // 小三 15pt
                ("SimHei", "Times New Roman", 28, true),  // 四号 14pt
                ("SimSun", "Times New Roman", 24),        // 小四 12pt
                LineSpacing {
                    line: 360, // 1.5 倍行距
                    rule: LineRule::Auto,
                },
                480, // 2 chars * 12pt * 20 = 480 twips
                120,
                80,
                sel,
            );
            Ok("academic")
        }
        "modern_business" | "business" | "商务" => {
            let (w, h) = PaperSize::A4.twips();
            doc.set_page(
                PageGeometry {
                    width: w,
                    height: h,
                    margin_top: cm_to_twips(2.4),
                    margin_bottom: cm_to_twips(2.4),
                    margin_left: cm_to_twips(2.4),
                    margin_right: cm_to_twips(2.4),
                },
                sel,
            );
            doc.add_page_numbers(0, "第 {PAGE} 页 / 共 {PAGES} 页", sel);
            style_existing_paragraphs(
                doc,
                ("PingFang SC", "Helvetica Neue", 40, true), // 20pt
                ("PingFang SC", "Helvetica Neue", 30, true), // 15pt
                ("PingFang SC", "Helvetica Neue", 26, true), // 13pt
                ("PingFang SC", "Helvetica Neue", 22),       // 11pt
                LineSpacing {
                    line: 360,
                    rule: LineRule::Auto,
                },
                0, // Modern business uses clean block spacing without first-line indent
                140,
                120,
                sel,
            );
            Ok("modern_business")
        }
        "clean_article" | "article" | "文章" | "" => {
            let (w, h) = PaperSize::A4.twips();
            doc.set_page(
                PageGeometry {
                    width: w,
                    height: h,
                    margin_top: cm_to_twips(2.6),
                    margin_bottom: cm_to_twips(2.6),
                    margin_left: cm_to_twips(2.8),
                    margin_right: cm_to_twips(2.8),
                },
                sel,
            );
            doc.add_page_numbers(0, "第 {PAGE} 页 / 共 {PAGES} 页", sel);
            style_existing_paragraphs(
                doc,
                ("PingFang SC", "Georgia", 40, true), // 20pt centered title
                ("PingFang SC", "Georgia", 30, true), // 15pt H1
                ("PingFang SC", "Georgia", 26, true), // 13pt H2
                ("Songti SC", "Georgia", 24),         // 12pt 小四正文
                LineSpacing {
                    line: 384, // 1.6 倍舒适阅读行距
                    rule: LineRule::Auto,
                },
                480, // 首行缩进 2 字符 (24pt = 480 twips)
                160,
                100,
                sel,
            );
            Ok("clean_article")
        }
        other => Err(format!(
            "未知排版预设 `{other}`（可选：`clean_article`、`official_zh`、`academic`、`modern_business`）"
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn style_existing_paragraphs(
    doc: &mut Document,
    title_spec: (&str, &str, u32, bool),
    h1_spec: (&str, &str, u32, bool),
    h2_spec: (&str, &str, u32, bool),
    body_spec: (&str, &str, u32),
    body_line: LineSpacing,
    body_first_twips: i32,
    heading_before: u32,
    body_after: u32,
    sel: DocSel,
) {
    let total = doc.len();
    if total == 0 || (total == 1 && doc.paragraph(0).text.is_empty()) {
        return;
    }
    for idx in 0..total {
        let p = doc.paragraph(idx).clone();
        if p.cell.is_some() {
            continue;
        }
        let style_id = p.props.style.as_deref().unwrap_or("Normal");
        let p_sel = DocSel {
            anchor: DocPos::new(idx, 0),
            head: DocPos::new(idx, p.text.len()),
        };
        let is_list = doc.list_kind(idx).is_some();
        match style_id {
            "Title" => {
                doc.format_paragraphs(p_sel, |pp| {
                    pp.align = Some(Align::Center);
                    pp.before = Some(120);
                    pp.after = Some(240);
                    pp.indent = Indent {
                        left: Some(0),
                        right: Some(0),
                        first: Some(0),
                    };
                    pp.contextual = Some(false);
                    pp.extras.remove("keepNext");
                    pp.extras.push("<w:keepNext/>");
                });
                doc.format(p_sel, |rp| {
                    rp.fonts = build_fonts(Some(title_spec.0), Some(title_spec.1));
                    rp.size = Some(title_spec.2);
                    rp.bold = Some(title_spec.3);
                });
            }
            "Subtitle" => {
                doc.format_paragraphs(p_sel, |pp| {
                    pp.align = Some(Align::Center);
                    pp.before = Some(0);
                    pp.after = Some(220);
                    pp.indent = Indent {
                        left: Some(0),
                        right: Some(0),
                        first: Some(0),
                    };
                    pp.contextual = Some(false);
                    pp.extras.remove("keepNext");
                    pp.extras.push("<w:keepNext/>");
                });
                doc.format(p_sel, |rp| {
                    rp.fonts = build_fonts(Some(body_spec.0), Some(body_spec.1));
                    rp.size = Some(24);
                    rp.color = Rgb::parse("475569");
                });
            }
            "Heading1" => {
                doc.format_paragraphs(p_sel, |pp| {
                    pp.align = Some(Align::Left);
                    pp.before = Some(heading_before.max(200));
                    pp.after = Some(120);
                    pp.indent = Indent {
                        left: Some(0),
                        right: Some(0),
                        first: Some(0),
                    };
                    pp.contextual = Some(false);
                    pp.extras.remove("keepNext");
                    pp.extras.push("<w:keepNext/>");
                });
                doc.format(p_sel, |rp| {
                    rp.fonts = build_fonts(Some(h1_spec.0), Some(h1_spec.1));
                    rp.size = Some(h1_spec.2);
                    rp.bold = Some(h1_spec.3);
                });
            }
            s if s.starts_with("Heading") => {
                doc.format_paragraphs(p_sel, |pp| {
                    pp.align = Some(Align::Left);
                    pp.before = Some(heading_before.max(140));
                    pp.after = Some(80);
                    pp.indent = Indent {
                        left: Some(0),
                        right: Some(0),
                        first: Some(0),
                    };
                    pp.contextual = Some(false);
                    pp.extras.remove("keepNext");
                    pp.extras.push("<w:keepNext/>");
                });
                doc.format(p_sel, |rp| {
                    rp.fonts = build_fonts(Some(h2_spec.0), Some(h2_spec.1));
                    rp.size = Some(h2_spec.2);
                    rp.bold = Some(h2_spec.3);
                });
            }
            "Quote" => {
                doc.format_paragraphs(p_sel, |pp| {
                    pp.align = Some(Align::Justify);
                    pp.before = Some(100);
                    pp.after = Some(100);
                    pp.line = Some(body_line);
                    pp.indent = Indent {
                        left: Some(cm_to_twips(0.8) as i32),
                        right: Some(cm_to_twips(0.5) as i32),
                        first: Some(0),
                    };
                });
                doc.format(p_sel, |rp| {
                    rp.fonts = build_fonts(Some("KaiTi"), Some(body_spec.1));
                    rp.size = Some(body_spec.2);
                    rp.color = Rgb::parse("334155");
                });
            }
            _ => {
                doc.format_paragraphs(p_sel, |pp| {
                    pp.align = Some(Align::Justify);
                    pp.before = Some(0);
                    pp.after = Some(body_after);
                    pp.line = Some(body_line);
                    if !is_list {
                        pp.indent = Indent {
                            left: Some(0),
                            right: Some(0),
                            first: Some(body_first_twips),
                        };
                    }
                    pp.contextual = Some(false);
                });
                doc.format(p_sel, |rp| {
                    if rp.fonts.is_empty() {
                        rp.fonts = build_fonts(Some(body_spec.0), Some(body_spec.1));
                    }
                    if rp.size.is_none() {
                        rp.size = Some(body_spec.2);
                    }
                });
            }
        }
    }
    let _ = sel;
}

fn sanitize_filename_stem(raw: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
            {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "未命名文档".into()
    } else {
        trimmed.chars().take(80).collect()
    }
}

fn resolve_workspace_path(workspace: &Path, raw_path: &str, expected_ext: Option<&str>) -> PathBuf {
    let mut p = PathBuf::from(raw_path.trim());
    if !p.is_absolute() {
        p = workspace.join(p);
    }
    if let Some(ext) = expected_ext
        && p.extension().and_then(|e| e.to_str()).is_none()
    {
        p.set_extension(ext);
    }
    p
}

fn unique_path_in_dir(dir: &Path, stem: &str, ext: &str, overwrite: bool) -> PathBuf {
    let base = dir.join(format!("{stem}.{ext}"));
    if overwrite || !base.exists() {
        return base;
    }
    for n in 2..1000 {
        let candidate = dir.join(format!("{stem} ({n}).{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    base
}

fn raster_to_png_bytes(raster: &pdf_engine::Raster) -> Result<Vec<u8>, String> {
    let img = image::RgbaImage::from_raw(raster.width, raster.height, raster.rgba.clone())
        .ok_or_else(|| "无法构建页面光栅图像缓冲区".to_owned())?;
    let mut png_bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .map_err(|e| format!("PNG 编码失败：{e}"))?;
    Ok(png_bytes)
}

pub struct RenderedPagePng {
    pub png_bytes: Vec<u8>,
    pub total_pages: usize,
    pub size_px: (u32, u32),
}

pub fn render_pdf_page_to_png(
    pdf_bytes: Vec<u8>,
    page_1based: usize,
    dpi: u32,
) -> Result<RenderedPagePng, String> {
    let pdf = PdfDoc::open(Arc::new(pdf_bytes), "")
        .map_err(|e| format!("打开预览 PDF 失败：{e}"))?;
    let total = pdf.page_count().max(1);
    let idx = page_1based.saturating_sub(1).min(total - 1);
    let scale = (dpi.clamp(72, 216) as f32) / 72.0;
    let renderer = PdfRenderer::new(&pdf);
    let raster = renderer
        .render(idx, scale, PdfTurn(0))
        .ok_or_else(|| format!("渲染第 {} 页失败", idx + 1))?;
    let dims = (raster.width, raster.height);
    let png = raster_to_png_bytes(&raster)?;
    Ok(RenderedPagePng {
        png_bytes: png,
        total_pages: total,
        size_px: dims,
    })
}

/// Inspect a `docs_engine::Document` deeply (page setup, sections, headers/footers, outline,
/// tables, images, floating text boxes, footnotes, comments, and page count).
pub fn inspect_word_document(doc: &Document, tab_id: u64, doc_name: &str) -> Value {
    let (page_count, orphan_warnings) = with_pdf_engine(|engine| {
        let laid = docs_pdf::lay_out(doc, engine);
        let pages = laid.paged.pages.len().max(1);
        let mut warnings = Vec::new();
        for (p_ix, page) in laid.paged.pages.iter().enumerate() {
            if let Some(last_line) = page.lines.iter().rfind(|l| !l.repeat) {
                let para = doc.paragraph(last_line.para);
                let is_heading = para
                    .props
                    .style
                    .as_deref()
                    .is_some_and(|s| s.starts_with("Heading"));
                let next_on_next_page = laid
                    .paged
                    .pages
                    .get(p_ix + 1)
                    .and_then(|np| np.lines.iter().find(|l| !l.repeat))
                    .is_some_and(|nl| nl.para != last_line.para);
                if is_heading && next_on_next_page {
                    warnings.push(format!(
                        "第 {} 页底端标题段落 #{}（{}）与下文跨页分离，建议开启 keep_next",
                        p_ix + 1,
                        last_line.para + 1,
                        clean_doc_para_text(doc, last_line.para)
                    ));
                }
            }
        }
        (pages, warnings)
    });

    let pg = doc.section_page(0);
    let paper = PaperSize::of(&pg).map_or("Custom", PaperSize::label);
    let orientation = if pg.width > pg.height {
        "landscape"
    } else {
        "portrait"
    };
    let round2 = |v: f64| (v * 100.0).round() / 100.0;

    let sections = doc.sections();
    let mut hf_summary = Vec::new();
    for si in 0..sections.len() {
        let (h_txt, _) = doc.hf_text(si, HfKind::Header, HfType::Default);
        let (f_txt, _) = doc.hf_text(si, HfKind::Footer, HfType::Default);
        if !h_txt.is_empty() || !f_txt.is_empty() {
            hf_summary.push(json!({
                "section": si + 1,
                "header": h_txt,
                "footer": f_txt,
            }));
        }
    }

    let mut outline = Vec::new();
    let mut inline_images = 0usize;
    let mut floating_objects = Vec::new();
    for idx in 0..doc.len() {
        let p = doc.paragraph(idx);
        if let Some(style_id) = p.props.style.as_deref()
            && (style_id == "Title" || style_id == "Subtitle" || style_id.starts_with("Heading"))
        {
            outline.push(json!({
                "paragraph": idx + 1,
                "style": doc.styles.display_name(style_id),
                "text": clean_doc_para_text(doc, idx),
            }));
        }
        for (_, obj) in p.objects() {
            if matches!(obj, Object::Image(_)) {
                inline_images += 1;
            }
            if let Some(fl) = obj.float() {
                let box_text = fl
                    .text
                    .as_ref()
                    .map(|tb| {
                        (0..tb.doc.len())
                            .map(|i| clean_doc_para_text(&tb.doc, i))
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                floating_objects.push(json!({
                    "anchor_paragraph": idx + 1,
                    "has_picture": matches!(fl.kind, docs_engine::FloatKind::Picture { .. }),
                    "text": box_text,
                }));
            }
        }
    }

    let mut tables = Vec::new();
    for tid in doc.table_ids() {
        if let Some(t) = doc.table(tid) {
            let range = doc.table_range(tid);
            let rows_n = t.rows.len();
            let cols_n = t.columns();
            let mut matrix: Vec<Vec<String>> = Vec::with_capacity(rows_n.min(15));
            for (r, row_entry) in t.rows.iter().enumerate().take(15) {
                let mut row_cells = Vec::new();
                for c in 0..row_entry.cells.len() {
                    let cref = CellRef {
                        table: tid,
                        row: r as u32,
                        cell: c as u32,
                    };
                    let txt: Vec<String> = (range.start..range.end)
                        .filter(|&pi| doc.paragraph(pi).cell == Some(cref))
                        .map(|pi| clean_doc_para_text(doc, pi))
                        .collect();
                    row_cells.push(txt.join("\n"));
                }
                matrix.push(row_cells);
            }
            tables.push(json!({
                "table_id": tid,
                "start_paragraph": range.start + 1,
                "end_paragraph": range.end,
                "rows": rows_n,
                "cols": cols_n,
                "has_header_row": t.rows.first().is_some_and(|r| r.header),
                "has_merges": t.has_merges(),
                "preview_cells": matrix,
            }));
        }
    }

    let notes: Vec<Value> = doc
        .notes()
        .iter()
        .filter(|n| !n.separator)
        .take(30)
        .map(|n| {
            let txt: Vec<String> = (0..n.doc.len())
                .map(|i| clean_doc_para_text(&n.doc, i))
                .filter(|s| !s.is_empty())
                .collect();
            json!({
                "id": n.id,
                "kind": match n.kind {
                    NoteKind::Footnote => "footnote",
                    NoteKind::Endnote => "endnote",
                },
                "text": txt.join(" "),
            })
        })
        .collect();

    let comments: Vec<Value> = doc
        .comments()
        .iter()
        .take(30)
        .map(|c| {
            json!({
                "id": c.id,
                "author": c.author,
                "text": c.text,
                "resolved": c.done,
            })
        })
        .collect();

    json!({
        "tab_id": tab_id,
        "document": doc_name,
        "path": doc.path().map(|p| p.display().to_string()),
        "needs_save_as": doc.needs_save_as(),
        "is_dirty": doc.is_dirty(),
        "page_count": page_count,
        "total_paragraphs": doc.len(),
        "word_count": doc.word_count(),
        "char_count": doc.char_count(),
        "available_styles": doc.paragraph_style_names(),
        "page_setup": {
            "paper": paper,
            "orientation": orientation,
            "width_cm": round2(twips_to_cm(pg.width)),
            "height_cm": round2(twips_to_cm(pg.height)),
            "margins_cm": {
                "top": round2(twips_to_cm(pg.margin_top)),
                "bottom": round2(twips_to_cm(pg.margin_bottom)),
                "left": round2(twips_to_cm(pg.margin_left)),
                "right": round2(twips_to_cm(pg.margin_right)),
            },
            "section_count": sections.len(),
            "headers_and_footers": hf_summary,
        },
        "outline": outline,
        "tables": tables,
        "inline_images_count": inline_images,
        "floating_objects": floating_objects,
        "notes": notes,
        "comments": comments,
        "layout_warnings": orphan_warnings,
    })
}

/// Format rich paragraph metadata for `office_read_word`.
pub fn format_word_paragraph_entry(doc: &Document, idx: usize) -> Value {
    let p = doc.paragraph(idx);
    let style_id = p.props.style.as_deref().unwrap_or("Normal");
    let style_display = if style_id == "Normal" {
        "Normal".to_owned()
    } else {
        doc.styles.display_name(style_id)
    };
    let rp = doc.resolve_para(p);
    let rr = doc.resolved_at(DocPos::new(idx, 0));
    let line_desc = match rp.line {
        ResolvedLine::Multiple(m) => json!({ "rule": "multiple", "value": (m * 100.0).round() / 100.0 }),
        ResolvedLine::Exact(pt) => json!({ "rule": "exact_pt", "value": (pt * 10.0).round() / 10.0 }),
        ResolvedLine::AtLeast(pt) => json!({ "rule": "at_least_pt", "value": (pt * 10.0).round() / 10.0 }),
    };
    let mut entry = json!({
        "paragraph": idx + 1,
        "style_id": style_id,
        "style": style_display,
        "text": clean_doc_para_text(doc, idx),
        "align": align_name(rp.align),
        "font": rr.font,
        "size_pt": (rr.size_pt * 10.0).round() / 10.0,
        "bold": rr.bold,
        "italic": rr.italic,
        "first_line_indent_pt": (rp.first_line_pt * 10.0).round() / 10.0,
        "line_spacing": line_desc,
    });
    if let Some(kind) = doc.list_kind(idx) {
        let level = doc.styles.effective_num(p).map_or(0, |n| n.level);
        entry["list"] = json!({
            "kind": match kind {
                ListKind::Bullet => "bullet",
                ListKind::Numbered => "number",
            },
            "level": level,
        });
    }
    if let Some(c) = p.cell {
        entry["table_cell"] = json!({
            "table_id": c.table,
            "row": c.row + 1,
            "col": c.cell + 1,
        });
    }
    if p.text.contains(PAGE_BREAK) || rp.page_break_before {
        entry["page_break"] = json!(true);
    }
    let img_count = p
        .objects()
        .filter(|(_, o)| matches!(o, Object::Image(_)))
        .count();
    if img_count > 0 {
        entry["images"] = json!(img_count);
    }
    entry
}

impl KarWorkbench {
    pub(crate) fn exec_create_document(
        &mut self,
        args: &Map<String, Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let kind_raw = args
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("word")
            .trim()
            .to_ascii_lowercase();
        let (kind, ext) = match kind_raw.as_str() {
            "word" | "docx" | "doc" => (OfficeKind::Word, "docx"),
            "excel" | "xlsx" | "sheet" | "sheets" => (OfficeKind::Excel, "xlsx"),
            "ppt" | "pptx" | "slides" | "powerpoint" => (OfficeKind::Ppt, "pptx"),
            other => {
                return Err(format!(
                    "不支持的文档类型 `{other}`（可选：`word`、`excel`、`ppt`）"
                ));
            }
        };

        let untitled = args
            .get("untitled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let overwrite = args
            .get("overwrite")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let title = args
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let preset = args
            .get("preset")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());

        if untitled && args.get("path").and_then(Value::as_str).is_none() {
            self.open_blank_office_tab(kind, window, cx);
            let tab = self
                .active_office_tab
                .and_then(|i| self.office_tabs.get(i))
                .cloned()
                .ok_or_else(|| "创建空白文档标签失败".to_owned())?;
            if let (OfficeKind::Word, Some(preset_name)) = (kind, preset)
                && let OfficeEditorView::Word(ref e) = tab.view
            {
                e.update(cx, |v, cx| {
                    v.agent_edit("应用排版预设", window, cx, |doc, sel| {
                        apply_word_preset(doc, preset_name, sel).map_err(anyhow::Error::msg)?;
                        Ok(((), sel))
                    })
                })
                .map_err(|e| e.to_string())?;
            }
            return serde_json::to_string_pretty(&json!({
                "status": "created",
                "tab_id": tab.id,
                "kind": kind.short_tag(),
                "name": clean_display_doc_name(&tab.doc_name(cx)),
                "path": Value::Null,
                "needs_save_as": true,
            }))
            .map_err(|e| e.to_string());
        }

        let workspace = self.effective_workspace_dir();
        let _ = std::fs::create_dir_all(&workspace);
        let dest = if let Some(raw_p) = args
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
        {
            let resolved = resolve_workspace_path(&workspace, raw_p, Some(ext));
            if resolved.exists() && !overwrite {
                return Err(format!(
                    "目标路径已存在文件 `{}`；如需覆盖请传 `overwrite: true` 或更换文件名",
                    resolved.display()
                ));
            }
            resolved
        } else {
            let stem = sanitize_filename_stem(title.unwrap_or("未命名文档"));
            unique_path_in_dir(&workspace, &stem, ext, overwrite)
        };

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("无法创建目录：{e}"))?;
        }

        match kind {
            OfficeKind::Word => {
                let mut doc = Document::new();
                let sel = DocSel::caret(DocPos::new(0, 0));
                let applied = apply_word_preset(&mut doc, preset.unwrap_or("clean_article"), sel)?;
                if let Some(t) = title {
                    let mut p = build_rich_paragraph(
                        &doc,
                        &json!({
                            "style": "Title",
                            "align": "center",
                            "text": t,
                        }),
                    )?;
                    if applied == "official_zh" {
                        for r in &mut p.runs {
                            r.props.fonts =
                                build_fonts(Some("FZXiaoBiaoSong-B05S"), Some("Times New Roman"));
                            r.props.size = Some(44);
                            r.props.bold = Some(true);
                        }
                    } else {
                        for r in &mut p.runs {
                            r.props.size = Some(40);
                            r.props.bold = Some(true);
                        }
                    }
                    doc.insert_rich_paragraphs(None, true, &[p], sel)?;
                }
                doc.save_as(&dest).map_err(|e| format!("保存初始文档失败：{e}"))?;
            }
            OfficeKind::Excel => {
                let mut book = sheets_engine::Workbook::new_empty()
                    .map_err(|e| format!("初始化空白工作簿失败：{e}"))?;
                book.save_as(&dest)
                    .map_err(|e| format!("保存初始工作簿失败：{e}"))?;
            }
            OfficeKind::Ppt => {
                let mut pres = slides_engine::Presentation::new();
                pres.save_as(&dest)
                    .map_err(|e| format!("保存初始演示文稿失败：{e}"))?;
            }
            OfficeKind::Pdf => unreachable!(),
        }

        self.open_office_file_tab(dest.clone(), false, None, window, cx);
        let tab = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .cloned()
            .ok_or_else(|| "在内置编辑器中打开新建文档失败".to_owned())?;
        let doc_name = clean_display_doc_name(&tab.doc_name(cx));
        let rev = self.fence_ledger.current_revision(&doc_name);

        serde_json::to_string_pretty(&json!({
            "status": "created",
            "tab_id": tab.id,
            "kind": kind.short_tag(),
            "name": doc_name,
            "path": dest.display().to_string(),
            "preset": preset.unwrap_or("clean_article"),
            "revision": rev,
            "needs_save_as": false,
        }))
        .map_err(|e| e.to_string())
    }

    pub(crate) fn exec_open_document(
        &mut self,
        args: &Map<String, Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let raw_path = args
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "缺少 `path` 参数".to_owned())?;
        let workspace = self.effective_workspace_dir();
        let resolved = resolve_workspace_path(&workspace, raw_path, None);
        if !resolved.is_file() {
            return Err(format!("未找到本地文档 `{}`", resolved.display()));
        }
        if OfficeKind::for_path(&resolved).is_none() {
            return Err(format!(
                "不支持作为 Office / PDF 文档打开：`{}`",
                resolved.display()
            ));
        }
        self.open_office_file_tab(resolved.clone(), false, None, window, cx);
        let canonical = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());
        let tab = self
            .office_tabs
            .iter()
            .find(|t| {
                !t.is_demo
                    && t.path(cx)
                        .map(|p| p.canonicalize().unwrap_or(p) == canonical)
                        .unwrap_or(false)
            })
            .cloned()
            .ok_or_else(|| format!("打开文档 `{}` 失败", resolved.display()))?;
        let doc_name = clean_display_doc_name(&tab.doc_name(cx));
        let rev = self.fence_ledger.current_revision(&doc_name);
        serde_json::to_string_pretty(&json!({
            "status": "opened",
            "tab_id": tab.id,
            "kind": tab.kind.short_tag(),
            "name": doc_name,
            "path": tab.path(cx).map(|p| p.display().to_string()),
            "needs_save_as": tab.needs_save_as(cx),
            "revision": rev,
        }))
        .map_err(|e| e.to_string())
    }

    pub(crate) fn exec_save_tab_upgraded(
        &mut self,
        tab: &OfficeTab,
        args: &Map<String, Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        if tab.is_demo {
            return Err("演示模式文档仅用于演示，禁止直接覆盖真实文件".into());
        }
        let ext = match tab.kind {
            OfficeKind::Word => "docx",
            OfficeKind::Excel => "xlsx",
            OfficeKind::Ppt => "pptx",
            OfficeKind::Pdf => "pdf",
        };
        let workspace = self.effective_workspace_dir();

        if let Some(raw_path) = args
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
        {
            let dest = resolve_workspace_path(&workspace, raw_path, Some(ext));
            let overwrite = args
                .get("overwrite")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if dest.exists()
                && !overwrite
                && tab
                    .path(cx)
                    .map(|p| p != dest)
                    .unwrap_or(true)
            {
                return Err(format!(
                    "路径 `{}` 已存在；如确认覆盖请传 `overwrite: true`",
                    dest.display()
                ));
            }
            let detail = tab
                .save_copy_to(&dest, window, cx)
                .map_err(|e| e.to_string())?;
            return serde_json::to_string_pretty(&json!({
                "status": "saved",
                "tab_id": tab.id,
                "kind": tab.kind.short_tag(),
                "path": dest.display().to_string(),
                "detail": detail,
            }))
            .map_err(|e| e.to_string());
        }

        if args
            .get("prompt_user")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            let suggested = self.suggest_tab_filename(tab, ext, cx);
            let receiver = cx.prompt_for_new_path(&workspace, Some(&suggested));
            let tab_clone = tab.clone();
            cx.spawn_in(window, async move |this, cx| {
                let Ok(Ok(Some(dest))) = receiver.await else {
                    return;
                };
                let _ = this.update_in(cx, |wb, win, cx| {
                    match tab_clone.save_copy_to(&dest, win, cx) {
                        Ok(_) => wb.notify_toast(format!("已保存至 {}", dest.display()), cx),
                        Err(err) => wb.notify_warning(format!("保存失败：{err:#}"), cx),
                    }
                });
            })
            .detach();
            return serde_json::to_string_pretty(&json!({
                "status": "save_dialog_opened",
                "tab_id": tab.id,
                "suggested_filename": suggested,
                "message": "已在 Kwrok 中为用户弹出原生另存为对话框",
            }))
            .map_err(|e| e.to_string());
        }

        if tab.needs_save_as(cx) {
            let suggested_name = self.suggest_tab_filename(tab, ext, cx);
            let suggested_path = unique_path_in_dir(
                &workspace,
                Path::new(&suggested_name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("未命名文档"),
                ext,
                false,
            );
            return serde_json::to_string_pretty(&json!({
                "status": "NEEDS_SAVE_AS",
                "tab_id": tab.id,
                "kind": tab.kind.short_tag(),
                "suggested_path": suggested_path.display().to_string(),
                "message": "当前文档尚未绑定磁盘路径。请再次调用 `office_save_tab` 并传入 `path`（例如 `suggested_path`），或传入 `prompt_user: true` 唤起保存对话框。",
            }))
            .map_err(|e| e.to_string());
        }

        let detail = tab.save_in_place(window, cx).map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&json!({
            "status": "saved",
            "tab_id": tab.id,
            "kind": tab.kind.short_tag(),
            "path": tab.path(cx).map(|p| p.display().to_string()),
            "detail": detail,
        }))
        .map_err(|e| e.to_string())
    }

    fn suggest_tab_filename(&self, tab: &OfficeTab, ext: &str, cx: &App) -> String {
        if let OfficeEditorView::Word(ref e) = tab.view {
            let heading = e.read(cx).agent_read(|doc, _| {
                (0..doc.len().min(5))
                    .map(|i| clean_doc_para_text(doc, i))
                    .find(|t| !t.trim().is_empty())
            });
            if let Some(first_line) = heading {
                let stem = sanitize_filename_stem(&first_line);
                if stem != "未命名文档" {
                    return format!("{stem}.{ext}");
                }
            }
        }
        tab.default_save_filename(cx)
    }

    pub(crate) fn exec_format_word(
        &mut self,
        tab: &OfficeTab,
        args: &Map<String, Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let OfficeEditorView::Word(ref e) = tab.view else {
            return Err("不是 Word 文档".into());
        };
        if let Some(preset) = args
            .get("preset")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
        {
            let applied = e
                .update(cx, |v, cx| {
                    v.agent_edit("应用文档排版预设", window, cx, |doc, sel| {
                        let p = apply_word_preset(doc, preset, sel).map_err(anyhow::Error::msg)?;
                        Ok((p, sel))
                    })
                })
                .map_err(|e| e.to_string())?;
            return serde_json::to_string_pretty(&json!({
                "status": "preset_applied",
                "tab_id": tab.id,
                "preset": applied,
            }))
            .map_err(|e| e.to_string());
        }

        let start_1 = args
            .get("start_paragraph")
            .or_else(|| args.get("paragraph"))
            .and_then(Value::as_u64)
            .ok_or_else(|| "请提供 `start_paragraph`（或 `paragraph`）或 `preset`".to_owned())?
            .max(1) as usize;
        let end_1 = args
            .get("end_paragraph")
            .and_then(Value::as_u64)
            .map(|v| v.max(start_1 as u64) as usize)
            .unwrap_or(start_1);
        let spec_val = Value::Object(args.clone());

        let formatted_range = e
            .update(cx, |v, cx| {
                v.agent_edit("智能体设置段落与字体排版", window, cx, |doc, _sel| {
                    let total = doc.len();
                    if start_1 > total {
                        anyhow::bail!("起始段落 #{start_1} 超出总段数 {total}");
                    }
                    let s0 = start_1 - 1;
                    let e0 = (end_1 - 1).min(total - 1);
                    let end_off = doc.paragraph(e0).text.len();
                    let range_sel = DocSel {
                        anchor: DocPos::new(s0, 0),
                        head: DocPos::new(e0, end_off),
                    };

                    if let Some(style_raw) = args.get("style").and_then(Value::as_str) {
                        let resolved =
                            resolve_style_alias(doc, style_raw).map_err(anyhow::Error::msg)?;
                        doc.set_style(range_sel, resolved.as_deref());
                    }
                    if let Some(align_raw) = args.get("align").and_then(Value::as_str) {
                        let a = parse_align_spec(align_raw)
                            .ok_or_else(|| anyhow::anyhow!("无效对齐方式 `{align_raw}`"))?;
                        doc.set_align(range_sel, a);
                    }

                    let rp_over = build_run_props(&spec_val, None);
                    if rp_over != RunProps::default() {
                        doc.format(range_sel, |rp| {
                            rp.overlay(&rp_over);
                            if !rp_over.extras.is_empty() {
                                rp.extras.remove("highlight");
                                for x in rp_over.extras.iter() {
                                    rp.extras.push(x);
                                }
                            }
                        });
                    }

                    let ref_pt = rp_over
                        .size
                        .map(|h| h as f64 / 2.0)
                        .unwrap_or_else(|| doc.resolved_at(DocPos::new(s0, 0)).size_pt as f64);

                    let mut indent = Indent::default();
                    let mut has_indent = false;
                    if let Some(chars) = args.get("first_line_indent_chars").and_then(Value::as_f64)
                    {
                        indent.first = Some((chars * ref_pt * 20.0).round() as i32);
                        has_indent = true;
                    } else if let Some(cm) =
                        args.get("first_line_indent_cm").and_then(Value::as_f64)
                    {
                        let sign = if cm < 0.0 { -1 } else { 1 };
                        indent.first = Some(sign * cm_to_twips(cm.abs()) as i32);
                        has_indent = true;
                    }
                    if let Some(cm) = args.get("left_indent_cm").and_then(Value::as_f64) {
                        indent.left = Some(cm_to_twips(cm.max(0.0)) as i32);
                        has_indent = true;
                    }
                    if let Some(cm) = args.get("right_indent_cm").and_then(Value::as_f64) {
                        indent.right = Some(cm_to_twips(cm.max(0.0)) as i32);
                        has_indent = true;
                    }
                    if has_indent {
                        doc.set_indent(range_sel, indent);
                    }

                    doc.format_paragraphs(range_sel, |pp| {
                        if let Some(mult) = args.get("line_spacing").and_then(Value::as_f64) {
                            pp.line = Some(LineSpacing {
                                line: (mult.clamp(0.8, 4.0) * 240.0).round() as i32,
                                rule: LineRule::Auto,
                            });
                        } else if let Some(exact_pt) =
                            args.get("line_spacing_exact_pt").and_then(Value::as_f64)
                        {
                            pp.line = Some(LineSpacing {
                                line: (exact_pt.clamp(8.0, 120.0) * 20.0).round() as i32,
                                rule: LineRule::Exact,
                            });
                        } else if let Some(min_pt) =
                            args.get("line_spacing_at_least_pt").and_then(Value::as_f64)
                        {
                            pp.line = Some(LineSpacing {
                                line: (min_pt.clamp(8.0, 120.0) * 20.0).round() as i32,
                                rule: LineRule::AtLeast,
                            });
                        }
                        if let Some(b) = args.get("space_before_pt").and_then(Value::as_f64) {
                            pp.before = Some((b.max(0.0) * 20.0).round() as u32);
                            pp.contextual = Some(false);
                        }
                        if let Some(a) = args.get("space_after_pt").and_then(Value::as_f64) {
                            pp.after = Some((a.max(0.0) * 20.0).round() as u32);
                            pp.contextual = Some(false);
                        }
                        if let Some(pb) = args.get("page_break_before").and_then(Value::as_bool) {
                            pp.page_break_before = Some(pb);
                        }
                        if let Some(kn) = args.get("keep_next").and_then(Value::as_bool) {
                            pp.extras.remove("keepNext");
                            if kn {
                                pp.extras.push("<w:keepNext/>");
                            }
                        }
                    });

                    if let Some(list_raw) = args.get("list").and_then(Value::as_str) {
                        match list_raw {
                            "bullet" | "unordered" => doc.toggle_list(range_sel, ListKind::Bullet),
                            "number" | "numbered" | "ordered" => {
                                doc.toggle_list(range_sel, ListKind::Numbered)
                            }
                            _ => {}
                        }
                    }

                    Ok(((s0 + 1, e0 + 1), range_sel))
                })
            })
            .map_err(|e| e.to_string())?;

        serde_json::to_string_pretty(&json!({
            "status": "formatted",
            "tab_id": tab.id,
            "start_paragraph": formatted_range.0,
            "end_paragraph": formatted_range.1,
        }))
        .map_err(|e| e.to_string())
    }

    pub(crate) fn exec_page_setup(
        &mut self,
        tab: &OfficeTab,
        args: &Map<String, Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let OfficeEditorView::Word(ref e) = tab.view else {
            return Err("页面设置仅支持 Word (.docx) 文档".into());
        };

        let result = e
            .update(cx, |v, cx| {
                v.agent_edit("智能体设置页面版式与页眉页脚", window, cx, |doc, sel| {
                    let mut pg = doc.section_page(0);
                    let cur_landscape = pg.width > pg.height;
                    let want_landscape = match args.get("orientation").and_then(Value::as_str) {
                        Some("landscape" | "横向") => true,
                        Some("portrait" | "纵向") => false,
                        None => cur_landscape,
                        Some(other) => anyhow::bail!("不支持的纸张方向 `{other}`"),
                    };

                    if let Some(paper_raw) = args.get("paper").and_then(Value::as_str) {
                        let paper = match paper_raw.trim().to_ascii_lowercase().as_str() {
                            "a4" => PaperSize::A4,
                            "letter" => PaperSize::Letter,
                            "legal" => PaperSize::Legal,
                            "a5" => PaperSize::A5,
                            "f4" | "folio" => PaperSize::F4,
                            other => anyhow::bail!("不支持的纸张规格 `{other}`"),
                        };
                        let (pw, ph) = paper.twips();
                        pg.width = pw;
                        pg.height = ph;
                    }
                    let (min_s, max_s) = (pg.width.min(pg.height), pg.width.max(pg.height));
                    if want_landscape {
                        pg.width = max_s;
                        pg.height = min_s;
                    } else {
                        pg.width = min_s;
                        pg.height = max_s;
                    }
                    let _ = Orientation::Portrait;

                    if let Some(preset_raw) = args.get("margin_preset").and_then(Value::as_str) {
                        let (t, b, l, r) = match preset_raw.trim().to_ascii_lowercase().as_str() {
                            "normal" | "常规" => MarginPreset::Normal.twips(),
                            "narrow" | "窄" => MarginPreset::Narrow.twips(),
                            "moderate" | "适中" => MarginPreset::Moderate.twips(),
                            "wide" | "宽" => MarginPreset::Wide.twips(),
                            "official_zh" | "gongwen" | "公文" => (
                                cm_to_twips(3.7),
                                cm_to_twips(3.5),
                                cm_to_twips(2.8),
                                cm_to_twips(2.6),
                            ),
                            other => anyhow::bail!("未知页边距预设 `{other}`"),
                        };
                        pg.margin_top = t;
                        pg.margin_bottom = b;
                        pg.margin_left = l;
                        pg.margin_right = r;
                    }
                    if let Some(cm) = args.get("top_cm").and_then(Value::as_f64) {
                        pg.margin_top = cm_to_twips(cm.clamp(0.5, 10.0));
                    }
                    if let Some(cm) = args.get("bottom_cm").and_then(Value::as_f64) {
                        pg.margin_bottom = cm_to_twips(cm.clamp(0.5, 10.0));
                    }
                    if let Some(cm) = args.get("left_cm").and_then(Value::as_f64) {
                        pg.margin_left = cm_to_twips(cm.clamp(0.5, 10.0));
                    }
                    if let Some(cm) = args.get("right_cm").and_then(Value::as_f64) {
                        pg.margin_right = cm_to_twips(cm.clamp(0.5, 10.0));
                    }
                    doc.set_page(pg, sel);

                    if let Some(first_diff) =
                        args.get("different_first_page").and_then(Value::as_bool)
                    {
                        doc.set_title_page(0, first_diff, sel);
                    }
                    if let Some(hdr) = args.get("header_text").and_then(Value::as_str) {
                        let h_align = args
                            .get("header_align")
                            .and_then(Value::as_str)
                            .and_then(parse_align_spec)
                            .or(Some(Align::Center));
                        doc.set_hf_text(0, HfKind::Header, HfType::Default, hdr, h_align, sel);
                    }
                    if let Some(ftr) = args.get("footer_text").and_then(Value::as_str) {
                        let f_align = args
                            .get("footer_align")
                            .and_then(Value::as_str)
                            .and_then(parse_align_spec)
                            .or(Some(Align::Center));
                        doc.set_hf_text(0, HfKind::Footer, HfType::Default, ftr, f_align, sel);
                    }
                    if args
                        .get("page_numbers")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        let custom = args
                            .get("footer_text")
                            .and_then(Value::as_str)
                            .filter(|s| s.contains("{PAGE}"));
                        doc.add_page_numbers(0, custom.unwrap_or("— {PAGE} —"), sel);
                    }

                    let round2 = |v: f64| (v * 100.0).round() / 100.0;
                    Ok((
                        json!({
                            "status": "updated",
                            "tab_id": tab.id,
                            "paper": PaperSize::of(&pg).map_or("Custom", PaperSize::label),
                            "orientation": if pg.width > pg.height { "landscape" } else { "portrait" },
                            "margins_cm": {
                                "top": round2(twips_to_cm(pg.margin_top)),
                                "bottom": round2(twips_to_cm(pg.margin_bottom)),
                                "left": round2(twips_to_cm(pg.margin_left)),
                                "right": round2(twips_to_cm(pg.margin_right)),
                            },
                        }),
                        sel,
                    ))
                })
            })
            .map_err(|e| e.to_string())?;

        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    pub(crate) fn exec_edit_word_table(
        &mut self,
        tab: &OfficeTab,
        args: &Map<String, Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let OfficeEditorView::Word(ref e) = tab.view else {
            return Err("表格操作仅支持 Word (.docx) 文档".into());
        };
        let action = args
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("insert");

        let out = e
            .update(cx, |v, cx| {
                v.agent_edit("智能体编辑表格", window, cx, |doc, sel| {
                    let parse_border_preset = |raw: &str| match raw {
                        "three_line" | "academic" | "三线表" => Some(TableBorderPreset::ThreeLine),
                        "none" | "borderless" | "无边框" => Some(TableBorderPreset::None),
                        "grid" | "网格" => Some(TableBorderPreset::Grid),
                        _ => None,
                    };
                    let parse_tbl_align = |raw: &str| match raw {
                        "left" => Some(TableAlign::Left),
                        "right" => Some(TableAlign::Right),
                        "center" => Some(TableAlign::Center),
                        _ => None,
                    };

                    if action == "insert" {
                        let cells_matrix = args.get("cells").and_then(Value::as_array);
                        let inferred_rows = cells_matrix.map_or(0, Vec::len);
                        let inferred_cols = cells_matrix
                            .and_then(|m| m.iter().filter_map(Value::as_array).map(Vec::len).max())
                            .unwrap_or(0);
                        let rows = args
                            .get("rows")
                            .and_then(Value::as_u64)
                            .map(|v| v as usize)
                            .unwrap_or(inferred_rows)
                            .clamp(1, 63);
                        let cols = args
                            .get("cols")
                            .and_then(Value::as_u64)
                            .map(|v| v as usize)
                            .unwrap_or(inferred_cols)
                            .clamp(1, 63);

                        let target_para = match args.get("insert_after").and_then(Value::as_u64) {
                            None => doc.len().saturating_sub(1),
                            Some(0) => 0,
                            Some(p1) => ((p1 - 1) as usize).min(doc.len().saturating_sub(1)),
                        };
                        let place_sel = DocSel::caret(DocPos::new(
                            target_para,
                            doc.paragraph(target_para).text.len(),
                        ));
                        let first_cell_sel = doc
                            .insert_table(place_sel, rows, cols)
                            .ok_or_else(|| anyhow::anyhow!("无法在当前表格内部嵌套插入新表格"))?;
                        let tid = doc
                            .cell_at(first_cell_sel.head)
                            .map(|c| c.table)
                            .ok_or_else(|| anyhow::anyhow!("未能定位新插入的表格"))?;

                        let header_row = args
                            .get("header_row")
                            .and_then(Value::as_bool)
                            .unwrap_or(true);
                        let border_preset = args
                            .get("border_style")
                            .and_then(Value::as_str)
                            .and_then(parse_border_preset)
                            .or(Some(TableBorderPreset::Grid));
                        let header_fill = args
                            .get("header_fill_hex")
                            .and_then(Value::as_str)
                            .or_else(|| {
                                (border_preset == Some(TableBorderPreset::Grid) && header_row)
                                    .then_some("#F1F5F9")
                            });
                        let tbl_align = args
                            .get("align")
                            .and_then(Value::as_str)
                            .and_then(parse_tbl_align)
                            .or(Some(TableAlign::Center));
                        let col_widths: Option<Vec<u32>> = args
                            .get("col_widths_cm")
                            .and_then(Value::as_array)
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(Value::as_f64)
                                    .map(|cm| cm_to_twips(cm.max(1.0)))
                                    .collect()
                            });

                        if let Some(matrix) = cells_matrix {
                            for (r, row_val) in matrix.iter().enumerate().take(rows) {
                                if let Some(row_arr) = row_val.as_array() {
                                    for (c, cell_val) in row_arr.iter().enumerate().take(cols) {
                                        let txt = match cell_val {
                                            Value::String(s) => s.clone(),
                                            other => other.to_string(),
                                        };
                                        let mut rp = RunProps {
                                            size: Some(21), // 五号 10.5pt 表格常用字号
                                            ..Default::default()
                                        };
                                        if r == 0 && header_row {
                                            rp.bold = Some(true);
                                        }
                                        let cell_align = if r == 0 && header_row {
                                            Some(Align::Center)
                                        } else {
                                            None
                                        };
                                        doc.set_table_cell_content(
                                            tid,
                                            r,
                                            c,
                                            &txt,
                                            Some(rp),
                                            cell_align,
                                            first_cell_sel,
                                        )
                                        .map_err(anyhow::Error::msg)?;
                                    }
                                }
                            }
                        }

                        let final_sel = doc
                            .style_table_wps(
                                tid,
                                border_preset,
                                Some(header_row),
                                header_fill,
                                col_widths.as_deref(),
                                tbl_align,
                                &[],
                                first_cell_sel,
                            )
                            .map_err(anyhow::Error::msg)?;

                        let r = doc.table_range(tid);
                        return Ok((
                            json!({
                                "status": "table_inserted",
                                "tab_id": tab.id,
                                "table_id": tid,
                                "rows": rows,
                                "cols": cols,
                                "start_paragraph": r.start + 1,
                                "end_paragraph": r.end,
                            }),
                            final_sel,
                        ));
                    }

                    let tid = args
                        .get("table_id")
                        .and_then(Value::as_u64)
                        .map(|v| v as u32)
                        .or_else(|| doc.cell_at(sel.head).map(|c| c.table))
                        .or_else(|| doc.table_ids().first().copied())
                        .ok_or_else(|| anyhow::anyhow!("文档中当前没有表格"))?;

                    let cell_first_para = |doc: &Document, r0: usize, c0: usize| -> Option<usize> {
                        let range = doc.table_range(tid);
                        let target = CellRef {
                            table: tid,
                            row: r0 as u32,
                            cell: c0 as u32,
                        };
                        (range.start..range.end).find(|&pi| doc.paragraph(pi).cell == Some(target))
                    };

                    match action {
                        "set_cells" => {
                            let edits = args
                                .get("edits")
                                .and_then(Value::as_array)
                                .ok_or_else(|| anyhow::anyhow!("`set_cells` 需要 `edits` 数组"))?;
                            let mut cell_fills = Vec::new();
                            let mut last_sel = sel;
                            for item in edits {
                                let r1 = item
                                    .get("row")
                                    .and_then(Value::as_u64)
                                    .ok_or_else(|| anyhow::anyhow!("缺少 1-based `row`"))?
                                    .max(1) as usize;
                                let c1 = item
                                    .get("col")
                                    .and_then(Value::as_u64)
                                    .ok_or_else(|| anyhow::anyhow!("缺少 1-based `col`"))?
                                    .max(1) as usize;
                                let txt = item
                                    .get("text")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default();
                                let rp = build_run_props(item, None);
                                let rp_opt = (rp != RunProps::default()).then_some(rp);
                                let ca = item
                                    .get("align")
                                    .and_then(Value::as_str)
                                    .and_then(parse_align_spec);
                                last_sel = doc
                                    .set_table_cell_content(
                                        tid,
                                        r1 - 1,
                                        c1 - 1,
                                        txt,
                                        rp_opt,
                                        ca,
                                        last_sel,
                                    )
                                    .map_err(anyhow::Error::msg)?;
                                if let Some(fill) = item.get("fill_hex").and_then(Value::as_str) {
                                    cell_fills.push((r1 - 1, c1 - 1, fill.to_owned()));
                                }
                            }
                            if !cell_fills.is_empty() {
                                last_sel = doc
                                    .style_table_wps(
                                        tid,
                                        None,
                                        None,
                                        None,
                                        None,
                                        None,
                                        &cell_fills,
                                        last_sel,
                                    )
                                    .map_err(anyhow::Error::msg)?;
                            }
                            Ok((
                                json!({
                                    "status": "cells_updated",
                                    "tab_id": tab.id,
                                    "table_id": tid,
                                    "updated_count": edits.len(),
                                }),
                                last_sel,
                            ))
                        }
                        "style_table" => {
                            let border_preset = args
                                .get("border_style")
                                .and_then(Value::as_str)
                                .and_then(parse_border_preset);
                            let header_row = args.get("header_row").and_then(Value::as_bool);
                            let header_fill = args.get("header_fill_hex").and_then(Value::as_str);
                            let tbl_align = args
                                .get("align")
                                .and_then(Value::as_str)
                                .and_then(parse_tbl_align);
                            let col_widths: Option<Vec<u32>> = args
                                .get("col_widths_cm")
                                .and_then(Value::as_array)
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(Value::as_f64)
                                        .map(|cm| cm_to_twips(cm.max(1.0)))
                                        .collect()
                                });
                            let s = doc
                                .style_table_wps(
                                    tid,
                                    border_preset,
                                    header_row,
                                    header_fill,
                                    col_widths.as_deref(),
                                    tbl_align,
                                    &[],
                                    sel,
                                )
                                .map_err(anyhow::Error::msg)?;
                            Ok((
                                json!({ "status": "table_styled", "tab_id": tab.id, "table_id": tid }),
                                s,
                            ))
                        }
                        "merge_cells" => {
                            let sr = args
                                .get("start_row")
                                .and_then(Value::as_u64)
                                .unwrap_or(1)
                                .max(1) as usize
                                - 1;
                            let sc = args
                                .get("start_col")
                                .and_then(Value::as_u64)
                                .unwrap_or(1)
                                .max(1) as usize
                                - 1;
                            let er = args
                                .get("end_row")
                                .and_then(Value::as_u64)
                                .unwrap_or(1)
                                .max(1) as usize
                                - 1;
                            let ec = args
                                .get("end_col")
                                .and_then(Value::as_u64)
                                .unwrap_or(1)
                                .max(1) as usize
                                - 1;
                            let p_start = cell_first_para(doc, sr, sc)
                                .ok_or_else(|| anyhow::anyhow!("起始单元格越界"))?;
                            let p_end = cell_first_para(doc, er, ec)
                                .ok_or_else(|| anyhow::anyhow!("结束单元格越界"))?;
                            let merge_sel = DocSel {
                                anchor: DocPos::new(p_start, 0),
                                head: DocPos::new(p_end, doc.paragraph(p_end).text.len()),
                            };
                            let (after, _) =
                                doc.merge_cells(merge_sel).map_err(anyhow::Error::msg)?;
                            Ok((
                                json!({ "status": "cells_merged", "tab_id": tab.id, "table_id": tid }),
                                after,
                            ))
                        }
                        "add_row" | "delete_row" | "add_column" | "delete_column"
                        | "delete_table" => {
                            let r0 = args
                                .get("row")
                                .and_then(Value::as_u64)
                                .unwrap_or(1)
                                .max(1) as usize
                                - 1;
                            let c0 = args
                                .get("col")
                                .and_then(Value::as_u64)
                                .unwrap_or(1)
                                .max(1) as usize
                                - 1;
                            let p_at = cell_first_para(doc, r0, c0)
                                .unwrap_or_else(|| doc.table_range(tid).start);
                            let at_sel = DocSel::caret(DocPos::new(p_at, 0));
                            let edit_op = match action {
                                "add_row" => TableEdit::RowBelow,
                                "delete_row" => TableEdit::DeleteRow,
                                "add_column" => TableEdit::ColumnRight,
                                "delete_column" => TableEdit::DeleteColumn,
                                _ => TableEdit::DeleteTable,
                            };
                            let after = doc
                                .edit_table(at_sel, edit_op)
                                .map_err(anyhow::Error::msg)?;
                            Ok((
                                json!({ "status": action, "tab_id": tab.id, "table_id": tid }),
                                after,
                            ))
                        }
                        other => anyhow::bail!(
                            "未知表格操作 `{other}`（可选：`insert`、`set_cells`、`style_table`、`merge_cells`、`add_row`、`delete_row`、`add_column`、`delete_column`、`delete_table`）"
                        ),
                    }
                })
            })
            .map_err(|e| e.to_string())?;

        serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
    }

    pub(crate) fn exec_inspect_document(
        &mut self,
        tab: &OfficeTab,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let doc_name = clean_display_doc_name(&tab.doc_name(cx));
        let payload = match &tab.view {
            OfficeEditorView::Word(e) => e
                .read(cx)
                .agent_read(|doc, _| inspect_word_document(doc, tab.id, &doc_name)),
            OfficeEditorView::Excel(e) => {
                if e.read(cx).is_busy() {
                    return Err("工作簿正在后台计算，请稍后重试".into());
                }
                e.read(cx).agent_read(cx, |book, active_sheet| {
                    let names = book.sheet_names();
                    let mut sheets_meta = Vec::new();
                    for (idx, name) in names.iter().enumerate() {
                        let mut non_empty = 0usize;
                        let mut formula_count = 0usize;
                        for r in 0..80u32 {
                            for c in 0..26u32 {
                                let raw = book.input(idx as u32, r, c);
                                if !raw.is_empty() {
                                    non_empty += 1;
                                    if raw.starts_with('=') {
                                        formula_count += 1;
                                    }
                                }
                            }
                        }
                        sheets_meta.push(json!({
                            "sheet_index": idx,
                            "name": name,
                            "sampled_non_empty_cells": non_empty,
                            "sampled_formulas": formula_count,
                        }));
                    }
                    json!({
                        "tab_id": tab.id,
                        "kind": "XLSX",
                        "workbook": doc_name,
                        "path": book.path().map(|p| p.display().to_string()),
                        "needs_save_as": book.path().is_none(),
                        "active_sheet": active_sheet,
                        "sheets": sheets_meta,
                    })
                })
            }
            OfficeEditorView::Ppt(e) => e.read(cx).agent_read(|pres, cur_slide| {
                let slides: Vec<Value> = (0..pres.len())
                    .map(|idx| {
                        let s = pres.slide(idx);
                        let texts: Vec<String> = s
                            .shapes
                            .iter()
                            .filter_map(|sh| sh.text.as_ref().map(|t| t.plain_text()))
                            .filter(|t| !t.trim().is_empty())
                            .collect();
                        json!({
                            "slide": idx + 1,
                            "hidden": s.hidden,
                            "shape_count": s.shapes.len(),
                            "preview_text": texts.first().cloned().unwrap_or_default(),
                        })
                    })
                    .collect();
                json!({
                    "tab_id": tab.id,
                    "kind": "PPTX",
                    "presentation": doc_name,
                    "path": pres.path().map(|p| p.display().to_string()),
                    "needs_save_as": pres.needs_save_as(),
                    "slide_count": pres.len(),
                    "active_slide": cur_slide + 1,
                    "slides": slides,
                })
            }),
            OfficeEditorView::Pdf(e) => {
                let path = e
                    .read(cx)
                    .path(cx)
                    .ok_or_else(|| "当前 PDF 未绑定磁盘路径".to_owned())?;
                let bytes = std::fs::read(&path).map_err(|err| format!("读取 PDF 失败：{err}"))?;
                let pdf = PdfDoc::open(Arc::new(bytes), "")
                    .map_err(|err| format!("解析 PDF 失败：{err}"))?;
                let page_count = pdf.page_count();
                let pages_preview: Vec<Value> = (0..page_count.min(20))
                    .map(|p| {
                        let txt = pdf.text(p).text;
                        let snippet: String = txt.chars().take(600).collect();
                        json!({
                            "page": p + 1,
                            "text_excerpt": snippet,
                        })
                    })
                    .collect();
                json!({
                    "tab_id": tab.id,
                    "kind": "PDF",
                    "document": doc_name,
                    "path": path.display().to_string(),
                    "title": pdf.title(),
                    "page_count": page_count,
                    "pages": pages_preview,
                })
            }
        };
        serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())
    }

    pub(crate) fn exec_render_preview(
        &mut self,
        tab: &OfficeTab,
        args: &Map<String, Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<McpToolOutput, String> {
        // Ensure the visual tab is visible in Split view so the human user sees the same page!
        self.main_stage_view = MainStageView::Conversation;
        self.ensure_split_visible(window, cx);
        self.right_surface = RightPaneSurface::Office;
        if let Some(pos) = self.office_tabs.iter().position(|t| t.id == tab.id) {
            self.active_office_tab = Some(pos);
        }

        let page_1 = args
            .get("page")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1) as usize;
        let dpi = args
            .get("dpi")
            .and_then(Value::as_u64)
            .unwrap_or(144)
            .clamp(72, 216) as u32;

        let mut warnings: Vec<String> = Vec::new();
        let pdf_bytes: Vec<u8> = match &tab.view {
            OfficeEditorView::Word(e) => e.read(cx).agent_read(|doc, _| {
                with_pdf_engine(|engine| {
                    let laid = docs_pdf::lay_out(doc, engine);
                    for (p_ix, page) in laid.paged.pages.iter().enumerate() {
                        if let Some(last_line) = page.lines.iter().rfind(|l| !l.repeat) {
                            let para = doc.paragraph(last_line.para);
                            let is_heading = para
                                .props
                                .style
                                .as_deref()
                                .is_some_and(|s| s.starts_with("Heading"));
                            let next_on_next = laid
                                .paged
                                .pages
                                .get(p_ix + 1)
                                .and_then(|np| np.lines.iter().find(|l| !l.repeat))
                                .is_some_and(|nl| nl.para != last_line.para);
                            if is_heading && next_on_next {
                                warnings.push(format!(
                                    "第 {} 页底端出现孤立标题（段落 #{}），建议调整段前分页或 keep_next",
                                    p_ix + 1,
                                    last_line.para + 1
                                ));
                            }
                        }
                    }
                    if laid.paged.pages.len() > 1
                        && let Some(last_page) = laid.paged.pages.last()
                    {
                        let non_repeat_lines = last_page.lines.iter().filter(|l| !l.repeat).count();
                        if non_repeat_lines <= 2 {
                            warnings.push(format!(
                                "最后一页（第 {} 页）仅有 {} 行内容，可适当微调行距或页边距避免零星尾页",
                                laid.paged.pages.len(),
                                non_repeat_lines
                            ));
                        }
                    }
                    docs_pdf::write(&docs_pdf::scene(doc, &laid)).map_err(|e| e.to_string())
                })
            })?,
            OfficeEditorView::Excel(e) => {
                if e.read(cx).is_busy() {
                    return Err("工作簿正在后台计算，请稍后重试".into());
                }
                let (bytes, _) = e
                    .update(cx, |v, cx| v.agent_export_pdf(cx))
                    .map_err(|e| e.to_string())?;
                bytes
            }
            OfficeEditorView::Ppt(e) => e.read(cx).agent_read(|pres, _| {
                with_pdf_engine(|engine| {
                    slides_pdf::export(pres, engine)
                        .map(|(b, _)| b)
                        .map_err(|e| e.to_string())
                })
            })?,
            OfficeEditorView::Pdf(e) => {
                let path = e
                    .read(cx)
                    .path(cx)
                    .ok_or_else(|| "当前 PDF 未绑定磁盘路径".to_owned())?;
                std::fs::read(&path).map_err(|err| format!("读取 PDF 失败：{err}"))?
            }
        };

        let RenderedPagePng {
            png_bytes,
            total_pages,
            size_px: (width_px, height_px),
        } = render_pdf_page_to_png(pdf_bytes, page_1, dpi)?;
        let actual_page = page_1.min(total_pages);
        let preview_path = std::env::temp_dir().join(format!(
            "kwrok-office-preview-tab{}-p{}.png",
            tab.id, actual_page
        ));
        let _ = std::fs::write(&preview_path, &png_bytes);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);

        let summary = serde_json::to_string_pretty(&json!({
            "status": "rendered",
            "tab_id": tab.id,
            "kind": tab.kind.short_tag(),
            "document": clean_display_doc_name(&tab.doc_name(cx)),
            "rendered_page": actual_page,
            "page_count": total_pages,
            "dpi": dpi,
            "image_size_px": { "width": width_px, "height": height_px },
            "saved_preview_png": preview_path.display().to_string(),
            "layout_warnings": warnings,
        }))
        .map_err(|e| e.to_string())?;

        Ok(McpToolOutput {
            text: summary,
            image_png_base64: Some(b64),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[std::prelude::v1::test]
    fn full_wps_word_lifecycle_rich_typography_table_inspect_and_visual_png_preview() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("阅读让生活更丰盈.docx");

        let mut doc = Document::new();
        let sel = DocSel::caret(DocPos::new(0, 0));
        apply_word_preset(&mut doc, "clean_article", sel).unwrap();

        let blocks = vec![
            build_rich_paragraph(
                &doc,
                &json!({
                    "style": "标题",
                    "align": "居中",
                    "font": "黑体",
                    "size_zh": "二号",
                    "bold": true,
                    "text": "阅读让生活更丰盈"
                }),
            )
            .unwrap(),
            build_rich_paragraph(
                &doc,
                &json!({
                    "style": "一级标题",
                    "font": "黑体",
                    "size_zh": "三号",
                    "bold": true,
                    "text": "一、阅读拓宽认知的边界"
                }),
            )
            .unwrap(),
            build_rich_paragraph(
                &doc,
                &json!({
                    "style": "正文",
                    "font": "宋体",
                    "ascii_font": "Times New Roman",
                    "size_zh": "小四",
                    "first_line_indent_chars": 2.0,
                    "line_spacing": 1.5,
                    "markdown": true,
                    "text": "**阅读**是一场跨越时空的深度对话，让平凡的日常拥有*诗意与厚度*。"
                }),
            )
            .unwrap(),
        ];
        doc.insert_rich_paragraphs(None, true, &blocks, sel)
            .unwrap();

        // Insert a 3x3 academic three-line table after paragraph 3
        let place_sel = DocSel::caret(DocPos::new(2, doc.paragraph(2).text.len()));
        let t_sel = doc.insert_table(place_sel, 3, 3).unwrap();
        let tid = doc.cell_at(t_sel.head).unwrap().table;
        doc.set_table_cell_content(tid, 0, 0, "阅读维度", None, Some(Align::Center), t_sel)
            .unwrap();
        doc.set_table_cell_content(tid, 0, 1, "推荐时长", None, Some(Align::Center), t_sel)
            .unwrap();
        doc.set_table_cell_content(tid, 0, 2, "核心收获", None, Some(Align::Center), t_sel)
            .unwrap();
        doc.set_table_cell_content(tid, 1, 0, "深度精读", None, None, t_sel)
            .unwrap();
        doc.set_table_cell_content(tid, 1, 1, "每日 45 分钟", None, None, t_sel)
            .unwrap();
        doc.set_table_cell_content(tid, 1, 2, "构建系统思维框架", None, None, t_sel)
            .unwrap();
        doc.style_table_wps(
            tid,
            Some(TableBorderPreset::ThreeLine),
            Some(true),
            None,
            None,
            Some(TableAlign::Center),
            &[],
            t_sel,
        )
        .unwrap();

        doc.save_as(&path).unwrap();
        assert!(!doc.needs_save_as());

        // Re-open from disk and verify styles, alignment, first-line indent, and table!
        let reopened = Document::open(&path).unwrap();
        let p0 = format_word_paragraph_entry(&reopened, 0);
        assert_eq!(p0["style_id"], "Title");
        assert_eq!(p0["align"], "center");
        assert_eq!(p0["size_pt"], 22.0); // 二号 = 22pt!

        let p1 = format_word_paragraph_entry(&reopened, 1);
        assert_eq!(p1["style_id"], "Heading1");
        assert_eq!(p1["size_pt"], 16.0); // 三号 = 16pt!

        let p2 = format_word_paragraph_entry(&reopened, 2);
        assert_eq!(p2["first_line_indent_pt"], 24.0); // 小四 12pt * 2 chars = 24pt!

        let inspection = inspect_word_document(&reopened, 1, "阅读让生活更丰盈.docx");
        assert_eq!(inspection["page_count"], 1);
        assert_eq!(inspection["tables"][0]["rows"], 3);
        assert_eq!(inspection["tables"][0]["cols"], 3);
        assert_eq!(inspection["outline"].as_array().unwrap().len(), 2);

        // Visual PNG preview rendering (pure Rust `docs_pdf` -> `hayro` -> PNG)!
        let pdf_bytes = with_pdf_engine(|engine| {
            let laid = docs_pdf::lay_out(&reopened, engine);
            docs_pdf::write(&docs_pdf::scene(&reopened, &laid)).unwrap()
        });
        let RenderedPagePng {
            png_bytes,
            total_pages,
            size_px: (w, h),
        } = render_pdf_page_to_png(pdf_bytes, 1, 144).unwrap();
        assert_eq!(total_pages, 1);
        assert!(w > 1000 && h > 1400, "144 DPI A4 PNG must be high-res: {w}x{h}");
        assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
}
