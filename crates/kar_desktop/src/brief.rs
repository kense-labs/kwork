//! Pure-Rust versioned Interactive Briefs (`KarBrief`) — zero HTML, zero WebView, zero JS.
//!
//! Reimagines ToWork's `brief` artifact system natively on GPUI + KUI (`AnimatedCounter`,
//! `BarChart`, `DonutChart`, `Sparkline`, `SankeyFlow`, `DataFlow`, and revision-pinned `ReviewForm`s
//! whose answers enter the normal durable conversation queue without authorizing side effects).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefTone {
    #[default]
    Info,
    Success,
    Warning,
    Danger,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BriefMetric {
    pub label: String,
    pub value: f64,
    #[serde(default)]
    pub decimals: usize,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub delta: Option<String>,
    #[serde(default = "default_true")]
    pub positive: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BriefSample {
    pub label: String,
    pub value: f32,
    #[serde(default)]
    pub comparison: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BriefFlowNode {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub column: Option<usize>,
    #[serde(default)]
    pub tone: BriefTone,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BriefFlowLink {
    pub source: String,
    pub target: String,
    pub value: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BriefPipeNode {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub meta: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default = "default_one")]
    pub share: f32,
}

fn default_one() -> f32 {
    1.0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefStepState {
    Done,
    Active,
    #[default]
    Pending,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefStep {
    pub title: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub state: BriefStepState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefFieldKind {
    #[default]
    SingleChoice,
    MultiChoice,
    Boolean,
    Note,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefFormField {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub kind: BriefFieldKind,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub default_value: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BriefRadarSeries {
    pub id: String,
    pub label: String,
    pub values: Vec<f32>,
    #[serde(default)]
    pub tone: BriefTone,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BriefFunnelStage {
    pub id: String,
    pub label: String,
    pub values: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BriefBlock {
    Metrics {
        items: Vec<BriefMetric>,
    },
    BarChart {
        title: String,
        samples: Vec<BriefSample>,
    },
    LineChart {
        title: String,
        samples: Vec<BriefSample>,
    },
    DonutChart {
        title: String,
        samples: Vec<BriefSample>,
    },
    Sparkline {
        title: String,
        samples: Vec<BriefSample>,
    },
    RadarChart {
        title: String,
        axes: Vec<String>,
        series: Vec<BriefRadarSeries>,
    },
    FunnelChart {
        title: String,
        segments: Vec<String>,
        stages: Vec<BriefFunnelStage>,
    },
    Gauge {
        label: String,
        value: f32,
        #[serde(default)]
        detail: String,
    },
    JsonTree {
        title: String,
        data: Value,
    },
    Sankey {
        title: String,
        nodes: Vec<BriefFlowNode>,
        links: Vec<BriefFlowLink>,
    },
    Pipeline {
        sources: Vec<BriefPipeNode>,
        core: BriefPipeNode,
        destinations: Vec<BriefPipeNode>,
        #[serde(default)]
        processed: u64,
        #[serde(default)]
        message: String,
    },
    Callout {
        #[serde(default)]
        tone: BriefTone,
        title: String,
        body: String,
    },
    Steps {
        items: Vec<BriefStep>,
    },
    ReviewForm {
        #[serde(default = "default_submit_label")]
        submit_label: String,
        fields: Vec<BriefFormField>,
    },
}

fn default_submit_label() -> String {
    "提交评审反馈".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KarBriefRevision {
    pub revision: u64,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub badge: Option<String>,
    #[serde(default)]
    pub turn_id: Option<Uuid>,
    pub updated_ms: u64,
    pub blocks: Vec<BriefBlock>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KarBrief {
    pub id: String,
    pub chat_id: Option<Uuid>,
    pub latest_revision: u64,
    /// Complete immutable revision history (`1..=latest_revision`).
    pub history: Vec<KarBriefRevision>,
    /// Revision numbers for which the user has already submitted form feedback.
    #[serde(default)]
    pub submitted_revisions: BTreeSet<u64>,
}

impl KarBrief {
    pub fn current(&self) -> &KarBriefRevision {
        self.history
            .last()
            .expect("KarBrief always retains at least revision 1")
    }

    pub fn at_revision(&self, rev: Option<u64>) -> Option<&KarBriefRevision> {
        match rev {
            None => self.history.last(),
            Some(r) => self.history.iter().find(|h| h.revision == r),
        }
    }
}

/// Interactive form draft state pinned to the revision the user started editing.
/// Publication of a newer revision cannot silently overwrite an active user draft!
#[derive(Clone, Debug, Default)]
pub struct BriefFormDraft {
    pub pinned_revision: Option<u64>,
    pub single: BTreeMap<String, String>,
    pub multi: BTreeMap<String, BTreeSet<String>>,
    pub boolean: BTreeMap<String, bool>,
    pub notes: BTreeMap<String, String>,
}

impl BriefFormDraft {
    pub fn touch_revision(&mut self, current_rev: u64) {
        if self.pinned_revision.is_none() {
            self.pinned_revision = Some(current_rev);
        }
    }

    pub fn has_stale_pin(&self, latest_rev: u64) -> Option<u64> {
        self.pinned_revision.filter(|&r| r < latest_rev)
    }

    pub fn upgrade_to_latest(&mut self, latest_rev: u64) {
        self.pinned_revision = Some(latest_rev);
    }

    pub fn format_reply_message(&self, brief: &KarBrief) -> String {
        let rev_num = self.pinned_revision.unwrap_or(brief.latest_revision);
        let snap = brief.at_revision(Some(rev_num)).unwrap_or_else(|| brief.current());
        let mut lines = vec![format!(
            "[交互简报反馈 · {} (id={}, v{})]",
            snap.title, brief.id, rev_num
        )];
        for block in &snap.blocks {
            if let BriefBlock::ReviewForm { fields, .. } = block {
                for field in fields {
                    let answer = match field.kind {
                        BriefFieldKind::SingleChoice => self
                            .single
                            .get(&field.id)
                            .cloned()
                            .or_else(|| field.default_value.clone())
                            .or_else(|| field.options.first().cloned())
                            .unwrap_or_else(|| "未选择".into()),
                        BriefFieldKind::MultiChoice => {
                            let set = self.multi.get(&field.id);
                            let picked: Vec<String> = set
                                .map(|s| s.iter().cloned().collect())
                                .unwrap_or_default();
                            if picked.is_empty() {
                                "未选择".into()
                            } else {
                                picked.join("、")
                            }
                        }
                        BriefFieldKind::Boolean => {
                            let val = self.boolean.get(&field.id).copied().unwrap_or(false);
                            if val { "是".into() } else { "否".into() }
                        }
                        BriefFieldKind::Note => self
                            .notes
                            .get(&field.id)
                            .map(|s| s.trim().to_owned())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "（无补充说明）".into()),
                    };
                    lines.push(format!("- {}：{}", field.label, answer));
                }
            }
        }
        lines.join("\n")
    }
}

/// Session-scoped Brief inventory with optimistic `expected_revision` checks.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BriefInventory {
    pub briefs: Vec<KarBrief>,
}

impl BriefInventory {
    pub fn load(store_dir: &Path, chat_key: &str) -> Self {
        let path = store_dir.join("briefs").join(format!("{chat_key}.json"));
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, store_dir: &Path, chat_key: &str) -> Result<()> {
        let dir = store_dir.join("briefs");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{chat_key}.json"));
        let tmp = dir.join(format!("{chat_key}.json.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn write_brief(
        &mut self,
        id: Option<String>,
        expected_revision: Option<u64>,
        chat_id: Option<Uuid>,
        turn_id: Option<Uuid>,
        title: String,
        summary: String,
        badge: Option<String>,
        blocks: Vec<BriefBlock>,
        now_ms: u64,
    ) -> Result<&KarBrief> {
        if let Some(ref existing_id) = id
            && let Some(pos) = self.briefs.iter().position(|b| &b.id == existing_id)
        {
            let current_rev = self.briefs[pos].latest_revision;
            let Some(exp) = expected_revision else {
                bail!(
                    "修订既有简报 `{existing_id}` 必须提供 expected_revision（当前最新版本为 v{current_rev}）"
                );
            };
            if exp != current_rev {
                bail!(
                    "简报 `{existing_id}` 版本冲突：expected_revision=v{exp}，当前最新为 v{current_rev}，请先调用 kar_read_brief 读取最新版本"
                );
            }
            let next_rev = current_rev + 1;
            self.briefs[pos].latest_revision = next_rev;
            self.briefs[pos].history.push(KarBriefRevision {
                revision: next_rev,
                title,
                summary,
                badge,
                turn_id,
                updated_ms: now_ms,
                blocks,
            });
            return Ok(&self.briefs[pos]);
        }

        if expected_revision.is_some_and(|r| r != 0) {
            bail!("新建简报时不应传入非零 expected_revision");
        }

        let new_id = id
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("brief-{}", &Uuid::new_v4().to_string()[..8]));
        let brief = KarBrief {
            id: new_id,
            chat_id,
            latest_revision: 1,
            history: vec![KarBriefRevision {
                revision: 1,
                title,
                summary,
                badge,
                turn_id,
                updated_ms: now_ms,
                blocks,
            }],
            submitted_revisions: BTreeSet::new(),
        };
        self.briefs.push(brief);
        Ok(self.briefs.last().expect("just pushed"))
    }

    pub fn get(&self, id: &str) -> Option<&KarBrief> {
        self.briefs.iter().find(|b| b.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut KarBrief> {
        self.briefs.iter_mut().find(|b| b.id == id)
    }
}

/// Parsed MCP `kar_write_brief` arguments: `(id, expected_rev, title, summary, badge, blocks)`.
pub type ParsedWriteBriefArgs = (
    Option<String>,
    Option<u64>,
    String,
    String,
    Option<String>,
    Vec<BriefBlock>,
);

/// Parse MCP `kar_write_brief` arguments into validated `(id, expected_rev, title, summary, badge, blocks)`.
pub fn parse_mcp_write_brief(args: &Map<String, Value>) -> Result<ParsedWriteBriefArgs> {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let expected_revision = args.get("expected_revision").and_then(Value::as_u64);
    let title = args
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("缺少必填字段 `title`"))?
        .to_owned();
    let summary = args
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    let badge = args
        .get("badge")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let blocks_val = args
        .get("blocks")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let blocks: Vec<BriefBlock> = serde_json::from_value(blocks_val)
        .map_err(|e| anyhow::anyhow!("解析 `blocks` 结构失败：{e}"))?;
    if blocks.is_empty() {
        bail!("`blocks` 至少需要包含一个 KUI 组件块");
    }
    Ok((id, expected_revision, title, summary, badge, blocks))
}

/// Built-in showcase Brief demonstrating pure-GPUI KUI interactive charts, flows, counters, and review forms.
pub fn build_showcase_kui_brief(turn_id: Option<Uuid>) -> KarBrief {
    let v1_blocks = vec![
        BriefBlock::Metrics {
            items: vec![
                BriefMetric {
                    label: "季度签约总额".into(),
                    value: 448.0,
                    decimals: 1,
                    prefix: "¥".into(),
                    suffix: "万".into(),
                    delta: Some("+19.2% YoY".into()),
                    positive: true,
                },
                BriefMetric {
                    label: "自动化周报节省工时".into(),
                    value: 118.0,
                    decimals: 0,
                    prefix: String::new(),
                    suffix: "h".into(),
                    delta: Some("+26h 环比".into()),
                    positive: true,
                },
            ],
        },
        BriefBlock::RadarChart {
            title: "多维度架构成熟度评估（v1 基线 vs v2 目标）".into(),
            axes: vec![
                "协同安全性".into(),
                "原生帧率".into(),
                "Git 集成".into(),
                "远程互联".into(),
                "Office 精度".into(),
            ],
            series: vec![
                BriefRadarSeries {
                    id: "v1".into(),
                    label: "v1 基线".into(),
                    values: vec![72.0, 90.0, 58.0, 76.0, 88.0],
                    tone: BriefTone::Warning,
                },
                BriefRadarSeries {
                    id: "v2".into(),
                    label: "v2 升级后".into(),
                    values: vec![98.0, 96.0, 94.0, 95.0, 97.0],
                    tone: BriefTone::Info,
                },
            ],
        },
        BriefBlock::Gauge {
            label: "自动化回归覆盖率".into(),
            value: 94.5,
            detail: "单元测试 + 真实 Axum / Git / Office 端到端验收".into(),
        },
        BriefBlock::Callout {
            tone: BriefTone::Info,
            title: "初稿说明（v1）".into(),
            body: "初稿包含雷达能力评估与覆盖率仪表盘；v2 补充了产品线对比柱状图、算力预算桑基流向图与版本锁定评审表单。".into(),
        },
    ];
    KarBrief {
        id: "brief-q4-review".into(),
        chat_id: None,
        latest_revision: 2,
        submitted_revisions: BTreeSet::new(),
        history: vec![
            KarBriefRevision {
                revision: 1,
                title: "Q4 经营复盘初稿".into(),
                summary: "早期汇总快照（点击右上角 v1 / v2 可无损切换历史修订版本）".into(),
                badge: Some("历史快照".into()),
                turn_id,
                updated_ms: 1_763_960_000_000,
                blocks: v1_blocks,
            },
            KarBriefRevision {
                revision: 2,
                title: "Q4 经营复盘与协同方案评审简报".into(),
                summary: "纯 GPUI + KUI 原生渲染：支持滚轮数字、交互柱状对比、桑基流向图与版本锁定的评审表单。".into(),
                badge: Some("KUI 原生简报".into()),
                turn_id,
                updated_ms: 1_763_963_600_000,
                blocks: vec![
                    BriefBlock::Metrics {
                        items: vec![
                            BriefMetric {
                                label: "季度签约总额".into(),
                                value: 486.5,
                                decimals: 1,
                                prefix: "¥".into(),
                                suffix: "万".into(),
                                delta: Some("+24.8% YoY".into()),
                                positive: true,
                            },
                            BriefMetric {
                                label: "自动化周报节省工时".into(),
                                value: 142.0,
                                decimals: 0,
                                prefix: String::new(),
                                suffix: "h".into(),
                                delta: Some("+38h 环比".into()),
                                positive: true,
                            },
                            BriefMetric {
                                label: "文档协同零冲突率".into(),
                                value: 98.4,
                                decimals: 1,
                                prefix: String::new(),
                                suffix: "%".into(),
                                delta: Some("Revision 围栏开启".into()),
                                positive: true,
                            },
                        ],
                    },
                    BriefBlock::BarChart {
                        title: "各产品线 Q4 营收对比（实绩 vs 预算目标 · 万）".into(),
                        samples: vec![
                            BriefSample {
                                label: "智能工作台".into(),
                                value: 186.0,
                                comparison: Some(150.0),
                            },
                            BriefSample {
                                label: "原生文档套件".into(),
                                value: 142.0,
                                comparison: Some(125.0),
                            },
                            BriefSample {
                                label: "私有节点部署".into(),
                                value: 98.5,
                                comparison: Some(90.0),
                            },
                            BriefSample {
                                label: "自动化连接器".into(),
                                value: 60.0,
                                comparison: Some(52.0),
                            },
                        ],
                    },
                    BriefBlock::Sankey {
                        title: "研发与算力预算流向（万）".into(),
                        nodes: vec![
                            BriefFlowNode {
                                id: "total".into(),
                                label: "Q4 总投入".into(),
                                column: Some(0),
                                tone: BriefTone::Info,
                            },
                            BriefFlowNode {
                                id: "gpui".into(),
                                label: "GPUI + KUI".into(),
                                column: Some(1),
                                tone: BriefTone::Success,
                            },
                            BriefFlowNode {
                                id: "kar".into(),
                                label: "Kar 运行时".into(),
                                column: Some(1),
                                tone: BriefTone::Info,
                            },
                            BriefFlowNode {
                                id: "office".into(),
                                label: "Office 引擎".into(),
                                column: Some(2),
                                tone: BriefTone::Success,
                            },
                            BriefFlowNode {
                                id: "link".into(),
                                label: "远程与排程".into(),
                                column: Some(2),
                                tone: BriefTone::Warning,
                            },
                        ],
                        links: vec![
                            BriefFlowLink {
                                source: "total".into(),
                                target: "gpui".into(),
                                value: 58.0,
                            },
                            BriefFlowLink {
                                source: "total".into(),
                                target: "kar".into(),
                                value: 62.0,
                            },
                            BriefFlowLink {
                                source: "gpui".into(),
                                target: "office".into(),
                                value: 42.0,
                            },
                            BriefFlowLink {
                                source: "gpui".into(),
                                target: "link".into(),
                                value: 16.0,
                            },
                            BriefFlowLink {
                                source: "kar".into(),
                                target: "office".into(),
                                value: 28.0,
                            },
                            BriefFlowLink {
                                source: "kar".into(),
                                target: "link".into(),
                                value: 34.0,
                            },
                        ],
                    },
                    BriefBlock::Callout {
                        tone: BriefTone::Info,
                        title: "安全边界说明".into(),
                        body: "下方评审表单绑定当前简报版本 v2；点击提交只会作为结构化消息进入对话队列，不会绕过工具权限自动修改工作区。".into(),
                    },
                    BriefBlock::ReviewForm {
                        submit_label: "提交评审结论到对话".into(),
                        fields: vec![
                            BriefFormField {
                                id: "rollout".into(),
                                label: "发布节奏选择".into(),
                                kind: BriefFieldKind::SingleChoice,
                                options: vec![
                                    "优先合入 P2P 配对与双写围栏".into(),
                                    "全量同步发布（含定时自动化）".into(),
                                    "先在内部沙盒试运行一周".into(),
                                ],
                                default_value: Some("全量同步发布（含定时自动化）".into()),
                            },
                            BriefFormField {
                                id: "targets".into(),
                                label: "需同步更新的右侧文档".into(),
                                kind: BriefFieldKind::MultiChoice,
                                options: vec![
                                    "会议纪要 (.docx)".into(),
                                    "预算公式表 (.xlsx)".into(),
                                    "发布会演示 (.pptx)".into(),
                                ],
                                default_value: None,
                            },
                            BriefFormField {
                                id: "note".into(),
                                label: "补充批注".into(),
                                kind: BriefFieldKind::Note,
                                options: Vec::new(),
                                default_value: None,
                            },
                        ],
                    },
                ],
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brief_inventory_enforces_expected_revision_and_preserves_history() {
        let mut inv = BriefInventory::default();
        let b1 = inv
            .write_brief(
                Some("plan-1".into()),
                None,
                None,
                None,
                "方案 v1".into(),
                "初稿".into(),
                None,
                vec![BriefBlock::Callout {
                    tone: BriefTone::Info,
                    title: "标题".into(),
                    body: "内容".into(),
                }],
                1000,
            )
            .unwrap()
            .clone();
        assert_eq!(b1.latest_revision, 1);

        // Missing or wrong expected_revision must fail
        assert!(
            inv.write_brief(
                Some("plan-1".into()),
                None,
                None,
                None,
                "方案 v2".into(),
                "".into(),
                None,
                b1.current().blocks.clone(),
                2000,
            )
            .is_err()
        );
        assert!(
            inv.write_brief(
                Some("plan-1".into()),
                Some(99),
                None,
                None,
                "方案 v2".into(),
                "".into(),
                None,
                b1.current().blocks.clone(),
                2000,
            )
            .is_err()
        );

        // Matching expected_revision advances to v2 and preserves v1
        let b2 = inv
            .write_brief(
                Some("plan-1".into()),
                Some(1),
                None,
                None,
                "方案 v2".into(),
                "修订稿".into(),
                None,
                b1.current().blocks.clone(),
                2000,
            )
            .unwrap();
        assert_eq!(b2.latest_revision, 2);
        assert_eq!(b2.at_revision(Some(1)).unwrap().title, "方案 v1");
        assert_eq!(b2.at_revision(Some(2)).unwrap().title, "方案 v2");
    }

    #[test]
    fn brief_draft_pins_revision_on_edit() {
        let brief = build_showcase_kui_brief(None);
        let mut draft = BriefFormDraft::default();
        draft.touch_revision(brief.latest_revision);
        draft
            .single
            .insert("rollout".into(), "优先合入 P2P 配对与双写围栏".into());
        assert_eq!(draft.has_stale_pin(2), None);
        assert_eq!(draft.has_stale_pin(3), Some(2));
        let msg = draft.format_reply_message(&brief);
        assert!(msg.contains("v2"));
        assert!(msg.contains("优先合入 P2P 配对与双写围栏"));
    }
}
