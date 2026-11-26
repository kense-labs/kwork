//! UI-independent Client projection, immutable Sent-Turn Captures, Session Associations,
//! and bounded memory budgets (`4 MiB` stream head/tail + `64 MiB` protected LRU detail cache).
//!
//! Borrowed from ToWork's `client` and `node-runtime` architecture, adapted in pure Rust for Kar.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use kar::ApprovalMode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::SelectionTarget;

/// Default per-command stream output cap: 4 MiB (1 MiB head + 3 MiB rolling tail).
pub const STREAM_HEAD_BYTES: usize = 1024 * 1024;
pub const STREAM_TAIL_BYTES: usize = 3 * 1024 * 1024;
pub const STREAM_MAX_BYTES: usize = STREAM_HEAD_BYTES + STREAM_TAIL_BYTES;
/// Shared Node-wide command stream retention budget: 256 MiB.
pub const TOTAL_STREAM_BUDGET_BYTES: usize = 256 * 1024 * 1024;
/// Client-side expanded detail LRU budget: 64 MiB.
pub const DETAIL_CACHE_BUDGET_BYTES: usize = 64 * 1024 * 1024;
/// Foreground command runtime threshold after which pending guidance promotes it to background.
pub const AUTO_BACKGROUND_THRESHOLD_MS: u64 = 30_000;

/// Immutable association linking a chat session to an external business object
/// (e.g. a Scheduled Task, an Office Document, or an Automation Connector).
/// Associations carry metadata only and never grant extra file or shell authority.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionAssociation {
    pub namespace: String,
    pub kind: String,
    pub id: String,
    pub label: String,
}

impl SessionAssociation {
    pub fn schedule(task_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            namespace: "schedules".into(),
            kind: "task".into(),
            id: task_id.into(),
            label: label.into(),
        }
    }

    pub fn document(doc_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            namespace: "office".into(),
            kind: "document".into(),
            id: doc_id.into(),
            label: label.into(),
        }
    }

    pub fn matches_filter(&self, namespace: &str, kind: &str, id: Option<&str>) -> bool {
        self.namespace == namespace
            && self.kind == kind
            && id.is_none_or(|expected| self.id == expected)
    }

    pub fn short_badge(&self) -> String {
        match (self.namespace.as_str(), self.kind.as_str()) {
            ("schedules", _) => format!("定时 · {}", self.label),
            ("office", _) => format!("文档 · {}", self.label),
            _ => format!("{} · {}", self.namespace, self.label),
        }
    }
}

/// Immutable configuration snapshot frozen at the moment a turn (`request_id`) is admitted.
/// Subsequent changes to global defaults never retroactively alter an admitted turn's capture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TurnCapture {
    pub turn_id: Uuid,
    pub chat_id: Option<Uuid>,
    pub runtime_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    pub approval_pretty: String,
    pub workspace: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_selection: Option<SelectionTarget>,
    /// This input was dispatched by the explicit plan approval action.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub approved_plan: bool,
    /// Open Office document revisions captured when the turn was admitted (`doc_name -> revision`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub document_revisions: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub association: Option<SessionAssociation>,
    pub captured_at_ms: u64,
}

impl TurnCapture {
    pub fn approval_label(mode: ApprovalMode) -> &'static str {
        #[allow(unreachable_patterns)]
        match mode {
            ApprovalMode::Host => "需确认",
            ApprovalMode::Deny => "自动拒绝",
            _ => "自动放行",
        }
    }

    pub fn summary_pill(&self) -> String {
        let mut parts = vec![crate::types::runtime_display_name(&self.runtime_id)];
        if let Some(ref m) = self.model {
            parts.push(m.clone());
        }
        if let Some(ref e) = self.effort {
            parts.push(match e.as_str() {
                "low" => "轻快".into(),
                "medium" => "均衡".into(),
                "high" => "深度".into(),
                other => other.to_owned(),
            });
        }
        parts.join(" · ")
    }
}

/// Persisted association directory with monotonic revision checks (`associations.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AssociationRegistry {
    pub revision: u64,
    pub by_chat: BTreeMap<Uuid, SessionAssociation>,
}

