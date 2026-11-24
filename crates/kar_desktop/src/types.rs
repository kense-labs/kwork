//! Shared domain types for Kar Desktop Workbench (Chat, Office Selection Binding, Providers).

use std::path::PathBuf;

use kar::chat::{ChatInfo, ChatTurn, Item, Role, TurnStatus};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OfficeKind {
    Word,
    Excel,
    Ppt,
}

impl OfficeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Word => "Word (.docx)",
            Self::Excel => "Excel (.xlsx)",
            Self::Ppt => "PPT (.pptx)",
        }
    }

    pub fn short_tag(self) -> &'static str {
        match self {
            Self::Word => "DOCX",
            Self::Excel => "XLSX",
            Self::Ppt => "PPTX",
        }
    }

    pub fn for_path(path: &std::path::Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "docx" | "doc" | "rtf" | "odt" => Some(Self::Word),
            "xlsx" | "xls" | "csv" | "ods" => Some(Self::Excel),
            "pptx" | "ppt" | "odp" => Some(Self::Ppt),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SelectionLocator {
    WordParagraph {
        start_para: usize,
        end_para: usize,
        total_paras: usize,
        style: String,
    },
    ExcelRange {
        sheet_index: u32,
        sheet_name: String,
        range: String,
        active_cell: String,
        raw_input: String,
        computed_value: String,
    },
    PptObject {
        slide_index: usize,
        slide_count: usize,
        shape_id: Option<u32>,
        shape_index: Option<usize>,
        shape_name: String,
        shape_kind: String,
        bounds_cm: Option<(f64, f64, f64, f64)>,
    },
}

impl SelectionLocator {
    pub fn target_label(&self) -> String {
        match self {
            Self::WordParagraph {
                start_para,
                end_para,
                ..
            } => {
                if start_para == end_para {
                    format!("段落 #{}", start_para + 1)
                } else {
                    format!("段落 #{}–#{}", start_para + 1, end_para + 1)
                }
            }
            Self::ExcelRange {
                sheet_name,
                range,
                active_cell,
                ..
            } => {
                if range == active_cell {
                    format!("{sheet_name}!{active_cell}")
                } else {
                    format!("{sheet_name}!{range} (活动格 {active_cell})")
                }
            }
            Self::PptObject {
                slide_index,
                shape_id,
                shape_name,
                ..
            } => match shape_id {
                Some(id) if !shape_name.is_empty() => {
                    format!("第 {} 页 · 对象 #{id} ({shape_name})", slide_index + 1)
                }
                Some(id) => format!("第 {} 页 · 对象 #{id}", slide_index + 1),
                None => format!("第 {} 页（整页，未选中单个形状）", slide_index + 1),
            },
        }
    }
}

/// Captures a concrete Office document selection so Chat turns permanently remember
/// which file & paragraph/cell/slide-object a user request targeted, even across tab switches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectionTarget {
    pub tab_id: u64,
    pub file_path: Option<PathBuf>,
    pub doc_name: String,
    pub kind: OfficeKind,
    pub base_revision: Option<u64>,
    pub locator: SelectionLocator,
    pub excerpt: String,
}

pub fn format_rev(rev: u64) -> String {
    if rev > 1_000_000 {
        format!("rev #{:08x}", rev as u32)
    } else {
        format!("rev #{rev}")
    }
}

impl SelectionTarget {
    pub fn short_badge(&self) -> String {
        format!("{} · {}", self.doc_name, self.locator.target_label())
    }

    pub fn detail_line(&self) -> String {
        let rev = self
            .base_revision
            .map_or_else(|| "rev -".to_owned(), format_rev);
        format!(
            "[{}] {} · {} ({rev})",
            self.kind.short_tag(),
            self.doc_name,
            self.locator.target_label()
        )
    }

