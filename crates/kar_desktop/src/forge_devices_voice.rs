//! Native Forge (GitHub / GitLab / Linear PRs & Issues), External Editors, Worktree Cleanup,
//! Voice Capture & STT, iOS/Android Simulators, External Transcript Import, ACP Catalog,
//! Local Dev-Server Port Detection, Storage Cleanup, and macOS Notifications / Sleep Guard.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ═══════════════════════════════════════════════════════════════════════════
// 1. External Editors Discovery & Launch
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct InstalledEditor {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub app_or_bin: String,
}

pub fn probe_installed_editors() -> Vec<InstalledEditor> {
    let mut out = Vec::new();
    let candidates = [
        ("xcode", "Xcode", "/Applications/Xcode.app"),
        ("sublime", "Sublime Text", "/Applications/Sublime Text.app"),
        ("iterm", "iTerm2", "/Applications/iTerm.app"),
    ];
    for (id, label, app_path) in candidates {
        if Path::new(app_path).exists() {
            out.push(InstalledEditor {
                id: id.into(),
                label: label.into(),
                app_or_bin: app_path.into(),
            });
        }
    }
    if cfg!(target_os = "macos") {
    out.push(InstalledEditor {
        id: "terminal".into(),
        label: "Terminal".into(),
        app_or_bin: "/System/Applications/Utilities/Terminal.app".into(),
    });
    }
    out
}