impl AssociationRegistry {
    pub fn load(store_dir: &Path) -> Self {
        let path = store_dir.join("associations.json");
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, store_dir: &Path) -> Result<()> {
        let _ = std::fs::create_dir_all(store_dir);
        let path = store_dir.join("associations.json");
        let tmp = store_dir.join("associations.json.tmp");
        let json = serde_json::to_vec_pretty(self)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// Bind an immutable association to `chat_id`. Re-binding an existing chat to a different
    /// association is rejected to preserve immutable provenance.
    pub fn bind(
        &mut self,
        chat_id: Uuid,
        assoc: SessionAssociation,
        expected_revision: Option<u64>,
    ) -> Result<u64> {
        if let Some(exp) = expected_revision
            && exp != self.revision
        {
            bail!(
                "关联目录版本冲突：期望 revision #{exp}，当前为 #{}",
                self.revision
            );
        }
        if let Some(existing) = self.by_chat.get(&chat_id) {
            if existing == &assoc {
                return Ok(self.revision);
            }
            bail!("会话 {chat_id} 已绑定不可变关联「{}」，不可篡改", existing.short_badge());
        }
        self.by_chat.insert(chat_id, assoc);
        self.revision = self.revision.saturating_add(1);
        Ok(self.revision)
    }

    pub fn get(&self, chat_id: &Uuid) -> Option<&SessionAssociation> {
        self.by_chat.get(chat_id)
    }

    /// List chat IDs matching `(namespace, kind, optional id)` in deterministic order.
    pub fn list_associated_chats(
        &self,
        namespace: &str,
        kind: &str,
        id: Option<&str>,
    ) -> Vec<Uuid> {
        self.by_chat
            .iter()
            .filter_map(|(cid, a)| a.matches_filter(namespace, kind, id).then_some(*cid))
            .collect()
    }
}

/// Persisted store of immutable `TurnCapture`s (`turn_captures.json`).
pub fn load_turn_captures(store_dir: &Path) -> BTreeMap<Uuid, TurnCapture> {
    let path = store_dir.join("turn_captures.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_turn_capture(store_dir: &Path, capture: TurnCapture) {
    let mut all = load_turn_captures(store_dir);
    // Freeze first capture: never overwrite an already admitted turn's capture.
    all.entry(capture.turn_id).or_insert(capture);
    let _ = std::fs::create_dir_all(store_dir);
    if let Ok(json) = serde_json::to_vec_pretty(&all) {
        let tmp = store_dir.join("turn_captures.json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(tmp, store_dir.join("turn_captures.json"));
        }
    }
}

/// Single command output stream bounded to `head_cap` + rolling `tail_cap` bytes.
#[derive(Clone, Debug)]
pub struct BoundedStreamBuffer {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    head_cap: usize,
    tail_cap: usize,
    pub total_written: usize,
    pub completed: bool,
    pub summary_preview: String,
    last_read_tick: u64,
}

impl BoundedStreamBuffer {
    pub fn new(head_cap: usize, tail_cap: usize, tick: u64) -> Self {
        Self {
            head: Vec::new(),
            tail: VecDeque::new(),
            head_cap: head_cap.max(64),
            tail_cap: tail_cap.max(64),
            total_written: 0,
            completed: false,
            summary_preview: String::new(),
            last_read_tick: tick,
        }
    }

    pub fn push_bytes(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        self.total_written = self.total_written.saturating_add(chunk.len());
        let head_room = self.head_cap.saturating_sub(self.head.len());
        let to_head = head_room.min(chunk.len());
        if to_head > 0 {
            self.head.extend_from_slice(&chunk[..to_head]);
        }
        let rest = &chunk[to_head..];
        if !rest.is_empty() {
            if rest.len() >= self.tail_cap {
                self.tail.clear();
                self.tail
                    .extend(rest[rest.len() - self.tail_cap..].iter().copied());
            } else {
                let overflow = (self.tail.len() + rest.len()).saturating_sub(self.tail_cap);
                for _ in 0..overflow {
                    self.tail.pop_front();
                }
                self.tail.extend(rest.iter().copied());
            }
        }
        if self.summary_preview.is_empty() {
            let text = String::from_utf8_lossy(&self.head);
            if let Some(line) = text.lines().find(|l| !l.trim().is_empty()) {
                self.summary_preview = line.chars().take(160).collect();
            }
        }
    }

    pub fn retained_bytes(&self) -> usize {
        self.head.len() + self.tail.len()
    }

    pub fn omitted_bytes(&self) -> usize {
        self.total_written.saturating_sub(self.retained_bytes())
    }

    pub fn render_text(&mut self, tick: u64) -> String {
        self.last_read_tick = tick;
        if self.head.is_empty() && self.tail.is_empty() {
            return self.summary_preview.clone();
        }
        let head_str = String::from_utf8_lossy(&self.head);
        if self.tail.is_empty() {
            return head_str.into_owned();
        }
        let tail_vec: Vec<u8> = self.tail.iter().copied().collect();
        let tail_str = String::from_utf8_lossy(&tail_vec);
        let omitted = self.omitted_bytes();
        if omitted > 0 {
            format!(
                "{head_str}\n\n… [已按 4 MiB 内存预算截断中间 {omitted} 字节 · 保留首段与最新滚动尾部] …\n\n{tail_str}"
            )
        } else {
            format!("{head_str}{tail_str}")
        }
    }

    fn evict_payload(&mut self) -> usize {
        let freed = self.retained_bytes();
        self.head.clear();
        self.head.shrink_to_fit();
        self.tail.clear();
        self.tail.shrink_to_fit();
        freed
    }
}

/// Shared Node-wide command stream manager enforcing per-stream (`4 MiB`) and total (`256 MiB`) budgets.
#[derive(Clone, Debug)]
pub struct StreamOutputBudget {
    head_cap: usize,
    tail_cap: usize,
    total_cap: usize,
    used_bytes: usize,
    tick: u64,
    streams: BTreeMap<String, BoundedStreamBuffer>,
}

impl Default for StreamOutputBudget {
    fn default() -> Self {
        Self::with_limits(
            STREAM_HEAD_BYTES,
            STREAM_TAIL_BYTES,
            TOTAL_STREAM_BUDGET_BYTES,
        )
    }
}

impl StreamOutputBudget {
    pub fn with_limits(head_cap: usize, tail_cap: usize, total_cap: usize) -> Self {
        Self {
            head_cap,
            tail_cap,
            total_cap: total_cap.max(head_cap + tail_cap),
            used_bytes: 0,
            tick: 0,
            streams: BTreeMap::new(),
        }
    }

    pub fn append(&mut self, stream_id: impl Into<String>, chunk: &[u8], completed: bool) {
        self.tick = self.tick.saturating_add(1);
        let id = stream_id.into();
        let tick = self.tick;
        let head_cap = self.head_cap;
        let tail_cap = self.tail_cap;
        let prev = self
            .streams
            .get(&id)
            .map_or(0, BoundedStreamBuffer::retained_bytes);
        let entry = self
            .streams
            .entry(id.clone())
            .or_insert_with(|| BoundedStreamBuffer::new(head_cap, tail_cap, tick));
        entry.push_bytes(chunk);
        if completed {
            entry.completed = true;
        }
        let next = entry.retained_bytes();
        self.used_bytes = self.used_bytes.saturating_sub(prev).saturating_add(next);
        self.evict_completed_if_needed(&id);
    }

    /// A feed projection contains complete tool outputs, including previously rendered tools.
    pub(crate) fn sync_tool_snapshots(
        &mut self,
        chat: Uuid,
        previous: &[kar::chat::ChatTurn],
        turns: &[kar::chat::ChatTurn],
    ) {
        use kar::chat::Item;
        for (turn_index, turn) in turns.iter().enumerate() {
            let old = previous.get(turn_index).filter(|old| old.id == turn.id);
            for (index, row) in turn.items.iter().enumerate() {
                let Item::Tool { id, activity } = &row.item else {
                    continue;
                };
                let parts = activity
                    .result
                    .as_ref()
                    .and_then(|result| result.content.as_ref());
                let old_tool =
                    old.and_then(|turn| turn.items.get(index))
                        .and_then(|row| match &row.item {
                            Item::Tool {
                                id: old_id,
                                activity,
                            } if old_id == id => Some(activity),
                            _ => None,
                        });
                let old_parts = old_tool.and_then(|tool| tool.result.as_ref()?.content.as_ref());
                if old_parts == parts
                    && (parts.is_none()
                        || old_tool.is_some_and(|tool| tool.finished == activity.finished))
                {
                    continue;
                }
                let key = format!("{chat}/{}/{index}", turn.id);
                if let Some(old) = self.streams.remove(&key) {
                    self.used_bytes -= old.retained_bytes();
                }
                if let Some(parts) = parts {
                    for part in parts {
                        if let kar::OutputPart::Text { text } = part {
                            self.append(&key, text.as_bytes(), activity.finished);
                        }
                    }
                }
            }
        }
    }

    pub fn mark_completed(&mut self, stream_id: &str) {
        if let Some(buf) = self.streams.get_mut(stream_id) {
            buf.completed = true;
        }
        self.evict_completed_if_needed("");
    }

    pub fn read(&mut self, stream_id: &str) -> Option<String> {
        self.tick = self.tick.saturating_add(1);
        let tick = self.tick;
        self.streams.get_mut(stream_id).map(|b| b.render_text(tick))
    }

    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    fn evict_completed_if_needed(&mut self, keep_id: &str) {
        while self.used_bytes > self.total_cap {
            let victim = self
                .streams
                .iter()
                .filter(|(k, v)| v.completed && v.retained_bytes() > 0 && k.as_str() != keep_id)
                .min_by_key(|(_, v)| v.last_read_tick)
                .map(|(k, _)| k.clone());
            let Some(victim_key) = victim else {
                break;
            };
            if let Some(v) = self.streams.get_mut(&victim_key) {
                let freed = v.evict_payload();
                self.used_bytes = self.used_bytes.saturating_sub(freed);
            }
        }
    }
}

/// Key identifying an expanded detail block in a conversation view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DetailKey {
    pub chat_id: Uuid,
    pub turn_id: Uuid,
    pub item_idx: usize,
}

#[derive(Clone, Debug)]
struct DetailEntry {
    pub content: String,
    pub bytes: usize,
    /// Pending approval payloads are protected and never evicted under memory contention.
    pub protected: bool,
    pub last_touch: u64,
}

/// `64 MiB` Client detail cache with explicit collapse release and pending-approval protection.
#[derive(Clone, Debug)]
pub struct DetailLruCache {
    budget_bytes: usize,
    used_bytes: usize,
    tick: u64,
    entries: BTreeMap<DetailKey, DetailEntry>,
}

impl Default for DetailLruCache {
    fn default() -> Self {
        Self::with_budget(DETAIL_CACHE_BUDGET_BYTES)
    }
}

impl DetailLruCache {
    pub fn with_budget(budget_bytes: usize) -> Self {
        Self {
            budget_bytes: budget_bytes.max(1),
            used_bytes: 0,
            tick: 0,
            entries: BTreeMap::new(),
        }
    }