    pub fn compose_prompt(&self, instruction: &str) -> String {
        let path_str = self
            .file_path
            .as_ref()
            .map_or_else(|| self.doc_name.clone(), |p| p.display().to_string());
        let extra = match &self.locator {
            SelectionLocator::WordParagraph { style, .. } => format!("段落样式: {style}"),
            SelectionLocator::ExcelRange {
                raw_input,
                computed_value,
                ..
            } => format!("单元格输入/公式: `{raw_input}`，当前计算结果: `{computed_value}`"),
            SelectionLocator::PptObject {
                shape_kind,
                bounds_cm,
                ..
            } => match bounds_cm {
                Some((x, y, w, h)) => {
                    format!(
                        "对象类型: {shape_kind}，位置尺寸: x={x:.2}cm, y={y:.2}cm, w={w:.2}cm, h={h:.2}cm"
                    )
                }
                None => format!("对象类型: {shape_kind}"),
            },
        };
        format!(
            "请针对以下 Office 文档选区给出修改方案或执行修改：\n\
             - 文件路径: {path_str}\n\
             - 文档类型: {}\n\
             - 目标位置: {}\n\
             - {extra}\n\
             - 当前内容快照: {}\n\n\
             用户修改要求：\n{instruction}",
            self.kind.label(),
            self.locator.target_label(),
            if self.excerpt.is_empty() {
                "(空)"
            } else {
                &self.excerpt
            },
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderAvailability {
    Available {
        version: String,
        command: Option<PathBuf>,
    },
    Missing,
    Failed {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderAuthState {
    LoggedIn { summary: String },
    LoggedOut { source: String },
    Unknown { note: String },
    NotChecked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderStatusInfo {
    pub id: String,
    pub protocol: String,
    pub availability: ProviderAvailability,
    pub auth: ProviderAuthState,
}

impl ProviderStatusInfo {
    pub fn is_ready(&self) -> bool {
        matches!(self.availability, ProviderAvailability::Available { .. })
            && !matches!(self.auth, ProviderAuthState::LoggedOut { .. })
    }

    pub fn status_badge(&self) -> (&'static str, bool) {
        match (&self.availability, &self.auth) {
            (ProviderAvailability::Available { .. }, ProviderAuthState::LoggedIn { .. }) => {
                ("可用 · 已登录", true)
            }
            (ProviderAvailability::Available { .. }, ProviderAuthState::LoggedOut { .. }) => {
                ("缺账号 · 未登录", false)
            }
            (ProviderAvailability::Available { .. }, _) => ("已安装 · 凭据待确认", true),
            (ProviderAvailability::Missing, _) => ("缺程序 · 未安装", false),
            (ProviderAvailability::Failed { .. }, _) => ("程序就绪检查失败", false),
        }
    }

    pub fn diagnostic_reason(&self) -> Option<String> {
        match (&self.availability, &self.auth) {
            (ProviderAvailability::Missing, _) => Some(format!(
                "未在本机 PATH 或默认安装目录检测到 `{}` 可执行程序（可通过 `kar install {}` 查看安装方案，或切换至独立演示入口）。",
                self.id, self.id
            )),
            (ProviderAvailability::Failed { reason }, _) => Some(format!(
                "`{}` 可执行程序未通过就绪检查：{reason}",
                self.id
            )),
            (
                ProviderAvailability::Available { version, command },
                ProviderAuthState::LoggedOut { source },
            ) => {
                let cmd = command
                    .as_ref()
                    .map_or_else(|| self.id.clone(), |p| p.display().to_string());
                Some(format!(
                    "已检测到 `{}` ({version}，路径 `{cmd}`)，但当前账号处于未登录状态（检测来源：`{source}`）。请先执行 `kar account {} login` 登录，或进入独立演示模式体验。",
                    self.id, self.id
                ))
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChatListEntry {
    pub info: ChatInfo,
    pub title: String,
    pub turn_count: usize,
    pub last_status: Option<TurnStatus>,
}

impl ChatListEntry {
    pub fn from_info_and_turns(info: ChatInfo, turns: &[ChatTurn]) -> Self {
        let title = info
            .settings
            .name
            .clone()
            .or_else(|| {
                turns.iter().find_map(|t| {
                    t.items.iter().find_map(|item| match &item.item {
                        Item::Message {
                            role: Role::User,
                            text,
                            ..
                        } => {
                            let first_line = text.lines().next().unwrap_or(text).trim();
                            if first_line.is_empty() {
                                None
                            } else {
                                let clipped: String = first_line.chars().take(34).collect();
                                Some(if first_line.chars().count() > 34 {
                                    format!("{clipped}…")
                                } else {
                                    clipped
                                })
                            }
                        }
                        _ => None,
                    })
                })
            })
            .unwrap_or_else(|| format!("新对话 {}", &info.id.to_string()[..8]));
        let last_status = turns.last().map(|t| t.status);
        Self {
            info,
            title,
            turn_count: turns.len(),
            last_status,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatAccessMode {
    NoneSelected,
    WriteController {
        chat_id: Uuid,
    },
    LocalReadOnlySubscriber {
        chat_id: Uuid,
        reason: String,
    },
    HttpReadOnly {
        url: String,
        chat_id: Option<Uuid>,
    },
    DemoShowcase,
}

impl ChatAccessMode {
    pub fn is_writable(&self) -> bool {
        matches!(self, Self::WriteController { .. } | Self::DemoShowcase)
    }

    pub fn is_http_read_only(&self) -> bool {
        matches!(self, Self::HttpReadOnly { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeedConnectionState {
    Idle,
    Connecting {
        endpoint: String,
    },
    Connected {
        cursor: Option<String>,
        duplicates_ignored: u64,
        gaps_recovered: u64,
    },
    Reconnecting {
        last_cursor: Option<String>,
        reason: String,
        attempt: u32,
    },
    Disconnected {
        last_cursor: Option<String>,
        reason: String,
    },
}

/// Captures a user prompt attempt so retries reuse the exact same request UUID.
#[derive(Clone, Debug)]
pub struct PendingPromptAttempt {
    pub request_id: Uuid,
    pub raw_user_text: String,
    pub sent_payload: String,
    pub target: Option<SelectionTarget>,
    pub attempts: u32,
    pub in_flight: bool,
    pub last_error: Option<String>,
}
