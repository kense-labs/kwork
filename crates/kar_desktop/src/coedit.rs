//! Replaceable Rust interface for Office human-AI co-editing + strictly isolated Demo implementation.
//!
//! Kar Phase 2's durable co-editing protocol is not yet published in Kar backend (`19f38db`).
//! Frontend code talks exclusively to [`CoEditService`], while [`DemoCoEditEngine`] drives
//! explicitly badged `"演示数据"` scenarios on isolated demo documents only.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::OfficeKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScenarioKey {
    WordSameParagraph,
    ExcelSameCellConflict,
    PptMoveAndText,
}

impl ScenarioKey {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WordSameParagraph => "word_same_para",
            Self::ExcelSameCellConflict => "excel_same_cell",
            Self::PptMoveAndText => "ppt_move_text",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::WordSameParagraph => "Word：人改日期 + AI 改同段负责人（双项保留）",
            Self::ExcelSameCellConflict => "Excel：同一单元格 B3 人机异值冲突（保留/采用/手工合并）",
            Self::PptMoveAndText => "PPT：人移文本框 + AI 改文字（正交生效 / 删除冲突）",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CoEditStatus {
    /// Non-overlapping / orthogonal edits can preserve both human and AI changes.
    ReadyDualKeep,
    /// Overlapping same-target edits require user choice (Keep / Adopt / Manual Merge).
    ConflictNeedsChoice,
    /// Target object was deleted by the human while AI targeted it.
    TargetDeletedConflict,
    /// Document changed again while the user was resolving conflict; requires re-confirmation.
    StaleNeedsReconfirm {
        seen_revision: u64,
        latest_revision: u64,
    },
    /// Transport or host timeout left the mutation outcome unconfirmed; never auto-replayed.
    OutcomeUnknown {
        reason: String,
        last_known_revision: u64,
    },
    /// Completed with recorded outcome.
    Resolved { summary: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserResolutionChoice {
    ApplyDualKeep,
    KeepCurrent,
    AdoptProposal,
    ManualMerge { merged_value: String },
    DismissDeletedTarget,
}

/// Live state sampled from the isolated demo Kantor view (`DocsView` / `SheetsView` / `SlidesView`).
#[derive(Clone, Debug, PartialEq)]
pub struct LiveTargetState {
    pub scenario: ScenarioKey,
    pub revision: u64,
    /// Current text / cell input in the live demo editor.
    pub current_value: String,
    /// Extra computed display (e.g. Excel dependent formula result or PPT `(x, y)` in cm).
    pub current_aux: String,
    /// For PPT shapes: whether the target shape ID still exists on the slide, and its `(x, y)` in cm.
    pub target_exists: bool,
    pub ppt_xy_cm: Option<(f64, f64)>,
}

/// Complete co-editing record covering every required protocol field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CoEditProposal {
    pub scenario: ScenarioKey,
    pub kind: OfficeKind,
    /// Explicit data source badge (`"演示数据"`).
    pub data_source_badge: String,
    /// Unique operation ID (`operation_id`).
    pub operation_id: Uuid,
    /// Revision the AI proposal was computed against (`base_revision`).
    pub base_revision: u64,
    /// Revision last acknowledged by the user in the conflict UI.
    pub acknowledged_revision: u64,
    /// Latest live revision observed on the document.
    pub latest_revision: u64,
    /// Human-readable target selection (`段落 #2` / `Sheet1!B3` / `第 1 页 · 形状 #id`).
    pub selection_desc: String,
    /// Original value at `base_revision`.
    pub base_value: String,
    /// Current value in the editor right now.
    pub current_value: String,
    /// AI's proposed value against `base_revision`.
    pub proposed_value: String,
    /// Computed dual-keep / merged value when orthogonal or span-separable.
    pub merged_preview: Option<String>,
    /// Why a conflict occurred, if any.
    pub conflict_reason: Option<String>,
    /// Final outcome description once resolved.
    pub resolution_result: Option<String>,
    pub status: CoEditStatus,
    pub target_shape_id: Option<u32>,
}

/// Mutation instruction returned to the frontend Office host to apply onto the demo tab.
#[derive(Clone, Debug, PartialEq)]
pub enum DemoEditorMutation {
    None,
    ReplaceWordParagraph {
        para_idx: usize,
        text: String,
    },
    WriteExcelCell {
        sheet: u32,
        cell_a1: String,
        value: String,
    },
    UpdatePptShape {
        slide: usize,
        shape_id: u32,
        text: Option<String>,
        xy_cm: Option<(f64, f64)>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubmissionOutcome {
    /// Document changed after the user started reviewing; user must inspect `latest_revision` and re-confirm.
    NeedsReconfirm {
        previous_revision: u64,
        latest_revision: u64,
        latest_value: String,
    },
    /// Resolution accepted; apply `mutation` to the isolated demo editor.
    Accepted {
        proposal: Box<CoEditProposal>,
        mutation: DemoEditorMutation,
    },
}

/// Replaceable Rust trait for Kar Phase-2 Office co-editing backends.
pub trait CoEditService: Send + Sync {
    fn backend_label(&self) -> &'static str;
    fn is_demo(&self) -> bool;
    fn proposals(&self) -> &[CoEditProposal];
    fn sync_live_state(&mut self, live: &LiveTargetState) -> Option<CoEditProposal>;
    fn acknowledge_latest_revision(&mut self, operation_id: Uuid, latest_revision: u64) -> Result<()>;
    fn reset_scenario(&mut self, scenario: ScenarioKey, base_rev: u64, shape_id: Option<u32>);
    fn submit_decision(
        &mut self,
        operation_id: Uuid,
        choice: UserResolutionChoice,
        live: &LiveTargetState,
    ) -> Result<SubmissionOutcome>;
}

pub struct DemoCoEditEngine {
    items: Vec<CoEditProposal>,
}

impl Default for DemoCoEditEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DemoCoEditEngine {
    pub fn new() -> Self {
        let mut engine = Self { items: Vec::new() };
        engine.items = vec![
            Self::build_word_proposal(1),
            Self::build_excel_proposal(1),
            Self::build_ppt_proposal(1, Some(2)),
        ];
        engine
    }

    fn build_word_proposal(base_rev: u64) -> CoEditProposal {
        let base = "发布排期：日期 2026-11-24，负责人：待定，状态：评审中";
        let ai_prop = "发布排期：日期 2026-11-24，负责人：林远，状态：评审中";
        CoEditProposal {
            scenario: ScenarioKey::WordSameParagraph,
            kind: OfficeKind::Word,
            data_source_badge: "演示数据".into(),
            operation_id: Uuid::from_u128(0x019a_ce01_0000_7000_8000_0000_0000_0101),
            base_revision: base_rev,
            acknowledged_revision: base_rev,
            latest_revision: base_rev,
            selection_desc: "[演示] demo_coedit_schedule.docx · 段落 #2".into(),
            base_value: base.into(),
            current_value: base.into(),
            proposed_value: ai_prop.into(),
            merged_preview: Some(ai_prop.into()),
            conflict_reason: None,
            resolution_result: None,
            status: CoEditStatus::ReadyDualKeep,
            target_shape_id: None,
        }
    }

    fn build_excel_proposal(base_rev: u64) -> CoEditProposal {
        CoEditProposal {
            scenario: ScenarioKey::ExcelSameCellConflict,
            kind: OfficeKind::Excel,
            data_source_badge: "演示数据".into(),
            operation_id: Uuid::from_u128(0x019a_ce02_0000_7000_8000_0000_0000_0202),
            base_revision: base_rev,
            acknowledged_revision: base_rev,
            latest_revision: base_rev,
            selection_desc: "[演示] demo_coedit_budget.xlsx · Sheet1!B3".into(),
            base_value: "120000".into(),
            current_value: "120000".into(),
            proposed_value: "148000".into(),
            merged_preview: None,
            conflict_reason: None,
            resolution_result: None,
            status: CoEditStatus::ReadyDualKeep,
            target_shape_id: None,
        }
    }

    fn build_ppt_proposal(base_rev: u64, shape_id: Option<u32>) -> CoEditProposal {
        let sid = shape_id.unwrap_or(2);
        CoEditProposal {
            scenario: ScenarioKey::PptMoveAndText,
            kind: OfficeKind::Ppt,
            data_source_badge: "演示数据".into(),
            operation_id: Uuid::from_u128(0x019a_ce03_0000_7000_8000_0000_0000_0303),
            base_revision: base_rev,
            acknowledged_revision: base_rev,
            latest_revision: base_rev,
            selection_desc: format!("[演示] demo_coedit_launch.pptx · 第 1 页 · 文本框 #{sid}"),
            base_value: "初版标题：Q4 增长复盘 @ (2.00cm, 3.00cm)".into(),
            current_value: "初版标题：Q4 增长复盘 @ (2.00cm, 3.00cm)".into(),
            proposed_value: "终版标题：Kar 原生双写工作台发布（保持人工移动后的新坐标）".into(),
            merged_preview: Some("终版标题：Kar 原生双写工作台发布 @ (2.00cm, 3.00cm)".into()),
            conflict_reason: None,
            resolution_result: None,
            status: CoEditStatus::ReadyDualKeep,
            target_shape_id: Some(sid),
        }
    }

    /// Merge human's date change with AI's owner change in the same Word paragraph when
    /// the human preserved `"负责人：待定"`. If human ALSO edited the owner clause, flag conflict!
    fn compute_word_merge(current_text: &str) -> (Option<String>, Option<String>) {
        if let Some(merged) = current_text
            .find("负责人：待定")
            .map(|pos| {
                let mut s = String::with_capacity(current_text.len() + 8);
                s.push_str(&current_text[..pos]);
                s.push_str("负责人：林远");
                s.push_str(&current_text[pos + "负责人：待定".len()..]);
                s
            })
        {
            (
                Some(merged),
                Some("人修改了同段的排期日期，AI 修改了同段的负责人字段，两处修改区间不重叠，可同时保留。".into()),
            )
        } else if current_text.contains("负责人：林远") {
            (Some(current_text.to_owned()), None)
        } else {
            (
                None,
                Some("人工已同时改动了该段的「负责人」字段，无法自动无损合并，请选择保留当前值、采用 AI 提议或手工合并。".into()),
            )
        }
    }
}

impl CoEditService for DemoCoEditEngine {
    fn backend_label(&self) -> &'static str {
        "样例驱动双写引擎（演示数据 · 不写回用户文件）"
    }

    fn is_demo(&self) -> bool {
        true
    }

    fn proposals(&self) -> &[CoEditProposal] {
        &self.items
    }

    fn reset_scenario(&mut self, scenario: ScenarioKey, base_rev: u64, shape_id: Option<u32>) {
        let fresh = match scenario {
            ScenarioKey::WordSameParagraph => Self::build_word_proposal(base_rev),
            ScenarioKey::ExcelSameCellConflict => Self::build_excel_proposal(base_rev),
            ScenarioKey::PptMoveAndText => Self::build_ppt_proposal(base_rev, shape_id),
        };
        if let Some(slot) = self.items.iter_mut().find(|p| p.scenario == scenario) {
            *slot = fresh;
        }
    }

    fn acknowledge_latest_revision(
        &mut self,
        operation_id: Uuid,
        latest_revision: u64,
    ) -> Result<()> {
        let prop = self
            .items
            .iter_mut()
            .find(|p| p.operation_id == operation_id)
            .ok_or_else(|| anyhow::anyhow!("未找到双写操作 {operation_id}"))?;
        prop.acknowledged_revision = latest_revision;
        prop.latest_revision = latest_revision;
        // Recompute non-stale status after user acknowledges the new revision
        if matches!(prop.status, CoEditStatus::StaleNeedsReconfirm { .. }) {
            prop.status = match prop.scenario {
                ScenarioKey::WordSameParagraph if prop.merged_preview.is_some() => {
                    CoEditStatus::ReadyDualKeep
                }
                ScenarioKey::PptMoveAndText if prop.merged_preview.is_some() => {
                    CoEditStatus::ReadyDualKeep
                }
                ScenarioKey::PptMoveAndText if prop.merged_preview.is_none() => {
                    CoEditStatus::TargetDeletedConflict
                }
                _ => CoEditStatus::ConflictNeedsChoice,
            };
        }
        Ok(())
    }

    fn sync_live_state(&mut self, live: &LiveTargetState) -> Option<CoEditProposal> {
        let prop = self.items.iter_mut().find(|p| p.scenario == live.scenario)?;
        if matches!(prop.status, CoEditStatus::Resolved { .. }) {
            return Some(prop.clone());
        }

        let prev_latest = prop.latest_revision;
        prop.latest_revision = live.revision;

        match live.scenario {
            ScenarioKey::WordSameParagraph => {
                prop.current_value = live.current_value.clone();
                let (merged, note) = Self::compute_word_merge(&live.current_value);
                prop.merged_preview = merged.clone();
                prop.conflict_reason = note;
                if prop.acknowledged_revision == prop.base_revision && prev_latest == prop.base_revision {
                    // Initial human edit transitioning from base -> active scenario
                    prop.acknowledged_revision = live.revision;
                }
                if live.revision != prop.acknowledged_revision {
                    prop.status = CoEditStatus::StaleNeedsReconfirm {
                        seen_revision: prop.acknowledged_revision,
                        latest_revision: live.revision,
                    };
                } else if merged.is_some() {
                    prop.status = CoEditStatus::ReadyDualKeep;
                } else {
                    prop.status = CoEditStatus::ConflictNeedsChoice;
                }
            }
            ScenarioKey::ExcelSameCellConflict => {
                if live.current_value == prop.base_value && prop.conflict_reason.is_none() {
                    prop.base_revision = live.revision;
                    prop.acknowledged_revision = live.revision;
                    prop.current_value = live.current_value.clone();
                    prop.status = CoEditStatus::ReadyDualKeep;
                    return Some(prop.clone());
                }
                if prop.acknowledged_revision == prop.base_revision && prev_latest == prop.base_revision {
                    prop.acknowledged_revision = live.revision;
                }
                prop.current_value = live.current_value.clone();
                if live.current_value != prop.base_value
                    && live.current_value != prop.proposed_value
                {
                    prop.conflict_reason = Some(format!(
                        "单元格 Sheet1!B3 原值为「{}」，人工已改为「{}」（{}），而 AI 基于 {} 提议改为「{}」。",
                        prop.base_value,
                        live.current_value,
                        live.current_aux,
                        crate::types::format_rev(prop.base_revision),
                        prop.proposed_value
                    ));
                    if live.revision != prop.acknowledged_revision {
                        prop.status = CoEditStatus::StaleNeedsReconfirm {
                            seen_revision: prop.acknowledged_revision,
                            latest_revision: live.revision,
                        };
                    } else {
                        prop.status = CoEditStatus::ConflictNeedsChoice;
                    }
                } else if live.revision != prop.acknowledged_revision {
                    prop.status = CoEditStatus::StaleNeedsReconfirm {
                        seen_revision: prop.acknowledged_revision,
                        latest_revision: live.revision,
                    };
                } else {
                    prop.conflict_reason = None;
                    prop.status = CoEditStatus::ReadyDualKeep;
                }
            }
            ScenarioKey::PptMoveAndText => {
                if !live.target_exists {
                    prop.current_value = "(目标文本框已被人工删除)".into();
                    prop.merged_preview = None;
                    prop.conflict_reason = Some(format!(
                        "目标冲突：AI 提议修改第 1 页文本框 #{} 的文字，但该文本框已被人工删除（当前 rev #{}）。",
                        prop.target_shape_id.unwrap_or(2),
                        live.revision
                    ));
                    prop.acknowledged_revision = live.revision;
                    prop.status = CoEditStatus::TargetDeletedConflict;
                } else {
                    let (x, y) = live.ppt_xy_cm.unwrap_or((2.0, 3.0));
                    prop.current_value =
                        format!("{} @ ({x:.2}cm, {y:.2}cm)", live.current_value);
                    prop.merged_preview = Some(format!(
                        "终版标题：Kar 原生双写工作台发布 @ ({x:.2}cm, {y:.2}cm)"
                    ));
                    prop.conflict_reason = Some(format!(
                        "人工将文本框移动至 ({x:.2}cm, {y:.2}cm)，AI 基于 rev #{} 修改文本框文字，位置与文字正交可同时生效。",
                        prop.base_revision
                    ));
                    if prop.acknowledged_revision == prop.base_revision
                        && prev_latest == prop.base_revision
                    {
                        prop.acknowledged_revision = live.revision;
                    }
                    if live.revision != prop.acknowledged_revision {
                        prop.status = CoEditStatus::StaleNeedsReconfirm {
                            seen_revision: prop.acknowledged_revision,
                            latest_revision: live.revision,
                        };
                    } else {
                        prop.status = CoEditStatus::ReadyDualKeep;
                    }
                }
            }
        }

        Some(prop.clone())
    }

    fn submit_decision(
        &mut self,
        operation_id: Uuid,
        choice: UserResolutionChoice,
        live: &LiveTargetState,
    ) -> Result<SubmissionOutcome> {
        let _ = self.sync_live_state(live);
        let prop = self
            .items
            .iter_mut()
            .find(|p| p.operation_id == operation_id)
            .ok_or_else(|| anyhow::anyhow!("未找到双写操作 {operation_id}"))?;

        // Guard: if the document changed again while the conflict UI was open, force re-confirmation!
        if live.revision != prop.acknowledged_revision {
            let prev = prop.acknowledged_revision;
            prop.status = CoEditStatus::StaleNeedsReconfirm {
                seen_revision: prev,
                latest_revision: live.revision,
            };
            return Ok(SubmissionOutcome::NeedsReconfirm {
                previous_revision: prev,
                latest_revision: live.revision,
                latest_value: prop.current_value.clone(),
            });
        }

        let (summary, mutation) = match (prop.scenario, choice) {
            (ScenarioKey::WordSameParagraph, UserResolutionChoice::ApplyDualKeep) => {
                let Some(merged) = prop.merged_preview.clone() else {
                    bail!("当前段落负责人字段也发生了冲突，无法自动双项保留，请使用保留/采用/手工合并");
                };
                (
                    format!("已合并保留人工日期与 AI 负责人：「{merged}」"),
                    DemoEditorMutation::ReplaceWordParagraph {
                        para_idx: 1,
                        text: merged,
                    },
                )
            }
            (ScenarioKey::WordSameParagraph, UserResolutionChoice::KeepCurrent) => (
                format!("已保留当前段落内容：「{}」", prop.current_value),
                DemoEditorMutation::None,
            ),
            (ScenarioKey::WordSameParagraph, UserResolutionChoice::AdoptProposal) => (
                format!("已采用 AI 提议覆盖段落：「{}」", prop.proposed_value),
                DemoEditorMutation::ReplaceWordParagraph {
                    para_idx: 1,
                    text: prop.proposed_value.clone(),
                },
            ),
            (
                ScenarioKey::WordSameParagraph,
                UserResolutionChoice::ManualMerge { merged_value },
            ) => {
                if merged_value.trim().is_empty() {
                    bail!("手工合并文本不能为空");
                }
                (
                    format!("已提交手工合并段落：「{merged_value}」"),
                    DemoEditorMutation::ReplaceWordParagraph {
                        para_idx: 1,
                        text: merged_value,
                    },
                )
            }
            (ScenarioKey::ExcelSameCellConflict, UserResolutionChoice::KeepCurrent) => (
                format!(
                    "已保留 Sheet1!B3 当前值「{}」（原值「{}」，AI 提议「{}」已拒绝）",
                    live.current_value, prop.base_value, prop.proposed_value
                ),
                DemoEditorMutation::None,
            ),
            (
                ScenarioKey::ExcelSameCellConflict,
                UserResolutionChoice::AdoptProposal | UserResolutionChoice::ApplyDualKeep,
            ) => (
                format!(
                    "已采用 AI 提议将 Sheet1!B3 更新为「{}」（原值「{}」，覆盖前当前值「{}」）",
                    prop.proposed_value, prop.base_value, live.current_value
                ),
                DemoEditorMutation::WriteExcelCell {
                    sheet: 0,
                    cell_a1: "B3".into(),
                    value: prop.proposed_value.clone(),
                },
            ),
            (
                ScenarioKey::ExcelSameCellConflict,
                UserResolutionChoice::ManualMerge { merged_value },
            ) => {
                let trimmed = merged_value.trim().to_owned();
                if trimmed.is_empty() {
                    bail!("请输入手工合并后的单元格数值或公式（如 142000 或 =(135000+148000)/2）");
                }
                (
                    format!("已将手工合并值「{trimmed}」写入 Sheet1!B3 并重算关联公式"),
                    DemoEditorMutation::WriteExcelCell {
                        sheet: 0,
                        cell_a1: "B3".into(),
                        value: trimmed,
                    },
                )
            }
            (ScenarioKey::PptMoveAndText, UserResolutionChoice::ApplyDualKeep) => {
                if !live.target_exists {
                    bail!("目标冲突：目标文本框已被删除，无法应用文字修改");
                }
                let sid = prop.target_shape_id.unwrap_or(2);
                let xy = live.ppt_xy_cm;
                let (x, y) = xy.unwrap_or((2.0, 3.0));
                (
                    format!(
                        "两项修改均已生效：保留人工新坐标 ({x:.2}cm, {y:.2}cm) + 应用 AI 新文字「终版标题：Kar 原生双写工作台发布」"
                    ),
                    DemoEditorMutation::UpdatePptShape {
                        slide: 0,
                        shape_id: sid,
                        text: Some("终版标题：Kar 原生双写工作台发布".into()),
                        xy_cm: xy,
                    },
                )
            }
            (ScenarioKey::PptMoveAndText, UserResolutionChoice::KeepCurrent) => (
                format!("已保留当前幻灯片状态：{}", prop.current_value),
                DemoEditorMutation::None,
            ),
            (ScenarioKey::PptMoveAndText, UserResolutionChoice::DismissDeletedTarget) => (
                format!(
                    "已确认目标冲突并放弃对已删除文本框 #{} 的 AI 文字修改",
                    prop.target_shape_id.unwrap_or(2)
                ),
                DemoEditorMutation::None,
            ),
            _ => bail!("当前场景不支持该处理选项"),
        };

        prop.resolution_result = Some(summary.clone());
        prop.status = CoEditStatus::Resolved { summary };
        Ok(SubmissionOutcome::Accepted {
            proposal: Box::new(prop.clone()),
            mutation,
        })
    }
}

// ── Kar Phase 2: Durable Revision-Fenced Live Office Ledger (`RevisionFenceLedger`) ──────────

use std::collections::BTreeMap;
use std::path::Path;
use ring::digest::{SHA256, digest};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FenceOutcome {
    Applied { new_revision: u64 },
    FencedConflict {
        expected_revision: u64,
        live_revision: u64,
        target_desc: String,
        proposed_summary: String,
    },
    OutcomeUnknown {
        reason: String,
        last_known_revision: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FenceReceipt {
    pub operation_id: Uuid,
    pub doc_key: String,
    pub kind: OfficeKind,
    pub base_revision: u64,
    pub snapshot_digest: String,
    pub outcome: FenceOutcome,
    pub recorded_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveFencedConflict {
    pub operation_id: Uuid,
    pub doc_key: String,
    pub kind: OfficeKind,
    pub expected_revision: u64,
    pub live_revision: u64,
    pub target_desc: String,
    pub current_summary: String,
    pub proposed_summary: String,
    pub pending_mutation: Option<FenceMutationPayload>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FenceMutationPayload {
    WordParagraph { paragraph_1based: usize, text: String },
    ExcelCell { sheet_idx: u32, cell_a1: String, value: String },
    PptShapeText { slide_1based: usize, shape_id: u32, text: String },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RevisionFenceLedger {
    /// Monotonic revision per open/tracked document key (`canonical path or tab name -> rev`).
    pub revisions: BTreeMap<String, u64>,
    /// Immutable SHA-256 content/state fingerprint per `(doc_key, rev)`.
    pub digests: BTreeMap<String, String>,
    /// Idempotent operation receipts (`operation_id -> receipt`).
    pub receipts: BTreeMap<Uuid, FenceReceipt>,
    /// Active fenced conflicts awaiting explicit human choice in the Office sheet.
    pub conflicts: Vec<LiveFencedConflict>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FenceAdmission {
    Proceed { operation_id: Uuid, base_revision: u64 },
    Duplicate(FenceReceipt),
    BlockedConflict(LiveFencedConflict),
}

impl RevisionFenceLedger {
    pub fn load(docs_dir: &Path) -> Self {
        let path = docs_dir.join(".coedit-fence-ledger.json");
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, docs_dir: &Path) {
        let _ = std::fs::create_dir_all(docs_dir);
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let tmp = docs_dir.join(".coedit-fence-ledger.json.tmp");
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(tmp, docs_dir.join(".coedit-fence-ledger.json"));
            }
        }
    }

    pub fn current_revision(&mut self, doc_key: &str) -> u64 {
        *self.revisions.entry(doc_key.to_owned()).or_insert(1)
    }

    pub fn bump_human_edit(&mut self, doc_key: &str, state_fingerprint: &str) -> u64 {
        let rev = self.revisions.entry(doc_key.to_owned()).or_insert(1);
        *rev = rev.saturating_add(1);
        let next = *rev;
        let hex = sha256_hex(state_fingerprint.as_bytes());
        self.digests.insert(format!("{doc_key}#{next}"), hex);
        next
    }

    /// Check `operation_id` idempotency and `expected_revision` fence before mutating an Office tab.
    #[allow(clippy::too_many_arguments)]
    pub fn check_admission(
        &mut self,
        doc_key: &str,
        kind: OfficeKind,
        operation_id: Option<Uuid>,
        expected_revision: Option<u64>,
        target_desc: String,
        current_summary: String,
        proposed_summary: String,
        mutation: Option<FenceMutationPayload>,
        now_ms: u64,
    ) -> FenceAdmission {
        let op_id = operation_id.unwrap_or_else(Uuid::new_v4);
        if let Some(existing) = self.receipts.get(&op_id) {
            return FenceAdmission::Duplicate(existing.clone());
        }
        let live_rev = self.current_revision(doc_key);
        if let Some(exp) = expected_revision
            && exp != live_rev
        {
            let conflict = LiveFencedConflict {
                operation_id: op_id,
                doc_key: doc_key.to_owned(),
                kind,
                expected_revision: exp,
                live_revision: live_rev,
                target_desc: target_desc.clone(),
                current_summary,
                proposed_summary: proposed_summary.clone(),
                pending_mutation: mutation,
            };
            self.conflicts.retain(|c| c.operation_id != op_id);
            self.conflicts.push(conflict.clone());
            let snap_digest = self
                .digests
                .get(&format!("{doc_key}#{live_rev}"))
                .cloned()
                .unwrap_or_else(|| sha256_hex(doc_key.as_bytes()));
            self.receipts.insert(
                op_id,
                FenceReceipt {
                    operation_id: op_id,
                    doc_key: doc_key.to_owned(),
                    kind,
                    base_revision: exp,
                    snapshot_digest: snap_digest,
                    outcome: FenceOutcome::FencedConflict {
                        expected_revision: exp,
                        live_revision: live_rev,
                        target_desc,
                        proposed_summary,
                    },
                    recorded_ms: now_ms,
                },
            );
            return FenceAdmission::BlockedConflict(conflict);
        }
        FenceAdmission::Proceed {
            operation_id: op_id,
            base_revision: live_rev,
        }
    }

    pub fn commit_applied(
        &mut self,
        doc_key: &str,
        kind: OfficeKind,
        operation_id: Uuid,
        base_revision: u64,
        state_fingerprint: &str,
        now_ms: u64,
    ) -> u64 {
        let next = base_revision.saturating_add(1);
        self.revisions.insert(doc_key.to_owned(), next);
        let hex = sha256_hex(state_fingerprint.as_bytes());
        self.digests.insert(format!("{doc_key}#{next}"), hex.clone());
        self.receipts.insert(
            operation_id,
            FenceReceipt {
                operation_id,
                doc_key: doc_key.to_owned(),
                kind,
                base_revision,
                snapshot_digest: hex,
                outcome: FenceOutcome::Applied { new_revision: next },
                recorded_ms: now_ms,
            },
        );
        next
    }

    pub fn record_outcome_unknown(
        &mut self,
        doc_key: &str,
        kind: OfficeKind,
        operation_id: Uuid,
        reason: impl Into<String>,
        now_ms: u64,
    ) -> FenceReceipt {
        let live_rev = self.current_revision(doc_key);
        let hex = self
            .digests
            .get(&format!("{doc_key}#{live_rev}"))
            .cloned()
            .unwrap_or_else(|| sha256_hex(doc_key.as_bytes()));
        let rec = FenceReceipt {
            operation_id,
            doc_key: doc_key.to_owned(),
            kind,
            base_revision: live_rev,
            snapshot_digest: hex,
            outcome: FenceOutcome::OutcomeUnknown {
                reason: reason.into(),
                last_known_revision: live_rev,
            },
            recorded_ms: now_ms,
        };
        self.receipts.insert(operation_id, rec.clone());
        rec
    }

    pub fn resolve_fenced_conflict(&mut self, operation_id: Uuid) -> Option<LiveFencedConflict> {
        let pos = self
            .conflicts
            .iter()
            .position(|c| c.operation_id == operation_id)?;
        Some(self.conflicts.remove(pos))
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let d = digest(&SHA256, bytes);
    let mut out = String::with_capacity(64);
    for b in d.as_ref() {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

#[cfg(test)]
mod fence_tests {
    use super::*;

    #[test]
    fn revision_fence_deduplicates_id_catches_stale_revision_and_records_outcome_unknown() {
        let mut ledger = RevisionFenceLedger::default();
        assert_eq!(ledger.current_revision("budget.xlsx"), 1);

        let op1 = Uuid::new_v4();
        match ledger.check_admission(
            "budget.xlsx",
            OfficeKind::Excel,
            Some(op1),
            Some(1),
            "Sheet1!B3".into(),
            "120000".into(),
            "148000".into(),
            None,
            1_000,
        ) {
            FenceAdmission::Proceed { base_revision, .. } => {
                let r = ledger.commit_applied(
                    "budget.xlsx",
                    OfficeKind::Excel,
                    op1,
                    base_revision,
                    "B3=148000",
                    1_010,
                );
                assert_eq!(r, 2);
            }
            other => panic!("unexpected {other:?}"),
        }

        // Retrying same op1 returns Duplicate receipt at rev #2 without re-applying!
        assert!(matches!(
            ledger.check_admission(
                "budget.xlsx",
                OfficeKind::Excel,
                Some(op1),
                Some(1),
                "Sheet1!B3".into(),
                "148000".into(),
                "148000".into(),
                None,
                1_020,
            ),
            FenceAdmission::Duplicate(_)
        ));

        // Human edits tab -> rev becomes #3; stale AI edit against rev #2 is fenced!
        assert_eq!(ledger.bump_human_edit("budget.xlsx", "B3=155000"), 3);
        let op2 = Uuid::new_v4();
        assert!(matches!(
            ledger.check_admission(
                "budget.xlsx",
                OfficeKind::Excel,
                Some(op2),
                Some(2),
                "Sheet1!B3".into(),
                "155000".into(),
                "160000".into(),
                Some(FenceMutationPayload::ExcelCell {
                    sheet_idx: 0,
                    cell_a1: "B3".into(),
                    value: "160000".into(),
                }),
                2_000,
            ),
            FenceAdmission::BlockedConflict(_)
        ));
        assert_eq!(ledger.conflicts.len(), 1);

        // Honest uncertainty recorded cleanly
        let op3 = Uuid::new_v4();
        let unk = ledger.record_outcome_unknown(
            "budget.xlsx",
            OfficeKind::Excel,
            op3,
            "等待桌面响应超时",
            3_000,
        );
        assert!(matches!(
            unk.outcome,
            FenceOutcome::OutcomeUnknown {
                last_known_revision: 3,
                ..
            }
        ));
    }
}
