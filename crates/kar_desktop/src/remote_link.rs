//! Pure-Rust Short-Code Pairing & HMAC-Authenticated Bidirectional Remote Control Link.
//!
//! Upgrades Kar Desktop's HTTP read-only feed into a ToWork-inspired `link` + `client` layer:
//! - 6-digit, 60-second single-use pairing code (`ring::rand::SystemRandom`, brute-force capped)
//! - Constant-time HMAC-SHA256 bearer tokens (`ring::hmac`) with instant per-device revocation
//! - Idempotent `request_id` command ledger distinguishing transport receipt from business outcome
//! - Full bidirectional remote control (`SendPrompt`, `SteerOrQueue`, `WithdrawQueued`, `AbortTurn`,
//!   `DecidePermission`, `SubmitBriefFeedback`) alongside legacy `/snapshot` & `/events` compatibility.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use kar::chat::{ChatStore, InputStatus, QueuedInput};
use ring::{
    digest::{SHA256, digest},
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    brief::KarBrief,
    engine::Engine,
    client_store::{SessionAssociation, TurnCapture},
    types::SelectionTarget,
};

/// Lifetime of a 6-digit pairing code before automatic expiry (60 seconds, matching ToWork).
pub const PAIRING_CODE_TTL_MS: u64 = 60_000;
/// Maximum wrong guesses before an active 6-digit ticket is burned immediately.
pub const MAX_PAIRING_ATTEMPTS: u8 = 5;

pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerRole {
    #[default]
    Controller,
    Viewer,
}