    pub fn put(&mut self, key: DetailKey, content: String, protected: bool) -> bool {
        self.tick = self.tick.saturating_add(1);
        let bytes = content.len();
        if let Some(old) = self.entries.remove(&key) {
            self.used_bytes = self.used_bytes.saturating_sub(old.bytes);
        }
        // Protected entries (pending approvals) are always admitted; ordinary entries evict LRU unprotected items.
        if !protected && bytes > self.budget_bytes {
            return false;
        }
        while !protected && self.used_bytes + bytes > self.budget_bytes {
            let victim = self
                .entries
                .iter()
                .filter(|(_, e)| !e.protected)
                .min_by_key(|(_, e)| e.last_touch)
                .map(|(k, _)| *k);
            let Some(victim_key) = victim else {
                return false;
            };
            if let Some(removed) = self.entries.remove(&victim_key) {
                self.used_bytes = self.used_bytes.saturating_sub(removed.bytes);
            }
        }
        self.used_bytes = self.used_bytes.saturating_add(bytes);
        self.entries.insert(
            key,
            DetailEntry {
                content,
                bytes,
                protected,
                last_touch: self.tick,
            },
        );
        true
    }

    pub fn get(&mut self, key: &DetailKey) -> Option<&str> {
        self.tick = self.tick.saturating_add(1);
        let tick = self.tick;
        let entry = self.entries.get_mut(key)?;
        entry.last_touch = tick;
        Some(entry.content.as_str())
    }