pub fn open_in_external_editor(
    preferred_id: &str,
    workspace: &Path,
    target_file: Option<&Path>,
) -> Result<String> {
    let editors = probe_installed_editors();
    let target = target_file.unwrap_or(workspace);
    let chosen = if preferred_id == "auto" || preferred_id.is_empty() {
        editors.first()
    } else {
        editors
            .iter()
            .find(|e| e.id == preferred_id)
    }
    .context("未找到可用的外部编辑器")?;

    let status = if chosen.id == "finder" {
        if target.is_file() {
            Command::new("open").arg("-R").arg(target).status()?
        } else {
            Command::new("open").arg(target).status()?
        }
    } else {
        Command::new("open")
            .arg("-a")
            .arg(&chosen.app_or_bin)
            .arg(target)
            .status()?
    };
    if !status.success() {
        bail!("打开 {} 失败", chosen.label);
    }
    Ok(chosen.label.clone())
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Git Clone, Managed Worktree Cleanup & Smart Commit Message Writer
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedWorktreeItem {
    pub path: PathBuf,
    pub branch: String,
    pub head_short: String,
    pub is_main: bool,
    pub is_kar_managed: bool,
    pub has_local_changes: bool,
}

pub fn list_managed_worktrees(workspace: &Path) -> Vec<ManagedWorktreeItem> {
    let Ok(out) = Command::new("git")
        .current_dir(workspace)
        .args(["worktree", "list", "--porcelain"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut items = Vec::new();
    let mut cur_path: Option<PathBuf> = None;
    let mut cur_head = String::new();
    let mut cur_branch = String::new();
    let mut index = 0usize;

    let flush = |items: &mut Vec<ManagedWorktreeItem>,
                 path: Option<PathBuf>,
                 head: &str,
                 branch: &str,
                 idx: usize| {
        let Some(p) = path else { return };
        let short_branch = branch.strip_prefix("refs/heads/").unwrap_or(branch).to_owned();
        let is_kar = short_branch.starts_with("kar/")
            || p.to_string_lossy().contains("kar-worktree")
            || p.to_string_lossy().contains(".kar-worktrees");
        let dirty = p.is_dir()
            && Command::new("git")
                .current_dir(&p)
                .args(["status", "--porcelain"])
                .output()
                .map_or(false, |o| !o.stdout.is_empty());
        items.push(ManagedWorktreeItem {
            path: p,
            branch: if short_branch.is_empty() {
                "detached".into()
            } else {
                short_branch
            },
            head_short: head.chars().take(7).collect(),
            is_main: idx == 0,
            is_kar_managed: is_kar,
            has_local_changes: dirty,
        });
    };

    for line in text.lines() {
        if line.trim().is_empty() {
            flush(&mut items, cur_path.take(), &cur_head, &cur_branch, index);
            cur_head.clear();
            cur_branch.clear();
            index += 1;
        } else if let Some(rest) = line.strip_prefix("worktree ") {
            cur_path = Some(PathBuf::from(rest.trim()));
        } else if let Some(rest) = line.strip_prefix("HEAD ") {
            cur_head = rest.trim().to_owned();
        } else if let Some(rest) = line.strip_prefix("branch ") {
            cur_branch = rest.trim().to_owned();
        }
    }
    flush(&mut items, cur_path.take(), &cur_head, &cur_branch, index);
    items
}

pub fn prune_managed_worktrees(workspace: &Path, keep_dirty: bool) -> Result<usize> {
    let list = list_managed_worktrees(workspace);
    let mut removed = 0usize;
    for wt in list {
        if wt.is_main || !wt.is_kar_managed {
            continue;
        }
        if keep_dirty && wt.has_local_changes {
            continue;
        }
        let mut cmd = Command::new("git");
        cmd.current_dir(workspace).args(["worktree", "remove"]);
        if !keep_dirty {
            cmd.arg("--force");
        }
        if cmd.arg(&wt.path).status().is_ok_and(|s| s.success()) {
            removed += 1;
        }
    }
    let _ = Command::new("git")
        .current_dir(workspace)
        .args(["worktree", "prune"])
        .status();
    Ok(removed)
}

/// Generate a crisp commit message from staged (or working-tree) Git changes,
/// respecting the user's configured commit writing style.
pub(crate) fn commit_diff_summary(workspace: &Path) -> Result<String> {
    use std::io::Read;
    const LIMIT: usize = 24 * 1024;
    for args in [
        &["diff", "--cached", "--no-ext-diff", "--no-textconv", "--stat", "--patch", "--"][..],
        &["diff", "--no-ext-diff", "--no-textconv", "--stat", "--patch", "--"][..],
        &["status", "--short"][..],
    ] {
        let mut child = Command::new("git").current_dir(workspace).args(args)
            .stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
        let mut bytes = Vec::new();
        child.stdout.take().context("Git 未返回输出")?.take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
        let clipped = bytes.len() > LIMIT;
        if clipped { let _ = child.kill(); }
        let status = child.wait()?;
        if !clipped && !status.success() { bail!("无法读取当前工作树的差异"); }
        if !bytes.is_empty() {
            bytes.truncate(LIMIT);
            let mut diff = String::from_utf8_lossy(&bytes).into_owned();
            if clipped { diff.push_str("\n[diff truncated]"); }
            return Ok(diff);
        }
    }
    bail!("当前工作区没有可用于生成提交说明的变更")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgePullRequest {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub state: String,
    pub head_branch: String,
    #[serde(default)]
    pub head_sha: String,
    pub base_branch: String,
    pub is_draft: bool,
    pub checks_summary: String,
    pub url: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeIssue {
    pub id: String,
    pub number_label: String,
    pub source: String,
    pub title: String,
    pub state: String,
    pub author: String,
    pub labels: Vec<String>,
    pub url: String,
    pub body: String,
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. ACP Registry Catalog & External Agent Transcript Discovery
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalTranscriptCandidate {
    pub source_agent: String,
    pub title: String,
    pub path: PathBuf,
    pub modified_ms: u64,
    pub message_count: usize,
    pub excerpt: String,
}

pub fn scan_external_transcripts() -> Vec<ExternalTranscriptCandidate> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let roots = [
        ("claude", home.join(".claude/projects")),
        ("codex", home.join(".codex/sessions")),
        ("pi", home.join(".pi/agent/sessions")),
    ];
    for (agent, root) in roots {
        if !root.is_dir() {
            continue;
        }
        collect_jsonl_candidates(agent, &root, 0, &mut out);
    }
    out.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
    out.truncate(30);
    out
}

fn collect_jsonl_candidates(
    agent: &str,
    dir: &Path,
    depth: usize,
    out: &mut Vec<ExternalTranscriptCandidate>,
) {
    if depth > 3 || out.len() >= 60 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_jsonl_candidates(agent, &p, depth + 1, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("jsonl")
            && let Ok(meta) = entry.metadata()
            && meta.len() > 32
        {
            let modified_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            if let Ok(raw) = std::fs::read_to_string(&p) {
                let mut count = 0usize;
                let mut first_user = String::new();
                for line in raw.lines().take(80) {
                    if let Ok(val) = serde_json::from_str::<Value>(line) {
                        count += 1;
                        if first_user.is_empty() {
                            if let Some(txt) = val
                                .pointer("/message/content")
                                .and_then(|c| c.as_str())
                                .or_else(|| val.get("text").and_then(|c| c.as_str()))
                                .or_else(|| val.get("prompt").and_then(|c| c.as_str()))
                            {
                                let clean = txt.lines().next().unwrap_or("").trim();
                                if !clean.is_empty() {
                                    first_user = clean.chars().take(64).collect();
                                }
                            }
                        }
                    }
                }
                if count > 0 {
                    let fallback = p
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("session")
                        .to_owned();
                    out.push(ExternalTranscriptCandidate {
                        source_agent: agent.to_owned(),
                        title: if first_user.is_empty() {
                            fallback
                        } else {
                            first_user.clone()
                        },
                        path: p,
                        modified_ms,
                        message_count: count,
                        excerpt: first_user,
                    });
                }
            }
        }
    }
}

pub fn summarize_external_transcript_for_handoff(path: &Path) -> Result<(String, String)> {
    let raw = std::fs::read_to_string(path).context("读取外部历史会话文件失败")?;
    let mut title = String::new();
    let mut turns = Vec::new();
    for line in raw.lines().take(200) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let role = v
            .pointer("/message/role")
            .or_else(|| v.get("role"))
            .and_then(|s| s.as_str())
            .unwrap_or("entry");
        let text = v
            .pointer("/message/content")
            .and_then(|c| {
                c.as_str().map(str::to_owned).or_else(|| {
                    c.as_array().map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                })
            })
            .or_else(|| v.get("text").and_then(|s| s.as_str()).map(str::to_owned))
            .unwrap_or_default();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if title.is_empty() && role == "user" {
            title = trimmed.lines().next().unwrap_or(trimmed).chars().take(56).collect();
        }
        let clipped: String = trimmed.chars().take(600).collect();
        turns.push(format!("### {role}\n{clipped}"));
        if turns.len() >= 18 {
            break;
        }
    }
    if title.is_empty() {
        title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("导入的历史会话")
            .to_owned();
    }
    let prompt = format!(
        "以下是从本机外部 CLI 会话（`{}`）导入的历史上下文记录，请先阅读并简要确认你已掌握上下文，等待我的下一步指令：\n\n{}",
        path.display(),
        turns.join("\n\n")
    );
    Ok((title, prompt))
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Local Dev-Server Port Scanner, Storage Cleanup & macOS Helpers
// ═══════════════════════════════════════════════════════════════════════════

pub fn probe_local_dev_ports(extra_ports: &[u16]) -> Vec<u16> {
    let mut candidates: Vec<u16> = vec![
        3000, 3001, 4000, 4173, 4200, 5173, 5174, 8000, 8080, 8787, 8888,
    ];
    for &p in extra_ports {
        if p >= 1024 && !candidates.contains(&p) {
            candidates.push(p);
        }
    }
    let mut live = Vec::new();
    for port in candidates {
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
        if TcpStream::connect_timeout(&addr, Duration::from_millis(18)).is_ok() {
            live.push(port);
        }
    }
    live
}

pub fn detect_primary_lan_ipv4() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    let ip = sock.local_addr().ok()?.ip();
    (!ip.is_loopback()).then(|| ip.to_string())
}

#[derive(Clone, Debug, Default)]
pub struct StorageUsageSummary {
    pub store_bytes: u64,
    pub catalog_bytes: u64,
    pub expired_upload_bytes: u64,
    pub rotated_log_bytes: u64,
}

impl StorageUsageSummary {
    pub(crate) fn from_report(report: &Value) -> Result<Self> {
        Ok(Self {
            store_bytes: report["storage"]["bytes_after"].as_u64().context("缺少总存储用量")?,
            catalog_bytes: report["storage"]["catalog_bytes"].as_u64().context("缺少目录用量")?,
            expired_upload_bytes: report["expired_uploads"]["bytes"].as_u64().context("缺少上传缓存用量")?,
            rotated_log_bytes: report["rotated_logs"]["bytes"].as_u64().context("缺少日志用量")?,
        })
    }
}

pub fn send_macos_notification(title: &str, body: &str, play_sound: bool) {
    #[cfg(target_os = "macos")]
    {
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let sound_clause = if play_sound {
            " sound name \"Glass\""
        } else {
            ""
        };
        let script = format!(
            "display notification \"{}\" with title \"{}\"{sound_clause}",
            esc(body),
            esc(title)
        );
        std::thread::spawn(move || {
            let _ = Command::new("osascript")
                .args(["-e", &script])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (title, body, play_sound);
    }
}

/// Keeps macOS awake (`caffeinate -i -w <pid>`) while active agent turns are running.
pub struct CaffeinateGuard {
    child: Option<Child>,
}

impl Default for CaffeinateGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl CaffeinateGuard {
    pub fn new() -> Self {
        Self { child: None }
    }

    pub fn sync(&mut self, should_stay_awake: bool) {
        #[cfg(target_os = "macos")]
        {
            if should_stay_awake {
                let alive = self
                    .child
                    .as_mut()
                    .is_some_and(|c| matches!(c.try_wait(), Ok(None)));
                if !alive {
                    let pid = std::process::id().to_string();
                    self.child = Command::new("caffeinate")
                        .args(["-i", "-w", &pid])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .ok();
                }
            } else if let Some(mut c) = self.child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = should_stay_awake;
        }
    }
}

impl Drop for CaffeinateGuard {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Desktop updates from the Kar workbench host
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, Default)]
pub struct DesktopAppUpdateReport {
    pub current_version: String,
    pub channel: String,
    pub available_version: Option<String>,
    pub notes_url: Option<String>,
    pub can_auto_install: bool,
    pub request_id: Option<uuid::Uuid>,
    pub status_message: String,
}

pub(crate) async fn check_desktop_app_update(
    engine: Option<crate::engine::Engine>,
    channel: String,
    install: Option<uuid::Uuid>,
) -> DesktopAppUpdateReport {
    use axum::http::Method;
    let mut report = DesktopAppUpdateReport {
        channel: channel.clone(),
        current_version: env!("CARGO_PKG_VERSION").into(),
        ..Default::default()
    };
    let result: Result<()> = async {
        let engine = engine.context("工作台引擎未连接")?;
        let base = "/server/desktop-update";
        let status = engine.request(Method::GET, base, None).await?;
        if status["available"] != true {
            report.status_message = "此构建使用手动更新，可前往 Kwork 发布页查看版本。".into();
            report.notes_url = Some("https://github.com/kense-labs/kwork/releases".into());
            return Ok(());
        }
        let id = install.unwrap_or_else(uuid::Uuid::new_v4);
        let path = format!("{base}/{id}");
        let mut job = match install {
            Some(_) => engine.request(Method::POST, &format!("{path}/commit"), None).await?,
            None => engine.request(Method::POST, base, Some(serde_json::json!({"request_id":id,"track":channel}))).await?,
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            let stage = job["stage"].as_str().context("更新回执缺少状态")?;
            let label = match stage {
                "ready" => "更新已就绪，可安装并重新启动",
                "current" => "当前已是最新版本",
                "installed" => "更新安装完成",
                "failed" => "更新失败",
                "unknown" => "更新结果待确认",
                _ if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(350)).await;
                    job = engine.request(Method::GET, &path, None).await?;
                    continue;
                }
                _ => "更新仍在进行中",
            };
            report.status_message = job["message"].as_str().filter(|m| !m.is_empty()).unwrap_or(label).to_owned();
            if stage == "ready" {
                report.request_id = Some(id);
                report.available_version = job["version"].as_str().map(str::to_owned);
                report.can_auto_install = true;
            }
            return Ok(());
        }
    }.await;
    if let Err(error) = result { report.status_message = format!("检查更新失败：{error}"); }
    report
}
