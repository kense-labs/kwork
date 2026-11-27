//! Interface language: English or Indonesian.
//!
//! English is the source language and the key: code writes `t("Insert Row
//! Above")`, and the Indonesian comes from the catalog files in
//! `crates/kantor_ui/i18n/` (one per app area, `English<TAB>Indonesian` per
//! line). Strings with values in them use `{}` placeholders:
//! `tf("Page {} of {}", &[&n, &m])`. A short string that is only used in one
//! place can also be written as a pair in place, `tr("Synced", "Tersinkron")`.
//!
//! The `catalog_*` tests fail when a `t`/`tf`/`tk` string in the UI crates
//! has no Indonesian, or a catalog line has no English use left.
//!
//! The language is chosen once per run: `KANTOR_LANG`, then `language` in
//! settings.toml, then the system locale (`LC_ALL`, `LC_MESSAGES`, `LANG`;
//! `id…` is Indonesian, anything else English).

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Id,
    Zh,
}

static LANG_OVERRIDE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Explicitly set the interface language for an embedding host (e.g. `kar_desktop` sets `Lang::Zh`).
pub fn set_lang(lang: Lang) {
    let v = match lang {
        Lang::En => 1,
        Lang::Id => 2,
        Lang::Zh => 3,
    };
    LANG_OVERRIDE.store(v, std::sync::atomic::Ordering::Relaxed);
}

impl Lang {
    /// From a setting or locale string (`id`, `id_ID.UTF-8`, `en_US`, `ind`).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        if s.is_empty() || s == "c" || s == "posix" {
            return None;
        }
        Some(
            if s.starts_with("id") || s.starts_with("ind") || s.starts_with("in_") {
                Self::Id
            } else {
                Self::En
            },
        )
    }

    /// The language from the environment and settings (see the module docs).
    pub fn detect() -> Self {
        let env = |k: &str| std::env::var(k).ok().and_then(|v| Self::parse(&v));
        env("KANTOR_LANG")
            .or_else(|| {
                kantor_core::settings::Settings::load()
                    .language
                    .as_deref()
                    .and_then(Self::parse)
            })
            .or_else(|| env("LC_ALL"))
            .or_else(|| env("LC_MESSAGES"))
            .or_else(|| env("LANG"))
            .unwrap_or(Self::En)
    }
}

/// The interface language for this run.
pub fn lang() -> Lang {
    match LANG_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => return Lang::En,
        2 => return Lang::Id,
        3 => return Lang::Zh,
        _ => {}
    }
    static LANG: OnceLock<Lang> = OnceLock::new();
    *LANG.get_or_init(Lang::detect)
}

/// The regional format numbers are shown and typed in: `number_format` in
/// settings.toml (`id` or `en`), else the interface language's
/// (Indonesian: `1.234,5`).
pub fn number_locale() -> kantor_numfmt::DisplayLocale {
    let setting = kantor_core::settings::Settings::load().number_format;
    number_locale_for(setting.as_deref().and_then(Lang::parse), lang())
}

/// [`number_locale`] from its inputs (testable without the globals).
pub fn number_locale_for(setting: Option<Lang>, ui: Lang) -> kantor_numfmt::DisplayLocale {
    match setting.unwrap_or(ui) {
        Lang::Id => kantor_numfmt::DisplayLocale::Indonesian,
        Lang::En | Lang::Zh => kantor_numfmt::DisplayLocale::EnUs,
    }
}

/// The string for the interface language.
pub fn tr(en: &'static str, id: &'static str) -> &'static str {
    match lang() {
        Lang::Zh => chinese(en).unwrap_or(en),
        other => pick(other, en, id),
    }
}

/// The string for `lang` (testable without the global).
pub fn pick<T>(lang: Lang, en: T, id: T) -> T {
    match lang {
        Lang::En | Lang::Zh => en,
        Lang::Id => id,
    }
}

/// The catalog files, `English<TAB>Indonesian` per line. `#` starts a
/// comment line; blank lines are ignored. A literal tab or newline in a
/// string is written `\t` / `\n`.
pub const CATALOG_FILES: &[(&str, &str)] = &[
    ("common.tsv", include_str!("../i18n/common.tsv")),
    ("kantor.tsv", include_str!("../i18n/kantor.tsv")),
    ("sheets.tsv", include_str!("../i18n/sheets.tsv")),
    ("docs.tsv", include_str!("../i18n/docs.tsv")),
    ("slides.tsv", include_str!("../i18n/slides.tsv")),
    ("pdf.tsv", include_str!("../i18n/pdf.tsv")),
    ("widgets.tsv", include_str!("../i18n/widgets.tsv")),
    ("content.tsv", include_str!("../i18n/content.tsv")),
];