    /// Release cached text for rows that were collapsed by the user, keeping protected entries intact.
    pub fn retain_expanded(&mut self, chat_id: Uuid, expanded: &BTreeSet<(Uuid, usize)>) {
        let mut to_remove = Vec::new();
        for (k, e) in &self.entries {
            if k.chat_id == chat_id && !e.protected && !expanded.contains(&(k.turn_id, k.item_idx))
            {
                to_remove.push(*k);
            }
        }
        for k in to_remove {
            if let Some(removed) = self.entries.remove(&k) {
                self.used_bytes = self.used_bytes.saturating_sub(removed.bytes);
            }
        }
    }

    /// Release unprotected details from hidden/inactive chats when switching conversations.
    pub fn release_inactive_chats(&mut self, active_chat: Option<Uuid>) {
        let mut to_remove = Vec::new();
        for (k, e) in &self.entries {
            if !e.protected && Some(k.chat_id) != active_chat {
                to_remove.push(*k);
            }
        }
        for k in to_remove {
            if let Some(removed) = self.entries.remove(&k) {
                self.used_bytes = self.used_bytes.saturating_sub(removed.bytes);
            }
        }
    }

    pub fn release_all_unlocked(&mut self) {
        self.release_inactive_chats(None);
    }

    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Foreground command promoted to background tracking (either automatically when running ≥ 30s
/// while user guidance is queued, or manually by the user).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundedCommand {
    pub tool_id: String,
    pub turn_id: Uuid,
    pub command_summary: String,
    pub started_ms: u64,
    pub promoted_ms: u64,
    pub auto_promoted_by_guidance: bool,
    pub follow_up_requested: bool,
    pub completed: bool,
    pub exit_code: Option<i32>,
    pub output_tail: String,
    pub follow_up_delivered: bool,
}

/// UI-independent Client state reducer managing sequence deduplication, immutable captures,
/// bounded memory caches, and backgrounded commands.
#[derive(Clone, Debug, Default)]
pub struct ClientReducer {
    pub store_dir: Option<PathBuf>,
    pub last_seq_by_chat: BTreeMap<Uuid, u64>,
    pub turn_captures: BTreeMap<Uuid, TurnCapture>,
    pub associations: AssociationRegistry,
    pub stream_budget: StreamOutputBudget,
    pub detail_cache: DetailLruCache,
    pub background_commands: BTreeMap<String, BackgroundedCommand>,
    pub synced_event_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SequenceDisposition {
    Accept,
    DuplicateOrOld,
    GapDetected { expected: u64, actual: u64 },
}

impl ClientReducer {
    pub fn new(store_dir: &Path) -> Self {
        Self {
            store_dir: Some(store_dir.to_path_buf()),
            last_seq_by_chat: BTreeMap::new(),
            turn_captures: load_turn_captures(store_dir),
            associations: AssociationRegistry::load(store_dir),
            stream_budget: StreamOutputBudget::default(),
            detail_cache: DetailLruCache::default(),
            background_commands: BTreeMap::new(),
            synced_event_count: 0,
        }
    }