impl PeerRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Controller => "双向控制",
            Self::Viewer => "只读观察",
        }
    }

    pub fn can_mutate(self) -> bool {
        matches!(self, Self::Controller)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingTicketView {
    pub code: String,
    pub issued_ms: u64,
    pub expires_ms: u64,
    pub failed_attempts: u8,
}

impl PairingTicketView {
    pub fn formatted_code(&self) -> String {
        if self.code.len() == 6 {
            format!("{} {}", &self.code[..3], &self.code[3..])
        } else {
            self.code.clone()
        }
    }

    pub fn remaining_fraction(&self, now_ms: u64) -> f32 {
        if now_ms >= self.expires_ms {
            return 0.0;
        }
        let total = self.expires_ms.saturating_sub(self.issued_ms).max(1) as f32;
        ((self.expires_ms - now_ms) as f32 / total).clamp(0.0, 1.0)
    }

    pub fn remaining_seconds(&self, now_ms: u64) -> u64 {
        self.expires_ms.saturating_sub(now_ms).div_ceil(1000)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedPeerInfo {
    pub peer_id: Uuid,
    pub device_name: String,
    pub role: PeerRole,
    pub paired_at_ms: u64,
    pub last_seen_ms: u64,
    pub rtt_ms: u32,
    pub commands_admitted: u64,
    pub revoked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuestShareLink {
    #[serde(default)]
    pub chat_id: Option<Uuid>,
    pub share_id: Uuid,
    pub token: String,
    pub created_ms: u64,
    pub expires_ms: u64,
}

impl GuestShareLink {
    pub(crate) async fn issue(engine: &Engine, chat_id: Uuid, ttl_minutes: u64) -> Result<Self> {
        use serde_json::json;
        let project = engine.project_id();
        let issued = engine.request(
            Method::POST,
            &format!("/projects/{project}/chats/{chat_id}/shares"),
            Some(json!({"ttl_seconds": ttl_minutes.saturating_mul(60)})),
        ).await?;
        let share_id: Uuid = issued["id"].as_str().context("missing share ID")?.parse()?;
        let credential = issued["credential"].as_str().context("missing share credential")?;
        let expires_ms = issued["expires_ms"].as_u64().context("missing share expiration")?;
        let redeemed = engine.request(
            Method::POST,
            &format!("/guest/{share_id}/redeem"),
            Some(json!({"name": "Web guest", "credential": credential})),
        ).await?;
        Ok(Self {
            chat_id: Some(chat_id),
            share_id,
            token: redeemed["token"].as_str().context("missing guest token")?.into(),
            created_ms: now_epoch_ms(),
            expires_ms,
        })
    }

    pub fn is_active(&self, now_ms: u64) -> bool {
        now_ms < self.expires_ms
    }

    pub fn full_url(&self, base_url: &str) -> String {
        format!(
            "{}/guest/{}?token={}",
            base_url.trim_end_matches('/'),
            self.share_id,
            self.token
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteHostTelemetry {
    #[serde(default)]
    pub chat_id: Option<Uuid>,
    pub listen_url: Option<String>,
    pub lan_url: Option<String>,
    pub active_ticket: Option<PairingTicketView>,
    pub peers: Vec<PairedPeerInfo>,
    #[serde(default)]
    pub guest_shares: Vec<GuestShareLink>,
    pub total_events_broadcast: u64,
    pub total_commands_admitted: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairRequest {
    pub code: String,
    pub device_name: String,
    #[serde(default)]
    pub requested_role: PeerRole,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairResponse {
    pub peer_id: Uuid,
    pub host_name: String,
    pub role: PeerRole,
    pub token: String,
    pub chat_id: Option<Uuid>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemoteAction {
    SendPrompt {
        text: String,
        #[serde(default)]
        target: Option<SelectionTarget>,
    },
    SteerOrQueue {
        text: String,
        #[serde(default)]
        target: Option<SelectionTarget>,
    },
    WithdrawQueued {
        input_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chat_id: Option<Uuid>,
    },
    AbortTurn,
    DecidePermission {
        #[serde(default)]
        pending_id: String,
        allow: bool,
    },
    SubmitBriefFeedback {
        brief_id: String,
        revision: u64,
        formatted_reply: String,
    },
    Ping {
        sent_ms: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteCommandEnvelope {
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub expected_chat_id: Option<Uuid>,
    pub request_id: Uuid,
    #[serde(default)]
    pub expected_revision: Option<u64>,
    pub action: RemoteAction,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RemoteOutcome {
    Admitted,
    Pong { server_ms: u64 },
    OutcomeUnknown { detail: String },
    Rejected { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteCommandReceipt {
    pub request_id: Uuid,
    pub peer_id: Uuid,
    pub outcome: RemoteOutcome,
    pub recorded_ms: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RemoteExtendedSnapshot {
    pub chat_id: Option<Uuid>,
    pub host_name: String,
    pub turn_captures: BTreeMap<Uuid, TurnCapture>,
    pub association: Option<SessionAssociation>,
    pub briefs: Vec<KarBrief>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_inputs: Option<Vec<QueuedInput>>,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueCopyError {
    Unavailable,
    Changed,
    HostAttachments,
}

impl RemoteExtendedSnapshot {
    pub fn pending_text(&self, chat_id: Uuid, input_id: Uuid) -> Result<String, QueueCopyError> {
        if self.chat_id != Some(chat_id) {
            return Err(QueueCopyError::Changed);
        }
        let queue = self
            .queued_inputs
            .as_ref()
            .ok_or(QueueCopyError::Unavailable)?;
        let input = queue
            .iter()
            .find(|q| q.id == input_id && q.status == InputStatus::Pending)
            .ok_or(QueueCopyError::Changed)?;
        if !input.attachments.is_empty() {
            return Err(QueueCopyError::HostAttachments);
        }
        Ok(crate::types::strip_selection_envelope(&input.text).to_owned())
    }
}

/// Cryptographic pairing & command admission state shared with the embedded Axum server.
pub struct RemoteLinkCore {
    signing_key: hmac::Key,
    rng: SystemRandom,
    pub host_name: String,
    pub listen_url: Option<String>,
    pub lan_url: Option<String>,
    pub chat_id: Option<Uuid>,
    pub ticket: Option<PairingTicketView>,
    peers: BTreeMap<Uuid, (PairedPeerInfo, String)>,
    peer_changes: tokio::sync::watch::Sender<()>,
    guest_shares: Vec<GuestShareLink>,
    persist_dir: Option<std::path::PathBuf>,
    /// Durable in-session idempotent ledger (`request_id -> receipt`).
    writable_chat: Option<Option<Uuid>>,
    ledger: BTreeMap<Uuid, RemoteCommandReceipt>,
    pub extended_snapshot: RemoteExtendedSnapshot,
    pub total_events_broadcast: u64,
    pub total_commands_admitted: u64,
    dispatch_tx: Option<async_channel::Sender<(Uuid, RemoteCommandEnvelope)>>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
struct PersistedLinkStore {
    peers: Vec<(PairedPeerInfo, String)>,
    #[serde(default)]
    guest_shares: Vec<GuestShareLink>,
}

// Pairing secrets and guest credentials must never be created with the process's
// default umask. Write privately before making each complete file visible.
fn write_private_new(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(data)?;
    file.sync_all()
}

impl RemoteLinkCore {
    pub fn new(
        host_name: impl Into<String>,
        dispatch_tx: Option<async_channel::Sender<(Uuid, RemoteCommandEnvelope)>>,
    ) -> Self {
        let rng = SystemRandom::new();
        let mut secret = [0u8; 32];
        rng.fill(&mut secret).expect("system random generator");
        let signing_key = hmac::Key::new(hmac::HMAC_SHA256, &secret);
        let host_name = host_name.into();
        Self {
            signing_key,
            rng,
            host_name: host_name.clone(),
            listen_url: None,
            lan_url: None,
            chat_id: None,
            ticket: None,
            peers: BTreeMap::new(),
            peer_changes: tokio::sync::watch::channel(()).0,
            guest_shares: Vec::new(),
            persist_dir: None,
            writable_chat: None,
            ledger: BTreeMap::new(),
            extended_snapshot: RemoteExtendedSnapshot {
                host_name,
                ..Default::default()
            },
            total_events_broadcast: 0,
            total_commands_admitted: 0,
            dispatch_tx,
        }
    }

    pub fn new_persistent(
        store_dir: &std::path::Path,
        host_name: impl Into<String>,
        dispatch_tx: Option<async_channel::Sender<(Uuid, RemoteCommandEnvelope)>>,
    ) -> Self {
        let _ = std::fs::create_dir_all(store_dir);
        let rng = SystemRandom::new();
        let key_path = store_dir.join("remote_link_secret.bin");
        let secret: [u8; 32] = match std::fs::read(&key_path) {
            Ok(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                arr
            }
            _ => {
                let mut arr = [0u8; 32];
                rng.fill(&mut arr).expect("system random generator");
                let staging = store_dir.join(format!(".remote-key-{}.tmp", Uuid::new_v4()));
                if write_private_new(&staging, &arr).is_ok() {
                    // If another window published its key first, use its complete key.
                    if std::fs::hard_link(&staging, &key_path).is_err()
                        && let Ok(existing) = std::fs::read(&key_path)
                        && let Ok(winner) = <[u8; 32]>::try_from(existing.as_slice())
                    {
                        arr = winner;
                    }
                    let _ = std::fs::remove_file(staging);
                }
                arr
            }
        };
        let signing_key = hmac::Key::new(hmac::HMAC_SHA256, &secret);
        let saved: PersistedLinkStore = std::fs::read_to_string(store_dir.join("paired_peers.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let mut peers = BTreeMap::new();
        for (p, digest) in saved.peers {
            peers.insert(p.peer_id, (p, digest));
        }
        let host_name = host_name.into();
        Self {
            signing_key,
            rng,
            host_name: host_name.clone(),
            listen_url: None,
            lan_url: None,
            chat_id: None,
            ticket: None,
            peers,
            peer_changes: tokio::sync::watch::channel(()).0,
            guest_shares: saved.guest_shares,
            persist_dir: Some(store_dir.to_path_buf()),
            writable_chat: None,
            ledger: BTreeMap::new(),
            extended_snapshot: RemoteExtendedSnapshot {
                host_name,
                ..Default::default()
            },
            total_events_broadcast: 0,
            total_commands_admitted: 0,
            dispatch_tx,
        }
    }

    fn persist_peers(&self) {
        let Some(ref dir) = self.persist_dir else {
            return;
        };
        let payload = PersistedLinkStore {
            peers: self.peers.values().cloned().collect(),
            guest_shares: self.guest_shares.clone(),
        };
        if let Ok(json) = serde_json::to_vec_pretty(&payload) {
            let tmp = dir.join(format!(".paired_peers-{}.tmp", Uuid::new_v4()));
            if write_private_new(&tmp, &json).is_ok() {
                let _ = std::fs::rename(&tmp, dir.join("paired_peers.json"));
            } else {
                let _ = std::fs::remove_file(tmp);
            }
        }
    }

    pub(crate) fn remember_guest_share(&mut self, share: GuestShareLink) {
        // Kar invalidates the previous invitation when the owner issues a new one for a chat.
        self.guest_shares.retain(|old| old.chat_id.is_some() && old.chat_id != share.chat_id && old.is_active(share.created_ms));
        self.guest_shares.push(share);
        self.persist_peers();
    }

    pub fn revoke_guest_share(&mut self, share_id: Uuid) -> bool {
        let before = self.guest_shares.len();
        self.guest_shares.retain(|g| g.share_id != share_id);
        let changed = self.guest_shares.len() < before;
        if changed {
            self.persist_peers();
        }
        changed
    }

    /// Generate a fresh 6-digit 60-second pairing code, replacing any previous unredeemed code.
    pub fn issue_pairing_ticket(&mut self, now_ms: u64) -> PairingTicketView {
        let mut bytes = [0u8; 4];
        self.rng.fill(&mut bytes).expect("system random generator");
        let num = u32::from_le_bytes(bytes) % 1_000_000;
        let code = format!("{num:06}");
        let view = PairingTicketView {
            code,
            issued_ms: now_ms,
            expires_ms: now_ms.saturating_add(PAIRING_CODE_TTL_MS),
            failed_attempts: 0,
        };
        self.ticket = Some(view.clone());
        view
    }

    pub fn clear_ticket(&mut self) {
        self.ticket = None;
    }

    /// Exchange a valid 6-digit short code for a revocable HMAC-signed session token.
    pub fn redeem_pairing_ticket(
        &mut self,
        req: PairRequest,
        now_ms: u64,
    ) -> std::result::Result<PairResponse, (StatusCode, String)> {
        let normalized: String = req.code.chars().filter(char::is_ascii_digit).collect();
        let Some(mut current) = self.ticket.take() else {
            return Err((
                StatusCode::UNAUTHORIZED,
                "当前没有开启中的配对码，请在主机端刷新生成新的 6 位配对码".into(),
            ));
        };
        if now_ms >= current.expires_ms {
            return Err((
                StatusCode::GONE,
                "该 6 位配对码已过期（有效期 60 秒），请刷新后重试".into(),
            ));
        }
        if normalized != current.code {
            current.failed_attempts = current.failed_attempts.saturating_add(1);
            if current.failed_attempts < MAX_PAIRING_ATTEMPTS {
                self.ticket = Some(current);
                return Err((StatusCode::UNAUTHORIZED, "配对码不匹配，请核对 6 位数字".into()));
            }
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                "连续尝试次数过多，当前配对码已自动失效".into(),
            ));
        }

        // Single-use ticket consumed! Issue peer identity + HMAC token.
        let peer_id = Uuid::new_v4();
        let mut nonce = [0u8; 16];
        let _ = self.rng.fill(&mut nonce);
        let nonce_b64 = URL_SAFE_NO_PAD.encode(nonce);
        let payload = format!("kar_link_v1.{peer_id}.{nonce_b64}");
        let tag = hmac::sign(&self.signing_key, payload.as_bytes());
        let sig_b64 = URL_SAFE_NO_PAD.encode(tag.as_ref());
        let token = format!("{payload}.{sig_b64}");
        let token_digest = URL_SAFE_NO_PAD.encode(digest(&SHA256, token.as_bytes()).as_ref());

        let clean_name = {
            let t = req.device_name.trim();
            if t.is_empty() {
                "远端控制器".into()
            } else {
                t.chars().take(40).collect()
            }
        };
        let info = PairedPeerInfo {
            peer_id,
            device_name: clean_name,
            role: req.requested_role,
            paired_at_ms: now_ms,
            last_seen_ms: now_ms,
            rtt_ms: 1,
            commands_admitted: 0,
            revoked: false,
        };
        self.peers.insert(peer_id, (info, token_digest));
        self.persist_peers();

        Ok(PairResponse {
            peer_id,
            host_name: self.host_name.clone(),
            role: req.requested_role,
            token,
            chat_id: self.chat_id,
        })
    }

    pub fn authenticate_token(
        &mut self,
        raw_token: &str,
        now_ms: u64,
    ) -> std::result::Result<PairedPeerInfo, (StatusCode, String)> {
        let token = raw_token.trim().trim_start_matches("Bearer ").trim();
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 4 || parts[0] != "kar_link_v1" {
            return Err((StatusCode::UNAUTHORIZED, "无效的节点访问凭证".into()));
        }
        let Ok(peer_id) = Uuid::parse_str(parts[1]) else {
            return Err((StatusCode::UNAUTHORIZED, "设备标识格式错误".into()));
        };
        let payload = format!("{}.{}.{}", parts[0], parts[1], parts[2]);
        let Ok(sig_bytes) = URL_SAFE_NO_PAD.decode(parts[3]) else {
            return Err((StatusCode::UNAUTHORIZED, "签名编码无效".into()));
        };
        if hmac::verify(&self.signing_key, payload.as_bytes(), &sig_bytes).is_err() {
            return Err((StatusCode::UNAUTHORIZED, "HMAC 签名校验未通过".into()));
        }
        let expected_digest = URL_SAFE_NO_PAD.encode(digest(&SHA256, token.as_bytes()).as_ref());
        let Some((info, stored_digest)) = self.peers.get_mut(&peer_id) else {
            return Err((StatusCode::UNAUTHORIZED, "该设备未在此主机注册".into()));
        };
        if info.revoked || *stored_digest != expected_digest {
            return Err((StatusCode::FORBIDDEN, "该设备的配对授权已被主机撤销".into()));
        }
        info.last_seen_ms = now_ms;
        Ok(info.clone())
    }

    pub fn revoke_peer(&mut self, peer_id: Uuid) -> bool {
        if let Some((info, _)) = self.peers.get_mut(&peer_id) {
            info.revoked = true;
            self.peer_changes.send_replace(());
            self.persist_peers();
            return true;
        }
        false
    }

    pub fn telemetry(&self, now_ms: u64) -> RemoteHostTelemetry {
        let active_ticket = self
            .ticket
            .as_ref()
            .filter(|t| now_ms < t.expires_ms)
            .cloned();
        let peers = self.peers.values().map(|(p, _)| p.clone()).collect();
        let guest_shares = self
            .guest_shares
            .iter()
            .filter(|g| self.chat_id.is_some() && g.chat_id == self.chat_id && g.is_active(now_ms))
            .cloned()
            .collect();
        RemoteHostTelemetry {
            chat_id: self.chat_id,
            listen_url: self.listen_url.clone(),
            lan_url: self.lan_url.clone(),
            active_ticket,
            peers,
            guest_shares,
            total_events_broadcast: self.total_events_broadcast,
            total_commands_admitted: self.total_commands_admitted,
        }
    }

    pub(crate) fn set_writable_chat(&mut self, chat: Option<Uuid>) {
        self.writable_chat = Some(chat);
    }

    pub(crate) fn may_control(&self, peer: Uuid, chat: Uuid) -> bool {
        self.peers.get(&peer).is_some_and(|(p,_)| !p.revoked && p.role.can_mutate())
            && self.chat_id.unwrap_or_default() == chat
            && self.writable_chat.is_none_or(|active| active == Some(chat))
    }

    pub fn admit_command(
        &mut self,
        peer: &PairedPeerInfo,
        mut env: RemoteCommandEnvelope,
        now_ms: u64,
    ) -> RemoteCommandReceipt {
        // Idempotent replay: return original receipt without re-running side effects!
        if let Some(existing) = self.ledger.get(&env.request_id) {
            if existing.peer_id != peer.peer_id {
                return RemoteCommandReceipt {request_id:env.request_id,peer_id:peer.peer_id,recorded_ms:now_ms,
                    outcome:RemoteOutcome::Rejected{reason:"此请求编号属于另一设备".into()}};
            }
            return existing.clone();
        }
        if let RemoteAction::Ping { sent_ms } = env.action {
            let rtt = now_ms.saturating_sub(sent_ms).min(9999) as u32;
            if let Some((p, _)) = self.peers.get_mut(&peer.peer_id) {
                p.rtt_ms = rtt.max(1);
                p.last_seen_ms = now_ms;
            }
            return RemoteCommandReceipt {
                request_id: env.request_id,
                peer_id: peer.peer_id,
                outcome: RemoteOutcome::Pong { server_ms: now_ms },
                recorded_ms: now_ms,
            };
        }
        if !peer.role.can_mutate() {
            return RemoteCommandReceipt {
                request_id: env.request_id,
                peer_id: peer.peer_id,
                outcome: RemoteOutcome::Rejected {
                    reason: "当前配对角色为「只读观察」，无权提交控制指令".into(),
                },
                recorded_ms: now_ms,
            };
        }
        let chat = self.chat_id.unwrap_or_default();
        if env.expected_chat_id.is_some_and(|expected|expected!=chat) || !self.may_control(peer.peer_id,chat) {
            return RemoteCommandReceipt {request_id:env.request_id,peer_id:peer.peer_id,recorded_ms:now_ms,
                outcome:RemoteOutcome::Rejected {reason:"共享会话已切换或主机当前只读，请在主机打开要控制的会话".into()}};
        }
        env.expected_chat_id=Some(chat);
        if let RemoteAction::WithdrawQueued {
            chat_id: Some(expected),
            ..
        } = env.action
            && self.chat_id != Some(expected)
        {
            return RemoteCommandReceipt {
                request_id: env.request_id,
                peer_id: peer.peer_id,
                recorded_ms: now_ms,
                outcome: RemoteOutcome::Rejected {
                    reason: "共享会话已切换，请刷新队列".into(),
                },
            };
        }
        if let Some(exp_rev) = env.expected_revision
            && exp_rev != self.extended_snapshot.revision
        {
            return RemoteCommandReceipt {
                request_id: env.request_id,
                peer_id: peer.peer_id,
                outcome: RemoteOutcome::Rejected {
                    reason: format!(
                        "远端状态版本已变化（期望 v{exp_rev}，当前 v{}），请同步后再操作",
                        self.extended_snapshot.revision
                    ),
                },
                recorded_ms: now_ms,
            };
        }

        let outcome = if let Some(ref tx) = self.dispatch_tx {
            match tx.try_send((peer.peer_id, env.clone())) {
                Ok(()) => {
                    self.total_commands_admitted = self.total_commands_admitted.saturating_add(1);
                    if let Some((p, _)) = self.peers.get_mut(&peer.peer_id) {
                        p.commands_admitted = p.commands_admitted.saturating_add(1);
                    }
                    RemoteOutcome::Admitted
                }
                Err(_) => RemoteOutcome::OutcomeUnknown {
                    detail: "主机桥接队列繁忙，未确认受理（请勿盲目更换 ID 重放）".into(),
                },
            }
        } else {
            self.total_commands_admitted = self.total_commands_admitted.saturating_add(1);
            if let Some((p, _)) = self.peers.get_mut(&peer.peer_id) {
                p.commands_admitted = p.commands_admitted.saturating_add(1);
            }
            RemoteOutcome::Admitted
        };

        let receipt = RemoteCommandReceipt {
            request_id: env.request_id,
            peer_id: peer.peer_id,
            outcome,
            recorded_ms: now_ms,
        };
        self.ledger.insert(env.request_id, receipt.clone());
        receipt
    }
}

pub type SharedRemoteLink = Arc<Mutex<RemoteLinkCore>>;

fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

async fn handle_pair(
    State(link): State<SharedRemoteLink>,
    Json(req): Json<PairRequest>,
    notify: impl FnOnce(RemoteHostTelemetry),
) -> Response {
    let now = now_epoch_ms();
    let (res, snapshot) = {
        let mut guard = link.lock().unwrap_or_else(|e| e.into_inner());
        (guard.redeem_pairing_ticket(req, now), guard.telemetry(now))
    };
    notify(snapshot);
    match res {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err((code, msg)) => (code, Json(serde_json::json!({ "error": msg }))).into_response(),
    }
}

async fn handle_v1_state(
    State(link): State<SharedRemoteLink>,
    headers: HeaderMap,
    shared_queue: Option<(Uuid, ChatStore)>,
) -> Response {
    let Some(token) = extract_bearer(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "缺少 Authorization 令牌" })),
        )
            .into_response();
    };
    let (peer, mut snapshot) = {
        let mut guard = link.lock().unwrap_or_else(|e| e.into_inner());
        let peer = match guard.authenticate_token(&token, now_epoch_ms()) {
            Ok(peer) => peer,
            Err((code, msg)) => {
                return (code, Json(serde_json::json!({ "error": msg }))).into_response();
            }
        };
        (peer, guard.extended_snapshot.clone())
    };
    snapshot.queued_inputs = None;
    if peer.role.can_mutate()
        && let Some((chat_id, store)) = shared_queue
        && snapshot.chat_id == Some(chat_id)
    {
        snapshot.queued_inputs = tokio::task::spawn_blocking(move || {
            store.queued_inputs(chat_id).map(|entries| {
                entries
                    .into_iter()
                    .filter(|q| matches!(q.status, InputStatus::Pending | InputStatus::Sending))
                    .collect()
            })
        })
        .await
        .ok()
        .and_then(Result::ok);
    }
    (StatusCode::OK, Json(snapshot)).into_response()
}

async fn handle_v1_command(
    State(link): State<SharedRemoteLink>,
    headers: HeaderMap,
    Json(env): Json<RemoteCommandEnvelope>,
) -> Response {
    let Some(token) = extract_bearer(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "缺少 Authorization 令牌" })),
        )
            .into_response();
    };
    let now = now_epoch_ms();
    let mut guard = link.lock().unwrap_or_else(|e| e.into_inner());
    match guard.authenticate_token(&token, now) {
        Ok(peer) => {
            let receipt = guard.admit_command(&peer, env, now);
            let status = match &receipt.outcome {
                RemoteOutcome::Admitted | RemoteOutcome::Pong { .. } => StatusCode::OK,
                RemoteOutcome::OutcomeUnknown { .. } => StatusCode::ACCEPTED,
                RemoteOutcome::Rejected { .. } => StatusCode::CONFLICT,
            };
            (status, Json(receipt)).into_response()
        }
        Err((code, msg)) => (code, Json(serde_json::json!({ "error": msg }))).into_response(),
    }
}

/// LAN listeners expose the raw Kar event feed only to paired peers. End an existing
/// event stream as soon as its peer is revoked or the host stops sharing.
pub(crate) fn shared_feed_routes(
    router: Router,
    link: SharedRemoteLink,
    require_pairing: bool,
    stopped: tokio::sync::watch::Receiver<bool>,
) -> Router {
    use axum::{body::Body, extract::Request, middleware::{self, Next}};
    use futures::StreamExt as _;
    router.route_layer(middleware::from_fn(move |request: Request, next: Next| {
        let link = link.clone();
        let mut stopped = stopped.clone();
        async move {
            let peer = if require_pairing {
                let Some(header) = extract_bearer(request.headers()) else {
                    return StatusCode::UNAUTHORIZED.into_response();
                };
                let mut core = link.lock().unwrap_or_else(|e| e.into_inner());
                match core.authenticate_token(&header, now_epoch_ms()) {
                    Ok(peer) => Some((peer.peer_id, core.peer_changes.subscribe())),
                    Err((code, _)) => return code.into_response(),
                }
            } else { None };
            let stop = async move {
                let stop_sharing = stopped.wait_for(|value| *value);
                let revoked = async {
                    if let Some((id, mut changes)) = peer {
                        while changes.changed().await.is_ok() {
                            let core = link.lock().unwrap_or_else(|e| e.into_inner());
                            if core.peers.get(&id).is_none_or(|(p, _)| p.revoked) { break; }
                        }
                    } else { std::future::pending::<()>().await; }
                };
                tokio::select! { _ = stop_sharing => {}, _ = revoked => {} }
            };
            let response = next.run(request).await;
            let (parts, body) = response.into_parts();
            Response::from_parts(parts, Body::from_stream(body.into_data_stream().take_until(stop)))
        }
    }))
}

pub(crate) fn subscriber_feed(url: &str, token: Option<&str>) -> Result<kar::chat::HttpChatFeed> {
    let Some(token) = token else { return Ok(kar::chat::HttpChatFeed::new(url)?); };
    let mut headers = reqwest::header::HeaderMap::new();
    let mut bearer = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))?;
    bearer.set_sensitive(true);
    headers.insert(reqwest::header::AUTHORIZATION, bearer);
    let client = reqwest::Client::builder().default_headers(headers)
        .redirect(reqwest::redirect::Policy::none()).build()?;
    Ok(kar::chat::HttpChatFeed::with_client(url, client)?)
}

async fn handle_guest_view(
    State(link): State<SharedRemoteLink>,
    axum::extract::Path(share_id): axum::extract::Path<Uuid>,
    axum::extract::Query(params): axum::extract::Query<BTreeMap<String, String>>,
    engine: Option<Engine>,
) -> Response {
    let Some(engine) = engine else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let shared_chat = link.lock().unwrap_or_else(|e| e.into_inner()).chat_id;
    let Some(token) = params.get("token").filter(|token| !token.is_empty()) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Ok(snapshot) = engine.guest_snapshot(share_id, token).await else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if shared_chat.is_none() || snapshot["chat"].as_str().and_then(|id| id.parse().ok()) != shared_chat {
        return StatusCode::FORBIDDEN.into_response();
    }

    let mut items = String::new();
    for turn in snapshot["turns"].as_array().into_iter().flatten() {
        for message in turn["messages"].as_array().into_iter().flatten() {
            let role = match message["role"].as_str() {
                Some("user") => "User",
                Some("assistant") => "Assistant",
                _ => continue,
            };
            if let Some(text) = message["text"].as_str() {
                let escaped = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
                items.push_str(&format!("<article><strong>{role}</strong><p>{escaped}</p></article>"));
            }
        }
    }
    let html = format!(
        "<!doctype html><html lang='zh'><meta charset='utf-8'><meta name='viewport' content='width=device-width, initial-scale=1'><meta http-equiv='refresh' content='5'><title>Kwrok · 只读对话</title><style>:root{{color-scheme:light dark}}body{{font:1rem/1.6 system-ui;max-width:48rem;margin:auto;padding:1rem}}p{{white-space:pre-wrap;overflow-wrap:anywhere}}strong{{opacity:.7}}</style><h1>只读对话</h1>{items}</html>"
    );
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "no-store"),
            (axum::http::header::REFERRER_POLICY, "no-referrer"),
        ],
        html,
    ).into_response()
}

/// Build the `/v1/*` bidirectional pairing & control router to merge onto Kar's `HttpChatFeed` router.
pub fn remote_link_router(link: SharedRemoteLink) -> Router {
    remote_link_router_with_updates(link, None, None, |_| {})
}

pub(crate) fn remote_link_router_with_updates(
    link: SharedRemoteLink,
    engine: Option<Engine>,
    shared_queue: Option<(Uuid, ChatStore)>,
    notify: impl Fn(RemoteHostTelemetry) + Clone + Send + Sync + 'static,
) -> Router {
    Router::new()
        .route(
            "/v1/pair",
            post(move |state, req| handle_pair(state, req, notify.clone())),
        )
        .route(
            "/v1/state",
            get(move |state, headers| handle_v1_state(state, headers, shared_queue.clone())),
        )
        .route("/v1/command", post(handle_v1_command))
        .route(
            "/guest/{share_id}",
            get(move |state, path, query| {
                handle_guest_view(state, path, query, engine.clone())
            }),
        )
        .with_state(link)
}

/// Blocking client helper (run inside `tokio::task::spawn_blocking`) to redeem a 6-digit code.
pub fn client_redeem_pair_code(
    base_url: &str,
    code: &str,
    device_name: &str,
    role: PeerRole,
) -> Result<PairResponse> {
    let url = format!("{}/v1/pair", base_url.trim_end_matches('/'));
    let body = PairRequest {
        code: code.to_owned(),
        device_name: device_name.to_owned(),
        requested_role: role,
    };
    let payload = serde_json::to_string(&body)?;
    let mut resp = ureq::post(&url)
        .header("content-type", "application/json")
        .send(payload.as_bytes())?;
    let raw = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| anyhow::anyhow!("读取配对响应失败：{e}"))?;
    let parsed: PairResponse =
        serde_json::from_str(&raw).map_err(|e| anyhow::anyhow!("解析配对响应失败：{e}"))?;
    Ok(parsed)
}

/// Blocking client helper to dispatch a typed `RemoteCommandEnvelope` with bearer token.
pub fn client_send_remote_command(
    base_url: &str,
    token: &str,
    env: &RemoteCommandEnvelope,
) -> Result<RemoteCommandReceipt> {
    let url = format!("{}/v1/command", base_url.trim_end_matches('/'));
    let auth = format!("Bearer {}", token.trim());
    let payload = serde_json::to_string(env)?;
    let res = ureq::post(&url)
        .config().http_status_as_error(false).max_redirects(0).build()
        .header("authorization", &auth)
        .header("content-type", "application/json")
        .send(payload.as_bytes());
    match res {
        Ok(mut resp) if resp.status().is_success() || resp.status().as_u16()==409 => {
            let raw = resp.body_mut().read_to_string().context("读取指令回执失败")?;
            let receipt: RemoteCommandReceipt = serde_json::from_str(&raw).context("解析指令回执失败")?;
            anyhow::ensure!(receipt.request_id==env.request_id,"主机返回了另一指令的回执");
            Ok(receipt)
        }
        Ok(resp) => bail!("远端指令未获确认（HTTP {}）",resp.status()),
        Err(e) => Ok(RemoteCommandReceipt {
            request_id: env.request_id,
            peer_id: Uuid::nil(),
            outcome: RemoteOutcome::OutcomeUnknown {
                detail: format!("传输中断，业务结果未知（保留同一 request_id 可安全重查）：{e}"),
            },
            recorded_ms: now_epoch_ms(),
        }),
    }
}

/// Blocking client helper to read extended remote state (`/v1/state`).
pub fn client_fetch_remote_state(base_url: &str, token: &str) -> Result<RemoteExtendedSnapshot> {
    let url = format!("{}/v1/state", base_url.trim_end_matches('/'));
    let auth = format!("Bearer {}", token.trim());
    let mut resp = ureq::get(&url)
        .header("authorization", &auth)
        .call()
        .map_err(|e| anyhow::anyhow!("读取远端扩展快照失败：{e}"))?;
    let raw = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| anyhow::anyhow!("读取远端扩展快照失败：{e}"))?;
    let snap: RemoteExtendedSnapshot =
        serde_json::from_str(&raw).map_err(|e| anyhow::anyhow!("解析远端扩展快照失败：{e}"))?;
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_copy_requires_a_pending_text_message_in_the_shared_thread() {
        let chat = Uuid::new_v4();
        let id = Uuid::new_v4();
        let mut input: QueuedInput = serde_json::from_value(serde_json::json!({
            "id":id, "text":"original", "status":"pending"
        }))
        .unwrap();
        let mut snapshot = RemoteExtendedSnapshot {
            chat_id: Some(chat),
            ..Default::default()
        };
        assert_eq!(
            snapshot.pending_text(chat, id),
            Err(QueueCopyError::Unavailable)
        );
        snapshot.queued_inputs = Some(vec![input.clone()]);
        assert_eq!(snapshot.pending_text(chat, id), Ok("original".into()));
        assert_eq!(
            snapshot.pending_text(Uuid::new_v4(), id),
            Err(QueueCopyError::Changed)
        );
        assert_eq!(
            snapshot.pending_text(chat, Uuid::new_v4()),
            Err(QueueCopyError::Changed)
        );
        input.text = "请针对以下工作区源码文件 src/main.rs\n用户要求：\nkeep my words".into();
        snapshot.queued_inputs = Some(vec![input.clone()]);
        assert_eq!(snapshot.pending_text(chat, id), Ok("keep my words".into()));
        input.status = InputStatus::Sending;
        snapshot.queued_inputs = Some(vec![input.clone()]);
        assert_eq!(
            snapshot.pending_text(chat, id),
            Err(QueueCopyError::Changed)
        );
        input.status = InputStatus::Pending;
        input.attachments.push(serde_json::from_value(serde_json::json!({
            "id":Uuid::new_v4(),"name":"fixture.txt","kind":"file","mime":"text/plain","bytes":3,"sha256":"fixture"
        })).unwrap());
        snapshot.queued_inputs = Some(vec![input]);
        assert_eq!(
            snapshot.pending_text(chat, id),
            Err(QueueCopyError::HostAttachments)
        );
    }

    #[test]
    fn short_code_pairing_single_use_hmac_auth_idempotency_and_revocation() {
        let mut core = RemoteLinkCore::new("Leon-MacStudio", None);
        let ticket = core.issue_pairing_ticket(1_000);
        assert_eq!(ticket.code.len(), 6);
        assert_eq!(ticket.remaining_seconds(1_000), 60);

        // Wrong code increments failed attempts without burning ticket immediately
        assert!(
            core.redeem_pairing_ticket(
                PairRequest {
                    code: "000000".into(),
                    device_name: "Phone".into(),
                    requested_role: PeerRole::Controller,
                },
                2_000,
            )
            .is_err()
                || ticket.code == "000000"
        );

        // Re-issue clean ticket for deterministic redemption test
        let ticket = core.issue_pairing_ticket(3_000);
        let paired = core
            .redeem_pairing_ticket(
                PairRequest {
                    code: ticket.formatted_code(), // spaces stripped cleanly
                    device_name: "MacBook-Air".into(),
                    requested_role: PeerRole::Controller,
                },
                4_000,
            )
            .expect("pair succeeds");

        // Second redemption of same single-use ticket fails immediately!
        assert!(
            core.redeem_pairing_ticket(
                PairRequest {
                    code: ticket.code.clone(),
                    device_name: "Attacker".into(),
                    requested_role: PeerRole::Controller,
                },
                4_100,
            )
            .is_err()
        );

        // Token authenticates cleanly
        let peer = core
            .authenticate_token(&paired.token, 5_000)
            .expect("valid token");
        assert_eq!(peer.device_name, "MacBook-Air");

        // Idempotent command admission
        let req_id = Uuid::new_v4();
        let env = RemoteCommandEnvelope {
            expected_chat_id: None,
            request_id: req_id,
            expected_revision: Some(0),
            action: RemoteAction::SendPrompt {
                text: "整理周报".into(),
                target: None,
            },
        };
        let r1 = core.admit_command(&peer, env.clone(), 5_100);
        let r2 = core.admit_command(&peer, env, 5_200);
        assert_eq!(r1.outcome, RemoteOutcome::Admitted);
        assert_eq!(r1.recorded_ms, r2.recorded_ms);
        assert_eq!(core.total_commands_admitted, 1);

        // Revoking peer immediately blocks token!
        assert!(core.revoke_peer(paired.peer_id));
        assert!(core.authenticate_token(&paired.token, 6_000).is_err());
    }
}