/// One catalog line: `(english, indonesian)`, unescaped.
pub fn parse_catalog(text: &'static str) -> impl Iterator<Item = (String, String)> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (en, id) = l.split_once('\t')?;
            Some((unescape(en), unescape(id)))
        })
}

fn unescape(s: &str) -> String {
    s.replace("\\t", "\t").replace("\\n", "\n")
}

/// English → Indonesian, from all catalog files. Strings are leaked once
/// so lookups hand out `&'static str`.
fn catalog() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = HashMap::new();
        for (_, text) in CATALOG_FILES {
            for (en, id) in parse_catalog(text) {
                let en: &'static str = Box::leak(en.into_boxed_str());
                let id: &'static str = Box::leak(id.into_boxed_str());
                map.insert(en, id);
            }
        }
        map
    })
}

/// The Indonesian for an English string, if the catalog has it.
pub fn indonesian(en: &str) -> Option<&'static str> {
    catalog().get(en).copied()
}

/// Simplified Chinese dictionary for embedded Kantor Office views (`DocsView`, `SheetsView`,
/// `SlidesView`, `FindBar`, and `gpui_kit` widgets). Uses native `⌘` shortcut symbols on macOS.
pub fn chinese(en: &str) -> Option<&'static str> {
    static ZH_MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    let map = ZH_MAP.get_or_init(|| {
        let mac = cfg!(target_os = "macos");
        let mut m: HashMap<&'static str, &'static str> = HashMap::from([
            // Common & Widgets
            ("File", "文件"),
            ("Edit", "编辑"),
            ("Format", "格式"),
            ("Data", "数据"),
            ("Home", "开始"),
            ("Open", "打开"),
            ("Open File…", "打开文件…"),
            ("Save", "保存"),
            ("Save As…", "另存为…"),
            ("Close", "关闭"),
            ("Print", "打印"),
            ("Print…", "打印…"),
            ("Export as PDF…", "导出为 PDF…"),
            ("Undo", "撤销"),
            ("Redo", "重做"),
            ("Cut", "剪切"),
            ("Copy", "复制"),
            ("Paste", "粘贴"),
            ("Delete", "删除"),
            ("Find", "查找"),
            ("Find…", "查找…"),
            ("Find in document", "在文档中查找"),
            ("Replace", "替换"),
            ("Replace all", "全部替换"),
            ("Replace All", "全部替换"),
            ("Replace with…", "替换为…"),
            ("Match case", "区分大小写"),
            ("Whole words", "全词匹配"),
            ("Previous match", "上一个匹配"),
            ("Next match", "下一个匹配"),
            ("No matches", "无匹配结果"),
            ("{} of {}", "{} / {}"),
            ("Select All", "全选"),
            ("Show password", "显示密码"),
            ("Hide password", "隐藏密码"),
            ("Decrease", "减少"),
            ("Increase", "增加"),
            ("Please select", "请选择"),
            ("Search…", "搜索…"),
            ("More options", "更多选项"),
            ("More formatting", "更多格式"),
            ("Previous", "上一个"),
            ("Next", "下一个"),
            ("Reset All", "全部重置"),
            ("Unnamed", "未命名"),
            ("Collapse", "收起"),
            ("Expand", "展开"),
            ("Bold", "加粗"),
            ("Italic", "斜体"),
            ("Underline", "下划线"),
            ("Strikethrough", "删除线"),
            ("Align Left", "左对齐"),
            ("Align Center", "居中对齐"),
            ("Align Right", "右对齐"),
            ("Justify", "两端对齐"),
            ("Cancel", "取消"),
            ("OK", "确定"),
            ("Apply", "应用"),
            ("Done", "完成"),
            ("Settings", "设置"),
            ("Zoom In", "放大"),
            ("Zoom Out", "缩小"),
            ("Font", "字体"),
            ("Font size", "字号"),
            ("Insert", "插入"),
            ("Shapes", "形状"),
            ("Table", "表格"),
            ("Picture", "图片"),
            ("Comment", "批注"),
            ("Header", "页眉"),
            ("Footer", "页脚"),
            ("Rename", "重命名"),
            ("Close Tab", "关闭标签页"),
            ("Page Setup", "页面设置"),
            ("Page Setup…", "页面设置…"),
            ("Margins", "页边距"),
            ("Orientation", "纸张方向"),
            ("Portrait", "纵向"),
            ("Landscape", "横向"),
            ("Remove", "移除"),
            ("Yes", "是"),
            ("No", "否"),
            ("Don't Save", "不保存"),
            ("None", "无"),
            ("Normal", "正文"),
            ("Heading 1", "标题 1"),
            ("Heading 2", "标题 2"),
            ("Heading 3", "标题 3"),
            ("Title", "大标题"),
            ("Subtitle", "副标题"),
            ("Paper size", "纸张大小"),
            ("Letter", "信纸 (Letter)"),
            ("Legal", "法律纸 (Legal)"),
            ("Page setup: size, orientation, margins", "页面设置：纸张、方向与页边距"),
            ("Print and export", "打印与导出"),
            ("Insert table", "插入表格"),
            ("Insert picture (PNG or JPEG)", "插入图片 (PNG / JPEG)"),
            ("Insert row below", "在下方插入行"),
            ("Insert row above", "在上方插入行"),
            ("Insert column right", "在右侧插入列"),
            ("Insert column left", "在左侧插入列"),
            ("Delete row", "删除行"),
            ("Delete column", "删除列"),
            ("Delete table", "删除表格"),
            ("Show or hide comments", "显示或隐藏批注"),
            ("Review: comments and tracked changes", "审阅：批注与修订"),
            ("Check Spelling as You Type", "输入时检查拼写"),
            ("Nothing to undo", "没有可撤销的操作"),
            ("Nothing to redo", "没有可重做的操作"),
            ("Page {} of {}", "第 {} / {} 页"),
            ("Page {} of {} · {}", "第 {} / {} 页 · {}"),
            ("Section {}", "第 {} 节"),
            ("Saved {}", "已保存 {}"),
            ("Save failed: {}", "保存失败：{}"),
            ("Opened {}", "已打开 {}"),
            ("Agent: {}", "智能体：{}"),
            // Sheets
            ("Ready", "就绪"),
            ("Value or =formula", "输入数值或 =公式"),
            ("Please wait until the workbook is ready", "正在加载工作簿，请稍候"),
            ("Larger Font", "增大字号"),
            ("Smaller Font", "减小字号"),
            ("Wrap Text", "自动换行"),
            ("Wrap text", "自动换行"),
            ("Merge & Center", "合并后居中"),
            ("All Borders", "所有框线"),
            ("Outside Borders", "外侧框线"),
            ("No Borders", "无框线"),
            ("Borders", "边框"),
            ("Fill Colour", "填充颜色"),
            ("Fill colour", "填充颜色"),
            ("No Fill", "无填充"),
            ("Text Colour", "文字颜色"),
            ("Text colour", "文字颜色"),
            ("Conditional Formatting", "条件格式"),
            ("Data Validation…", "数据验证…"),
            ("Insert Chart", "插入图表"),
            ("PivotTable", "数据透视表"),
            ("Filter", "筛选"),
            ("Freeze Panes", "冻结窗格"),
            ("Freeze First Column", "冻结首列"),
            ("Freeze Top Row", "冻结首行"),
            ("Unfreeze Panes", "取消冻结窗格"),
            ("Insert Column Left", "在左侧插入列"),
            ("Insert Column Right", "在右侧插入列"),
            ("Delete Columns", "删除列"),
            ("Autofit Column Width", "自动调整列宽"),
            ("Insert Row Above", "在上方插入行"),
            ("Insert Row Below", "在下方插入行"),
            ("Delete Rows", "删除行"),
            ("Autofit Row Height", "自动调整行高"),
            ("Calculating…", "计算中…"),
            ("Calculated", "计算完成"),
            ("Calculated in {} s", "计算完成（{} 秒）"),
            ("Opening {}…", "正在打开 {}…"),
            ("Saving…", "正在保存…"),
            ("Saving {}…", "正在保存 {}…"),
            ("Undone", "已撤销"),
            ("Redone", "已重做"),
            // Slides
            ("Delete slide", "删除幻灯片"),
            ("Insert text box", "插入文本框"),
            ("Insert rectangle", "插入矩形"),
            ("Insert ellipse", "插入椭圆"),
            ("Edit table or chart data", "编辑表格或图表数据"),
            ("Numbered list", "编号列表"),
            ("Speaker notes", "演讲者备注"),
            ("Click to add notes", "点击此处添加演讲者备注"),
            ("Shape fill", "形状填充"),
            ("Shape outline: colour and width", "形状轮廓：颜色与线宽"),
            ("No fill", "无填充"),
            ("No outline", "无轮廓"),
            ("Theme colours", "主题颜色"),
            ("Width", "宽度"),
            ("Slide {} of {}", "第 {} / {} 页"),
            ("Text box", "文本框"),
            ("Placeholder", "占位框"),
            ("Ellipse", "椭圆"),
            ("Rectangle", "矩形"),
            ("Rounded rectangle", "圆角矩形"),
            ("Line", "线条"),
            ("Shape ({})", "形状 ({})"),
            ("Group", "组合"),
            ("Ungroup", "取消组合"),
        ]);
        if mac {
            m.extend([
                ("Bold · Ctrl+B", "加粗 · ⌘B"),
                ("Italic · Ctrl+I", "斜体 · ⌘I"),
                ("Underline · Ctrl+U", "下划线 · ⌘U"),
                ("Normal text · Ctrl+Alt+0", "正文 · ⌘⌥0"),
                ("Heading 1 · Ctrl+Alt+1", "标题 1 · ⌘⌥1"),
                ("Heading 2 · Ctrl+Alt+2", "标题 2 · ⌘⌥2"),
                ("Heading 3 · Ctrl+Alt+3", "标题 3 · ⌘⌥3"),
                ("Align left · Ctrl+L", "左对齐 · ⌘L"),
                ("Center · Ctrl+E", "居中 · ⌘E"),
                ("Align right · Ctrl+R", "右对齐 · ⌘R"),
                ("Justify · Ctrl+J", "两端对齐 · ⌘J"),
                ("Bulleted list · Ctrl+Shift+8", "项目符号列表 · ⌘⇧8"),
                ("Numbered list · Ctrl+Shift+7", "编号列表 · ⌘⇧7"),
                ("Decrease list level · Shift+Tab", "降低列表级别 · ⇧Tab"),
                ("Increase list level · Tab", "提高列表级别 · Tab"),
                ("Zoom out · Ctrl+-", "缩小 · ⌘-"),
                ("Zoom in · Ctrl+=", "放大 · ⌘="),
                ("Actual size · Ctrl+0", "适应页面 / 实际大小 · ⌘0"),
                ("Track Changes  Ctrl+Shift+E", "修订模式  ⌘⇧E"),
                ("Insert footnote  Ctrl+Alt+F", "插入脚注  ⌘⌥F"),
                ("Insert endnote  Ctrl+Alt+D", "插入尾注  ⌘⌥D"),
                ("New comment  Ctrl+Alt+M", "新建批注  ⌘⌥M"),
                ("New slide · Ctrl+M", "新建幻灯片 · ⌘M"),
                ("Duplicate slide · Ctrl+D", "复制幻灯片 · ⌘D"),
                ("Bulleted list · Tab / Shift+Tab change the level", "项目符号列表 · Tab / ⇧Tab 调整级别"),
                ("Present · F5 (Shift+F5 from this slide)", "放映幻灯片 · F5（⇧F5 从当前页开始）"),
            ]);
        } else {
            m.extend([
                ("Bold · Ctrl+B", "加粗 · Ctrl+B"),
                ("Italic · Ctrl+I", "斜体 · Ctrl+I"),
                ("Underline · Ctrl+U", "下划线 · Ctrl+U"),
                ("Normal text · Ctrl+Alt+0", "正文 · Ctrl+Alt+0"),
                ("Heading 1 · Ctrl+Alt+1", "标题 1 · Ctrl+Alt+1"),
                ("Heading 2 · Ctrl+Alt+2", "标题 2 · Ctrl+Alt+2"),
                ("Heading 3 · Ctrl+Alt+3", "标题 3 · Ctrl+Alt+3"),
                ("Align left · Ctrl+L", "左对齐 · Ctrl+L"),
                ("Center · Ctrl+E", "居中 · Ctrl+E"),
                ("Align right · Ctrl+R", "右对齐 · Ctrl+R"),
                ("Justify · Ctrl+J", "两端对齐 · Ctrl+J"),
                ("Bulleted list · Ctrl+Shift+8", "项目符号列表 · Ctrl+Shift+8"),
                ("Numbered list · Ctrl+Shift+7", "编号列表 · Ctrl+Shift+7"),
                ("Decrease list level · Shift+Tab", "降低列表级别 · Shift+Tab"),
                ("Increase list level · Tab", "提高列表级别 · Tab"),
                ("Zoom out · Ctrl+-", "缩小 · Ctrl+-"),
                ("Zoom in · Ctrl+=", "放大 · Ctrl+="),
                ("Actual size · Ctrl+0", "实际大小 · Ctrl+0"),
                ("New slide · Ctrl+M", "新建幻灯片 · Ctrl+M"),
                ("Duplicate slide · Ctrl+D", "复制幻灯片 · Ctrl+D"),
            ]);
        }
        m
    });
    map.get(en).copied()
}