    pub fn association_for(&self, chat_id: &Uuid) -> Option<&SessionAssociation> {
        self.associations.get(chat_id)
    }

    pub fn bind_association(&mut self, chat_id: Uuid, assoc: SessionAssociation) {
        if self.associations.bind(chat_id, assoc, None).is_ok()
            && let Some(ref dir) = self.store_dir
        {
            let _ = self.associations.save(dir);
        }
    }

    pub fn record_turn_capture(&mut self, capture: TurnCapture) {
        if let Some(ref dir) = self.store_dir {
            save_turn_capture(dir, capture.clone());
        }
        self.turn_captures.entry(capture.turn_id).or_insert(capture);
    }

    pub fn register_background_command(&mut self, bg: BackgroundedCommand) {
        self.background_commands.insert(bg.tool_id.clone(), bg);
    }

    pub fn sync_background_commands(&mut self, turns: &[kar::chat::ChatTurn]) {
        for turn in turns {
            for ti in &turn.items {
                if let kar::chat::Item::Tool { id, activity } = &ti.item
                    && activity.finished
                    && let Some(bg) = self.background_commands.get_mut(id)
                    && bg.turn_id == turn.id
                {
                    bg.completed = true;
                    bg.exit_code = activity
                        .result
                        .as_ref()
                        .and_then(|r| r.exit_code)
                        .map(|c| c as i32);
                }
            }
        }
    }
    pub fn observe_sequence(&mut self, chat_id: Uuid, seq: u64) -> SequenceDisposition {
        if let Some(&prev) = self.last_seq_by_chat.get(&chat_id) {
            if seq <= prev {
                return SequenceDisposition::DuplicateOrOld;
            }
            if seq > prev + 1 {
                return SequenceDisposition::GapDetected {
                    expected: prev + 1,
                    actual: seq,
                };
            }
        }
        self.last_seq_by_chat.insert(chat_id, seq);
        self.synced_event_count = self.synced_event_count.saturating_add(1);
        SequenceDisposition::Accept
    }