/// `en` in the interface language (English when the catalog lacks it).
pub fn t(en: &'static str) -> &'static str {
    t_in(lang(), en)
}

/// [`t`] for any string (titles kept in data, built at run time); the
/// result borrows `en` when there's no translation.
pub fn t_dyn(en: &str) -> &str {
    match lang() {
        Lang::En => en,
        Lang::Id => indonesian(en).unwrap_or(en),
        Lang::Zh => chinese(en).unwrap_or(en),
    }
}

/// [`t`] for `lang` (testable without the global).
pub fn t_in(lang: Lang, en: &'static str) -> &'static str {
    match lang {
        Lang::En => en,
        Lang::Id => indonesian(en).unwrap_or(en),
        Lang::Zh => chinese(en).unwrap_or(en),
    }
}

/// A catalog string with `{}` placeholders filled in order:
/// `tf("Page {} of {}", &[&3, &12])`.
pub fn tf(template: &'static str, args: &[&dyn Display]) -> String {
    fill(t(template), args)
}

/// Fill `{}` placeholders in order (extra placeholders stay empty).
pub fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut args = args.iter();
    let mut rest = template;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        if let Some(a) = args.next() {
            out.push_str(&a.to_string());
        }
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// Marks an English string kept in data (a table of titles) as a catalog
/// key; translate it where it's shown with [`t`] or [`t_dyn`].
pub const fn tk(en: &'static str) -> &'static str {
    en
}

/// Whether `query` matches `text` or its translation, for searches that
/// should find a command by either language's name.
pub fn both_languages(en: &str) -> String {
    match indonesian(en) {
        Some(id) if id != en => format!("{en} {id}"),
        _ => en.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    #[std::prelude::v1::test]
    fn number_locale_follows_the_setting_then_the_interface() {
        use kantor_numfmt::DisplayLocale::{EnUs, Indonesian};
        assert_eq!(number_locale_for(None, Lang::Id), Indonesian);
        assert_eq!(number_locale_for(None, Lang::En), EnUs);
        assert_eq!(number_locale_for(Some(Lang::En), Lang::Id), EnUs);
        assert_eq!(number_locale_for(Some(Lang::Id), Lang::En), Indonesian);
    }

    #[std::prelude::v1::test]
    fn parses_locales() {
        assert_eq!(Lang::parse("id_ID.UTF-8"), Some(Lang::Id));
        assert_eq!(Lang::parse("id"), Some(Lang::Id));
        assert_eq!(Lang::parse("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(Lang::parse("de_DE"), Some(Lang::En));
        assert_eq!(Lang::parse("C"), None);
        assert_eq!(Lang::parse(""), None);
        assert_eq!(pick(Lang::Id, "Save", "Simpan"), "Simpan");
    }

    #[std::prelude::v1::test]
    fn fills_placeholders() {
        assert_eq!(fill("Page {} of {}", &[&3, &12]), "Page 3 of 12");
        assert_eq!(fill("{} rows", &[&"5"]), "5 rows");
        assert_eq!(fill("none", &[]), "none");
        assert_eq!(fill("{} and {}", &[&1]), "1 and ");
    }

    #[std::prelude::v1::test]
    fn looks_up_the_catalog() {
        assert_eq!(t_in(Lang::Id, "Save"), "Simpan");
        assert_eq!(t_in(Lang::En, "Save"), "Save");
        assert_eq!(
            t_in(Lang::Id, "no such string, kept"),
            "no such string, kept"
        );
        assert!(both_languages("Save").contains("Simpan"));
    }

    /// The UI crates whose sources are scanned for catalog keys.
    const UI_CRATES: &[&str] = &[
        "kantor",
        "kantor_ui",
        "sheets_ui",
        "docs_ui",
        "slides_ui",
        "pdf_ui",
    ];

    fn crates_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/")
            .to_path_buf()
    }

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                rust_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    /// Every string literal passed straight to `t(`, `tf(` or `tk(`, with
    /// where it was found. Raw strings and escapes beyond `\"`, `\\`, `\n`
    /// and `\t` aren't used for UI text.
    pub(super) fn keys_in_sources() -> BTreeMap<String, String> {
        let mut files = Vec::new();
        for c in UI_CRATES {
            rust_files(&crates_dir().join(c).join("src"), &mut files);
        }
        let mut keys = BTreeMap::new();
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap_or_default();
            // This file's own examples and tests aren't UI text.
            if f.ends_with("kantor_ui/src/i18n.rs") {
                continue;
            }
            let lines: Vec<&str> = text.lines().collect();
            for (line_no, line) in lines.iter().enumerate() {
                for call in ["t(", "tf(", "tk("] {
                    let mut rest = *line;
                    while let Some(i) = rest.find(call) {
                        let before = rest[..i].chars().next_back();
                        rest = &rest[i + call.len()..];
                        // `t(` must be the whole name, not `get(` or `split(`.
                        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                            continue;
                        }
                        // The literal right after the call, or on the next
                        // line when rustfmt moved it there.
                        let arg = if let Some(r) = rest.strip_prefix('"') {
                            r
                        } else if rest.trim().is_empty()
                            && let Some(r) = lines
                                .get(line_no + 1)
                                .and_then(|l| l.trim_start().strip_prefix('"'))
                        {
                            r
                        } else {
                            continue;
                        };
                        if let Some(s) = literal(arg) {
                            keys.entry(s).or_insert_with(|| {
                                format!(
                                    "{}:{}",
                                    f.strip_prefix(crates_dir()).unwrap_or(&f).display(),
                                    line_no + 1
                                )
                            });
                        }
                    }
                }
            }
        }
        keys
    }

    /// The contents of a string literal starting at `s` (after its `"`).
    fn literal(s: &str) -> Option<String> {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Some(out),
                '\\' => match chars.next()? {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    c => out.push(c),
                },
                c => out.push(c),
            }
        }
        // A literal continued on the next line: not used for keys.
        None
    }

    #[std::prelude::v1::test]
    fn catalog_has_every_key() {
        let missing: Vec<String> = keys_in_sources()
            .into_iter()
            .filter(|(k, _)| indonesian(k).is_none())
            .map(|(k, at)| format!("{at}: {k:?}"))
            .collect();
        assert!(
            missing.is_empty(),
            "{} string(s) have no Indonesian in crates/kantor_ui/i18n/*.tsv:\n{}",
            missing.len(),
            missing.join("\n")
        );
    }

    #[std::prelude::v1::test]
    fn catalog_lines_are_well_formed() {
        let mut seen: BTreeMap<String, (String, &str)> = BTreeMap::new();
        let mut problems = Vec::new();
        for (file, text) in CATALOG_FILES {
            for (n, line) in text.lines().enumerate() {
                if line.trim().is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((en, id)) = line.split_once('\t') else {
                    problems.push(format!("{file}:{}: no tab: {line:?}", n + 1));
                    continue;
                };
                if en.trim().is_empty() || id.trim().is_empty() {
                    problems.push(format!("{file}:{}: empty side: {line:?}", n + 1));
                }
                if id.contains('\t') {
                    problems.push(format!("{file}:{}: more than one tab", n + 1));
                }
                if en.matches("{}").count() != id.matches("{}").count() {
                    problems.push(format!("{file}:{}: placeholders differ: {line:?}", n + 1));
                }
                if let Some((other, f)) = seen.get(en)
                    && other != id
                {
                    problems.push(format!(
                        "{file}:{}: {en:?} is {id:?} here but {other:?} in {f}",
                        n + 1
                    ));
                }
                seen.insert(en.to_owned(), (id.to_owned(), file));
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    /// Catalog lines no code uses any more. Keys kept in data and shown with
    /// `t_dyn` must be listed in a test that checks them (see
    /// `extra_keys`), or marked with `tk("…")` in the code.
    #[std::prelude::v1::test]
    fn catalog_has_no_unused_lines() {
        let used: BTreeSet<String> = keys_in_sources().into_keys().collect();
        let unused: Vec<String> = CATALOG_FILES
            .iter()
            .flat_map(|(f, text)| parse_catalog(text).map(move |(en, _)| (f, en)))
            .filter(|(_, en)| !used.contains(en))
            .map(|(f, en)| format!("{f}: {en:?}"))
            .collect();
        assert!(
            unused.is_empty(),
            "{} catalog line(s) are not used by any t/tf/tk call:\n{}",
            unused.len(),
            unused.join("\n")
        );
    }
}