    pub fn reset_sequence(&mut self, chat_id: Uuid, snapshot_seq: Option<u64>) {
        if let Some(seq) = snapshot_seq {
            self.last_seq_by_chat.insert(chat_id, seq);
        } else {
            self.last_seq_by_chat.remove(&chat_id);
        }
    }
}

/// Record the local conversation selected by this desktop, independently of chat update times.
pub(crate) fn remember_chat(dir: &std::path::Path, chat: Uuid) -> std::io::Result<()> {
    write_desktop_preference(dir, "last-chat", &chat.to_string())
}

pub(crate) fn desktop_preference(dir: &Path, key: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(key)).ok()
}

pub(crate) fn write_desktop_preference(dir: &Path, key: &str, value: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{key}-{}.tmp", Uuid::new_v4()));
    let result = std::fs::write(&tmp, value).and_then(|_| std::fs::rename(&tmp, dir.join(key)));
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

pub(crate) fn last_chat(dir: &std::path::Path) -> Option<Uuid> {
    std::fs::read_to_string(dir.join("last-chat"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_budget_retains_head_and_rolling_tail_and_evicts_completed_lru() {
        let mut budget = StreamOutputBudget::with_limits(64, 64, 200);
        let payload: Vec<u8> = (0..300u32).map(|i| b'A' + (i % 26) as u8).collect();
        budget.append("cmd-1", &payload, true);
        let text1 = budget.read("cmd-1").expect("cmd-1");
        assert!(text1.contains("截断中间 172 字节"));
        assert!(budget.used_bytes() <= 128);

        budget.append("cmd-2", &payload, false); // active stream: never evicted!
        // Total would be 256 > 200 -> completed cmd-1 payload gets evicted, keeping summary preview!
        assert!(budget.used_bytes() <= 200);
        assert!(budget.read("cmd-2").unwrap().len() >= 128);
        assert!(!budget.read("cmd-1").unwrap().is_empty());
    }

    #[test]
    fn detail_cache_protects_pending_approvals_and_releases_collapsed() {
        let mut cache = DetailLruCache::with_budget(120);
        let chat = Uuid::new_v4();
        let turn = Uuid::new_v4();
        let k_approval = DetailKey {
            chat_id: chat,
            turn_id: turn,
            item_idx: 0,
        };
        let k_one = DetailKey {
            chat_id: chat,
            turn_id: turn,
            item_idx: 1,
        };
        let k_two = DetailKey {
            chat_id: chat,
            turn_id: turn,
            item_idx: 2,
        };

        assert!(cache.put(k_approval, "A".repeat(60), true));
        assert!(cache.put(k_one, "B".repeat(50), false));
        // Inserting 50 more bytes exceeds 120 -> evicts k_one, NEVER k_approval!
        assert!(cache.put(k_two, "C".repeat(50), false));
        assert!(cache.get(&k_approval).is_some());
        assert!(cache.get(&k_one).is_none());
        assert!(cache.get(&k_two).is_some());

        // Collapsing all disclosures removes k_two but preserves protected k_approval!
        cache.retain_expanded(chat, &BTreeSet::new());
        assert!(cache.get(&k_approval).is_some());
        assert!(cache.get(&k_two).is_none());
    }

    #[test]
    fn association_registry_enforces_immutability_and_revision_fence() {
        let mut reg = AssociationRegistry::default();
        let c1 = Uuid::new_v4();
        let assoc = SessionAssociation::schedule("daily-sales", "每日销售汇总");
        let rev = reg.bind(c1, assoc.clone(), Some(0)).unwrap();
        assert_eq!(rev, 1);
        // Stale revision rejected
        assert!(reg.bind(Uuid::new_v4(), assoc.clone(), Some(0)).is_err());
        // Different association on same chat rejected
        let other = SessionAssociation::document("report.xlsx", "报表");
        assert!(reg.bind(c1, other, Some(1)).is_err());
        assert_eq!(
            reg.list_associated_chats("schedules", "task", Some("daily-sales")),
            vec![c1]
        );
    }

    #[test]
    fn sequence_reducer_detects_duplicates_and_gaps() {
        let mut r = ClientReducer::default();
        let cid = Uuid::new_v4();
        assert_eq!(r.observe_sequence(cid, 1), SequenceDisposition::Accept);
        assert_eq!(
            r.observe_sequence(cid, 1),
            SequenceDisposition::DuplicateOrOld
        );
        assert_eq!(
            r.observe_sequence(cid, 4),
            SequenceDisposition::GapDetected {
                expected: 2,
                actual: 4
            }
        );
    }
}
